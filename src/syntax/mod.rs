pub mod highlighter;
pub mod language;

use ropey::Rope;
use tree_sitter::{Parser, Tree};

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
    /// Passes the previous tree for incremental re-parsing. If no language is
    /// configured, or if parsing fails, the stored tree is set to `None`.
    pub fn reparse_rope(&mut self, rope: &Rope) {
        if self.language == Lang::Unknown {
            self.tree = None;
            return;
        }

        // Build source bytes. We use rope.to_string() for Phase 4 simplicity.
        // Phase 7 will switch to parse_with() + rope chunk callbacks to avoid
        // this allocation.
        let source = rope.to_string();

        // Disable incremental parsing - full reparse is safer
        let _old_tree = self.tree.take();
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
}
