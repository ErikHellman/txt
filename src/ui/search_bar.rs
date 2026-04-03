use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::search::SearchState;

/// Render the find / replace bar at the bottom of the editor area.
///
/// When `show_replace` is false, occupies 1 row.
/// When `show_replace` is true, occupies 2 rows.
pub fn render(search: &SearchState, area: Rect, buf: &mut TermBuffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let bg = Color::Rgb(25, 30, 50);
    let bar_style = Style::default().bg(bg).fg(Color::White);
    let label_style = Style::default().bg(bg).fg(Color::Rgb(140, 160, 200));
    let active_input_style = Style::default().bg(Color::Rgb(35, 40, 65)).fg(Color::White);
    let inactive_input_style = Style::default().bg(bg).fg(Color::Rgb(190, 190, 210));
    let flag_on_style = Style::default()
        .bg(Color::Rgb(40, 80, 40))
        .fg(Color::Rgb(120, 220, 120))
        .add_modifier(Modifier::BOLD);
    let flag_off_style = Style::default().bg(bg).fg(Color::Rgb(80, 80, 100));
    let count_style = Style::default().bg(bg).fg(Color::Rgb(180, 180, 100));

    // ── Search row ────────────────────────────────────────────────────────────
    let search_y = area.y;
    fill_row(buf, area.x, search_y, area.width, bar_style);

    let mut x = area.x;

    // Label
    let label = " Find: ";
    buf.set_string(x, search_y, label, label_style);
    x += label.len() as u16;

    // Query input
    let query_style = if !search.focus_replace {
        active_input_style
    } else {
        inactive_input_style
    };
    let query_display = format!("{}_", search.query);
    let query_w = ((area.width as usize).saturating_sub(x as usize + 30)).max(10);
    let query_str = pad_or_clip(&query_display, query_w);
    buf.set_string(x, search_y, &query_str, query_style);
    x += query_w as u16;

    // Spacing
    x += 1;

    // Flags: [Rx] [Cc]
    let rx_style = if search.is_regex {
        flag_on_style
    } else {
        flag_off_style
    };
    let cc_style = if search.case_sensitive {
        flag_on_style
    } else {
        flag_off_style
    };
    let rx_label = if search.is_regex { " Rx " } else { " rx " };
    let cc_label = if search.case_sensitive {
        " Cc "
    } else {
        " cc "
    };

    if x + 4 < area.x + area.width {
        buf.set_string(x, search_y, rx_label, rx_style);
        x += 4;
    }
    if x + 4 < area.x + area.width {
        buf.set_string(x, search_y, cc_label, cc_style);
        x += 4;
    }

    // Match count
    let count_str = if search.matches.is_empty() {
        if search.query.is_empty() {
            String::new()
        } else {
            " No matches".to_string()
        }
    } else {
        format!(" {}/{}", search.current_match + 1, search.matches.len())
    };
    if !count_str.is_empty() && x + (count_str.len() as u16) < area.x + area.width {
        buf.set_string(x + 1, search_y, &count_str, count_style);
    }

    // ── Replace row ───────────────────────────────────────────────────────────
    if search.show_replace && area.height >= 2 {
        let replace_y = area.y + 1;
        fill_row(buf, area.x, replace_y, area.width, bar_style);

        let mut rx = area.x;
        let rlabel = " Replace: ";
        buf.set_string(rx, replace_y, rlabel, label_style);
        rx += rlabel.len() as u16;

        let repl_style = if search.focus_replace {
            active_input_style
        } else {
            inactive_input_style
        };
        let repl_display = format!("{}_", search.replace_text);
        let repl_w = ((area.width as usize).saturating_sub(rx as usize + 22)).max(10);
        let repl_str = pad_or_clip(&repl_display, repl_w);
        buf.set_string(rx, replace_y, &repl_str, repl_style);
        rx += repl_w as u16 + 1;

        // Hints
        let hint_style = Style::default().bg(bg).fg(Color::Rgb(100, 100, 130));
        let hints = " Enter=replace  Ctrl+A=all  Tab=switch  Esc=close";
        let hint_available = (area.x + area.width).saturating_sub(rx) as usize;
        if hint_available > 5 {
            let h = &hints[..hints.len().min(hint_available)];
            buf.set_string(rx, replace_y, h, hint_style);
        }
    }
}

fn fill_row(buf: &mut TermBuffer, x: u16, y: u16, width: u16, style: Style) {
    for col in x..x + width {
        buf.set_string(col, y, " ", style);
    }
}

/// Pad string to `width` with spaces, or clip at `width` chars.
fn pad_or_clip(s: &str, width: usize) -> String {
    if s.len() >= width {
        let mut end = width;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    } else {
        format!("{:<width$}", s, width = width)
    }
}
