pub mod command_palette;
pub mod editor_view;
pub mod fuzzy_picker;
pub mod help_overlay;
pub mod search_bar;
pub mod sidebar;
pub mod status_bar;
pub mod tab_bar;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
};

use crate::app::{AppState, SIDEBAR_WIDTH};

/// Top-level render function. Called once per frame with an immutable reference
/// to the application state. Builds the layout and delegates to sub-renderers.
pub fn render(state: &AppState, frame: &mut Frame) {
    let area = frame.area();
    let buf = frame.buffer_mut();

    // ── Reserve status bar (1 row at very bottom) ─────────────────────────────
    let status_y = area.y + area.height.saturating_sub(1);
    let status_area = Rect::new(area.x, status_y, area.width, 1.min(area.height));
    let above_status = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));

    // ── Reserve search bar (above status bar, when active) ────────────────────
    let search_h = state
        .search_state
        .as_ref()
        .map(|s| s.bar_height())
        .unwrap_or(0);
    let (search_area_opt, content_area) = if search_h > 0 && above_status.height > search_h {
        let search_y = above_status.y + above_status.height.saturating_sub(search_h);
        let sa = Rect::new(above_status.x, search_y, above_status.width, search_h);
        let ca = Rect::new(
            above_status.x,
            above_status.y,
            above_status.width,
            above_status.height.saturating_sub(search_h),
        );
        (Some(sa), ca)
    } else {
        (None, above_status)
    };

    // ── Optional tab bar (1 row at top) ───────────────────────────────────────
    let show_tabs = state.editor.tab_count() > 1;
    let (tab_area, editor_content_area) = if show_tabs && content_area.height >= 1 {
        let tab_a = Rect::new(content_area.x, content_area.y, content_area.width, 1);
        let rest = Rect::new(
            content_area.x,
            content_area.y + 1,
            content_area.width,
            content_area.height.saturating_sub(1),
        );
        (Some(tab_a), rest)
    } else {
        (None, content_area)
    };

    // ── Optional sidebar (left panel) ─────────────────────────────────────────
    let sidebar_total_w = SIDEBAR_WIDTH + 1; // +1 for separator
    let (sidebar_area, editor_area) =
        if state.sidebar.is_some() && editor_content_area.width > sidebar_total_w {
            let side = Rect::new(
                editor_content_area.x,
                editor_content_area.y,
                sidebar_total_w,
                editor_content_area.height,
            );
            let ed = Rect::new(
                editor_content_area.x + sidebar_total_w,
                editor_content_area.y,
                editor_content_area.width.saturating_sub(sidebar_total_w),
                editor_content_area.height,
            );
            (Some(side), ed)
        } else {
            (None, editor_content_area)
        };

    // ── Compute syntax highlights for visible range ───────────────────────────
    let handle = state.editor.active();
    let highlight_spans = if editor_area.height > 0 {
        let visible_start = handle.viewport.scroll_row;
        let visible_end =
            (visible_start + editor_area.height as usize).min(handle.buffer.len_lines());
        if visible_start < visible_end {
            let start_byte = handle
                .buffer
                .rope()
                .char_to_byte(handle.buffer.rope().line_to_char(visible_start));
            let end_line = visible_end.min(handle.buffer.len_lines());
            let end_byte = if end_line >= handle.buffer.len_lines() {
                handle.buffer.rope().len_bytes()
            } else {
                handle
                    .buffer
                    .rope()
                    .char_to_byte(handle.buffer.rope().line_to_char(end_line))
            };
            let source = handle.buffer.to_string();
            handle
                .syntax
                .highlight_spans(source.as_bytes(), start_byte, end_byte)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // ── Render panels ─────────────────────────────────────────────────────────

    if let Some(tab_a) = tab_area {
        tab_bar::render(&state.editor, tab_a, buf);
    }

    if let Some(side_a) = sidebar_area {
        let sb_inner = Rect::new(
            side_a.x,
            side_a.y,
            side_a.width.saturating_sub(1),
            side_a.height,
        );
        if let Some(sidebar) = &state.sidebar {
            sidebar::render(sidebar, state.sidebar_focused, sb_inner, buf);
        }
        let sep_x = side_a.x + side_a.width.saturating_sub(1);
        let sep_style = Style::default()
            .bg(Color::Rgb(20, 20, 35))
            .fg(Color::Rgb(60, 60, 80));
        for y in side_a.y..side_a.y + side_a.height {
            buf.set_string(sep_x, y, "│", sep_style);
        }
    }

    editor_view::render(
        handle,
        state.search_state.as_ref(),
        &highlight_spans,
        state.git_gutter.as_ref(),
        editor_area,
        buf,
    );

    if let Some(sa) = search_area_opt
        && let Some(ss) = &state.search_state
    {
        search_bar::render(ss, sa, buf);
    }

    status_bar::render(state, status_area, buf);

    // ── Confirm-quit overlay (replaces status bar) ────────────────────────────
    if state.confirm_quit {
        let prompt_style = Style::default()
            .bg(Color::Rgb(180, 40, 40))
            .fg(Color::White);
        let msg = " Unsaved changes. Quit anyway? (y/n) ";
        for x in status_area.x..status_area.x + status_area.width {
            buf.set_string(x, status_area.y, " ", prompt_style);
        }
        let msg_len = msg.len().min(status_area.width as usize);
        buf.set_string(status_area.x, status_area.y, &msg[..msg_len], prompt_style);
    }

    // ── Fuzzy picker floating overlay ─────────────────────────────────────────
    if let Some(picker) = &state.fuzzy_picker {
        fuzzy_picker::render(picker, area, buf);
    }

    // ── Command palette overlay ───────────────────────────────────────────────
    if let Some(palette) = &state.command_palette {
        command_palette::render(palette, area, buf);
    }

    // ── Help overlay ─────────────────────────────────────────────────────────
    if state.show_help {
        help_overlay::render(area, buf, state.help_scroll);
    }
}
