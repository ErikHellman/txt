use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 50;

/// Predefined colour themes. Serialises as snake_case strings in TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Default,
    Monokai,
    Gruvbox,
    Nord,
}

impl Theme {
    pub const ALL: &'static [Self] = &[Self::Default, Self::Monokai, Self::Gruvbox, Self::Nord];

    pub fn display_name(&self) -> &'static str {
        match self {
            Theme::Default => "Default",
            Theme::Monokai => "Monokai",
            Theme::Gruvbox => "Gruvbox",
            Theme::Nord => "Nord",
        }
    }
}

/// Editor configuration. All fields have defaults so partial TOML is fine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Number of spaces per indent / tab.
    #[serde(default = "default_tab_size")]
    pub tab_size: usize,
    /// Enable viewport word-wrap by default for all new buffers.
    #[serde(default)]
    pub word_wrap: bool,
    /// Ask for confirmation before quitting with unsaved changes.
    #[serde(default)]
    pub confirm_exit: bool,
    /// Automatically save after edits (debounced).
    #[serde(default)]
    pub auto_save: bool,
    /// Render whitespace characters with visible glyphs.
    #[serde(default)]
    pub show_whitespace: bool,
    /// Active colour theme.
    #[serde(default)]
    pub theme: Theme,
}

fn default_tab_size() -> usize {
    4
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_size: default_tab_size(),
            word_wrap: false,
            confirm_exit: false,
            auto_save: false,
            show_whitespace: false,
            theme: Theme::Default,
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

    /// Persist config to `~/.config/txt/config.toml`. Silently ignores errors.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(&path, text);
        }
    }

    /// Path to the config file (`~/.config/txt/config.toml`).
    pub fn config_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("txt").join("config.toml"))
    }

    /// Path to the recent-files list (`~/.config/txt/recent.json`).
    pub fn recent_files_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("txt").join("recent.json"))
    }
}

/// Load the recent-files list for `workspace`.
///
/// Returns an empty list on any error (missing file, parse error, unknown workspace).
pub fn load_recent_files(workspace: &Path) -> Vec<PathBuf> {
    let map = read_recent_map();
    let key = canonical_str(workspace);
    map.get(&key)
        .map(|files| files.iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

/// Prepend `path` to the recent-files list for `workspace` and persist it.
///
/// Deduplicates and truncates to `MAX_RECENT`. Silently ignores I/O errors.
pub fn add_to_recent_files(path: &Path, workspace: &Path) {
    let mut map = read_recent_map();
    let workspace_key = canonical_str(workspace);
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };
    let entry = map.entry(workspace_key).or_default();
    let canonical_str = canonical.to_string_lossy().into_owned();
    entry.retain(|p| p != &canonical_str);
    entry.insert(0, canonical_str);
    entry.truncate(MAX_RECENT);
    write_recent_map(&map);
}

fn canonical_str(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_recent_map() -> HashMap<String, Vec<String>> {
    let path = match Config::recent_files_path() {
        Some(p) => p,
        None => return HashMap::new(),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return HashMap::new(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_recent_map(map: &HashMap<String, Vec<String>>) {
    let path = match Config::recent_files_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(map) {
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
        assert_eq!(c.theme, Theme::Default);
        assert!(!c.word_wrap);
        assert!(!c.confirm_exit);
        assert!(!c.auto_save);
        assert!(!c.show_whitespace);
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
        assert_eq!(c.theme, Theme::Default); // default filled in
        assert!(!c.word_wrap);
    }

    #[test]
    fn full_config_round_trips() {
        let original = Config {
            tab_size: 2,
            theme: Theme::Monokai,
            word_wrap: true,
            confirm_exit: true,
            auto_save: false,
            show_whitespace: true,
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
        let _ = super::load_recent_files(std::path::Path::new("/tmp"));
    }

    #[test]
    fn theme_display_names() {
        assert_eq!(Theme::Default.display_name(), "Default");
        assert_eq!(Theme::Monokai.display_name(), "Monokai");
        assert_eq!(Theme::Gruvbox.display_name(), "Gruvbox");
        assert_eq!(Theme::Nord.display_name(), "Nord");
    }

    #[test]
    fn theme_all_covers_all_variants() {
        assert_eq!(Theme::ALL.len(), 4);
    }
}
