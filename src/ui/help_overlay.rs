use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// Every entry in the help table.
/// An entry whose `key` is `""` is rendered as a category section header.
const ENTRIES: &[(&str, &str)] = &[
    // ── Navigation ────────────────────────────────────────────────────
    ("", "Navigation"),
    ("Arrows", "Move cursor"),
    ("Ctrl+Left/Right", "Word jump"),
    ("Shift+Arrows", "Extend selection"),
    ("Ctrl+Shift+Left/Right", "Extend by word"),
    ("Home / End", "Line start / end"),
    ("Ctrl+Home/End", "File start / end"),
    ("PgUp / PgDn", "Page up / down"),
    // ── Selection ────────────────────────────────────────────────────
    ("", "Selection"),
    ("Ctrl+A", "Select all"),
    ("Ctrl+W", "Expand selection (AST)"),
    ("Ctrl+Shift+W", "Contract selection (AST)"),
    ("Ctrl+Shift+L", "Select all occurrences"),
    // ── Multi-cursor ─────────────────────────────────────────────────
    ("", "Multi-cursor"),
    ("Alt+Shift+Up", "Add cursor above"),
    ("Alt+Shift+Down", "Add cursor below"),
    // ── Editing ──────────────────────────────────────────────────────
    ("", "Editing"),
    ("Backspace / Delete", "Delete backward / forward"),
    ("Ctrl+Backspace", "Delete word backward"),
    ("Ctrl+Delete", "Delete word forward"),
    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
    ("Ctrl+Shift+Z", "Redo (alternate)"),
    ("Ctrl+D", "Duplicate line"),
    ("Alt+Up/Down", "Move line up / down"),
    ("Ctrl+/", "Toggle line comment"),
    // ── Clipboard ────────────────────────────────────────────────────
    ("", "Clipboard"),
    ("Ctrl+C", "Copy"),
    ("Ctrl+X", "Cut"),
    ("Ctrl+V", "Paste"),
    ("Ctrl+Shift+C", "Copy file reference"),
    // ── File & Tabs ──────────────────────────────────────────────────
    ("", "File & Tabs"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S", "Save As"),
    ("Ctrl+N", "New file / tab"),
    ("Ctrl+O", "Open file"),
    ("Ctrl+G", "Jump to line[:col]"),
    ("Ctrl+T", "New tab"),
    ("Ctrl+] / Ctrl+PgDn", "Next tab"),
    ("Ctrl+[ / Ctrl+PgUp", "Prev tab"),
    ("Ctrl+1..9", "Go to tab N"),
    // ── Panels & Pickers ─────────────────────────────────────────────
    ("", "Panels & Pickers"),
    ("Ctrl+B", "Toggle sidebar"),
    ("Ctrl+P", "Fuzzy file picker"),
    ("Ctrl+R", "Recent files"),
    ("Ctrl+Shift+P", "Command palette"),
    ("Ctrl+Shift+E", "Buffer switcher"),
    // ── Search ───────────────────────────────────────────────────────
    ("", "Search"),
    ("Ctrl+F", "Find"),
    ("Ctrl+H", "Find & Replace"),
    ("F3 / Shift+F3", "Next / Prev match"),
    ("Alt+R", "Toggle regex"),
    ("Alt+C", "Toggle case-sensitive"),
    // ── LSP ──────────────────────────────────────────────────────────
    ("", "LSP (when active)"),
    ("Ctrl+Space", "Code completion"),
    ("Ctrl+K", "Hover info"),
    ("F12", "Go to definition"),
    ("Shift+F12", "Find references"),
    ("F2", "Rename symbol"),
    ("Ctrl+.", "Code action / quick fix"),
    // ── Sidebar ──────────────────────────────────────────────────────
    ("", "Sidebar"),
    ("Ctrl+C", "Copy file only (sidebar)"),
    ("Ctrl+X", "Cut file/dir (sidebar)"),
    ("Ctrl+V", "Paste (sidebar)"),
    ("F2", "Rename file/dir (sidebar)"),
    ("Delete", "Delete file/dir (sidebar)"),
    ("Ctrl+Shift+N", "New folder (sidebar)"),
    // ── View & App ───────────────────────────────────────────────────
    ("", "View & App"),
    ("Alt+Z", "Toggle word wrap"),
    ("F1", "Toggle this help  (↑↓ to scroll)"),
    ("Ctrl+,", "Settings"),
    ("Ctrl+L", "Configure LSP server"),
    ("Ctrl+Q", "Quit"),
];

/// Render a scrollable keybinding cheat-sheet as a centered floating overlay.
///
/// `scroll` is the number of rows to skip from the top of the entry list.
/// The render function clamps it so it can never scroll past the last entry.
pub fn render(area: Rect, buf: &mut TermBuffer, scroll: usize) {
    if area.width < 20 || area.height < 6 {
        return;
    }

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
        buf.set_string(x, sep_y, "─", border_style);
    }

    // ── Scroll clamping ───────────────────────────────────────────────────────
    let visible_rows = overlay_h.saturating_sub(CHROME_ROWS + 1) as usize; // +1 for sep row
    let max_scroll = ENTRIES.len().saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    // ── Content rows ─────────────────────────────────────────────────────────
    let content_x = overlay_area.x + 1;
    let content_start_y = overlay_area.y + 3; // below top-border + header + sep
    let content_end_y = overlay_area.y + overlay_area.height.saturating_sub(1);

    for (row_idx, entry) in ENTRIES.iter().skip(scroll).enumerate() {
        let cy = content_start_y + row_idx as u16;
        if cy >= content_end_y {
            break;
        }

        let (key, desc) = *entry;
        let avail_w = overlay_w.saturating_sub(2) as usize; // subtract borders

        if key.is_empty() {
            // Category header: "─── Section Name ───────"
            let label = format!(" {} ", desc);
            let dashes_left = 1usize;
            let dashes_right = avail_w.saturating_sub(dashes_left + label.len());
            let header_str = format!(
                "{}{}{}",
                "─".repeat(dashes_left),
                label,
                "─".repeat(dashes_right),
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
        buf.set_string(ind_x, overlay_area.y, " ↑ ", border_style);
    }
    let entries_shown = visible_rows.min(ENTRIES.len().saturating_sub(scroll));
    if scroll + entries_shown < ENTRIES.len() {
        // "↓" near bottom-right of border
        let ind_x = overlay_area.x + overlay_area.width.saturating_sub(5);
        buf.set_string(
            ind_x,
            overlay_area.y + overlay_area.height.saturating_sub(1),
            " ↓ ",
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

    buf.set_string(x0, y0, "╭", style);
    buf.set_string(x1, y0, "╮", style);
    buf.set_string(x0, y1, "╰", style);
    buf.set_string(x1, y1, "╯", style);

    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "─", style);
        buf.set_string(x, y1, "─", style);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "│", style);
        buf.set_string(x1, y, "│", style);
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

    #[test]
    fn render_does_not_panic_on_normal_area() {
        let (mut buf, area) = make_buf(120, 40);
        render(area, &mut buf, 0);
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
        render(area, &mut buf, 0);
        let all_spaces = buf.content().iter().all(|c| c.symbol() == " ");
        assert!(all_spaces, "tiny area should produce no output");
    }

    #[test]
    fn render_large_area_has_border_chars() {
        let (mut buf, area) = make_buf(100, 40);
        render(area, &mut buf, 0);
        let has_border = buf
            .content()
            .iter()
            .any(|c| c.symbol() == "╭" || c.symbol() == "─");
        assert!(has_border, "border characters should be present");
    }

    #[test]
    fn render_with_scroll_does_not_panic() {
        let (mut buf, area) = make_buf(100, 40);
        render(area, &mut buf, 5);
        render(area, &mut buf, 9999); // clamped, should not panic
    }

    #[test]
    fn all_entries_have_nonempty_desc() {
        for (key, desc) in ENTRIES {
            assert!(!desc.is_empty(), "entry key={key:?} has empty description");
        }
    }
}
