pub mod highlighter;
pub mod language;
pub mod queries;

use ropey::Rope;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::buffer::cursor::ByteRange;
use crate::syntax::language::Lang;

/// Manages tree-sitter state for a single buffer.
///
/// # Parsing strategy (Phase 4)
/// Parsing is synchronous: `reparse_rope()` is called after every edit in
/// `AppState::update()`. tree-sitter incremental re-parse is fast enough
/// (< 5 ms for typical edits) that this does not measurably affect input latency.
///
/// Phase 7 will migrate to an async background worker so the render loop is
/// never blocked during the initial parse of large files.
pub struct SyntaxHost {
    parser: Parser,
    /// The most recently parsed tree, if a supported language is active.
    pub tree: Option<Tree>,
    /// Active language for the current buffer.
    pub language: Lang,
    /// History stack for AST-aware selection (Ctrl+W / Ctrl+Shift+W).
    /// Each entry is the selection *before* an expansion, allowing contraction.
    selection_history: Vec<ByteRange>,
}

impl SyntaxHost {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            tree: None,
            language: Lang::Unknown,
            selection_history: Vec::new(),
        }
    }

    /// Set the language for parsing. Re-configures the parser; call before the first parse.
    pub fn set_language(&mut self, lang: Lang) {
        self.language = lang;
        self.tree = None;
        self.selection_history.clear();

        if let Some(ts_lang) = lang.ts_language() {
            if let Err(e) = self.parser.set_language(&ts_lang) {
                // ABI mismatch — treat as unknown, no parsing.
                eprintln!("tree-sitter language ABI mismatch for {:?}: {e}", lang);
                self.language = Lang::Unknown;
            }
        } else {
            // Unknown language — reset to no language so the parser won't be used.
            self.parser.reset();
        }
    }

    /// (Re-)parse the buffer content using the configured language.
    ///
    /// Always performs a full reparse. Tree-sitter's incremental parsing
    /// requires the previous tree to be informed of edits via `Tree::edit()`
    /// before being passed back to `parse()`; without that, reused subtrees
    /// keep stale byte positions and produce highlight spans pointing into the
    /// wrong locations of the new source. Until edits are tracked through the
    /// buffer, the only correct option is a full reparse from `None`.
    /// See `future_syntax_parse_plan.md` for the planned incremental design.
    pub fn reparse_rope(&mut self, rope: &Rope) {
        if self.language == Lang::Unknown {
            self.tree = None;
            return;
        }

        // Build source bytes. We use rope.to_string() for Phase 4 simplicity.
        // Phase 7 will switch to parse_with() + rope chunk callbacks to avoid
        // this allocation.
        let source = rope.to_string();
        self.tree = self.parser.parse(source.as_bytes(), None);
    }

    /// Returns true if a valid parse tree is available.
    #[allow(dead_code)]
    pub fn has_tree(&self) -> bool {
        self.tree.is_some()
    }

    // ── AST-aware selection ────────────────────────────────────────────────

    /// Ctrl+W: expand the selection to the next enclosing AST node.
    ///
    /// If `current` is empty (cursor, no selection), expands to the smallest
    /// node at the cursor position. On subsequent presses, walks up to the
    /// parent node.
    ///
    /// Returns the new `ByteRange` to select, or `None` if no tree is available
    /// or the root has been reached.
    pub fn expand_selection(&mut self, current: ByteRange) -> Option<ByteRange> {
        let tree = self.tree.as_ref()?;
        let root = tree.root_node();

        let candidate = if current.is_empty() {
            // No selection — find the leaf at the cursor position.
            root.descendant_for_byte_range(current.start, current.start)?
        } else {
            // We have a selection — find the smallest node that is *strictly larger*.
            let mut node = root.descendant_for_byte_range(current.start, current.end)?;

            // Walk up until we find a node whose range differs from current selection.
            loop {
                let node_range = ByteRange::new(node.start_byte(), node.end_byte());
                if node_range.start != current.start || node_range.end != current.end {
                    break;
                }
                node = node.parent()?;
            }
            node
        };

        let new_range = ByteRange::new(candidate.start_byte(), candidate.end_byte());

        // Don't expand if the result is identical to current (already at root).
        if new_range.start == current.start && new_range.end == current.end {
            return None;
        }

        // Push current onto the history stack so contraction can restore it.
        self.selection_history.push(current);

        Some(new_range)
    }

    /// Ctrl+Shift+W: contract the selection by popping the last expansion.
    ///
    /// Returns the previous `ByteRange`, or `None` if there is no history
    /// (nothing to contract to).
    pub fn contract_selection(&mut self) -> Option<ByteRange> {
        self.selection_history.pop()
    }

    /// Clear the expansion history. Call whenever the cursor moves by means
    /// other than Ctrl+W / Ctrl+Shift+W (typing, arrow keys, mouse click, etc.)
    /// so that the next Ctrl+W always starts fresh from the actual cursor position.
    pub fn clear_selection_history(&mut self) {
        self.selection_history.clear();
    }

    #[allow(dead_code)]
    pub fn selection_history_depth(&self) -> usize {
        self.selection_history.len()
    }

    /// Returns the line-comment prefix for the current language, or `None`.
    pub fn comment_prefix(&self) -> Option<&'static str> {
        self.language.comment_prefix()
    }

    /// Return syntax highlight spans for the visible byte range `[start_byte, end_byte)`.
    /// Returns an empty `Vec` if no parse tree is available.
    pub fn highlight_spans(
        &self,
        source: &[u8],
        start_byte: usize,
        end_byte: usize,
    ) -> Vec<highlighter::HighlightSpan> {
        match &self.tree {
            Some(tree) => highlighter::highlight(tree, source, self.language, start_byte, end_byte),
            None => Vec::new(),
        }
    }

    /// Walk up the parse tree from `byte_offset` and collect the enclosing
    /// named containers (functions, classes, modules, etc.) outermost first.
    ///
    /// Returns an empty `Vec` if no parse tree is available or if the cursor
    /// isn't inside any named container. Names whose tree-sitter `name` field
    /// can't be decoded are skipped silently.
    pub fn enclosing_named_path(&self, rope: &Rope, byte_offset: usize) -> Vec<EnclosingSymbol> {
        let tree = match &self.tree {
            Some(t) => t,
            None => return Vec::new(),
        };
        let root = tree.root_node();
        let bound = byte_offset.min(rope.len_bytes());
        let leaf = match root.descendant_for_byte_range(bound, bound) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut path = Vec::new();
        let mut node = Some(leaf);
        while let Some(n) = node {
            if let Some(label) = container_label(n.kind()) {
                let name_node = ["name", "type", "path", "declarator"]
                    .into_iter()
                    .find_map(|f| n.child_by_field_name(f));
                if let Some(name_node) = name_node {
                    let start = name_node.start_byte().min(rope.len_bytes());
                    let end = name_node.end_byte().min(rope.len_bytes());
                    if start < end {
                        let start_char = rope.byte_to_char(start);
                        let end_char = rope.byte_to_char(end);
                        let text: String = rope.slice(start_char..end_char).chars().collect();
                        let trimmed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !trimmed.is_empty() {
                            path.push(EnclosingSymbol {
                                name: trimmed,
                                kind: label,
                            });
                        }
                    }
                }
            }
            node = n.parent();
        }
        path.reverse();
        path
    }
}

/// One entry on an enclosing-symbol path: a node name and a short kind label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnclosingSymbol {
    pub name: String,
    pub kind: &'static str,
}

/// A named definition discovered by a tree-sitter symbols query (used by the
/// Ctrl+Shift+O picker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Display name of the symbol (e.g. `"foo"` for `fn foo`).
    pub name: String,
    /// Short kind label such as `"fn"`, `"class"`, `"struct"`. Derived from
    /// the `@symbol.<kind>` capture in the query.
    pub kind: &'static str,
    /// Byte range of the *definition* node — i.e. the `@symbol.kind` capture,
    /// not just the name. Used to jump the cursor.
    pub byte_range: ByteRange,
}

impl SyntaxHost {
    /// Run the language's `@fold` query and return the byte ranges of every
    /// foldable region (function body, class body, etc.).
    ///
    /// Returns an empty `Vec` when no tree is available, when the language
    /// has no fold query, or when the query fails to compile. Ranges are
    /// sorted by start byte; duplicates are removed.
    pub fn fold_ranges(&self, rope: &Rope) -> Vec<ByteRange> {
        let tree = match &self.tree {
            Some(t) => t,
            None => return Vec::new(),
        };
        let pattern = match queries::folds_query_for(self.language) {
            Some(p) if !p.trim().is_empty() => p,
            _ => return Vec::new(),
        };
        let ts_lang = match self.language.ts_language() {
            Some(l) => l,
            None => return Vec::new(),
        };
        let query = match Query::new(&ts_lang, pattern) {
            Ok(q) => q,
            Err(_) => return Vec::new(),
        };
        let source = rope.to_string();
        let bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), bytes);
        let mut out: Vec<ByteRange> = Vec::new();
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let r = ByteRange::new(cap.node.start_byte(), cap.node.end_byte());
                out.push(r);
            }
        }
        out.sort_by_key(|r| (r.start, r.end));
        out.dedup();
        // Drop folds that are entirely on a single line — folding gives no
        // visual benefit there.
        out.retain(|r| {
            let start_line = rope.byte_to_line(r.start.min(rope.len_bytes()));
            let end_line = rope.byte_to_line(r.end.min(rope.len_bytes()));
            end_line > start_line
        });
        out
    }

    /// Run the language-specific symbols query over the current parse tree
    /// and collect every `@symbol.<kind>` match.
    ///
    /// Returns an empty `Vec` when no tree is available, when the language has
    /// no symbols query, or when the query fails to compile (which shouldn't
    /// happen in production but is recovered from gracefully).
    pub fn collect_symbols(&self, rope: &Rope) -> Vec<Symbol> {
        let tree = match &self.tree {
            Some(t) => t,
            None => return Vec::new(),
        };
        let pattern = match queries::symbols_query_for(self.language) {
            Some(p) if !p.trim().is_empty() => p,
            _ => return Vec::new(),
        };
        let ts_lang = match self.language.ts_language() {
            Some(l) => l,
            None => return Vec::new(),
        };
        let query = match Query::new(&ts_lang, pattern) {
            Ok(q) => q,
            Err(_) => return Vec::new(),
        };

        let source = rope.to_string();
        let bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), bytes);

        let capture_names: Vec<&str> = query.capture_names().to_vec();
        let mut out = Vec::new();
        while let Some(m) = matches.next() {
            let mut name_text: Option<String> = None;
            let mut symbol_range: Option<(ByteRange, &'static str)> = None;
            for cap in m.captures {
                let cap_name = match capture_names.get(cap.index as usize) {
                    Some(s) => *s,
                    None => continue,
                };
                if cap_name == "name" {
                    let start = cap.node.start_byte();
                    let end = cap.node.end_byte();
                    let text = std::str::from_utf8(&bytes[start..end])
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !text.is_empty() {
                        name_text = Some(text);
                    }
                } else if let Some(rest) = cap_name.strip_prefix("symbol.") {
                    let kind = symbol_kind_label(rest);
                    let r = ByteRange::new(cap.node.start_byte(), cap.node.end_byte());
                    symbol_range = Some((r, kind));
                }
            }
            if let (Some(name), Some((range, kind))) = (name_text, symbol_range) {
                out.push(Symbol {
                    name,
                    kind,
                    byte_range: range,
                });
            }
        }
        // Sort by line number for stable display.
        out.sort_by_key(|s| s.byte_range.start);
        out
    }
}

/// Map a query capture suffix (`fn`, `class`, etc.) to a stable `&'static str`
/// kind label. Unknown suffixes fall back to `"?"`.
fn symbol_kind_label(suffix: &str) -> &'static str {
    match suffix {
        "fn" => "fn",
        "impl" => "impl",
        "struct" => "struct",
        "enum" => "enum",
        "trait" => "trait",
        "mod" => "mod",
        "const" => "const",
        "static" => "static",
        "macro" => "macro",
        "type" => "type",
        "class" => "class",
        "interface" => "interface",
        "object" => "object",
        "ns" => "ns",
        "key" => "key",
        "tag" => "tag",
        "rule" => "rule",
        "table" => "table",
        "var" => "var",
        "property" => "property",
        "h1" => "h1",
        "h2" => "h2",
        "h3" => "h3",
        "h4" => "h4",
        _ => "?",
    }
}

/// Map a tree-sitter node kind to a short label for display, or `None` if the
/// node kind isn't a "named container" worth showing in breadcrumbs.
fn container_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        // Functions / methods (covers Rust, Python, JS/TS, Go, Java, C#, Kotlin, Groovy, Bash).
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "method_definition"
        | "method_declaration"
        | "constructor_declaration"
        | "constructor_definition" => "fn",
        // Rust-only.
        "impl_item" => "impl",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "mod_item" => "mod",
        // Classes / interfaces / objects (Java, C#, JS/TS, Python, Kotlin, Groovy).
        "class_definition" | "class_declaration" => "class",
        "interface_declaration" => "interface",
        "object_declaration" => "object",
        // C# / Kotlin namespaces.
        "namespace_declaration" | "package_clause" => "ns",
        // Go top-level type declarations.
        "type_declaration" | "type_spec" => "type",
        // Markdown structural sections.
        "section" | "atx_heading" | "setext_heading" => "§",
        _ => return None,
    })
}

impl Default for SyntaxHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::cursor::ByteRange;

    fn host_for_rust(src: &str) -> (SyntaxHost, Rope) {
        let mut host = SyntaxHost::new();
        host.set_language(Lang::Rust);
        let rope = Rope::from_str(src);
        host.reparse_rope(&rope);
        (host, rope)
    }

    fn host_for_json(src: &str) -> (SyntaxHost, Rope) {
        let mut host = SyntaxHost::new();
        host.set_language(Lang::Json);
        let rope = Rope::from_str(src);
        host.reparse_rope(&rope);
        (host, rope)
    }

    #[test]
    fn parses_rust() {
        let (host, _) = host_for_rust("fn main() {}");
        assert!(host.has_tree());
        let root = host.tree.as_ref().unwrap().root_node();
        assert!(!root.has_error());
    }

    #[test]
    fn parses_json() {
        let (host, _) = host_for_json(r#"{"key": "value"}"#);
        assert!(host.has_tree());
        assert!(!host.tree.as_ref().unwrap().root_node().has_error());
    }

    #[test]
    fn unknown_language_no_tree() {
        let mut host = SyntaxHost::new();
        let rope = Rope::from_str("hello world");
        host.reparse_rope(&rope);
        assert!(!host.has_tree());
    }

    #[test]
    fn expand_from_cursor_position() {
        // Source: `fn main() {}`
        // Cursor is inside "main" (byte 3)
        let (mut host, _) = host_for_rust("fn main() {}");
        let cursor = ByteRange::new(3, 3); // zero-width, inside "main"
        let expanded = host.expand_selection(cursor).unwrap();
        // Should expand to at least encompass "main"
        assert!(expanded.start <= 3);
        assert!(expanded.end >= 7); // "main" ends at byte 7
    }

    #[test]
    fn expand_grows_to_parent() {
        let (mut host, _) = host_for_rust("fn main() {}");
        // Start at "main" word (bytes 3..7)
        let sel1 = ByteRange::new(3, 7);
        let sel2 = host.expand_selection(sel1).unwrap();
        // sel2 should be strictly larger than sel1
        assert!(sel2.start <= sel1.start && sel2.end >= sel1.end);
        assert!(sel2.start != sel1.start || sel2.end != sel1.end);
    }

    #[test]
    fn contract_restores_previous() {
        let (mut host, _) = host_for_rust("fn main() {}");
        let original = ByteRange::new(3, 3);
        let expanded = host.expand_selection(original).unwrap();
        let contracted = host.contract_selection().unwrap();
        assert_eq!(contracted, original);
        let _ = expanded;
    }

    #[test]
    fn contract_with_no_history_returns_none() {
        let mut host = SyntaxHost::new();
        assert!(host.contract_selection().is_none());
    }

    #[test]
    fn clear_history_resets_stack() {
        let (mut host, _) = host_for_rust("fn main() {}");
        let _ = host.expand_selection(ByteRange::new(3, 3));
        assert_eq!(host.selection_history_depth(), 1);
        host.clear_selection_history();
        assert_eq!(host.selection_history_depth(), 0);
    }

    #[test]
    fn expand_no_tree_returns_none() {
        let mut host = SyntaxHost::new(); // Unknown language, no tree
        assert!(host.expand_selection(ByteRange::new(0, 0)).is_none());
    }

    #[test]
    fn reparse_updates_tree() {
        let mut host = SyntaxHost::new();
        host.set_language(Lang::Rust);

        let rope1 = Rope::from_str("fn a() {}");
        host.reparse_rope(&rope1);
        assert!(host.has_tree());

        let rope2 = Rope::from_str("fn b() { let x = 1; }");
        host.reparse_rope(&rope2);
        assert!(host.has_tree());
        assert!(!host.tree.as_ref().unwrap().root_node().has_error());
    }

    #[test]
    fn enclosing_path_for_function_returns_fn_only() {
        let src = "fn foo() { let x = 1; }";
        let (host, rope) = host_for_rust(src);
        // Cursor inside `let x = 1;` — about byte 16.
        let path = host.enclosing_named_path(&rope, 16);
        // At minimum, the function `foo` is in the path.
        assert!(
            path.iter().any(|e| e.name == "foo" && e.kind == "fn"),
            "expected fn foo in path, got {:?}",
            path
        );
    }

    #[test]
    fn enclosing_path_for_method_inside_impl_includes_both() {
        let src = "impl Bar { fn baz(&self) { let x = 1; } }";
        let (host, rope) = host_for_rust(src);
        // Cursor inside `let x = 1;` — about byte 32.
        let path = host.enclosing_named_path(&rope, 32);
        let kinds: Vec<&str> = path.iter().map(|e| e.kind).collect();
        let names: Vec<&str> = path.iter().map(|e| e.name.as_str()).collect();
        assert!(kinds.contains(&"impl"), "kinds={kinds:?}");
        assert!(kinds.contains(&"fn"), "kinds={kinds:?}");
        assert!(names.contains(&"Bar"), "names={names:?}");
        assert!(names.contains(&"baz"), "names={names:?}");
    }

    #[test]
    fn enclosing_path_no_tree_returns_empty() {
        let host = SyntaxHost::new();
        let rope = Rope::from_str("anything");
        assert!(host.enclosing_named_path(&rope, 0).is_empty());
    }

    /// Regression for the sticky header: walking down through an `impl`
    /// block must land on the *current* method, not a sibling. The user
    /// reported the header staying on the first function of a class even
    /// after scrolling into the second.
    #[test]
    fn enclosing_path_tracks_current_method_in_multi_method_impl() {
        let src = "impl Bar {\n    fn first() { let x = 1; }\n    fn second() { let y = 2; }\n}\n";
        let (host, rope) = host_for_rust(src);
        let byte = src.find("let y = 2").expect("test source must contain y");
        let path = host.enclosing_named_path(&rope, byte);
        let names: Vec<&str> = path.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Bar"), "names={names:?}");
        assert!(
            names.contains(&"second"),
            "expected the second method to be in the path, got {names:?}"
        );
        assert!(
            !names.contains(&"first"),
            "first method must not be in the path, got {names:?}"
        );
    }

    #[test]
    fn collect_symbols_rust_finds_functions() {
        let src = "fn alpha() {}\nfn beta() { let x = 1; }\nstruct Gamma { x: u32 }\n";
        let (host, rope) = host_for_rust(src);
        let syms = host.collect_symbols(&rope);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(names.contains(&"Gamma"));
        let kinds: Vec<&str> = syms.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&"fn"));
        assert!(kinds.contains(&"struct"));
    }

    #[test]
    fn collect_symbols_unknown_language_returns_empty() {
        let host = SyntaxHost::new();
        let rope = Rope::from_str("fn foo() {}");
        assert!(host.collect_symbols(&rope).is_empty());
    }

    /// Regression: highlight span byte offsets must match the *current* source
    /// after a reparse. A previous incremental-parse implementation reused
    /// stale node positions from the old tree, causing spans to drift with
    /// every edit.
    #[test]
    fn spans_correct_after_reparse_with_new_content() {
        use crate::syntax::highlighter::HighlightKind;

        let mut host = SyntaxHost::new();
        host.set_language(Lang::Rust);

        // Initial parse — establishes a previous tree internally.
        host.reparse_rope(&Rope::from_str("let x = 1;"));

        // Reparse with content that grew at the front and the back.
        let src = "fn a() { let x = 1; }";
        host.reparse_rope(&Rope::from_str(src));

        let spans = host.highlight_spans(src.as_bytes(), 0, src.len());

        // "fn" must be at bytes 0..2.
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 2 && s.kind == HighlightKind::Keyword),
            "expected 'fn' Keyword span at 0..2, got: {:?}",
            spans
        );
        // "let" must be at bytes 9..12 in the new source.
        assert!(
            spans
                .iter()
                .any(|s| s.start == 9 && s.end == 12 && s.kind == HighlightKind::Keyword),
            "expected 'let' Keyword span at 9..12, got: {:?}",
            spans
        );
    }
}
