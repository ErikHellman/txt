use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use crate::buffer::cursor::ByteRange;
use crate::editor::tab::BufferHandle;
use crate::git::{GitGutter, GutterMark};
use crate::lsp::types::DiagSeverity;
use crate::search::SearchState;
use crate::syntax::highlighter::{HighlightSpan, style_for_kind};
use crate::theme::ThemeColors;

/// Width of a single space used as a separator between gutter and text.
const GUTTER_PAD: u16 = 1;
/// Width of the git gutter column (shown left of line numbers when active).
const GIT_GUTTER_W: u16 = 1;
/// Width of the diagnostic gutter column (shown when diagnostics are present).
const DIAG_GUTTER_W: u16 = 1;

/// Render the text editing area into the ratatui terminal buffer.
///
/// Highlights (in priority order, highest first):
///   1. Cursor position
///   2. Cursor selection
///   3. Current search match
///   4. Other search matches
///   5. Bracket-pair highlight
///   6. Syntax highlight (tree-sitter)
///   7. Plain text
#[allow(clippy::too_many_arguments)]
pub fn render(
    handle: &BufferHandle,
    search: Option<&SearchState>,
    highlights: &[HighlightSpan],
    git_gutter: Option<&GitGutter>,
    focused: bool,
    show_whitespace: bool,
    tab_size: usize,
    theme: &ThemeColors,
    area: Rect,
    buf: &mut TermBuffer,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let total_lines = handle.buffer.len_lines();
    let has_git = git_gutter.is_some();
    let git_col_w: u16 = if has_git { GIT_GUTTER_W } else { 0 };
    let has_diag = !handle.lsp_state.diagnostics.is_empty();
    let diag_col_w: u16 = if has_diag { DIAG_GUTTER_W } else { 0 };
    let gw = gutter_width(total_lines);
    let text_area = text_area(area, gw, git_col_w, diag_col_w);

    // Build per-line diagnostic severity map (highest severity per line).
    let diag_line_severity = if has_diag {
        let rope = handle.buffer.rope();
        let mut map = std::collections::HashMap::<usize, DiagSeverity>::new();
        for diag in &handle.lsp_state.diagnostics {
            let line = rope.byte_to_char(diag.range.start.min(rope.len_bytes()));
            let line_idx = rope.char_to_line(line);
            let entry = map.entry(line_idx).or_insert(DiagSeverity::Hint);
            if diag.severity < *entry {
                *entry = diag.severity;
            }
        }
        map
    } else {
        std::collections::HashMap::new()
    };

    let cursor = handle.buffer.cursors.primary();
    let selection = cursor.selection_bytes();
    let has_selection = cursor.has_selection();

    // Collect secondary cursor byte offsets for multi-cursor rendering.
    let secondary_cursor_offsets: Vec<usize> = if handle.buffer.cursors.is_multi() {
        let primary_idx = handle.buffer.cursors.primary_idx();
        handle
            .buffer
            .cursors
            .cursors()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != primary_idx)
            .map(|(_, c)| c.byte_offset)
            .collect()
    } else {
        vec![]
    };
    // Secondary cursor structs needed for end-of-line rendering.
    let secondary_cursors_eol: Vec<(usize, usize)> = if handle.buffer.cursors.is_multi() {
        let primary_idx = handle.buffer.cursors.primary_idx();
        handle
            .buffer
            .cursors
            .cursors()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != primary_idx)
            .map(|(_, c)| (c.line, c.col))
            .collect()
    } else {
        vec![]
    };

    // Pre-compute bracket-match positions.
    let bracket_pair = find_matching_bracket(handle.buffer.rope(), cursor.byte_offset);

    // Styles
    let line_num_style = Style::default().fg(Color::DarkGray);
    let line_num_current_style = Style::default().fg(theme.line_num_cur);
    let text_style = Style::default().fg(theme.text);
    let selection_style = Style::default().bg(theme.selection_bg).fg(theme.text);
    let cursor_style = if focused {
        Style::default()
            .bg(Color::White)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::Rgb(70, 70, 95))
            .fg(Color::Rgb(160, 160, 180))
    };
    let secondary_cursor_style = Style::default()
        .bg(Color::Rgb(60, 140, 80))
        .fg(Color::Black);
    let match_style = Style::default()
        .bg(Color::Rgb(80, 70, 20))
        .fg(Color::Rgb(255, 230, 100));
    let current_match_style = Style::default()
        .bg(Color::Rgb(180, 140, 0))
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let bracket_style = Style::default()
        .bg(Color::Rgb(60, 80, 60))
        .fg(Color::Rgb(140, 220, 140))
        .add_modifier(Modifier::BOLD);
    let whitespace_style = Style::default().fg(Color::Rgb(80, 80, 100));

    // Git gutter styles.
    let git_added_style = Style::default().fg(Color::Rgb(80, 200, 80));
    let git_modified_style = Style::default().fg(Color::Rgb(200, 160, 60));
    let git_deleted_style = Style::default().fg(Color::Rgb(200, 80, 80));

    // Diagnostic gutter styles.
    let diag_error_style = Style::default().fg(Color::Rgb(240, 80, 80));
    let diag_warning_style = Style::default().fg(Color::Rgb(240, 200, 60));
    let diag_info_style = Style::default().fg(Color::Rgb(80, 160, 240));
    let diag_hint_style = Style::default().fg(Color::Rgb(120, 120, 140));

    // Collect visual lines — either wrapped or plain depending on the viewport mode.
    struct VisualLine {
        line_idx: usize,
        /// Byte offset of first byte of this segment within the line string.
        seg_byte: usize,
        display: String,
        /// True only for the first visual row of a logical line (shows line number).
        is_first_seg: bool,
    }

    let height = area.height as usize;
    let visual_lines: Vec<VisualLine> = if handle.viewport.word_wrap && text_area.width > 0 {
        let wrapped =
            handle
                .viewport
                .visible_lines_wrapped(&handle.buffer, height, text_area.width as usize);
        let mut last_line = usize::MAX;
        wrapped
            .into_iter()
            .map(|(line_idx, seg_byte, display)| {
                let is_first_seg = line_idx != last_line;
                last_line = line_idx;
                VisualLine {
                    line_idx,
                    seg_byte,
                    display,
                    is_first_seg,
                }
            })
            .collect()
    } else {
        handle
            .viewport
            .visible_lines(&handle.buffer, height)
            .map(|(line_idx, display)| {
                let seg_byte = scroll_col_byte_offset(
                    &handle.buffer.line_str(line_idx),
                    handle.viewport.scroll_col,
                );
                VisualLine {
                    line_idx,
                    seg_byte,
                    display,
                    is_first_seg: true,
                }
            })
            .collect()
    };

    for (screen_row, vl) in visual_lines.iter().enumerate() {
        let line_idx = vl.line_idx;
        let y = area.y + screen_row as u16;

        // ── Git gutter ───────────────────────────────────────────────────────
        if has_git && vl.is_first_seg {
            let (git_sym, git_sty) = match git_gutter.and_then(|g| g.get(line_idx)) {
                Some(GutterMark::Added) => ("▌", git_added_style),
                Some(GutterMark::Modified) => ("▌", git_modified_style),
                Some(GutterMark::Deleted) => ("▾", git_deleted_style),
                None => (" ", Style::default()),
            };
            buf.set_string(area.x, y, git_sym, git_sty);
        }

        // ── Diagnostic gutter ────────────────────────────────────────────────
        if has_diag && vl.is_first_seg {
            let diag_x = area.x + git_col_w;
            let (diag_sym, diag_sty) = match diag_line_severity.get(&line_idx) {
                Some(DiagSeverity::Error) => ("●", diag_error_style),
                Some(DiagSeverity::Warning) => ("▲", diag_warning_style),
                Some(DiagSeverity::Information) => ("ℹ", diag_info_style),
                Some(DiagSeverity::Hint) => ("·", diag_hint_style),
                None => (" ", Style::default()),
            };
            buf.set_string(diag_x, y, diag_sym, diag_sty);
        }

        // ── Gutter (line number) ─────────────────────────────────────────────
        let gutter_x = area.x + git_col_w + diag_col_w;
        let is_current_line = line_idx == cursor.line;
        let num_style = if is_current_line {
            line_num_current_style
        } else {
            line_num_style
        };
        if vl.is_first_seg {
            let num_str = format!("{:>width$}", line_idx + 1, width = gw as usize);
            buf.set_string(gutter_x, y, &num_str, num_style);
        } else {
            // Continuation rows of a wrapped line show blank gutter.
            let blank = " ".repeat(gw as usize);
            buf.set_string(gutter_x, y, &blank, line_num_style);
        }
        buf.set_string(gutter_x + gw, y, " ", line_num_style);

        // ── Text content ─────────────────────────────────────────────────────
        if text_area.width == 0 {
            continue;
        }

        let line_start_byte = handle
            .buffer
            .rope()
            .char_to_byte(handle.buffer.rope().line_to_char(line_idx));

        let mut screen_x = text_area.x;
        let max_x = text_area.x + text_area.width;
        let mut byte_offset = line_start_byte + vl.seg_byte;

        for grapheme in line_str_graphemes(&vl.display) {
            if screen_x >= max_x {
                break;
            }
            let gw_g = UnicodeWidthStr::width(grapheme) as u16;

            // Tabs have zero display width per unicode-width; expand them to the
            // next tab stop manually.
            if gw_g == 0 {
                if grapheme == "\t" && screen_x < max_x {
                    let col = (screen_x - text_area.x) as usize;
                    let tab_w = (tab_size - (col % tab_size)).max(1) as u16;
                    let style = style_for_byte(
                        byte_offset,
                        cursor.byte_offset,
                        &secondary_cursor_offsets,
                        has_selection,
                        selection,
                        search,
                        bracket_pair,
                        highlights,
                        theme,
                        cursor_style,
                        secondary_cursor_style,
                        selection_style,
                        current_match_style,
                        match_style,
                        bracket_style,
                        if show_whitespace {
                            whitespace_style
                        } else {
                            text_style
                        },
                    );
                    if show_whitespace {
                        // Render arrow glyph at the tab position, then fill with spaces.
                        buf.set_string(screen_x, y, "→", style);
                        let fill_end = (screen_x + tab_w).min(max_x);
                        for fx in (screen_x + 1)..fill_end {
                            buf.set_string(fx, y, " ", style);
                        }
                    } else {
                        // Render spaces to fill to next tab stop.
                        let fill_end = (screen_x + tab_w).min(max_x);
                        for fx in screen_x..fill_end {
                            buf.set_string(fx, y, " ", style);
                        }
                    }
                    screen_x = (screen_x + tab_w).min(max_x);
                }
                byte_offset += grapheme.len();
                continue;
            }

            // In show_whitespace mode, substitute space with middle dot.
            let (display_glyph, is_ws) = if show_whitespace && grapheme == " " {
                ("·", true)
            } else {
                (grapheme, false)
            };

            let style = style_for_byte(
                byte_offset,
                cursor.byte_offset,
                &secondary_cursor_offsets,
                has_selection,
                selection,
                search,
                bracket_pair,
                highlights,
                theme,
                cursor_style,
                secondary_cursor_style,
                selection_style,
                current_match_style,
                match_style,
                bracket_style,
                if is_ws { whitespace_style } else { text_style },
            );

            buf.set_string(screen_x, y, display_glyph, style);
            if gw_g > 1 && screen_x + 1 < max_x {
                buf.set_string(screen_x + 1, y, " ", style);
            }

            screen_x += gw_g;
            byte_offset += grapheme.len();
        }

        // Draw cursor at end of line (only on the last visual segment of the line).
        let is_last_seg = screen_row + 1 >= visual_lines.len()
            || visual_lines[screen_row + 1].line_idx != line_idx;
        if cursor.line == line_idx
            && cursor.col >= line_str_byte_len(&vl.display)
            && (is_last_seg || !handle.viewport.word_wrap)
            && screen_x < max_x
        {
            buf.set_string(screen_x, y, " ", cursor_style);
        }
        // Draw secondary cursors at end of line.
        if is_last_seg || !handle.viewport.word_wrap {
            for &(sc_line, sc_col) in &secondary_cursors_eol {
                if sc_line == line_idx
                    && sc_col >= line_str_byte_len(&vl.display)
                    && screen_x < max_x
                {
                    buf.set_string(screen_x, y, " ", secondary_cursor_style);
                }
            }
        }
    }

    // If the buffer is empty, show cursor on line 0.
    if total_lines == 0 && area.height > 0 {
        let y = area.y;
        let gutter_x = area.x + git_col_w;
        buf.set_string(gutter_x, y, "1", line_num_current_style);
        buf.set_string(gutter_x + gw, y, " ", line_num_style);
        if text_area.width > 0 {
            buf.set_string(text_area.x, y, " ", cursor_style);
        }
    }
}

/// Choose the highlight style for a grapheme at `byte_offset`.
#[allow(clippy::too_many_arguments)]
fn style_for_byte(
    byte_offset: usize,
    cursor_byte: usize,
    secondary_cursors: &[usize],
    has_selection: bool,
    selection: ByteRange,
    search: Option<&SearchState>,
    bracket_pair: Option<(usize, usize)>,
    highlights: &[HighlightSpan],
    theme: &ThemeColors,
    cursor_style: Style,
    secondary_cursor_style: Style,
    selection_style: Style,
    current_match_style: Style,
    match_style: Style,
    bracket_style: Style,
    text_style: Style,
) -> Style {
    // 1. Primary cursor
    if byte_offset == cursor_byte {
        return cursor_style;
    }
    // 1.5. Secondary cursors (multi-cursor mode)
    if secondary_cursors.contains(&byte_offset) {
        return secondary_cursor_style;
    }
    // 2. Selection
    if has_selection && byte_offset >= selection.start && byte_offset < selection.end {
        return selection_style;
    }
    // 3. Current search match
    if let Some(ss) = search {
        if let Some(cur) = ss.current_range()
            && byte_offset >= cur.start
            && byte_offset < cur.end
        {
            return current_match_style;
        }
        // 4. Other search matches
        for m in &ss.matches {
            if byte_offset >= m.start && byte_offset < m.end {
                return match_style;
            }
        }
    }
    // 5. Bracket pair
    if let Some((open, close)) = bracket_pair
        && (byte_offset == open || byte_offset == close)
    {
        return bracket_style;
    }
    // 6. Syntax highlight
    if let Some(span) = find_highlight(highlights, byte_offset) {
        return style_for_kind(span.kind, theme);
    }
    // 7. Default text
    text_style
}

/// Binary-search for the first highlight span that contains `byte`.
fn find_highlight(highlights: &[HighlightSpan], byte: usize) -> Option<&HighlightSpan> {
    // Spans are sorted by start and non-overlapping.
    let pos = highlights.partition_point(|s| s.end <= byte);
    highlights[pos..]
        .iter()
        .find(|s| s.start <= byte && byte < s.end)
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Number of columns needed for line numbers (at least 1).
pub fn gutter_width(total_lines: usize) -> u16 {
    let digits = if total_lines == 0 {
        1
    } else {
        (total_lines as f64).log10().floor() as u16 + 1
    };
    digits.max(1)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn text_area(area: Rect, gutter_w: u16, git_col_w: u16, diag_col_w: u16) -> Rect {
    let gutter_total = git_col_w + diag_col_w + gutter_w + GUTTER_PAD;
    if area.width <= gutter_total {
        return Rect::new(area.x + area.width, area.y, 0, area.height);
    }
    Rect::new(
        area.x + gutter_total,
        area.y,
        area.width - gutter_total,
        area.height,
    )
}

/// Byte offset in `line` at which display column `scroll_col` starts.
fn scroll_col_byte_offset(line: &str, scroll_col: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    if scroll_col == 0 {
        return 0;
    }
    let mut col = 0usize;
    for (byte_idx, grapheme) in line.grapheme_indices(true) {
        let w = UnicodeWidthStr::width(grapheme);
        if col + w > scroll_col {
            return byte_idx;
        }
        col += w;
        if col >= scroll_col {
            return byte_idx + grapheme.len();
        }
    }
    line.len()
}

fn line_str_graphemes(s: &str) -> impl Iterator<Item = &str> {
    use unicode_segmentation::UnicodeSegmentation;
    s.graphemes(true)
}

fn line_str_byte_len(s: &str) -> usize {
    s.len()
}

/// Find the matching bracket for the character at `cursor_byte`.
///
/// Returns `Some((open_byte, close_byte))` if a pair is found, else `None`.
/// Scans at most 100 000 chars in each direction.
fn find_matching_bracket(rope: &ropey::Rope, cursor_byte: usize) -> Option<(usize, usize)> {
    if cursor_byte >= rope.len_bytes() {
        return None;
    }
    let char_idx = rope.byte_to_char(cursor_byte);
    let ch = rope.char(char_idx);

    const MAX_SCAN: usize = 100_000;
    let total_chars = rope.len_chars();

    let (open_ch, close_ch, forward) = match ch {
        '{' => ('{', '}', true),
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '}' => ('{', '}', false),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        _ => return None,
    };

    if forward {
        let mut depth = 0i32;
        let limit = (char_idx + MAX_SCAN).min(total_chars);
        for i in char_idx..limit {
            let c = rope.char(i);
            if c == open_ch {
                depth += 1;
            } else if c == close_ch {
                depth -= 1;
                if depth == 0 {
                    return Some((cursor_byte, rope.char_to_byte(i)));
                }
            }
        }
    } else {
        let mut depth = 0i32;
        let start = char_idx.saturating_sub(MAX_SCAN);
        for i in (start..=char_idx).rev() {
            let c = rope.char(i);
            if c == close_ch {
                depth += 1;
            } else if c == open_ch {
                depth -= 1;
                if depth == 0 {
                    return Some((rope.char_to_byte(i), cursor_byte));
                }
            }
        }
    }

    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gutter_width_values() {
        assert_eq!(gutter_width(0), 1);
        assert_eq!(gutter_width(1), 1);
        assert_eq!(gutter_width(9), 1);
        assert_eq!(gutter_width(10), 2);
        assert_eq!(gutter_width(99), 2);
        assert_eq!(gutter_width(100), 3);
        assert_eq!(gutter_width(1000), 4);
    }

    #[test]
    fn text_area_layout_no_git() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, 0, 0);
        assert_eq!(ta.x, 4);
        assert_eq!(ta.width, 76);
    }

    #[test]
    fn text_area_layout_with_git_gutter() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, GIT_GUTTER_W, 0);
        // git(1) + line_num(3) + pad(1) = 5
        assert_eq!(ta.x, 5);
        assert_eq!(ta.width, 75);
    }

    #[test]
    fn text_area_too_narrow() {
        let area = Rect::new(0, 0, 3, 24);
        let ta = text_area(area, 3, 0, 0);
        assert_eq!(ta.width, 0);
    }

    #[test]
    fn text_area_layout_with_diagnostics() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, GIT_GUTTER_W, DIAG_GUTTER_W);
        // git(1) + diag(1) + line_num(3) + pad(1) = 6
        assert_eq!(ta.x, 6);
        assert_eq!(ta.width, 74);
    }

    #[test]
    fn bracket_match_open_brace() {
        use ropey::Rope;
        // "fn foo() { bar }"
        //  0123456789012345
        // '{' is at byte 9, '}' is at byte 15
        let rope = Rope::from_str("fn foo() { bar }");
        let result = find_matching_bracket(&rope, 9);
        assert_eq!(result, Some((9, 15)));
    }

    #[test]
    fn bracket_match_close_brace() {
        use ropey::Rope;
        let rope = Rope::from_str("fn foo() { bar }");
        let result = find_matching_bracket(&rope, 15);
        assert_eq!(result, Some((9, 15)));
    }

    #[test]
    fn bracket_match_nested() {
        use ropey::Rope;
        let rope = Rope::from_str("{ { } }");
        // outer '{' at 0 matches '}' at 6
        let result = find_matching_bracket(&rope, 0);
        assert_eq!(result, Some((0, 6)));
    }

    #[test]
    fn bracket_match_no_match() {
        use ropey::Rope;
        let rope = Rope::from_str("{ no close");
        let result = find_matching_bracket(&rope, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn bracket_match_non_bracket_char() {
        use ropey::Rope;
        let rope = Rope::from_str("hello");
        let result = find_matching_bracket(&rope, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn find_highlight_returns_matching_span() {
        let spans = vec![
            HighlightSpan {
                start: 0,
                end: 2,
                kind: crate::syntax::highlighter::HighlightKind::Keyword,
            },
            HighlightSpan {
                start: 5,
                end: 10,
                kind: crate::syntax::highlighter::HighlightKind::String,
            },
        ];
        assert!(find_highlight(&spans, 0).is_some());
        assert_eq!(
            find_highlight(&spans, 0).unwrap().kind,
            crate::syntax::highlighter::HighlightKind::Keyword
        );
        assert!(find_highlight(&spans, 1).is_some());
        assert!(find_highlight(&spans, 2).is_none()); // end is exclusive
        assert!(find_highlight(&spans, 3).is_none());
        assert!(find_highlight(&spans, 5).is_some());
        assert_eq!(
            find_highlight(&spans, 7).unwrap().kind,
            crate::syntax::highlighter::HighlightKind::String
        );
        assert!(find_highlight(&spans, 10).is_none());
    }

    #[test]
    fn find_highlight_empty_spans() {
        assert!(find_highlight(&[], 5).is_none());
    }
}
