//! `Ctrl+Shift+V` overlay: list the recent clipboard entries (most-recent
//! first), Enter pastes the highlighted entry. Shares the visual chrome of
//! `references_list.rs` deliberately so the editor's overlay family stays
//! consistent.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::ClipboardRingState;

const MAX_VISIBLE: usize = 15;

/// Render the clipboard-ring overlay centered in `area`.
pub fn render(ring: &ClipboardRingState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let num_items = ring.entries.len();
    let visible = num_items.min(MAX_VISIBLE);
    let popup_h = (visible as u16 + 4).min(area.height);
    let popup_w = (area.width * 2 / 3).max(40).min(area.width);
    let ox = area.x + area.width.saturating_sub(popup_w) / 2;
    let oy = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect::new(ox, oy, popup_w, popup_h);

    let bg = Color::Rgb(18, 22, 40);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 100, 160));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 200, 255))
        .add_modifier(Modifier::BOLD);
    let selected_bg = Color::Rgb(40, 55, 110);
    let idx_style = Style::default().bg(bg).fg(Color::Rgb(140, 160, 220));
    let body_style = Style::default().bg(bg).fg(Color::Rgb(220, 220, 230));
    let dim_style = Style::default().bg(bg).fg(Color::Rgb(120, 130, 160));
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(100, 110, 150));

    // Background.
    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    draw_border(buf, popup, border_style);

    let header = format!(" Clipboard ring ({}) ", num_items);
    let hx = popup.x + popup.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, popup.y, &header, header_style);

    draw_h_separator(buf, popup, popup.y + 2, border_style);

    let scroll = if ring.selected >= MAX_VISIBLE {
        ring.selected - MAX_VISIBLE + 1
    } else {
        0
    };

    let inner_w = popup.width.saturating_sub(2) as usize;

    for (i, entry) in ring.entries.iter().skip(scroll).take(visible).enumerate() {
        let y = popup.y + 3 + i as u16;
        if y >= popup.y + popup.height - 1 {
            break;
        }
        let is_selected = scroll + i == ring.selected;
        let row_bg = if is_selected { selected_bg } else { bg };

        for x in popup.x + 1..popup.x + popup.width - 1 {
            buf.set_string(x, y, " ", Style::default().bg(row_bg));
        }

        let idx_label = format!(" {:>2}  ", scroll + i + 1);
        buf.set_string(popup.x + 1, y, &idx_label, idx_style.bg(row_bg));

        // Show the first line, collapsing tabs/newlines so the preview is
        // single-row friendly. Trailing dots indicate truncation.
        let preview = first_line_preview(entry, inner_w.saturating_sub(idx_label.len() + 1));
        let preview_style = body_style.bg(row_bg);
        buf.set_string(
            popup.x + 1 + idx_label.len() as u16,
            y,
            &preview,
            preview_style,
        );

        if entry.contains('\n') {
            let suffix = " ⏎";
            let sx = popup.x + popup.width - 1 - suffix.len() as u16;
            buf.set_string(sx, y, suffix, dim_style.bg(row_bg));
        }
    }

    let hint_y = popup.y + popup.height - 1;
    let hint = " Enter: paste  ↑↓: select  Esc: close ";
    let hint_x = popup.x + popup.width.saturating_sub(hint.len() as u16) / 2;
    buf.set_string(hint_x, hint_y, hint, hint_style);
}

/// First-line preview, truncated to `max_w` display columns. Tabs become
/// spaces; trailing whitespace is trimmed. Adds `…` when truncated.
fn first_line_preview(text: &str, max_w: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    let line = line.replace('\t', "    ");
    let trimmed = line.trim_end_matches([' ']);
    if max_w == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (count, ch) in trimmed.chars().enumerate() {
        if count + 1 >= max_w {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
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

fn draw_h_separator(buf: &mut TermBuffer, area: Rect, y: u16, style: Style) {
    if area.width < 2 {
        return;
    }
    buf.set_string(area.x, y, "├", style);
    buf.set_string(area.x + area.width - 1, y, "┤", style);
    for x in area.x + 1..area.x + area.width - 1 {
        buf.set_string(x, y, "─", style);
    }
}

#[cfg(test)]
mod tests {
    use super::first_line_preview;

    #[test]
    fn preview_truncates_with_ellipsis() {
        let p = first_line_preview("hello world", 5);
        assert_eq!(p, "hell…");
    }

    #[test]
    fn preview_takes_first_line_only() {
        let p = first_line_preview("first\nsecond", 80);
        assert_eq!(p, "first");
    }

    #[test]
    fn preview_replaces_tabs_with_spaces() {
        let p = first_line_preview("a\tb", 80);
        assert_eq!(p, "a    b");
    }
}
