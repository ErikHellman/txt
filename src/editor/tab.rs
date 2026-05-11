use std::path::PathBuf;

use crate::buffer::Buffer;
use crate::buffer::folds::FoldState;
use crate::editor::viewport::Viewport;
use crate::editorconfig::{EditorConfigOverrides, load_for_file};
use crate::lsp::types::{DiagSeverity, LspDiagnostic, SemanticTokenSpan};
use crate::snippet::session::SnippetSession;
use crate::syntax::{SyntaxHost, language::Lang};

pub type BufferId = usize;

/// Per-buffer LSP state: document version and diagnostics.
pub struct LspState {
    /// Document version, incremented on every edit. Sent with didChange.
    pub version: u64,
    /// Diagnostics received from the LSP server (converted to byte offsets).
    pub diagnostics: Vec<LspDiagnostic>,
    /// Semantic tokens from the LSP server (decoded to absolute byte positions).
    pub semantic_tokens: Option<Vec<SemanticTokenSpan>>,
}

impl LspState {
    pub fn new() -> Self {
        Self {
            version: 0,
            diagnostics: Vec::new(),
            semantic_tokens: None,
        }
    }

    /// Count diagnostics by severity.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagSeverity::Warning)
            .count()
    }
}

/// One open buffer — the unit of a tab in the editor.
///
/// Owns the text buffer, its scroll viewport, file path, and the tree-sitter
/// syntax state. Each tab is completely independent.
pub struct BufferHandle {
    #[allow(dead_code)]
    pub id: BufferId,
    pub buffer: Buffer,
    pub viewport: Viewport,
    pub path: Option<PathBuf>,
    pub syntax: SyntaxHost,
    pub lsp_state: LspState,
    /// Per-buffer overrides resolved from `.editorconfig`. Empty for buffers
    /// without a path or when no `.editorconfig` is found.
    pub editorconfig: EditorConfigOverrides,
    /// Code-folding state, derived from the tree-sitter parse tree after
    /// every reparse.
    pub folds: FoldState,
    /// Active snippet session, if a snippet has been expanded and the user
    /// hasn't yet finished cycling through its tab stops.
    pub snippet_session: Option<SnippetSession>,
}

impl BufferHandle {
    /// Create an empty, unnamed buffer.
    pub fn new_empty(id: BufferId) -> Self {
        Self {
            id,
            buffer: Buffer::new(),
            viewport: Viewport::new(),
            path: None,
            syntax: SyntaxHost::new(),
            lsp_state: LspState::new(),
            editorconfig: EditorConfigOverrides::default(),
            folds: FoldState::new(),
            snippet_session: None,
        }
    }

    /// Open a file from disk. Detects the language, loads content, and
    /// runs an initial tree-sitter parse.
    pub fn from_path(id: BufferId, path: PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(&path)?;
        let mut buffer = Buffer::from_str(&content);
        buffer.modified = false;

        let lang = Lang::from_path(&path);
        let mut syntax = SyntaxHost::new();
        syntax.set_language(lang);
        syntax.reparse_rope(buffer.rope());
        let editorconfig = load_for_file(&path);
        let mut folds = FoldState::new();
        folds.refresh(buffer.rope(), &syntax.fold_ranges(buffer.rope()));

        Ok(Self {
            id,
            buffer,
            viewport: Viewport::new(),
            path: Some(path),
            syntax,
            lsp_state: LspState::new(),
            editorconfig,
            folds,
            snippet_session: None,
        })
    }

    /// Save the buffer to its current path.
    pub fn save(&mut self) -> anyhow::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No file path — use Save As"))?;
        let text = serialise_for_save(&self.buffer.to_string(), &self.editorconfig);
        std::fs::write(path, text)?;
        self.buffer.modified = false;
        Ok(())
    }

    /// Save to a new path and update the stored path.
    pub fn save_as(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let editorconfig = load_for_file(&path);
        let text = serialise_for_save(&self.buffer.to_string(), &editorconfig);
        std::fs::write(&path, text)?;
        // Re-detect language when the path changes.
        let new_lang = Lang::from_path(&path);
        if new_lang != self.syntax.language {
            self.syntax.set_language(new_lang);
            self.syntax.reparse_rope(self.buffer.rope());
        }
        self.path = Some(path);
        self.buffer.modified = false;
        self.editorconfig = editorconfig;
        Ok(())
    }

    /// Short display name used in the tab bar and status bar.
    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("[No Name]")
                .to_string(),
            None => "[No Name]".to_string(),
        }
    }

    /// Adjust the viewport so the primary cursor remains visible.
    pub fn scroll_to_cursor(&mut self, text_height: usize, text_width: usize) {
        self.viewport
            .scroll_to_cursor(&self.buffer, text_height, text_width);
    }

    /// Re-parse the buffer after an edit. Called from AppState::update().
    pub fn reparse(&mut self) {
        let rope = self.buffer.rope().clone();
        self.syntax.reparse_rope(&rope);
        let ranges = self.syntax.fold_ranges(&rope);
        self.folds.refresh(&rope, &ranges);
    }
}

/// Apply `.editorconfig`-driven serialisation tweaks to `text` before writing
/// to disk: trim trailing whitespace from each line, normalise EOLs, and add
/// a trailing newline when requested.
pub(crate) fn serialise_for_save(text: &str, overrides: &EditorConfigOverrides) -> String {
    use crate::editorconfig::EolStyle;
    let trim = overrides.trim_trailing_whitespace.unwrap_or(false);
    let final_nl = overrides.insert_final_newline.unwrap_or(false);
    let eol = overrides.end_of_line;

    // Split on `\n` so we can rebuild with the requested EOL. If the original
    // ends in `\n`, the split yields an empty trailing element.
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| {
            let l = l.strip_suffix('\r').unwrap_or(l);
            if trim {
                l.trim_end_matches([' ', '\t']).to_string()
            } else {
                l.to_string()
            }
        })
        .collect();

    // Determine the EOL string. If `end_of_line` is unset, leave the
    // existing line endings untouched (use `\n` since we just split on it,
    // which is the dominant form for files we open).
    let eol_str = match eol {
        Some(EolStyle::Lf) => "\n",
        Some(EolStyle::Crlf) => "\r\n",
        Some(EolStyle::Cr) => "\r",
        None => "\n",
    };

    // Drop the trailing empty element if it's present so we control whether
    // a final newline is added.
    let had_trailing_newline = matches!(lines.last(), Some(s) if s.is_empty());
    if had_trailing_newline {
        lines.pop();
    }

    let mut out = lines.join(eol_str);
    let want_final = if overrides.insert_final_newline.is_some() {
        final_nl
    } else {
        had_trailing_newline
    };
    if want_final && !out.is_empty() {
        out.push_str(eol_str);
    } else if want_final && out.is_empty() && !lines.is_empty() {
        // Edge case: a file containing only blank lines.
        out.push_str(eol_str);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editorconfig::EolStyle;
    use crate::formatting::IndentStyle;

    #[test]
    fn serialise_no_overrides_passes_text_through() {
        let s = serialise_for_save("hello\nworld\n", &EditorConfigOverrides::default());
        assert_eq!(s, "hello\nworld\n");
    }

    #[test]
    fn serialise_trim_trailing_whitespace_strips_spaces_and_tabs() {
        let o = EditorConfigOverrides {
            trim_trailing_whitespace: Some(true),
            ..Default::default()
        };
        let s = serialise_for_save("hello   \nworld\t\n", &o);
        assert_eq!(s, "hello\nworld\n");
    }

    #[test]
    fn serialise_insert_final_newline_adds_one_when_missing() {
        let o = EditorConfigOverrides {
            insert_final_newline: Some(true),
            ..Default::default()
        };
        let s = serialise_for_save("no_newline_here", &o);
        assert_eq!(s, "no_newline_here\n");
    }

    #[test]
    fn serialise_insert_final_newline_false_strips_one() {
        let o = EditorConfigOverrides {
            insert_final_newline: Some(false),
            ..Default::default()
        };
        let s = serialise_for_save("trailing\n", &o);
        assert_eq!(s, "trailing");
    }

    #[test]
    fn serialise_eol_crlf_replaces_lf() {
        let o = EditorConfigOverrides {
            end_of_line: Some(EolStyle::Crlf),
            ..Default::default()
        };
        let s = serialise_for_save("a\nb\nc\n", &o);
        assert_eq!(s, "a\r\nb\r\nc\r\n");
    }

    #[test]
    fn editorconfig_overrides_default_indent_style_is_unset() {
        let o = EditorConfigOverrides::default();
        assert!(o.indent_style.is_none());
        assert!(o.indent_size.is_none());
        // Sanity: IndentStyle is reachable through this module's imports.
        let _ = IndentStyle::Spaces;
    }
}
