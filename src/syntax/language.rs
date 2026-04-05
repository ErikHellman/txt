use std::path::Path;
use tree_sitter::Language;

/// Languages with tree-sitter grammar support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    Json,
    Kotlin,
    #[default]
    Unknown,
}

impl Lang {
    /// Detect language from a file path (by extension).
    pub fn from_path(path: &Path) -> Self {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        Self::from_extension(ext)
    }

    /// Detect language from a file extension string (lowercase expected).
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            "kt" | "kts" => Self::Kotlin,
            _ => Self::Unknown,
        }
    }

    /// Returns the tree-sitter `Language` for this grammar, or `None` if unsupported.
    pub fn ts_language(self) -> Option<Language> {
        match self {
            Self::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Self::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Self::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Self::Json => Some(tree_sitter_json::LANGUAGE.into()),
            Self::Kotlin => Some(tree_sitter_kotlin_codanna::language()),
            Self::Unknown => None,
        }
    }

    /// Human-readable name for the status bar.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Json => "JSON",
            Self::Kotlin => "Kotlin",
            Self::Unknown => "",
        }
    }

    /// The string to prepend (and remove) when toggling line comments.
    /// Returns `None` for languages that don't support line comments.
    pub fn comment_prefix(self) -> Option<&'static str> {
        match self {
            Self::Rust | Self::JavaScript | Self::Kotlin => Some("// "),
            Self::Python => Some("# "),
            Self::Json | Self::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust() {
        assert_eq!(Lang::from_extension("rs"), Lang::Rust);
        assert_eq!(Lang::from_path(Path::new("main.rs")), Lang::Rust);
    }

    #[test]
    fn detect_python() {
        assert_eq!(Lang::from_extension("py"), Lang::Python);
        assert_eq!(Lang::from_extension("pyw"), Lang::Python);
    }

    #[test]
    fn detect_javascript() {
        assert_eq!(Lang::from_extension("js"), Lang::JavaScript);
        assert_eq!(Lang::from_extension("mjs"), Lang::JavaScript);
    }

    #[test]
    fn detect_json() {
        assert_eq!(Lang::from_extension("json"), Lang::Json);
        assert_eq!(Lang::from_extension("jsonc"), Lang::Json);
    }

    #[test]
    fn detect_kotlin() {
        assert_eq!(Lang::from_extension("kt"), Lang::Kotlin);
        assert_eq!(Lang::from_extension("kts"), Lang::Kotlin);
        assert_eq!(Lang::from_path(Path::new("Main.kt")), Lang::Kotlin);
    }

    #[test]
    fn unknown_extension() {
        assert_eq!(Lang::from_extension("txt"), Lang::Unknown);
        assert_eq!(Lang::from_extension(""), Lang::Unknown);
    }

    #[test]
    fn ts_language_available_for_known() {
        assert!(Lang::Rust.ts_language().is_some());
        assert!(Lang::Python.ts_language().is_some());
        assert!(Lang::JavaScript.ts_language().is_some());
        assert!(Lang::Json.ts_language().is_some());
        assert!(Lang::Kotlin.ts_language().is_some());
    }

    #[test]
    fn ts_language_none_for_unknown() {
        assert!(Lang::Unknown.ts_language().is_none());
    }

    #[test]
    fn names() {
        assert_eq!(Lang::Rust.name(), "Rust");
        assert_eq!(Lang::Kotlin.name(), "Kotlin");
        assert_eq!(Lang::Unknown.name(), "");
    }

    #[test]
    fn comment_prefix_supported_languages() {
        assert_eq!(Lang::Rust.comment_prefix(), Some("// "));
        assert_eq!(Lang::JavaScript.comment_prefix(), Some("// "));
        assert_eq!(Lang::Python.comment_prefix(), Some("# "));
        assert_eq!(Lang::Kotlin.comment_prefix(), Some("// "));
    }

    #[test]
    fn comment_prefix_unsupported_languages() {
        assert_eq!(Lang::Json.comment_prefix(), None);
        assert_eq!(Lang::Unknown.comment_prefix(), None);
    }
}
