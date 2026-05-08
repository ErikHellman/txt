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
    Heading,
    Link,
    Emphasis,
    CodeBlock,
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
        HighlightKind::Heading => Style::default().fg(theme.syn_heading),
        HighlightKind::Link => Style::default().fg(theme.syn_link),
        HighlightKind::Emphasis => Style::default()
            .fg(theme.syn_emphasis)
            .add_modifier(Modifier::ITALIC),
        HighlightKind::CodeBlock => Style::default().fg(theme.syn_codeblock),
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

    // Special-case for markdown: 'inline' nodes contain text with potential formatting.
    // Don't treat as leaf - always recurse.
    if lang == Lang::Markdown && kind == "inline" {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                visit(child, lang, source, start_byte, end_byte, spans);
            }
        }
        return;
    }

    // Special-case for markdown: fenced code blocks with embedded language highlighting
    if lang == Lang::Markdown && kind == "fenced_code_block" {
        handle_markdown_code_fence(node, source, start_byte, end_byte, spans);
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
        Lang::Sh => match node_kind {
            "string" | "raw_string" | "ansi_c_string" => Some(HighlightKind::String),
            "heredoc_body" | "heredoc_start" | "heredoc_end" => Some(HighlightKind::String),
            "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::TypeScript | Lang::Tsx => match node_kind {
            "string" | "template_string" | "template_literal" => Some(HighlightKind::String),
            "comment" => Some(HighlightKind::Comment),
            "regex" => Some(HighlightKind::String),
            _ => None,
        },
        Lang::CSharp => match node_kind {
            "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
            | "character_literal" => Some(HighlightKind::String),
            "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Java => match node_kind {
            "string_literal" | "character_literal" => Some(HighlightKind::String),
            "line_comment" | "block_comment" => Some(HighlightKind::Comment),
            "marker_annotation" | "annotation" => Some(HighlightKind::Attribute),
            _ => None,
        },
        Lang::Go => match node_kind {
            "interpreted_string_literal" | "raw_string_literal" | "rune_literal" => {
                Some(HighlightKind::String)
            }
            "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Kotlin => match node_kind {
            "line_string_literal"
            | "multi_line_string_literal"
            | "string_literal"
            | "character_literal" => Some(HighlightKind::String),
            "line_comment" | "block_comment" | "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Groovy => match node_kind {
            "string_literal"
            | "gstring"
            | "gstring_literal"
            | "slashy_string"
            | "dollar_slashy_string" => Some(HighlightKind::String),
            "line_comment" | "block_comment" | "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Yaml => match node_kind {
            "comment" => Some(HighlightKind::Comment),
            "double_quote_scalar" | "single_quote_scalar" | "block_scalar" => {
                Some(HighlightKind::String)
            }
            _ => None,
        },
        Lang::Properties => match node_kind {
            "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Toml => match node_kind {
            "string" | "literal_string" | "multiline_string" | "multiline_literal_string" => {
                Some(HighlightKind::String)
            }
            "comment" => Some(HighlightKind::Comment),
            _ => None,
        },
        Lang::Markdown => None,
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
        Lang::Markdown => markdown_leaf(node_kind, parent_kind),
        Lang::Sh => sh_leaf(node_kind, parent_kind),
        Lang::TypeScript | Lang::Tsx => ts_leaf(node_kind, parent_kind),
        Lang::CSharp => csharp_leaf(node_kind, parent_kind),
        Lang::Java => java_leaf(node_kind, parent_kind),
        Lang::Go => go_leaf(node_kind, parent_kind),
        Lang::Kotlin => kotlin_leaf(node_kind, parent_kind),
        Lang::Groovy => groovy_leaf(node_kind, parent_kind),
        Lang::Yaml => yaml_leaf(node_kind),
        Lang::Properties => properties_leaf(node_kind, parent_kind),
        Lang::Toml => toml_leaf(node_kind, parent_kind),
        Lang::Unknown => None,
    }
}

fn markdown_leaf(kind: &str, _parent: &str) -> Option<HighlightKind> {
    match kind {
        "atx_h1_marker"
        | "atx_h2_marker"
        | "atx_h3_marker"
        | "atx_h4_marker"
        | "atx_h5_marker"
        | "atx_h6_marker"
        | "setext_heading_marker" => Some(HighlightKind::Heading),
        "[" | "]" | "(" | ")" => Some(HighlightKind::Link),
        "*" | "_" | "`" | "**" | "***" | "__" | "___" => Some(HighlightKind::Emphasis),
        "fenced_code_block_delimiter" => Some(HighlightKind::CodeBlock),
        _ => None,
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

fn sh_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "if" | "then" | "else" | "elif" | "fi" | "for" | "while" | "until" | "do" | "done"
        | "case" | "esac" | "in" | "function" | "select" | "return" | "break" | "continue" => {
            Some(HighlightKind::Keyword)
        }
        "[[" | "]]" | "((" | "))" => Some(HighlightKind::Keyword),
        "number" => Some(HighlightKind::Number),
        "$" => Some(HighlightKind::Punctuation),
        "variable_name" => Some(HighlightKind::Type),
        "word" if parent == "command_name" => Some(HighlightKind::Function),
        "{" | "}" | "(" | ")" | ";" | ";;" | "|" | "&" | "&&" | "||" => {
            Some(HighlightKind::Punctuation)
        }
        _ => None,
    }
}

fn ts_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "function" | "var" | "let" | "const" | "if" | "else" | "for" | "while" | "do"
        | "return" | "new" | "this" | "class" | "extends" | "import" | "export" | "from"
        | "default" | "switch" | "case" | "break" | "continue" | "throw" | "try" | "catch"
        | "finally" | "in" | "of" | "typeof" | "instanceof" | "void" | "delete" | "async"
        | "await" | "yield" | "static" | "get" | "set" | "debugger" | "type" | "interface"
        | "enum" | "namespace" | "module" | "readonly" | "abstract" | "implements" | "private"
        | "protected" | "public" | "declare" | "is" | "keyof" | "infer" | "satisfies" | "as"
        | "override" => Some(HighlightKind::Keyword),
        "true" | "false" | "null" | "undefined" => Some(HighlightKind::Keyword),
        "number" => Some(HighlightKind::Number),
        "type_identifier" | "predefined_type" => Some(HighlightKind::Type),
        "identifier" if matches!(parent, "call_expression" | "new_expression") => {
            Some(HighlightKind::Function)
        }
        _ => None,
    }
}

fn csharp_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "class" | "struct" | "interface" | "enum" | "record" | "namespace" | "using" | "public"
        | "private" | "protected" | "internal" | "static" | "readonly" | "abstract" | "virtual"
        | "override" | "sealed" | "void" | "var" | "new" | "if" | "else" | "for" | "foreach"
        | "while" | "do" | "switch" | "case" | "default" | "break" | "continue" | "return"
        | "this" | "base" | "in" | "out" | "ref" | "params" | "is" | "as" | "throw" | "try"
        | "catch" | "finally" | "async" | "await" | "yield" | "lock" | "unsafe" | "fixed"
        | "checked" | "unchecked" | "delegate" | "event" | "extern" | "operator" | "implicit"
        | "explicit" | "where" | "typeof" | "sizeof" | "nameof" | "stackalloc" | "partial"
        | "global" | "init" | "required" => Some(HighlightKind::Keyword),
        "null" | "true" | "false" => Some(HighlightKind::Keyword),
        "predefined_type" => Some(HighlightKind::Type),
        "integer_literal" | "real_literal" => Some(HighlightKind::Number),
        "identifier" if parent == "invocation_expression" => Some(HighlightKind::Function),
        _ => None,
    }
}

fn java_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "class" | "interface" | "enum" | "record" | "extends" | "implements" | "package"
        | "import" | "public" | "private" | "protected" | "static" | "final" | "abstract"
        | "synchronized" | "native" | "transient" | "volatile" | "void" | "if" | "else" | "for"
        | "while" | "do" | "switch" | "case" | "default" | "break" | "continue" | "return"
        | "this" | "super" | "new" | "throw" | "try" | "catch" | "finally" | "throws"
        | "instanceof" | "yield" | "var" | "sealed" | "permits" => Some(HighlightKind::Keyword),
        "null_literal" | "true" | "false" => Some(HighlightKind::Keyword),
        "type_identifier"
        | "boolean_type"
        | "void_type"
        | "integral_type"
        | "floating_point_type" => Some(HighlightKind::Type),
        "decimal_integer_literal"
        | "hex_integer_literal"
        | "binary_integer_literal"
        | "octal_integer_literal"
        | "decimal_floating_point_literal"
        | "hex_floating_point_literal" => Some(HighlightKind::Number),
        "identifier" if parent == "method_invocation" => Some(HighlightKind::Function),
        _ => None,
    }
}

fn go_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "func" | "var" | "const" | "type" | "struct" | "interface" | "package" | "import"
        | "return" | "if" | "else" | "for" | "range" | "switch" | "case" | "default" | "break"
        | "continue" | "goto" | "go" | "defer" | "select" | "chan" | "map" | "fallthrough" => {
            Some(HighlightKind::Keyword)
        }
        "true" | "false" | "nil" => Some(HighlightKind::Keyword),
        "type_identifier" => Some(HighlightKind::Type),
        "int_literal" | "float_literal" | "imaginary_literal" => Some(HighlightKind::Number),
        "identifier" if parent == "call_expression" => Some(HighlightKind::Function),
        _ => None,
    }
}

fn kotlin_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "class" | "interface" | "object" | "fun" | "val" | "var" | "if" | "else" | "when"
        | "for" | "while" | "do" | "return" | "this" | "super" | "is" | "as" | "in" | "out"
        | "by" | "package" | "import" | "public" | "private" | "protected" | "internal"
        | "open" | "final" | "abstract" | "override" | "sealed" | "data" | "inline"
        | "operator" | "infix" | "lateinit" | "const" | "companion" | "init" | "constructor"
        | "enum" | "annotation" | "suspend" | "tailrec" | "external" | "noinline"
        | "crossinline" | "reified" | "vararg" | "throw" | "try" | "catch" | "finally"
        | "break" | "continue" => Some(HighlightKind::Keyword),
        "true" | "false" | "null" => Some(HighlightKind::Keyword),
        "type_identifier" | "user_type" => Some(HighlightKind::Type),
        "integer_literal" | "long_literal" | "hex_literal" | "bin_literal" | "real_literal"
        | "double_literal" | "float_literal" | "unsigned_literal" => Some(HighlightKind::Number),
        "simple_identifier" if parent == "call_expression" => Some(HighlightKind::Function),
        _ => None,
    }
}

fn groovy_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "def" | "class" | "interface" | "trait" | "enum" | "extends" | "implements" | "package"
        | "import" | "public" | "private" | "protected" | "static" | "final" | "abstract"
        | "synchronized" | "void" | "if" | "else" | "for" | "while" | "do" | "switch" | "case"
        | "default" | "break" | "continue" | "return" | "this" | "super" | "new" | "throw"
        | "try" | "catch" | "finally" | "throws" | "instanceof" | "in" | "as" => {
            Some(HighlightKind::Keyword)
        }
        "null" | "true" | "false" => Some(HighlightKind::Keyword),
        "type_identifier" => Some(HighlightKind::Type),
        "integer_literal"
        | "decimal_floating_point_literal"
        | "hex_integer_literal"
        | "octal_integer_literal"
        | "binary_integer_literal" => Some(HighlightKind::Number),
        "identifier" if parent == "method_invocation" || parent == "method_call" => {
            Some(HighlightKind::Function)
        }
        _ => None,
    }
}

fn yaml_leaf(kind: &str) -> Option<HighlightKind> {
    match kind {
        "true" | "false" | "null" => Some(HighlightKind::Keyword),
        "integer_scalar" | "float_scalar" => Some(HighlightKind::Number),
        "boolean_scalar" => Some(HighlightKind::Keyword),
        "null_scalar" => Some(HighlightKind::Keyword),
        ":" | "-" | "?" | "[" | "]" | "{" | "}" | "," => Some(HighlightKind::Punctuation),
        _ => None,
    }
}

fn properties_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "=" | ":" => Some(HighlightKind::Punctuation),
        // Property keys (the LHS of `key=value`) read nicely as Type.
        "name" | "key" if parent == "property" => Some(HighlightKind::Type),
        _ => None,
    }
}

fn toml_leaf(kind: &str, parent: &str) -> Option<HighlightKind> {
    match kind {
        "true" | "false" => Some(HighlightKind::Keyword),
        "integer" | "float" => Some(HighlightKind::Number),
        "local_date" | "local_time" | "local_date_time" | "offset_date_time" => {
            Some(HighlightKind::Number)
        }
        "[" | "]" | "[[" | "]]" | "{" | "}" | "=" | "," | "." => Some(HighlightKind::Punctuation),
        "bare_key"
            if matches!(
                parent,
                "pair" | "table" | "table_array_element" | "dotted_key"
            ) =>
        {
            Some(HighlightKind::Type)
        }
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
        Lang::Sh => matches!(ctx, "function_definition"),
        Lang::TypeScript | Lang::Tsx => matches!(
            ctx,
            "function_declaration" | "method_definition" | "function" | "method_signature"
        ),
        Lang::CSharp => matches!(
            ctx,
            "method_declaration"
                | "constructor_declaration"
                | "destructor_declaration"
                | "local_function_statement"
        ),
        Lang::Java => matches!(ctx, "method_declaration" | "constructor_declaration"),
        Lang::Go => matches!(ctx, "function_declaration" | "method_declaration"),
        Lang::Kotlin => matches!(ctx, "function_declaration"),
        Lang::Groovy => matches!(ctx, "method_declaration"),
        Lang::Markdown => false,
        _ => false,
    }
}

/// Extract text from a tree-sitter node
fn node_text(node: Node<'_>, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    if start >= source.len() || end > source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end]).to_string()
}

/// Handle markdown fenced code blocks with embedded language highlighting
#[allow(clippy::only_used_in_recursion)]
fn handle_markdown_code_fence(
    node: Node<'_>,
    source: &[u8],
    start_byte: usize,
    end_byte: usize,
    spans: &mut Vec<HighlightSpan>,
) {
    // Find info_string (language) and code_fence_content
    let mut embedded_lang: Option<Lang> = None;
    let mut code_start: usize = 0;
    let mut code_end: usize = 0;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "info_string" => {
                    let text = node_text(child, source).trim().to_string();
                    if let Some(lang_name) = text.split_whitespace().next() {
                        let normalized_lang_name = lang_name
                            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                            .to_ascii_lowercase();
                        if !normalized_lang_name.is_empty() {
                            embedded_lang = Some(Lang::from_extension(&normalized_lang_name));
                        }
                    }
                }
                "code_fence_content" => {
                    code_start = child.start_byte();
                    code_end = child.end_byte();
                }
                _ => {}
            }
        }
    }

    // Recursively visit all children to highlight fence markers and info_string
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "code_fence_content" {
                continue; // Handle separately for embedded highlighting
            }
            visit(child, Lang::Markdown, source, start_byte, end_byte, spans);
        }
    }

    // If embedded language detected, parse and highlight the code content
    if let Some(lang) = embedded_lang
        && lang != Lang::Unknown
        && code_start < code_end
        && code_end <= source.len()
    {
        let mut parser = tree_sitter::Parser::new();
        if let Some(ts_lang) = lang.ts_language()
            && parser.set_language(&ts_lang).is_ok()
        {
            let content_bytes = &source[code_start..code_end];
            if let Some(tree) = parser.parse(content_bytes, None) {
                let offset = code_start;
                collect_embedded_spans(
                    &tree.root_node(),
                    lang,
                    offset,
                    start_byte,
                    end_byte,
                    spans,
                );
            }
        }
    }
}

/// Collect spans from embedded language parsing with offset adjustment
#[allow(clippy::too_many_arguments)]
fn collect_embedded_spans(
    node: &Node<'_>,
    lang: Lang,
    offset: usize,
    start_byte: usize,
    end_byte: usize,
    spans: &mut Vec<HighlightSpan>,
) {
    let node_start = node.start_byte();
    let node_end = node.end_byte();

    if node_start >= node_end {
        return;
    }

    // Prune: skip outside visible range
    if node_end + offset <= start_byte || node_start + offset >= end_byte {
        return;
    }

    let kind = node.kind();

    // Atomic nodes
    if let Some(hk) = atomic_kind(kind, lang) {
        let s = (node_start + offset).max(start_byte);
        let e = (node_end + offset).min(end_byte);
        if s < e {
            spans.push(HighlightSpan {
                start: s,
                end: e,
                kind: hk,
            });
        }
        return;
    }

    // Leaf nodes
    if node.child_count() == 0 {
        let parent_kind = node.parent().map(|p| p.kind()).unwrap_or("");
        if let Some(hk) = leaf_kind(kind, parent_kind, lang) {
            let s = (node_start + offset).max(start_byte);
            let e = (node_end + offset).min(end_byte);
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

    // Recurse
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_embedded_spans(&child, lang, offset, start_byte, end_byte, spans);
        }
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

    fn parse_markdown(source: &str) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_md::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn parse_shell(source: &str) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn parse_with(source: &str, lang: tree_sitter::Language) -> Tree {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang).unwrap();
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

    // ── Markdown ───────────────────────────────────────────────────────────────

    #[test]
    fn markdown_atx_heading_marker() {
        let src = "# Hello World";
        let tree = parse_markdown(src);
        let spans = spans_for(src, &tree, Lang::Markdown);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Heading && &src[s.start..s.end] == "#"),
            "expected Heading span for '#', got: {:?}",
            spans
        );
    }

    #[test]
    fn markdown_link_punctuation() {
        let src = "[link](https://example.com)";
        let tree = parse_markdown(src);
        let spans = spans_for(src, &tree, Lang::Markdown);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Link && src.get(s.start..s.end) == Some("[")),
            "expected Link span for '[', got: {:?}",
            spans
        );
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Link && src.get(s.start..s.end) == Some("]")),
            "expected Link span for ']', got: {:?}",
            spans
        );
    }

    #[test]
    fn markdown_fenced_code_block_with_embedded_rust() {
        let src = "```rust\nlet x = 1;\n```";
        let tree = parse_markdown(src);
        let spans = spans_for(src, &tree, Lang::Markdown);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::CodeBlock && &src[s.start..s.end] == "```"),
            "expected CodeBlock span for fence markers, got: {:?}",
            spans
        );
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "let"),
            "expected embedded 'let' as Keyword, got: {:?}",
            spans
        );
    }

    // ── Shell ──────────────────────────────────────────────────────────────────

    #[test]
    fn sh_comment() {
        let src = "# hello\necho hi\n";
        let tree = parse_shell(src);
        let spans = spans_for(src, &tree, Lang::Sh);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "# hello"),
            "expected Comment span for '# hello', got: {:?}",
            spans
        );
    }

    #[test]
    fn sh_double_quoted_string() {
        let src = r#"echo "hi""#;
        let tree = parse_shell(src);
        let spans = spans_for(src, &tree, Lang::Sh);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::String && &src[s.start..s.end] == "\"hi\""),
            "expected String span for '\"hi\"', got: {:?}",
            spans
        );
    }

    #[test]
    fn sh_keyword_if() {
        let src = "if true; then :; fi";
        let tree = parse_shell(src);
        let spans = spans_for(src, &tree, Lang::Sh);
        for kw in ["if", "then", "fi"] {
            assert!(
                spans
                    .iter()
                    .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == kw),
                "expected Keyword span for '{}', got: {:?}",
                kw,
                spans
            );
        }
    }

    #[test]
    fn sh_command_name() {
        let src = "echo hello";
        let tree = parse_shell(src);
        let spans = spans_for(src, &tree, Lang::Sh);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Function && &src[s.start..s.end] == "echo"),
            "expected Function span for 'echo', got: {:?}",
            spans
        );
    }

    #[test]
    fn sh_variable_expansion() {
        let src = "echo $HOME";
        let tree = parse_shell(src);
        let spans = spans_for(src, &tree, Lang::Sh);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Type && &src[s.start..s.end] == "HOME"),
            "expected Type span for 'HOME', got: {:?}",
            spans
        );
    }

    // ── TypeScript / TSX ──────────────────────────────────────────────────────

    #[test]
    fn ts_keyword_interface() {
        let src = "interface User { name: string; }";
        let tree = parse_with(src, tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into());
        let spans = spans_for(src, &tree, Lang::TypeScript);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "interface"),
            "expected Keyword span for 'interface', got: {:?}",
            spans
        );
    }

    #[test]
    fn ts_string_literal() {
        let src = "const x = \"hi\";";
        let tree = parse_with(src, tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into());
        let spans = spans_for(src, &tree, Lang::TypeScript);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::String && &src[s.start..s.end] == "\"hi\""),
            "expected String span for '\"hi\"', got: {:?}",
            spans
        );
    }

    #[test]
    fn tsx_keyword() {
        let src = "const X = () => <div/>;";
        let tree = parse_with(src, tree_sitter_typescript::LANGUAGE_TSX.into());
        let spans = spans_for(src, &tree, Lang::Tsx);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "const"),
            "expected Keyword span for 'const', got: {:?}",
            spans
        );
    }

    // ── C# ─────────────────────────────────────────────────────────────────────

    #[test]
    fn csharp_keyword_class() {
        let src = "public class Foo { }";
        let tree = parse_with(src, tree_sitter_c_sharp::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::CSharp);
        for kw in ["public", "class"] {
            assert!(
                spans
                    .iter()
                    .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == kw),
                "expected Keyword span for '{}', got: {:?}",
                kw,
                spans
            );
        }
    }

    #[test]
    fn csharp_comment() {
        let src = "// hi\nclass X {}";
        let tree = parse_with(src, tree_sitter_c_sharp::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::CSharp);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "// hi"),
            "expected Comment span for '// hi', got: {:?}",
            spans
        );
    }

    // ── Java ───────────────────────────────────────────────────────────────────

    #[test]
    fn java_keyword_class() {
        let src = "public class Foo {}";
        let tree = parse_with(src, tree_sitter_java::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Java);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "class"),
            "expected Keyword span for 'class', got: {:?}",
            spans
        );
    }

    #[test]
    fn java_comment() {
        let src = "// hi\nclass X {}";
        let tree = parse_with(src, tree_sitter_java::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Java);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "// hi"),
            "expected Comment span for '// hi', got: {:?}",
            spans
        );
    }

    // ── Go ─────────────────────────────────────────────────────────────────────

    #[test]
    fn go_keyword_func() {
        let src = "package main\nfunc f() {}";
        let tree = parse_with(src, tree_sitter_go::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Go);
        for kw in ["package", "func"] {
            assert!(
                spans
                    .iter()
                    .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == kw),
                "expected Keyword span for '{}', got: {:?}",
                kw,
                spans
            );
        }
    }

    #[test]
    fn go_string() {
        let src = "package main\nvar s = \"hi\"";
        let tree = parse_with(src, tree_sitter_go::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Go);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::String && &src[s.start..s.end] == "\"hi\""),
            "expected String span for '\"hi\"', got: {:?}",
            spans
        );
    }

    // ── Kotlin ─────────────────────────────────────────────────────────────────

    #[test]
    fn kotlin_keyword_fun() {
        let src = "fun main() {}";
        let tree = parse_with(src, tree_sitter_kotlin_ng::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Kotlin);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "fun"),
            "expected Keyword span for 'fun', got: {:?}",
            spans
        );
    }

    // ── Groovy ─────────────────────────────────────────────────────────────────

    #[test]
    fn groovy_keyword_def() {
        let src = "def x = 1";
        let tree = parse_with(src, tree_sitter_groovy::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Groovy);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword && &src[s.start..s.end] == "def"),
            "expected Keyword span for 'def', got: {:?}",
            spans
        );
    }

    // ── YAML ───────────────────────────────────────────────────────────────────

    #[test]
    fn yaml_comment() {
        let src = "# hello\nname: foo\n";
        let tree = parse_with(src, tree_sitter_yaml::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Yaml);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "# hello"),
            "expected Comment span for '# hello', got: {:?}",
            spans
        );
    }

    // ── Properties ─────────────────────────────────────────────────────────────

    #[test]
    fn properties_comment() {
        let src = "# hello\nfoo=bar\n";
        let tree = parse_with(src, tree_sitter_properties::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Properties);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "# hello"),
            "expected Comment span for '# hello', got: {:?}",
            spans
        );
    }

    // ── TOML ───────────────────────────────────────────────────────────────────

    #[test]
    fn toml_string() {
        let src = "name = \"hi\"";
        let tree = parse_with(src, tree_sitter_toml_ng::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Toml);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::String && &src[s.start..s.end] == "\"hi\""),
            "expected String span for '\"hi\"', got: {:?}",
            spans
        );
    }

    #[test]
    fn toml_comment() {
        let src = "# hi\nname = 1";
        let tree = parse_with(src, tree_sitter_toml_ng::LANGUAGE.into());
        let spans = spans_for(src, &tree, Lang::Toml);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == HighlightKind::Comment && &src[s.start..s.end] == "# hi"),
            "expected Comment span for '# hi', got: {:?}",
            spans
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
