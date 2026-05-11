//! Render the symbols-in-file picker (Ctrl+Shift+O).
//!
//! Same overall layout as [`super::fuzzy_picker`] — centered float with a
//! query line and a scrollable result list — but each row shows a kind glyph
//! (`fn`, `struct`, `class`, …) before the symbol name.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::SymbolPickerState;
use crate::theme::ThemeColors;

pub fn render(picker: &SymbolPickerState, theme: &ThemeColors, area: Rect, buf: &mut TermBuffer) {
    let overlay_w = (area.width * 2 / 3).max(40).min(area.width);
    let overlay_h = (area.height * 2 / 3).max(8).min(area.height);
    let overlay_x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
    let overlay_y = area.y + area.height / 6;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_w, overlay_h);

    let bg_style = Style::default().bg(theme.picker_bg).fg(Color::White);
    let border_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(80, 80, 140));
    let header_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(180, 180, 220))
        .add_modifier(Modifier::BOLD);
    let query_style = Style::default().bg(theme.picker_bg).fg(Color::White);
    let selected_style = Style::default().bg(theme.picker_sel_bg).fg(Color::White);
    let kind_style = Style::default()
        .bg(theme.picker_bg)
        .fg(Color::Rgb(150, 180, 210));
    let kind_sel_style = Style::default()
        .bg(theme.picker_sel_bg)
        .fg(Color::Rgb(200, 220, 240))
        .add_modifier(Modifier::BOLD);
    let item_style = bg_style;

    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", bg_style);
        }
    }

    // Border (top, sides, bottom).
    buf.set_string(overlay.x, overlay.y, "┌", border_style);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, overlay.y, "─", border_style);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, overlay.y, "┐", border_style);
    }
    let bot_y = overlay.y + overlay.height.saturating_sub(1);
    buf.set_string(overlay.x, bot_y, "└", border_style);
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, bot_y, "─", border_style);
    }
    if overlay.width >= 2 {
        buf.set_string(overlay.x + overlay.width - 1, bot_y, "┘", border_style);
    }
    for y in overlay.y + 1..bot_y {
        buf.set_string(overlay.x, y, "│", border_style);
        if overlay.width >= 2 {
            buf.set_string(overlay.x + overlay.width - 1, y, "│", border_style);
        }
    }

    if overlay.height < 3 || overlay.width < 4 {
        return;
    }

    let inner_x = overlay.x + 1;
    let inner_w = overlay.width.saturating_sub(2);
    let mut current_y = overlay.y + 1;

    let header = " Go to symbol";
    let header_line = format!("{:<width$}", header, width = inner_w as usize);
    buf.set_string(inner_x, current_y, &header_line, header_style);
    current_y += 1;

    if current_y >= bot_y {
        return;
    }
    let query_prompt = format!(" > {}_", picker.query);
    let query_line = format!("{:<width$}", query_prompt, width = inner_w as usize);
    buf.set_string(inner_x, current_y, &query_line, query_style);
    current_y += 1;

    if current_y < bot_y {
        for x in inner_x..inner_x + inner_w {
            buf.set_string(x, current_y, "─", border_style);
        }
        current_y += 1;
    }

    let list_rows = bot_y.saturating_sub(current_y) as usize;
    let scroll = if picker.selected >= list_rows && list_rows > 0 {
        picker.selected - list_rows + 1
    } else {
        0
    };

    let kind_col_w: usize = 9;
    for (screen_row, (_, sym_idx)) in picker
        .filtered
        .iter()
        .skip(scroll)
        .take(list_rows)
        .enumerate()
    {
        let y = current_y + screen_row as u16;
        let global_idx = scroll + screen_row;
        let is_selected = global_idx == picker.selected;
        let row_style = if is_selected {
            selected_style
        } else {
            item_style
        };
        let kind_st = if is_selected {
            kind_sel_style
        } else {
            kind_style
        };

        let Some(sym) = picker.all_symbols.get(*sym_idx) else {
            continue;
        };

        // Fill row background.
        let blank = format!("{:<width$}", "", width = inner_w as usize);
        buf.set_string(inner_x, y, &blank, row_style);

        let kind_label = format!(" {:<width$}", sym.kind, width = kind_col_w - 1);
        let kind_display = if kind_label.len() > kind_col_w {
            kind_label[..kind_col_w].to_string()
        } else {
            kind_label
        };
        buf.set_string(inner_x, y, &kind_display, kind_st);

        let name_x = inner_x + kind_col_w as u16;
        let name_avail = (inner_w as usize).saturating_sub(kind_col_w);
        let name = if sym.name.len() > name_avail {
            &sym.name[..name_avail]
        } else {
            &sym.name
        };
        buf.set_string(name_x, y, name, row_style);
    }

    if picker.filtered.is_empty() && list_rows > 0 {
        let msg = " No matching symbols";
        buf.set_string(
            inner_x,
            current_y,
            format!("{:<width$}", msg, width = inner_w as usize),
            Style::default()
                .bg(Color::Rgb(25, 25, 40))
                .fg(Color::DarkGray),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::cursor::ByteRange;
    use crate::syntax::Symbol;

    fn make_state(symbols: Vec<Symbol>) -> SymbolPickerState {
        SymbolPickerState::new(symbols)
    }

    #[test]
    fn render_does_not_panic_on_empty_picker() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = TermBuffer::empty(area);
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let state = make_state(Vec::new());
        render(&state, &theme, area, &mut buf);
    }

    #[test]
    fn render_does_not_panic_with_symbols() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = TermBuffer::empty(area);
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let state = make_state(vec![Symbol {
            name: "foo".into(),
            kind: "fn",
            byte_range: ByteRange::new(0, 10),
        }]);
        render(&state, &theme, area, &mut buf);
    }
}
