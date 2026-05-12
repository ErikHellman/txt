//! Minimal `.editorconfig` parser and resolver.
//!
//! Walks parent directories from a file path looking for `.editorconfig`
//! files and merges the matching sections into a single
//! [`EditorConfigOverrides`] value. Only the editor-relevant keys are read
//! (`indent_style`, `indent_size`, `tab_width`, `end_of_line`,
//! `insert_final_newline`, `trim_trailing_whitespace`); other keys are
//! ignored. Pattern matching covers the dialects seen in the wild —
//! `*`, `*.ext`, `*.{ext1,ext2}`, exact filenames — and falls through to a
//! literal compare for anything fancier.
//!
//! Used by [`crate::editor::tab::BufferHandle`] when opening a file: the
//! resolved overrides override the equivalent fields from the global config.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::formatting::IndentStyle;

/// EOL style read from `.editorconfig`. Plain enum for serialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EolStyle {
    Lf,
    Crlf,
    Cr,
}

/// Per-buffer overrides resolved from `.editorconfig`. Every field is
/// optional — `None` means "fall through to the global config".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorConfigOverrides {
    pub indent_style: Option<IndentStyle>,
    pub indent_size: Option<usize>,
    pub tab_width: Option<usize>,
    pub end_of_line: Option<EolStyle>,
    pub insert_final_newline: Option<bool>,
    pub trim_trailing_whitespace: Option<bool>,
}

impl EditorConfigOverrides {
    /// Effective indent width: `indent_size` if set, otherwise `tab_width`,
    /// otherwise `None`.
    pub fn effective_width(&self) -> Option<usize> {
        self.indent_size.or(self.tab_width)
    }

    /// Merge `other` on top of `self` (later wins). Used when walking from
    /// outer `.editorconfig` to inner.
    fn merge_in(&mut self, other: &Self) {
        if other.indent_style.is_some() {
            self.indent_style = other.indent_style;
        }
        if other.indent_size.is_some() {
            self.indent_size = other.indent_size;
        }
        if other.tab_width.is_some() {
            self.tab_width = other.tab_width;
        }
        if other.end_of_line.is_some() {
            self.end_of_line = other.end_of_line;
        }
        if other.insert_final_newline.is_some() {
            self.insert_final_newline = other.insert_final_newline;
        }
        if other.trim_trailing_whitespace.is_some() {
            self.trim_trailing_whitespace = other.trim_trailing_whitespace;
        }
    }
}

/// One `[pattern]` section parsed out of an `.editorconfig`.
struct Section {
    pattern: String,
    overrides: EditorConfigOverrides,
}

/// One `.editorconfig` file parsed: the `root = true` flag and the sections.
struct ParsedFile {
    root: bool,
    sections: Vec<Section>,
}

/// Resolve `.editorconfig` overrides for `path`.
///
/// Walks from `path`'s parent directory up to the filesystem root, parsing
/// every `.editorconfig` it finds. Stops walking when a file with
/// `root = true` is encountered (per the EditorConfig spec). Returns
/// [`EditorConfigOverrides`] with the merged settings; outer files contribute
/// first, inner files override.
pub fn load_for_file(path: &Path) -> EditorConfigOverrides {
    let mut files: Vec<(PathBuf, ParsedFile)> = Vec::new();
    let mut dir = path.parent().map(PathBuf::from);
    while let Some(d) = dir {
        let candidate = d.join(".editorconfig");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let parsed = parse_text(&text);
            let stop = parsed.root;
            files.push((d.clone(), parsed));
            if stop {
                break;
            }
        }
        dir = d.parent().map(PathBuf::from);
    }
    // Apply outer (last in walk) to inner (first in walk).
    let mut merged = EditorConfigOverrides::default();
    for (config_dir, parsed) in files.iter().rev() {
        for section in &parsed.sections {
            if pattern_matches(&section.pattern, path, config_dir) {
                merged.merge_in(&section.overrides);
            }
        }
    }
    merged
}

/// Parse the textual contents of one `.editorconfig` file.
fn parse_text(text: &str) -> ParsedFile {
    let mut root = false;
    let mut sections: Vec<Section> = Vec::new();
    let mut current_pattern: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[')
            && let Some(close) = rest.find(']')
        {
            let pattern = rest[..close].trim().to_string();
            current_pattern = Some(pattern.clone());
            sections.push(Section {
                pattern,
                overrides: EditorConfigOverrides::default(),
            });
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().to_string()),
            None => continue,
        };
        let value_lc = value.to_ascii_lowercase();

        if current_pattern.is_none() {
            // Pre-section keys: only `root` is meaningful.
            if key == "root" && parse_bool(&value_lc) == Some(true) {
                root = true;
            }
            continue;
        }
        let section = sections
            .last_mut()
            .expect("current_pattern implies section");
        match key.as_str() {
            "indent_style" => match value_lc.as_str() {
                "tab" => section.overrides.indent_style = Some(IndentStyle::Tabs),
                "space" => section.overrides.indent_style = Some(IndentStyle::Spaces),
                _ => {}
            },
            "indent_size" => {
                if let Ok(n) = value_lc.parse::<usize>() {
                    section.overrides.indent_size = Some(n);
                }
                // "tab" means "follow tab_width" — leave indent_size unset.
            }
            "tab_width" => {
                if let Ok(n) = value_lc.parse::<usize>() {
                    section.overrides.tab_width = Some(n);
                }
            }
            "end_of_line" => {
                section.overrides.end_of_line = match value_lc.as_str() {
                    "lf" => Some(EolStyle::Lf),
                    "crlf" => Some(EolStyle::Crlf),
                    "cr" => Some(EolStyle::Cr),
                    _ => None,
                };
            }
            "insert_final_newline" => {
                section.overrides.insert_final_newline = parse_bool(&value_lc);
            }
            "trim_trailing_whitespace" => {
                section.overrides.trim_trailing_whitespace = parse_bool(&value_lc);
            }
            _ => {}
        }
    }
    ParsedFile { root, sections }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Match a glob-ish `pattern` from a section header against `path`, where
/// `config_dir` is the directory containing the `.editorconfig` file.
///
/// Supports the patterns seen in nearly all real-world `.editorconfig`
/// files: `*`, `*.ext`, `*.{a,b,c}`, exact filenames, and `**/...` prefixes.
/// Falls back to a literal filename compare for anything else.
fn pattern_matches(pattern: &str, path: &Path, config_dir: &Path) -> bool {
    // Strip a leading `/` (which in EditorConfig means "rooted at config_dir").
    let pattern = pattern.trim_start_matches('/');

    // Compute the file's path relative to the config directory, falling back
    // to the file name only if the rebase fails.
    let rel = path.strip_prefix(config_dir).ok();
    let rel_str = rel.map(|r| r.to_string_lossy().replace('\\', "/"));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Strip a leading `**/` — match any number of intermediate directories.
    let pattern = pattern.trim_start_matches("**/");

    // Pattern with brace expansion: `*.{a,b}` → try each branch.
    if let Some(open) = pattern.find('{')
        && let Some(close) = pattern[open..].find('}')
    {
        let prefix = &pattern[..open];
        let suffix = &pattern[open + close + 1..];
        let alts = pattern[open + 1..open + close].split(',');
        for alt in alts {
            let expanded = format!("{prefix}{}{suffix}", alt.trim());
            if pattern_matches_simple(&expanded, &file_name, rel_str.as_deref()) {
                return true;
            }
        }
        return false;
    }

    pattern_matches_simple(pattern, &file_name, rel_str.as_deref())
}

/// Match a brace-free pattern. Handles `*`, `*.ext`, `name.*`, exact name.
fn pattern_matches_simple(pattern: &str, file_name: &str, rel: Option<&str>) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(ext_pat) = pattern.strip_prefix("*.") {
        // `*.ext` → file ends with `.ext` (no directory wildcard handling).
        return file_name
            .rsplit_once('.')
            .map(|(_, ext)| ext == ext_pat)
            .unwrap_or(false);
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return file_name
            .rsplit_once('.')
            .map(|(stem, _)| stem == prefix)
            .unwrap_or(false);
    }
    // Pattern with a directory separator is matched against the relative path
    // first, falling back to a no-match.
    if pattern.contains('/') {
        return rel.map(|r| r == pattern).unwrap_or(false);
    }
    pattern == file_name
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_root_flag() {
        let p = parse_text("root = true\n[*]\nindent_style = space\n");
        assert!(p.root);
        assert_eq!(p.sections.len(), 1);
        assert_eq!(p.sections[0].pattern, "*");
        assert_eq!(
            p.sections[0].overrides.indent_style,
            Some(IndentStyle::Spaces)
        );
    }

    #[test]
    fn parse_indent_size_and_eol() {
        let text = r#"
[*.rs]
indent_style = space
indent_size = 2
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
"#;
        let p = parse_text(text);
        let s = &p.sections[0];
        assert_eq!(s.overrides.indent_style, Some(IndentStyle::Spaces));
        assert_eq!(s.overrides.indent_size, Some(2));
        assert_eq!(s.overrides.end_of_line, Some(EolStyle::Lf));
        assert_eq!(s.overrides.insert_final_newline, Some(true));
        assert_eq!(s.overrides.trim_trailing_whitespace, Some(true));
    }

    #[test]
    fn parse_indent_size_tab_falls_back_to_tab_width() {
        let p = parse_text("[*]\nindent_style = tab\nindent_size = tab\ntab_width = 8\n");
        assert_eq!(
            p.sections[0].overrides.indent_style,
            Some(IndentStyle::Tabs)
        );
        assert_eq!(p.sections[0].overrides.indent_size, None);
        assert_eq!(p.sections[0].overrides.tab_width, Some(8));
        assert_eq!(p.sections[0].overrides.effective_width(), Some(8));
    }

    #[test]
    fn parse_ignores_comments_and_blank_lines() {
        let text = "# header\n\n; semicolon comment\n[*]\nindent_size = 4\n# tail\n";
        let p = parse_text(text);
        assert_eq!(p.sections.len(), 1);
        assert_eq!(p.sections[0].overrides.indent_size, Some(4));
    }

    #[test]
    fn pattern_star_matches_any_filename() {
        let dir = Path::new("/tmp");
        assert!(pattern_matches("*", Path::new("/tmp/foo.rs"), dir));
        assert!(pattern_matches("*", Path::new("/tmp/bar"), dir));
    }

    #[test]
    fn pattern_extension() {
        let dir = Path::new("/tmp");
        assert!(pattern_matches("*.rs", Path::new("/tmp/main.rs"), dir));
        assert!(!pattern_matches("*.rs", Path::new("/tmp/main.py"), dir));
    }

    #[test]
    fn pattern_brace_alternatives() {
        let dir = Path::new("/tmp");
        assert!(pattern_matches("*.{js,py}", Path::new("/tmp/x.js"), dir));
        assert!(pattern_matches("*.{js,py}", Path::new("/tmp/x.py"), dir));
        assert!(!pattern_matches("*.{js,py}", Path::new("/tmp/x.rs"), dir));
    }

    #[test]
    fn pattern_double_star_prefix_treated_as_recursive() {
        let dir = Path::new("/tmp");
        assert!(pattern_matches(
            "**/*.rs",
            Path::new("/tmp/sub/main.rs"),
            dir
        ));
    }

    #[test]
    fn pattern_exact_filename() {
        let dir = Path::new("/tmp");
        assert!(pattern_matches("Makefile", Path::new("/tmp/Makefile"), dir));
        assert!(!pattern_matches("Makefile", Path::new("/tmp/Other"), dir));
    }

    #[test]
    fn merge_in_later_wins() {
        let mut a = EditorConfigOverrides {
            indent_style: Some(IndentStyle::Tabs),
            indent_size: Some(4),
            ..Default::default()
        };
        let b = EditorConfigOverrides {
            indent_size: Some(2),
            insert_final_newline: Some(true),
            ..Default::default()
        };
        a.merge_in(&b);
        assert_eq!(a.indent_style, Some(IndentStyle::Tabs));
        assert_eq!(a.indent_size, Some(2));
        assert_eq!(a.insert_final_newline, Some(true));
    }

    #[test]
    fn load_for_file_reads_temp_editorconfig() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join(".editorconfig");
        let mut f = std::fs::File::create(&cfg_path).unwrap();
        writeln!(
            f,
            "root = true\n[*.rs]\nindent_style = space\nindent_size = 2"
        )
        .unwrap();
        let target = dir.path().join("main.rs");
        std::fs::write(&target, "").unwrap();
        let o = load_for_file(&target);
        assert_eq!(o.indent_style, Some(IndentStyle::Spaces));
        assert_eq!(o.indent_size, Some(2));
    }

    #[test]
    fn load_for_file_root_stops_upward_walk() {
        use std::io::Write;
        let outer = tempfile::tempdir().unwrap();
        let inner = outer.path().join("project");
        std::fs::create_dir(&inner).unwrap();
        // Outer config: should be IGNORED because inner has root = true.
        let mut o_cfg = std::fs::File::create(outer.path().join(".editorconfig")).unwrap();
        writeln!(o_cfg, "[*]\nindent_size = 8").unwrap();
        let mut i_cfg = std::fs::File::create(inner.join(".editorconfig")).unwrap();
        writeln!(i_cfg, "root = true\n[*]\nindent_size = 2").unwrap();
        let target = inner.join("foo.rs");
        std::fs::write(&target, "").unwrap();
        let o = load_for_file(&target);
        assert_eq!(o.indent_size, Some(2));
    }

    #[test]
    fn load_for_file_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("foo.rs");
        std::fs::write(&target, "").unwrap();
        let o = load_for_file(&target);
        assert_eq!(o, EditorConfigOverrides::default());
    }
}
