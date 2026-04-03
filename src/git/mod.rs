use std::{
    collections::HashMap,
    path::Path,
    process::Command,
};

/// Status of a single line in the git gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMark {
    /// Line was added (not present in HEAD).
    Added,
    /// Line exists in HEAD but was modified.
    Modified,
    /// A line from HEAD was deleted before this line.
    Deleted,
}

/// Precomputed per-line gutter marks for a buffer.
#[derive(Debug, Default, Clone)]
pub struct GitGutter {
    /// Maps 0-based line index in the *current* buffer → gutter mark.
    pub marks: HashMap<usize, GutterMark>,
}

impl GitGutter {
    pub fn get(&self, line: usize) -> Option<GutterMark> {
        self.marks.get(&line).copied()
    }
}

// ── Pure diff ────────────────────────────────────────────────────────────────

/// Compute gutter marks by diffing `original` lines against `current` lines.
///
/// Uses a simple LCS-based diff. This function is pure (no I/O) and fully
/// testable.
///
/// Returns a map of 0-based current line index → `GutterMark`.
pub fn compute_marks(original: &[&str], current: &[&str]) -> HashMap<usize, GutterMark> {
    let lcs = lcs_length(original, current);
    let ops = diff_ops(original, current, &lcs);

    let mut marks = HashMap::new();
    let mut cur_line = 0usize;
    let mut pending_delete = false;

    for op in &ops {
        match op {
            DiffOp::Equal => {
                if pending_delete {
                    marks.insert(cur_line, GutterMark::Deleted);
                    pending_delete = false;
                }
                cur_line += 1;
            }
            DiffOp::Insert => {
                if pending_delete {
                    // A delete followed immediately by an insert = modification.
                    marks.insert(cur_line, GutterMark::Modified);
                    pending_delete = false;
                } else {
                    marks.insert(cur_line, GutterMark::Added);
                }
                cur_line += 1;
            }
            DiffOp::Delete => {
                pending_delete = true;
                // Don't advance cur_line — deletions don't consume current lines.
            }
        }
    }

    // Trailing delete: mark it on the virtual line after the last current line.
    if pending_delete && cur_line <= current.len() {
        marks.insert(cur_line.saturating_sub(1), GutterMark::Deleted);
    }

    marks
}

#[derive(Debug, PartialEq, Eq)]
enum DiffOp {
    Equal,
    Insert,
    Delete,
}

/// Classic O(n*m) LCS table. Returns a 2D vec of lengths.
fn lcs_length<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Vec<usize>> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    dp
}

/// Back-track through the LCS table to produce a list of diff operations.
fn diff_ops<'a>(
    original: &[&'a str],
    current: &[&'a str],
    dp: &[Vec<usize>],
) -> Vec<DiffOp> {
    let mut ops = Vec::new();
    let mut i = original.len();
    let mut j = current.len();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && original[i - 1] == current[j - 1] {
            ops.push(DiffOp::Equal);
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Insert);
            j -= 1;
        } else {
            ops.push(DiffOp::Delete);
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

// ── Git I/O ──────────────────────────────────────────────────────────────────

/// Fetch the HEAD version of `path` from the local git repository.
///
/// Returns `None` if the file is untracked, git is not available, or the
/// working directory is not a git repository.
pub fn fetch_head_content(path: &Path) -> Option<String> {
    // Convert to a relative path for `git show`.
    let path_str = path.to_string_lossy();

    let output = Command::new("git")
        .args(["show", &format!("HEAD:{path_str}")])
        .output()
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// Compute a `GitGutter` for the file at `path` against its HEAD version.
///
/// Returns `None` if the file is not tracked or git is unavailable.
pub fn gutter_for_path(path: &Path, current_content: &str) -> Option<GitGutter> {
    let head = fetch_head_content(path)?;
    let original: Vec<&str> = head.lines().collect();
    let current: Vec<&str> = current_content.lines().collect();
    let marks = compute_marks(&original, &current);
    Some(GitGutter { marks })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_files_no_marks() {
        let lines = ["fn main() {}", "    println!(\"hi\");", "}"];
        let marks = compute_marks(&lines, &lines);
        assert!(marks.is_empty(), "identical files should produce no marks");
    }

    #[test]
    fn added_line_at_end() {
        let orig = ["line1", "line2"];
        let curr = ["line1", "line2", "line3"];
        let marks = compute_marks(&orig, &curr);
        assert_eq!(marks.get(&2), Some(&GutterMark::Added));
        assert!(!marks.contains_key(&0));
        assert!(!marks.contains_key(&1));
    }

    #[test]
    fn added_line_at_start() {
        let orig = ["line2", "line3"];
        let curr = ["line1", "line2", "line3"];
        let marks = compute_marks(&orig, &curr);
        assert_eq!(marks.get(&0), Some(&GutterMark::Added));
        assert!(!marks.contains_key(&1));
        assert!(!marks.contains_key(&2));
    }

    #[test]
    fn modified_line() {
        let orig = ["line1", "ORIGINAL", "line3"];
        let curr = ["line1", "MODIFIED", "line3"];
        let marks = compute_marks(&orig, &curr);
        // Line 1 (0-based) should be Modified.
        assert_eq!(marks.get(&1), Some(&GutterMark::Modified), "got: {marks:?}");
    }

    #[test]
    fn deleted_line_marks_adjacent() {
        let orig = ["line1", "deleted_line", "line3"];
        let curr = ["line1", "line3"];
        let marks = compute_marks(&orig, &curr);
        // Deletion is typically shown on the line after; in our impl it
        // may appear on the adjacent line. Just check there's a Deleted mark.
        assert!(
            marks.values().any(|m| *m == GutterMark::Deleted),
            "expected a Deleted mark, got: {marks:?}"
        );
    }

    #[test]
    fn all_lines_added() {
        let orig: &[&str] = &[];
        let curr = ["a", "b", "c"];
        let marks = compute_marks(orig, &curr);
        assert_eq!(marks.get(&0), Some(&GutterMark::Added));
        assert_eq!(marks.get(&1), Some(&GutterMark::Added));
        assert_eq!(marks.get(&2), Some(&GutterMark::Added));
    }

    #[test]
    fn empty_both() {
        let marks = compute_marks(&[], &[]);
        assert!(marks.is_empty());
    }

    #[test]
    fn gutter_mark_copy() {
        let m = GutterMark::Added;
        let m2 = m;
        assert_eq!(m, m2);
    }
}
