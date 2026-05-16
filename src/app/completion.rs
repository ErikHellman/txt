use std::path::PathBuf;

use crate::input::action::{Direction, EditorAction};

use super::util::{completion_kind_label, extract_hover_text, parse_locations, same_file};
use super::{
    AppState, CompletionItemEntry, CompletionState, HoverState, ReferenceItem, ReferencesListState,
};

impl AppState {
    pub(super) fn trigger_completion(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let anchor_byte = cursor.byte_offset;
        let anchor_line = cursor.line;
        let anchor_col = cursor.col;

        let _ = registry
            .client_mut()
            .request_completion(&uri, pos.line, pos.character);

        self.completion = Some(CompletionState::new(anchor_byte, anchor_line, anchor_col));
    }
    pub(super) fn handle_completion_input(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(c) = &mut self.completion {
                    c.selected = c.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(c) = &mut self.completion
                    && !c.filtered.is_empty()
                {
                    c.selected = (c.selected + 1).min(c.filtered.len() - 1);
                }
                true
            }
            EditorAction::InsertNewline | EditorAction::InsertTab => {
                self.accept_completion();
                true
            }
            EditorAction::CloseSearch => {
                self.completion = None;
                true
            }
            // Cursor movement dismisses completion.
            EditorAction::MoveCursor(_)
            | EditorAction::MoveCursorWord(_)
            | EditorAction::MoveCursorHome
            | EditorAction::MoveCursorEnd => {
                self.completion = None;
                false // let the movement fall through
            }
            // Characters fall through to editing, then refilter.
            _ => false,
        }
    }
    pub(super) fn accept_completion(&mut self) {
        let insert_text = match &self.completion {
            Some(c) => match c.selected_item() {
                Some(item) => item.insert_text.clone(),
                None => {
                    self.completion = None;
                    return;
                }
            },
            None => return,
        };
        let anchor = self.completion.as_ref().unwrap().anchor_byte;
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;

        // Delete the typed prefix and insert the completion text.
        if cursor_byte > anchor {
            let rope = self.editor.active().buffer.rope();
            let start_char = rope.byte_to_char(anchor);
            let end_char = rope.byte_to_char(cursor_byte);
            self.editor
                .active_mut()
                .buffer
                .delete_range(start_char, end_char);
        }
        self.editor.active_mut().buffer.insert_str(&insert_text);

        self.completion = None;
    }
    pub(super) fn refilter_completion(&mut self) {
        let Some(comp) = &mut self.completion else {
            return;
        };
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        if cursor_byte < comp.anchor_byte {
            self.completion = None;
            return;
        }
        let rope = self.editor.active().buffer.rope();
        let start = rope.byte_to_char(comp.anchor_byte);
        let end = rope.byte_to_char(cursor_byte);
        let prefix: String = rope.slice(start..end).chars().collect();
        comp.filter(&prefix);
    }
    pub(super) fn apply_completion_response(&mut self, items: Vec<serde_json::Value>) {
        let Some(comp) = &mut self.completion else {
            return;
        };
        comp.items = items
            .iter()
            .map(|item| {
                let label = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let detail = item
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let insert_text = item
                    .get("insertText")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&label)
                    .to_string();
                let filter_text = item
                    .get("filterText")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&label)
                    .to_string();
                let kind = item.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
                let kind_label = completion_kind_label(kind);
                CompletionItemEntry {
                    label,
                    detail,
                    insert_text,
                    filter_text,
                    kind_label,
                }
            })
            .collect();
        comp.filtered = (0..comp.items.len()).collect();
        self.refilter_completion();
    }
    pub(super) fn trigger_hover(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.hover_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let anchor_line = cursor.line;
        let anchor_col = cursor.col;

        let _ = registry
            .client_mut()
            .request_hover(&uri, pos.line, pos.character);

        // We'll set hover state when the response arrives.
        let _ = (anchor_line, anchor_col);
    }
    pub(super) fn apply_hover_response(&mut self, contents: Option<serde_json::Value>) {
        let Some(contents) = contents else { return };
        let text = extract_hover_text(&contents);
        if text.is_empty() {
            return;
        }
        let cursor = self.editor.active().buffer.cursors.primary();
        self.hover = Some(HoverState {
            content: text,
            anchor_line: cursor.line,
            anchor_col: cursor.col,
        });
    }
    pub(super) fn trigger_go_to_definition(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.definition_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_definition(&uri, pos.line, pos.character);
    }
    pub(super) fn apply_definition_response(&mut self, locations: serde_json::Value) {
        let locs = parse_locations(&locations);
        if locs.is_empty() {
            return;
        }
        if locs.len() == 1 {
            self.jump_to_location(&locs[0]);
        } else {
            self.show_references_list(locs);
        }
    }
    pub(super) fn trigger_find_references(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.references_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_references(&uri, pos.line, pos.character);
    }
    pub(super) fn apply_references_response(&mut self, locations: serde_json::Value) {
        let locs = parse_locations(&locations);
        if locs.is_empty() {
            return;
        }
        self.show_references_list(locs);
    }
    pub(super) fn show_references_list(&mut self, locs: Vec<(PathBuf, usize, usize)>) {
        let items: Vec<ReferenceItem> = locs
            .into_iter()
            .map(|(path, line, col)| {
                let context = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.lines().nth(line).map(|l| l.trim().to_string()))
                    .unwrap_or_default();
                ReferenceItem {
                    path,
                    line,
                    col,
                    context,
                }
            })
            .collect();
        self.references_list = Some(ReferencesListState { items, selected: 0 });
    }
    /// Walk to the next/previous quickfix entry and jump to it. Does not
    /// open the overlay — just navigates the list when it is already
    /// populated. Builds a fresh list from current diagnostics if the
    /// overlay state is empty.
    pub(super) fn quickfix_step(&mut self, step: i32) {
        if self.quickfix.is_none() {
            let entries = crate::quickfix::collect_lsp_diagnostics(&self.editor);
            if entries.is_empty() {
                self.status_error = Some("No diagnostics".into());
                return;
            }
            self.quickfix = Some(crate::quickfix::QuickfixState::new(entries));
        }
        let entry_opt = {
            let qf = self.quickfix.as_mut().unwrap();
            let n = qf.entries.len();
            if n == 0 {
                None
            } else {
                let i = if step > 0 {
                    (qf.selected + 1) % n
                } else {
                    (qf.selected + n - 1) % n
                };
                qf.selected = i;
                Some(qf.entries[i].clone())
            }
        };
        if let Some(e) = entry_opt {
            self.jump_to_location(&(e.path, e.line, e.col));
        }
    }
    /// Handle input while the quickfix list overlay is open.
    pub(super) fn handle_quickfix_input(&mut self, action: &EditorAction) -> bool {
        let n = self.quickfix.as_ref().map(|q| q.entries.len()).unwrap_or(0);
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(q) = &mut self.quickfix {
                    q.selected = q.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(q) = &mut self.quickfix {
                    q.selected = (q.selected + 1).min(n.saturating_sub(1));
                }
                true
            }
            EditorAction::InsertNewline => {
                let target = self.quickfix.as_ref().and_then(|q| {
                    q.entries
                        .get(q.selected)
                        .map(|e| (e.path.clone(), e.line, e.col))
                });
                self.quickfix = None;
                if let Some(loc) = target {
                    self.jump_to_location(&loc);
                }
                true
            }
            EditorAction::CloseSearch => {
                self.quickfix = None;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.quickfix = None;
                false
            }
            _ => false,
        }
    }
    /// Handle input while the clipboard-ring picker is open. Returns `true`
    /// when the action was consumed, `false` to allow global routing.
    pub(super) fn handle_clipboard_ring(&mut self, action: &EditorAction) -> bool {
        let num_items = self
            .clipboard_ring
            .as_ref()
            .map(|r| r.entries.len())
            .unwrap_or(0);
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(r) = &mut self.clipboard_ring {
                    r.selected = r.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(r) = &mut self.clipboard_ring {
                    r.selected = (r.selected + 1).min(num_items.saturating_sub(1));
                }
                true
            }
            EditorAction::InsertNewline => {
                if let Some(r) = &self.clipboard_ring {
                    let idx = r.selected;
                    if let Some(text) = self.clipboard.pick(idx) {
                        // Reuse the buffer's insert_str — it already handles
                        // replacing the active selection, exactly like Paste.
                        self.editor.active_mut().buffer.insert_str(&text);
                    }
                }
                self.clipboard_ring = None;
                true
            }
            EditorAction::CloseSearch => {
                self.clipboard_ring = None;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.clipboard_ring = None;
                false
            }
            _ => false,
        }
    }
    pub(super) fn handle_references_input(&mut self, action: &EditorAction) -> bool {
        let num_items = self
            .references_list
            .as_ref()
            .map(|r| r.items.len())
            .unwrap_or(0);
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(r) = &mut self.references_list {
                    r.selected = r.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(r) = &mut self.references_list {
                    r.selected = (r.selected + 1).min(num_items.saturating_sub(1));
                }
                true
            }
            EditorAction::InsertNewline => {
                if let Some(r) = &self.references_list
                    && let Some(item) = r.items.get(r.selected)
                {
                    let path = item.path.clone();
                    let line = item.line;
                    let col = item.col;
                    self.references_list = None;
                    self.jump_to_location(&(path, line, col));
                }
                true
            }
            EditorAction::CloseSearch => {
                self.references_list = None;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.references_list = None;
                false
            }
            _ => false,
        }
    }
    pub(super) fn jump_to_location(&mut self, loc: &(PathBuf, usize, usize)) {
        let (path, line, col) = loc;

        // Check if file is already open in a tab.
        let existing = self
            .editor
            .tabs
            .iter()
            .position(|t| t.path.as_ref().is_some_and(|p| same_file(p, path)));

        if let Some(idx) = existing {
            self.editor.go_to_tab(idx);
        } else {
            // Open in new tab.
            if self.editor.open_tab(path.clone()).is_err() {
                return;
            }
            self.after_file_open_or_save();
        }

        // Jump cursor to position.
        let rope = self.editor.active().buffer.rope().clone();
        let cursor = crate::buffer::cursor::Cursor::from_line_col(&rope, *line, *col);
        *self.editor.active_mut().buffer.cursors.primary_mut() = cursor;
    }
}
