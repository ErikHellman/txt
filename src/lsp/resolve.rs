//! Binary resolution and SHA-256 hashing for LSP server executables.
//!
//! - `resolve_binary("rust-analyzer")` walks `PATH` and canonicalizes the result
//!   so version-manager shims (mise/asdf) hash the underlying binary, not the
//!   shim that points at it.
//! - Absolute paths are passed through directly (still canonicalized).
//! - `hash_file` computes SHA-256 of the file's contents.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// The result of resolving a `command` string from the LSP config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinary {
    /// The path returned by PATH lookup (or the absolute path the user gave).
    /// May be a symlink or shim. Shown to the user for transparency.
    pub display_path: PathBuf,
    /// The canonical path after symlink resolution. This is what gets hashed
    /// and stored in the trust store.
    pub canonical_path: PathBuf,
}

/// Resolve a `command` string to a concrete binary on disk.
///
/// - If `command` looks like a path (contains a path separator), use it
///   directly. Otherwise look it up in `PATH` via `which`.
/// - Both branches canonicalize the result so symlinks are followed.
pub fn resolve_binary(command: &str) -> Result<ResolvedBinary> {
    let display_path: PathBuf = if has_path_separator(command) {
        let p = PathBuf::from(command);
        if !p.exists() {
            anyhow::bail!("LSP binary not found: {}", command);
        }
        p
    } else {
        which::which(command).with_context(|| format!("LSP binary not found in PATH: {command}"))?
    };

    let canonical_path = display_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", display_path.display()))?;

    Ok(ResolvedBinary {
        display_path,
        canonical_path,
    })
}

fn has_path_separator(s: &str) -> bool {
    s.contains('/') || s.contains('\\')
}

/// SHA-256 of the file contents at `path`, returned as lowercase hex (64 chars).
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read error while hashing {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hash_known_content() {
        // SHA-256 of empty input is a well-known constant.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty");
        std::fs::write(&p, b"").unwrap();
        let h = hash_file(&p).unwrap();
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_abc() {
        // SHA-256("abc") is also a well-known constant.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"abc").unwrap();
        drop(f);
        let h = hash_file(&p).unwrap();
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_missing_file_errors() {
        let r = hash_file(Path::new("/tmp/txt_definitely_not_here.bin"));
        assert!(r.is_err());
    }

    #[test]
    fn has_path_separator_works() {
        assert!(has_path_separator("/usr/bin/foo"));
        assert!(has_path_separator("./foo"));
        assert!(!has_path_separator("foo"));
        assert!(!has_path_separator("rust-analyzer"));
    }

    #[test]
    fn resolve_absolute_existing_path() {
        // Use Cargo.toml as a stand-in "binary" — has_path_separator triggers
        // the absolute-path branch. We just need the canonicalize step to work.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fakebin");
        std::fs::write(&p, b"#!/bin/sh\necho hi\n").unwrap();
        let abs = p.to_string_lossy().to_string();
        let r = resolve_binary(&abs).unwrap();
        assert_eq!(r.display_path, p);
        // canonical_path should resolve any symlink components in tempdir.
        assert!(r.canonical_path.is_absolute());
    }

    #[test]
    fn resolve_absolute_missing_path_errors() {
        let r = resolve_binary("/tmp/txt_definitely_not_here_xyz");
        assert!(r.is_err());
    }

    #[test]
    fn resolve_canonicalizes_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::write(&real, b"hello").unwrap();
        let link = dir.path().join("link");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&real, &link).unwrap();

        let r = resolve_binary(&link.to_string_lossy()).unwrap();
        assert_eq!(r.display_path, link);
        assert_eq!(
            r.canonical_path.canonicalize().unwrap(),
            real.canonicalize().unwrap()
        );
    }

    #[test]
    fn resolve_unknown_in_path_errors() {
        let r = resolve_binary("txt_lsp_definitely_not_a_real_command_xyz");
        assert!(r.is_err());
    }
}
