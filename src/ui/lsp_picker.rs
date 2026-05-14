use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::{LSP_SERVER_OPTIONS, LspPickerState};
use crate::ui::overlay_chrome::{draw_border, draw_h_separator};

// ── Layout constants ─────────────────────────────────────────────────────────

const OVERLAY_W: u16 = 50;
// Rows: border-top + header + separator + (1 Disabled + N servers) + separator + 2 notice rows + hint + border-bot
const CHROME_ROWS: u16 = 8; // top border + header + sep + sep + notice×2 + hint + bottom border

/// Render the LSP configuration picker overlay centered in `area`.
pub fn render(picker: &LspPickerState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let num_items = picker.num_rows();
    let oh = (CHROME_ROWS + num_items as u16).min(area.height);
    let ow = OVERLAY_W.min(area.width);
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    // ── Styles ───────────────────────────────────────────────────────────────
    let bg = Color::Rgb(18, 22, 40);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 100, 160));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 200, 255))
        .add_modifier(Modifier::BOLD);
    let selected_bg = Color::Rgb(40, 55, 110);
    let disabled_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(180, 100, 100))
        .add_modifier(Modifier::BOLD);
    let server_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));
    let active_marker_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(120, 220, 120))
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(100, 110, 150));
    let notice_style = Style::default().bg(bg).fg(Color::Rgb(255, 200, 80));

    // ── Background fill ──────────────────────────────────────────────────────
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // ── Border ───────────────────────────────────────────────────────────────
    draw_border(buf, overlay, border_style);

    // ── Header ───────────────────────────────────────────────────────────────
    let header = " LSP Server ";
    let hx = overlay.x + overlay.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, overlay.y, header, header_style);

    // Separator after header
    draw_h_separator(buf, overlay, overlay.y + 2, border_style);

    // ── Option rows ──────────────────────────────────────────────────────────
    let inner_w = overlay.width.saturating_sub(2) as usize;

    for i in 0..num_items {
        let row_y = overlay.y + 3 + i as u16;
        if row_y >= overlay.y + overlay.height - 2 {
            break;
        }

        let is_selected = picker.selected == i;
        let row_bg = if is_selected { selected_bg } else { bg };

        // Fill row background
        for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
            buf.set_string(x, row_y, " ", Style::default().bg(row_bg));
        }

        let cx = overlay.x + 2;

        if i == 0 {
            // "Disabled" option
            let label = "Disabled (tree-sitter only)";
            let style = disabled_style.bg(row_bg);
            let display: String = label.chars().take(inner_w.saturating_sub(2)).collect();
            buf.set_string(cx, row_y, &display, style);
        } else {
            // Server option: "Language (command)"
            let (language, _name, command, _) = LSP_SERVER_OPTIONS[i - 1];
            let label = format!("{} ({})", language, command);
            let style = server_style.bg(row_bg);
            let display: String = label.chars().take(inner_w.saturating_sub(6)).collect();
            buf.set_string(cx, row_y, &display, style);
        }

        // Show a check mark for the currently selected item
        if is_selected {
            let marker = ">";
            let marker_style = active_marker_style.bg(row_bg);
            buf.set_string(overlay.x + 1, row_y, marker, marker_style);
        }
    }

    // Separator before hint
    let sep_y = overlay.y + 3 + num_items as u16;
    if sep_y < overlay.y + overlay.height - 1 {
        draw_h_separator(buf, overlay, sep_y, border_style);
    }

    // ── Notice rows ──────────────────────────────────────────────────────────
    let notices = [
        "Install LSP server externally before enabling.",
        "Edit .txt/lsp.toml for advanced configuration.",
    ];
    for (i, line) in notices.iter().enumerate() {
        let row_y = sep_y + 1 + i as u16;
        if row_y >= overlay.y + overlay.height {
            break;
        }
        let row_x = overlay.x + overlay.width.saturating_sub(line.len() as u16 + 2) / 2 + 1;
        let truncated: String = line.chars().take(inner_w).collect();
        buf.set_string(row_x, row_y, &truncated, notice_style);
    }

    // ── Hint row ─────────────────────────────────────────────────────────────
    let hint = "Enter: select  ·  Esc: cancel";
    let hint_y = sep_y + 1 + notices.len() as u16;
    if hint_y < overlay.y + overlay.height {
        let hint_x = overlay.x + overlay.width.saturating_sub(hint.len() as u16 + 2) / 2 + 1;
        let truncated: String = hint.chars().take(inner_w).collect();
        buf.set_string(hint_x, hint_y, &truncated, hint_style);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────
