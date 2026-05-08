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

/// Predefined keybinding presets. Serialises as snake_case strings in TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeymapPreset {
    #[default]
    Default,
    IntellijIdea,
    VsCode,
}

impl KeymapPreset {
    pub const ALL: &'static [Self] = &[Self::Default, Self::IntellijIdea, Self::VsCode];

    pub fn display_name(&self) -> &'static str {
        match self {
            KeymapPreset::Default => "Default",
            KeymapPreset::IntellijIdea => "IntelliJ IDEA",
            KeymapPreset::VsCode => "VS Code",
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
    /// Active keybinding preset.
    #[serde(default)]
    pub keymap_preset: KeymapPreset,
    /// The last version of `txt` whose welcome / changelog screen the user has
    /// dismissed. `None` on a brand-new install; updated to the current version
    /// each time an onboarding overlay is shown.
    #[serde(default)]
    pub last_seen_version: Option<String>,
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
            keymap_preset: KeymapPreset::Default,
            last_seen_version: None,
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

    /// Whether a config file already exists on disk. Used to distinguish a
    /// brand-new install (no file) from an existing user (file present, but
    /// possibly missing newer fields).
    pub fn config_file_exists() -> bool {
        Self::config_path().map(|p| p.exists()).unwrap_or(false)
    }
}

/// Parse a semver-ish version string into `(major, minor)`. Accepts a leading
/// `v`. Returns `None` if either component is missing or non-numeric.
pub fn parse_minor_version(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Parse a semver-ish version string into `(major, minor, patch)`. Accepts a
/// leading `v`. A missing patch component is treated as `0`. Returns `None`
/// if major or minor are missing or non-numeric.
pub fn parse_full_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Whether a changelog overlay should be shown when going from `last` to
/// `current`. True only when both parse and the (major, minor) pair has
/// advanced — patch-only bumps are silent.
pub fn is_minor_or_major_upgrade(last: &str, current: &str) -> bool {
    match (parse_minor_version(last), parse_minor_version(current)) {
        (Some(l), Some(c)) => l < c,
        _ => false,
    }
}

/// Returns the path to the project-local recent-files list: `<workspace>/.txt/recents.json`.
fn recents_path(workspace: &Path) -> PathBuf {
    workspace.join(".txt").join("recents.json")
}

/// Load the recent-files list for `workspace` from `<workspace>/.txt/recents.json`.
///
/// Returns an empty list on any error (missing file, parse error).
pub fn load_recent_files(workspace: &Path) -> Vec<PathBuf> {
    let path = recents_path(workspace);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let entries: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    entries.into_iter().map(PathBuf::from).collect()
}

/// Prepend `path` to the recent-files list for `workspace` and persist it.
///
/// Deduplicates and truncates to `MAX_RECENT`. Silently ignores I/O errors.
pub fn add_to_recent_files(path: &Path, workspace: &Path) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_str = canonical.to_string_lossy().into_owned();

    let recents_file = recents_path(workspace);
    let mut entries: Vec<String> = std::fs::read_to_string(&recents_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();

    entries.retain(|p| p != &canonical_str);
    entries.insert(0, canonical_str);
    entries.truncate(MAX_RECENT);

    if let Some(parent) = recents_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&entries) {
        let _ = std::fs::write(&recents_file, json);
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
        assert_eq!(c.keymap_preset, KeymapPreset::Default);
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
            keymap_preset: KeymapPreset::IntellijIdea,
            last_seen_version: Some("0.3.0".to_string()),
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
    fn recents_path_is_inside_workspace() {
        let ws = std::path::Path::new("/tmp/myproject");
        let p = super::recents_path(ws);
        assert_eq!(p, ws.join(".txt").join("recents.json"));
    }

    #[test]
    fn load_recent_files_missing_returns_empty() {
        // No .txt/recents.json under /tmp/no_such_workspace — must not panic.
        let _ = super::load_recent_files(std::path::Path::new("/tmp/no_such_workspace_xyz"));
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

    #[test]
    fn keymap_preset_display_names() {
        assert_eq!(KeymapPreset::Default.display_name(), "Default");
        assert_eq!(KeymapPreset::IntellijIdea.display_name(), "IntelliJ IDEA");
        assert_eq!(KeymapPreset::VsCode.display_name(), "VS Code");
    }

    #[test]
    fn keymap_preset_all_covers_all_variants() {
        assert_eq!(KeymapPreset::ALL.len(), 3);
    }

    #[test]
    fn parse_minor_version_accepts_leading_v_and_three_parts() {
        assert_eq!(super::parse_minor_version("v0.3.0"), Some((0, 3)));
        assert_eq!(super::parse_minor_version("0.3.0"), Some((0, 3)));
        assert_eq!(super::parse_minor_version("1.10.5"), Some((1, 10)));
    }

    #[test]
    fn parse_full_version_handles_two_or_three_parts() {
        assert_eq!(super::parse_full_version("0.3.0"), Some((0, 3, 0)));
        assert_eq!(super::parse_full_version("v1.2.4"), Some((1, 2, 4)));
        // Two-part versions get a 0 patch.
        assert_eq!(super::parse_full_version("1.5"), Some((1, 5, 0)));
        // Garbage patch falls back to 0 rather than failing.
        assert_eq!(super::parse_full_version("0.3.foo"), Some((0, 3, 0)));
    }

    #[test]
    fn parse_minor_version_rejects_garbage() {
        assert_eq!(super::parse_minor_version(""), None);
        assert_eq!(super::parse_minor_version("foo"), None);
        assert_eq!(super::parse_minor_version("1"), None);
    }

    #[test]
    fn upgrade_detection() {
        assert!(super::is_minor_or_major_upgrade("0.2.0", "0.3.0"));
        assert!(super::is_minor_or_major_upgrade("0.2.5", "0.3.0"));
        assert!(super::is_minor_or_major_upgrade("0.9.0", "1.0.0"));
        // Patch-only bumps are silent.
        assert!(!super::is_minor_or_major_upgrade("0.3.0", "0.3.1"));
        assert!(!super::is_minor_or_major_upgrade("0.3.0", "0.3.0"));
        // Same or newer minor → no overlay.
        assert!(!super::is_minor_or_major_upgrade("0.4.0", "0.3.0"));
    }
}
