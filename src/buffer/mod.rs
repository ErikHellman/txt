pub mod cursor;
pub mod edit;
pub mod history;

use ropey::Rope;

use crate::buffer::{
    cursor::{
        Cursor, MultiCursor, byte_col_at_display_col, line_byte_len_no_newline, word_span_at,
    },
    edit as rope_edit,
    history::{EditCommand, UndoStack},
};
use crate::formatting::{IndentConfig, IndentRules, IndentStyle};

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

    /// Insert a newline at the primary cursor, applying language-aware
    /// auto-indent.
    ///
    /// Copies the leading whitespace from the current line and adds one extra
    /// indent level when the character before the cursor matches one of the
    /// language's `increase_after` triggers (e.g. `{ ( [` for C-family,
    /// `:` for Python).
    pub fn insert_newline(&mut self, indent: &IndentConfig, rules: IndentRules) {
        let cursor = self.cursors.primary();
        let line = cursor.line;
        let leading = self.leading_indent(line);
        let prev_char = self.char_before_cursor(cursor.byte_offset);
        let bump = matches!(prev_char, Some(c) if rules.increase_after.contains(&c));
        let extra = if bump {
            indent.one_level()
        } else {
            String::new()
        };
        let new_text = format!("\n{leading}{extra}");
        self.insert_str(&new_text);
    }

    /// Insert one indent level at the primary cursor.
    ///
    /// With spaces: smart-tab from the current display column to the next
    /// multiple of `width` (so Tab on column 3 with width 4 inserts one
    /// space). With tabs: inserts a single `\t`.
    pub fn insert_tab(&mut self, indent: &IndentConfig) {
        match indent.style {
            IndentStyle::Tabs => {
                self.insert_str("\t");
            }
            IndentStyle::Spaces => {
                let width = indent.width.max(1);
                let display_col = self.cursors.primary().preferred_col;
                let count = width - (display_col % width);
                self.insert_str(&" ".repeat(count));
            }
        }
    }

    /// Indent every line touched by the primary cursor's selection (or the
    /// current line if there's no selection) by one indent level.
    /// Recorded as a single undo entry. The cursor and selection move with
    /// their lines.
    pub fn indent_lines(&mut self, indent: &IndentConfig) {
        let unit = indent.one_level();
        let unit_bytes = unit.len();
        let (first_line, last_line) = self.touched_line_range();

        let primary = self.cursors.primary();
        let original_offset = primary.byte_offset;
        let original_sel = primary.selection;
        let mut tracked: Vec<usize> = vec![original_offset];
        if let Some(s) = original_sel {
            tracked.push(s.anchor);
            tracked.push(s.active);
        }

        self.history.begin_batch();
        // Insert at each line's start, descending so earlier line offsets stay
        // valid for the next iteration.
        for line in (first_line..=last_line).rev() {
            let start = self.line_start_byte(line);
            rope_edit::insert(&mut self.rope, start, &unit);
            self.history.record(EditCommand::Insert {
                at: start,
                text: unit.clone(),
            });
            for off in tracked.iter_mut() {
                if *off >= start {
                    *off += unit_bytes;
                }
            }
        }
        self.history.commit_batch();

        let new_offset = tracked[0];
        *self.cursors.primary_mut() = Cursor::from_byte_offset(&self.rope, new_offset);
        if original_sel.is_some() {
            let new_sel = crate::buffer::cursor::Selection::new(tracked[1], tracked[2]);
            self.cursors.primary_mut().selection = Some(new_sel);
        }
        self.modified = true;
    }

    /// Dedent every line touched by the primary cursor's selection (or the
    /// current line) by one indent level. Lines without enough leading
    /// whitespace are left unchanged. Recorded as a single undo entry.
    pub fn dedent_lines(&mut self, indent: &IndentConfig) {
        let (first_line, last_line) = self.touched_line_range();

        // Strip-per-line, computed up front in original-rope coordinates.
        let mut strips: Vec<(usize, String)> = Vec::new();
        for line in first_line..=last_line {
            let line_str = self.line_str(line);
            let strip = leading_dedent_match(&line_str, indent);
            if !strip.is_empty() {
                let start = self.line_start_byte(line);
                strips.push((start, strip));
            }
        }

        if strips.is_empty() {
            return;
        }

        let primary = self.cursors.primary();
        let original_offset = primary.byte_offset;
        let original_sel = primary.selection;
        let mut tracked: Vec<usize> = vec![original_offset];
        if let Some(s) = original_sel {
            tracked.push(s.anchor);
            tracked.push(s.active);
        }

        self.history.begin_batch();
        // Apply highest-offset deletions first so earlier offsets stay valid.
        for (start, strip) in strips.iter().rev() {
            let end = start + strip.len();
            let deleted = rope_edit::delete(&mut self.rope, *start, end);
            self.history.record(EditCommand::Delete {
                start: *start,
                end,
                deleted,
            });
            let strip_len = strip.len();
            for off in tracked.iter_mut() {
                if *off >= end {
                    *off -= strip_len;
                } else if *off > *start {
                    // Was inside the stripped whitespace — clamp to start.
                    *off = *start;
                }
            }
        }
        self.history.commit_batch();

        let new_offset = tracked[0];
        *self.cursors.primary_mut() = Cursor::from_byte_offset(&self.rope, new_offset);
        if original_sel.is_some() {
            let new_sel = crate::buffer::cursor::Selection::new(tracked[1], tracked[2]);
            self.cursors.primary_mut().selection = Some(new_sel);
        }
        self.modified = true;
    }

    /// Insert a single character with optional auto-dedent on closing
    /// brackets. When `c` is in `rules.decrease_on` and the line up to the
    /// cursor is whitespace-only, dedent the line by one level before
    /// inserting `c`. The whole operation is a single undo entry.
    pub fn insert_char_with_indent(&mut self, c: char, indent: &IndentConfig, rules: IndentRules) {
        if !rules.decrease_on.contains(&c) {
            self.insert_char(c);
            return;
        }
        // Auto-dedent only fires for single-cursor edits where the line up
        // to the cursor is purely whitespace.
        if self.cursors.is_multi() {
            self.insert_char(c);
            return;
        }
        let cursor = self.cursors.primary();
        if cursor.has_selection() {
            self.insert_char(c);
            return;
        }
        let line = cursor.line;
        let line_start = self.line_start_byte(line);
        let prefix_bytes = cursor.byte_offset - line_start;
        let line_str = self.line_str(line);
        let prefix = &line_str[..prefix_bytes.min(line_str.len())];
        if prefix.is_empty() || !prefix.chars().all(|ch| ch == ' ' || ch == '\t') {
            self.insert_char(c);
            return;
        }
        let strip = leading_dedent_match(prefix, indent);
        if strip.is_empty() {
            self.insert_char(c);
            return;
        }
        self.history.begin_batch();
        let strip_end = line_start + strip.len();
        let deleted = rope_edit::delete(&mut self.rope, line_start, strip_end);
        self.history.record(EditCommand::Delete {
            start: line_start,
            end: strip_end,
            deleted,
        });
        let new_offset = cursor.byte_offset - strip.len();
        *self.cursors.primary_mut() = Cursor::from_byte_offset(&self.rope, new_offset);
        let mut s = String::with_capacity(c.len_utf8());
        s.push(c);
        // insert_str handles cursor advance + history recording.
        self.insert_str(&s);
        self.history.commit_batch();
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

    /// Sublime/VS Code "Ctrl+D" motion.
    ///
    /// If the primary cursor has no selection, expand it to the surrounding
    /// word (identifier-like run of `\w` chars). Otherwise, find the next
    /// occurrence of the primary cursor's selection text — searching forward
    /// from the highest existing cursor's selection end and wrapping to the
    /// start of the buffer — and add a new cursor with that selection,
    /// promoting it to primary so the viewport follows.
    ///
    /// Returns `true` if a cursor was added or selection grew.
    pub fn add_cursor_at_next_match(&mut self) -> bool {
        // Phase 1: no selection → expand primary to surrounding word.
        if !self.cursors.primary().has_selection() {
            let cursor = *self.cursors.primary();
            if let Some((start, end)) = word_span_at(&self.rope, cursor.byte_offset) {
                self.move_cursor_to(start, false);
                self.move_cursor_to(end, true);
                return start != end;
            }
            return false;
        }

        // Phase 2: selection present → find next occurrence.
        let needle = match self.cursors.primary().selection {
            Some(sel) => self.text_in_range(sel.as_byte_range().start, sel.as_byte_range().end),
            None => return false,
        };
        if needle.is_empty() {
            return false;
        }
        // Search starts after the highest cursor's selection end (or its byte_offset).
        let max_end = self
            .cursors
            .cursors()
            .iter()
            .map(|c| match c.selection {
                Some(s) => s.as_byte_range().end,
                None => c.byte_offset,
            })
            .max()
            .unwrap_or(0);
        let text = self.rope.to_string();
        let pos = match text.get(max_end..).and_then(|s| s.find(&needle)) {
            Some(p) => Some(max_end + p),
            None => text.find(&needle), // wrap to start
        };
        let Some(start) = pos else {
            return false;
        };
        let end = start + needle.len();
        // If the wrapped match equals an existing cursor's selection, don't re-add.
        if self.cursors.cursors().iter().any(|c| match c.selection {
            Some(s) => {
                let r = s.as_byte_range();
                r.start == start && r.end == end
            }
            None => false,
        }) {
            return false;
        }
        self.cursors
            .add_cursor_with_selection(&self.rope, start, end);
        true
    }

    /// Like `add_cursor_at_next_match`, but first removes the primary cursor
    /// (so the user "skips" the current match instead of adding to it).
    pub fn skip_current_match_to_next(&mut self) -> bool {
        if !self.cursors.is_multi() {
            // With one cursor, just behave like add-next.
            return self.add_cursor_at_next_match();
        }
        let added = self.add_cursor_at_next_match();
        if added {
            // Find the cursor that was the primary *before* the add — it's the
            // second-newest in add_stack. We need to remove it.
            // Simpler: remove the lowest-byte cursor that has a selection
            // matching the needle, since we'll have added the new one ahead.
            // We just remove the cursor with the lowest byte offset that is
            // not the new primary.
            let new_primary_off = self.cursors.primary().byte_offset;
            let lowest_idx = self
                .cursors
                .cursors()
                .iter()
                .enumerate()
                .filter(|(_, c)| c.byte_offset != new_primary_off)
                .min_by_key(|(_, c)| c.byte_offset)
                .map(|(i, _)| i);
            if let Some(idx) = lowest_idx {
                let off = self.cursors.cursors()[idx].byte_offset;
                // Direct removal via cursors_mut to keep invariants and stack tidy.
                self.cursors.cursors_mut().remove(idx);
                let new_primary_idx = self
                    .cursors
                    .cursors()
                    .iter()
                    .position(|c| c.byte_offset == new_primary_off)
                    .unwrap_or(0);
                // Rebuild internals via from_cursors_with_primary while
                // preserving the recently-added stack on the new MultiCursor.
                let preserved_stack: Vec<usize> = self
                    .cursors
                    .recent_adds()
                    .iter()
                    .copied()
                    .filter(|o| *o != off)
                    .collect();
                let new_cursors = self.cursors.cursors().to_vec();
                let mut mc = MultiCursor::from_cursors_with_primary(new_cursors, new_primary_idx);
                mc.set_recent_adds(preserved_stack);
                self.cursors = mc;
            }
        }
        added
    }

    /// Pop the most-recently-added cursor.
    pub fn pop_last_cursor(&mut self) {
        self.cursors.pop_added_cursor();
    }

    /// Replace the cursor list with a rectangular ("box") selection from
    /// `(anchor_line, anchor_display_col)` to `(active_line, active_display_col)`.
    ///
    /// One cursor is placed on each line in the inclusive range. Each cursor's
    /// selection runs between the byte offsets corresponding to the two
    /// display columns on its own line (lines shorter than `active_display_col`
    /// land at end-of-line). The cursor on `active_line` becomes primary.
    pub fn set_box_cursors(
        &mut self,
        anchor_line: usize,
        anchor_display_col: usize,
        active_line: usize,
        active_display_col: usize,
    ) {
        let last_line = self.rope.len_lines().saturating_sub(1);
        let active_line = active_line.min(last_line);
        let anchor_line = anchor_line.min(last_line);
        let (low, high) = if anchor_line <= active_line {
            (anchor_line, active_line)
        } else {
            (active_line, anchor_line)
        };
        let mut cursors: Vec<Cursor> = Vec::with_capacity(high - low + 1);
        let mut primary_idx = 0usize;
        for line in low..=high {
            let line_start = self.line_start_byte(line);
            let anchor_byte = line_start
                + crate::buffer::cursor::byte_col_at_display_col(
                    &self.rope,
                    line,
                    anchor_display_col,
                );
            let active_byte = line_start
                + crate::buffer::cursor::byte_col_at_display_col(
                    &self.rope,
                    line,
                    active_display_col,
                );
            let mut cursor = Cursor::from_byte_offset(&self.rope, active_byte);
            if anchor_byte != active_byte {
                cursor.selection = Some(crate::buffer::cursor::Selection::new(
                    anchor_byte,
                    active_byte,
                ));
            }
            cursor.preferred_col = active_display_col;
            if line == active_line {
                primary_idx = cursors.len();
            }
            cursors.push(cursor);
        }
        if cursors.is_empty() {
            return;
        }
        self.cursors = MultiCursor::from_cursors_with_primary(cursors, primary_idx);
    }

    /// Extend a rectangular selection by one display cell in `dir`.
    ///
    /// The "anchor" of the box is inferred from the existing cursor set: it is
    /// the line/column of the corner opposite to the primary cursor. When only
    /// one cursor is active, the anchor matches the cursor itself, so the
    /// first call seeds the box.
    pub fn extend_box_selection(&mut self, dir: crate::input::action::Direction) {
        let primary = *self.cursors.primary();
        let active_line = primary.line;
        let active_col = primary.preferred_col;

        let cursor_lines: Vec<usize> = self.cursors.cursors().iter().map(|c| c.line).collect();
        let min_line = *cursor_lines.iter().min().unwrap_or(&active_line);
        let max_line = *cursor_lines.iter().max().unwrap_or(&active_line);
        let anchor_line = if !self.cursors.is_multi() {
            active_line
        } else if active_line == min_line {
            max_line
        } else {
            min_line
        };

        // Anchor display col: from primary's selection anchor (if any), else
        // the primary's current display col.
        let anchor_col = match primary.selection {
            Some(sel) => {
                let anchor_byte = sel.anchor;
                let anchor_line_idx = self.rope.char_to_line(self.rope.byte_to_char(anchor_byte));
                let line_start = self.line_start_byte(anchor_line_idx);
                let byte_col = anchor_byte - line_start;
                crate::buffer::cursor::display_col_at(&self.rope, anchor_line_idx, byte_col)
            }
            None => active_col,
        };

        let last_line = self.rope.len_lines().saturating_sub(1);
        let (new_line, new_col) = match dir {
            crate::input::action::Direction::Up => (active_line.saturating_sub(1), active_col),
            crate::input::action::Direction::Down => ((active_line + 1).min(last_line), active_col),
            crate::input::action::Direction::Left => (active_line, active_col.saturating_sub(1)),
            crate::input::action::Direction::Right => (active_line, active_col + 1),
        };
        self.set_box_cursors(anchor_line, anchor_col, new_line, new_col);
    }

    /// Slice text in `[start, end)` as a `String`.
    fn text_in_range(&self, start: usize, end: usize) -> String {
        let cs = self.rope.byte_to_char(start);
        let ce = self.rope.byte_to_char(end);
        self.rope.slice(cs..ce).to_string()
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
        ops.sort_by_key(|b| std::cmp::Reverse(b.ins_pt));

        // new_positions[cursor_idx] = byte offset after the edit.
        let mut new_positions = vec![0usize; n];

        self.history.begin_batch();
        for op in &ops {
            if op.del_end > op.ins_pt {
                let del_len = op.del_end - op.ins_pt;
                let deleted = rope_edit::delete(&mut self.rope, op.ins_pt, op.del_end);
                self.history.record(EditCommand::Delete {
                    start: op.ins_pt,
                    end: op.del_end,
                    deleted,
                });
                // Deletion at [ins_pt, del_end) shifts all tracked positions >= del_end down.
                for pos in new_positions.iter_mut() {
                    if *pos >= op.del_end {
                        *pos -= del_len;
                    }
                }
            }
            rope_edit::insert(&mut self.rope, op.ins_pt, text);
            self.history.record(EditCommand::Insert {
                at: op.ins_pt,
                text: text.to_string(),
            });
            // Insertion at ins_pt shifts all tracked positions >= ins_pt up.
            // (Processed descending, so all previously-set positions are above ins_pt.)
            for pos in new_positions.iter_mut() {
                if *pos >= op.ins_pt {
                    *pos += text.len();
                }
            }
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
        ops.sort_by_key(|b| std::cmp::Reverse(b.del_start));

        // Start with current positions (unchanged for cursors with no op).
        let mut new_positions: Vec<usize> = self
            .cursors
            .cursors()
            .iter()
            .map(|c| c.byte_offset)
            .collect();

        self.history.begin_batch();
        for op in &ops {
            let del_len = op.del_end - op.del_start;
            let deleted = rope_edit::delete(&mut self.rope, op.del_start, op.del_end);
            self.history.record(EditCommand::Delete {
                start: op.del_start,
                end: op.del_end,
                deleted,
            });
            // Deletion at [del_start, del_end) shifts all tracked positions >= del_end down.
            for pos in new_positions.iter_mut() {
                if *pos >= op.del_end {
                    *pos -= del_len;
                }
            }
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

        ops.sort_by_key(|b| std::cmp::Reverse(b.del_start));

        let mut new_positions: Vec<usize> = self
            .cursors
            .cursors()
            .iter()
            .map(|c| c.byte_offset)
            .collect();

        self.history.begin_batch();
        for op in &ops {
            let del_len = op.del_end - op.del_start;
            let deleted = rope_edit::delete(&mut self.rope, op.del_start, op.del_end);
            self.history.record(EditCommand::Delete {
                start: op.del_start,
                end: op.del_end,
                deleted,
            });
            // Deletion at [del_start, del_end) shifts all tracked positions >= del_end down.
            for pos in new_positions.iter_mut() {
                if *pos >= op.del_end {
                    *pos -= del_len;
                }
            }
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

    /// Range of lines `(first, last)` inclusive that the primary cursor
    /// (and its selection, if any) touches. Used by indent / dedent.
    fn touched_line_range(&self) -> (usize, usize) {
        let cursor = self.cursors.primary();
        if !cursor.has_selection() {
            return (cursor.line, cursor.line);
        }
        let range = cursor.selection_bytes();
        let start_line = self.rope.char_to_line(self.rope.byte_to_char(range.start));
        // If the selection ends exactly at a line start (i.e. the user
        // selected up to but not into the next line), don't include that
        // next line — Tab on a 3-line selection that ends at the start of
        // line 4 should indent lines 1-3.
        let end_char = self.rope.byte_to_char(range.end);
        let end_line_raw = self.rope.char_to_line(end_char);
        let end_line_start_char = self.rope.line_to_char(end_line_raw);
        let end_line = if range.end > range.start && end_char == end_line_start_char {
            end_line_raw.saturating_sub(1)
        } else {
            end_line_raw
        };
        (start_line, end_line.max(start_line))
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

/// Returns the leading whitespace prefix of `line_str` to strip when
/// dedenting once. Matches one `\t` if the line starts with a tab, otherwise
/// up to `indent.width` leading spaces. Returns the empty string when there
/// is no leading whitespace to strip.
fn leading_dedent_match(line_str: &str, indent: &IndentConfig) -> String {
    let mut chars = line_str.chars();
    match chars.next() {
        Some('\t') => "\t".to_string(),
        Some(' ') => {
            let width = indent.width.max(1);
            let count = line_str
                .chars()
                .take(width)
                .take_while(|c| *c == ' ')
                .count();
            " ".repeat(count)
        }
        _ => String::new(),
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
    use crate::syntax::language::Lang;

    fn spaces(width: usize) -> IndentConfig {
        IndentConfig {
            style: IndentStyle::Spaces,
            width,
        }
    }

    fn rust_rules() -> IndentRules {
        IndentRules::for_lang(Lang::Rust)
    }

    fn python_rules() -> IndentRules {
        IndentRules::for_lang(Lang::Python)
    }

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
        buf.insert_newline(&spaces(4), rust_rules());
        // New line should have the same 4-space indent
        assert!(buf.to_string().starts_with("    hello\n    "));
    }

    #[test]
    fn newline_after_brace_increases_indent() {
        let mut buf = Buffer::from_str("fn foo() {");
        buf.move_cursor_to(10, false);
        buf.insert_newline(&spaces(4), rust_rules());
        let content = buf.to_string();
        // Should add one extra indent level after '{'
        assert!(content.contains("fn foo() {\n    "));
    }

    #[test]
    fn newline_python_colon_increases_indent() {
        let mut buf = Buffer::from_str("def f():");
        buf.move_cursor_to(8, false);
        buf.insert_newline(&spaces(4), python_rules());
        assert_eq!(buf.to_string(), "def f():\n    ");
    }

    #[test]
    fn newline_python_brace_does_not_increase_indent() {
        // In Python, `{ ( [` are dict / list / call literals — not blocks.
        let mut buf = Buffer::from_str("xs = {");
        buf.move_cursor_to(6, false);
        buf.insert_newline(&spaces(4), python_rules());
        // No extra indent: just the leading whitespace from the previous line (none).
        assert_eq!(buf.to_string(), "xs = {\n");
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
    fn insert_tab_spaces_default_at_column_zero() {
        let mut buf = Buffer::new();
        buf.insert_tab(&spaces(4));
        assert_eq!(buf.to_string(), "    ");
        assert_eq!(buf.cursors.primary().byte_offset, 4);
    }

    #[test]
    fn insert_tab_spaces_smart_advances_to_column_boundary() {
        // At column 1, Tab with width=4 should fill 3 spaces (to column 4).
        let mut buf = Buffer::from_str("x");
        buf.move_cursor_to(1, false);
        buf.insert_tab(&spaces(4));
        assert_eq!(buf.to_string(), "x   ");
    }

    #[test]
    fn insert_tab_tabs_inserts_tab_char() {
        let mut buf = Buffer::new();
        buf.insert_tab(&IndentConfig {
            style: IndentStyle::Tabs,
            width: 4,
        });
        assert_eq!(buf.to_string(), "\t");
    }

    #[test]
    fn indent_lines_three_line_selection() {
        let mut buf = Buffer::from_str("a\nb\nc\n");
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(6, true); // select all 3 lines + trailing newline
        buf.indent_lines(&spaces(4));
        assert_eq!(buf.to_string(), "    a\n    b\n    c\n");
        // One undo restores the original.
        buf.undo();
        assert_eq!(buf.to_string(), "a\nb\nc\n");
    }

    #[test]
    fn indent_lines_no_selection_only_current_line() {
        let mut buf = Buffer::from_str("a\nb\nc\n");
        buf.move_cursor_to(2, false); // line 1, col 0 (the 'b')
        buf.indent_lines(&spaces(4));
        assert_eq!(buf.to_string(), "a\n    b\nc\n");
    }

    #[test]
    fn dedent_lines_handles_tabs_and_spaces() {
        let mut buf = Buffer::from_str("\tline1\n    line2\n  line3\n");
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(buf.rope().len_bytes(), true);
        buf.dedent_lines(&spaces(4));
        assert_eq!(buf.to_string(), "line1\nline2\nline3\n");
    }

    #[test]
    fn dedent_lines_no_op_on_unindented_lines() {
        let mut buf = Buffer::from_str("a\nb\nc\n");
        let before = buf.to_string();
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(buf.rope().len_bytes(), true);
        buf.dedent_lines(&spaces(4));
        assert_eq!(buf.to_string(), before);
    }

    #[test]
    fn insert_close_brace_dedents_when_only_whitespace_before() {
        // "fn x() {\n    " — typing '}' on the indented line dedents and inserts.
        let mut buf = Buffer::from_str("fn x() {\n    ");
        buf.move_cursor_to(13, false); // after the 4 spaces
        buf.insert_char_with_indent('}', &spaces(4), rust_rules());
        assert_eq!(buf.to_string(), "fn x() {\n}");
        // Single undo restores both.
        buf.undo();
        assert_eq!(buf.to_string(), "fn x() {\n    ");
    }

    #[test]
    fn insert_close_brace_no_dedent_with_content_on_line() {
        let mut buf = Buffer::from_str("foo");
        buf.move_cursor_to(3, false);
        buf.insert_char_with_indent('}', &spaces(4), rust_rules());
        assert_eq!(buf.to_string(), "foo}");
    }

    #[test]
    fn insert_close_brace_python_dedent_works() {
        // Python's IndentRules also include `}` `)` `]` for literal collections.
        let mut buf = Buffer::from_str("xs = [\n    ");
        buf.move_cursor_to(11, false);
        buf.insert_char_with_indent(']', &spaces(4), python_rules());
        assert_eq!(buf.to_string(), "xs = [\n]");
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

    #[test]
    fn multi_insert_multiple_chars_three_cursors() {
        // Type "hi" with 3 cursors — each cursor should get exactly "hi" prepended.
        let mut buf = Buffer::from_str("aa\nbb\ncc");
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 0);
        buf.add_cursor_at_display_col(2, 0);
        assert_eq!(buf.cursors.len(), 3);
        buf.multi_insert_char('h');
        buf.multi_insert_char('i');
        assert_eq!(buf.to_string(), "hiaa\nhibb\nhicc");
        // All three cursors should be after their inserted "hi" (col 2 on each line).
        for c in buf.cursors.cursors() {
            assert_eq!(c.col, 2, "cursor on line {} should be at col 2", c.line);
        }
    }

    #[test]
    fn multi_delete_backward_three_cursors() {
        // Delete one char backward with 3 cursors — each line loses its last char.
        let mut buf = Buffer::from_str("abc\ndef\nghi");
        // Place cursors at end of each line.
        buf.move_cursor_to(3, false); // after 'c'
        buf.add_cursor_at_display_col(1, 3); // after 'f'
        buf.add_cursor_at_display_col(2, 3); // after 'i'
        assert_eq!(buf.cursors.len(), 3);
        buf.multi_delete_backward();
        assert_eq!(buf.to_string(), "ab\nde\ngh");
        // Cursors should land at end of their (now shorter) lines.
        for c in buf.cursors.cursors() {
            assert_eq!(c.col, 2, "cursor on line {} should be at col 2", c.line);
        }
    }

    #[test]
    fn multi_delete_forward_three_cursors() {
        // Delete forward at start of each line.
        let mut buf = Buffer::from_str("abc\ndef\nghi");
        buf.move_cursor_to(0, false);
        buf.add_cursor_at_display_col(1, 0);
        buf.add_cursor_at_display_col(2, 0);
        assert_eq!(buf.cursors.len(), 3);
        buf.multi_delete_forward();
        assert_eq!(buf.to_string(), "bc\nef\nhi");
        for c in buf.cursors.cursors() {
            assert_eq!(c.col, 0, "cursor on line {} should stay at col 0", c.line);
        }
    }

    // ── add_cursor_at_next_match (Ctrl+D) ───────────────────────────────

    #[test]
    fn add_cursor_next_match_no_selection_selects_word() {
        let mut buf = Buffer::from_str("foo bar foo bar");
        buf.move_cursor_to(1, false); // mid-"foo"
        let added = buf.add_cursor_at_next_match();
        assert!(added);
        // Single cursor with selection 0..3.
        assert_eq!(buf.cursors.len(), 1);
        let sel = buf.cursors.primary().selection.unwrap();
        assert_eq!(sel.as_byte_range().start, 0);
        assert_eq!(sel.as_byte_range().end, 3);
    }

    #[test]
    fn add_cursor_next_match_finds_next_occurrence() {
        let mut buf = Buffer::from_str("foo bar foo bar");
        // Select the first "foo" (0..3).
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(3, true);
        // Add cursor at next "foo" → should appear at 8..11.
        let added = buf.add_cursor_at_next_match();
        assert!(added);
        assert_eq!(buf.cursors.len(), 2);
        // Primary should be the newly-added one (offset 11 after selection).
        assert_eq!(buf.cursors.primary().byte_offset, 11);
    }

    #[test]
    fn add_cursor_next_match_wraps_around() {
        let mut buf = Buffer::from_str("foo bar foo");
        buf.move_cursor_to(8, false); // before second "foo"
        buf.move_cursor_to(11, true);
        // No "foo" after offset 11 → should wrap and add cursor at 0..3.
        let added = buf.add_cursor_at_next_match();
        assert!(added);
        assert_eq!(buf.cursors.len(), 2);
    }

    #[test]
    fn pop_last_cursor_removes_added() {
        let mut buf = Buffer::from_str("foo bar foo bar");
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(3, true);
        buf.add_cursor_at_next_match();
        assert_eq!(buf.cursors.len(), 2);
        buf.pop_last_cursor();
        assert_eq!(buf.cursors.len(), 1);
    }

    #[test]
    fn skip_current_match_replaces_cursor() {
        let mut buf = Buffer::from_str("foo foo foo");
        buf.move_cursor_to(0, false);
        buf.move_cursor_to(3, true);
        buf.add_cursor_at_next_match(); // now have cursors at 0..3 and 4..7
        assert_eq!(buf.cursors.len(), 2);
        // Skip the "current" → drop the lower one, add the third "foo".
        buf.skip_current_match_to_next();
        assert_eq!(buf.cursors.len(), 2);
        // Cursors should be at 4..7 and 8..11 now.
        let offsets: Vec<usize> = buf
            .cursors
            .cursors()
            .iter()
            .map(|c| c.byte_offset)
            .collect();
        assert!(offsets.contains(&7));
        assert!(offsets.contains(&11));
    }

    // ── Box / column selection ──────────────────────────────────────────

    #[test]
    fn set_box_cursors_one_per_line() {
        // 3 lines, all length >= active_col (3). Box from col 1 to col 3 across
        // lines 0..=2 should give 3 cursors with selections of length 2.
        let mut buf = Buffer::from_str("abcdef\n123456\nfoobar");
        buf.set_box_cursors(0, 1, 2, 3);
        assert_eq!(buf.cursors.len(), 3);
        for c in buf.cursors.cursors() {
            assert!(c.has_selection());
            let r = c.selection_bytes();
            assert_eq!(r.end - r.start, 2);
        }
    }

    #[test]
    fn set_box_cursors_clamps_short_lines() {
        // Line 1 ("hi") shorter than active_col (5).
        let mut buf = Buffer::from_str("hello\nhi\nworld");
        buf.set_box_cursors(0, 0, 2, 5);
        assert_eq!(buf.cursors.len(), 3);
        // Cursor on line 1 should land at end of "hi".
        let line1_cursor = buf
            .cursors
            .cursors()
            .iter()
            .find(|c| c.line == 1)
            .expect("must have line-1 cursor");
        assert_eq!(line1_cursor.col, 2);
    }

    #[test]
    fn extend_box_selection_grows_down() {
        let mut buf = Buffer::from_str("abc\ndef\nghi");
        // Start with a single cursor at line 0, col 1.
        buf.move_cursor_to(1, false);
        buf.extend_box_selection(crate::input::action::Direction::Down);
        // Should now have 2 cursors (line 0 + line 1) at col 1, no selection.
        assert_eq!(buf.cursors.len(), 2);
    }

    #[test]
    fn extend_box_selection_grows_right_creates_selection() {
        let mut buf = Buffer::from_str("abc\ndef");
        buf.move_cursor_to(0, false);
        buf.extend_box_selection(crate::input::action::Direction::Right);
        assert_eq!(buf.cursors.len(), 1);
        assert!(buf.cursors.primary().has_selection());
    }
}
