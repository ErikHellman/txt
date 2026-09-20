//! Render the sidebar file-search overlay (Ctrl+F while the sidebar is
//! focused).
//!
//! A floating picker that matches both files **and** directories. It is
//! anchored over the sidebar when the sidebar is visible; otherwise it falls
//! back to a centered layout like [`super::fuzzy_picker`]. Chrome (fill,
//! rounded border, separator, centred header) comes from
//! [`super::overlay_chrome`] so it stays consistent with the other overlays.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::SidebarSearchState;
use crate::theme::ThemeColors;
use crate::ui::overlay_chrome::{draw_border, draw_h_separator, fill_rect, render_centered_header};
use crate::ui::text_utils::truncate_to_width;

pub fn render(
    picker: &SidebarSearchState,
    theme: &ThemeColors,
    area: Rect,
    sidebar_anchor: Option<Rect>,
    buf: &mut TermBuffer,
) {
    let overlay = overlay_rect(area, sidebar_anchor);

    // ── Styles ────────────────────────────────────────────────────────────────
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
    let dir_style = bg_style.fg(theme.sidebar_dir_fg);
    let file_style = bg_style.fg(theme.sidebar_fg);

    // ── Chrome ────────────────────────────────────────────────────────────────
    fill_rect(buf, overlay, bg_style);
    draw_border(buf, overlay, border_style);

    if overlay.height < 6 || overlay.width < 8 {
        // Too small for the header + query + separator + one result row.
        return;
    }

    let inner_x = overlay.x + 1;
    let inner_w = overlay.width.saturating_sub(2);
    let bot_y = overlay.y + overlay.height.saturating_sub(1);

    // Header
    render_centered_header(buf, overlay, overlay.y + 1, " Search files", header_style);

    // Query input
    let query_prompt = format!(" > {}_", picker.query);
    let query_line = truncate_to_width(&query_prompt, inner_w as usize);
    buf.set_string(inner_x, overlay.y + 2, query_line, query_style);

    // Separator
    draw_h_separator(buf, overlay, overlay.y + 3, border_style);

    // ── Results list ──────────────────────────────────────────────────────────
    let list_top = overlay.y + 4;
    let list_rows = bot_y.saturating_sub(list_top) as usize;

    // Compute scroll offset so the selected row is always in view.
    let scroll = if picker.selected >= list_rows && list_rows > 0 {
        picker.selected - list_rows + 1
    } else {
        0
    };

    for (screen_row, (_, entry_idx)) in picker
        .filtered
        .iter()
        .skip(scroll)
        .take(list_rows)
        .enumerate()
    {
        let y = list_top + screen_row as u16;
        let global_idx = scroll + screen_row;
        let is_selected = global_idx == picker.selected;

        let Some((path, is_dir)) = picker.all_entries.get(*entry_idx) else {
            continue;
        };

        let mut label = format!(" {}", path.to_string_lossy());
        if *is_dir {
            label.push('/');
        }
        let label = truncate_to_width(&label, inner_w as usize);

        let (row_style, label_style) = if is_selected {
            (selected_style, selected_style)
        } else if *is_dir {
            (dir_style, dir_style)
        } else {
            (file_style, file_style)
        };

        // Fill the full row so the selection bar spans the overlay width.
        let blank = " ".repeat(inner_w as usize);
        buf.set_string(inner_x, y, &blank, row_style);
        buf.set_string(inner_x, y, label, label_style);
    }

    // Empty state
    if picker.filtered.is_empty() && list_rows > 0 {
        let msg = truncate_to_width(" No matching files", inner_w as usize);
        buf.set_string(
            inner_x,
            list_top,
            msg,
            Style::default().bg(theme.picker_bg).fg(Color::DarkGray),
        );
    }
}

/// Compute the overlay rect: anchored over the sidebar when it is large
/// enough, otherwise a centered float like the other pickers. The height is
/// always clamped so the overlay stays inside the terminal area.
fn overlay_rect(area: Rect, sidebar_anchor: Option<Rect>) -> Rect {
    let (overlay_w, overlay_x, overlay_y, avail_h) = match sidebar_anchor {
        Some(sa) if sa.width >= 24 && sa.height >= 9 => {
            // Span the sidebar, widening into the editor when it is too
            // narrow to show paths comfortably.
            let w = sa.width.max(34).min(area.width);
            let x = sa.x.min(area.x + area.width.saturating_sub(w));
            let avail = area
                .height
                .saturating_sub(sa.y.saturating_sub(area.y))
                .min(sa.height);
            (w, x, sa.y, avail)
        }
        _ => {
            let w = (area.width * 2 / 3).max(40).min(area.width);
            let x = area.x + (area.width.saturating_sub(w)) / 2;
            let y_off = area.height / 6;
            (w, x, area.y + y_off, area.height.saturating_sub(y_off))
        }
    };
    let h = (avail_h * 2 / 3).max(8).min(avail_h);
    Rect::new(overlay_x, overlay_y, overlay_w, h)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_state() -> SidebarSearchState {
        let mut st = SidebarSearchState {
            query: String::new(),
            all_entries: vec![
                (PathBuf::from("src"), true),
                (PathBuf::from("src/hello.rs"), false),
                (PathBuf::from("src/main.rs"), false),
            ],
            filtered: Vec::new(),
            selected: 0,
        };
        st.update_query("hello".to_string());
        st
    }

    #[test]
    fn render_does_not_panic_on_narrow_widths() {
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        for w in 0..=30u16 {
            for h in 0..=12u16 {
                let area = Rect::new(0, 0, w, h);
                let mut buf = TermBuffer::empty(area);
                render(&make_state(), &theme, area, None, &mut buf);
            }
        }
    }

    #[test]
    fn render_does_not_panic_on_tiny_narrow_anchor() {
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let area = Rect::new(0, 0, 60, 20);
        // Anchors below the 24×9 threshold fall back to the centered layout
        // without panicking.
        for (aw, ah) in [(0u16, 0u16), (1, 1), (23, 8), (23, 9), (24, 8)] {
            let mut buf = TermBuffer::empty(area);
            render(
                &make_state(),
                &theme,
                area,
                Some(Rect::new(0, 0, aw, ah)),
                &mut buf,
            );
        }
    }

    #[test]
    fn render_skips_tiny_area() {
        // With a 1×1 area nothing but the (single-cell) background fill may
        // be written — in particular no border glyphs.
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = TermBuffer::empty(area);
        render(&make_state(), &theme, area, None, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn directories_render_with_dir_color_and_trailing_slash() {
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = TermBuffer::empty(area);
        let st = SidebarSearchState {
            query: "z".into(),
            all_entries: vec![
                (PathBuf::from("zzz"), true),
                (PathBuf::from("main.rs"), false),
            ],
            filtered: vec![(0, 0), (0, 1)],
            // Highlight the file row so the directory row is unselected and
            // shows its own style.
            selected: 1,
        };
        render(&st, &theme, area, None, &mut buf);

        // Centered float for an 80×24 area: x=13, y=4; results start at y=8.
        let (x, y) = (14u16, 8u16);
        let cell = &buf[(x, y)];
        assert_eq!(cell.fg, theme.sidebar_dir_fg);
        // Check the trailing slash: label is " zzz/" starting at inner_x=14.
        assert_eq!(buf[(x + 4, y)].symbol(), "/");
    }

    #[test]
    fn anchored_overlay_sits_over_sidebar() {
        let theme = ThemeColors::for_theme(&crate::config::Theme::Default);
        let area = Rect::new(0, 0, 100, 30);
        let anchor = Rect::new(0, 1, 30, 28);
        let mut buf = TermBuffer::empty(area);
        render(&make_state(), &theme, area, Some(anchor), &mut buf);

        // The rounded border starts at the sidebar's top-left.
        assert_eq!(buf[(0, 1)].symbol(), "╭");
        // Header text is present inside the overlay.
        let header_row: String = (1..30).map(|x| buf[(x, 2)].symbol().to_string()).collect();
        assert!(header_row.contains("Search files"));
    }
}
