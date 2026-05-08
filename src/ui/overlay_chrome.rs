//! Shared chrome helpers for floating overlays (welcome, changelog, etc.).
//!
//! `help_overlay.rs` and `settings_overlay.rs` predate this module and keep
//! their own private `draw_border`. New overlays should reuse the helper here.

use ratatui::{buffer::Buffer as TermBuffer, layout::Rect, style::Style};

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

    buf.set_string(x0, y0, "\u{256d}", style);
    buf.set_string(x1, y0, "\u{256e}", style);
    buf.set_string(x0, y1, "\u{2570}", style);
    buf.set_string(x1, y1, "\u{256f}", style);

    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "\u{2500}", style);
        buf.set_string(x, y1, "\u{2500}", style);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "\u{2502}", style);
        buf.set_string(x1, y, "\u{2502}", style);
    }
}
