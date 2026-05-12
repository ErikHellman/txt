//! Asynchronous "is there a newer version of `txt`?" check.
//!
//! `VersionChecker::spawn` returns immediately and starts a background thread
//! that queries the GitHub Releases API for `ErikHellman/txt`. Any failure
//! (no `curl`, no network, malformed JSON, non-200 response, …) is silent —
//! nothing reaches the main thread and the editor behaves as if the feature
//! had never been enabled.
//!
//! Honours `TXT_DISABLE_VERSION_CHECK` so the PTY-driven test harness stays
//! deterministic and offline.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

const RELEASES_URL: &str = "https://api.github.com/repos/ErikHellman/txt/releases/latest";

pub struct VersionChecker {
    rx: Option<Receiver<String>>,
    latest: Option<String>,
}

impl VersionChecker {
    pub fn spawn() -> Self {
        if std::env::var_os("TXT_DISABLE_VERSION_CHECK").is_some() {
            return Self {
                rx: None,
                latest: None,
            };
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if let Some(tag) = fetch_latest_tag() {
                let _ = tx.send(tag.trim_start_matches('v').to_string());
            }
        });
        Self {
            rx: Some(rx),
            latest: None,
        }
    }

    /// Drain any pending message from the worker. Non-blocking.
    pub fn poll(&mut self) {
        let Some(rx) = self.rx.as_ref() else { return };
        match rx.try_recv() {
            Ok(v) => {
                self.latest = Some(v);
                self.rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
            }
        }
    }

    /// Latest tag, only when strictly newer than the built-in version.
    pub fn newer_version(&self) -> Option<&str> {
        let latest = self.latest.as_deref()?;
        if is_strictly_newer(env!("CARGO_PKG_VERSION"), latest) {
            Some(latest)
        } else {
            None
        }
    }
}

fn fetch_latest_tag() -> Option<String> {
    use std::process::Command;
    let ua = format!("txt/{}", env!("CARGO_PKG_VERSION"));
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "5",
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            &ua,
            RELEASES_URL,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = std::str::from_utf8(&output.stdout).ok()?;
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    Some(json.get("tag_name")?.as_str()?.to_string())
}

/// Strict semver-ish comparison. Numeric components are compared as integers;
/// missing trailing components are treated as zero. Pre-release/build suffixes
/// (`-rc1`, `+build`) are dropped so an unsuffixed `0.5.0` is not "newer" than
/// `0.5.0-rc1`.
pub fn is_strictly_newer(current: &str, latest: &str) -> bool {
    let a = parse_parts(current);
    let b = parse_parts(latest);
    let n = a.len().max(b.len());
    for i in 0..n {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if bv > av {
            return true;
        }
        if bv < av {
            return false;
        }
    }
    false
}

fn parse_parts(v: &str) -> Vec<u64> {
    let v = v.trim_start_matches('v');
    let v = v.split(['-', '+']).next().unwrap_or(v);
    v.split('.').filter_map(|p| p.parse::<u64>().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_minor() {
        assert!(is_strictly_newer("0.5.0", "0.6.0"));
    }

    #[test]
    fn newer_patch() {
        assert!(is_strictly_newer("0.5.0", "0.5.1"));
    }

    #[test]
    fn newer_major() {
        assert!(is_strictly_newer("0.5.9", "1.0.0"));
    }

    #[test]
    fn equal_not_newer() {
        assert!(!is_strictly_newer("0.5.0", "0.5.0"));
    }

    #[test]
    fn older_not_newer() {
        assert!(!is_strictly_newer("0.6.0", "0.5.9"));
    }

    #[test]
    fn v_prefix_ok() {
        assert!(is_strictly_newer("0.5.0", "v0.6.0"));
        assert!(is_strictly_newer("v0.5.0", "0.6.0"));
    }

    #[test]
    fn missing_components_treated_as_zero() {
        assert!(!is_strictly_newer("0.5", "0.5.0"));
        assert!(is_strictly_newer("0.5", "0.5.1"));
    }

    #[test]
    fn prerelease_suffix_stripped() {
        assert!(!is_strictly_newer("0.5.0", "0.5.0-rc1"));
        assert!(is_strictly_newer("0.5.0-rc1", "0.5.1"));
    }

    #[test]
    fn disabled_via_env_yields_no_news() {
        // Safety: this test only reads the env var via Self::spawn, doesn't
        // modify globals. The harness sets this var; ensure spawn produces an
        // inert checker.
        unsafe {
            std::env::set_var("TXT_DISABLE_VERSION_CHECK", "1");
        }
        let mut vc = VersionChecker::spawn();
        vc.poll();
        assert!(vc.newer_version().is_none());
        unsafe {
            std::env::remove_var("TXT_DISABLE_VERSION_CHECK");
        }
    }
}
