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

    for layout in tab_layouts(editor, area) {
        let is_active = layout.index == editor.active_idx;
        let style = if is_active {
            active_style
        } else {
            inactive_style
        };
        let tab = &editor.tabs[layout.index];
        let dot = if tab.buffer.modified { "•" } else { " " };
        let name = tab.display_name();
        let label = format!(" {}{} ", dot, name);
        let visible = &label[..layout.label_byte_len];
        buf.set_string(layout.start_x, area.y, visible, style);

        if layout.has_separator {
            let sep_x = layout.start_x + layout.width;
            buf.set_string(sep_x, area.y, "│", sep_style);
        }
    }
}

/// Geometry of one rendered tab inside the tab strip.
struct TabLayout {
    /// Index of the tab in `editor.tabs`.
    index: usize,
    /// Absolute terminal column where the tab label starts.
    start_x: u16,
    /// Number of terminal columns consumed by the label (excluding the
    /// trailing separator).
    width: u16,
    /// Byte length of the rendered slice of the label string.
    label_byte_len: usize,
    /// `true` if a separator column follows this tab.
    has_separator: bool,
}

/// Lay out the tab strip and yield one `TabLayout` per visible tab.
///
/// Mirrors the iteration in [`render`] so hit-testing stays in sync with
/// what was actually drawn. Tabs that don't fit inside `area` are not
/// included in the result.
fn tab_layouts(editor: &Editor, area: Rect) -> Vec<TabLayout> {
    let mut out = Vec::with_capacity(editor.tabs.len());
    let mut x = area.x;
    let limit = area.x + area.width;
    for (i, tab) in editor.tabs.iter().enumerate() {
        if x >= limit {
            break;
        }
        let dot = if tab.buffer.modified { "•" } else { " " };
        let name = tab.display_name();
        let label = format!(" {}{} ", dot, name);
        let max_w = limit.saturating_sub(x) as usize;
        let label_byte_len = label.len().min(max_w);
        let width = label_byte_len as u16;
        let next_x = x + width;
        let has_separator = i + 1 < editor.tabs.len() && next_x < limit;
        out.push(TabLayout {
            index: i,
            start_x: x,
            width,
            label_byte_len,
            has_separator,
        });
        x = next_x + if has_separator { 1 } else { 0 };
    }
    out
}

/// Return the tab index at the given screen position, or `None` if the
/// point is not on a rendered tab label. Separator columns and empty
/// space past the last tab are treated as misses.
pub fn tab_at(editor: &Editor, area: Rect, col: u16, row: u16) -> Option<usize> {
    if area.height == 0 || row != area.y {
        return None;
    }
    if col < area.x || col >= area.x + area.width {
        return None;
    }
    tab_layouts(editor, area)
        .into_iter()
        .find(|t| col >= t.start_x && col < t.start_x + t.width)
        .map(|t| t.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;
    use crate::editor::tab::BufferHandle;
    use std::path::PathBuf;

    fn editor_with_names(names: &[&str]) -> Editor {
        let mut editor = Editor::new();
        editor.tabs.clear();
        for (i, name) in names.iter().enumerate() {
            let mut handle = BufferHandle::new_empty(i);
            handle.path = Some(PathBuf::from(name));
            editor.tabs.push(handle);
        }
        editor.active_idx = 0;
        editor
    }

    #[test]
    fn tab_at_returns_clicked_index() {
        let editor = editor_with_names(&["a.rs", "bb.rs", "ccc.rs"]);
        let area = Rect::new(0, 0, 80, 1);

        // Labels are formatted as " {dot}{name} ": for an unmodified file
        // that's two leading spaces plus the name plus a trailing space.
        // Tab 0 "  a.rs " spans cols 0..7, separator at col 7.
        assert_eq!(tab_at(&editor, area, 0, 0), Some(0));
        assert_eq!(tab_at(&editor, area, 6, 0), Some(0));
        assert_eq!(tab_at(&editor, area, 7, 0), None);
        // Tab 1 "  bb.rs " spans cols 8..16, separator at col 16.
        assert_eq!(tab_at(&editor, area, 8, 0), Some(1));
        assert_eq!(tab_at(&editor, area, 15, 0), Some(1));
        assert_eq!(tab_at(&editor, area, 16, 0), None);
        // Tab 2 "  ccc.rs " spans cols 17..26 (last tab, no trailing sep).
        assert_eq!(tab_at(&editor, area, 17, 0), Some(2));
        assert_eq!(tab_at(&editor, area, 25, 0), Some(2));
        // Past the last tab: empty space, no hit.
        assert_eq!(tab_at(&editor, area, 30, 0), None);
    }

    #[test]
    fn tab_at_ignores_wrong_row() {
        let editor = editor_with_names(&["a.rs", "b.rs"]);
        let area = Rect::new(0, 0, 80, 1);
        assert_eq!(tab_at(&editor, area, 0, 1), None);
    }

    #[test]
    fn tab_at_respects_area_offset() {
        let editor = editor_with_names(&["a.rs", "b.rs"]);
        let area = Rect::new(10, 3, 80, 1);
        // Outside on the left.
        assert_eq!(tab_at(&editor, area, 9, 3), None);
        // First tab starts at col 10.
        assert_eq!(tab_at(&editor, area, 10, 3), Some(0));
        // Wrong row.
        assert_eq!(tab_at(&editor, area, 10, 2), None);
    }

    #[test]
    fn tab_at_truncated_when_area_too_narrow() {
        let editor = editor_with_names(&["a.rs", "b.rs", "c.rs"]);
        // Only enough room for the first label "  a.rs " (7 cols).
        let area = Rect::new(0, 0, 7, 1);
        assert_eq!(tab_at(&editor, area, 0, 0), Some(0));
        assert_eq!(tab_at(&editor, area, 6, 0), Some(0));
        // Column 7 is outside the area.
        assert_eq!(tab_at(&editor, area, 7, 0), None);
    }
}
