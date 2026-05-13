use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use crate::buffer::cursor::ByteRange;
use crate::buffer::edit::find_matching_bracket;
use crate::editor::tab::BufferHandle;
use crate::git::{GitGutter, GutterMark};
use crate::lsp::types::DiagSeverity;
use crate::search::SearchState;
use crate::syntax::highlighter::{HighlightSpan, style_for_kind};
use crate::theme::ThemeColors;

/// Width of a single space used as a separator between gutter and text.
pub const GUTTER_PAD: u16 = 1;
/// Width of the git gutter column (shown left of line numbers when active).
pub const GIT_GUTTER_W: u16 = 1;
/// Width of the diagnostic gutter column (shown when diagnostics are present).
pub const DIAG_GUTTER_W: u16 = 1;
/// Width of the fold-chevron gutter column (shown when any fold candidates exist).
pub const FOLD_GUTTER_W: u16 = 1;

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
    new_version: Option<&str>,
    focused: bool,
    show_whitespace: bool,
    tab_size: usize,
    indent_guides: bool,
    rulers: &[usize],
    highlight_trailing_ws: bool,
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
    // Fold gutter is shown only when the buffer has at least one foldable
    // candidate range — keeps the gutter tight for plaintext and short files.
    let has_folds = (0..total_lines).any(|i| handle.folds.is_fold_start_candidate(i));
    let fold_col_w: u16 = if has_folds { FOLD_GUTTER_W } else { 0 };
    // When a newer release is available, widen the line-number gutter just
    // enough to fit `↑X.Y.Z` on row 0 so the badge never has to truncate.
    let version_label = new_version.map(|v| format!("↑{v}"));
    let gw = effective_gutter_width(total_lines, version_label.as_deref());
    let text_area = text_area(area, gw, git_col_w, diag_col_w, fold_col_w);

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

    let (secondary_cursor_offsets, secondary_cursors_eol): (Vec<usize>, Vec<(usize, usize)>) =
        handle
            .buffer
            .cursors
            .secondary_cursors()
            .map(|c| (c.byte_offset, (c.line, c.col)))
            .unzip();

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
    let secondary_cursor_style = if focused {
        Style::default()
            .bg(Color::Rgb(60, 140, 80))
            .fg(Color::Black)
    } else {
        Style::default()
            .bg(Color::Rgb(40, 80, 50))
            .fg(Color::Rgb(120, 160, 120))
    };
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
    // Walk extra lines so folds collapsing several screen rows don't leave
    // empty space at the bottom of the editor. 4x is a generous upper bound
    // for typical fold ratios.
    let walk_lines = if has_folds { height * 4 } else { height };
    let visual_lines: Vec<VisualLine> = if handle.viewport.word_wrap && text_area.width > 0 {
        let wrapped = handle.viewport.visible_lines_wrapped(
            &handle.buffer,
            walk_lines,
            text_area.width as usize,
        );
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
            .filter(|vl| !handle.folds.is_line_hidden(vl.line_idx))
            .take(height)
            .collect()
    } else {
        handle
            .viewport
            .visible_lines(&handle.buffer, walk_lines)
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
            .filter(|vl| !handle.folds.is_line_hidden(vl.line_idx))
            .take(height)
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

        // ── Fold gutter ──────────────────────────────────────────────────────
        if has_folds && vl.is_first_seg {
            let fold_x = area.x + git_col_w + diag_col_w;
            let folded = handle.folds.is_fold_start_folded(line_idx);
            let candidate = handle.folds.is_fold_start_candidate(line_idx);
            let (sym, sty) = if folded {
                ("▸", Style::default().fg(Color::Rgb(220, 180, 80)))
            } else if candidate {
                ("▾", Style::default().fg(Color::Rgb(90, 100, 120)))
            } else {
                (" ", Style::default())
            };
            buf.set_string(fold_x, y, sym, sty);
        }

        // ── Gutter (line number) ─────────────────────────────────────────────
        let gutter_x = area.x + git_col_w + diag_col_w + fold_col_w;
        let is_current_line = line_idx == cursor.line;
        let num_style = if is_current_line {
            line_num_current_style
        } else {
            line_num_style
        };
        if screen_row == 0 && version_label.is_some() {
            // Overlay the topmost gutter row with the "new release available"
            // badge — the line number for this row gets replaced.
            let label = version_label.as_deref().unwrap();
            let badge_style = Style::default()
                .fg(Color::Rgb(255, 230, 100))
                .bg(Color::Rgb(80, 60, 0))
                .add_modifier(Modifier::BOLD);
            let mut padded = label.to_string();
            let label_w = UnicodeWidthStr::width(label) as u16;
            if label_w < gw {
                padded.push_str(&" ".repeat((gw - label_w) as usize));
            }
            buf.set_string(gutter_x, y, &padded, badge_style);
        } else if vl.is_first_seg {
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

        // Append "▸ N lines" marker on the start line of a folded range so
        // the reader knows the content is hidden, not just blank.
        if vl.is_first_seg
            && let Some(end_line) = handle.folds.folded_end_line(line_idx)
            && screen_x + 2 < max_x
        {
            let n = end_line - line_idx;
            let label = format!("  ▸ {n} lines");
            let fold_inline_style = Style::default().fg(Color::Rgb(180, 140, 40));
            let avail = (max_x - screen_x) as usize;
            let clipped: String = label.chars().take(avail).collect();
            buf.set_string(screen_x, y, &clipped, fold_inline_style);
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

    // ── Indent guides and column rulers (post-pass overlay) ───────────────
    // Both are drawn *after* the main text pass and only on cells that still
    // show a plain space — they never overwrite cursors, selections, or the
    // first non-whitespace character of a line.
    if (indent_guides && tab_size > 0) || !rulers.is_empty() {
        let guide_style = Style::default().fg(Color::Rgb(70, 70, 95));
        let max_x = text_area.x + text_area.width;
        for (screen_row, vl) in visual_lines.iter().enumerate() {
            let y = area.y + screen_row as u16;

            // Compute first non-whitespace display column for this logical
            // line (only meaningful on the first visual segment).
            let first_non_ws = if indent_guides && vl.is_first_seg {
                first_non_whitespace_display_col(&handle.buffer.line_str(vl.line_idx), tab_size)
            } else {
                0
            };

            if indent_guides && vl.is_first_seg && first_non_ws >= tab_size {
                let mut col = tab_size;
                while col < first_non_ws {
                    let x = text_area.x + col as u16;
                    if x >= max_x {
                        break;
                    }
                    overlay_guide(buf, x, y, "│", guide_style);
                    col += tab_size;
                }
            }

            for &ruler in rulers {
                if ruler == 0 || ruler >= text_area.width as usize {
                    continue;
                }
                let x = text_area.x + ruler as u16;
                if x < max_x {
                    overlay_guide(buf, x, y, "│", guide_style);
                }
            }
        }
    }

    // ── Trailing-whitespace highlight (post-pass overlay) ─────────────────
    // Paint a subtle red background on cells that hold a trailing space or
    // tab in the *logical* line. Skipped if the line is the cursor's current
    // line (so editing midway through a line doesn't strobe red while the
    // user is typing). Also skips lines that are entirely whitespace —
    // legitimate blank lines in source files.
    if highlight_trailing_ws && text_area.width > 0 {
        let tws_style = Style::default().bg(Color::Rgb(110, 30, 30));
        let max_x = text_area.x + text_area.width;
        let cursor_line = handle.buffer.cursors.primary().line;
        for (screen_row, vl) in visual_lines.iter().enumerate() {
            if vl.line_idx == cursor_line {
                continue;
            }
            let line_str = handle.buffer.line_str(vl.line_idx);
            if line_str.is_empty() {
                continue;
            }
            // Find byte offset of last non-whitespace char. If all whitespace,
            // skip — that's a blank line, not a trailing-ws violation.
            let trimmed_end = line_str.trim_end_matches([' ', '\t']);
            if trimmed_end.is_empty() || trimmed_end.len() == line_str.len() {
                continue;
            }
            let trail_start_byte = trimmed_end.len();
            // Convert byte offset within the displayed segment to display
            // column. Trailing-ws lives at the end so it should appear in
            // the last visual segment of the line.
            let display_start_byte = vl.seg_byte;
            if trail_start_byte < display_start_byte {
                // Trailing-ws was before the start of this segment (shouldn't
                // happen given the line was non-empty), bail.
                continue;
            }
            // Walk the visible display until we reach trail_start_byte.
            let mut col_x = text_area.x;
            let mut byte = display_start_byte;
            for grapheme in line_str_graphemes(&vl.display) {
                if byte >= trail_start_byte {
                    break;
                }
                let gw_g = UnicodeWidthStr::width(grapheme) as u16;
                if grapheme == "\t" {
                    let col = (col_x - text_area.x) as usize;
                    let tab_w = (tab_size - (col % tab_size)).max(1) as u16;
                    col_x = (col_x + tab_w).min(max_x);
                } else {
                    col_x += gw_g.max(1);
                }
                byte += grapheme.len();
            }
            // Fill from col_x to end of segment with the trailing-ws style,
            // but only across cells whose symbol is still a plain space (so
            // we don't clobber cursors or selections).
            for x in col_x..max_x {
                let y = area.y + screen_row as u16;
                let pos = ratatui::layout::Position::new(x, y);
                if let Some(cell) = buf.cell(pos) {
                    let sym = cell.symbol();
                    if sym == " " || sym.is_empty() {
                        buf.set_string(x, y, " ", tws_style);
                    } else if sym == "·" {
                        // show_whitespace already drew middle-dots; tint them red too.
                        buf.set_string(x, y, "·", tws_style.fg(Color::Rgb(220, 180, 180)));
                    } else if sym == "→" {
                        buf.set_string(x, y, "→", tws_style.fg(Color::Rgb(220, 180, 180)));
                    }
                }
            }
        }
    }
}

/// Display column of the first non-whitespace character on `line`. If the line
/// is entirely whitespace (or empty), returns the display width of all leading
/// whitespace — i.e. "any indent guide column up to here is fine to draw".
fn first_non_whitespace_display_col(line: &str, tab_size: usize) -> usize {
    let mut col = 0usize;
    for grapheme in line_str_graphemes(line) {
        match grapheme {
            " " => col += 1,
            "\t" => col += tab_size.saturating_sub(col % tab_size).max(1),
            _ => return col,
        }
    }
    col
}

/// Overlay a guide glyph at `(x, y)` only when the underlying cell still
/// shows a plain space and has the default background. Prevents indent/ruler
/// guides from clobbering cursors, selections, or syntax highlights.
fn overlay_guide(buf: &mut TermBuffer, x: u16, y: u16, glyph: &str, style: Style) {
    use ratatui::layout::Position;
    let pos = Position::new(x, y);
    if let Some(cell) = buf.cell(pos) {
        let symbol = cell.symbol();
        let has_bg_style = cell.bg != Color::Reset;
        if (symbol == " " || symbol.is_empty()) && !has_bg_style {
            buf.set_string(x, y, glyph, style);
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

/// Line-number gutter width, widened to fit a row-0 badge such as the
/// "new release available" indicator. Mouse hit-testing must use this so the
/// text area's x-origin matches the renderer.
pub fn effective_gutter_width(total_lines: usize, version_label: Option<&str>) -> u16 {
    let base = gutter_width(total_lines);
    match version_label {
        Some(l) => base.max(UnicodeWidthStr::width(l) as u16),
        None => base,
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn text_area(area: Rect, gutter_w: u16, git_col_w: u16, diag_col_w: u16, fold_col_w: u16) -> Rect {
    let gutter_total = git_col_w + diag_col_w + fold_col_w + gutter_w + GUTTER_PAD;
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
    fn first_non_ws_display_col_blank_line_returns_indent_width() {
        // 8 spaces, no content → returns 8 so guides span the whole indent.
        assert_eq!(first_non_whitespace_display_col("        ", 4), 8);
    }

    #[test]
    fn first_non_ws_display_col_spaces_then_text() {
        assert_eq!(first_non_whitespace_display_col("    fn foo()", 4), 4);
        assert_eq!(first_non_whitespace_display_col("        x", 4), 8);
    }

    #[test]
    fn first_non_ws_display_col_tabs_round_to_tab_stop() {
        // One tab at width 4 → column 4.
        assert_eq!(first_non_whitespace_display_col("\tfoo", 4), 4);
        // Two tabs → column 8.
        assert_eq!(first_non_whitespace_display_col("\t\tfoo", 4), 8);
        // Tab at width 8.
        assert_eq!(first_non_whitespace_display_col("\tfoo", 8), 8);
    }

    #[test]
    fn first_non_ws_display_col_no_indent_returns_zero() {
        assert_eq!(first_non_whitespace_display_col("hello", 4), 0);
    }

    #[test]
    fn text_area_layout_no_git() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, 0, 0, 0);
        assert_eq!(ta.x, 4);
        assert_eq!(ta.width, 76);
    }

    #[test]
    fn text_area_layout_with_git_gutter() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, GIT_GUTTER_W, 0, 0);
        // git(1) + line_num(3) + pad(1) = 5
        assert_eq!(ta.x, 5);
        assert_eq!(ta.width, 75);
    }

    #[test]
    fn text_area_too_narrow() {
        let area = Rect::new(0, 0, 3, 24);
        let ta = text_area(area, 3, 0, 0, 0);
        assert_eq!(ta.width, 0);
    }

    #[test]
    fn text_area_layout_with_diagnostics() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, GIT_GUTTER_W, DIAG_GUTTER_W, 0);
        // git(1) + diag(1) + line_num(3) + pad(1) = 6
        assert_eq!(ta.x, 6);
        assert_eq!(ta.width, 74);
    }

    #[test]
    fn text_area_layout_with_fold_gutter() {
        let area = Rect::new(0, 0, 80, 24);
        let ta = text_area(area, 3, GIT_GUTTER_W, DIAG_GUTTER_W, FOLD_GUTTER_W);
        // git(1) + diag(1) + fold(1) + line_num(3) + pad(1) = 7
        assert_eq!(ta.x, 7);
        assert_eq!(ta.width, 73);
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
