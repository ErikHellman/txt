use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Style},
};

use crate::app::HoverState;

const MAX_W: u16 = 60;
const MAX_H: u16 = 15;

/// Render the hover info popup near the cursor.
pub fn render(
    hover: &HoverState,
    cursor_screen_row: u16,
    cursor_screen_col: u16,
    area: Rect,
    buf: &mut TermBuffer,
) {
    if hover.content.is_empty() {
        return;
    }

    let lines: Vec<&str> = hover.content.lines().collect();
    let content_h = lines.len().min(MAX_H as usize) as u16;
    let content_w = lines
        .iter()
        .map(|l| l.len())
        .max()
        .unwrap_or(10)
        .min(MAX_W as usize) as u16;

    let popup_h = content_h + 2; // borders
    let popup_w = (content_w + 2).min(area.width);

    // Position: above the cursor if possible, otherwise below.
    let py = if cursor_screen_row > popup_h {
        cursor_screen_row - popup_h
    } else {
        cursor_screen_row + 1
    };
    let px = cursor_screen_col.min(area.x + area.width - popup_w);

    let popup = Rect::new(px, py, popup_w, popup_h);

    let bg = Color::Rgb(30, 33, 50);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 90, 140));
    let text_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));

    // Background.
    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // Border.
    draw_border(buf, popup, border_style);

    // Content.
    let inner_w = popup.width.saturating_sub(2) as usize;
    for (i, line) in lines.iter().take(content_h as usize).enumerate() {
        let y = popup.y + 1 + i as u16;
        let display: String = line.chars().take(inner_w).collect();
        buf.set_string(popup.x + 1, y, &display, text_style);
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
