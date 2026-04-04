use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Per-workspace LSP configuration, loaded from `<workspace>/.txt/lsp.toml`.
///
/// When `enabled` is false (the default), the editor uses tree-sitter for
/// syntax highlighting and no LSP server is spawned. When enabled, the user
/// selects a server by name from the `[servers]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkspaceLspConfig {
    /// Whether LSP mode is active for this workspace.
    #[serde(default)]
    pub enabled: bool,
    /// Key into `servers` — the server to use (e.g. `"rust-analyzer"`).
    #[serde(default)]
    pub server: Option<String>,
    /// Available server definitions.
    #[serde(default)]
    pub servers: HashMap<String, LspServerEntry>,
}

/// A single LSP server definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LspServerEntry {
    /// The executable name or path (e.g. `"rust-analyzer"`).
    pub command: String,
    /// Command-line arguments (e.g. `["--stdio"]`).
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional JSON value passed as `initializationOptions` during handshake.
    #[serde(default)]
    pub init_options: Option<serde_json::Value>,
}

impl WorkspaceLspConfig {
    /// Load from `<workspace>/.txt/lsp.toml`.
    ///
    /// Returns a disabled default on missing file, I/O error, or parse error
    /// (same graceful-degradation pattern as `Config::load()`).
    pub fn load(workspace: &Path) -> Self {
        let path = workspace.join(".txt").join("lsp.toml");
        Self::load_from_path(&path)
    }

    /// Load from a specific file path. Returns default on any error.
    pub fn load_from_path(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Whether LSP mode should be activated: enabled *and* a server is selected
    /// that exists in the `servers` table.
    pub fn is_active(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.server {
            Some(key) => self.servers.contains_key(key),
            None => false,
        }
    }

    /// Returns the selected server entry, if the config is active.
    #[allow(dead_code)]
    pub fn active_server(&self) -> Option<&LspServerEntry> {
        if !self.enabled {
            return None;
        }
        self.server.as_ref().and_then(|key| self.servers.get(key))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        let cfg = WorkspaceLspConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.server.is_none());
        assert!(cfg.servers.is_empty());
        assert!(!cfg.is_active());
    }

    #[test]
    fn missing_file_returns_default() {
        let cfg = WorkspaceLspConfig::load_from_path(Path::new("/tmp/txt_nonexistent_lsp.toml"));
        assert!(!cfg.enabled);
    }

    #[test]
    fn invalid_toml_returns_default() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b": not valid toml {{{").unwrap();
        let cfg = WorkspaceLspConfig::load_from_path(f.path());
        assert!(!cfg.enabled);
    }

    #[test]
    fn minimal_enabled_config() {
        let toml_str = r#"
enabled = true
server = "rust-analyzer"

[servers.rust-analyzer]
command = "rust-analyzer"
"#;
        let cfg: WorkspaceLspConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.server.as_deref(), Some("rust-analyzer"));
        assert!(cfg.is_active());
        let entry = cfg.active_server().unwrap();
        assert_eq!(entry.command, "rust-analyzer");
        assert!(entry.args.is_empty());
    }

    #[test]
    fn enabled_but_missing_server_key_not_active() {
        let toml_str = r#"
enabled = true
server = "nonexistent"
"#;
        let cfg: WorkspaceLspConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.enabled);
        assert!(!cfg.is_active());
        assert!(cfg.active_server().is_none());
    }

    #[test]
    fn enabled_without_server_not_active() {
        let toml_str = "enabled = true\n";
        let cfg: WorkspaceLspConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.enabled);
        assert!(!cfg.is_active());
    }

    #[test]
    fn server_with_args_and_init_options() {
        let toml_str = r#"
enabled = true
server = "pyright"

[servers.pyright]
command = "pyright-langserver"
args = ["--stdio"]
init_options = { pythonPath = "/usr/bin/python3" }
"#;
        let cfg: WorkspaceLspConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.is_active());
        let entry = cfg.active_server().unwrap();
        assert_eq!(entry.command, "pyright-langserver");
        assert_eq!(entry.args, vec!["--stdio"]);
        assert!(entry.init_options.is_some());
    }

    #[test]
    fn multiple_servers_selects_correct_one() {
        let toml_str = r#"
enabled = true
server = "tsserver"

[servers.rust-analyzer]
command = "rust-analyzer"

[servers.tsserver]
command = "typescript-language-server"
args = ["--stdio"]
"#;
        let cfg: WorkspaceLspConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.is_active());
        let entry = cfg.active_server().unwrap();
        assert_eq!(entry.command, "typescript-language-server");
    }

    #[test]
    fn load_from_workspace_dir() {
        let dir = tempfile::tempdir().unwrap();
        let txt_dir = dir.path().join(".txt");
        std::fs::create_dir_all(&txt_dir).unwrap();
        std::fs::write(
            txt_dir.join("lsp.toml"),
            "enabled = true\nserver = \"ra\"\n\n[servers.ra]\ncommand = \"rust-analyzer\"\n",
        )
        .unwrap();
        let cfg = WorkspaceLspConfig::load(dir.path());
        assert!(cfg.is_active());
        assert_eq!(cfg.active_server().unwrap().command, "rust-analyzer");
    }

    #[test]
    fn round_trip_serialize() {
        let cfg = WorkspaceLspConfig {
            enabled: true,
            server: Some("ra".into()),
            servers: {
                let mut m = HashMap::new();
                m.insert(
                    "ra".into(),
                    LspServerEntry {
                        command: "rust-analyzer".into(),
                        args: vec![],
                        init_options: None,
                    },
                );
                m
            },
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let deserialized: WorkspaceLspConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, deserialized);
    }
}
