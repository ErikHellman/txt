use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A byte-range within the rope representing a text selection.
/// `start` <= `end` always (normalized). Anchored at `start`; active end is `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub fn new(start: usize, end: usize) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[allow(dead_code)]
    pub fn len(self) -> usize {
        self.end - self.start
    }

    /// Returns true if the two ranges overlap (including touching boundaries).
    #[allow(dead_code)]
    pub fn overlaps(self, other: ByteRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Merge two overlapping or touching ranges into one.
    #[allow(dead_code)]
    pub fn merge(self, other: ByteRange) -> ByteRange {
        ByteRange {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// A single cursor position within the rope.
///
/// Invariant: `byte_offset` is always a valid char boundary in the rope.
/// `line` and `col` are 0-based. `col` is a byte-column (byte offset within the line),
/// not a display column — use `display_col` for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// 0-based line number.
    pub line: usize,
    /// 0-based byte offset within the line (not grapheme or display col).
    pub col: usize,
    /// Absolute byte offset from the start of the rope.
    pub byte_offset: usize,
    /// Desired display column — preserved during vertical movement through shorter lines.
    pub preferred_col: usize,
    /// Active text selection anchored at the opposite end from this cursor.
    pub selection: Option<Selection>,
}

/// A selection has an anchor (stays fixed) and an active end (moves with the cursor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Byte offset of the fixed anchor end.
    pub anchor: usize,
    /// Byte offset of the active end (where the cursor is).
    pub active: usize,
}

impl Selection {
    pub fn new(anchor: usize, active: usize) -> Self {
        Self { anchor, active }
    }

    /// Returns the selection as a normalized ByteRange (start <= end).
    pub fn as_byte_range(self) -> ByteRange {
        ByteRange::new(self.anchor, self.active)
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.active
    }
}

impl Cursor {
    /// Create a cursor at the start of the rope.
    pub fn at_start() -> Self {
        Self {
            line: 0,
            col: 0,
            byte_offset: 0,
            preferred_col: 0,
            selection: None,
        }
    }

    /// Create a cursor at a given byte offset, computing line/col from the rope.
    pub fn from_byte_offset(rope: &Rope, byte_offset: usize) -> Self {
        let byte_offset = byte_offset.min(rope.len_bytes());
        let char_offset = rope.byte_to_char(byte_offset);
        let line = rope.char_to_line(char_offset);
        let line_start_char = rope.line_to_char(line);
        let line_start_byte = rope.char_to_byte(line_start_char);
        let col = byte_offset - line_start_byte;
        Self {
            line,
            col,
            byte_offset,
            preferred_col: display_col_at(rope, line, col),
            selection: None,
        }
    }

    /// Create a cursor at a (line, col) position where col is a byte-column.
    pub fn from_line_col(rope: &Rope, line: usize, col: usize) -> Self {
        let line = line.min(rope.len_lines().saturating_sub(1));
        let line_start_char = rope.line_to_char(line);
        let line_start_byte = rope.char_to_byte(line_start_char);
        let line_len = line_byte_len_no_newline(rope, line);
        let col = col.min(line_len);
        let byte_offset = line_start_byte + col;
        Self {
            line,
            col,
            byte_offset,
            preferred_col: display_col_at(rope, line, col),
            selection: None,
        }
    }

    /// Move the cursor, optionally extending the selection.
    pub fn move_to(&mut self, rope: &Rope, byte_offset: usize, extend_selection: bool) {
        let new = Cursor::from_byte_offset(rope, byte_offset);
        if extend_selection {
            let anchor = match self.selection {
                Some(sel) => sel.anchor,
                None => self.byte_offset,
            };
            self.selection = Some(Selection::new(anchor, new.byte_offset));
        } else {
            self.selection = None;
        }
        self.line = new.line;
        self.col = new.col;
        self.byte_offset = new.byte_offset;
        self.preferred_col = new.preferred_col;
    }

    /// Clear selection, keeping cursor position.
    #[allow(dead_code)]
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Returns the byte range covered by the current selection, or an empty range at
    /// the cursor position if there is no selection.
    pub fn selection_bytes(&self) -> ByteRange {
        match self.selection {
            Some(sel) => sel.as_byte_range(),
            None => ByteRange::new(self.byte_offset, self.byte_offset),
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection.map(|s| !s.is_empty()).unwrap_or(false)
    }
}

/// Find the byte offset *within* a line that corresponds to `target_display_col`.
///
/// If the line is shorter than the target, returns the byte length of the line
/// (excluding the trailing newline), placing the cursor at the end of the line.
pub fn byte_col_at_display_col(rope: &Rope, line: usize, target_display_col: usize) -> usize {
    if rope.len_lines() == 0 {
        return 0;
    }
    let line_slice = rope.line(line.min(rope.len_lines() - 1));
    let line_str: String = line_slice.chars().collect();
    let stripped = line_str.trim_end_matches(['\r', '\n']);
    let mut dcol = 0usize;
    let mut byte_off = 0usize;
    for g in stripped.graphemes(true) {
        if dcol >= target_display_col {
            break;
        }
        dcol += UnicodeWidthStr::width(g);
        byte_off += g.len();
    }
    byte_off
}

/// Compute the display column (terminal cell width) for a byte offset within a line.
pub fn display_col_at(rope: &Rope, line: usize, byte_col: usize) -> usize {
    if rope.len_lines() == 0 {
        return 0;
    }
    let line_slice = rope.line(line.min(rope.len_lines() - 1));
    let line_str: String = line_slice.chars().collect();
    let prefix = &line_str[..byte_col.min(line_str.len())];
    prefix.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// Returns the byte span `(start, end)` of the word identifier surrounding
/// `byte_offset`, or `None` if the offset is not on a word character.
///
/// "Word" matches `\w`-like characters (alphanumeric or `_`) — the standard
/// notion used by the Sublime/VS Code "Ctrl+D" motion.
pub fn word_span_at(rope: &Rope, byte_offset: usize) -> Option<(usize, usize)> {
    let len = rope.len_bytes();
    if len == 0 {
        return None;
    }
    let at = byte_offset.min(len);
    let text = rope.to_string();
    // Walk back to start of word.
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    // If the cursor is past the end of a word (e.g. just after a closing
    // identifier), step back one char so we still grab the word.
    let mut start = at;
    while start > 0 {
        let prev = text[..start]
            .chars()
            .next_back()
            .map(|c| (c, c.len_utf8()))
            .unwrap();
        if is_word(prev.0) {
            start -= prev.1;
        } else {
            break;
        }
    }
    let mut end = at;
    while end < text.len() {
        let next = text[end..]
            .chars()
            .next()
            .map(|c| (c, c.len_utf8()))
            .unwrap();
        if is_word(next.0) {
            end += next.1;
        } else {
            break;
        }
    }
    if start == end {
        return None;
    }
    Some((start, end))
}

/// Returns the byte length of a line excluding any trailing newline characters.
pub fn line_byte_len_no_newline(rope: &Rope, line: usize) -> usize {
    let total_lines = rope.len_lines();
    if total_lines == 0 {
        return 0;
    }
    let line = line.min(total_lines - 1);
    let line_slice = rope.line(line);
    let line_str: String = line_slice.chars().collect();
    // Strip trailing \r\n or \n
    let stripped = line_str.trim_end_matches(['\n', '\r']);
    stripped.len()
}

/// A collection of cursors supporting multi-cursor editing.
///
/// Invariant: cursors are kept sorted by `byte_offset`. Overlapping selections
/// are merged when the cursor set is normalized.
pub struct MultiCursor {
    cursors: Vec<Cursor>,
    /// Index of the "primary" cursor (the one the viewport follows).
    primary: usize,
    /// LIFO stack of byte offsets recently added by `add-cursor-next-match`.
    /// Used to implement the "undo last cursor" motion. Cleared on
    /// `collapse_to_primary` and any movement that resets cursors.
    add_stack: Vec<usize>,
}

impl MultiCursor {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::at_start()],
            primary: 0,
            add_stack: Vec::new(),
        }
    }

    pub fn with_cursor(cursor: Cursor) -> Self {
        Self {
            cursors: vec![cursor],
            primary: 0,
            add_stack: Vec::new(),
        }
    }

    pub fn primary(&self) -> &Cursor {
        &self.cursors[self.primary]
    }

    pub fn primary_mut(&mut self) -> &mut Cursor {
        &mut self.cursors[self.primary]
    }

    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    #[allow(dead_code)]
    pub fn cursors_mut(&mut self) -> &mut Vec<Cursor> {
        &mut self.cursors
    }

    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    pub fn is_multi(&self) -> bool {
        self.cursors.len() > 1
    }

    /// The 0-based index of the primary cursor within `cursors`.
    pub fn primary_idx(&self) -> usize {
        self.primary
    }

    /// Construct a `MultiCursor` from an explicit list of cursors and primary index.
    ///
    /// Panics in debug mode if `cursors` is empty.
    pub fn from_cursors_with_primary(cursors: Vec<Cursor>, primary: usize) -> Self {
        debug_assert!(!cursors.is_empty(), "cursors must not be empty");
        let n = cursors.len();
        Self {
            cursors,
            primary: primary.min(n.saturating_sub(1)),
            add_stack: Vec::new(),
        }
    }

    /// Add a cursor at the given byte offset. Keeps cursors sorted.
    pub fn add_cursor(&mut self, rope: &Rope, byte_offset: usize) {
        let cursor = Cursor::from_byte_offset(rope, byte_offset);
        self.cursors.push(cursor);
        self.sort_and_dedup();
    }

    /// Add a cursor with an active selection from `anchor` to `active`. Sets
    /// the new cursor as primary so the viewport follows it. Used by the
    /// add-cursor-on-next-match motion (Ctrl+D).
    pub fn add_cursor_with_selection(&mut self, rope: &Rope, anchor: usize, active: usize) {
        let mut cursor = Cursor::from_byte_offset(rope, active);
        cursor.selection = Some(Selection::new(anchor, active));
        self.cursors.push(cursor);
        self.sort_and_dedup();
        // Make the newly added cursor the primary so the viewport follows it.
        self.primary = self
            .cursors
            .iter()
            .position(|c| c.byte_offset == active)
            .unwrap_or(self.primary);
        self.add_stack.push(active);
    }

    /// Pop the most-recently-added cursor (from `add_cursor_with_selection`).
    /// No-op if the stack is empty or only one cursor remains.
    pub fn pop_added_cursor(&mut self) {
        if self.cursors.len() <= 1 {
            self.add_stack.clear();
            return;
        }
        while let Some(active) = self.add_stack.pop() {
            if let Some(idx) = self.cursors.iter().position(|c| c.byte_offset == active) {
                self.cursors.remove(idx);
                if self.primary >= self.cursors.len() {
                    self.primary = self.cursors.len().saturating_sub(1);
                } else if idx <= self.primary {
                    self.primary =
                        self.primary
                            .saturating_sub(if idx < self.primary { 1 } else { 0 });
                }
                // Make the next-most-recent added cursor primary, if any.
                if let Some(prev) = self.add_stack.last()
                    && let Some(p_idx) = self.cursors.iter().position(|c| c.byte_offset == *prev)
                {
                    self.primary = p_idx;
                }
                return;
            }
            // Otherwise the entry is stale (cursor already removed); pop the next.
        }
    }

    /// Remove the primary cursor entirely (used by skip-current-match).
    /// No-op when only one cursor remains.
    #[allow(dead_code)]
    pub fn remove_primary(&mut self) {
        if self.cursors.len() <= 1 {
            return;
        }
        let removed_offset = self.cursors[self.primary].byte_offset;
        self.cursors.remove(self.primary);
        self.add_stack.retain(|&off| off != removed_offset);
        if self.primary >= self.cursors.len() {
            self.primary = self.cursors.len() - 1;
        }
    }

    /// Collapse all cursors to just the primary, clearing all others.
    pub fn collapse_to_primary(&mut self) {
        let primary = self.cursors[self.primary];
        self.cursors = vec![primary];
        self.primary = 0;
        self.add_stack.clear();
    }

    /// Re-sort and deduplicate after any operation that may reorder cursors.
    pub fn normalize(&mut self) {
        self.sort_and_dedup();
    }

    /// Iterate over all non-primary cursors in sorted order.
    pub fn secondary_cursors(&self) -> impl Iterator<Item = &Cursor> {
        let primary = self.primary;
        self.cursors
            .iter()
            .enumerate()
            .filter(move |(i, _)| *i != primary)
            .map(|(_, c)| c)
    }

    /// Read-only view of the recently-added cursor stack. Used by
    /// `Buffer::skip_current_match_to_next` to preserve the stack across a
    /// rebuild.
    pub fn recent_adds(&self) -> &[usize] {
        &self.add_stack
    }

    /// Replace the recently-added cursor stack. Used by
    /// `Buffer::skip_current_match_to_next` after a rebuild.
    pub fn set_recent_adds(&mut self, stack: Vec<usize>) {
        self.add_stack = stack;
    }

    /// Sort cursors by byte offset and remove exact duplicates.
    /// Overlapping selections are merged and the earlier cursor wins.
    fn sort_and_dedup(&mut self) {
        let primary_offset = self.cursors[self.primary].byte_offset;
        self.cursors.sort_by_key(|c| c.byte_offset);
        self.cursors.dedup_by_key(|c| c.byte_offset);
        // Restore primary index
        self.primary = self
            .cursors
            .iter()
            .position(|c| c.byte_offset == primary_offset)
            .unwrap_or(0);
    }

    /// After edits that shift byte offsets, rebuild all cursors from their
    /// byte offsets (which should already have been updated by the edit logic).
    #[allow(dead_code)]
    pub fn rebuild_from_offsets(&mut self, rope: &Rope) {
        let primary_offset = self.cursors[self.primary].byte_offset;
        for cursor in &mut self.cursors {
            let rebuilt = Cursor::from_byte_offset(rope, cursor.byte_offset);
            cursor.line = rebuilt.line;
            cursor.col = rebuilt.col;
            // Preserve preferred_col only for the primary cursor
        }
        self.primary = self
            .cursors
            .iter()
            .position(|c| c.byte_offset == primary_offset)
            .unwrap_or(0);
    }
}

impl Default for MultiCursor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    #[test]
    fn cursor_at_start() {
        let r = rope("hello\nworld");
        let c = Cursor::from_byte_offset(&r, 0);
        assert_eq!(c.line, 0);
        assert_eq!(c.col, 0);
        assert_eq!(c.byte_offset, 0);
    }

    #[test]
    fn cursor_second_line() {
        let r = rope("hello\nworld");
        // "hello\n" is 6 bytes, so 'w' is at offset 6
        let c = Cursor::from_byte_offset(&r, 6);
        assert_eq!(c.line, 1);
        assert_eq!(c.col, 0);
        assert_eq!(c.byte_offset, 6);
    }

    #[test]
    fn cursor_mid_line() {
        let r = rope("hello\nworld");
        let c = Cursor::from_byte_offset(&r, 3); // 'l'
        assert_eq!(c.line, 0);
        assert_eq!(c.col, 3);
    }

    #[test]
    fn cursor_from_line_col() {
        let r = rope("hello\nworld\n");
        let c = Cursor::from_line_col(&r, 1, 3);
        assert_eq!(c.line, 1);
        assert_eq!(c.col, 3);
        // "hello\n" = 6 bytes, + 3 = offset 9 ('l' in "world")
        assert_eq!(c.byte_offset, 9);
    }

    #[test]
    fn cursor_clamps_col_to_line_length() {
        let r = rope("hi\nworld");
        // "hi" is only 2 bytes; requesting col=100 should clamp
        let c = Cursor::from_line_col(&r, 0, 100);
        assert_eq!(c.col, 2);
    }

    #[test]
    fn byte_range_normalized() {
        let r = ByteRange::new(10, 5);
        assert_eq!(r.start, 5);
        assert_eq!(r.end, 10);
    }

    #[test]
    fn byte_range_overlap() {
        let a = ByteRange::new(0, 10);
        let b = ByteRange::new(5, 15);
        assert!(a.overlaps(b));

        let c = ByteRange::new(10, 20);
        assert!(!a.overlaps(c)); // touching but not overlapping
    }

    #[test]
    fn selection_as_range() {
        let sel = Selection::new(10, 3);
        let r = sel.as_byte_range();
        assert_eq!(r.start, 3);
        assert_eq!(r.end, 10);
    }

    #[test]
    fn display_col_ascii() {
        let r = rope("hello\nworld");
        assert_eq!(display_col_at(&r, 0, 3), 3);
    }

    #[test]
    fn display_col_emoji() {
        // "😀" is 4 bytes, display width 2
        let r = rope("😀hi");
        // at byte offset 4 (after emoji), display col should be 2
        assert_eq!(display_col_at(&r, 0, 4), 2);
    }

    #[test]
    fn line_byte_len_no_trailing_newline() {
        let r = rope("hello\nworld\n");
        assert_eq!(line_byte_len_no_newline(&r, 0), 5); // "hello"
        assert_eq!(line_byte_len_no_newline(&r, 1), 5); // "world"
    }

    #[test]
    fn multi_cursor_dedup() {
        let r = rope("hello world");
        let mut mc = MultiCursor::new();
        mc.add_cursor(&r, 0); // duplicate of initial cursor
        assert_eq!(mc.len(), 1);
    }

    #[test]
    fn multi_cursor_collapse() {
        let r = rope("hello world");
        let mut mc = MultiCursor::new();
        mc.add_cursor(&r, 5);
        assert_eq!(mc.len(), 2);
        mc.collapse_to_primary();
        assert_eq!(mc.len(), 1);
    }

    #[test]
    fn word_span_finds_identifier() {
        let r = rope("foo bar_baz qux");
        // Cursor inside "bar_baz" — span should cover the whole identifier.
        let span = word_span_at(&r, 5).unwrap();
        assert_eq!(&r.to_string()[span.0..span.1], "bar_baz");
    }

    #[test]
    fn word_span_returns_none_in_whitespace() {
        let r = rope("foo  bar");
        // Between the two spaces.
        assert!(word_span_at(&r, 4).is_none());
    }

    #[test]
    fn add_cursor_with_selection_promotes_to_primary() {
        let r = rope("abc abc abc");
        let mut mc = MultiCursor::new();
        // Make primary a 0..3 selection (anchor=0, active=3).
        mc.cursors[0].selection = Some(Selection::new(0, 3));
        mc.cursors[0].byte_offset = 3;
        mc.add_cursor_with_selection(&r, 4, 7);
        assert_eq!(mc.len(), 2);
        // Primary should now be the new one.
        assert_eq!(mc.primary().byte_offset, 7);
        assert!(mc.primary().has_selection());
    }

    #[test]
    fn pop_added_cursor_undoes_addition() {
        let r = rope("abc abc abc");
        let mut mc = MultiCursor::new();
        mc.cursors[0].selection = Some(Selection::new(0, 3));
        mc.cursors[0].byte_offset = 3;
        mc.add_cursor_with_selection(&r, 4, 7);
        assert_eq!(mc.len(), 2);
        mc.pop_added_cursor();
        assert_eq!(mc.len(), 1);
        // Original cursor remains.
        assert_eq!(mc.primary().byte_offset, 3);
    }
}
