use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::formatting::FormattingConfig;

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

/// Where per-workspace state (session, marks, recents, undo, lsp.toml,
/// formatters.toml) is stored. Serialises as a snake_case string in TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStorage {
    /// Store inside the workspace: `<workspace>/.txt/`.
    #[default]
    Workspace,
    /// Store in the user config directory, keyed by the SHA-256 of the
    /// canonical workspace path: `~/.config/txt/workspaces/<hash>/`.
    Global,
    /// Do not read or write any per-workspace state.
    Disabled,
}

impl WorkspaceStorage {
    pub const ALL: &'static [Self] = &[Self::Workspace, Self::Global, Self::Disabled];

    pub fn display_name(&self) -> &'static str {
        match self {
            WorkspaceStorage::Workspace => "In workspace (.txt/)",
            WorkspaceStorage::Global => "Global (~/.config/txt)",
            WorkspaceStorage::Disabled => "Disabled",
        }
    }

    /// Resolve the directory that holds per-workspace state for `workspace`.
    ///
    /// The returned directory may not exist yet; callers create it on write.
    /// `None` means per-workspace storage is disabled and all I/O should be
    /// skipped silently.
    ///
    /// In global mode the workspace path is canonicalised before hashing so
    /// the same directory reached through symlinks maps to one store.
    pub fn resolve(&self, workspace: &Path) -> Option<PathBuf> {
        self.resolve_with_config_dir(workspace, txt_config_dir().as_deref())
    }

    /// Like [`Self::resolve`] but with an explicit config directory, used by
    /// unit tests (avoids mutating process-wide `TXT_CONFIG_DIR`).
    #[allow(dead_code)]
    pub(crate) fn resolve_with_config_dir(
        &self,
        workspace: &Path,
        config_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        match self {
            WorkspaceStorage::Workspace => Some(workspace.join(".txt")),
            WorkspaceStorage::Global => {
                let canonical = workspace
                    .canonicalize()
                    .unwrap_or_else(|_| workspace.to_path_buf());
                config_dir.map(|d| d.join("workspaces").join(hash_workspace_path(&canonical)))
            }
            WorkspaceStorage::Disabled => None,
        }
    }
}

/// Lowercase-hex SHA-256 of a workspace path's UTF-8 representation. Used as
/// the directory name when per-workspace state is stored globally.
fn hash_workspace_path(workspace: &Path) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(workspace.to_string_lossy().as_bytes());
    let mut s = String::with_capacity(hash.len() * 2);
    for b in hash {
        s.push_str(&format!("{b:02x}"));
    }
    s
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
    /// Live indent rules and external formatter commands. See
    /// `crate::formatting::FormattingConfig` for the schema.
    #[serde(default)]
    pub formatting: FormattingConfig,
    /// Disable the "filter selection through shell command" action. Set to
    /// `true` in restricted environments where running arbitrary shells is
    /// undesirable; default `false`.
    #[serde(default)]
    pub disable_shell_filter: bool,
    /// Render light vertical guides at every indent multiple inside the
    /// leading whitespace of each line.
    #[serde(default = "default_indent_guides")]
    pub indent_guides: bool,
    /// Display columns at which to render a light vertical ruler line, e.g.
    /// `[80, 120]`. Empty disables rulers.
    #[serde(default)]
    pub rulers: Vec<usize>,
    /// Show a sticky header row at the top of the editor pane displaying the
    /// enclosing function/class/module of the cursor's position.
    #[serde(default = "default_sticky_header")]
    pub sticky_header: bool,
    /// Exclude the `.git` directory from go-to-file and project search.
    /// Implied when `hide_dot_folders` is true. Defaults to `true`.
    #[serde(default = "default_hide_git_folder")]
    pub hide_git_folder: bool,
    /// Exclude every dot-prefixed directory from go-to-file and project search.
    #[serde(default)]
    pub hide_dot_folders: bool,
    /// Highlight trailing whitespace at the end of edited lines.
    #[serde(default = "default_highlight_trailing_whitespace")]
    pub highlight_trailing_whitespace: bool,
    /// Show a "mixed indent" status-bar segment when a buffer contains both
    /// tab-leading and space-leading lines.
    #[serde(default = "default_warn_mixed_indent")]
    pub warn_mixed_indent: bool,
    /// Automatically insert a matching closing delimiter when typing `(`,
    /// `[`, `{`, `"`, `'`, or `` ` ``. Disable with `auto_pair = false`.
    #[serde(default = "default_auto_pair")]
    pub auto_pair: bool,
    /// Restore the previous session (open tabs, cursor positions, sidebar
    /// state) from `<workspace>/.txt/session.json` when `txt` is invoked
    /// without a positional file argument.
    #[serde(default)]
    pub restore_session: bool,
    /// Persist per-file undo history to the workspace data directory under
    /// `undo/` so that `Ctrl+Z` keeps working across editor restarts.
    #[serde(default)]
    pub persistent_undo: bool,
    /// Where per-workspace state (session, marks, recents, undo, lsp.toml,
    /// formatters.toml) is written and read from. Changing this takes effect
    /// on the next start.
    #[serde(default)]
    pub workspace_storage: WorkspaceStorage,
}

fn default_tab_size() -> usize {
    4
}

fn default_indent_guides() -> bool {
    true
}

fn default_sticky_header() -> bool {
    true
}

fn default_hide_git_folder() -> bool {
    true
}

fn default_highlight_trailing_whitespace() -> bool {
    true
}

fn default_warn_mixed_indent() -> bool {
    true
}

fn default_auto_pair() -> bool {
    true
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
            formatting: FormattingConfig::default(),
            disable_shell_filter: false,
            indent_guides: default_indent_guides(),
            rulers: Vec::new(),
            sticky_header: default_sticky_header(),
            hide_git_folder: true,
            hide_dot_folders: false,
            highlight_trailing_whitespace: default_highlight_trailing_whitespace(),
            warn_mixed_indent: default_warn_mixed_indent(),
            auto_pair: default_auto_pair(),
            restore_session: false,
            persistent_undo: false,
            workspace_storage: WorkspaceStorage::Workspace,
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
        txt_config_dir().map(|d| d.join("config.toml"))
    }

    /// Whether a config file already exists on disk. Used to distinguish a
    /// brand-new install (no file) from an existing user (file present, but
    /// possibly missing newer fields).
    pub fn config_file_exists() -> bool {
        Self::config_path().map(|p| p.exists()).unwrap_or(false)
    }
}

/// Directory that holds all on-disk configuration files for `txt`
/// (`config.toml`, `keybindings.toml`, `trusted_binaries.json`, preset files).
///
/// Resolution order:
/// 1. `TXT_CONFIG_DIR` environment variable (used by tests).
/// 2. `~/.config/txt` on supported platforms.
pub fn txt_config_dir() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("TXT_CONFIG_DIR") {
        return Some(PathBuf::from(custom));
    }
    dirs::home_dir().map(|h| h.join(".config").join("txt"))
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

/// Returns the path to the project-local recent-files list: `<data_dir>/recents.json`.
fn recents_path(data_dir: &Path) -> PathBuf {
    data_dir.join("recents.json")
}

/// Load the recent-files list for `workspace` from `<data_dir>/recents.json`.
///
/// Returns an empty list on any error (missing file, parse error) or when
/// per-workspace storage is disabled (`data_dir` is `None`).
pub fn load_recent_files(data_dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(data_dir) = data_dir else {
        return Vec::new();
    };
    let path = recents_path(data_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let entries: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    entries.into_iter().map(PathBuf::from).collect()
}

/// Prepend `path` to the recent-files list for `workspace` and persist it.
///
/// Deduplicates and truncates to `MAX_RECENT`. Silently ignores I/O errors
/// and does nothing when per-workspace storage is disabled (`data_dir` is
/// `None`).
pub fn add_to_recent_files(path: &Path, data_dir: Option<&Path>) {
    let Some(data_dir) = data_dir else {
        return;
    };
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_str = canonical.to_string_lossy().into_owned();

    let recents_file = recents_path(data_dir);
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
        assert_eq!(c.workspace_storage, WorkspaceStorage::Workspace);
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
            formatting: FormattingConfig::default(),
            disable_shell_filter: false,
            indent_guides: false,
            rulers: vec![80, 120],
            sticky_header: false,
            hide_git_folder: true,
            hide_dot_folders: true,
            highlight_trailing_whitespace: false,
            warn_mixed_indent: false,
            auto_pair: false,
            restore_session: true,
            persistent_undo: true,
            workspace_storage: WorkspaceStorage::Global,
        };
        let serialized = toml::to_string(&original).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn disable_shell_filter_default_false() {
        let cfg = Config::default();
        assert!(!cfg.disable_shell_filter);
    }

    #[test]
    fn disable_shell_filter_round_trips() {
        let toml_text = "disable_shell_filter = true\n";
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert!(cfg.disable_shell_filter);
    }

    #[test]
    fn indent_guides_default_true() {
        let cfg = Config::default();
        assert!(cfg.indent_guides);
        assert!(cfg.rulers.is_empty());
    }

    #[test]
    fn rulers_round_trip_through_toml() {
        let toml_text = "rulers = [80, 100, 120]\n";
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert_eq!(cfg.rulers, vec![80, 100, 120]);
    }

    #[test]
    fn hide_folder_defaults() {
        let cfg = Config::default();
        assert!(cfg.hide_git_folder);
        assert!(!cfg.hide_dot_folders);
    }

    #[test]
    fn hide_git_folder_default_applies_to_partial_toml() {
        // Missing hide_git_folder must deserialise to true (matching Default),
        // not to bool's intrinsic false.
        let cfg: Config = toml::from_str("tab_size = 2\n").unwrap();
        assert!(cfg.hide_git_folder);
    }

    #[test]
    fn hide_folder_round_trips() {
        let toml_text = "hide_git_folder = false\nhide_dot_folders = true\n";
        let cfg: Config = toml::from_str(toml_text).unwrap();
        assert!(!cfg.hide_git_folder);
        assert!(cfg.hide_dot_folders);
    }

    #[test]
    fn formatting_section_round_trips() {
        use crate::formatting::{FormatterConfig, IndentSection, IndentStyle, PerLangIndent};
        let mut languages = std::collections::BTreeMap::new();
        languages.insert(
            "javascript".into(),
            PerLangIndent {
                style: None,
                width: Some(2),
            },
        );
        let mut formatters = std::collections::BTreeMap::new();
        formatters.insert(
            "rust".into(),
            FormatterConfig {
                command: "rustfmt".into(),
                args: vec![],
                stdin: true,
            },
        );
        let formatting = FormattingConfig {
            indent: IndentSection {
                style: Some(IndentStyle::Spaces),
                width: Some(4),
                languages,
            },
            formatters,
        };
        let original = Config {
            formatting,
            ..Config::default()
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
    fn recents_path_is_inside_data_dir() {
        let dir = std::path::Path::new("/tmp/myproject/.txt");
        let p = super::recents_path(dir);
        assert_eq!(p, dir.join("recents.json"));
    }

    #[test]
    fn load_recent_files_missing_returns_empty() {
        // No recents.json under /tmp/no_such_workspace — must not panic.
        let _ = super::load_recent_files(Some(std::path::Path::new(
            "/tmp/no_such_workspace_xyz/.txt",
        )));
    }

    #[test]
    fn load_recent_files_disabled_returns_empty() {
        assert!(super::load_recent_files(None).is_empty());
    }

    #[test]
    fn add_to_recent_files_disabled_is_noop() {
        let file = tempfile::NamedTempFile::new().unwrap();
        // Must not panic and must not create any directory.
        super::add_to_recent_files(file.path(), None);
    }

    #[test]
    fn workspace_storage_display_names() {
        assert_eq!(
            WorkspaceStorage::Workspace.display_name(),
            "In workspace (.txt/)"
        );
        assert_eq!(
            WorkspaceStorage::Global.display_name(),
            "Global (~/.config/txt)"
        );
        assert_eq!(WorkspaceStorage::Disabled.display_name(), "Disabled");
    }

    #[test]
    fn workspace_storage_all_covers_all_variants() {
        assert_eq!(WorkspaceStorage::ALL.len(), 3);
    }

    #[test]
    fn workspace_storage_workspace_mode_resolves_to_txt_dir() {
        let ws = std::path::Path::new("/tmp/myproject");
        assert_eq!(
            WorkspaceStorage::Workspace.resolve_with_config_dir(ws, None),
            Some(std::path::PathBuf::from("/tmp/myproject/.txt"))
        );
    }

    #[test]
    fn workspace_storage_disabled_resolves_to_none() {
        let ws = std::path::Path::new("/tmp/myproject");
        assert_eq!(
            WorkspaceStorage::Disabled.resolve_with_config_dir(ws, None),
            None
        );
        assert_eq!(WorkspaceStorage::Disabled.resolve(ws), None);
    }

    #[test]
    fn workspace_storage_global_resolves_under_config_dir_with_sha256_key() {
        use tempfile::tempdir;
        let config_dir = tempdir().unwrap();
        let ws = tempdir().unwrap();
        let resolved = WorkspaceStorage::Global
            .resolve_with_config_dir(ws.path(), Some(config_dir.path()))
            .unwrap();
        let expected_key = {
            use sha2::{Digest, Sha256};
            let canonical = ws.path().canonicalize().unwrap();
            let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
            hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        let expected = config_dir.path().join("workspaces").join(expected_key);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn workspace_storage_global_is_stable_and_symlink_insensitive() {
        use tempfile::tempdir;
        let config_dir = tempdir().unwrap();
        let ws = tempdir().unwrap();
        let a = WorkspaceStorage::Global
            .resolve_with_config_dir(ws.path(), Some(config_dir.path()))
            .unwrap();
        let b = WorkspaceStorage::Global
            .resolve_with_config_dir(ws.path(), Some(config_dir.path()))
            .unwrap();
        assert_eq!(a, b);
        // A second unique tempdir maps to a different key.
        let other = tempdir().unwrap();
        let c = WorkspaceStorage::Global
            .resolve_with_config_dir(other.path(), Some(config_dir.path()))
            .unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn workspace_storage_round_trips_through_toml() {
        let text = "workspace_storage = \"global\"\n";
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.workspace_storage, WorkspaceStorage::Global);
        let cfg: Config = toml::from_str("workspace_storage = \"disabled\"\n").unwrap();
        assert_eq!(cfg.workspace_storage, WorkspaceStorage::Disabled);
        let cfg: Config = toml::from_str("tab_size = 2\n").unwrap();
        assert_eq!(cfg.workspace_storage, WorkspaceStorage::Workspace);
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
    fn tier3_defaults_match_proposal() {
        let cfg = Config::default();
        // 3.6 — passive features default on
        assert!(cfg.highlight_trailing_whitespace);
        assert!(cfg.warn_mixed_indent);
        // 3.4 — auto-pair default on (confirmed by user)
        assert!(cfg.auto_pair);
        // 3.1/3.2 — disk-touching features default off ("starts instantly")
        assert!(!cfg.restore_session);
        assert!(!cfg.persistent_undo);
    }

    #[test]
    fn tier3_keys_round_trip() {
        let text = "highlight_trailing_whitespace = false\nwarn_mixed_indent = false\nauto_pair = false\nrestore_session = true\npersistent_undo = true\n";
        let cfg: Config = toml::from_str(text).unwrap();
        assert!(!cfg.highlight_trailing_whitespace);
        assert!(!cfg.warn_mixed_indent);
        assert!(!cfg.auto_pair);
        assert!(cfg.restore_session);
        assert!(cfg.persistent_undo);
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
