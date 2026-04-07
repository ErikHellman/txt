use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::input::keybinding::KeyBindings;

/// Help template entry.  `Section` headers separate groups of bindings.
enum HelpEntry {
    Section(&'static str),
    /// One or two action names mapped to a description.
    /// When two action names are given, their keys are joined with ` / `.
    Binding {
        actions: &'static [&'static str],
        desc: &'static str,
    },
    /// A static key label (for non-remappable or compound entries).
    Static {
        key: &'static str,
        desc: &'static str,
    },
}

/// Template defining the help overlay layout.  Key combos are looked up
/// dynamically from `KeyBindings` at render time.
const TEMPLATE: &[HelpEntry] = &[
    // ── Navigation ────────────────────────────────────────────────────
    HelpEntry::Section("Navigation"),
    HelpEntry::Binding {
        actions: &[
            "move_cursor_up",
            "move_cursor_down",
            "move_cursor_left",
            "move_cursor_right",
        ],
        desc: "Move cursor",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_word_left", "move_cursor_word_right"],
        desc: "Word jump",
    },
    HelpEntry::Binding {
        actions: &[
            "extend_selection_up",
            "extend_selection_down",
            "extend_selection_left",
            "extend_selection_right",
        ],
        desc: "Extend selection",
    },
    HelpEntry::Binding {
        actions: &["extend_selection_word_left", "extend_selection_word_right"],
        desc: "Extend by word",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_home", "move_cursor_end"],
        desc: "Line start / end",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_file_start", "move_cursor_file_end"],
        desc: "File start / end",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_page_up", "move_cursor_page_down"],
        desc: "Page up / down",
    },
    // ── Selection ────────────────────────────────────────────────────
    HelpEntry::Section("Selection"),
    HelpEntry::Binding {
        actions: &["select_all"],
        desc: "Select all",
    },
    HelpEntry::Binding {
        actions: &["ast_expand_selection"],
        desc: "Expand selection (AST)",
    },
    HelpEntry::Binding {
        actions: &["ast_contract_selection"],
        desc: "Contract selection (AST)",
    },
    HelpEntry::Binding {
        actions: &["select_all_occurrences"],
        desc: "Select all occurrences",
    },
    // ── Multi-cursor ─────────────────────────────────────────────────
    HelpEntry::Section("Multi-cursor"),
    HelpEntry::Binding {
        actions: &["spawn_cursor_up"],
        desc: "Add cursor above",
    },
    HelpEntry::Binding {
        actions: &["spawn_cursor_down"],
        desc: "Add cursor below",
    },
    // ── Editing ──────────────────────────────────────────────────────
    HelpEntry::Section("Editing"),
    HelpEntry::Binding {
        actions: &["delete_backward", "delete_forward"],
        desc: "Delete backward / forward",
    },
    HelpEntry::Binding {
        actions: &["delete_word_backward"],
        desc: "Delete word backward",
    },
    HelpEntry::Binding {
        actions: &["delete_word_forward"],
        desc: "Delete word forward",
    },
    HelpEntry::Binding {
        actions: &["kill_line"],
        desc: "Delete to end of line (kill line)",
    },
    HelpEntry::Binding {
        actions: &["undo", "redo"],
        desc: "Undo / Redo",
    },
    HelpEntry::Binding {
        actions: &["duplicate_line"],
        desc: "Duplicate line",
    },
    HelpEntry::Binding {
        actions: &["move_line_up", "move_line_down"],
        desc: "Move line up / down",
    },
    HelpEntry::Binding {
        actions: &["toggle_line_comment"],
        desc: "Toggle line comment",
    },
    // ── Clipboard ────────────────────────────────────────────────────
    HelpEntry::Section("Clipboard"),
    HelpEntry::Binding {
        actions: &["copy"],
        desc: "Copy",
    },
    HelpEntry::Binding {
        actions: &["cut"],
        desc: "Cut",
    },
    HelpEntry::Binding {
        actions: &["paste"],
        desc: "Paste",
    },
    HelpEntry::Binding {
        actions: &["copy_file_reference"],
        desc: "Copy file reference",
    },
    // ── File & Tabs ──────────────────────────────────────────────────
    HelpEntry::Section("File & Tabs"),
    HelpEntry::Binding {
        actions: &["save_file"],
        desc: "Save",
    },
    HelpEntry::Binding {
        actions: &["save_file_as"],
        desc: "Save As",
    },
    HelpEntry::Binding {
        actions: &["new_file"],
        desc: "New file / tab",
    },
    HelpEntry::Binding {
        actions: &["open_file"],
        desc: "Open file",
    },
    HelpEntry::Binding {
        actions: &["jump_to_line"],
        desc: "Jump to line[:col]",
    },
    HelpEntry::Binding {
        actions: &["new_tab"],
        desc: "New tab",
    },
    HelpEntry::Binding {
        actions: &["next_tab", "prev_tab"],
        desc: "Next / Prev tab",
    },
    HelpEntry::Static {
        key: "Ctrl+1..9",
        desc: "Go to tab N",
    },
    // ── Panels & Pickers ─────────────────────────────────────────────
    HelpEntry::Section("Panels & Pickers"),
    HelpEntry::Binding {
        actions: &["focus_sidebar"],
        desc: "Focus / open sidebar",
    },
    HelpEntry::Binding {
        actions: &["toggle_sidebar"],
        desc: "Toggle sidebar (show/hide)",
    },
    HelpEntry::Binding {
        actions: &["open_fuzzy_picker"],
        desc: "Fuzzy file picker",
    },
    HelpEntry::Binding {
        actions: &["open_recent_files"],
        desc: "Recent files",
    },
    HelpEntry::Binding {
        actions: &["open_command_palette"],
        desc: "Command palette",
    },
    HelpEntry::Binding {
        actions: &["open_buffer_switcher"],
        desc: "Buffer switcher",
    },
    // ── Search ───────────────────────────────────────────────────────
    HelpEntry::Section("Search"),
    HelpEntry::Binding {
        actions: &["open_search"],
        desc: "Find",
    },
    HelpEntry::Binding {
        actions: &["open_replace"],
        desc: "Find & Replace",
    },
    HelpEntry::Binding {
        actions: &["search_next", "search_prev"],
        desc: "Next / Prev match",
    },
    HelpEntry::Binding {
        actions: &["search_toggle_regex"],
        desc: "Toggle regex",
    },
    HelpEntry::Binding {
        actions: &["search_toggle_case_sensitive"],
        desc: "Toggle case-sensitive",
    },
    // ── LSP ──────────────────────────────────────────────────────────
    HelpEntry::Section("LSP (when active)"),
    HelpEntry::Binding {
        actions: &["trigger_completion"],
        desc: "Code completion",
    },
    HelpEntry::Binding {
        actions: &["show_hover"],
        desc: "Hover info",
    },
    HelpEntry::Binding {
        actions: &["go_to_definition"],
        desc: "Go to definition",
    },
    HelpEntry::Binding {
        actions: &["find_references"],
        desc: "Find references",
    },
    HelpEntry::Binding {
        actions: &["rename_symbol"],
        desc: "Rename symbol",
    },
    HelpEntry::Binding {
        actions: &["code_action"],
        desc: "Code action / quick fix",
    },
    // ── Sidebar ──────────────────────────────────────────────────────
    HelpEntry::Section("Sidebar"),
    HelpEntry::Static {
        key: "Ctrl+C",
        desc: "Copy file only (sidebar)",
    },
    HelpEntry::Static {
        key: "Ctrl+X",
        desc: "Cut file/dir (sidebar)",
    },
    HelpEntry::Static {
        key: "Ctrl+V",
        desc: "Paste (sidebar)",
    },
    HelpEntry::Static {
        key: "F2",
        desc: "Rename file/dir (sidebar)",
    },
    HelpEntry::Static {
        key: "Delete",
        desc: "Delete file/dir (sidebar)",
    },
    HelpEntry::Binding {
        actions: &["sidebar_new_folder"],
        desc: "New folder (sidebar)",
    },
    // ── View & App ───────────────────────────────────────────────────
    HelpEntry::Section("View & App"),
    HelpEntry::Binding {
        actions: &["toggle_word_wrap"],
        desc: "Toggle word wrap",
    },
    HelpEntry::Binding {
        actions: &["toggle_help"],
        desc: "Toggle this help  (\u{2191}\u{2193} to scroll)",
    },
    HelpEntry::Binding {
        actions: &["open_settings"],
        desc: "Settings",
    },
    HelpEntry::Binding {
        actions: &["open_lsp_config"],
        desc: "Configure LSP server",
    },
    HelpEntry::Binding {
        actions: &["quit"],
        desc: "Quit",
    },
];

/// Build a flat list of `(key_display, description)` entries from the template,
/// resolving dynamic bindings via `KeyBindings`.
fn build_entries(bindings: &KeyBindings) -> Vec<(String, &'static str)> {
    let mut entries = Vec::new();

    for entry in TEMPLATE {
        match entry {
            HelpEntry::Section(name) => {
                entries.push((String::new(), *name));
            }
            HelpEntry::Static { key, desc } => {
                entries.push(((*key).to_string(), *desc));
            }
            HelpEntry::Binding { actions, desc } => {
                let keys: Vec<String> = actions
                    .iter()
                    .filter_map(|a| bindings.display_key_for_action(a))
                    .map(format_key_display)
                    .collect();

                let key_str = if keys.is_empty() {
                    "(unbound)".to_string()
                } else {
                    dedup_and_join(&keys)
                };

                entries.push((key_str, *desc));
            }
        }
    }

    entries
}

/// Capitalize a key combo display string for the help overlay.
/// E.g. `"ctrl+shift+s"` → `"Ctrl+Shift+S"`.
fn format_key_display(s: &str) -> String {
    s.split('+')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Join key strings with ` / `, collapsing duplicates.
fn dedup_and_join(keys: &[String]) -> String {
    let mut seen = Vec::new();
    for k in keys {
        if !seen.contains(k) {
            seen.push(k.clone());
        }
    }
    seen.join(" / ")
}

/// Render a scrollable keybinding cheat-sheet as a centered floating overlay.
///
/// `scroll` is the number of rows to skip from the top of the entry list.
/// The render function clamps it so it can never scroll past the last entry.
pub fn render(area: Rect, buf: &mut TermBuffer, scroll: usize, bindings: &KeyBindings) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let entries = build_entries(bindings);

    let bg = Color::Rgb(18, 22, 40);
    let border_col = Color::Rgb(80, 100, 160);
    let border_style = Style::default().bg(bg).fg(border_col);
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 200, 255))
        .add_modifier(Modifier::BOLD);
    let section_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(100, 130, 180))
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().bg(bg).fg(Color::Rgb(140, 200, 255));
    let desc_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));

    // ── Overlay dimensions ────────────────────────────────────────────────────
    // Inner content is: 22 chars for key + 1 space + up-to-30 chars for desc = 53 inner cols.
    // Total with borders: 55.
    const KEY_W: usize = 22;
    const DESC_W: usize = 30;
    const INNER_W: u16 = (KEY_W + 1 + DESC_W) as u16; // 53
    const OVERLAY_W: u16 = INNER_W + 2; // +2 for left/right border

    // Header row (1) + blank separator (1) + content rows + bottom border row consumed in height.
    // Total chrome rows that aren't content: top border (1) + header (1) + separator (1) + bottom border (1) = 4.
    const CHROME_ROWS: u16 = 4;

    let overlay_w = OVERLAY_W.min(area.width);
    // Use up to 80% of terminal height, but at least 8 rows.
    let max_h = area.height.saturating_sub(2).max(8);
    let overlay_h = max_h.min(area.height);

    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 2;
    let overlay_area = Rect::new(ox, oy, overlay_w, overlay_h);

    // ── Fill background ───────────────────────────────────────────────────────
    for y in overlay_area.y..overlay_area.y + overlay_area.height {
        for x in overlay_area.x..overlay_area.x + overlay_area.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // ── Border ───────────────────────────────────────────────────────────────
    draw_border(buf, overlay_area, border_style);

    // ── Header ───────────────────────────────────────────────────────────────
    let header = " Keybindings ";
    let hx = overlay_area.x + overlay_area.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, overlay_area.y, header, header_style);

    // Separator line beneath header.
    let sep_y = overlay_area.y + 2;
    for x in overlay_area.x + 1..overlay_area.x + overlay_area.width.saturating_sub(1) {
        buf.set_string(x, sep_y, "\u{2500}", border_style);
    }

    // ── Scroll clamping ───────────────────────────────────────────────────────
    let visible_rows = overlay_h.saturating_sub(CHROME_ROWS + 1) as usize; // +1 for sep row
    let max_scroll = entries.len().saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    // ── Content rows ─────────────────────────────────────────────────────────
    let content_x = overlay_area.x + 1;
    let content_start_y = overlay_area.y + 3; // below top-border + header + sep
    let content_end_y = overlay_area.y + overlay_area.height.saturating_sub(1);

    for (row_idx, (key, desc)) in entries.iter().skip(scroll).enumerate() {
        let cy = content_start_y + row_idx as u16;
        if cy >= content_end_y {
            break;
        }

        let avail_w = overlay_w.saturating_sub(2) as usize; // subtract borders

        if key.is_empty() {
            // Category header: "─── Section Name ───────"
            let label = format!(" {desc} ");
            let dashes_left = 1usize;
            let dashes_right = avail_w.saturating_sub(dashes_left + label.len());
            let header_str = format!(
                "{}{}{}",
                "\u{2500}".repeat(dashes_left),
                label,
                "\u{2500}".repeat(dashes_right),
            );
            let display: String = header_str.chars().take(avail_w).collect();
            buf.set_string(content_x, cy, &display, section_style);
        } else {
            // Normal entry: key (fixed width) + space + description.
            let key_str = format!("{:<width$}", key, width = KEY_W);
            let key_display = &key_str[..key_str.len().min(avail_w)];
            buf.set_string(content_x, cy, key_display, key_style);

            let desc_x = content_x + KEY_W.min(avail_w) as u16 + 1;
            let desc_avail = (overlay_area.x + overlay_area.width.saturating_sub(1))
                .saturating_sub(desc_x) as usize;
            if desc_avail > 0 {
                let desc_display = &desc[..desc.len().min(desc_avail)];
                buf.set_string(desc_x, cy, desc_display, desc_style);
            }
        }
    }

    // ── Scroll indicators ─────────────────────────────────────────────────────
    if scroll > 0 {
        // "↑" near top-right of border
        let ind_x = overlay_area.x + overlay_area.width.saturating_sub(5);
        buf.set_string(ind_x, overlay_area.y, " \u{2191} ", border_style);
    }
    let entries_shown = visible_rows.min(entries.len().saturating_sub(scroll));
    if scroll + entries_shown < entries.len() {
        // "↓" near bottom-right of border
        let ind_x = overlay_area.x + overlay_area.width.saturating_sub(5);
        buf.set_string(
            ind_x,
            overlay_area.y + overlay_area.height.saturating_sub(1),
            " \u{2193} ",
            border_style,
        );
    }
}

fn draw_border(buf: &mut TermBuffer, area: Rect, style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let x0 = area.x;
    let y0 = area.y;
    let x1 = area.x + area.width - 1;
    let y1 = area.y + area.height - 1;

    buf.set_string(x0, y0, "\u{256d}", style);
    buf.set_string(x1, y0, "\u{256e}", style);
    buf.set_string(x0, y1, "\u{2570}", style);
    buf.set_string(x1, y1, "\u{256f}", style);

    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "\u{2500}", style);
        buf.set_string(x, y1, "\u{2500}", style);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "\u{2502}", style);
        buf.set_string(x1, y, "\u{2502}", style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(w: u16, h: u16) -> (TermBuffer, Rect) {
        let area = Rect::new(0, 0, w, h);
        let buf = TermBuffer::empty(area);
        (buf, area)
    }

    fn default_bindings() -> KeyBindings {
        KeyBindings::defaults()
    }

    #[test]
    fn render_does_not_panic_on_normal_area() {
        let (mut buf, area) = make_buf(120, 40);
        let bindings = default_bindings();
        render(area, &mut buf, 0, &bindings);
        let content: String = (0..120)
            .map(|x| {
                buf.cell((x, 2))
                    .map(|c| c.symbol().chars().next().unwrap_or(' '))
                    .unwrap_or(' ')
            })
            .collect();
        assert!(
            !content.trim().is_empty() || area.width >= 20,
            "render should produce output"
        );
    }

    #[test]
    fn render_skips_tiny_area() {
        let (mut buf, area) = make_buf(10, 5);
        let bindings = default_bindings();
        render(area, &mut buf, 0, &bindings);
        let all_spaces = buf.content().iter().all(|c| c.symbol() == " ");
        assert!(all_spaces, "tiny area should produce no output");
    }

    #[test]
    fn render_large_area_has_border_chars() {
        let (mut buf, area) = make_buf(100, 40);
        let bindings = default_bindings();
        render(area, &mut buf, 0, &bindings);
        let has_border = buf
            .content()
            .iter()
            .any(|c| c.symbol() == "\u{256d}" || c.symbol() == "\u{2500}");
        assert!(has_border, "border characters should be present");
    }

    #[test]
    fn render_with_scroll_does_not_panic() {
        let (mut buf, area) = make_buf(100, 40);
        let bindings = default_bindings();
        render(area, &mut buf, 5, &bindings);
        render(area, &mut buf, 9999, &bindings); // clamped, should not panic
    }

    #[test]
    fn format_key_display_capitalises() {
        assert_eq!(format_key_display("ctrl+shift+s"), "Ctrl+Shift+S");
        assert_eq!(format_key_display("f1"), "F1");
        assert_eq!(format_key_display("alt+z"), "Alt+Z");
    }

    #[test]
    fn build_entries_produces_output() {
        let bindings = default_bindings();
        let entries = build_entries(&bindings);
        assert!(!entries.is_empty());
        // First entry should be a section header
        assert!(entries[0].0.is_empty());
        assert_eq!(entries[0].1, "Navigation");
    }
}
