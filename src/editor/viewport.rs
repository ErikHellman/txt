use crate::buffer::{Buffer, cursor::display_col_at};

/// Tracks the visible portion of the buffer.
///
/// `scroll_row` is the first visible line (0-based).
/// `scroll_col` is the first visible display column (for horizontal scrolling,
/// unused when `word_wrap` is true).
#[derive(Debug, Clone, Default)]
pub struct Viewport {
    pub scroll_row: usize,
    pub scroll_col: usize,
    /// When true, long lines are wrapped at the text area width instead of
    /// scrolling horizontally.
    pub word_wrap: bool,
}

impl Viewport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adjust the viewport so the primary cursor is always visible.
    /// `height` is the number of text rows available (terminal height minus status bar).
    /// `width` is the number of display columns available (terminal width minus gutter).
    pub fn scroll_to_cursor(&mut self, buffer: &Buffer, height: usize, width: usize) {
        if height == 0 || width == 0 {
            return;
        }
        let cursor = buffer.cursors.primary();
        let line = cursor.line;
        let display_col = display_col_at(buffer.rope(), cursor.line, cursor.col);

        // Vertical scroll
        if line < self.scroll_row {
            self.scroll_row = line;
        } else if line >= self.scroll_row + height {
            self.scroll_row = line.saturating_sub(height - 1);
        }

        // Horizontal scroll — keep cursor within [scroll_col, scroll_col + width)
        if display_col < self.scroll_col {
            self.scroll_col = display_col;
        } else if display_col >= self.scroll_col + width {
            // Keep a small margin of 4 columns when scrolling right
            self.scroll_col = display_col.saturating_sub(width.saturating_sub(4));
        }
    }

    /// Returns an iterator of `(line_index, line_string_without_newline)` for all
    /// lines visible in the current viewport.
    ///
    /// `height`: available text rows.
    /// `scroll_col`: applied to clip horizontal content; caller passes `self.scroll_col`.
    pub fn visible_lines<'b>(
        &self,
        buffer: &'b Buffer,
        height: usize,
    ) -> impl Iterator<Item = (usize, String)> + 'b {
        let start = self.scroll_row;
        let end = (start + height).min(buffer.len_lines());
        let scroll_col = self.scroll_col;
        (start..end).map(move |line_idx| {
            let s = buffer.line_str(line_idx);
            // Clip horizontal scroll: skip `scroll_col` display columns.
            let clipped = clip_display_cols(&s, scroll_col);
            (line_idx, clipped)
        })
    }

    /// Like `visible_lines`, but wraps long lines at `width` display columns.
    ///
    /// Returns `(line_idx, byte_offset_of_segment_within_line, segment_string)`.
    /// Multiple items may share the same `line_idx` when a line is wider than `width`.
    pub fn visible_lines_wrapped(
        &self,
        buffer: &Buffer,
        height: usize,
        width: usize,
    ) -> Vec<(usize, usize, String)> {
        let mut result = Vec::with_capacity(height);
        let total = buffer.len_lines();
        let start = self.scroll_row;

        for line_idx in start..total {
            if result.len() >= height {
                break;
            }
            let line_str = buffer.line_str(line_idx);
            for (seg_byte, seg_str) in split_line_at_width(&line_str, width) {
                if result.len() >= height {
                    break;
                }
                result.push((line_idx, seg_byte, seg_str));
            }
        }
        result
    }
}

/// Split `line` into segments of at most `width` display columns.
///
/// Returns `(byte_offset_within_line, segment_string)` pairs. Always returns
/// at least one entry (even for an empty line). `width == 0` treats each
/// grapheme as its own segment.
fn split_line_at_width(line: &str, width: usize) -> Vec<(usize, String)> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    if width == 0 || line.is_empty() {
        return vec![(0, line.to_string())];
    }

    let mut segments: Vec<(usize, String)> = Vec::new();
    let mut seg_start = 0usize;
    let mut col = 0usize;

    for (byte_idx, grapheme) in line.grapheme_indices(true) {
        let w = UnicodeWidthStr::width(grapheme);
        // If adding this grapheme would exceed the width, flush the current segment.
        if col > 0 && col + w > width {
            segments.push((seg_start, line[seg_start..byte_idx].to_string()));
            seg_start = byte_idx;
            col = 0;
        }
        col += w;
    }
    // Final segment (may be empty for an empty line).
    segments.push((seg_start, line[seg_start..].to_string()));
    segments
}

/// Map a display column offset within a line string to a byte offset.
///
/// If `target_col` exceeds the line's display width, returns `s.len()` (end of line).
pub fn display_col_to_byte(s: &str, target_col: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut col = 0usize;
    for (byte_idx, grapheme) in s.grapheme_indices(true) {
        if col >= target_col {
            return byte_idx;
        }
        col += UnicodeWidthStr::width(grapheme);
    }
    s.len()
}

/// Convert a screen position to `(line_index, display_column_in_line)`.
/// Mirrors `screen_pos_to_byte_offset` but returns the display-column
/// representation, suitable for box / column selection where each line on
/// the rectangle wants the same display column regardless of width.
///
/// When `viewport.word_wrap` is true, multiple visual rows may map to the
/// same logical line; this resolves the click to the segment under the
/// cursor and adds the segment's start display column to the in-segment
/// column so the returned display column is correct for the original line.
pub fn screen_pos_to_line_display_col(
    screen_col: u16,
    screen_row: u16,
    editor_area_y: u16,
    gutter_cols: u16,
    text_width: u16,
    buffer: &Buffer,
    viewport: &Viewport,
) -> (usize, usize) {
    let row_in_area = (screen_row as usize).saturating_sub(editor_area_y as usize);
    let col_in_area = screen_col as usize;
    let gutter = gutter_cols as usize;
    let click_in_gutter = col_in_area <= gutter;
    let col_in_text = col_in_area.saturating_sub(gutter);

    if viewport.word_wrap && text_width > 0 {
        let (line_idx, seg_byte, _seg_str) =
            visual_row_at(buffer, viewport, row_in_area, text_width as usize);
        let seg_start_display_col = display_col_for_byte_in_line(buffer, line_idx, seg_byte);
        let display_col = if click_in_gutter {
            seg_start_display_col
        } else {
            seg_start_display_col + col_in_text
        };
        (line_idx, display_col)
    } else {
        let line_idx =
            (viewport.scroll_row + row_in_area).min(buffer.len_lines().saturating_sub(1));
        let display_col = if click_in_gutter {
            0
        } else {
            col_in_text + viewport.scroll_col
        };
        (line_idx, display_col)
    }
}

/// Convert a screen position (absolute terminal column + row) to a rope byte offset.
///
/// Parameters:
/// - `screen_col`, `screen_row`: absolute terminal coordinates of the click/drag
/// - `editor_area_y`: the top Y coordinate of the editor area (0 if no title bar)
/// - `gutter_cols`: number of columns consumed by the line-number gutter + separator
/// - `text_width`: number of columns available for text (used only when word wrap is on)
/// - `buffer`, `viewport`: current editor state
///
/// Returns a valid byte offset clamped to `[0, rope.len_bytes()]`.
pub fn screen_pos_to_byte_offset(
    screen_col: u16,
    screen_row: u16,
    editor_area_y: u16,
    gutter_cols: u16,
    text_width: u16,
    buffer: &Buffer,
    viewport: &Viewport,
) -> usize {
    let row_in_area = (screen_row as usize).saturating_sub(editor_area_y as usize);
    let col_in_area = screen_col as usize;
    let gutter = gutter_cols as usize;
    let click_in_gutter = col_in_area <= gutter;
    let col_in_text = col_in_area.saturating_sub(gutter);

    if viewport.word_wrap && text_width > 0 {
        let (line_idx, seg_byte, seg_str) =
            visual_row_at(buffer, viewport, row_in_area, text_width as usize);
        let line_start = buffer
            .rope()
            .char_to_byte(buffer.rope().line_to_char(line_idx));
        if click_in_gutter {
            // Click in the gutter — jump to the start of this visual segment.
            return line_start + seg_byte;
        }
        let byte_in_segment = display_col_to_byte(&seg_str, col_in_text);
        line_start + seg_byte + byte_in_segment
    } else {
        let line_idx =
            (viewport.scroll_row + row_in_area).min(buffer.len_lines().saturating_sub(1));
        let line_start = buffer
            .rope()
            .char_to_byte(buffer.rope().line_to_char(line_idx));
        if click_in_gutter {
            // Click in the gutter — jump to the logical start of the line.
            return line_start;
        }
        let line_str = buffer.line_str(line_idx);
        let byte_in_line = display_col_to_byte(&line_str, col_in_text + viewport.scroll_col);
        line_start + byte_in_line
    }
}

/// Resolve a visual row index (counted from `viewport.scroll_row` at row 0)
/// to the corresponding wrapped segment. Returns
/// `(line_idx, seg_byte_within_line, segment_string)`.
///
/// If the row is past the end of the buffer, returns the last visible
/// segment. If the buffer is empty, returns `(0, 0, "")`.
fn visual_row_at(
    buffer: &Buffer,
    viewport: &Viewport,
    row_in_area: usize,
    text_width: usize,
) -> (usize, usize, String) {
    if buffer.len_lines() == 0 {
        return (0, 0, String::new());
    }
    let mut visited = 0usize;
    let mut last: Option<(usize, usize, String)> = None;
    for line_idx in viewport.scroll_row..buffer.len_lines() {
        let line_str = buffer.line_str(line_idx);
        for (seg_byte, seg_str) in split_line_at_width(&line_str, text_width) {
            if visited == row_in_area {
                return (line_idx, seg_byte, seg_str);
            }
            visited += 1;
            last = Some((line_idx, seg_byte, seg_str));
        }
    }
    last.unwrap_or((0, 0, String::new()))
}

/// Display column of `byte_in_line` within `line_idx`'s line string.
fn display_col_for_byte_in_line(buffer: &Buffer, line_idx: usize, byte_in_line: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let line_str = buffer.line_str(line_idx);
    let prefix = &line_str[..byte_in_line.min(line_str.len())];
    prefix.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// Returns the substring of `s` starting from display column `skip_cols`.
/// Handles multi-byte / wide characters correctly.
fn clip_display_cols(s: &str, skip_cols: usize) -> String {
    if skip_cols == 0 {
        return s.to_string();
    }
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut col = 0usize;
    for (byte_idx, grapheme) in s.grapheme_indices(true) {
        let w = UnicodeWidthStr::width(grapheme);
        if col + w > skip_cols {
            return s[byte_idx..].to_string();
        }
        col += w;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;

    fn buf(s: &str) -> Buffer {
        Buffer::from_str(s)
    }

    #[test]
    fn visible_lines_simple() {
        let b = buf("a\nb\nc\nd\ne");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: false,
        };
        let lines: Vec<_> = vp.visible_lines(&b, 3).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], (0, "a".to_string()));
        assert_eq!(lines[1], (1, "b".to_string()));
        assert_eq!(lines[2], (2, "c".to_string()));
    }

    #[test]
    fn visible_lines_with_scroll() {
        let b = buf("a\nb\nc\nd\ne");
        let vp = Viewport {
            scroll_row: 2,
            scroll_col: 0,
            word_wrap: false,
        };
        let lines: Vec<_> = vp.visible_lines(&b, 3).collect();
        assert_eq!(lines[0], (2, "c".to_string()));
        assert_eq!(lines[1], (3, "d".to_string()));
        assert_eq!(lines[2], (4, "e".to_string()));
    }

    #[test]
    fn visible_lines_at_end_of_file() {
        let b = buf("a\nb");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: false,
        };
        let lines: Vec<_> = vp.visible_lines(&b, 10).collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn scroll_to_cursor_down() {
        let content = (0..20).map(|i| format!("line{i}\n")).collect::<String>();
        let mut b = Buffer::from_str(&content);
        let mut vp = Viewport::new();
        // Jump cursor to line 15
        b.move_cursor_to(b.rope().char_to_byte(b.rope().line_to_char(15)), false);
        vp.scroll_to_cursor(&b, 10, 80);
        // Line 15 should now be visible
        assert!(vp.scroll_row <= 15);
        assert!(vp.scroll_row + 10 > 15);
    }

    #[test]
    fn scroll_to_cursor_up() {
        let content = (0..20).map(|i| format!("line{i}\n")).collect::<String>();
        let b = Buffer::from_str(&content);
        let mut vp = Viewport {
            scroll_row: 15,
            scroll_col: 0,
            word_wrap: false,
        };
        // Cursor is at line 0 (default)
        vp.scroll_to_cursor(&b, 10, 80);
        assert_eq!(vp.scroll_row, 0);
    }

    #[test]
    fn clip_display_cols_ascii() {
        assert_eq!(clip_display_cols("hello world", 6), "world");
    }

    #[test]
    fn clip_display_cols_zero() {
        assert_eq!(clip_display_cols("hello", 0), "hello");
    }

    // ── display_col_to_byte ────────────────────────────────────────────

    #[test]
    fn col_to_byte_ascii() {
        assert_eq!(display_col_to_byte("hello", 0), 0);
        assert_eq!(display_col_to_byte("hello", 3), 3);
        assert_eq!(display_col_to_byte("hello", 5), 5); // past end
    }

    #[test]
    fn col_to_byte_past_end() {
        assert_eq!(display_col_to_byte("hi", 100), 2);
    }

    #[test]
    fn col_to_byte_wide_char() {
        // "😀" has display width 2 and byte length 4
        let s = "a😀b";
        // col 0 → byte 0 ('a')
        assert_eq!(display_col_to_byte(s, 0), 0);
        // col 1 → byte 1 (start of '😀')
        assert_eq!(display_col_to_byte(s, 1), 1);
        // col 3 → byte 5 ('b', after the 4-byte emoji)
        assert_eq!(display_col_to_byte(s, 3), 5);
    }

    // ── screen_pos_to_byte_offset ──────────────────────────────────────

    #[test]
    fn screen_pos_click_on_first_char() {
        let b = buf("hello\nworld");
        let vp = Viewport::new(); // scroll_row=0, scroll_col=0
        // gutter for 2 lines = 1 digit + 1 pad = 2 cols
        let offset = screen_pos_to_byte_offset(2, 0, 0, 2, 80, &b, &vp);
        assert_eq!(offset, 0); // first char of line 0
    }

    #[test]
    fn screen_pos_click_on_second_line() {
        let b = buf("hello\nworld");
        let vp = Viewport::new();
        // gutter = 2, clicking col=4 on row=1 → line 1, display_col=2
        let offset = screen_pos_to_byte_offset(4, 1, 0, 2, 80, &b, &vp);
        // line 1 starts at byte 6 ("hello\n" = 6), display_col=2 → byte 2 within line
        assert_eq!(offset, 8);
    }

    #[test]
    fn screen_pos_click_in_gutter_goes_to_line_start() {
        let b = buf("hello\nworld");
        let vp = Viewport::new();
        // click at col=1 (inside 2-col gutter) → should land at start of line 1
        let offset = screen_pos_to_byte_offset(1, 1, 0, 2, 80, &b, &vp);
        assert_eq!(offset, 6); // "hello\n".len() = 6
    }

    #[test]
    fn screen_pos_respects_scroll() {
        let content = (0..20).map(|i| format!("line{i}\n")).collect::<String>();
        let b = Buffer::from_str(&content);
        let vp = Viewport {
            scroll_row: 10,
            scroll_col: 0,
            word_wrap: false,
        };
        // click on row=0 of the editor area → should map to buffer line 10
        let offset = screen_pos_to_byte_offset(2, 0, 0, 2, 80, &b, &vp);
        let expected_line_start = b.rope().char_to_byte(b.rope().line_to_char(10));
        assert_eq!(offset, expected_line_start);
    }

    // ── Word-wrap mouse mapping ────────────────────────────────────────

    #[test]
    fn screen_pos_word_wrap_second_segment() {
        // "abcdefghij" wrapped at width 5 renders as:
        //   row 0: "abcde"
        //   row 1: "fghij"
        // Clicking row=1 col=gutter+2 → 'h' (byte 7 in the line).
        let b = buf("abcdefghij");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: true,
        };
        let offset = screen_pos_to_byte_offset(/*col*/ 4, /*row*/ 1, 0, 2, 5, &b, &vp);
        assert_eq!(offset, 7);
    }

    #[test]
    fn screen_pos_word_wrap_after_short_line() {
        // "ab\nabcdefghij" with width 5:
        //   row 0: "ab"          (line 0)
        //   row 1: "abcde"       (line 1, seg 0)
        //   row 2: "fghij"       (line 1, seg 1)
        // Click row=2 col=gutter+1 → 'g' (byte 1 in segment 2 of line 1).
        let b = buf("ab\nabcdefghij");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: true,
        };
        let offset = screen_pos_to_byte_offset(3, 2, 0, 2, 5, &b, &vp);
        // line 1 starts at byte 3 ("ab\n"), seg_byte=5, byte_in_seg=1 → 3 + 5 + 1 = 9
        assert_eq!(offset, 9);
    }

    #[test]
    fn screen_pos_word_wrap_first_segment_first_char() {
        // Sanity: clicking the first character of a wrapped line still works.
        let b = buf("abcdefghij");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: true,
        };
        let offset = screen_pos_to_byte_offset(2, 0, 0, 2, 5, &b, &vp);
        assert_eq!(offset, 0);
    }

    #[test]
    fn screen_pos_word_wrap_past_end_clamps_to_last_segment() {
        // Click below the last visual row should land at the end of the buffer.
        let b = buf("abcde");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: true,
        };
        // row=10 is way past the only visible row.
        let offset = screen_pos_to_byte_offset(100, 10, 0, 2, 5, &b, &vp);
        assert_eq!(offset, 5);
    }

    #[test]
    fn screen_pos_word_wrap_respects_scroll_row() {
        // Two long lines. With scroll_row=1, the top of the editor area shows
        // the second line's first segment.
        let b = buf("abcdefghij\nklmnopqrst");
        let vp = Viewport {
            scroll_row: 1,
            scroll_col: 0,
            word_wrap: true,
        };
        // Click row=0 col=gutter+1 → 'l' (byte 1 in line 1).
        let offset = screen_pos_to_byte_offset(3, 0, 0, 2, 5, &b, &vp);
        // line 1 starts at byte 11 ("abcdefghij\n"), seg 0, byte in seg = 1 → 12
        assert_eq!(offset, 12);
    }

    // ── screen_pos_to_line_display_col + word wrap ─────────────────────

    #[test]
    fn screen_pos_line_display_col_wrap_second_segment() {
        // "abcdefghij" wrapped at width 5 → row 1 col=gutter+2 should yield
        // line 0 with display_col = 5 + 2 = 7.
        let b = buf("abcdefghij");
        let vp = Viewport {
            scroll_row: 0,
            scroll_col: 0,
            word_wrap: true,
        };
        let (line, dcol) = screen_pos_to_line_display_col(4, 1, 0, 2, 5, &b, &vp);
        assert_eq!(line, 0);
        assert_eq!(dcol, 7);
    }

    // ── split_line_at_width ────────────────────────────────────────────────

    #[test]
    fn split_short_line_fits_in_one_segment() {
        let segs = split_line_at_width("hello", 10);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], (0, "hello".to_string()));
    }

    #[test]
    fn split_empty_line_returns_one_empty_segment() {
        let segs = split_line_at_width("", 10);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], (0, String::new()));
    }

    #[test]
    fn split_exact_width_fits_in_one_segment() {
        let segs = split_line_at_width("hello", 5);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].1, "hello");
    }

    #[test]
    fn split_long_line_splits_at_width() {
        // "abcde" width 3 → "abc" (0) and "de" (3)
        let segs = split_line_at_width("abcde", 3);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], (0, "abc".to_string()));
        assert_eq!(segs[1], (3, "de".to_string()));
    }

    #[test]
    fn split_three_segments() {
        // 9 chars, width 3 → three segments of 3
        let segs = split_line_at_width("abcdefghi", 3);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].1, "abc");
        assert_eq!(segs[1].1, "def");
        assert_eq!(segs[2].1, "ghi");
        assert_eq!(segs[1].0, 3); // byte offset of second segment
        assert_eq!(segs[2].0, 6);
    }

    // ── visible_lines_wrapped ──────────────────────────────────────────────

    #[test]
    fn visible_lines_wrapped_short_lines() {
        // Lines shorter than width: one visual row per logical line.
        let b = buf("ab\ncd\nef");
        let vp = Viewport::new();
        let rows = vp.visible_lines_wrapped(&b, 10, 80);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], (0, 0, "ab".to_string()));
        assert_eq!(rows[1], (1, 0, "cd".to_string()));
        assert_eq!(rows[2], (2, 0, "ef".to_string()));
    }

    #[test]
    fn visible_lines_wrapped_wraps_long_line() {
        // One 6-char line, width 3 → two visual rows.
        let b = buf("abcdef");
        let vp = Viewport::new();
        let rows = vp.visible_lines_wrapped(&b, 10, 3);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (0, 0, "abc".to_string()));
        assert_eq!(rows[1], (0, 3, "def".to_string()));
    }

    #[test]
    fn visible_lines_wrapped_height_limit() {
        // 3 logical lines, each wrapping into 2 visual rows, height=4 → 4 rows.
        let b = buf("abcd\nefgh\nijkl");
        let vp = Viewport::new();
        let rows = vp.visible_lines_wrapped(&b, 4, 2);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (0, 0, "ab".to_string()));
        assert_eq!(rows[1], (0, 2, "cd".to_string()));
        assert_eq!(rows[2], (1, 0, "ef".to_string()));
        assert_eq!(rows[3], (1, 2, "gh".to_string()));
    }

    #[test]
    fn visible_lines_wrapped_respects_scroll_row() {
        // Skip first logical line via scroll_row.
        let b = buf("skip\nshow");
        let vp = Viewport {
            scroll_row: 1,
            scroll_col: 0,
            word_wrap: true,
        };
        let rows = vp.visible_lines_wrapped(&b, 10, 80);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 1); // line_idx = 1
        assert_eq!(rows[0].2, "show");
    }
}
