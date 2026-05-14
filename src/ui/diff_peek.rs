//! Inline diff-peek float: small overlay showing the HEAD version of the
//! hunk at the cursor. Anchored near the cursor's screen row.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::DiffPeekState;
use crate::ui::overlay_chrome::draw_border;

const MAX_VISIBLE: usize = 10;

pub fn render(peek: &DiffPeekState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let visible = peek.head_lines.len().min(MAX_VISIBLE);
    let popup_h = (visible as u16 + 3).min(area.height);
    let popup_w = (area.width * 2 / 3).max(40).min(area.width);
    let ox = area.x + area.width.saturating_sub(popup_w) / 2;
    // Anchor below the cursor row when possible, otherwise centre.
    let oy = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect::new(ox, oy, popup_w, popup_h);

    let bg = Color::Rgb(15, 25, 30);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 140, 110));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 220, 200))
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().bg(bg).fg(Color::Rgb(220, 220, 220));
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(110, 130, 110));

    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }
    draw_border(buf, popup, border_style);

    let header = " HEAD (peek) ";
    let hx = popup.x + popup.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, popup.y, header, header_style);

    let inner_w = popup.width.saturating_sub(2) as usize;
    for (i, line) in peek.head_lines.iter().take(visible).enumerate() {
        let y = popup.y + 1 + i as u16;
        if y >= popup.y + popup.height - 1 {
            break;
        }
        let preview: String = line.chars().take(inner_w.saturating_sub(2)).collect();
        buf.set_string(popup.x + 2, y, &preview, body_style);
    }

    let hint_y = popup.y + popup.height - 1;
    let hint = " Alt+H toggles ";
    let hint_x = popup.x + popup.width.saturating_sub(hint.len() as u16) / 2;
    buf.set_string(hint_x, hint_y, hint, hint_style);
}
