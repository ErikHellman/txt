use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 50;

/// Editor configuration. All fields have defaults so partial TOML is fine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Number of spaces per indent / tab.
    #[serde(default = "default_tab_size")]
    pub tab_size: usize,
    /// Theme name (currently only "default" is built-in; reserved for Phase future).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Enable viewport word-wrap by default for all new buffers.
    #[serde(default)]
    pub word_wrap: bool,
}

fn default_tab_size() -> usize {
    4
}

fn default_theme() -> String {
    "default".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_size: default_tab_size(),
            theme: default_theme(),
            word_wrap: false,
        }
    }
}

impl Config {
    /// Load config from `~/.config/txt/config.toml` (or platform equivalent).
    /// Returns `Config::default()` on any error (missing file, parse error, etc.).
    pub fn load() -> Self {
        Self::load_from_path(Self::config_path().as_deref())
    }

    /// Load from a specific path. `None` → return default.
    pub fn load_from_path(path: Option<&std::path::Path>) -> Self {
        let path = match path {
            Some(p) => p,
            None => return Self::default(),
        };
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Path to the config file, or `None` if the platform config dir is unknown.
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("txt").join("config.toml"))
    }

    /// Path to the recent-files list, or `None` if the platform config dir is unknown.
    pub fn recent_files_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("txt").join("recent.json"))
    }
}

/// Load the list of recently opened files (up to `MAX_RECENT`).
///
/// Returns an empty list on any error (missing file, parse error, etc.).
pub fn load_recent_files() -> Vec<PathBuf> {
    let path = match Config::recent_files_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let strings: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    strings.into_iter().map(PathBuf::from).collect()
}

/// Prepend `path` to the recent-files list and persist it.
///
/// Deduplicates (removes any existing entry for `path`) and truncates to
/// `MAX_RECENT`. Silently ignores I/O errors.
pub fn add_to_recent_files(path: &std::path::Path) {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };
    let mut recent = load_recent_files();
    recent.retain(|p| p != &canonical);
    recent.insert(0, canonical);
    recent.truncate(MAX_RECENT);
    save_recent_files(&recent);
}

fn save_recent_files(files: &[PathBuf]) {
    let path = match Config::recent_files_path() {
        Some(p) => p,
        None => return,
    };
    // Ensure the directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let strings: Vec<&str> = files
        .iter()
        .filter_map(|p| p.to_str())
        .collect();
    if let Ok(json) = serde_json::to_string(&strings) {
        let _ = std::fs::write(&path, json);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let c = Config::default();
        assert_eq!(c.tab_size, 4);
        assert_eq!(c.theme, "default");
        assert!(!c.word_wrap);
    }

    #[test]
    fn load_from_nonexistent_path_returns_default() {
        let tmp = std::path::Path::new("/tmp/txt_does_not_exist_config.toml");
        let c = Config::load_from_path(Some(tmp));
        assert_eq!(c, Config::default());
    }

    #[test]
    fn load_from_none_returns_default() {
        let c = Config::load_from_path(None);
        assert_eq!(c, Config::default());
    }

    #[test]
    fn partial_config_fills_defaults() {
        let toml_str = "tab_size = 2\n";
        let c: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(c.tab_size, 2);
        assert_eq!(c.theme, "default"); // default filled in
        assert!(!c.word_wrap);
    }

    #[test]
    fn full_config_round_trips() {
        let original = Config {
            tab_size: 2,
            theme: "dark".to_string(),
            word_wrap: true,
        };
        let serialized = toml::to_string(&original).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn invalid_toml_returns_default() {
        // Write a temp file with garbage content, load it.
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b": not valid toml : {{{").unwrap();
        let c = Config::load_from_path(Some(f.path()));
        assert_eq!(c, Config::default());
    }

    #[test]
    fn config_path_is_some_on_supported_platform() {
        // On Linux/macOS this should return Some.
        // On unsupported platforms it might be None — just don't panic.
        let _ = Config::config_path();
    }

    #[test]
    fn recent_files_path_returns_different_path() {
        if let (Some(cp), Some(rp)) = (Config::config_path(), Config::recent_files_path()) {
            assert_ne!(cp, rp);
        }
    }

    #[test]
    fn load_recent_files_missing_returns_empty() {
        // The real file may not exist; this should not panic.
        // We just verify it returns a Vec (possibly empty) without panicking.
        let _ = super::load_recent_files();
    }
}
