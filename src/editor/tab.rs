use std::path::PathBuf;

use crate::buffer::Buffer;
use crate::editor::viewport::Viewport;
use crate::lsp::types::{DiagSeverity, LspDiagnostic, SemanticTokenSpan};
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

        Ok(Self {
            id,
            buffer,
            viewport: Viewport::new(),
            path: Some(path),
            syntax,
            lsp_state: LspState::new(),
        })
    }

    /// Save the buffer to its current path.
    pub fn save(&mut self) -> anyhow::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No file path — use Save As"))?;
        std::fs::write(path, self.buffer.to_string())?;
        self.buffer.modified = false;
        Ok(())
    }

    /// Save to a new path and update the stored path.
    pub fn save_as(&mut self, path: PathBuf) -> anyhow::Result<()> {
        std::fs::write(&path, self.buffer.to_string())?;
        // Re-detect language when the path changes.
        let new_lang = Lang::from_path(&path);
        if new_lang != self.syntax.language {
            self.syntax.set_language(new_lang);
            self.syntax.reparse_rope(self.buffer.rope());
        }
        self.path = Some(path);
        self.buffer.modified = false;
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
    }
}
