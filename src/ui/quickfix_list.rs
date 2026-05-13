//! Quickfix / location list overlay (Alt+1).

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::lsp::types::DiagSeverity;
use crate::quickfix::QuickfixState;

const MAX_VISIBLE: usize = 15;

pub fn render(qf: &QuickfixState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 10 || area.height < 6 {
        return;
    }
    let num_items = qf.entries.len();
    let visible = num_items.min(MAX_VISIBLE);
    let popup_h = (visible as u16 + 4).min(area.height);
    let popup_w = (area.width * 3 / 4).max(50).min(area.width);
    let ox = area.x + area.width.saturating_sub(popup_w) / 2;
    let oy = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect::new(ox, oy, popup_w, popup_h);

    let bg = Color::Rgb(18, 22, 40);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(110, 130, 180));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(220, 220, 255))
        .add_modifier(Modifier::BOLD);
    let selected_bg = Color::Rgb(40, 55, 110);
    let err_style = Style::default().bg(bg).fg(Color::Rgb(240, 100, 100));
    let warn_style = Style::default().bg(bg).fg(Color::Rgb(240, 200, 80));
    let info_style = Style::default().bg(bg).fg(Color::Rgb(120, 180, 240));
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(160, 160, 200));
    let path_style = Style::default().bg(bg).fg(Color::Rgb(140, 200, 240));
    let msg_style = Style::default().bg(bg).fg(Color::Rgb(220, 220, 230));

    for y in popup.y..popup.y + popup.height {
        for x in popup.x..popup.x + popup.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }
    draw_border(buf, popup, border_style);

    let header = format!(" Quickfix ({}) ", num_items);
    let hx = popup.x + popup.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, popup.y, &header, header_style);

    draw_h_separator(buf, popup, popup.y + 2, border_style);

    let scroll = if qf.selected >= MAX_VISIBLE {
        qf.selected - MAX_VISIBLE + 1
    } else {
        0
    };

    let inner_w = popup.width.saturating_sub(2) as usize;

    for (i, entry) in qf.entries.iter().skip(scroll).take(visible).enumerate() {
        let y = popup.y + 3 + i as u16;
        if y >= popup.y + popup.height - 1 {
            break;
        }
        let is_selected = scroll + i == qf.selected;
        let row_bg = if is_selected { selected_bg } else { bg };

        for x in popup.x + 1..popup.x + popup.width - 1 {
            buf.set_string(x, y, " ", Style::default().bg(row_bg));
        }

        // Severity glyph.
        let (sev_glyph, sev_style) = match entry.severity {
            DiagSeverity::Error => ("E", err_style.bg(row_bg)),
            DiagSeverity::Warning => ("W", warn_style.bg(row_bg)),
            DiagSeverity::Information => ("I", info_style.bg(row_bg)),
            DiagSeverity::Hint => ("H", hint_style.bg(row_bg)),
        };
        buf.set_string(popup.x + 2, y, sev_glyph, sev_style);

        let file_name = entry
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        let loc = format!(" {}:{}:{}", file_name, entry.line + 1, entry.col + 1);
        let path_display: String = loc.chars().take(inner_w.saturating_sub(4)).collect();
        buf.set_string(popup.x + 4, y, &path_display, path_style.bg(row_bg));

        let msg_x = popup.x + 4 + path_display.len() as u16 + 2;
        let msg_w = (popup.x + popup.width - 1).saturating_sub(msg_x) as usize;
        if msg_w > 3 && !entry.message.is_empty() {
            let msg: String = entry
                .message
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(msg_w)
                .collect();
            buf.set_string(msg_x, y, &msg, msg_style.bg(row_bg));
        }
    }

    let hint_y = popup.y + popup.height - 1;
    let hint = " Enter: jump  ↑↓: select  Esc: close ";
    let hx2 = popup.x + popup.width.saturating_sub(hint.len() as u16) / 2;
    buf.set_string(hx2, hint_y, hint, hint_style);
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
