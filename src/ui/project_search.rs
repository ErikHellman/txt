use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::ProjectSearchState;
use crate::theme::ThemeColors;

/// Render the project-wide search/replace overlay as a centered floating panel.
pub fn render(state: &ProjectSearchState, theme: &ThemeColors, area: Rect, buf: &mut TermBuffer) {
    let overlay_w = (area.width * 3 / 4).max(50).min(area.width);
    let overlay_h = (area.height * 3 / 4).max(10).min(area.height);
    let overlay_x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
    let overlay_y = area.y + area.height / 8;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_w, overlay_h);

    let bg = Style::default().bg(theme.picker_bg).fg(Color::White);
    let border = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(80, 80, 140));
    let header_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(180, 180, 220))
        .add_modifier(Modifier::BOLD);
    let query_style = Style::default().bg(theme.picker_bg).fg(Color::White);
    let path_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(150, 200, 255))
        .add_modifier(Modifier::BOLD);
    let match_style = bg;
    let selected_style = Style::default().bg(theme.picker_sel_bg).fg(Color::White);
    let dim_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(120, 120, 140));

    // Clear overlay area.
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", bg);
        }
    }

    // Border.
    buf.set_string(overlay.x, overlay.y, "┌", border);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, overlay.y, "─", border);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, overlay.y, "┐", border);
    }
    let bot_y = overlay.y + overlay.height.saturating_sub(1);
    buf.set_string(overlay.x, bot_y, "└", border);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, bot_y, "─", border);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, bot_y, "┘", border);
    }
    for y in overlay.y + 1..bot_y {
        buf.set_string(overlay.x, y, "│", border);
        if overlay.width >= 2 {
            buf.set_string(overlay.x + overlay.width - 1, y, "│", border);
        }
    }

    if overlay.height < 5 || overlay.width < 6 {
        return;
    }

    let inner_x = overlay.x + 1;
    let inner_w = overlay.width.saturating_sub(2);
    let mut current_y = overlay.y + 1;

    // Header.
    let header = format!(
        " Search in workspace   {} match{}{}",
        state.results.matches.len(),
        if state.results.matches.len() == 1 {
            ""
        } else {
            "es"
        },
        if state.results.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    let header_line = format!("{:<width$}", header, width = inner_w as usize);
    buf.set_string(inner_x, current_y, &header_line, header_style);
    current_y += 1;

    // Query input.
    if current_y >= bot_y {
        return;
    }
    let query_caret = if state.focus_replace { "" } else { "_" };
    let regex_flag = if state.is_regex { " [Rx]" } else { "" };
    let case_flag = if state.case_sensitive { " [Aa]" } else { "" };
    let query_prompt = format!(
        " Find:    {}{}{}{}",
        state.query, query_caret, regex_flag, case_flag
    );
    let query_line = format!("{:<width$}", query_prompt, width = inner_w as usize);
    let st = if state.focus_replace {
        dim_style
    } else {
        query_style
    };
    buf.set_string(inner_x, current_y, &query_line, st);
    current_y += 1;

    // Optional replace input.
    if state.show_replace && current_y < bot_y {
        let caret = if state.focus_replace { "_" } else { "" };
        let replace_prompt = format!(" Replace: {}{}", state.replace_text, caret);
        let replace_line = format!("{:<width$}", replace_prompt, width = inner_w as usize);
        let st = if state.focus_replace {
            query_style
        } else {
            dim_style
        };
        buf.set_string(inner_x, current_y, &replace_line, st);
        current_y += 1;
    }

    // Hints row.
    if current_y < bot_y {
        let hint = " Tab=replace field  Enter=open  Alt+R=regex  Alt+C=case  Ctrl+Enter=replace all  Esc=close";
        let hint_truncated = if hint.len() > inner_w as usize {
            &hint[..inner_w as usize]
        } else {
            hint
        };
        let hint_line = format!("{:<width$}", hint_truncated, width = inner_w as usize);
        buf.set_string(inner_x, current_y, &hint_line, dim_style);
        current_y += 1;
    }

    // Separator.
    if current_y < bot_y {
        for x in inner_x..inner_x + inner_w {
            buf.set_string(x, current_y, "─", border);
        }
        current_y += 1;
    }

    // Result list — flat rows with file path as a header before each file group.
    let list_rows = bot_y.saturating_sub(current_y) as usize;
    if list_rows == 0 {
        return;
    }

    // Build display rows: (kind, text) where kind is path-header or match.
    enum Row<'a> {
        Path(&'a std::path::Path),
        Match(usize), // index into state.results.matches
    }
    let mut rows: Vec<Row<'_>> = Vec::new();
    let mut last_path: Option<&std::path::Path> = None;
    for (idx, m) in state.results.matches.iter().enumerate() {
        let path: &std::path::Path = &m.path;
        if last_path != Some(path) {
            rows.push(Row::Path(path));
            last_path = Some(path);
        }
        rows.push(Row::Match(idx));
    }

    // Find which row corresponds to the selected match so we can keep it visible.
    let selected_row_idx = rows.iter().position(|r| match r {
        Row::Match(idx) => *idx == state.selected,
        _ => false,
    });

    let scroll = match selected_row_idx {
        Some(sel) if sel >= list_rows => sel.saturating_sub(list_rows - 1),
        _ => 0,
    };
    let scroll = scroll.min(rows.len().saturating_sub(list_rows.max(1)));

    for (screen_row, row) in rows.iter().skip(scroll).take(list_rows).enumerate() {
        let y = current_y + screen_row as u16;
        match row {
            Row::Path(p) => {
                let label = format!(" {}", p.display());
                let line = format!("{:<width$}", label, width = inner_w as usize);
                let display = if line.len() > inner_w as usize {
                    &line[..inner_w as usize]
                } else {
                    &line
                };
                buf.set_string(inner_x, y, display, path_style);
            }
            Row::Match(idx) => {
                let m = &state.results.matches[*idx];
                let is_selected = *idx == state.selected;
                let style = if is_selected {
                    selected_style
                } else {
                    match_style
                };
                let label = format!("   {:>5}: {}", m.line + 1, m.line_text);
                let line = format!("{:<width$}", label, width = inner_w as usize);
                let display = if line.len() > inner_w as usize {
                    &line[..inner_w as usize]
                } else {
                    &line
                };
                buf.set_string(inner_x, y, display, style);
            }
        }
    }

    if state.results.matches.is_empty() && !state.query.is_empty() {
        let msg = " No matches";
        let line = format!("{:<width$}", msg, width = inner_w as usize);
        buf.set_string(inner_x, current_y, line, dim_style);
    }
}
