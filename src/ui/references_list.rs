use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::ReferencesListState;

const MAX_VISIBLE: usize = 15;

/// Render the references list overlay centered in `area`.
pub fn render(refs: &ReferencesListState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let num_items = refs.items.len();
    let visible = num_items.min(MAX_VISIBLE);
    let popup_h = (visible as u16 + 4).min(area.height); // border + header + sep + items + border
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
    let path_style = Style::default().bg(bg).fg(Color::Rgb(120, 180, 220));
    let context_style = Style::default().bg(bg).fg(Color::Rgb(180, 180, 200));
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(100, 110, 150));

    // Background.
    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    draw_border(buf, popup, border_style);

    // Header.
    let header = format!(" References ({}) ", num_items);
    let hx = popup.x + popup.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, popup.y, &header, header_style);

    // Separator.
    draw_h_separator(buf, popup, popup.y + 2, border_style);

    // Scroll.
    let scroll = if refs.selected >= MAX_VISIBLE {
        refs.selected - MAX_VISIBLE + 1
    } else {
        0
    };

    let inner_w = popup.width.saturating_sub(2) as usize;

    for (i, item) in refs.items.iter().skip(scroll).take(visible).enumerate() {
        let y = popup.y + 3 + i as u16;
        if y >= popup.y + popup.height - 1 {
            break;
        }

        let is_selected = scroll + i == refs.selected;
        let row_bg = if is_selected { selected_bg } else { bg };

        for x in popup.x + 1..popup.x + popup.width - 1 {
            buf.set_string(x, y, " ", Style::default().bg(row_bg));
        }

        let file_name = item
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        let loc = format!("{}:{}:{}", file_name, item.line + 1, item.col + 1);
        let display: String = loc.chars().take(inner_w.saturating_sub(1)).collect();
        buf.set_string(popup.x + 2, y, &display, path_style.bg(row_bg));

        // Show context if space allows.
        let ctx_x = popup.x + 2 + display.len() as u16 + 2;
        let ctx_w = (popup.x + popup.width - 1).saturating_sub(ctx_x) as usize;
        if ctx_w > 3 && !item.context.is_empty() {
            let ctx: String = item.context.chars().take(ctx_w).collect();
            buf.set_string(ctx_x, y, &ctx, context_style.bg(row_bg));
        }
    }

    // Hint at bottom.
    let hint_y = popup.y + popup.height - 1;
    let hint = " Enter: go  Esc: close ";
    let hint_x = popup.x + popup.width.saturating_sub(hint.len() as u16) / 2;
    buf.set_string(hint_x, hint_y, hint, hint_style);
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
