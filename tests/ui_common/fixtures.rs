//! Tempdir + config-dir builders for UI tests.  Every test gets its own
//! isolated workspace and `TXT_CONFIG_DIR`, with a seeded `config.toml`
//! that skips the first-run welcome overlay.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::harness::{SessionOptions, TxtSession};

/// Bundle of tempdirs and seeded files that a test owns.  Drop deletes
/// the directories.
pub struct Fixture {
    pub workspace: TempDir,
    pub config: TempDir,
}

impl Fixture {
    /// Build an empty workspace and a config dir whose `config.toml` records
    /// the current crate version so `AppState::new` does not pop the welcome
    /// overlay.
    pub fn new() -> Self {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let config = tempfile::tempdir().expect("config tempdir");
        seed_config(config.path());
        Self { workspace, config }
    }

    pub fn workspace_path(&self) -> PathBuf {
        self.workspace.path().to_path_buf()
    }

    pub fn config_path(&self) -> PathBuf {
        self.config.path().to_path_buf()
    }

    /// Write `contents` to `<workspace>/<rel>` and return the absolute path.
    pub fn write_file(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.workspace.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create_dir_all");
        }
        std::fs::write(&path, contents).expect("write fixture");
        path
    }

    /// Launch a session with no file argument (empty editor).
    pub fn launch_empty(&self) -> TxtSession {
        let session = TxtSession::launch(SessionOptions::new(
            self.workspace_path(),
            self.config_path(),
        ));
        session.wait_for_first_paint();
        session
    }

    /// Launch a session opening `path` and wait until its filename appears
    /// in the status bar.  `path` must be inside the workspace tempdir.
    pub fn open(&self, path: &Path) -> TxtSession {
        let session = TxtSession::launch(
            SessionOptions::new(self.workspace_path(), self.config_path())
                .arg(path.to_string_lossy()),
        );
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("path must have a UTF-8 filename")
            .to_string();
        session.wait_until(
            |s| s.contents().contains(&filename),
            std::time::Duration::from_secs(5),
        );
        session
    }
}

fn seed_config(dir: &std::path::Path) {
    let version = env!("CARGO_PKG_VERSION");
    let body = format!("last_seen_version = \"{version}\"\n");
    std::fs::write(dir.join("config.toml"), body).expect("seed config.toml");
}
