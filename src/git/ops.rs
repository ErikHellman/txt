//! Subprocess wrappers for the common git operations exposed by the
//! [`crate::ui::git_dialog`] overlay.
//!
//! All commands shell out to `git` via [`std::process::Command`], matching the
//! pattern used by [`crate::git::fetch_head_content`] in `mod.rs`. Each public
//! function returns `Result<T, String>`: `Ok` carries successfully parsed
//! output, `Err` carries a human-readable error suitable for showing in the
//! dialog's error banner.

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Status entries ───────────────────────────────────────────────────────────

/// One entry from `git status --porcelain=v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Status of the file in the index (staged area). Space if unchanged.
    pub index: char,
    /// Status of the file in the working tree (unstaged). Space if unchanged.
    pub worktree: char,
    /// File path relative to the repository root.
    pub path: PathBuf,
}

impl StatusEntry {
    pub fn is_staged(&self) -> bool {
        self.index != ' ' && self.index != '?'
    }

    #[allow(dead_code)]
    pub fn is_unstaged(&self) -> bool {
        self.worktree != ' '
    }

    #[allow(dead_code)]
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }
}

/// One entry from `git branch --list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEntry {
    pub name: String,
    pub current: bool,
}

/// One entry from `git stash list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    /// 0-based index, matches `stash@{N}`.
    pub index: usize,
    /// Display message (the part after `stash@{N}: `).
    pub message: String,
}

// ── Pure parsers (testable, no I/O) ──────────────────────────────────────────

pub fn parse_porcelain(out: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        // Format: "XY <path>" — exactly two status chars, then a space.
        let mut chars = line.chars();
        let x = match chars.next() {
            Some(c) => c,
            None => continue,
        };
        let y = match chars.next() {
            Some(c) => c,
            None => continue,
        };
        // The third char must be a space.
        if chars.next() != Some(' ') {
            continue;
        }
        let rest: String = chars.collect();
        // Renames are reported as "old -> new"; show the new name only.
        let path_str = rest.split(" -> ").last().unwrap_or(&rest);
        entries.push(StatusEntry {
            index: x,
            worktree: y,
            path: PathBuf::from(path_str),
        });
    }
    entries
}

pub fn parse_branches(out: &str) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        // git branch --list uses "* current\n  other\n  + worktree\n".
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (marker, name) = line.split_at(2.min(line.len()));
        let current = marker.starts_with('*');
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // Skip detached-HEAD lines like "(HEAD detached at 1234abc)".
        if name.starts_with('(') {
            continue;
        }
        entries.push(BranchEntry {
            name: name.to_string(),
            current,
        });
    }
    entries
}

pub fn parse_stash_list(out: &str) -> Vec<StashEntry> {
    let mut entries = Vec::new();
    for (idx, line) in out.lines().enumerate() {
        // Format: "stash@{0}: WIP on main: 1234abc message"
        let message = match line.split_once(": ") {
            Some((_prefix, rest)) => rest.to_string(),
            None => line.to_string(),
        };
        entries.push(StashEntry {
            index: idx,
            message,
        });
    }
    entries
}

// ── Subprocess helpers ───────────────────────────────────────────────────────

fn run(workspace: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            Err(format!("git exited with status {}", output.status))
        } else {
            Err(stderr.to_string())
        }
    }
}

/// Returns `true` if `workspace` is inside a git working tree.
pub fn is_repo(workspace: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(workspace)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Status / staging ─────────────────────────────────────────────────────────

pub fn status(workspace: &Path) -> Result<Vec<StatusEntry>, String> {
    let out = run(workspace, &["status", "--porcelain=v1"])?;
    Ok(parse_porcelain(&out))
}

pub fn status_summary(workspace: &Path) -> Result<String, String> {
    run(workspace, &["status"])
}

pub fn add(workspace: &Path, paths: &[&Path]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["add", "--"];
    let path_strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into()).collect();
    for p in &path_strs {
        args.push(p);
    }
    run(workspace, &args).map(|_| ())
}

pub fn reset(workspace: &Path, paths: &[&Path]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["reset", "HEAD", "--"];
    let path_strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into()).collect();
    for p in &path_strs {
        args.push(p);
    }
    run(workspace, &args).map(|_| ())
}

// ── Commit / push / pull ─────────────────────────────────────────────────────

pub fn commit(workspace: &Path, message: &str) -> Result<String, String> {
    run(workspace, &["commit", "-m", message])
}

pub fn push(workspace: &Path) -> Result<String, String> {
    run(workspace, &["push"])
}

pub fn pull(workspace: &Path) -> Result<String, String> {
    run(workspace, &["pull"])
}

// ── Branches ─────────────────────────────────────────────────────────────────

pub fn branches(workspace: &Path) -> Result<Vec<BranchEntry>, String> {
    let out = run(workspace, &["branch", "--list"])?;
    Ok(parse_branches(&out))
}

pub fn checkout(workspace: &Path, branch: &str) -> Result<String, String> {
    run(workspace, &["checkout", branch])
}

pub fn create_branch(workspace: &Path, name: &str) -> Result<String, String> {
    run(workspace, &["checkout", "-b", name])
}

pub fn delete_branch(workspace: &Path, name: &str) -> Result<String, String> {
    run(workspace, &["branch", "-d", name])
}

// ── Stashes ──────────────────────────────────────────────────────────────────

pub fn stashes(workspace: &Path) -> Result<Vec<StashEntry>, String> {
    let out = run(workspace, &["stash", "list"])?;
    Ok(parse_stash_list(&out))
}

pub fn stash_push(workspace: &Path, message: Option<&str>) -> Result<String, String> {
    match message {
        Some(m) if !m.is_empty() => run(workspace, &["stash", "push", "-m", m]),
        _ => run(workspace, &["stash", "push"]),
    }
}

pub fn stash_apply(workspace: &Path, idx: usize) -> Result<String, String> {
    let r = format!("stash@{{{}}}", idx);
    run(workspace, &["stash", "apply", &r])
}

pub fn stash_pop(workspace: &Path, idx: usize) -> Result<String, String> {
    let r = format!("stash@{{{}}}", idx);
    run(workspace, &["stash", "pop", &r])
}

pub fn stash_drop(workspace: &Path, idx: usize) -> Result<String, String> {
    let r = format!("stash@{{{}}}", idx);
    run(workspace, &["stash", "drop", &r])
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_porcelain_modified_and_added() {
        let out = " M src/main.rs\nA  src/lib.rs\n?? foo.txt\n";
        let entries = parse_porcelain(out);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].index, ' ');
        assert_eq!(entries[0].worktree, 'M');
        assert_eq!(entries[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(entries[1].index, 'A');
        assert_eq!(entries[1].worktree, ' ');
        assert!(entries[2].is_untracked());
    }

    #[test]
    fn parse_porcelain_rename_keeps_new_name() {
        let out = "R  old.txt -> new.txt\n";
        let entries = parse_porcelain(out);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("new.txt"));
    }

    #[test]
    fn parse_porcelain_skips_blank_and_short_lines() {
        let out = "\n M\nXY foo\n";
        let entries = parse_porcelain(out);
        // " M\n" has only 2 chars (no path), should be skipped.
        // "XY foo" is valid.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("foo"));
    }

    #[test]
    fn parse_branches_marks_current() {
        let out = "  feature/a\n* main\n  feature/b\n";
        let entries = parse_branches(out);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "feature/a");
        assert!(!entries[0].current);
        assert_eq!(entries[1].name, "main");
        assert!(entries[1].current);
        assert!(!entries[2].current);
    }

    #[test]
    fn parse_branches_skips_detached_head() {
        let out = "* (HEAD detached at 1234abc)\n  main\n";
        let entries = parse_branches(out);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "main");
    }

    #[test]
    fn parse_stash_list_indexes_by_position() {
        let out =
            "stash@{0}: WIP on main: 1234 work in progress\nstash@{1}: On feature: 5678 thing\n";
        let entries = parse_stash_list(out);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert!(entries[0].message.contains("work in progress"));
        assert_eq!(entries[1].index, 1);
    }

    #[test]
    fn parse_stash_list_empty() {
        assert!(parse_stash_list("").is_empty());
    }

    #[test]
    fn status_entry_categories() {
        let staged = StatusEntry {
            index: 'M',
            worktree: ' ',
            path: PathBuf::from("a"),
        };
        assert!(staged.is_staged());
        assert!(!staged.is_unstaged());

        let unstaged = StatusEntry {
            index: ' ',
            worktree: 'M',
            path: PathBuf::from("b"),
        };
        assert!(!unstaged.is_staged());
        assert!(unstaged.is_unstaged());

        let untracked = StatusEntry {
            index: '?',
            worktree: '?',
            path: PathBuf::from("c"),
        };
        assert!(untracked.is_untracked());
        assert!(!untracked.is_staged());
    }
}
