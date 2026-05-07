//! Trust-on-first-use store for external binaries (e.g. LSP servers).
//!
//! Persisted as JSON at `~/.config/txt/trusted_binaries.json`. Each entry maps
//! a canonical absolute path to a SHA-256 hash and metadata. The store is
//! generic over `kind` so future external-binary categories (formatters, DAPs)
//! can share the same file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One entry in the trust store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
    /// What kind of binary this is (currently always `"lsp"`).
    pub kind: String,
    /// SHA-256 of the binary, lowercase hex (64 chars).
    pub hash: String,
    /// Server identifier for display (e.g. `"rust-analyzer"`). Best-effort hint.
    #[serde(default)]
    pub server_hint: Option<String>,
    /// ISO-8601 date the entry was created or last updated (e.g. `"2026-05-07"`).
    #[serde(default)]
    pub approved_at: Option<String>,
}

/// User-global trust store. Path → entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrustStore {
    entries: HashMap<PathBuf, TrustEntry>,
}

/// The result of checking a (path, hash) pair against the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
    /// Path is in the store and hash matches.
    Approved,
    /// Path has never been seen.
    Unknown,
    /// Path is known but the hash differs from the stored one.
    HashMismatch { previous_hash: String },
}

impl TrustStore {
    /// Default path: `~/.config/txt/trusted_binaries.json`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("txt").join("trusted_binaries.json"))
    }

    /// Load the store from `~/.config/txt/trusted_binaries.json`.
    /// Returns an empty store on missing path, missing file, or parse error.
    pub fn load() -> Self {
        match Self::default_path() {
            Some(p) => Self::load_from_path(&p),
            None => Self::default(),
        }
    }

    /// Load from a specific path. Returns default on any error.
    pub fn load_from_path(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist to the default path. Best-effort; logs nothing on error.
    pub fn save(&self) {
        if let Some(path) = Self::default_path() {
            let _ = self.save_to_path(&path);
        }
    }

    /// Persist to the given path via atomic write (`<path>.tmp` + rename).
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Check whether `(path, hash)` is approved.
    pub fn check(&self, path: &Path, hash: &str) -> TrustDecision {
        match self.entries.get(path) {
            None => TrustDecision::Unknown,
            Some(entry) if entry.hash == hash => TrustDecision::Approved,
            Some(entry) => TrustDecision::HashMismatch {
                previous_hash: entry.hash.clone(),
            },
        }
    }

    /// Approve `(path, hash)` with the given metadata. Overwrites any existing
    /// entry for `path`.
    pub fn approve(&mut self, path: PathBuf, hash: String, server_hint: Option<String>) {
        self.entries.insert(
            path,
            TrustEntry {
                kind: "lsp".into(),
                hash,
                server_hint,
                approved_at: Some(today_iso()),
            },
        );
    }

    /// Number of entries (for tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Returns today's date as `YYYY-MM-DD` (UTC). Pure stdlib, no chrono dep.
fn today_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Days since 1970-01-01 (UTC).
    let days = (secs / 86_400) as i64;
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Convert days-since-1970-01-01 to (year, month, day). Civil-from-days
/// algorithm (Howard Hinnant's `civil_from_days`).
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_hash(seed: u8) -> String {
        let mut s = String::with_capacity(64);
        for _ in 0..32 {
            s.push_str(&format!("{:02x}", seed));
        }
        s
    }

    #[test]
    fn empty_store_is_default() {
        let s = TrustStore::default();
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn missing_file_returns_default() {
        let s = TrustStore::load_from_path(Path::new("/tmp/txt_nonexistent_trust.json"));
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn corrupt_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("trusted_binaries.json");
        std::fs::write(&p, b": not valid json {{{").unwrap();
        let s = TrustStore::load_from_path(&p);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn check_unknown_path() {
        let s = TrustStore::default();
        let d = s.check(Path::new("/usr/bin/whatever"), &fake_hash(0xab));
        assert_eq!(d, TrustDecision::Unknown);
    }

    #[test]
    fn approve_then_check_matches() {
        let mut s = TrustStore::default();
        let p = PathBuf::from("/usr/bin/rust-analyzer");
        let h = fake_hash(0x42);
        s.approve(p.clone(), h.clone(), Some("rust-analyzer".into()));
        assert_eq!(s.check(&p, &h), TrustDecision::Approved);
    }

    #[test]
    fn approve_then_check_hash_mismatch() {
        let mut s = TrustStore::default();
        let p = PathBuf::from("/usr/bin/rust-analyzer");
        let h_old = fake_hash(0x42);
        let h_new = fake_hash(0x99);
        s.approve(p.clone(), h_old.clone(), None);
        match s.check(&p, &h_new) {
            TrustDecision::HashMismatch { previous_hash } => assert_eq!(previous_hash, h_old),
            other => panic!("expected HashMismatch, got {:?}", other),
        }
    }

    #[test]
    fn approve_overwrites_existing_entry() {
        let mut s = TrustStore::default();
        let p = PathBuf::from("/usr/bin/rust-analyzer");
        s.approve(p.clone(), fake_hash(0x01), None);
        s.approve(p.clone(), fake_hash(0x02), None);
        assert_eq!(s.len(), 1);
        assert_eq!(s.check(&p, &fake_hash(0x02)), TrustDecision::Approved);
    }

    #[test]
    fn round_trip_serialize() {
        let mut s = TrustStore::default();
        s.approve(
            PathBuf::from("/usr/bin/rust-analyzer"),
            fake_hash(0x42),
            Some("rust-analyzer".into()),
        );
        s.approve(PathBuf::from("/usr/bin/clangd"), fake_hash(0x07), None);

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("trusted_binaries.json");
        s.save_to_path(&p).unwrap();
        let loaded = TrustStore::load_from_path(&p);
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            loaded.check(Path::new("/usr/bin/rust-analyzer"), &fake_hash(0x42)),
            TrustDecision::Approved
        );
    }

    #[test]
    fn save_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("trusted_binaries.json");
        let s = TrustStore::default();
        s.save_to_path(&nested).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("trusted_binaries.json");
        TrustStore::default().save_to_path(&p).unwrap();
        let tmp = p.with_extension("json.tmp");
        assert!(!tmp.exists(), ".tmp file should be gone after rename");
    }

    #[test]
    fn today_iso_is_well_formed() {
        let s = today_iso();
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        // Year should be plausible: between 2024 and 2100.
        let y: i32 = s[..4].parse().unwrap();
        assert!((2024..=2100).contains(&y), "year {} out of range", y);
    }

    #[test]
    fn days_to_ymd_known_values() {
        // 1970-01-01 is day 0.
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // 2000-01-01 is day 10957.
        assert_eq!(days_to_ymd(10_957), (2000, 1, 1));
        // 2026-05-07 is day 20_580.
        assert_eq!(days_to_ymd(20_580), (2026, 5, 7));
    }
}
