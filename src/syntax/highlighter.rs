use ratatui::style::{Modifier, Style};
use tree_sitter::{Node, Tree};

use crate::theme::ThemeColors;

use crate::syntax::language::Lang;

/// Coarse semantic categories for syntax coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
    Attribute,
    Punctuation,
}

/// A highlighted byte range within the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
    pub kind: HighlightKind,
}

/// Collect highlight spans for the visible byte range `[start_byte, end_byte)`.
///
/// Returns spans sorted by `start`. Spans never overlap — atomic nodes (strings,
/// comments) prevent recursion into their children, so children can't produce
/// conflicting spans.
pub fn highlight(
    tree: &Tree,
    source: &[u8],
    lang: Lang,
    start_byte: usize,
    end_byte: usize,
) -> Vec<HighlightSpan> {
    if lang == Lang::Unknown || start_byte >= end_byte {
        return Vec::new();
    }
    let mut spans = Vec::new();
    visit(
        tree.root_node(),
        lang,
        source,
        start_byte,
        end_byte,
        &mut spans,
    );
    spans
}

/// Convert a `HighlightKind` to a ratatui `Style` using the active theme colors.
pub fn style_for_kind(kind: HighlightKind, theme: &ThemeColors) -> Style {
    match kind {
        HighlightKind::Keyword => Style::default().fg(theme.syn_keyword),
        HighlightKind::String => Style::default().fg(theme.syn_string),
        HighlightKind::Comment => Style::default()
            .fg(theme.syn_comment)
            .add_modifier(Modifier::ITALIC),
        HighlightKind::Number => Style::default().fg(theme.syn_number),
        HighlightKind::Type => Style::default().fg(theme.syn_type),
        HighlightKind::Function => Style::default().fg(theme.syn_function),
        HighlightKind::Attribute => Style::default().fg(theme.syn_attribute),
        HighlightKind::Punctuation => Style::default().fg(theme.syn_punctuation),
    }
}

/// Map an LSP semantic token type index to a `HighlightKind`.
///
/// Token type indices follow the order declared in `ClientCapabilities`:
///   0=namespace, 1=type, 2=class, 3=enum, 4=interface, 5=struct,
///   6=typeParameter, 7=parameter, 8=variable, 9=property,
///   10=enumMember, 11=event, 12=function, 13=method, 14=macro,
///   15=keyword, 16=modifier, 17=comment, 18=string, 19=number,
///   20=regexp, 21=operator, 22=decorator
pub fn semantic_token_to_kind(token_type: u32) -> Option<HighlightKind> {
    match token_type {
        0 => Some(HighlightKind::Type),           // namespace
        1..=6 => Some(HighlightKind::Type), // type, class, enum, interface, struct, typeParameter
        7..=10 => None,                     // parameter, variable, property, enumMember — plain
        11 => None,                         // event
        12 | 13 => Some(HighlightKind::Function), // function, method
        14 => Some(HighlightKind::Attribute), // macro
        15 | 16 => Some(HighlightKind::Keyword), // keyword, modifier
        17 => Some(HighlightKind::Comment), // comment
        18 => Some(HighlightKind::String),  // string
        19 => Some(HighlightKind::Number),  // number
        20 => Some(HighlightKind::String),  // regexp
        21 => Some(HighlightKind::Punctuation), // operator
        22 => Some(HighlightKind::Attribute), // decorator
        _ => None,
    }
}

/// Convert a slice of `SemanticTokenSpan`s to `HighlightSpan`s for a visible
/// byte range. Filters and maps only tokens that overlap `[start_byte, end_byte)`.
pub fn semantic_tokens_to_highlights(
    tokens: &[crate::lsp::types::SemanticTokenSpan],
    start_byte: usize,
    end_byte: usize,
) -> Vec<HighlightSpan> {
    tokens
        .iter()
        .filter(|t| t.end_byte > start_byte && t.start_byte < end_byte)
        .filter_map(|t| {
            semantic_token_to_kind(t.token_type).map(|kind| HighlightSpan {
                start: t.start_byte,
                end: t.end_byte,
                kind,
            })
        })
        .collect()
}

// ── Tree walker ───────────────────────────────────────────────────────────────

#[allow(clippy::only_used_in_recursion)]
fn visit(
    node: Node<'_>,
    lang: Lang,
    source: &[u8],
    start_byte: usize,
    end_byte: usize,
    spans: &mut Vec<HighlightSpan>,
) {
    // Prune: skip subtrees entirely outside the visible range.
    if node.end_byte() <= start_byte || node.start_byte() >= end_byte {
        return;
    }

    let kind = node.kind();

    // Atomic nodes: emit a span for the whole node and do NOT recurse.
    if let Some(hk) = atomic_kind(kind, lang) {
        let s = node.start_byte().max(start_byte);
        let e = node.end_byte().min(end_byte);
        if s < e {
            spans.push(HighlightSpan {
                start: s,
                end: e,
                kind: hk,
            });
        }
        return;
    }

    // Leaf nodes: match by kind (keywords, numbers, operators, etc.)
    if node.child_count() == 0 {
        let parent_kind = node.parent().map(|p| p.kind()).unwrap_or("");
        if let Some(hk) = leaf_kind(kind, parent_kind, lang) {
            let s = node.start_byte().max(start_byte);
            let e = node.end_byte().min(end_byte);
            if s < e {
                spans.push(HighlightSpan {
                    start: s,
                    end: e,
                    kind: hk,
                });
            }
        }
        return;
    }

    // Structural node: recurse into children.
    // Pass the current node's kind as context for children that need it
    // (e.g., identifiers inside function declarations).
    let ctx = kind;
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            // Special-case: identifier whose parent context implies Function.
            if child.kind() == "identifier" && is_function_context(ctx, lang) {
                // Only the first named identifier child is the function name.
                // Check field_name to be sure.
                let field = node.field_name_for_child(i as u32);
                if matches!(field, Some("name")) {
                    let s = child.start_byte().max(start_byte);
                    let e = child.end_byte().min(end_byte);
                    if s < e && child.end_byte() > start_byte && child.start_byte() < end_byte {
                        spans.push(HighlightSpan {
                            start: s,
                            end: e,
                            kind: HighlightKind::Function,
                        });
                    }
                    continue;
                }
            }
            visit(child, lang, source, start_byte, end_byte, spans);
        }
    }
}

/// Returns `Some(HighlightKind)` if `node_kind` should be highlighted as an
/// *atomic unit* (no recursion into children).
fn atomic_kind(node_kind: &str, lang: Lang) -> Option<HighlightKind> {
    match lang {
        Lang::Rust => match node_kind {
            "string_literal" | "raw_string_literal" | "char_literal" => Some(HighlightKind::String),
            "line_comment" | "block_comment" => Some(HighlightKind::Comment),
            "attribute_item" | "inner_attribute_item" => Some(HighlightKind::Attribute),
            _ => None,
        },
        Lang::Python => match node_kind {
            "string" | "concatenated_string" | "interpolated_string" => Some(HighlightKind::String),
            "comment" => Some(HighlightKind::Comment),
            "decorator" => Some(HighlightKind::Attribute),
            _ => None,
        },
        Lang::JavaScript => match node_kind {
            "string" | "template_string" | "template_literal" => Some(HighlightKind::String),
            "comment" => Some(HighlightKind::Comment),
            "regex" => Some(HighlightKind::String),
            _ => None,
        },
        Lang::Json => match node_kind {
            "string" => Some(HighlightKind::String),
            _ => None,
        },
        Lang::Unknown => None,
    }
}

/// Returns `Some(HighlightKind)` for a *leaf* node (no children).
fn leaf_kind(node_kind: &str, parent_kind: &str, lang: Lang) -> Option<HighlightKind> {
    match lang {
        Lang::Rust => rust_leaf(node_kind, parent_kind),
        Lang::Python => python_leaf(node_kind, parent_kind),
        Lang::JavaScript => js_leaf(node_kind, parent_kind),
        Lang::Json => json_leaf(node_kind),
        Lang::Unknown => None,
    }
}

fn rust_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        // Keywords
        "fn" | "let" | "pub" | "use" | "mod" | "struct" | "enum" | "impl" | "trait" | "type"
        | "const" | "static" | "where" | "for" | "if" | "else" | "match" | "loop" | "while"
        | "return" | "self" | "Self" | "super" | "crate" | "in" | "as" | "ref" | "dyn"
        | "unsafe" | "extern" | "async" | "await" | "move" | "continue" | "break" => {
            Some(HighlightKind::Keyword)
        }
        // `mut` appears as a `mutable_specifier` node in tree-sitter-rust
        "mut" | "mutable_specifier" => Some(HighlightKind::Keyword),
        "true" | "false" => Some(HighlightKind::Keyword),

        // Numbers
        "integer_literal" | "float_literal" => Some(HighlightKind::Number),

        // Types
        "type_identifier" => Some(HighlightKind::Type),
        "primitive_type" => Some(HighlightKind::Type),

        // Function call (identifier used as callee)
        "identifier" if matches!(parent, "call_expression") => Some(HighlightKind::Function),

        // Punctuation
        "{" | "}" | "(" | ")" | "[" | "]" | ";" | ":" | "::" | "," | "." | ".." | "..." => {
            Some(HighlightKind::Punctuation)
        }

        _ => None,
    }
}

fn python_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "def" | "class" | "if" | "elif" | "else" | "for" | "while" | "import" | "from"
        | "return" | "pass" | "lambda" | "with" | "as" | "in" | "not" | "and" | "or" | "is"
        | "try" | "except" | "finally" | "raise" | "yield" | "del" | "global" | "nonlocal"
        | "assert" | "async" | "await" | "break" | "continue" => Some(HighlightKind::Keyword),
        // tree-sitter-python uses lowercase node kinds for these literals
        "none" | "true" | "false" => Some(HighlightKind::Keyword),
        "integer" | "float" => Some(HighlightKind::Number),
        "type" => Some(HighlightKind::Type),
        "identifier" if parent == "call" => Some(HighlightKind::Function),
        _ => None,
    }
}

fn js_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "function" | "var" | "let" | "const" | "if" | "else" | "for" | "while" | "do"
        | "return" | "new" | "this" | "class" | "extends" | "import" | "export" | "from"
        | "default" | "switch" | "case" | "break" | "continue" | "throw" | "try" | "catch"
        | "finally" | "in" | "of" | "typeof" | "instanceof" | "void" | "delete" | "async"
        | "await" | "yield" | "static" | "get" | "set" | "debugger" => Some(HighlightKind::Keyword),
        "true" | "false" | "null" | "undefined" => Some(HighlightKind::Keyword),
        "number" => Some(HighlightKind::Number),
        "identifier" if matches!(parent, "call_expression" | "new_expression") => {
            Some(HighlightKind::Function)
        }
        _ => None,
    }
}

fn json_leaf(kind: &str) -> Option<HighlightKind> {
    match kind {
        "true" | "false" | "null" => Some(HighlightKind::Keyword),
        "number" => Some(HighlightKind::Number),
        "{" | "}" | "[" | "]" | ":" | "," => Some(HighlightKind::Punctuation),
        _ => None,
    }
}

/// True if a node of kind `ctx` is a context where the `name` child is a function name.
fn is_function_context(ctx: &str, lang: Lang) -> bool {
    match lang {
        Lang::Rust => matches!(
            ctx,
            "function_item" | "function_signature_item" | "method_signature"
        ),
        Lang::Python => matches!(ctx, "function_definition" | "decorated_definition"),
        Lang::JavaScript => matches!(
            ctx,
            "function_declaration" | "method_definition" | "function"
        ),
        _ => false,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust(source: &str) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn parse_python(source: &str) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn parse_json(source: &str) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_json::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn spans_for(source: &str, tree: &Tree, lang: Lang) -> Vec<HighlightSpan> {
        highlight(tree, source.as_bytes(), lang, 0, source.len())
    }

    fn has_span_of_kind(
        spans: &[HighlightSpan],
        start: usize,
        end: usize,
        kind: HighlightKind,
    ) -> bool {
        spans
            .iter()
            .any(|s| s.start == start && s.end == end && s.kind == kind)
    }

    // ── Rust ──────────────────────────────────────────────────────────────────

    #[test]
    fn rust_fn_keyword() {
        let src = "fn main() {}";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // "fn" is at bytes 0..2
        assert!(
            has_span_of_kind(&spans, 0, 2, HighlightKind::Keyword),
            "expected Keyword span at 0..2, got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_string_literal() {
        let src = r#"let x = "hello";"#;
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // "hello" (with quotes) is at bytes 8..15
        assert!(
            has_span_of_kind(&spans, 8, 15, HighlightKind::String),
            "expected String span, got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_line_comment() {
        let src = "// a comment\nfn foo() {}";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        assert!(
            has_span_of_kind(&spans, 0, 12, HighlightKind::Comment),
            "expected Comment span, got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_integer_literal() {
        let src = "let x = 42;";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // "42" at bytes 8..10
        assert!(
            has_span_of_kind(&spans, 8, 10, HighlightKind::Number),
            "expected Number span, got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_type_identifier() {
        let src = "let x: String = String::new();";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // "String" appears as type_identifier
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Type && &src[s.start..s.end] == "String"),
            "expected Type span for 'String', got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_function_name() {
        let src = "fn greet(name: &str) {}";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // "greet" should be highlighted as Function
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Function && &src[s.start..s.end] == "greet"),
            "expected Function span for 'greet', got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_keyword_let_mut() {
        let src = "let mut x = 0;";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "let")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "mut")
        );
    }

    #[test]
    fn rust_attribute() {
        let src = "#[derive(Debug)]\nstruct Foo;";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // attribute_item starts at 0
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Attribute && s.start == 0),
            "expected Attribute span, got: {:?}",
            spans
        );
    }

    #[test]
    fn rust_char_literal() {
        let src = "let c = 'a';";
        let tree = parse_rust(src);
        let spans = spans_for(src, &tree, Lang::Rust);
        // 'a' at bytes 8..11 (including single quotes)
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::String && &src[s.start..s.end] == "'a'"),
            "expected String span for char literal, got: {:?}",
            spans
        );
    }

    // ── Python ────────────────────────────────────────────────────────────────

    #[test]
    fn python_def_keyword() {
        let src = "def foo():\n    pass\n";
        let tree = parse_python(src);
        let spans = spans_for(src, &tree, Lang::Python);
        assert!(
            has_span_of_kind(&spans, 0, 3, HighlightKind::Keyword),
            "expected 'def' as Keyword, got: {:?}",
            spans
        );
    }

    #[test]
    fn python_comment() {
        let src = "# this is a comment\nx = 1\n";
        let tree = parse_python(src);
        let spans = spans_for(src, &tree, Lang::Python);
        assert!(
            has_span_of_kind(&spans, 0, 19, HighlightKind::Comment),
            "expected Comment span, got: {:?}",
            spans
        );
    }

    #[test]
    fn python_none_keyword() {
        let src = "x = None\n";
        let tree = parse_python(src);
        let spans = spans_for(src, &tree, Lang::Python);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "None"),
            "expected 'None' as Keyword, got: {:?}",
            spans
        );
    }

    // ── JSON ──────────────────────────────────────────────────────────────────

    #[test]
    fn json_string_key() {
        let src = r#"{"key": 1}"#;
        let tree = parse_json(src);
        let spans = spans_for(src, &tree, Lang::Json);
        // "key" (with quotes) at bytes 1..6
        assert!(
            has_span_of_kind(&spans, 1, 6, HighlightKind::String),
            "expected String span for JSON key, got: {:?}",
            spans
        );
    }

    #[test]
    fn json_number() {
        let src = r#"{"x": 42}"#;
        let tree = parse_json(src);
        let spans = spans_for(src, &tree, Lang::Json);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Number && &src[s.start..s.end] == "42"),
            "expected Number span for 42, got: {:?}",
            spans
        );
    }

    #[test]
    fn json_true_false_null() {
        let src = r#"{"a":true,"b":false,"c":null}"#;
        let tree = parse_json(src);
        let spans = spans_for(src, &tree, Lang::Json);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "true")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "false")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "null")
        );
    }

    // ── Filtering ─────────────────────────────────────────────────────────────

    #[test]
    fn visible_range_filter() {
        // Source: 3 lines. Request only line 1 (bytes 6..12 in "fn a;\nfn b;\nfn c;")
        let src = "fn a;\nfn b;\nfn c;";
        //          012345 6789A  BCDE
        let tree = parse_rust(src);
        let spans = highlight(&tree, src.as_bytes(), Lang::Rust, 6, 11);
        // Only spans within bytes 6..11 should be present
        assert!(
            spans.iter().all(|s| s.start >= 6 && s.end <= 11),
            "spans outside visible range returned: {:?}",
            spans
        );
        // The 'fn' at byte 6 should be present
        assert!(
            has_span_of_kind(&spans, 6, 8, HighlightKind::Keyword),
            "expected 'fn' at 6..8, got: {:?}",
            spans
        );
    }

    #[test]
    fn unknown_lang_returns_empty() {
        // Unknown language has no tree — but the API requires a &Tree.
        // Test the guard: if we had a tree but Lang::Unknown, still empty.
        // We create a dummy Rust tree and pass Lang::Unknown.
        let src = "fn main() {}";
        let tree = parse_rust(src);
        let spans = highlight(&tree, src.as_bytes(), Lang::Unknown, 0, src.len());
        assert!(spans.is_empty(), "expected empty spans for Unknown lang");
    }

    #[test]
    fn style_for_kind_produces_distinct_styles() {
        use ratatui::style::Color;
        let theme = crate::theme::ThemeColors::for_theme(&crate::config::Theme::Default);
        // Each kind should produce a non-default style.
        let kinds = [
            HighlightKind::Keyword,
            HighlightKind::String,
            HighlightKind::Comment,
            HighlightKind::Number,
            HighlightKind::Type,
            HighlightKind::Function,
            HighlightKind::Attribute,
            HighlightKind::Punctuation,
        ];
        let default_style = Style::default().fg(Color::White);
        for kind in kinds {
            let style = style_for_kind(kind, &theme);
            assert_ne!(
                style, default_style,
                "{:?} should not map to default White style",
                kind
            );
        }
    }
}
