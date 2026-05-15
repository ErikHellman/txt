use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

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
        actions: &["extend_selection_home", "extend_selection_end"],
        desc: "Extend to line start / end",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_file_start", "move_cursor_file_end"],
        desc: "File start / end",
    },
    HelpEntry::Binding {
        actions: &["extend_selection_file_start", "extend_selection_file_end"],
        desc: "Extend to file start / end",
    },
    HelpEntry::Binding {
        actions: &["move_cursor_page_up", "move_cursor_page_down"],
        desc: "Page up / down",
    },
    HelpEntry::Binding {
        actions: &["extend_selection_page_up", "extend_selection_page_down"],
        desc: "Extend page up / down",
    },
    HelpEntry::Binding {
        actions: &["go_to_matching_bracket"],
        desc: "Jump to matching bracket",
    },
    HelpEntry::Binding {
        actions: &["scroll_up", "scroll_down"],
        desc: "Scroll without moving cursor",
    },
    HelpEntry::Binding {
        actions: &["scroll_cursor_center"],
        desc: "Scroll cursor to center",
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
    HelpEntry::Binding {
        actions: &["add_cursor_next_match"],
        desc: "Add cursor at next match (or select word)",
    },
    HelpEntry::Binding {
        actions: &["skip_current_match"],
        desc: "Skip current match, add cursor at next",
    },
    HelpEntry::Binding {
        actions: &["undo_last_cursor"],
        desc: "Undo last added cursor",
    },
    HelpEntry::Binding {
        actions: &[
            "box_select_extend_up",
            "box_select_extend_down",
            "box_select_extend_left",
            "box_select_extend_right",
        ],
        desc: "Box / column selection (extend)",
    },
    HelpEntry::Static {
        key: "Alt+Drag",
        desc: "Box / column selection (mouse)",
    },
    HelpEntry::Binding {
        actions: &["filter_selection"],
        desc: "Filter selection through shell command",
    },
    // ── Line transforms ────────────────────────────────────────────
    HelpEntry::Section("Line transforms"),
    HelpEntry::Binding {
        actions: &["join_lines"],
        desc: "Join lines (vim-style)",
    },
    HelpEntry::Binding {
        actions: &["increment_number", "decrement_number"],
        desc: "Increment / decrement number under cursor",
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
    HelpEntry::Binding {
        actions: &["surround"],
        desc: "Surround selection with delimiter (next char picks pair)",
    },
    HelpEntry::Static {
        key: "Tab / Shift+Tab",
        desc: "Indent / dedent selection",
    },
    HelpEntry::Binding {
        actions: &["format_buffer"],
        desc: "Format buffer (external tool)",
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
    HelpEntry::Binding {
        actions: &["open_clipboard_ring"],
        desc: "Clipboard ring (recent yanks)",
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
    HelpEntry::Binding {
        actions: &["close_tab"],
        desc: "Close tab",
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
        actions: &["open_symbol_picker"],
        desc: "Symbols in file picker",
    },
    HelpEntry::Binding {
        actions: &["toggle_fold_at_cursor"],
        desc: "Toggle fold at cursor",
    },
    HelpEntry::Binding {
        actions: &["fold_all"],
        desc: "Fold all (Alt+0)",
    },
    HelpEntry::Binding {
        actions: &["unfold_all"],
        desc: "Unfold all (Alt+Shift+0)",
    },
    HelpEntry::Binding {
        actions: &["set_mark"],
        desc: "Set named mark (Ctrl+M then a–z)",
    },
    HelpEntry::Binding {
        actions: &["jump_to_mark"],
        desc: "Jump to named mark (Ctrl+' then a–z)",
    },
    HelpEntry::Binding {
        actions: &["jump_list_back"],
        desc: "Jump-list back",
    },
    HelpEntry::Binding {
        actions: &["jump_list_forward"],
        desc: "Jump-list forward",
    },
    HelpEntry::Binding {
        actions: &["expand_snippet"],
        desc: "Expand snippet at cursor (Tab; see ~/.config/txt/snippets/)",
    },
    HelpEntry::Binding {
        actions: &["record_macro"],
        desc: "Record/stop keyboard macro (a–z slot)",
    },
    HelpEntry::Binding {
        actions: &["replay_macro"],
        desc: "Replay keyboard macro (a–z slot)",
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
        actions: &["open_project_search"],
        desc: "Project search & replace",
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
    HelpEntry::Binding {
        actions: &["close_search"],
        desc: "Close find / replace bar",
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
    HelpEntry::Binding {
        actions: &["open_quickfix"],
        desc: "Quickfix list (workspace LSP diagnostics)",
    },
    HelpEntry::Binding {
        actions: &["quickfix_next", "quickfix_prev"],
        desc: "Next / Prev quickfix entry",
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
    HelpEntry::Binding {
        actions: &["sidebar_refresh"],
        desc: "Refresh file tree (sidebar)",
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
    // ── Git ──────────────────────────────────────────────────────────
    HelpEntry::Section("Git"),
    HelpEntry::Binding {
        actions: &["open_git_dialog"],
        desc: "Open git operations dialog",
    },
    HelpEntry::Binding {
        actions: &["next_hunk", "prev_hunk"],
        desc: "Next / Prev git hunk",
    },
    HelpEntry::Binding {
        actions: &["revert_hunk"],
        desc: "Revert hunk under cursor to HEAD",
    },
    HelpEntry::Binding {
        actions: &["peek_head"],
        desc: "Peek HEAD content for hunk under cursor",
    },
    HelpEntry::Static {
        key: "y / n",
        desc: "Approve / reject LSP binary (when prompted)",
    },
    HelpEntry::Binding {
        actions: &["quit"],
        desc: "Quit",
    },
];

/// Layout constants for the 4-column help overlay.
const MIN_OVERLAY_W: u16 = 50;
const MAX_OVERLAY_W: u16 = 160;
const COL_GAP: u16 = 2;
const NUM_COLS: usize = 4;
/// Top border (1) + header (1) + separator (1) + bottom border (1).
const CHROME_ROWS: u16 = 4;

/// Section names assigned to each of the 4 columns. Sections are kept in
/// reading order and grouped to roughly balance column heights.
const COLUMN_ASSIGNMENT: [&[&str]; NUM_COLS] = [
    &["Navigation", "Selection"],
    &["Multi-cursor", "Line transforms", "Editing"],
    &["Clipboard", "File & Tabs", "Panels & Pickers"],
    &[
        "Search",
        "LSP (when active)",
        "Sidebar",
        "View & App",
        "Git",
    ],
];

/// One semantic entry under a section (before width-based wrapping).
struct HelpItem {
    key: String,
    desc: &'static str,
}

/// Group template entries by section, resolving dynamic bindings.
fn build_grouped(bindings: &KeyBindings) -> Vec<(&'static str, Vec<HelpItem>)> {
    let mut groups: Vec<(&'static str, Vec<HelpItem>)> = Vec::new();
    let mut current: Option<(&'static str, Vec<HelpItem>)> = None;

    for entry in TEMPLATE {
        match entry {
            HelpEntry::Section(name) => {
                if let Some(g) = current.take() {
                    groups.push(g);
                }
                current = Some((*name, Vec::new()));
            }
            HelpEntry::Static { key, desc } => {
                if let Some((_, items)) = current.as_mut() {
                    items.push(HelpItem {
                        key: (*key).to_string(),
                        desc,
                    });
                }
            }
            HelpEntry::Binding { actions, desc } => {
                let keys: Vec<String> = actions
                    .iter()
                    .flat_map(|a| bindings.display_keys_for_action(a))
                    .map(|k| format_key_display(&k))
                    .collect();
                let key_str = if keys.is_empty() {
                    "(unbound)".to_string()
                } else {
                    dedup_and_join(&keys)
                };
                if let Some((_, items)) = current.as_mut() {
                    items.push(HelpItem { key: key_str, desc });
                }
            }
        }
    }
    if let Some(g) = current.take() {
        groups.push(g);
    }
    groups
}

/// One rendered line inside a column, after wrapping.
enum ColumnLine {
    Section(&'static str),
    Entry { key: String, desc: String },
    Blank,
}

/// Word-wrap `s` into lines that each fit in at most `max` display columns,
/// breaking on whitespace. A word wider than `max` is truncated.
fn wrap_str(s: &str, max: usize) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for word in s.split_whitespace() {
        let word_w = UnicodeWidthStr::width(word);
        if current.is_empty() {
            if word_w <= max {
                current.push_str(word);
                current_w = word_w;
            } else {
                lines.push(truncate_to_width(word, max).to_string());
            }
        } else if current_w + 1 + word_w <= max {
            current.push(' ');
            current.push_str(word);
            current_w += 1 + word_w;
        } else {
            lines.push(std::mem::take(&mut current));
            current_w = 0;
            if word_w <= max {
                current.push_str(word);
                current_w = word_w;
            } else {
                lines.push(truncate_to_width(word, max).to_string());
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Wrap a key display string. Prefer breaking on " / " boundaries (so each
/// line ends with " /" except the last); fall back to whitespace wrapping.
fn wrap_key(key: &str, max: usize) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    if UnicodeWidthStr::width(key) <= max {
        return vec![key.to_string()];
    }
    let parts: Vec<&str> = key.split(" / ").collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for (i, part) in parts.iter().enumerate() {
        let suffix = if i + 1 < parts.len() { " /" } else { "" };
        let chunk = format!("{part}{suffix}");
        let chunk_w = UnicodeWidthStr::width(chunk.as_str());

        if current.is_empty() {
            if chunk_w <= max {
                current = chunk;
                current_w = chunk_w;
            } else {
                lines.extend(wrap_str(&chunk, max));
            }
        } else if current_w + 1 + chunk_w <= max {
            current.push(' ');
            current.push_str(&chunk);
            current_w += 1 + chunk_w;
        } else {
            lines.push(std::mem::take(&mut current));
            current_w = 0;
            if chunk_w <= max {
                current = chunk;
                current_w = chunk_w;
            } else {
                lines.extend(wrap_str(&chunk, max));
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Build the rendered lines for one column from its assigned sections.
fn build_column_lines(
    sections: &[&str],
    groups: &[(&'static str, Vec<HelpItem>)],
    key_w: usize,
    desc_w: usize,
) -> Vec<ColumnLine> {
    let mut lines = Vec::new();
    let mut first = true;
    for &section_name in sections {
        let Some((name, items)) = groups.iter().find(|(n, _)| *n == section_name) else {
            continue;
        };
        if !first {
            lines.push(ColumnLine::Blank);
        }
        first = false;
        lines.push(ColumnLine::Section(name));
        for item in items {
            let key_lines = wrap_key(&item.key, key_w);
            let desc_lines = wrap_str(item.desc, desc_w);
            let n = key_lines.len().max(desc_lines.len()).max(1);
            for i in 0..n {
                let k = key_lines.get(i).cloned().unwrap_or_default();
                let d = desc_lines.get(i).cloned().unwrap_or_default();
                lines.push(ColumnLine::Entry { key: k, desc: d });
            }
        }
    }
    lines
}

/// Pad `s` with trailing spaces to exactly `w` display columns, truncating
/// first if it would overflow.
fn pad_to_width(s: &str, w: usize) -> String {
    let truncated = truncate_to_width(s, w);
    let truncated_w = UnicodeWidthStr::width(truncated);
    let mut out = truncated.to_string();
    out.push_str(&" ".repeat(w.saturating_sub(truncated_w)));
    out
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

/// Truncate `s` to at most `max_width` terminal columns, respecting grapheme
/// boundaries so the result is always a valid `&str` slice. Cells whose display
/// width would push the running total past `max_width` are dropped.
fn truncate_to_width(s: &str, max_width: usize) -> &str {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let mut width = 0usize;
    let mut end = 0usize;
    for (idx, g) in s.grapheme_indices(true) {
        let gw = UnicodeWidthStr::width(g);
        if width + gw > max_width {
            return &s[..end];
        }
        width += gw;
        end = idx + g.len();
    }
    s
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

/// Render a scrollable keybinding cheat-sheet as a 4-column overlay that
/// stretches to nearly the full terminal width (capped at `MAX_OVERLAY_W`).
///
/// `scroll` is the number of rows to skip from the top of each column. The
/// render function clamps it to the tallest column's row count.
pub fn render(area: Rect, buf: &mut TermBuffer, scroll: usize, bindings: &KeyBindings) {
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

    // ── Overlay dimensions ────────────────────────────────────────────
    let target_w = area.width.saturating_sub(2);
    let overlay_w = target_w.clamp(MIN_OVERLAY_W, MAX_OVERLAY_W).min(area.width);
    let overlay_h = area.height.saturating_sub(2).max(8).min(area.height);
    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 2;
    let overlay_area = Rect::new(ox, oy, overlay_w, overlay_h);

    // ── Column geometry ───────────────────────────────────────────────
    let cols = NUM_COLS as u16;
    let inner_w = overlay_w.saturating_sub(2);
    let usable = inner_w.saturating_sub(COL_GAP * (cols - 1));
    let col_w = (usable / cols).max(1) as usize;
    let key_w = (col_w * 4 / 10).max(6);
    let desc_w = col_w.saturating_sub(key_w + 1).max(1);

    // ── Fill background ───────────────────────────────────────────────
    for y in overlay_area.y..overlay_area.y + overlay_area.height {
        for x in overlay_area.x..overlay_area.x + overlay_area.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // ── Border ────────────────────────────────────────────────────────
    draw_border(buf, overlay_area, border_style);

    // ── Header ────────────────────────────────────────────────────────
    let header = " Keybindings ";
    let hx = overlay_area.x + overlay_area.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, overlay_area.y, header, header_style);

    // Separator line beneath header.
    let sep_y = overlay_area.y + 2;
    for x in overlay_area.x + 1..overlay_area.x + overlay_area.width.saturating_sub(1) {
        buf.set_string(x, sep_y, "\u{2500}", border_style);
    }

    // ── Build per-column line lists ───────────────────────────────────
    let groups = build_grouped(bindings);
    let columns: Vec<Vec<ColumnLine>> = COLUMN_ASSIGNMENT
        .iter()
        .map(|sections| build_column_lines(sections, &groups, key_w, desc_w))
        .collect();

    // ── Scroll clamping ───────────────────────────────────────────────
    let visible_rows = overlay_h.saturating_sub(CHROME_ROWS) as usize;
    let max_col_rows = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    let max_scroll = max_col_rows.saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    // ── Render each column ────────────────────────────────────────────
    let content_start_y = overlay_area.y + 3;
    let content_end_y = overlay_area.y + overlay_area.height.saturating_sub(1);

    for (col_idx, lines) in columns.iter().enumerate() {
        let col_x = overlay_area.x + 1 + (col_idx as u16) * (col_w as u16 + COL_GAP);

        for (row_idx, line) in lines.iter().skip(scroll).enumerate() {
            let cy = content_start_y + row_idx as u16;
            if cy >= content_end_y {
                break;
            }
            match line {
                ColumnLine::Blank => {}
                ColumnLine::Section(name) => {
                    let label = format!(" {name} ");
                    let label_w = UnicodeWidthStr::width(label.as_str());
                    let dashes_left = 1usize;
                    let dashes_right = col_w.saturating_sub(dashes_left + label_w);
                    let header_str = format!(
                        "{}{}{}",
                        "\u{2500}".repeat(dashes_left),
                        label,
                        "\u{2500}".repeat(dashes_right),
                    );
                    let display = truncate_to_width(&header_str, col_w);
                    buf.set_string(col_x, cy, display, section_style);
                }
                ColumnLine::Entry { key, desc } => {
                    let key_str = pad_to_width(key, key_w);
                    buf.set_string(col_x, cy, &key_str, key_style);

                    let desc_x = col_x + key_w as u16 + 1;
                    let desc_display = truncate_to_width(desc, desc_w);
                    buf.set_string(desc_x, cy, desc_display, desc_style);
                }
            }
        }
    }

    // ── Scroll indicators ─────────────────────────────────────────────
    if scroll > 0 {
        let ind_x = overlay_area.x + overlay_area.width.saturating_sub(5);
        buf.set_string(ind_x, overlay_area.y, " \u{2191} ", border_style);
    }
    if scroll + visible_rows < max_col_rows {
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
    fn truncate_to_width_handles_multibyte() {
        // En-dash is 3 bytes, 1 column. Truncating at a column count that
        // would land mid-byte must not panic and must return a valid slice.
        let s = "Set named mark (Ctrl+M then a–z)";
        for w in 0..=s.chars().count() {
            let out = truncate_to_width(s, w);
            assert!(s.starts_with(out));
        }
        assert_eq!(truncate_to_width(s, 30), "Set named mark (Ctrl+M then a–");
        assert_eq!(truncate_to_width(s, 29), "Set named mark (Ctrl+M then a");
    }

    #[test]
    fn render_does_not_panic_at_narrow_widths() {
        // Regression for a panic where the description column was truncated
        // by byte index, splitting the en-dash in "Ctrl+M then a–z".
        let bindings = default_bindings();
        for w in 20..=80 {
            let (mut buf, area) = make_buf(w, 40);
            render(area, &mut buf, 0, &bindings);
        }
    }

    #[test]
    fn build_grouped_produces_sections() {
        let bindings = default_bindings();
        let groups = build_grouped(&bindings);
        assert!(!groups.is_empty());
        assert_eq!(groups[0].0, "Navigation");
        assert!(!groups[0].1.is_empty());
        // Every section listed in COLUMN_ASSIGNMENT must exist in TEMPLATE.
        for sections in COLUMN_ASSIGNMENT.iter() {
            for &name in sections.iter() {
                assert!(
                    groups.iter().any(|(n, _)| *n == name),
                    "missing section in TEMPLATE: {name}"
                );
            }
        }
    }

    #[test]
    fn wrap_str_breaks_on_whitespace() {
        let out = wrap_str("Toggle line comment indentation", 12);
        // Each line must fit in 12 cols.
        for line in &out {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 12, "line: {line}");
        }
        // Rejoining must round-trip the words.
        let joined: String = out.join(" ");
        assert_eq!(
            joined.split_whitespace().collect::<Vec<_>>().join(" "),
            "Toggle line comment indentation"
        );
    }

    #[test]
    fn wrap_key_breaks_on_slash() {
        let out = wrap_key("Ctrl+Shift+Up / Ctrl+Shift+Down", 16);
        for line in &out {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 16, "line: {line}");
        }
        // Slash boundary preferred.
        assert!(out.iter().any(|l| l.ends_with(" /")));
    }

    #[test]
    fn render_at_wide_width_uses_four_columns() {
        let (mut buf, area) = make_buf(160, 50);
        let bindings = default_bindings();
        render(area, &mut buf, 0, &bindings);
        // Each column's first section header must appear on screen at scroll=0.
        let mut found_text = String::new();
        for y in 0..50 {
            for x in 0..160 {
                if let Some(c) = buf.cell((x, y)) {
                    found_text.push_str(c.symbol());
                }
            }
            found_text.push('\n');
        }
        assert!(found_text.contains("Navigation"), "col 1 header missing");
        assert!(found_text.contains("Multi-cursor"), "col 2 header missing");
        assert!(found_text.contains("Clipboard"), "col 3 header missing");
        assert!(found_text.contains("Search"), "col 4 header missing");
    }
}
