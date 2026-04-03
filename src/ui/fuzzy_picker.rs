use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::FuzzyPickerState;

/// Render the fuzzy file picker as a centered floating overlay.
pub fn render(picker: &FuzzyPickerState, area: Rect, buf: &mut TermBuffer) {
    // ── Compute overlay dimensions ────────────────────────────────────────────
    let overlay_w = (area.width * 2 / 3).max(40).min(area.width);
    let overlay_h = (area.height * 2 / 3).max(8).min(area.height);
    let overlay_x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
    let overlay_y = area.y + area.height / 6; // slightly above centre

    let overlay = Rect::new(overlay_x, overlay_y, overlay_w, overlay_h);

    // ── Styles ────────────────────────────────────────────────────────────────
    let bg_style = Style::default()
        .bg(Color::Rgb(25, 25, 40))
        .fg(Color::White);
    let border_style = Style::default()
        .bg(Color::Rgb(25, 25, 40))
        .fg(Color::Rgb(80, 80, 140));
    let header_style = Style::default()
        .bg(Color::Rgb(35, 35, 55))
        .fg(Color::Rgb(180, 180, 220))
        .add_modifier(Modifier::BOLD);
    let query_style = Style::default()
        .bg(Color::Rgb(35, 35, 55))
        .fg(Color::White);
    let selected_style = Style::default()
        .bg(Color::Rgb(60, 80, 140))
        .fg(Color::White);
    let item_style = bg_style;

    // ── Clear the overlay area ────────────────────────────────────────────────
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", bg_style);
        }
    }

    // ── Border (top + bottom + sides) ────────────────────────────────────────
    // Top border
    buf.set_string(overlay.x, overlay.y, "┌", border_style);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, overlay.y, "─", border_style);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, overlay.y, "┐", border_style);
    }
    // Bottom border
    let bot_y = overlay.y + overlay.height.saturating_sub(1);
    buf.set_string(overlay.x, bot_y, "└", border_style);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, bot_y, "─", border_style);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, bot_y, "┘", border_style);
    }
    // Side borders
    for y in overlay.y + 1..bot_y {
        buf.set_string(overlay.x, y, "│", border_style);
        if overlay.width >= 2 {
            buf.set_string(overlay.x + overlay.width - 1, y, "│", border_style);
        }
    }

    if overlay.height < 3 || overlay.width < 4 {
        return;
    }

    // Inner area (inside the border).
    let inner_x = overlay.x + 1;
    let inner_w = overlay.width.saturating_sub(2);
    let mut current_y = overlay.y + 1;

    // ── Header ────────────────────────────────────────────────────────────────
    let header = " Go to file";
    let header_line = format!("{:<width$}", header, width = inner_w as usize);
    buf.set_string(inner_x, current_y, &header_line, header_style);
    current_y += 1;

    // ── Query input ───────────────────────────────────────────────────────────
    if current_y >= bot_y {
        return;
    }
    let query_prompt = format!(" > {}_", picker.query);
    let query_line = format!("{:<width$}", query_prompt, width = inner_w as usize);
    buf.set_string(inner_x, current_y, &query_line, query_style);
    current_y += 1;

    // Separator
    if current_y < bot_y {
        for x in inner_x..inner_x + inner_w {
            buf.set_string(x, current_y, "─", border_style);
        }
        current_y += 1;
    }

    // ── Results list ──────────────────────────────────────────────────────────
    let list_rows = bot_y.saturating_sub(current_y) as usize;

    // Compute scroll offset so the selected row is always in view.
    let scroll = if picker.selected >= list_rows && list_rows > 0 {
        picker.selected - list_rows + 1
    } else {
        0
    };

    for (screen_row, (_, file_idx)) in picker.filtered.iter().skip(scroll).take(list_rows).enumerate() {
        let y = current_y + screen_row as u16;
        let global_idx = scroll + screen_row;
        let is_selected = global_idx == picker.selected;
        let style = if is_selected { selected_style } else { item_style };

        let path = picker.all_files.get(*file_idx)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let label = format!(" {}", path);
        let label_line = format!("{:<width$}", label, width = inner_w as usize);
        // Clip to inner_w bytes (might truncate mid-grapheme for very long paths, acceptable).
        let display = if label_line.len() > inner_w as usize {
            &label_line[..inner_w as usize]
        } else {
            &label_line
        };
        buf.set_string(inner_x, y, display, style);
    }

    // Empty state
    if picker.filtered.is_empty() && list_rows > 0 {
        let msg = " No files found";
        buf.set_string(inner_x, current_y, &format!("{:<width$}", msg, width = inner_w as usize),
            Style::default().bg(Color::Rgb(25, 25, 40)).fg(Color::DarkGray));
    }
}
