//! Code formatting: indent config, per-language defaults, and external
//! formatter invocation.
//!
//! Two concerns live here:
//!
//! * **Live indent rules** (`IndentConfig`, `default_indent`) — drive the
//!   Tab key, Shift+Tab, auto-indent on Enter, and auto-dedent on `}`/`)`/`]`.
//! * **Whole-buffer formatting** (`FormatterConfig`, `default_formatter`,
//!   `run_formatter`) — shells out to an external tool such as `rustfmt`,
//!   `black`, or `prettier`, replacing the buffer atomically.
//!
//! Resolution precedence is project > global > built-in default. The
//! [`FormattingResolver`] type encapsulates the merge.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::syntax::language::Lang;

pub mod project;

// ── Indent style and config ───────────────────────────────────────────────

/// Tabs versus spaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndentStyle {
    #[default]
    Spaces,
    Tabs,
}

/// Resolved indentation rule for a single language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentConfig {
    pub style: IndentStyle,
    pub width: usize,
}

impl IndentConfig {
    /// Materialise the string for one indent level.
    pub fn one_level(&self) -> String {
        match self.style {
            IndentStyle::Tabs => "\t".to_string(),
            IndentStyle::Spaces => " ".repeat(self.width.max(1)),
        }
    }
}

// ── Per-language overrides (config schema) ────────────────────────────────

/// Per-language indent override; both fields are optional so you can override
/// just the style or just the width.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerLangIndent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<IndentStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
}

/// `[indent]` section: global defaults plus per-language overrides under
/// `[indent.languages.<lang>]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndentSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<IndentStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub languages: std::collections::BTreeMap<String, PerLangIndent>,
}

// ── Formatter config ──────────────────────────────────────────────────────

/// External formatter invocation: command, args (with `{path}` substitution),
/// and whether to pipe the buffer text on stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatterConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_stdin")]
    pub stdin: bool,
}

fn default_stdin() -> bool {
    true
}

/// Top-level formatting config — embedded as `Config::formatting`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormattingConfig {
    #[serde(default)]
    pub indent: IndentSection,
    #[serde(default)]
    pub formatters: std::collections::BTreeMap<String, FormatterConfig>,
}

// ── Language indent behaviour ─────────────────────────────────────────────

/// Static rules driving auto-indent on Enter and auto-dedent on close
/// brackets. Independent of the global indent style — that controls *what*
/// the indent looks like, this controls *when* to add or remove one.
#[derive(Debug, Clone, Copy)]
pub struct IndentRules {
    /// Trailing chars on the previous line that bump the new line's indent.
    pub increase_after: &'static [char],
    /// Chars that, when typed on an otherwise-whitespace line, dedent that
    /// line by one level before being inserted.
    pub decrease_on: &'static [char],
}

impl IndentRules {
    /// Indent rules for `lang`. Defaults to the C-family braces rule for
    /// languages without a special case.
    pub fn for_lang(lang: Lang) -> Self {
        match lang {
            Lang::Python => Self {
                increase_after: &[':'],
                // Python uses literal collections too, so `}` `)` `]` still
                // align — but they don't bump indent on Enter.
                decrease_on: &[')', ']', '}'],
            },
            Lang::Yaml | Lang::Toml | Lang::Properties | Lang::Markdown => Self {
                increase_after: &[],
                decrease_on: &[],
            },
            _ => Self {
                increase_after: &['{', '(', '['],
                decrease_on: &[')', ']', '}'],
            },
        }
    }
}

// ── Built-in defaults ─────────────────────────────────────────────────────

/// Built-in indent defaults per language. Used when neither the global config
/// nor the project config specifies a value.
pub fn default_indent(lang: Lang) -> IndentConfig {
    match lang {
        Lang::JavaScript
        | Lang::TypeScript
        | Lang::Tsx
        | Lang::Json
        | Lang::Yaml
        | Lang::Css
        | Lang::Html => IndentConfig {
            style: IndentStyle::Spaces,
            width: 2,
        },
        _ => IndentConfig {
            style: IndentStyle::Spaces,
            width: 4,
        },
    }
}

/// Built-in default external formatter per language.
///
/// Returns `None` for languages where no formatter is bundled. Adding new
/// languages: append a match arm here and remove the `TODO` comment below.
pub fn default_formatter(lang: Lang) -> Option<FormatterConfig> {
    match lang {
        Lang::Rust => Some(FormatterConfig {
            command: "rustfmt".into(),
            args: vec![],
            stdin: true,
        }),
        Lang::Python => Some(FormatterConfig {
            command: "black".into(),
            args: vec!["-".into(), "--quiet".into()],
            stdin: true,
        }),
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => Some(FormatterConfig {
            command: "prettier".into(),
            args: vec!["--stdin-filepath".into(), "{path}".into()],
            stdin: true,
        }),
        // TODO: add default formatter for Json (e.g. prettier --parser json)
        // TODO: add default formatter for Go (gofmt)
        // TODO: add default formatter for Java
        // TODO: add default formatter for CSharp (dotnet format)
        // TODO: add default formatter for Kotlin (ktlint)
        // TODO: add default formatter for Groovy
        // TODO: add default formatter for Yaml / Toml / Properties
        // TODO: add default formatter for Html / Css (prettier)
        // TODO: add default formatter for Markdown / Sh (shfmt)
        _ => None,
    }
}

// ── Resolver ──────────────────────────────────────────────────────────────

/// Merges project-level overrides over the global config to produce a
/// concrete [`IndentConfig`] or [`FormatterConfig`] for a given language.
pub struct FormattingResolver<'a> {
    pub global: &'a FormattingConfig,
    pub project: Option<&'a FormattingConfig>,
    /// Legacy `Config::tab_size`, used as the global indent width when no
    /// `[indent].width` is set.
    pub legacy_tab_size: usize,
}

impl FormattingResolver<'_> {
    /// Resolved indent config for `lang`. Falls back through:
    /// project per-lang → project global → global per-lang → global global
    /// → legacy `tab_size` (width only) → built-in default.
    pub fn indent(&self, lang: Lang) -> IndentConfig {
        let key = lang.config_key();
        let mut style: Option<IndentStyle> = None;
        let mut width: Option<usize> = None;

        if let Some(proj) = self.project {
            if let Some(p) = proj.indent.languages.get(key) {
                style = style.or(p.style);
                width = width.or(p.width);
            }
            style = style.or(proj.indent.style);
            width = width.or(proj.indent.width);
        }

        if let Some(p) = self.global.indent.languages.get(key) {
            style = style.or(p.style);
            width = width.or(p.width);
        }
        style = style.or(self.global.indent.style);
        width = width.or(self.global.indent.width);

        // Fall back to legacy `tab_size` for width when nothing else specified
        // a width — this preserves behaviour for users who only have the old
        // `tab_size` setting and no `[indent]` section.
        let default = default_indent(lang);
        IndentConfig {
            style: style.unwrap_or(default.style),
            width: width.unwrap_or(if self.legacy_tab_size > 0 {
                self.legacy_tab_size
            } else {
                default.width
            }),
        }
    }

    /// Resolved formatter for `lang`, or `None` if no formatter is configured
    /// (and no built-in default exists). Project formatters override global,
    /// and global overrides built-in defaults.
    pub fn formatter(&self, lang: Lang) -> Option<FormatterConfig> {
        let key = lang.config_key();
        if !key.is_empty() {
            if let Some(proj) = self.project
                && let Some(fc) = proj.formatters.get(key)
            {
                return Some(fc.clone());
            }
            if let Some(fc) = self.global.formatters.get(key) {
                return Some(fc.clone());
            }
        }
        default_formatter(lang)
    }
}

// ── External formatter invocation ─────────────────────────────────────────

/// Errors from running an external formatter.
#[derive(Debug)]
pub enum FormatError {
    /// `Command::spawn` failed (binary missing, permission denied, etc.).
    Spawn(String),
    /// Formatter exited non-zero. `stderr` is captured for display.
    NonZero { stderr: String },
    /// I/O error reading from / writing to the child process.
    Io(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(s) => write!(f, "{s}"),
            Self::NonZero { stderr } => {
                let first = stderr.lines().next().unwrap_or("(no output)");
                write!(f, "{first}")
            }
            Self::Io(s) => write!(f, "{s}"),
        }
    }
}

/// Substitute `{path}` placeholders in `args` with the buffer's path.
/// Missing path → empty string.
fn substitute_path(args: &[String], path: Option<&Path>) -> Vec<String> {
    let p = path.map(|p| p.display().to_string()).unwrap_or_default();
    args.iter().map(|a| a.replace("{path}", &p)).collect()
}

/// Run the configured formatter on `input`, returning the formatted text.
///
/// When `fc.stdin` is true, the buffer is written to the child's stdin from a
/// dedicated writer thread (so large buffers don't deadlock against a small
/// pipe buffer). Stdout is captured in the main thread via `wait_with_output`.
///
/// On non-zero exit, the captured stderr is returned in [`FormatError::NonZero`].
pub fn run_formatter(
    fc: &FormatterConfig,
    input: &str,
    path: Option<&Path>,
) -> Result<String, FormatError> {
    let args = substitute_path(&fc.args, path);
    let mut cmd = Command::new(&fc.command);
    cmd.args(&args);
    if fc.stdin {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| FormatError::Spawn(format!("{}: {e}", fc.command)))?;

    let writer_handle = if fc.stdin {
        child.stdin.take().map(|mut stdin| {
            let input = input.to_string();
            thread::spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
                // stdin dropped here, signalling EOF to the child.
            })
        })
    } else {
        None
    };

    let out = child
        .wait_with_output()
        .map_err(|e| FormatError::Io(e.to_string()))?;
    if let Some(h) = writer_handle {
        let _ = h.join();
    }

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(FormatError::NonZero { stderr });
    }
    String::from_utf8(out.stdout).map_err(|e| FormatError::Io(e.to_string()))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_indent_javascript_is_two() {
        assert_eq!(default_indent(Lang::JavaScript).width, 2);
        assert_eq!(default_indent(Lang::TypeScript).width, 2);
        assert_eq!(default_indent(Lang::Tsx).width, 2);
    }

    #[test]
    fn default_indent_rust_is_four_spaces() {
        let i = default_indent(Lang::Rust);
        assert_eq!(i.width, 4);
        assert_eq!(i.style, IndentStyle::Spaces);
    }

    #[test]
    fn one_level_tabs_inserts_tab() {
        let i = IndentConfig {
            style: IndentStyle::Tabs,
            width: 4,
        };
        assert_eq!(i.one_level(), "\t");
    }

    #[test]
    fn one_level_spaces_repeats_width() {
        let i = IndentConfig {
            style: IndentStyle::Spaces,
            width: 3,
        };
        assert_eq!(i.one_level(), "   ");
    }

    #[test]
    fn default_formatter_unknown_returns_none() {
        assert!(default_formatter(Lang::Unknown).is_none());
        assert!(default_formatter(Lang::Yaml).is_none());
    }

    #[test]
    fn default_formatter_rust_is_rustfmt() {
        let f = default_formatter(Lang::Rust).unwrap();
        assert_eq!(f.command, "rustfmt");
        assert!(f.stdin);
    }

    #[test]
    fn default_formatter_javascript_uses_prettier_with_path_placeholder() {
        let f = default_formatter(Lang::JavaScript).unwrap();
        assert_eq!(f.command, "prettier");
        assert!(f.args.iter().any(|a| a == "{path}"));
    }

    #[test]
    fn substitute_path_replaces_placeholder() {
        let args = vec!["--stdin-filepath".into(), "{path}".into()];
        let path = PathBuf::from("/tmp/foo.js");
        let out = substitute_path(&args, Some(&path));
        assert_eq!(out, vec!["--stdin-filepath", "/tmp/foo.js"]);
    }

    #[test]
    fn substitute_path_empty_when_missing() {
        let args = vec!["{path}".into()];
        let out = substitute_path(&args, None);
        assert_eq!(out, vec![""]);
    }

    fn empty_global() -> FormattingConfig {
        FormattingConfig::default()
    }

    #[test]
    fn resolver_returns_built_in_default_when_unconfigured() {
        // legacy_tab_size = 0 means the legacy field is absent — fall back to
        // the per-language built-in defaults.
        let g = empty_global();
        let r = FormattingResolver {
            global: &g,
            project: None,
            legacy_tab_size: 0,
        };
        assert_eq!(r.indent(Lang::Rust).width, 4);
        assert_eq!(r.indent(Lang::JavaScript).width, 2);
    }

    #[test]
    fn resolver_legacy_tab_size_acts_as_global_width() {
        // A user with only `tab_size = 4` (no `[indent]` section) gets that
        // value for every language, including JS whose built-in default
        // would otherwise be 2.
        let g = empty_global();
        let r = FormattingResolver {
            global: &g,
            project: None,
            legacy_tab_size: 4,
        };
        assert_eq!(r.indent(Lang::Rust).width, 4);
        assert_eq!(r.indent(Lang::JavaScript).width, 4);
    }

    #[test]
    fn resolver_uses_legacy_tab_size_for_width_when_no_indent_section() {
        let g = empty_global();
        // User only set tab_size = 8 in the legacy field.
        let r = FormattingResolver {
            global: &g,
            project: None,
            legacy_tab_size: 8,
        };
        // Built-in default for Rust is 4, but legacy tab_size overrides.
        assert_eq!(r.indent(Lang::Rust).width, 8);
        // Same for JS (would be 2 by default).
        assert_eq!(r.indent(Lang::JavaScript).width, 8);
    }

    #[test]
    fn resolver_per_language_overrides_global() {
        let mut g = empty_global();
        g.indent.width = Some(8);
        g.indent.style = Some(IndentStyle::Tabs);
        g.indent.languages.insert(
            "rust".into(),
            PerLangIndent {
                style: Some(IndentStyle::Spaces),
                width: Some(4),
            },
        );
        let r = FormattingResolver {
            global: &g,
            project: None,
            legacy_tab_size: 4,
        };
        let i = r.indent(Lang::Rust);
        assert_eq!(i.style, IndentStyle::Spaces);
        assert_eq!(i.width, 4);

        let py = r.indent(Lang::Python);
        assert_eq!(py.style, IndentStyle::Tabs);
        assert_eq!(py.width, 8);
    }

    #[test]
    fn resolver_project_overrides_global() {
        let mut g = empty_global();
        g.indent.languages.insert(
            "rust".into(),
            PerLangIndent {
                style: Some(IndentStyle::Spaces),
                width: Some(4),
            },
        );
        let mut p = empty_global();
        p.indent.languages.insert(
            "rust".into(),
            PerLangIndent {
                style: Some(IndentStyle::Tabs),
                width: Some(2),
            },
        );
        let r = FormattingResolver {
            global: &g,
            project: Some(&p),
            legacy_tab_size: 4,
        };
        let i = r.indent(Lang::Rust);
        assert_eq!(i.style, IndentStyle::Tabs);
        assert_eq!(i.width, 2);
    }

    #[test]
    fn resolver_formatter_project_overrides_global() {
        let mut g = empty_global();
        g.formatters.insert(
            "rust".into(),
            FormatterConfig {
                command: "rustfmt".into(),
                args: vec![],
                stdin: true,
            },
        );
        let mut p = empty_global();
        p.formatters.insert(
            "rust".into(),
            FormatterConfig {
                command: "rustfmt-nightly".into(),
                args: vec!["--edition".into(), "2024".into()],
                stdin: true,
            },
        );
        let r = FormattingResolver {
            global: &g,
            project: Some(&p),
            legacy_tab_size: 4,
        };
        let f = r.formatter(Lang::Rust).unwrap();
        assert_eq!(f.command, "rustfmt-nightly");
    }

    #[test]
    fn resolver_formatter_falls_back_to_built_in() {
        let g = empty_global();
        let r = FormattingResolver {
            global: &g,
            project: None,
            legacy_tab_size: 4,
        };
        assert!(r.formatter(Lang::Rust).is_some());
        assert!(r.formatter(Lang::Unknown).is_none());
    }

    // ── run_formatter end-to-end (uses tiny shell helpers) ────────────────

    #[test]
    fn run_formatter_passes_stdin_through_cat() {
        // `cat` echoes stdin → stdout; perfect stub formatter.
        let fc = FormatterConfig {
            command: "cat".into(),
            args: vec![],
            stdin: true,
        };
        let out = run_formatter(&fc, "hello\nworld\n", None).unwrap();
        assert_eq!(out, "hello\nworld\n");
    }

    #[test]
    fn run_formatter_substitutes_path_placeholder() {
        // `sh -c 'echo "$1"' --` echoes the second argument.
        let fc = FormatterConfig {
            command: "sh".into(),
            args: vec![
                "-c".into(),
                "echo \"$1\"".into(),
                "--".into(),
                "{path}".into(),
            ],
            stdin: false,
        };
        let path = PathBuf::from("/tmp/foo.js");
        let out = run_formatter(&fc, "", Some(&path)).unwrap();
        assert_eq!(out.trim_end(), "/tmp/foo.js");
    }

    #[test]
    fn run_formatter_nonzero_exit_returns_error_with_stderr() {
        let fc = FormatterConfig {
            command: "sh".into(),
            args: vec!["-c".into(), "echo 'bad input' >&2; exit 7".into()],
            stdin: false,
        };
        match run_formatter(&fc, "", None) {
            Err(FormatError::NonZero { stderr }) => {
                assert!(stderr.contains("bad input"));
            }
            other => panic!("expected NonZero error, got {other:?}"),
        }
    }

    #[test]
    fn run_formatter_missing_binary_returns_spawn_error() {
        let fc = FormatterConfig {
            command: "this-binary-does-not-exist-xyzzy".into(),
            args: vec![],
            stdin: false,
        };
        match run_formatter(&fc, "", None) {
            Err(FormatError::Spawn(_)) => {}
            other => panic!("expected Spawn error, got {other:?}"),
        }
    }
}
