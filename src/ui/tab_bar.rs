use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::editor::Editor;

/// Render the horizontal tab strip at the top of the editor.
pub fn render(editor: &Editor, area: Rect, buf: &mut TermBuffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let active_style = Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let inactive_style = Style::default()
        .bg(Color::Rgb(20, 20, 30))
        .fg(Color::Rgb(160, 160, 180));
    let sep_style = Style::default()
        .bg(Color::Rgb(20, 20, 30))
        .fg(Color::Rgb(60, 60, 80));

    // Fill the row with the inactive background.
    for x in area.x..area.x + area.width {
        buf.set_string(x, area.y, " ", inactive_style);
    }

    let mut x = area.x;
    for (i, tab) in editor.tabs.iter().enumerate() {
        if x >= area.x + area.width {
            break;
        }
        let is_active = i == editor.active_idx;
        let style = if is_active { active_style } else { inactive_style };
        let dot = if tab.buffer.modified { "•" } else { " " };
        let name = tab.display_name();
        let label = format!(" {}{} ", dot, name);

        let max_w = (area.x + area.width).saturating_sub(x) as usize;
        let label_w = label.len().min(max_w);
        buf.set_string(x, area.y, &label[..label_w], style);
        x += label_w as u16;

        // Separator between tabs (not after the last one).
        if i + 1 < editor.tabs.len() && x < area.x + area.width {
            buf.set_string(x, area.y, "│", sep_style);
            x += 1;
        }
    }
}
