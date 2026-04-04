use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Style},
};

use crate::app::CompletionState;

const MAX_VISIBLE: usize = 10;

/// Render the completion popup near the cursor position.
pub fn render(
    comp: &CompletionState,
    cursor_screen_row: u16,
    cursor_screen_col: u16,
    area: Rect,
    buf: &mut TermBuffer,
) {
    if comp.filtered.is_empty() {
        return;
    }

    let num_items = comp.filtered.len().min(MAX_VISIBLE);
    let popup_h = num_items as u16 + 2; // +2 for top/bottom border
    let popup_w = 40u16.min(area.width.saturating_sub(2));

    // Position: below the cursor, or above if near the bottom.
    let below = cursor_screen_row + 1;
    let py = if below + popup_h <= area.y + area.height {
        below
    } else {
        cursor_screen_row.saturating_sub(popup_h)
    };
    let px = cursor_screen_col.min(area.x + area.width - popup_w);

    let popup = Rect::new(px, py, popup_w, popup_h);

    // Styles.
    let bg = Color::Rgb(25, 28, 45);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(70, 80, 130));
    let selected_bg = Color::Rgb(50, 60, 120);
    let label_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));
    let kind_style = Style::default().bg(bg).fg(Color::Rgb(140, 160, 200));
    let detail_style = Style::default().bg(bg).fg(Color::Rgb(120, 120, 140));

    // Background.
    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // Border.
    draw_border(buf, popup, border_style);

    // Scroll offset.
    let scroll = if comp.selected >= MAX_VISIBLE {
        comp.selected - MAX_VISIBLE + 1
    } else {
        0
    };

    let inner_w = popup.width.saturating_sub(2) as usize;

    for (i, &item_idx) in comp
        .filtered
        .iter()
        .skip(scroll)
        .take(MAX_VISIBLE)
        .enumerate()
    {
        let y = popup.y + 1 + i as u16;
        let is_selected = scroll + i == comp.selected;
        let row_bg = if is_selected { selected_bg } else { bg };

        // Fill row.
        for x in popup.x + 1..popup.x + popup.width - 1 {
            buf.set_string(x, y, " ", Style::default().bg(row_bg));
        }

        if let Some(item) = comp.items.get(item_idx) {
            let cx = popup.x + 1;

            // Kind label (3 chars).
            buf.set_string(cx, y, item.kind_label, kind_style.bg(row_bg));

            // Label.
            let label: String = item.label.chars().take(inner_w.saturating_sub(4)).collect();
            buf.set_string(cx + 4, y, &label, label_style.bg(row_bg));

            // Detail (right-aligned, if space allows).
            if let Some(detail) = &item.detail {
                let max_detail = inner_w.saturating_sub(label.len() + 6);
                if max_detail > 3 {
                    let d: String = detail.chars().take(max_detail).collect();
                    let dx = popup.x + popup.width - 1 - d.len() as u16;
                    if dx > cx + 4 + label.len() as u16 {
                        buf.set_string(dx, y, &d, detail_style.bg(row_bg));
                    }
                }
            }
        }
    }
}

fn draw_border(buf: &mut TermBuffer, area: Rect, style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);
    buf.set_string(x0, y0, "╭", style);
    buf.set_string(x1, y0, "╮", style);
    buf.set_string(x0, y1, "╰", style);
    buf.set_string(x1, y1, "╯", style);
    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "─", style);
        buf.set_string(x, y1, "─", style);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "│", style);
        buf.set_string(x1, y, "│", style);
    }
}
