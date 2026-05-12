//! TextMate-style snippet engine.
//!
//! Snippets live in `<config_dir>/txt/snippets/<language_id>.toml` and are
//! parsed lazily on first use per language. A snippet body uses the standard
//! placeholder syntax: `$1`, `$2`, `${1:default text}`, with `$0` as the
//! cursor's final position. Backslash escapes the special characters.
//!
//! The runtime is split into two halves:
//!
//! * **Parsing & loading** (this module) — produces a [`ParsedBody`] of
//!   literal segments and tab-stop placeholders.
//! * **Active session** ([`session`]) — installs the expansion in the buffer,
//!   tracks tab-stop byte ranges, and advances them as the user edits.

pub mod session;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One parsed snippet definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub prefix: String,
    pub body: String,
    /// Optional human-readable description shown in the picker.
    #[serde(default)]
    pub description: String,
}

/// One segment of a parsed body — either literal text or a tab-stop
/// placeholder with an optional default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Stop { index: u32, default: String },
}

/// A snippet body broken into segments ready for expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBody {
    pub segments: Vec<Segment>,
}

impl Snippet {
    /// Parse `self.body` into segments. Pure function — call once per
    /// expansion and pair the result with a `SnippetSession`.
    pub fn parse_body(&self) -> ParsedBody {
        parse_body(&self.body)
    }
}

/// Parse a TextMate-style snippet body. Recognises:
///
/// * `$1`, `$2`, ... — numbered tab stops
/// * `${1:default}` — tab stop with a default placeholder string
/// * `$0` — final-cursor stop
/// * Backslash-escaped `\$`, `\\`, `\{`, `\}`
///
/// Unrecognised `${...}` forms degrade to literal text so a typo in a
/// snippet doesn't make the editor swallow user content.
pub fn parse_body(body: &str) -> ParsedBody {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut chars = body.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    literal.push(next);
                } else {
                    literal.push('\\');
                }
            }
            '$' => {
                if let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() {
                        flush_literal(&mut segments, &mut literal);
                        let mut digits = String::new();
                        while let Some(&n) = chars.peek() {
                            if n.is_ascii_digit() {
                                digits.push(n);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Ok(idx) = digits.parse::<u32>() {
                            segments.push(Segment::Stop {
                                index: idx,
                                default: String::new(),
                            });
                            continue;
                        }
                        literal.push('$');
                        literal.push_str(&digits);
                        continue;
                    }
                    if next == '{' {
                        chars.next();
                        let mut head = String::new();
                        while let Some(&n) = chars.peek() {
                            if n.is_ascii_digit() {
                                head.push(n);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        let idx_res: Result<u32, _> = head.parse::<u32>().map_err(|_| ());
                        let mut default = String::new();
                        let mut closed = false;
                        let saw_colon = chars.peek() == Some(&':');
                        if saw_colon {
                            chars.next(); // consume ':'
                            while let Some(c) = chars.next() {
                                if c == '}' {
                                    closed = true;
                                    break;
                                }
                                if c == '\\' {
                                    if let Some(esc) = chars.next() {
                                        default.push(esc);
                                    }
                                } else {
                                    default.push(c);
                                }
                            }
                        } else if chars.peek() == Some(&'}') {
                            chars.next();
                            closed = true;
                        }
                        if let (Ok(idx), true) = (idx_res, closed) {
                            flush_literal(&mut segments, &mut literal);
                            segments.push(Segment::Stop {
                                index: idx,
                                default,
                            });
                            continue;
                        }
                        // Malformed placeholder — emit as literal so user text
                        // is preserved.
                        literal.push('$');
                        literal.push('{');
                        if let Ok(idx) = idx_res {
                            literal.push_str(&idx.to_string());
                        }
                        if saw_colon {
                            literal.push(':');
                        }
                        literal.push_str(&default);
                        if closed {
                            literal.push('}');
                        }
                        continue;
                    }
                }
                literal.push('$');
            }
            _ => literal.push(c),
        }
    }
    flush_literal(&mut segments, &mut literal);
    ParsedBody { segments }
}

fn flush_literal(segments: &mut Vec<Segment>, literal: &mut String) {
    if !literal.is_empty() {
        segments.push(Segment::Literal(std::mem::take(literal)));
    }
}

/// Top-level TOML file for a language: `[snippets.<name>]` tables.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SnippetFile {
    #[serde(default)]
    pub snippets: HashMap<String, Snippet>,
}

/// In-memory snippet store keyed by language id (`"rust"`, `"python"`, …).
#[derive(Default)]
pub struct SnippetStore {
    by_language: HashMap<String, Vec<Snippet>>,
    /// Languages whose file has already been loaded (success or failure)
    /// so we don't hit disk repeatedly on a missing file.
    loaded: std::collections::HashSet<String>,
}

impl SnippetStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up every snippet whose prefix matches `prefix` exactly for the
    /// given `language_id`. Loads the language's snippets file on first call.
    pub fn lookup(&mut self, language_id: &str, prefix: &str) -> Vec<Snippet> {
        self.ensure_loaded(language_id);
        self.by_language
            .get(language_id)
            .map(|v| v.iter().filter(|s| s.prefix == prefix).cloned().collect())
            .unwrap_or_default()
    }

    /// Return every loaded snippet for `language_id` whose prefix *starts with*
    /// `query`. Useful for picker UIs; loads the file on first call.
    #[allow(dead_code)]
    pub fn prefix_matches(&mut self, language_id: &str, query: &str) -> Vec<Snippet> {
        self.ensure_loaded(language_id);
        self.by_language
            .get(language_id)
            .map(|v| {
                v.iter()
                    .filter(|s| s.prefix.starts_with(query))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn ensure_loaded(&mut self, language_id: &str) {
        if self.loaded.contains(language_id) {
            return;
        }
        self.loaded.insert(language_id.to_string());
        let Some(path) = snippet_path(language_id) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(parsed) = toml::from_str::<SnippetFile>(&text) else {
            return;
        };
        let entries: Vec<Snippet> = parsed.snippets.into_values().collect();
        self.by_language.insert(language_id.to_string(), entries);
    }

    /// Replace the snippets for a language. Used by tests; production code
    /// goes through `ensure_loaded` from disk.
    #[allow(dead_code)]
    pub fn insert(&mut self, language_id: &str, snippets: Vec<Snippet>) {
        self.loaded.insert(language_id.to_string());
        self.by_language.insert(language_id.to_string(), snippets);
    }
}

/// Path to the on-disk snippets file for `language_id`, or `None` if the
/// platform doesn't expose a config directory.
fn snippet_path(language_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".config")
            .join("txt")
            .join("snippets")
            .join(format!("{language_id}.toml")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_numbered_stops() {
        let body = parse_body("for $1 in $2 { $0 }");
        assert_eq!(
            body.segments,
            vec![
                Segment::Literal("for ".into()),
                Segment::Stop {
                    index: 1,
                    default: String::new()
                },
                Segment::Literal(" in ".into()),
                Segment::Stop {
                    index: 2,
                    default: String::new()
                },
                Segment::Literal(" { ".into()),
                Segment::Stop {
                    index: 0,
                    default: String::new()
                },
                Segment::Literal(" }".into()),
            ]
        );
    }

    #[test]
    fn parses_default_placeholder() {
        let body = parse_body("for ${1:i} in ${2:iter}");
        assert_eq!(
            body.segments,
            vec![
                Segment::Literal("for ".into()),
                Segment::Stop {
                    index: 1,
                    default: "i".into()
                },
                Segment::Literal(" in ".into()),
                Segment::Stop {
                    index: 2,
                    default: "iter".into()
                },
            ]
        );
    }

    #[test]
    fn parses_escaped_dollar_sign() {
        let body = parse_body(r"\$1 is not a stop");
        assert_eq!(
            body.segments,
            vec![Segment::Literal("$1 is not a stop".into())]
        );
    }

    #[test]
    fn malformed_placeholder_falls_back_to_literal() {
        let body = parse_body("${not_a_number}");
        assert!(
            body.segments
                .iter()
                .any(|s| matches!(s, Segment::Literal(_)))
        );
        assert!(
            !body
                .segments
                .iter()
                .any(|s| matches!(s, Segment::Stop { .. }))
        );
    }

    #[test]
    fn store_lookup_returns_matching_prefix() {
        let mut store = SnippetStore::new();
        store.insert(
            "rust",
            vec![Snippet {
                prefix: "for".into(),
                body: "for $1 in $2 {\n    $0\n}".into(),
                description: String::new(),
            }],
        );
        let matches = store.lookup("rust", "for");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].prefix, "for");
    }

    #[test]
    fn store_prefix_matches_for_picker() {
        let mut store = SnippetStore::new();
        store.insert(
            "rust",
            vec![
                Snippet {
                    prefix: "fn".into(),
                    body: "fn $1() {}".into(),
                    description: String::new(),
                },
                Snippet {
                    prefix: "for".into(),
                    body: "for $1 in $2 {}".into(),
                    description: String::new(),
                },
            ],
        );
        let m = store.prefix_matches("rust", "f");
        assert_eq!(m.len(), 2);
        let m = store.prefix_matches("rust", "fn");
        assert_eq!(m.len(), 1);
    }
}
