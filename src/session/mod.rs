//! Per-workspace session persistence.
//!
//! When `restore_session = true` is set in `~/.config/txt/config.toml`,
//! every clean shutdown writes the list of open tabs (with cursor positions
//! and viewport scroll) to `<workspace>/.txt/session.json`. The next time
//! `txt` is launched in the same workspace *without* a positional file
//! argument, the saved tabs are reopened.
//!
//! The format is JSON via `serde_json` to match the existing workspace-local
//! files (`recents.json`, `marks.json`, `jumps.json`). Files that no longer
//! exist on disk are silently skipped at load time so a renamed/moved file
//! never breaks startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A snapshot of one open tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabState {
    pub path: PathBuf,
    /// Primary cursor byte offset. Clamped to the file's byte length on
    /// load — a file may have shrunk since the session was saved.
    pub cursor_byte: usize,
    /// Viewport top line.
    pub viewport_top: usize,
}

/// A snapshot of the editor at quit time.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<TabState>,
    pub active: usize,
    pub sidebar_open: bool,
}

impl Session {
    /// Path to the session file inside `workspace`.
    fn path_for(workspace: &Path) -> PathBuf {
        workspace.join(".txt").join("session.json")
    }

    /// Load the session for `workspace`. Returns `None` when the file is
    /// missing or unparseable.
    pub fn load(workspace: &Path) -> Option<Self> {
        let p = Self::path_for(workspace);
        let text = std::fs::read_to_string(&p).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Persist `self` to `<workspace>/.txt/session.json`. Silently ignores
    /// I/O errors.
    pub fn save(&self, workspace: &Path) {
        let p = Self::path_for(workspace);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&p, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Session {
            tabs: vec![
                TabState {
                    path: PathBuf::from("foo.rs"),
                    cursor_byte: 42,
                    viewport_top: 7,
                },
                TabState {
                    path: PathBuf::from("bar.rs"),
                    cursor_byte: 0,
                    viewport_top: 0,
                },
            ],
            active: 1,
            sidebar_open: true,
        };
        s.save(tmp.path());
        let loaded = Session::load(tmp.path()).expect("load");
        assert_eq!(loaded, s);
    }

    #[test]
    fn missing_session_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Session::load(tmp.path()).is_none());
    }
}
