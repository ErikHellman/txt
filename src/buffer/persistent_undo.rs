//! Persist per-file undo history under `<workspace>/.txt/undo/`.
//!
//! Each persisted file is keyed by a stable digest of the canonical file
//! path so it survives moves of the workspace itself but is per-absolute-
//! file. The on-disk format is JSON via `serde_json` for consistency with
//! the rest of `.txt/`.
//!
//! Loading is gated by a content-hash check: if the file's bytes on disk
//! differ from the hash recorded at save time, the saved history is
//! discarded silently. This prevents stale undo from being applied to a
//! buffer that was edited externally between sessions.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::buffer::history::UndoStackSnapshot;

/// On-disk record. Holds the snapshot plus the content hash at save time.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDisk {
    /// Hex-encoded BLAKE3-or-FNV (we use FNV-1a 64-bit to avoid a new
    /// dependency) of the file's content at save time.
    file_hash: String,
    snapshot: UndoStackSnapshot,
}

/// Compute the on-disk filename for `file_path` inside `workspace`.
fn record_path(workspace: &Path, file_path: &Path) -> PathBuf {
    let canonical = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let key = fnv1a_64(canonical.to_string_lossy().as_bytes());
    workspace
        .join(".txt")
        .join("undo")
        .join(format!("{key:016x}.json"))
}

/// Persist `snapshot` to disk. Silent on I/O failure.
pub fn save(workspace: &Path, file_path: &Path, file_content: &str, snapshot: &UndoStackSnapshot) {
    let p = record_path(workspace, file_path);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = OnDisk {
        file_hash: format!("{:016x}", fnv1a_64(file_content.as_bytes())),
        snapshot: snapshot.clone(),
    };
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = std::fs::write(&p, text);
    }
}

/// Load a saved snapshot for `file_path` from `workspace`. Returns `None` if
/// the record is missing, unparseable, or its file hash no longer matches
/// `current_content`.
pub fn load(
    workspace: &Path,
    file_path: &Path,
    current_content: &str,
) -> Option<UndoStackSnapshot> {
    let p = record_path(workspace, file_path);
    let text = std::fs::read_to_string(&p).ok()?;
    let parsed: OnDisk = serde_json::from_str(&text).ok()?;
    let expected = format!("{:016x}", fnv1a_64(current_content.as_bytes()));
    if parsed.file_hash != expected {
        return None;
    }
    Some(parsed.snapshot)
}

/// 64-bit FNV-1a hash. Sufficient for content-change detection — we are not
/// using this for security.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::history::{EditCommand, UndoEntry};

    fn sample_snapshot() -> UndoStackSnapshot {
        UndoStackSnapshot {
            undo: vec![UndoEntry::Single(EditCommand::Insert {
                at: 3,
                text: "hi".into(),
            })],
            redo: vec![],
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("foo.txt");
        std::fs::write(&file, "content").unwrap();
        let snap = sample_snapshot();
        save(tmp.path(), &file, "content", &snap);
        let loaded = load(tmp.path(), &file, "content").expect("load");
        assert_eq!(loaded.undo.len(), 1);
    }

    #[test]
    fn load_returns_none_on_hash_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("bar.txt");
        std::fs::write(&file, "before").unwrap();
        let snap = sample_snapshot();
        save(tmp.path(), &file, "before", &snap);
        // Pretend the file was changed externally.
        let loaded = load(tmp.path(), &file, "after");
        assert!(loaded.is_none(), "stale undo must not be loaded");
    }

    #[test]
    fn missing_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("never-saved.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(load(tmp.path(), &file, "x").is_none());
    }
}
