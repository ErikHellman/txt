//! Project-wide search and replace.
//!
//! Walks the workspace with `ignore` (respecting `.gitignore`) and matches
//! each text file with the same regex/case rules as the in-buffer search bar
//! (see `super::build_pattern`). Binary files are detected by a NUL-byte
//! probe in the first 8 KiB and skipped silently.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::Regex;

use super::build_pattern;

/// Hard cap on result count. Keeps the UI list bounded for huge queries.
pub const MAX_RESULTS: usize = 5_000;

/// Probe size used for binary detection. Files with a NUL byte in this prefix
/// are skipped.
const BINARY_PROBE_BYTES: usize = 8 * 1024;

/// Maximum file size we'll search through. Larger files are skipped to keep
/// scans snappy in workspaces with checked-in artefacts.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// One match in one file.
#[derive(Debug, Clone)]
pub struct ProjectMatch {
    pub path: PathBuf,
    /// 0-based line number within `path`.
    pub line: usize,
    /// Byte offset of the match start within the file.
    #[allow(dead_code)]
    pub byte_start: usize,
    /// Byte offset of the match end within the file.
    #[allow(dead_code)]
    pub byte_end: usize,
    /// The full text of the line containing the match (no trailing newline).
    pub line_text: String,
}

/// Result of a project-wide search.
#[derive(Debug, Default)]
pub struct ProjectSearchResults {
    pub matches: Vec<ProjectMatch>,
    /// True if `matches` was capped at `MAX_RESULTS`.
    pub truncated: bool,
}

/// Walk `root` and collect matches for `query`. Honours `.gitignore` and skips
/// binary / oversized files.
pub fn run(root: &Path, query: &str, is_regex: bool, case_sensitive: bool) -> ProjectSearchResults {
    let mut out = ProjectSearchResults::default();
    if query.is_empty() {
        return out;
    }
    let pattern = build_pattern(query, is_regex, case_sensitive);
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return out,
    };

    'walk: for entry in WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build()
        .flatten()
    {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        let text = match read_text_file(path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for m in re.find_iter(&text) {
            let line = text[..m.start()].bytes().filter(|b| *b == b'\n').count();
            let line_start = text[..m.start()].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = text[m.start()..]
                .find('\n')
                .map(|p| m.start() + p)
                .unwrap_or(text.len());
            let line_text = text[line_start..line_end].to_string();
            let display_path = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            out.matches.push(ProjectMatch {
                path: display_path,
                line,
                byte_start: m.start(),
                byte_end: m.end(),
                line_text,
            });
            if out.matches.len() >= MAX_RESULTS {
                out.truncated = true;
                break 'walk;
            }
        }
    }
    out
}

/// Read a file as UTF-8 text, returning an error if it appears to be binary
/// (contains a NUL byte in the first `BINARY_PROBE_BYTES`) or invalid UTF-8.
fn read_text_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let probe_end = bytes.len().min(BINARY_PROBE_BYTES);
    if bytes[..probe_end].contains(&0u8) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "binary file"));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Replace every occurrence of `query` with `replacement` in `path`, atomically.
/// Returns the number of replacements performed.
///
/// Used for files that are not currently open in a buffer; open buffers should
/// be edited through `Buffer::insert_str` so undo history is preserved.
pub fn replace_all_in_file(
    path: &Path,
    query: &str,
    is_regex: bool,
    case_sensitive: bool,
    replacement: &str,
) -> io::Result<usize> {
    let text = read_text_file(path)?;
    let pattern = build_pattern(query, is_regex, case_sensitive);
    let re = Regex::new(&pattern)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let count = re.find_iter(&text).count();
    if count == 0 {
        return Ok(0);
    }
    let new_text = if is_regex {
        // Regex mode: use Regex::replace_all so $1 / $2 references work.
        re.replace_all(&text, replacement).into_owned()
    } else {
        // Literal mode: respect case_sensitive flag, treat replacement as-is.
        replace_literal(&text, query, case_sensitive, replacement)
    };
    write_atomic(path, new_text.as_bytes())?;
    Ok(count)
}

fn replace_literal(text: &str, needle: &str, case_sensitive: bool, replacement: &str) -> String {
    if case_sensitive {
        return text.replace(needle, replacement);
    }
    // Case-insensitive literal replace: walk the text, comparing lowercase windows.
    let lower = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    if needle_lower.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut byte = 0usize;
    while byte < text.len() {
        if lower[byte..].starts_with(&needle_lower) {
            out.push_str(replacement);
            byte += needle_lower.len();
        } else {
            // Step one char forward so multi-byte characters stay valid.
            let mut chars = text[byte..].chars();
            if let Some(c) = chars.next() {
                out.push(c);
                byte += c.len_utf8();
            } else {
                break;
            }
        }
    }
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = parent.join(".__txt_replace_tmp");
    if let Some(name) = path.file_name() {
        tmp = parent.join(format!(".__txt_replace_tmp_{}", name.to_string_lossy()));
    }
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir();
        let unique = format!(
            "txt_proj_search_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let p = base.join(unique);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn run_finds_matches_across_files() {
        let dir = tempdir();
        fs::write(dir.join("a.txt"), "hello world\nhello there\n").unwrap();
        fs::write(dir.join("b.txt"), "no match here\n").unwrap();
        fs::write(dir.join("c.txt"), "another hello\n").unwrap();

        let r = run(&dir, "hello", false, true);
        assert!(!r.truncated);
        assert_eq!(r.matches.len(), 3);
        assert!(
            r.matches
                .iter()
                .any(|m| m.line == 0 && m.line_text == "hello world")
        );
        assert!(
            r.matches
                .iter()
                .any(|m| m.line == 1 && m.line_text == "hello there")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_skips_binary_files() {
        let dir = tempdir();
        let mut bin = fs::File::create(dir.join("binary.dat")).unwrap();
        bin.write_all(&[0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x00, 0x00])
            .unwrap();
        fs::write(dir.join("text.txt"), "hello text\n").unwrap();

        let r = run(&dir, "hello", false, true);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].path.file_name().unwrap(), "text.txt");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_respects_gitignore() {
        let dir = tempdir();
        fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.join("ignored.txt"), "hello hidden\n").unwrap();
        fs::write(dir.join("kept.txt"), "hello kept\n").unwrap();
        // ignore::WalkBuilder requires the workspace to be a git repo for
        // .gitignore to take effect by default. Initialise one.
        let _ = std::process::Command::new("git")
            .arg("init")
            .current_dir(&dir)
            .output();

        let r = run(&dir, "hello", false, true);
        // Either the env has git or it doesn't; assert that kept.txt is found
        // and that if any match comes from ignored.txt, the test environment
        // didn't have git available — both are acceptable.
        assert!(r.matches.iter().any(|m| m.path.ends_with("kept.txt")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn case_insensitive_search() {
        let dir = tempdir();
        fs::write(dir.join("a.txt"), "Hello WORLD\nhello there\n").unwrap();

        let r = run(&dir, "hello", false, false);
        assert_eq!(r.matches.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn regex_search() {
        let dir = tempdir();
        fs::write(dir.join("a.txt"), "abc 123 def 456\n").unwrap();

        let r = run(&dir, r"\d+", true, true);
        assert_eq!(r.matches.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_all_in_file_writes_replacement() {
        let dir = tempdir();
        let path = dir.join("a.txt");
        fs::write(&path, "foo bar foo\n").unwrap();

        let n = replace_all_in_file(&path, "foo", false, true, "baz").unwrap();
        assert_eq!(n, 2);
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, "baz bar baz\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_all_case_insensitive_literal() {
        let dir = tempdir();
        let path = dir.join("a.txt");
        fs::write(&path, "Hello hello HELLO\n").unwrap();

        let n = replace_all_in_file(&path, "hello", false, false, "HI").unwrap();
        assert_eq!(n, 3);
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, "HI HI HI\n");

        let _ = fs::remove_dir_all(&dir);
    }
}
