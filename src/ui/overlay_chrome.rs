//! Shared chrome helpers for floating overlays.
//!
//! Every centred float in `src/ui/` follows the same chrome pattern:
//! a solid background fill, a rounded-corner border, optional internal
//! `├─────┤` separators, and a centred header line. The helpers below are
//! the single source of truth for that chrome — overlay modules should
//! call them instead of reimplementing the loops.

use ratatui::{buffer::Buffer as TermBuffer, layout::Rect, style::Style};

/// Fill every cell of `area` with a single space using `style`. Use this to
/// paint the background of an overlay before drawing its border and content.
pub fn fill_rect(buf: &mut TermBuffer, area: Rect, style: Style) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf.set_string(x, y, " ", style);
        }
    }
}

/// Draw a rounded box around `area` using the given style. No-op for areas
/// smaller than 2×2.
pub fn draw_border(buf: &mut TermBuffer, area: Rect, style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let x0 = area.x;
    let y0 = area.y;
    let x1 = area.x + area.width - 1;
    let y1 = area.y + area.height - 1;

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

/// Draw a horizontal `├─────┤` separator at row `y` spanning the width of
/// `area`. The caller is responsible for ensuring `y` lies inside the border.
pub fn draw_h_separator(buf: &mut TermBuffer, area: Rect, y: u16, style: Style) {
    if area.width < 2 {
        return;
    }
    buf.set_string(area.x, y, "├", style);
    buf.set_string(area.x + area.width - 1, y, "┤", style);
    for x in area.x + 1..area.x + area.width - 1 {
        buf.set_string(x, y, "─", style);
    }
}

/// Render `text` horizontally centred on row `y` of `area`, using `style`.
/// The string is written verbatim — callers must pre-truncate it to fit.
pub fn render_centered_header(buf: &mut TermBuffer, area: Rect, y: u16, text: &str, style: Style) {
    let tx = area.x + area.width.saturating_sub(text.len() as u16) / 2;
    buf.set_string(tx, y, text, style);
}
