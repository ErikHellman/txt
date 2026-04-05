use std::path::Path;

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::{SidebarClipboard, SidebarState};
use crate::theme::ThemeColors;

/// Render the file tree sidebar.
/// `focused` controls whether the header is highlighted to indicate keyboard focus.
/// `clipboard` is used to show italic on cut entries.
pub fn render(
    sidebar: &SidebarState,
    clipboard: Option<&SidebarClipboard>,
    focused: bool,
    theme: &ThemeColors,
    area: Rect,
    buf: &mut TermBuffer,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let root_style = if focused {
        Style::default()
            .bg(Color::Rgb(60, 80, 160))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(theme.sidebar_bg)
            .fg(Color::Rgb(180, 180, 200))
            .add_modifier(Modifier::BOLD)
    };
    let selected_style = Style::default().bg(theme.sidebar_sel_bg).fg(Color::White);
    let dir_style = Style::default()
        .bg(theme.sidebar_bg)
        .fg(theme.sidebar_dir_fg);
    let file_style = Style::default().bg(theme.sidebar_bg).fg(theme.sidebar_fg);

    let visible_rows = area.height as usize;

    // Compute scroll offset so the selected entry is always visible.
    let scroll = if sidebar.selected >= visible_rows {
        sidebar.selected - visible_rows + 1
    } else {
        0
    };

    for (screen_row, entry) in sidebar
        .entries
        .iter()
        .skip(scroll)
        .take(visible_rows)
        .enumerate()
    {
        let global_idx = scroll + screen_row;
        let y = area.y + screen_row as u16;
        let is_selected = global_idx == sidebar.selected;
        let is_root = entry.path == sidebar.root;

        let is_cut = clipboard
            .map(|c| c.is_cut && c.path == entry.path)
            .unwrap_or(false);

        let base_style = if is_root {
            // Root node always uses the header style.
            if is_selected {
                root_style.bg(Color::Rgb(60, 60, 100))
            } else {
                root_style
            }
        } else if is_selected {
            if is_cut {
                selected_style.add_modifier(Modifier::ITALIC)
            } else {
                selected_style
            }
        } else if entry.is_dir {
            if is_cut {
                dir_style.add_modifier(Modifier::ITALIC)
            } else {
                dir_style
            }
        } else if is_cut {
            file_style.add_modifier(Modifier::ITALIC)
        } else {
            file_style
        };

        // Fill the entire row.
        let blank = format!("{:<width$}", "", width = area.width as usize);
        buf.set_string(area.x, y, &blank, base_style);

        // Build the label with indentation.
        let indent = "  ".repeat(entry.depth);
        let icon = if entry.is_dir {
            // Root is always expanded; other dirs show expand/collapse state.
            if is_root || entry.expanded {
                "▾ "
            } else {
                "▸ "
            }
        } else {
            "  "
        };
        let name = if is_root {
            // Show the root directory name (last path component).
            entry
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("/")
        } else {
            entry
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
        };
        let label = format!(" {}{}{}", indent, icon, name);

        let max_w = area.width as usize;
        let label_w = label.len().min(max_w);
        buf.set_string(area.x, y, &label[..label_w], base_style);
    }
}

/// Choose the best header text for the given root path and column budget.
///
/// Priority:
/// 1. Full path with a leading space: " /home/user/project"
/// 2. Directory name only:            " project"
/// 3. Directory name truncated with ellipsis: " projec…"
#[allow(dead_code)]
fn path_header(root: &Path, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }

    // 1. Try the full path.
    let full = format!(" {}", root.display());
    if full.chars().count() <= max_cols {
        return full;
    }

    // 2. Try just the last component (directory name).
    let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("/");
    let short = format!(" {}", name);
    if short.chars().count() <= max_cols {
        return short;
    }

    // 3. Truncate the directory name with an ellipsis, keeping the leading space.
    // Reserve 1 column for '…'.
    if max_cols <= 1 {
        return " ".to_string();
    }
    let truncated: String = short.chars().take(max_cols - 1).collect();
    format!("{}…", truncated)
}
