use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::{AppState, InputMode};

/// Render the status bar at the bottom of the screen.
///
/// Normal layout:
///   [ filename [+]  lang ]  ...spacer...  [ line:col  UTF-8 ]
///
/// Modal layout:
///   [ JumpToLine / OpenFile / SaveAs prompt ]
pub fn render(state: &AppState, area: Rect, buf: &mut TermBuffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    // Show prompt when a modal input is active.
    if let Some(prompt) = modal_prompt(&state.input_mode) {
        let prompt_style = Style::default()
            .bg(Color::Rgb(20, 60, 100))
            .fg(Color::White);
        for x in area.x..area.x + area.width {
            buf.set_string(x, area.y, " ", prompt_style);
        }
        let truncated = truncate_str(&prompt, area.width as usize);
        buf.set_string(area.x, area.y, &truncated, prompt_style);
        return;
    }

    let handle = state.editor.active();

    let bar_style = Style::default().bg(Color::Rgb(40, 40, 60)).fg(Color::White);
    let name_style = Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let modified_style = Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .fg(Color::Rgb(255, 150, 50))
        .add_modifier(Modifier::BOLD);
    let lang_style = Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .fg(Color::Rgb(140, 180, 140));
    let info_style = Style::default()
        .bg(Color::Rgb(60, 60, 80))
        .fg(Color::Rgb(200, 200, 220));

    // Fill the entire status bar row with the background colour.
    for x in area.x..area.x + area.width {
        buf.set_string(x, area.y, " ", bar_style);
    }

    // Mode badge — leftmost element, shows the active pane/overlay.
    let (mode_label, mode_bg) = active_mode(state);
    let mode_style = Style::default()
        .bg(mode_bg)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    buf.set_string(area.x, area.y, mode_label, mode_style);
    let mode_width = mode_label.len() as u16;

    let cursor = handle.buffer.cursors.primary();
    let line = cursor.line + 1; // 1-based for display
    let col = cursor.col + 1;

    let name = handle.display_name();
    let modified_str = if handle.buffer.modified { " [+]" } else { "" };
    let lang = handle.syntax.language.name();
    let pos = format!(" {}:{} ", line, col);
    let enc = " UTF-8  F1:Help ";
    let wrap_flag = if handle.viewport.word_wrap { " WW" } else { "" };

    // Right side: word-wrap flag + language + position + encoding
    let right = format!(
        "{}{}  {}{}",
        wrap_flag,
        if !lang.is_empty() {
            format!(" {}", lang)
        } else {
            String::new()
        },
        pos,
        enc
    );
    let width = area.width as usize;

    // Render right side first (rightmost block)
    let right_x = area.x + (width.saturating_sub(right.len())) as u16;
    buf.set_string(right_x, area.y, &right, info_style);

    // Left side: " filename" — starts after the mode badge
    let left_start = area.x + mode_width;
    let left_available = (right_x as usize).saturating_sub(left_start as usize);
    let name_part = format!(" {}", name);
    let name_end = name_part.len().min(left_available);
    if name_end > 0 {
        buf.set_string(left_start, area.y, &name_part[..name_end], name_style);
    }

    // Modified flag after name
    let modified_x = left_start + name_end as u16;
    let modified_available = (right_x as usize).saturating_sub(modified_x as usize);
    if !modified_str.is_empty() && modified_available > 0 {
        let m = truncate_str(modified_str, modified_available);
        buf.set_string(modified_x, area.y, &m, modified_style);
    }

    // Language name after modified flag
    if !modified_str.is_empty() {
        let lang_x = modified_x + modified_str.len().min(modified_available) as u16;
        let lang_available = (right_x as usize).saturating_sub(lang_x as usize);
        if lang_available > 2 && lang != "plain" {
            let lang_label = format!("  {}", lang);
            let l = truncate_str(&lang_label, lang_available);
            buf.set_string(lang_x, area.y, &l, lang_style);
        }
    } else if lang != "plain" {
        let lang_x = modified_x;
        let lang_available = (right_x as usize).saturating_sub(lang_x as usize);
        if lang_available > 2 {
            let lang_label = format!("  {}", lang);
            let l = truncate_str(&lang_label, lang_available);
            buf.set_string(lang_x, area.y, &l, lang_style);
        }
    }
}

fn active_mode(state: &AppState) -> (&'static str, Color) {
    if state.sidebar_focused {
        (" SIDEBAR ", Color::Rgb(30, 100, 120))
    } else if state.show_help {
        (" HELP ", Color::Rgb(70, 50, 130))
    } else if state.show_settings {
        (" SETTINGS ", Color::Rgb(120, 85, 30))
    } else if state.fuzzy_picker.is_some() {
        (" PICKER ", Color::Rgb(30, 90, 130))
    } else if state.command_palette.is_some() {
        (" PALETTE ", Color::Rgb(90, 50, 130))
    } else {
        (" EDITOR ", Color::Rgb(40, 110, 60))
    }
}

fn modal_prompt(mode: &InputMode) -> Option<String> {
    match mode {
        InputMode::Normal => None,
        InputMode::JumpToLine(s) => Some(format!(" Go to [line:col]: {}_", s)),
        InputMode::OpenFilePath(s) => Some(format!(" Open: {}_", s)),
        InputMode::SaveAsPath(s) => Some(format!(" Save as: {}_", s)),
        InputMode::RenamePath(_, s) => Some(format!(" Rename: {}_", s)),
        InputMode::NewFolderName(_, s) => Some(format!(" New folder: {}_", s)),
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        // Truncate at a char boundary.
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}
