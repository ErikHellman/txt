use std::path::Path;
use tree_sitter::Language;

/// Languages with tree-sitter grammar support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    Json,
    Markdown,
    Sh,
    TypeScript,
    Tsx,
    CSharp,
    Java,
    Go,
    Kotlin,
    Groovy,
    Yaml,
    Properties,
    Toml,
    Html,
    Css,
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
    /// Also accepts common language identifiers used in code fences.
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            // File extensions
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            "md" | "markdown" => Self::Markdown,
            "sh" | "bash" | "zsh" => Self::Sh,
            "ts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "cs" => Self::CSharp,
            "java" => Self::Java,
            "go" => Self::Go,
            "kt" | "kts" => Self::Kotlin,
            "groovy" | "gradle" => Self::Groovy,
            "yml" | "yaml" => Self::Yaml,
            "properties" => Self::Properties,
            "toml" => Self::Toml,
            "html" | "htm" => Self::Html,
            "css" => Self::Css,
            // Code fence language identifiers (lowercase) for supported grammars only
            "rust" => Self::Rust,
            "python" => Self::Python,
            "javascript" => Self::JavaScript,
            "shell" => Self::Sh,
            "typescript" => Self::TypeScript,
            "csharp" => Self::CSharp,
            "kotlin" => Self::Kotlin,
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
            Self::Markdown => Some(tree_sitter_md::LANGUAGE.into()),
            Self::Sh => Some(tree_sitter_bash::LANGUAGE.into()),
            Self::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Self::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            Self::CSharp => Some(tree_sitter_c_sharp::LANGUAGE.into()),
            Self::Java => Some(tree_sitter_java::LANGUAGE.into()),
            Self::Go => Some(tree_sitter_go::LANGUAGE.into()),
            Self::Kotlin => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
            Self::Groovy => Some(tree_sitter_groovy::LANGUAGE.into()),
            Self::Yaml => Some(tree_sitter_yaml::LANGUAGE.into()),
            Self::Properties => Some(tree_sitter_properties::LANGUAGE.into()),
            Self::Toml => Some(tree_sitter_toml_ng::LANGUAGE.into()),
            Self::Html => Some(tree_sitter_html::LANGUAGE.into()),
            Self::Css => Some(tree_sitter_css::LANGUAGE.into()),
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
            Self::Markdown => "Markdown",
            Self::Sh => "Shell",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
            Self::CSharp => "C#",
            Self::Java => "Java",
            Self::Go => "Go",
            Self::Kotlin => "Kotlin",
            Self::Groovy => "Groovy",
            Self::Yaml => "YAML",
            Self::Properties => "Properties",
            Self::Toml => "TOML",
            Self::Unknown => "",
        }
    }

    /// The string to prepend (and remove) when toggling line comments.
    /// Returns `None` for languages that don't support line comments.
    pub fn comment_prefix(self) -> Option<&'static str> {
        match self {
            Self::Rust
            | Self::JavaScript
            | Self::TypeScript
            | Self::Tsx
            | Self::CSharp
            | Self::Java
            | Self::Go
            | Self::Kotlin
            | Self::Groovy => Some("// "),
            Self::Python | Self::Sh | Self::Yaml | Self::Properties | Self::Toml => Some("# "),
            Self::Json | Self::Markdown | Self::Html | Self::Css | Self::Unknown => None,
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
    fn detect_shell() {
        assert_eq!(Lang::from_extension("sh"), Lang::Sh);
        assert_eq!(Lang::from_extension("bash"), Lang::Sh);
        assert_eq!(Lang::from_extension("zsh"), Lang::Sh);
        assert_eq!(Lang::from_extension("shell"), Lang::Sh);
        assert_eq!(Lang::from_path(Path::new("script.sh")), Lang::Sh);
    }

    #[test]
    fn detect_typescript() {
        assert_eq!(Lang::from_extension("ts"), Lang::TypeScript);
        assert_eq!(Lang::from_extension("typescript"), Lang::TypeScript);
        assert_eq!(Lang::from_extension("tsx"), Lang::Tsx);
        assert_eq!(Lang::from_path(Path::new("App.tsx")), Lang::Tsx);
    }

    #[test]
    fn detect_csharp() {
        assert_eq!(Lang::from_extension("cs"), Lang::CSharp);
        assert_eq!(Lang::from_extension("csharp"), Lang::CSharp);
    }

    #[test]
    fn detect_java() {
        assert_eq!(Lang::from_extension("java"), Lang::Java);
    }

    #[test]
    fn detect_go() {
        assert_eq!(Lang::from_extension("go"), Lang::Go);
    }

    #[test]
    fn detect_kotlin() {
        assert_eq!(Lang::from_extension("kt"), Lang::Kotlin);
        assert_eq!(Lang::from_extension("kts"), Lang::Kotlin);
        assert_eq!(Lang::from_path(Path::new("build.gradle.kts")), Lang::Kotlin);
    }

    #[test]
    fn detect_groovy() {
        assert_eq!(Lang::from_extension("groovy"), Lang::Groovy);
        assert_eq!(Lang::from_extension("gradle"), Lang::Groovy);
        assert_eq!(Lang::from_path(Path::new("build.gradle")), Lang::Groovy);
    }

    #[test]
    fn detect_yaml() {
        assert_eq!(Lang::from_extension("yml"), Lang::Yaml);
        assert_eq!(Lang::from_extension("yaml"), Lang::Yaml);
    }

    #[test]
    fn detect_properties() {
        assert_eq!(Lang::from_extension("properties"), Lang::Properties);
    }

    #[test]
    fn detect_toml() {
        assert_eq!(Lang::from_extension("toml"), Lang::Toml);
    }

    #[test]
    fn detect_html() {
        assert_eq!(Lang::from_extension("html"), Lang::Html);
        assert_eq!(Lang::from_extension("htm"), Lang::Html);
        assert_eq!(Lang::from_path(Path::new("index.html")), Lang::Html);
    }

    #[test]
    fn detect_css() {
        assert_eq!(Lang::from_extension("css"), Lang::Css);
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
    }

    #[test]
    fn ts_language_none_for_unknown() {
        assert!(Lang::Unknown.ts_language().is_none());
    }

    #[test]
    fn names() {
        assert_eq!(Lang::Rust.name(), "Rust");
        assert_eq!(Lang::Unknown.name(), "");
    }

    #[test]
    fn comment_prefix_supported_languages() {
        assert_eq!(Lang::Rust.comment_prefix(), Some("// "));
        assert_eq!(Lang::JavaScript.comment_prefix(), Some("// "));
        assert_eq!(Lang::Python.comment_prefix(), Some("# "));
    }

    #[test]
    fn comment_prefix_unsupported_languages() {
        assert_eq!(Lang::Json.comment_prefix(), None);
        assert_eq!(Lang::Unknown.comment_prefix(), None);
    }
}
