pub mod changelog_overlay;
pub mod command_palette;
pub mod completion_popup;
pub mod editor_view;
pub mod fuzzy_picker;
pub mod git_dialog;
pub mod help_overlay;
pub mod hover_popup;
pub mod lsp_approval;
pub mod lsp_picker;
pub mod overlay_chrome;
pub mod project_search;
pub mod references_list;
pub mod search_bar;
pub mod settings_overlay;
pub mod sidebar;
pub mod status_bar;
pub mod sticky_header;
pub mod symbol_picker;
pub mod tab_bar;
pub mod welcome_overlay;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
};

use crate::app::{AppState, ConfirmDelete};
use crate::theme::ThemeColors;

/// Top-level render function. Called once per frame. Stores the rendered
/// sidebar `Rect` on `state` so the next mouse event can hit-test against it.
pub fn render(state: &mut AppState, frame: &mut Frame) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    let theme = ThemeColors::for_theme(&state.config.theme);

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
    let sidebar_total_w = state.sidebar_width + 1; // +1 for separator
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
    // Store the rendered sidebar rect (or `None`) so mouse-event handlers in
    // the next `update()` call can hit-test against it.
    state.sidebar_area = sidebar_area;

    // ── Compute syntax highlights for visible range ───────────────────────────
    // Prefer LSP semantic tokens when available; fall back to tree-sitter.
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

            // Use semantic tokens if available from LSP; otherwise tree-sitter.
            if let Some(tokens) = &handle.lsp_state.semantic_tokens {
                use crate::syntax::highlighter::semantic_tokens_to_highlights;
                semantic_tokens_to_highlights(tokens, start_byte, end_byte)
            } else {
                let source = handle.buffer.to_string();
                handle
                    .syntax
                    .highlight_spans(source.as_bytes(), start_byte, end_byte)
            }
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
            sidebar::render(
                sidebar,
                state.sidebar_clipboard.as_ref(),
                state.sidebar_focused,
                &theme,
                sb_inner,
                buf,
            );
        }
        let sep_x = side_a.x + side_a.width.saturating_sub(1);
        let sep_style = Style::default()
            .bg(Color::Rgb(20, 20, 35))
            .fg(Color::Rgb(60, 60, 80));
        for y in side_a.y..side_a.y + side_a.height {
            buf.set_string(sep_x, y, "│", sep_style);
        }
    }

    let editor_focused = !state.sidebar_focused
        && state.fuzzy_picker.is_none()
        && state.symbol_picker.is_none()
        && state.command_palette.is_none()
        && !state.show_help
        && !state.show_settings
        && !state.show_welcome
        && !state.show_changelog;

    // Compute the sticky header path for the active buffer's cursor and, when
    // it's non-empty and there's room, reserve the first row of the editor
    // pane for it.
    let sticky_path = if state.config.sticky_header && editor_area.height >= 3 {
        let cursor_byte = handle.buffer.cursors.primary().byte_offset;
        handle
            .syntax
            .enclosing_named_path(handle.buffer.rope(), cursor_byte)
    } else {
        Vec::new()
    };
    let (header_area, editor_area) = if !sticky_path.is_empty() {
        let header = Rect::new(editor_area.x, editor_area.y, editor_area.width, 1);
        let body = Rect::new(
            editor_area.x,
            editor_area.y + 1,
            editor_area.width,
            editor_area.height.saturating_sub(1),
        );
        (Some(header), body)
    } else {
        (None, editor_area)
    };

    editor_view::render(
        handle,
        state.search_state.as_ref(),
        &highlight_spans,
        state.git_gutter.as_ref(),
        editor_focused,
        state.config.show_whitespace,
        state.config.tab_size,
        state.config.indent_guides,
        &state.config.rulers,
        &theme,
        editor_area,
        buf,
    );

    if let Some(ha) = header_area {
        sticky_header::render(&sticky_path, ha, buf);
    }

    if let Some(sa) = search_area_opt
        && let Some(ss) = &state.search_state
    {
        search_bar::render(ss, sa, buf);
    }

    status_bar::render(state, &theme, status_area, buf);

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

    // ── Confirm-delete overlay (replaces status bar) ─────────────────────────
    if let Some(cd) = &state.confirm_delete {
        let prompt_style = Style::default()
            .bg(Color::Rgb(180, 40, 40))
            .fg(Color::White);
        let msg = match cd {
            ConfirmDelete::File(p) => {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                format!(" Delete \"{}\"? (y/n) ", name)
            }
            ConfirmDelete::Dir(p) => {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                format!(" Delete directory \"{}\" and all contents? (y/n) ", name)
            }
            ConfirmDelete::DirConfirmed(p) => {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                format!(" Are you sure? Press Enter to delete \"{}\" ", name)
            }
        };
        for x in status_area.x..status_area.x + status_area.width {
            buf.set_string(x, status_area.y, " ", prompt_style);
        }
        let msg_len = msg.len().min(status_area.width as usize);
        buf.set_string(status_area.x, status_area.y, &msg[..msg_len], prompt_style);
    }

    // ── Fuzzy picker floating overlay ─────────────────────────────────────────
    if let Some(picker) = &state.fuzzy_picker {
        fuzzy_picker::render(picker, &theme, area, buf);
    }

    // ── Symbols-in-file picker overlay ────────────────────────────────────────
    if let Some(picker) = &state.symbol_picker {
        symbol_picker::render(picker, &theme, area, buf);
    }

    // ── Command palette overlay ───────────────────────────────────────────────
    if let Some(palette) = &state.command_palette {
        command_palette::render(palette, &theme, area, buf);
    }

    // ── Project search overlay ────────────────────────────────────────────────
    if let Some(ps) = &state.project_search {
        project_search::render(ps, &theme, area, buf);
    }

    // ── Welcome overlay (first launch) ───────────────────────────────────────
    if state.show_welcome {
        welcome_overlay::render(area, buf, state.welcome_scroll, env!("CARGO_PKG_VERSION"));
    }

    // ── Changelog overlay (post-upgrade) ─────────────────────────────────────
    if state.show_changelog {
        changelog_overlay::render(area, buf, state.changelog_scroll, &state.changelog_sections);
    }

    // ── Help overlay ─────────────────────────────────────────────────────────
    if state.show_help {
        help_overlay::render(area, buf, state.help_scroll, state.input.keybindings());
    }

    // ── Settings overlay ──────────────────────────────────────────────────────
    if state.show_settings {
        settings_overlay::render(state, area, buf);
    }

    // ── LSP picker overlay ───────────────────────────────────────────────────
    if let Some(picker) = &state.lsp_picker {
        lsp_picker::render(picker, area, buf);
    }

    // ── Git operations dialog ────────────────────────────────────────────────
    if let Some(dialog) = &state.git_dialog {
        git_dialog::render(dialog, &theme, area, buf);
    }

    // ── LSP-binary trust approval overlay (security-critical, render last) ───
    if let Some(pending) = &state.pending_lsp_approval {
        lsp_approval::render(pending, area, buf);
    }

    // ── Completion popup ─────────────────────────────────────────────────────
    if let Some(comp) = &state.completion {
        let cursor = handle.buffer.cursors.primary();
        let cursor_row =
            editor_area.y + cursor.line.saturating_sub(handle.viewport.scroll_row) as u16;
        let cursor_col = editor_area.x + cursor.col as u16;
        completion_popup::render(comp, cursor_row, cursor_col, area, buf);
    }

    // ── Hover popup ──────────────────────────────────────────────────────────
    if let Some(hover) = &state.hover {
        let cursor = handle.buffer.cursors.primary();
        let cursor_row =
            editor_area.y + cursor.line.saturating_sub(handle.viewport.scroll_row) as u16;
        let cursor_col = editor_area.x + cursor.col as u16;
        hover_popup::render(hover, cursor_row, cursor_col, area, buf);
    }

    // ── References list overlay ──────────────────────────────────────────────
    if let Some(refs) = &state.references_list {
        references_list::render(refs, area, buf);
    }
}
