pub mod cursor;
pub mod edit;
pub mod history;

use ropey::Rope;

use crate::buffer::{
    cursor::{Cursor, MultiCursor, byte_col_at_display_col, line_byte_len_no_newline},
    edit as rope_edit,
    history::{EditCommand, UndoStack},
};

/// High-level text buffer.
///
/// Owns the rope, undo stack, and multi-cursor state. All edits go through
/// this struct so that history is always recorded consistently.
pub struct Buffer {
    rope: Rope,
    history: UndoStack,
    pub cursors: MultiCursor,
    /// True if the buffer has unsaved changes.
    pub modified: bool,
}

impl Buffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            history: UndoStack::new(),
            cursors: MultiCursor::new(),
            modified: false,
        }
    }

    /// Create a buffer pre-populated with `text`.
    /// Cursor starts at position 0; history is empty (loading a file is not undoable).
    pub fn from_str(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            history: UndoStack::new(),
            cursors: MultiCursor::new(),
            modified: false,
        }
    }

    // ------------------------------------------------------------------ //
    // Rope accessors (read-only)
    // ------------------------------------------------------------------ //

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    #[allow(dead_code)]
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Retrieve a single line as a `String` (without trailing newline).
    pub fn line_str(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let slice = self.rope.line(line);
        let s: String = slice.chars().collect();
        s.trim_end_matches(['\r', '\n']).to_string()
    }

    // ------------------------------------------------------------------ //
    // Undo / redo
    // ------------------------------------------------------------------ //

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Number of undo entries currently on the stack. Changes whenever the buffer
    /// content is modified (edit, undo, or redo) — useful for detecting actual edits.
    pub fn undo_depth(&self) -> usize {
        self.history.undo_depth()
    }

    /// Undo the most recent command (or batch). Returns the byte offset the cursor
    /// should land on after the undo.
    pub fn undo(&mut self) -> Option<usize> {
        let cmds = self.history.pop_undo()?;
        let mut cursor_pos = 0;
        // Apply commands in reverse order to undo them.
        for cmd in cmds.iter().rev() {
            cursor_pos = self.apply_inverse(cmd);
        }
        self.cursors = MultiCursor::with_cursor(Cursor::from_byte_offset(&self.rope, cursor_pos));
        self.modified = self.history.can_undo(); // heuristic: unmodified when undo stack empty
        Some(cursor_pos)
    }

    /// Redo the most recently undone command (or batch).
    pub fn redo(&mut self) -> Option<usize> {
        let cmds = self.history.pop_redo()?;
        let mut cursor_pos = 0;
        for cmd in &cmds {
            cursor_pos = self.apply_forward(cmd);
        }
        self.cursors = MultiCursor::with_cursor(Cursor::from_byte_offset(&self.rope, cursor_pos));
        self.modified = true;
        Some(cursor_pos)
    }

    /// Apply the *inverse* of a command (used by undo).
    fn apply_inverse(&mut self, cmd: &EditCommand) -> usize {
        match cmd {
            EditCommand::Insert { at, text } => {
                rope_edit::delete(&mut self.rope, *at, at + text.len());
                *at
            }
            EditCommand::Delete { start, deleted, .. } => {
                rope_edit::insert(&mut self.rope, *start, deleted);
                start + deleted.len()
            }
            EditCommand::Replace {
                start,
                end: _,
                old_text,
                new_text,
            } => {
                rope_edit::delete(&mut self.rope, *start, start + new_text.len());
                rope_edit::insert(&mut self.rope, *start, old_text);
                start + old_text.len()
            }
        }
    }

    /// Apply a command in the forward direction (used by redo).
    fn apply_forward(&mut self, cmd: &EditCommand) -> usize {
        match cmd {
            EditCommand::Insert { at, text } => {
                rope_edit::insert(&mut self.rope, *at, text);
                at + text.len()
            }
            EditCommand::Delete { start, end, .. } => {
                rope_edit::delete(&mut self.rope, *start, *end);
                *start
            }
            EditCommand::Replace {
                start,
                end,
                new_text,
                ..
            } => {
                rope_edit::delete(&mut self.rope, *start, *end);
                rope_edit::insert(&mut self.rope, *start, new_text);
                start + new_text.len()
            }
        }
    }

    // ------------------------------------------------------------------ //
    // Edit operations (primary cursor)
    // ------------------------------------------------------------------ //

    /// Insert a character at the primary cursor. If the cursor has a selection,
    /// the selection is deleted first.
    pub fn insert_char(&mut self, ch: char) {
        let mut s = String::with_capacity(ch.len_utf8());
        s.push(ch);
        self.insert_str(&s);
    }

    /// Insert a string at the primary cursor, replacing any active selection.
    pub fn insert_str(&mut self, text: &str) {
        let cursor = self.cursors.primary_mut();
        let at = if cursor.has_selection() {
            let range = cursor.selection_bytes();
            let deleted = rope_edit::delete(&mut self.rope, range.start, range.end);
            self.history.record(EditCommand::Delete {
                start: range.start,
                end: range.end,
                deleted,
            });
            cursor.byte_offset = range.start;
            cursor.selection = None;
            range.start
        } else {
            cursor.byte_offset
        };

        rope_edit::insert(&mut self.rope, at, text);
        self.history.record(EditCommand::Insert {
            at,
            text: text.to_string(),
        });

        // Move cursor to after the inserted text
        let new_offset = at + text.len();
        *self.cursors.primary_mut() = Cursor::from_byte_offset(&self.rope, new_offset);
        self.modified = true;
    }

    /// Delete the character before the primary cursor (Backspace).
    /// If there is a selection, delete the selection instead.
    pub fn delete_backward(&mut self) {
        let cursor = self.cursors.primary();
        if cursor.has_selection() {
            let range = cursor.selection_bytes();
            self.delete_range(range.start, range.end);
            return;
        }
        let at = cursor.byte_offset;
        if at == 0 {
            return;
        }
        let prev = rope_edit::prev_grapheme_boundary(&self.rope, at);
        self.delete_range(prev, at);
    }

    /// Delete the character at/after the primary cursor (Delete key).
    /// If there is a selection, delete the selection instead.
    pub fn delete_forward(&mut self) {
        let cursor = self.cursors.primary();
        if cursor.has_selection() {
            let range = cursor.selection_bytes();
            self.delete_range(range.start, range.end);
            return;
        }
        let at = cursor.byte_offset;
        if at >= self.rope.len_bytes() {
            return;
        }
        let next = rope_edit::next_grapheme_boundary(&self.rope, at);
        self.delete_range(at, next);
    }

    /// Delete bytes in `[start, end)` and move cursor to `start`.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        if start == end {
            return;
        }
        let deleted = rope_edit::delete(&mut self.rope, start, end);
        self.history.record(EditCommand::Delete {
            start,
            end,
            deleted,
        });
        *self.cursors.primary_mut() = Cursor::from_byte_offset(&self.rope, start);
        self.cursors.primary_mut().selection = None;
        self.modified = true;
    }

    /// Insert a newline at the primary cursor, applying auto-indent.
    pub fn insert_newline(&mut self) {
        let cursor = self.cursors.primary();
        let line = cursor.line;
        let indent = self.leading_indent(line);
        let prev_char = self.char_before_cursor(cursor.byte_offset);
        let extra = if matches!(prev_char, Some('{') | Some('(') | Some('[') | Some(':')) {
            self.indent_unit()
        } else {
            String::new()
        };
        let new_text = format!("\n{}{}", indent, extra);
        self.insert_str(&new_text);
    }

    /// Insert `tab_size` spaces at the primary cursor.
    pub fn insert_tab(&mut self, tab_size: usize) {
        let spaces = " ".repeat(tab_size.max(1));
        self.insert_str(&spaces);
    }

    /// Duplicate the current line (or selection).
    pub fn duplicate_line(&mut self) {
        let cursor = self.cursors.primary();
        let line = cursor.line;
        let line_start = self.line_start_byte(line);
        let line_end = self.line_end_byte_inclusive(line);
        let text: String = self
            .rope
            .slice(self.rope.byte_to_char(line_start)..self.rope.byte_to_char(line_end))
            .chars()
            .collect();
        rope_edit::insert(&mut self.rope, line_end, &text);
        self.history
            .record(EditCommand::Insert { at: line_end, text });
        self.modified = true;
    }

    /// Move the current line (or selected lines) up by one.
    pub fn move_line_up(&mut self) {
        let (line, col) = {
            let c = self.cursors.primary();
            (c.line, c.col)
        };
        if line == 0 {
            return;
        }
        self.swap_lines(line - 1, line);
        *self.cursors.primary_mut() = Cursor::from_line_col(&self.rope, line - 1, col);
        self.modified = true;
    }

    /// Move the current line down by one.
    pub fn move_line_down(&mut self) {
        let (line, col) = {
            let c = self.cursors.primary();
            (c.line, c.col)
        };
        let last = self.rope.len_lines().saturating_sub(1);
        if line >= last {
            return;
        }
        self.swap_lines(line, line + 1);
        *self.cursors.primary_mut() = Cursor::from_line_col(&self.rope, line + 1, col);
        self.modified = true;
    }

    // ------------------------------------------------------------------ //
    // Cursor movement (primary cursor)
    // ------------------------------------------------------------------ //

    /// Move the primary cursor, optionally extending the selection.
    pub fn move_cursor_to(&mut self, byte_offset: usize, extend: bool) {
        self.cursors
            .primary_mut()
            .move_to(&self.rope, byte_offset, extend);
    }

    pub fn move_cursor_left(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let offsets: Vec<usize> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| rope_edit::prev_grapheme_boundary(&self.rope, c.byte_offset))
                .collect();
            self.multi_apply_offsets(offsets, extend);
        } else {
            let at = self.cursors.primary().byte_offset;
            let prev = rope_edit::prev_grapheme_boundary(&self.rope, at);
            self.move_cursor_to(prev, extend);
        }
    }

    pub fn move_cursor_right(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let offsets: Vec<usize> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| rope_edit::next_grapheme_boundary(&self.rope, c.byte_offset))
                .collect();
            self.multi_apply_offsets(offsets, extend);
        } else {
            let at = self.cursors.primary().byte_offset;
            let next = rope_edit::next_grapheme_boundary(&self.rope, at);
            self.move_cursor_to(next, extend);
        }
    }

    pub fn move_cursor_up(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let moves: Vec<(usize, usize)> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| {
                    let preferred = c.preferred_col;
                    let target_line = c.line.saturating_sub(1);
                    let col = preferred.min(line_byte_len_no_newline(&self.rope, target_line));
                    let offset = self.rope.char_to_byte(self.rope.line_to_char(target_line)) + col;
                    (offset, preferred)
                })
                .collect();
            self.multi_apply_moves(moves, extend);
        } else {
            let cursor = self.cursors.primary();
            if cursor.line == 0 {
                self.move_cursor_to(0, extend);
                return;
            }
            let target_line = cursor.line - 1;
            let preferred = cursor.preferred_col;
            let col = preferred.min(line_byte_len_no_newline(&self.rope, target_line));
            let new_offset = self.rope.char_to_byte(self.rope.line_to_char(target_line)) + col;
            self.cursors
                .primary_mut()
                .move_to(&self.rope, new_offset, extend);
            // Restore preferred col — move_to recalculates it from display position.
            self.cursors.primary_mut().preferred_col = preferred;
        }
    }

    pub fn move_cursor_down(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let last_line = self.rope.len_lines().saturating_sub(1);
            let moves: Vec<(usize, usize)> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| {
                    let preferred = c.preferred_col;
                    let target_line = (c.line + 1).min(last_line);
                    let col = preferred.min(line_byte_len_no_newline(&self.rope, target_line));
                    let offset = self.rope.char_to_byte(self.rope.line_to_char(target_line)) + col;
                    (offset, preferred)
                })
                .collect();
            self.multi_apply_moves(moves, extend);
        } else {
            let cursor = self.cursors.primary();
            let last_line = self.rope.len_lines().saturating_sub(1);
            if cursor.line >= last_line {
                self.move_cursor_to(self.rope.len_bytes(), extend);
                return;
            }
            let target_line = cursor.line + 1;
            let preferred = cursor.preferred_col;
            let col = preferred.min(line_byte_len_no_newline(&self.rope, target_line));
            let new_offset = self.rope.char_to_byte(self.rope.line_to_char(target_line)) + col;
            self.cursors
                .primary_mut()
                .move_to(&self.rope, new_offset, extend);
            self.cursors.primary_mut().preferred_col = preferred;
        }
    }

    pub fn move_cursor_word_left(&mut self, extend: bool) {
        let at = self.cursors.primary().byte_offset;
        let prev = rope_edit::prev_word_boundary(&self.rope, at);
        self.move_cursor_to(prev, extend);
    }

    pub fn move_cursor_word_right(&mut self, extend: bool) {
        let at = self.cursors.primary().byte_offset;
        let next = rope_edit::next_word_boundary(&self.rope, at);
        self.move_cursor_to(next, extend);
    }

    pub fn move_cursor_home(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let offsets: Vec<usize> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| {
                    let line_start = self.line_start_byte(c.line);
                    let first_non_ws = self.first_non_whitespace_byte(c.line);
                    if c.byte_offset != first_non_ws {
                        first_non_ws
                    } else {
                        line_start
                    }
                })
                .collect();
            self.multi_apply_offsets(offsets, extend);
        } else {
            let cursor = self.cursors.primary();
            let line = cursor.line;
            let line_start = self.line_start_byte(line);
            let first_non_ws = self.first_non_whitespace_byte(line);
            // Smart home: if not already at first non-ws, go there; else go to column 0.
            let target = if cursor.byte_offset != first_non_ws {
                first_non_ws
            } else {
                line_start
            };
            self.move_cursor_to(target, extend);
        }
    }

    pub fn move_cursor_end(&mut self, extend: bool) {
        if self.cursors.is_multi() {
            let offsets: Vec<usize> = self
                .cursors
                .cursors()
                .iter()
                .map(|c| {
                    self.line_start_byte(c.line) + line_byte_len_no_newline(&self.rope, c.line)
                })
                .collect();
            self.multi_apply_offsets(offsets, extend);
        } else {
            let cursor = self.cursors.primary();
            let line = cursor.line;
            let end = self.line_start_byte(line) + line_byte_len_no_newline(&self.rope, line);
            self.move_cursor_to(end, extend);
        }
    }

    /// Apply pre-computed target offsets to all cursors and normalize.
    /// Used by multi-cursor movement where preferred_col can be recalculated from position.
    fn multi_apply_offsets(&mut self, offsets: Vec<usize>, extend: bool) {
        for (cursor, offset) in self.cursors.cursors_mut().iter_mut().zip(offsets) {
            cursor.move_to(&self.rope, offset, extend);
        }
        self.cursors.normalize();
    }

    /// Apply pre-computed `(offset, preferred_col)` moves to all cursors and normalize.
    /// Used by up/down movement where preferred_col must be preserved across short lines.
    fn multi_apply_moves(&mut self, moves: Vec<(usize, usize)>, extend: bool) {
        for (cursor, (offset, preferred)) in self.cursors.cursors_mut().iter_mut().zip(moves) {
            cursor.move_to(&self.rope, offset, extend);
            cursor.preferred_col = preferred;
        }
        self.cursors.normalize();
    }

    pub fn move_cursor_file_start(&mut self, extend: bool) {
        self.move_cursor_to(0, extend);
    }

    pub fn move_cursor_file_end(&mut self, extend: bool) {
        let end = self.rope.len_bytes();
        self.move_cursor_to(end, extend);
    }

    pub fn select_all(&mut self) {
        let end = self.rope.len_bytes();
        self.move_cursor_to(0, false);
        self.move_cursor_to(end, true);
    }

    // ------------------------------------------------------------------ //
    // Batch support (for multi-step operations like Replace All)
    // ------------------------------------------------------------------ //

    pub fn begin_batch(&mut self) {
        self.history.begin_batch();
    }

    pub fn commit_batch(&mut self) {
        self.history.commit_batch();
    }

    // ------------------------------------------------------------------ //
    // Multi-cursor operations
    // ------------------------------------------------------------------ //

    /// Add a cursor at the given `line` / `display_col` (terminal cell column).
    ///
    /// If the line is shorter than `display_col`, the cursor lands at the end
    /// of that line — the standard behaviour for column-edit mode.
    pub fn add_cursor_at_display_col(&mut self, line: usize, display_col: usize) {
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        let line_start_byte = self.rope.char_to_byte(self.rope.line_to_char(line));
        let byte_within_line = byte_col_at_display_col(&self.rope, line, display_col);
        let byte_offset = (line_start_byte + byte_within_line).min(self.rope.len_bytes());
        self.cursors.add_cursor(&self.rope, byte_offset);
    }

    /// Collapse all cursors to only the primary cursor.
    pub fn collapse_cursors(&mut self) {
        self.cursors.collapse_to_primary();
    }

    /// Insert `ch` at every cursor position (multi-cursor broadcast).
    ///
    /// Cursors are processed in descending byte order so that earlier
    /// byte offsets remain valid when we work down the list.  Falls back
    /// to the single-cursor path when only one cursor is active.
    pub fn multi_insert_char(&mut self, ch: char) {
        let mut s = String::with_capacity(ch.len_utf8());
        s.push(ch);
        self.multi_insert_str(&s);
    }

    /// Insert `text` at every cursor position (multi-cursor broadcast).
    pub fn multi_insert_str(&mut self, text: &str) {
        if !self.cursors.is_multi() {
            self.insert_str(text);
            return;
        }
        self.multi_insert_str_impl(text);
    }

    fn multi_insert_str_impl(&mut self, text: &str) {
        struct Op {
            cursor_idx: usize,
            ins_pt: usize,  // byte offset where the insert will happen
            del_end: usize, // == ins_pt unless there's a selection to delete first
        }

        let primary_cursor_idx = self.cursors.primary_idx();
        let n = self.cursors.len();

        // Collect all op data while `self.cursors` is immutably borrowed.
        let mut ops: Vec<Op> = self
            .cursors
            .cursors()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if c.has_selection() {
                    let r = c.selection_bytes();
                    Op {
                        cursor_idx: i,
                        ins_pt: r.start,
                        del_end: r.end,
                    }
                } else {
                    Op {
                        cursor_idx: i,
                        ins_pt: c.byte_offset,
                        del_end: c.byte_offset,
                    }
                }
            })
            .collect();

        // Process descending so higher-offset inserts don't shift lower positions.
        ops.sort_by(|a, b| b.ins_pt.cmp(&a.ins_pt));

        // new_positions[cursor_idx] = byte offset after the edit.
        let mut new_positions = vec![0usize; n];

        self.history.begin_batch();
        for op in &ops {
            if op.del_end > op.ins_pt {
                let deleted = rope_edit::delete(&mut self.rope, op.ins_pt, op.del_end);
                self.history.record(EditCommand::Delete {
                    start: op.ins_pt,
                    end: op.del_end,
                    deleted,
                });
            }
            rope_edit::insert(&mut self.rope, op.ins_pt, text);
            self.history.record(EditCommand::Insert {
                at: op.ins_pt,
                text: text.to_string(),
            });
            new_positions[op.cursor_idx] = op.ins_pt + text.len();
        }
        self.history.commit_batch();

        let primary_new = new_positions[primary_cursor_idx];
        let new_cursors: Vec<Cursor> = new_positions
            .iter()
            .map(|&off| Cursor::from_byte_offset(&self.rope, off))
            .collect();
        let primary_idx = new_cursors
            .iter()
            .position(|c| c.byte_offset == primary_new)
            .unwrap_or(0);
        self.cursors = MultiCursor::from_cursors_with_primary(new_cursors, primary_idx);
        self.modified = true;
    }

    /// Delete one grapheme backward at every cursor (multi-cursor broadcast).
    ///
    /// If a cursor has a selection, the selection is deleted instead.
    /// Cursors at byte 0 with no selection are silently skipped.
    pub fn multi_delete_backward(&mut self) {
        if !self.cursors.is_multi() {
            self.delete_backward();
            return;
        }

        struct DelOp {
            cursor_idx: usize,
            del_start: usize,
            del_end: usize,
        }

        let primary_cursor_idx = self.cursors.primary_idx();

        let mut ops: Vec<DelOp> = self
            .cursors
            .cursors()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if c.has_selection() {
                    let r = c.selection_bytes();
                    if r.start < r.end {
                        Some(DelOp {
                            cursor_idx: i,
                            del_start: r.start,
                            del_end: r.end,
                        })
                    } else {
                        None
                    }
                } else {
                    let prev = rope_edit::prev_grapheme_boundary(&self.rope, c.byte_offset);
                    if prev < c.byte_offset {
                        Some(DelOp {
                            cursor_idx: i,
                            del_start: prev,
                            del_end: c.byte_offset,
                        })
                    } else {
                        None
                    }
                }
            })
            .collect();

        if ops.is_empty() {
            return;
        }

        // Descending so higher-offset deletes don't affect lower positions.
        ops.sort_by(|a, b| b.del_start.cmp(&a.del_start));

        // Start with current positions (unchanged for cursors with no op).
        let mut new_positions: Vec<usize> = self
            .cursors
            .cursors()
            .iter()
            .map(|c| c.byte_offset)
            .collect();

        self.history.begin_batch();
        for op in &ops {
            let deleted = rope_edit::delete(&mut self.rope, op.del_start, op.del_end);
            self.history.record(EditCommand::Delete {
                start: op.del_start,
                end: op.del_end,
                deleted,
            });
            new_positions[op.cursor_idx] = op.del_start;
        }
        self.history.commit_batch();

        let primary_new = new_positions[primary_cursor_idx];
        let new_cursors: Vec<Cursor> = new_positions
            .iter()
            .map(|&off| Cursor::from_byte_offset(&self.rope, off))
            .collect();
        let primary_idx = new_cursors
            .iter()
            .position(|c| c.byte_offset == primary_new)
            .unwrap_or(0);
        self.cursors = MultiCursor::from_cursors_with_primary(new_cursors, primary_idx);
        self.modified = true;
    }

    /// Delete one grapheme forward at every cursor (multi-cursor broadcast).
    ///
    /// If a cursor has a selection, the selection is deleted instead.
    /// Cursors at end-of-file with no selection are silently skipped.
    pub fn multi_delete_forward(&mut self) {
        if !self.cursors.is_multi() {
            self.delete_forward();
            return;
        }

        struct DelOp {
            cursor_idx: usize,
            del_start: usize,
            del_end: usize,
        }

        let primary_cursor_idx = self.cursors.primary_idx();

        let mut ops: Vec<DelOp> = self
            .cursors
            .cursors()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if c.has_selection() {
                    let r = c.selection_bytes();
                    if r.start < r.end {
                        Some(DelOp {
                            cursor_idx: i,
                            del_start: r.start,
                            del_end: r.end,
                        })
                    } else {
                        None
                    }
                } else {
                    let next = rope_edit::next_grapheme_boundary(&self.rope, c.byte_offset);
                    if next > c.byte_offset {
                        Some(DelOp {
                            cursor_idx: i,
                            del_start: c.byte_offset,
                            del_end: next,
                        })
                    } else {
                        None
                    }
                }
            })
            .collect();

        if ops.is_empty() {
            return;
        }

        ops.sort_by(|a, b| b.del_start.cmp(&a.del_start));

        let mut new_positions: Vec<usize> = self
            .cursors
            .cursors()
            .iter()
            .map(|c| c.byte_offset)
            .collect();

        self.history.begin_batch();
        for op in &ops {
            let deleted = rope_edit::delete(&mut self.rope, op.del_start, op.del_end);
            self.history.record(EditCommand::Delete {
                start: op.del_start,
                end: op.del_end,
                deleted,
            });
            new_positions[op.cursor_idx] = op.del_start;
        }
        self.history.commit_batch();

        let primary_new = new_positions[primary_cursor_idx];
        let new_cursors: Vec<Cursor> = new_positions
            .iter()
            .map(|&off| Cursor::from_byte_offset(&self.rope, off))
            .collect();
        let primary_idx = new_cursors
            .iter()
            .position(|c| c.byte_offset == primary_new)
            .unwrap_or(0);
        self.cursors = MultiCursor::from_cursors_with_primary(new_cursors, primary_idx);
        self.modified = true;
    }

    // ------------------------------------------------------------------ //
    // Helpers
    // ------------------------------------------------------------------ //

    fn line_start_byte(&self, line: usize) -> usize {
        self.rope.char_to_byte(self.rope.line_to_char(line))
    }

    /// Byte offset just past the end of `line` (including newline character).
    fn line_end_byte_inclusive(&self, line: usize) -> usize {
        let next_line = line + 1;
        if next_line >= self.rope.len_lines() {
            self.rope.len_bytes()
        } else {
            self.rope.char_to_byte(self.rope.line_to_char(next_line))
        }
    }

    fn first_non_whitespace_byte(&self, line: usize) -> usize {
        let start = self.line_start_byte(line);
        let s = self.line_str(line);
        let ws_bytes: usize = s
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(|c| c.len_utf8())
            .sum();
        start + ws_bytes
    }

    fn leading_indent(&self, line: usize) -> String {
        let s = self.line_str(line);
        let ws: String = s.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        ws
    }

    fn indent_unit(&self) -> String {
        // 4 spaces by default; Phase 8 will read from config.
        "    ".to_string()
    }

    fn char_before_cursor(&self, byte_offset: usize) -> Option<char> {
        if byte_offset == 0 {
            return None;
        }
        let char_offset = self.rope.byte_to_char(byte_offset);
        if char_offset == 0 {
            return None;
        }
        Some(self.rope.char(char_offset - 1))
    }

    fn swap_lines(&mut self, a: usize, b: usize) {
        debug_assert!(a < b);
        let line_a = self.line_str(a);
        let line_b = self.line_str(b);
        let a_start = self.line_start_byte(a);
        let a_end = a_start + line_a.len();
        let b_start = self.line_start_byte(b);
        let b_end = b_start + line_b.len();

        // Replace b first (higher offset) so a's offsets stay valid.
        let old_b = rope_edit::replace(&mut self.rope, b_start, b_end, &line_a);
        self.history.record(EditCommand::Replace {
            start: b_start,
            end: b_end,
            old_text: old_b,
            new_text: line_a.clone(),
        });
        let old_a = rope_edit::replace(&mut self.rope, a_start, a_end, &line_b);
        self.history.record(EditCommand::Replace {
            start: a_start,
            end: a_end,
            old_text: old_a,
            new_text: line_b,
        });
    }
}

impl std::fmt::Display for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.rope)
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_read() {
        let mut buf = Buffer::new();
        buf.insert_str("hello");
        assert_eq!(buf.to_string(), "hello");
        assert!(buf.modified);
    }

    #[test]
    fn insert_char_sequence() {
        let mut buf = Buffer::new();
        for ch in "hello".chars() {
            buf.insert_char(ch);
        }
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn delete_backward() {
        let mut buf = Buffer::from_str("hello");
        buf.move_cursor_to(5, false);
        buf.delete_backward();
        assert_eq!(buf.to_string(), "hell");
        assert_eq!(buf.cursors.primary().byte_offset, 4);
    }

    #[test]
    fn delete_forward() {
        let mut buf = Buffer::from_str("hello");
        buf.move_cursor_to(0, false);
        buf.delete_forward();
        assert_eq!(buf.to_string(), "ello");
        assert_eq!(buf.cursors.primary().byte_offset, 0);
    }

    #[test]
    fn undo_insert() {
        let mut buf = Buffer::new();
        buf.insert_str("hello");
        assert_eq!(buf.to_string(), "hello");
        buf.undo();
        assert_eq!(buf.to_string(), "");
    }

    #[test]
    fn undo_delete() {
        let mut buf = Buffer::from_str("hello world");
        buf.move_cursor_to(5, false);
        buf.delete_backward(); // deletes 'o'
        assert_eq!(buf.to_string(), "hell world");
        buf.undo();
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn redo_after_undo() {
        let mut buf = Buffer::new();
        buf.insert_str("hello");
        buf.undo();
        assert_eq!(buf.to_string(), "");
        buf.redo();
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn undo_redo_sequence() {
        let mut buf = Buffer::new();
        buf.insert_str("a");
        buf.insert_str("b");
        buf.insert_str("c");
        assert_eq!(buf.to_string(), "abc");
        buf.undo();
        assert_eq!(buf.to_string(), "ab");
        buf.undo();
        assert_eq!(buf.to_string(), "a");
        buf.redo();
        assert_eq!(buf.to_string(), "ab");
        buf.redo();
        assert_eq!(buf.to_string(), "abc");
    }

    #[test]
    fn insert_unicode_emoji() {
        let mut buf = Buffer::new();
        buf.insert_str("hi ");
        buf.insert_char('😀'); // 4-byte emoji
        assert_eq!(buf.to_string(), "hi 😀");
        buf.delete_backward();
        assert_eq!(buf.to_string(), "hi ");
    }

    #[test]
    fn cursor_movement() {
        let mut buf = Buffer::from_str("hello\nworld");
        buf.move_cursor_to(0, false);
        buf.move_cursor_right(false);
        assert_eq!(buf.cursors.primary().byte_offset, 1);
        buf.move_cursor_down(false);
        // Should be on line 1, col 1 (byte 7: 6 + 1)
        assert_eq!(buf.cursors.primary().line, 1);
        assert_eq!(buf.cursors.primary().col, 1);
    }

    #[test]
    fn selection_with_shift() {
        let mut buf = Buffer::from_str("hello");
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(5, true); // shift-end
        assert!(buf.cursors.primary().has_selection());
        let range = buf.cursors.primary().selection_bytes();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 5);
    }

    #[test]
    fn insert_replaces_selection() {
        let mut buf = Buffer::from_str("hello world");
        buf.move_cursor_to(6, false);
        buf.move_cursor_to(11, true); // select "world"
        buf.insert_str("Rust");
        assert_eq!(buf.to_string(), "hello Rust");
    }

    #[test]
    fn select_all() {
        let mut buf = Buffer::from_str("hello world");
        buf.select_all();
        let range = buf.cursors.primary().selection_bytes();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 11);
    }

    #[test]
    fn newline_with_auto_indent() {
        let mut buf = Buffer::from_str("    hello");
        buf.move_cursor_to(9, false); // end of line
        buf.insert_newline();
        // New line should have the same 4-space indent
        assert!(buf.to_string().starts_with("    hello\n    "));
    }

    #[test]
    fn newline_after_brace_increases_indent() {
        let mut buf = Buffer::from_str("fn foo() {");
        buf.move_cursor_to(10, false);
        buf.insert_newline();
        let content = buf.to_string();
        // Should add one extra indent level after '{'
        assert!(content.contains("fn foo() {\n    "));
    }

    #[test]
    fn move_line_up_and_down() {
        let mut buf = Buffer::from_str("line1\nline2\nline3");
        // Move cursor to line 1
        buf.move_cursor_to(6, false);
        assert_eq!(buf.cursors.primary().line, 1);
        buf.move_line_up();
        // "line2" should now be on line 0
        assert_eq!(buf.line_str(0), "line2");
        assert_eq!(buf.line_str(1), "line1");
    }

    #[test]
    fn from_str_not_modified() {
        let buf = Buffer::from_str("hello");
        assert!(!buf.modified);
    }

    #[test]
    fn word_navigation() {
        let mut buf = Buffer::from_str("hello world foo");
        buf.move_cursor_to(0, false);
        buf.move_cursor_word_right(false);
        assert_eq!(buf.cursors.primary().byte_offset, 6);
        buf.move_cursor_word_right(false);
        assert_eq!(buf.cursors.primary().byte_offset, 12);
    }

    #[test]
    fn smart_home() {
        let mut buf = Buffer::from_str("    hello");
        buf.move_cursor_to(9, false); // end of line
        buf.move_cursor_home(false);
        // Should jump to first non-whitespace (byte 4)
        assert_eq!(buf.cursors.primary().byte_offset, 4);
        buf.move_cursor_home(false);
        // Second press: jump to column 0
        assert_eq!(buf.cursors.primary().byte_offset, 0);
    }

    #[test]
    fn batch_undo() {
        let mut buf = Buffer::new();
        buf.begin_batch();
        buf.insert_str("hello");
        buf.insert_str(" world");
        buf.commit_batch();
        assert_eq!(buf.to_string(), "hello world");
        assert_eq!(buf.history.undo_depth(), 1);
        buf.undo();
        assert_eq!(buf.to_string(), "");
    }

    #[test]
    fn insert_tab_default_size() {
        let mut buf = Buffer::new();
        buf.insert_tab(4);
        assert_eq!(buf.to_string(), "    ");
        assert_eq!(buf.cursors.primary().byte_offset, 4);
    }

    #[test]
    fn insert_tab_custom_size() {
        let mut buf = Buffer::new();
        buf.insert_tab(2);
        assert_eq!(buf.to_string(), "  ");
    }

    #[test]
    fn insert_tab_size_one() {
        let mut buf = Buffer::new();
        buf.insert_tab(1);
        assert_eq!(buf.to_string(), " ");
    }

    // ── Multi-cursor tests ────────────────────────────────────────────────

    #[test]
    fn multi_insert_char_two_cursors() {
        // "hello\nworld" — one cursor at col 0 of each line
        let mut buf = Buffer::from_str("hello\nworld");
        buf.move_cursor_to(0, false); // line 0
        buf.add_cursor_at_display_col(1, 0); // line 1 col 0
        assert_eq!(buf.cursors.len(), 2);
        buf.multi_insert_char('X');
        assert_eq!(buf.to_string(), "Xhello\nXworld");
    }

    #[test]
    fn multi_insert_single_cursor_delegates() {
        // With only one cursor, multi_insert_char should behave like insert_char
        let mut buf = Buffer::from_str("hello");
        buf.move_cursor_to(0, false);
        assert!(!buf.cursors.is_multi());
        buf.multi_insert_char('X');
        assert_eq!(buf.to_string(), "Xhello");
    }

    #[test]
    fn multi_delete_backward_two_cursors() {
        let mut buf = Buffer::from_str("abc\ndef");
        // Cursors after 'c' on line 0 (offset 3) and after 'd' on line 1 (offset 5)
        buf.move_cursor_to(3, false);
        buf.add_cursor_at_display_col(1, 1); // col 1 on line 1 = after 'd'
        assert_eq!(buf.cursors.len(), 2);
        buf.multi_delete_backward();
        assert_eq!(buf.to_string(), "ab\nef");
    }

    #[test]
    fn multi_delete_forward_two_cursors() {
        let mut buf = Buffer::from_str("abc\ndef");
        // Cursors at col 0 of each line
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 0);
        buf.multi_delete_forward();
        assert_eq!(buf.to_string(), "bc\nef");
    }

    #[test]
    fn add_cursor_at_display_col_short_line() {
        // Line 1 is shorter than display_col — cursor should land at end of line
        let mut buf = Buffer::from_str("hello world\nhi");
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 10); // line 1 only has 2 chars
        assert_eq!(buf.cursors.len(), 2);
        let c = buf.cursors.cursors().iter().find(|c| c.line == 1).unwrap();
        assert_eq!(c.col, 2); // clamped to end of "hi"
    }

    #[test]
    fn collapse_cursors() {
        let mut buf = Buffer::from_str("hello\nworld");
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 0);
        assert_eq!(buf.cursors.len(), 2);
        buf.collapse_cursors();
        assert_eq!(buf.cursors.len(), 1);
    }

    #[test]
    fn multi_insert_undo() {
        // After multi-cursor insert, a single undo should remove all inserted chars.
        let mut buf = Buffer::from_str("abc\ndef");
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 0);
        buf.multi_insert_char('X');
        assert_eq!(buf.to_string(), "Xabc\nXdef");
        buf.undo();
        assert_eq!(buf.to_string(), "abc\ndef");
    }
}
