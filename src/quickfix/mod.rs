//! Quickfix / location list — a workspace-level view over per-buffer LSP
//! diagnostics. The list is rebuilt on demand each time the user opens the
//! overlay so it always reflects the current diagnostic snapshots.
//!
//! Per the implementation plan, only LSP diagnostics are surfaced for now;
//! project-search results and external-command output are out of scope and
//! will be wired through the same list in a later release.

use std::path::PathBuf;

use crate::editor::Editor;
use crate::lsp::types::DiagSeverity;

/// A single entry in the quickfix list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickfixEntry {
    pub path: PathBuf,
    /// 0-based line number.
    pub line: usize,
    /// 0-based column (byte offset from line start).
    pub col: usize,
    pub message: String,
    pub severity: DiagSeverity,
}

/// State for the quickfix list overlay (Alt+1).
#[derive(Debug, Default)]
pub struct QuickfixState {
    pub entries: Vec<QuickfixEntry>,
    pub selected: usize,
}

impl QuickfixState {
    pub fn new(entries: Vec<QuickfixEntry>) -> Self {
        Self {
            entries,
            selected: 0,
        }
    }
}

/// Walk every open tab and collect each buffer's LSP diagnostics into a
/// single flat list. Sorted by (severity, path, line) so errors float to the
/// top. Buffers without a saved path are skipped — there's no useful jump
/// target.
pub fn collect_lsp_diagnostics(editor: &Editor) -> Vec<QuickfixEntry> {
    let mut out = Vec::new();
    for tab in &editor.tabs {
        let Some(path) = &tab.path else {
            continue;
        };
        for diag in &tab.lsp_state.diagnostics {
            let byte = diag.range.start;
            let rope = tab.buffer.rope();
            let char_idx = rope.byte_to_char(byte.min(rope.len_bytes()));
            let line = rope.char_to_line(char_idx);
            let line_start_char = rope.line_to_char(line);
            let col = char_idx.saturating_sub(line_start_char);
            out.push(QuickfixEntry {
                path: path.clone(),
                line,
                col,
                message: diag.message.clone(),
                severity: diag.severity,
            });
        }
    }
    out.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::cursor::ByteRange;
    use crate::editor::tab::BufferHandle;
    use crate::lsp::types::LspDiagnostic;

    fn make_tab(id: usize, path: Option<&str>, content: &str) -> BufferHandle {
        let mut h = BufferHandle::new_empty(id);
        h.buffer.insert_str(content);
        h.path = path.map(PathBuf::from);
        h
    }

    #[test]
    fn collect_is_sorted_by_severity_then_path() {
        let mut ed = Editor::new();
        ed.tabs.clear();
        let mut t1 = make_tab(0, Some("b.rs"), "fn b() {}\n");
        t1.lsp_state.diagnostics = vec![LspDiagnostic {
            range: ByteRange::new(0, 0),
            severity: DiagSeverity::Warning,
            message: "warn-b".into(),
            source: None,
        }];
        let mut t2 = make_tab(1, Some("a.rs"), "fn a() {}\n");
        t2.lsp_state.diagnostics = vec![LspDiagnostic {
            range: ByteRange::new(0, 0),
            severity: DiagSeverity::Error,
            message: "err-a".into(),
            source: None,
        }];
        ed.tabs.push(t1);
        ed.tabs.push(t2);
        let entries = collect_lsp_diagnostics(&ed);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "err-a");
        assert_eq!(entries[1].message, "warn-b");
    }

    #[test]
    fn collect_skips_unsaved_tabs() {
        let mut ed = Editor::new();
        ed.tabs.clear();
        let mut h = make_tab(0, None, "x\n");
        h.lsp_state.diagnostics = vec![LspDiagnostic {
            range: ByteRange::new(0, 0),
            severity: DiagSeverity::Error,
            message: "ignored".into(),
            source: None,
        }];
        ed.tabs.push(h);
        assert!(collect_lsp_diagnostics(&ed).is_empty());
    }
}
