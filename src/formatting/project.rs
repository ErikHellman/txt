//! Per-workspace formatting overrides loaded from
//! `<workspace>/.txt/formatters.toml`.
//!
//! Same TOML schema as the global `[formatting]` section in
//! `~/.config/txt/config.toml`: `[indent]` with optional `style` / `width`
//! and `[indent.languages.<lang>]` overrides, plus `[formatters.<lang>]`
//! entries. Project values override global on a field-by-field basis (see
//! [`super::FormattingResolver`]).

use std::path::Path;

use super::FormattingConfig;

/// Load `<workspace>/.txt/formatters.toml`.
///
/// Returns `None` when the file is missing or cannot be parsed (mirrors
/// `WorkspaceLspConfig::load_from_path` graceful-degradation pattern).
pub fn load(workspace: &Path) -> Option<FormattingConfig> {
    let path = workspace.join(".txt").join("formatters.toml");
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str::<FormattingConfig>(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatting::{IndentStyle, PerLangIndent};

    #[test]
    fn missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
    }

    #[test]
    fn invalid_toml_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let txt_dir = dir.path().join(".txt");
        std::fs::create_dir_all(&txt_dir).unwrap();
        std::fs::write(txt_dir.join("formatters.toml"), ": not valid {{").unwrap();
        assert!(load(dir.path()).is_none());
    }

    #[test]
    fn loads_indent_and_formatters() {
        let dir = tempfile::tempdir().unwrap();
        let txt_dir = dir.path().join(".txt");
        std::fs::create_dir_all(&txt_dir).unwrap();
        let toml_str = r#"
[indent]
style = "spaces"
width = 4

[indent.languages.javascript]
width = 2

[formatters.rust]
command = "rustfmt"
args = []
stdin = true
"#;
        std::fs::write(txt_dir.join("formatters.toml"), toml_str).unwrap();
        let cfg = load(dir.path()).expect("should parse");
        assert_eq!(cfg.indent.style, Some(IndentStyle::Spaces));
        assert_eq!(cfg.indent.width, Some(4));
        assert_eq!(
            cfg.indent.languages.get("javascript"),
            Some(&PerLangIndent {
                style: None,
                width: Some(2),
            })
        );
        assert_eq!(cfg.formatters.get("rust").unwrap().command, "rustfmt");
    }
}
