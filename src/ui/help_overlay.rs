use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use crate::input::keybinding::KeyBindings;
use crate::ui::overlay_chrome::draw_border;
use crate::ui::text_utils::truncate_to_width;

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
    HelpEntry::Binding {
        actions: &["open_search"],
        desc: "Search files (sidebar focus)",
    },
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
        desc: "Toggle this help  (\u{2190}/\u{2192} tabs, \u{2191}\u{2193} scroll)",
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

/// One help tab: a display label plus the template sections it contains.
pub struct HelpTab {
    pub label: &'static str,
    pub sections: &'static [&'static str],
}

/// The help overlay is split into tabs so each screen shows a small,
/// related set of keybindings instead of one long multi-column dump.
pub const TABS: &[HelpTab] = &[
    HelpTab {
        label: "Basics",
        sections: &["Navigation", "Selection"],
    },
    HelpTab {
        label: "Editing",
        sections: &["Multi-cursor", "Line transforms", "Editing"],
    },
    HelpTab {
        label: "Files",
        sections: &["Clipboard", "File & Tabs"],
    },
    HelpTab {
        label: "Panels",
        sections: &["Panels & Pickers", "Sidebar"],
    },
    HelpTab {
        label: "Search & LSP",
        sections: &["Search", "LSP (when active)"],
    },
    HelpTab {
        label: "Git & App",
        sections: &["View & App", "Git"],
    },
];

/// Number of help tabs. Used for digit jumps (1–9) and wrap-around cycling.
pub const NUM_TABS: usize = TABS.len();

/// Layout constants for the help overlay.
const MIN_OVERLAY_W: u16 = 50;
const MAX_OVERLAY_W: u16 = 160;
const COL_GAP: u16 = 2;
/// Upper bound on content columns; fewer are used when the content fits.
const MAX_COLS: usize = 4;
/// Minimum sensible display width for one content column.
const MIN_COL_W: usize = 22;
/// Top border w/ title (1) + tab bar (1) + separator (1) + bottom border (1).
const CHROME_ROWS: u16 = 4;

/// Compute the centred overlay rect for a given terminal area. Shared by
/// the renderer and the mouse hit-tester so they always agree.
fn overlay_rect(area: Rect) -> Rect {
    let target_w = area.width.saturating_sub(2);
    let overlay_w = target_w.clamp(MIN_OVERLAY_W, MAX_OVERLAY_W).min(area.width);
    let overlay_h = area.height.saturating_sub(2).max(8).min(area.height);
    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 2;
    Rect::new(ox, oy, overlay_w, overlay_h)
}

/// Width of one content column when `n` columns are displayed inside
/// `inner_w`.
fn col_width(inner_w: u16, n: usize) -> usize {
    let n16 = n as u16;
    let usable = inner_w.saturating_sub(COL_GAP * (n16 - 1));
    (usable / n16).max(1) as usize
}

/// Positions of the tab-bar labels for a given terminal area.
struct TabLayout {
    /// True when the full label bar does not fit and a compact
    /// `n / N label` indicator is rendered instead.
    compact: bool,
    x: u16,
    y: u16,
    widths: Vec<u16>,
    gap: u16,
}

fn tab_layout(area: Rect) -> TabLayout {
    let overlay = overlay_rect(area);
    let inner_w = overlay.width.saturating_sub(2) as usize;
    let y = overlay.y + 1;
    let widths: Vec<usize> = TABS
        .iter()
        .map(|t| UnicodeWidthStr::width(t.label) + 2)
        .collect();
    let sum: usize = widths.iter().sum();
    let n = TABS.len();
    let wide_total = sum + 3 * (n - 1);
    let (gap, total) = if wide_total <= inner_w {
        (3, wide_total)
    } else if sum + (n - 1) <= inner_w {
        (1, sum + (n - 1))
    } else {
        return TabLayout {
            compact: true,
            x: overlay.x,
            y,
            widths: Vec::new(),
            gap: 0,
        };
    };
    let x = overlay.x + ((overlay.width as usize).saturating_sub(total) / 2) as u16;
    TabLayout {
        compact: false,
        x,
        y,
        widths: widths.iter().map(|&w| w as u16).collect(),
        gap: gap as u16,
    }
}

/// Hit-test the help overlay's tab bar. Returns the tab index when
/// `(col, row)` lands on a rendered tab label. In compact mode (bar too
/// wide for the terminal) no tabs are clickable.
pub fn tab_at(area: Rect, col: u16, row: u16) -> Option<usize> {
    let layout = tab_layout(area);
    if layout.compact || row != layout.y {
        return None;
    }
    let mut x = layout.x;
    for (i, w) in layout.widths.iter().enumerate() {
        if col >= x && col < x + *w {
            return Some(i);
        }
        x = x.saturating_add(*w).saturating_add(layout.gap);
    }
    None
}

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
#[derive(Clone)]
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

/// Build the rendered lines (one header line plus wrapped entries) for a
/// single section.
fn build_section_block(
    name: &'static str,
    items: &[HelpItem],
    key_w: usize,
    desc_w: usize,
) -> Vec<ColumnLine> {
    let mut lines = vec![ColumnLine::Section(name)];
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
    lines
}

/// Build the wrapped line blocks of one tab's sections, in order.
fn build_section_blocks(
    sections: &[&str],
    groups: &[(&'static str, Vec<HelpItem>)],
    key_w: usize,
    desc_w: usize,
) -> Vec<Vec<ColumnLine>> {
    sections
        .iter()
        .filter_map(|section_name| {
            groups
                .iter()
                .find(|(n, _)| n == section_name)
                .map(|(name, items)| build_section_block(name, items, key_w, desc_w))
        })
        .collect()
}

/// Greedily distribute whole section blocks into `n` columns, inserting a
/// blank separator line between blocks within a column. A new column is
/// only started when the current one already holds content and the next
/// block would overflow it, so content is never split mid-section unless
/// a single section alone exceeds `visible` rows.
fn split_blocks(blocks: &[Vec<ColumnLine>], n: usize, visible: usize) -> Vec<Vec<ColumnLine>> {
    let mut cols: Vec<Vec<ColumnLine>> = vec![Vec::new(); n];
    let mut ci = 0usize;
    for block in blocks {
        if !cols[ci].is_empty() && cols[ci].len() + 1 + block.len() > visible && ci + 1 < n {
            ci += 1;
        }
        if !cols[ci].is_empty() {
            cols[ci].push(ColumnLine::Blank);
        }
        cols[ci].extend_from_slice(block);
    }
    cols
}

/// True when every block of `blocks` fits in `n` columns of `visible` rows
/// without scrolling.
fn fits(blocks: &[Vec<ColumnLine>], n: usize, visible: usize) -> bool {
    split_blocks(blocks, n, visible)
        .iter()
        .all(|col| col.len() <= visible)
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

/// Render the tab bar row (`overlay.y + 1`). In compact mode a centred
/// `n / N label` indicator replaces the per-tab labels.
fn render_tab_bar(
    buf: &mut TermBuffer,
    area: Rect,
    active: usize,
    active_style: Style,
    inactive_style: Style,
    border_style: Style,
) {
    let layout = tab_layout(area);
    if layout.compact {
        let overlay = overlay_rect(area);
        let text = format!(" {} / {} {} ", active + 1, NUM_TABS, TABS[active].label);
        let text = truncate_to_width(&text, overlay.width.saturating_sub(2) as usize);
        let tx = overlay.x.saturating_add(
            overlay
                .width
                .saturating_sub(UnicodeWidthStr::width(text) as u16)
                / 2,
        );
        buf.set_string(tx, layout.y, text, active_style);
        return;
    }
    let mut x = layout.x;
    for (i, tab) in TABS.iter().enumerate() {
        let label = format!(" {} ", tab.label);
        let style = if i == active {
            active_style
        } else {
            inactive_style
        };
        let display = truncate_to_width(&label, layout.widths[i] as usize);
        buf.set_string(x, layout.y, display, style);
        x += layout.widths[i];
        if i + 1 < TABS.len() {
            // Draw the separator glyph centred in the gap between labels.
            let sep_x = x + layout.gap / 2;
            buf.set_string(sep_x, layout.y, "\u{2502}", border_style);
            x += layout.gap;
        }
    }
}

/// Render one tab of the keybinding cheat-sheet as a centred overlay that
/// stretches to nearly the full terminal width (capped at `MAX_OVERLAY_W`).
///
/// `tab` selects the active help tab (clamped to range). `scroll` is the
/// number of rows to skip from the top of each content column; it is
/// clamped to the tallest column's row count. Content is laid out in the
/// smallest number of columns (1–4) that fits without scrolling.
pub fn render(area: Rect, buf: &mut TermBuffer, tab: usize, scroll: usize, bindings: &KeyBindings) {
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
    let active_tab_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(160, 210, 255))
        .add_modifier(Modifier::BOLD);
    let inactive_tab_style = Style::default().bg(bg).fg(Color::Rgb(110, 125, 165));
    let section_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(100, 130, 180))
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().bg(bg).fg(Color::Rgb(140, 200, 255));
    let desc_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));

    // ── Overlay dimensions ────────────────────────────────────────────
    let overlay_area = overlay_rect(area);
    let tab = tab.min(NUM_TABS.saturating_sub(1));

    // ── Fill background ───────────────────────────────────────────────
    for y in overlay_area.y..overlay_area.y + overlay_area.height {
        for x in overlay_area.x..overlay_area.x + overlay_area.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // ── Border ────────────────────────────────────────────────────────
    draw_border(buf, overlay_area, border_style);

    // ── Header (on the top border row) ────────────────────────────────
    let header = " Keybindings ";
    let hx = overlay_area.x + overlay_area.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, overlay_area.y, header, header_style);

    // ── Tab bar ───────────────────────────────────────────────────────
    render_tab_bar(
        buf,
        area,
        tab,
        active_tab_style,
        inactive_tab_style,
        border_style,
    );

    // Separator line beneath the tab bar.
    let sep_y = overlay_area.y + 2;
    for x in overlay_area.x + 1..overlay_area.x + overlay_area.width.saturating_sub(1) {
        buf.set_string(x, sep_y, "\u{2500}", border_style);
    }

    // ── Adaptive column layout for the active tab ─────────────────────
    let inner_w = overlay_area.width.saturating_sub(2);
    let visible_rows = overlay_area.height.saturating_sub(CHROME_ROWS) as usize;
    let max_cols_by_width = (1..=MAX_COLS)
        .take_while(|&n| n == 1 || col_width(inner_w, n) >= MIN_COL_W)
        .count()
        .max(1);

    let groups = build_grouped(bindings);
    let active_tab = &TABS[tab];

    let build_cols = |n: usize| {
        let c = col_width(inner_w, n);
        let key_w = (c * 4 / 10).max(6);
        let desc_w = c.saturating_sub(key_w + 1).max(1);
        let blocks = build_section_blocks(active_tab.sections, &groups, key_w, desc_w);
        (c, key_w, desc_w, blocks)
    };

    let (col_w, key_w, desc_w, columns) = {
        let mut chosen: Option<(usize, usize, usize, Vec<Vec<ColumnLine>>)> = None;
        for n in 1..=max_cols_by_width {
            let (c, kw, dw, blocks) = build_cols(n);
            if fits(&blocks, n, visible_rows) {
                chosen = Some((c, kw, dw, split_blocks(&blocks, n, visible_rows)));
                break;
            }
        }
        match chosen {
            Some((c, kw, dw, cols)) => (c, kw, dw, cols),
            None => {
                let (c, kw, dw, blocks) = build_cols(max_cols_by_width);
                let cols = split_blocks(&blocks, max_cols_by_width, visible_rows);
                (c, kw, dw, cols)
            }
        }
    };

    // ── Scroll clamping ───────────────────────────────────────────────
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
        render(area, &mut buf, 0, 0, &bindings);
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
        render(area, &mut buf, 0, 0, &bindings);
        let all_spaces = buf.content().iter().all(|c| c.symbol() == " ");
        assert!(all_spaces, "tiny area should produce no output");
    }

    #[test]
    fn render_large_area_has_border_chars() {
        let (mut buf, area) = make_buf(100, 40);
        let bindings = default_bindings();
        render(area, &mut buf, 0, 0, &bindings);
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
        render(area, &mut buf, 0, 5, &bindings);
        render(area, &mut buf, 0, 9999, &bindings); // clamped, should not panic
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
            render(area, &mut buf, 0, 0, &bindings);
        }
    }

    #[test]
    fn build_grouped_produces_sections() {
        let bindings = default_bindings();
        let groups = build_grouped(&bindings);
        assert!(!groups.is_empty());
        assert_eq!(groups[0].0, "Navigation");
        assert!(!groups[0].1.is_empty());
        // Every section listed in TABS must exist in TEMPLATE.
        for tab in TABS.iter() {
            for &name in tab.sections.iter() {
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

    fn dump(area: Rect, buf: &TermBuffer) -> String {
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(c) = buf.cell((x, y)) {
                    text.push_str(c.symbol());
                }
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn render_shows_tab_bar_and_active_sections() {
        let (mut buf, area) = make_buf(160, 50);
        let bindings = default_bindings();
        render(area, &mut buf, 0, 0, &bindings);
        let text = dump(area, &buf);
        // All tab labels are visible on a wide terminal.
        for tab in TABS.iter() {
            assert!(text.contains(tab.label), "tab label missing: {}", tab.label);
        }
        // Tab 0 shows both of its sections.
        assert!(text.contains("Navigation"), "tab 0 header missing");
        assert!(text.contains("Selection"), "tab 0 second header missing");
        // Sections from other tabs must not leak into tab 0.
        assert!(!text.contains("Clipboard"), "foreign section leaked");
    }

    #[test]
    fn render_each_tab_shows_its_sections() {
        let bindings = default_bindings();
        for (i, tab) in TABS.iter().enumerate() {
            let (mut buf, area) = make_buf(160, 50);
            render(area, &mut buf, i, 0, &bindings);
            let text = dump(area, &buf);
            for &section in tab.sections.iter() {
                assert!(
                    text.contains(section),
                    "tab {i} ({}) missing section: {section}",
                    tab.label
                );
            }
        }
    }

    #[test]
    fn tab_at_hit_tests_tab_bar() {
        let area = Rect::new(0, 0, 120, 40);
        let overlay = overlay_rect(area);
        let layout = tab_layout(area);
        assert!(!layout.compact, "labels should fit at 120 cols");
        assert_eq!(layout.y, overlay.y + 1);

        // First label starts at layout.x; edges outside the bar are not tabs.
        assert_eq!(tab_at(area, layout.x, layout.y), Some(0));
        assert_eq!(tab_at(area, overlay.x, layout.y), None);
        assert_eq!(tab_at(area, layout.x, layout.y + 1), None);

        // Just past the final label is not a tab.
        let end =
            layout.x + layout.widths.iter().sum::<u16>() + layout.gap * (TABS.len() as u16 - 1);
        assert_eq!(tab_at(area, end, layout.y), None);

        // Every tab label midpoint resolves to its own index.
        let mut x = layout.x;
        for (i, w) in layout.widths.iter().enumerate() {
            let mid = x + w / 2;
            assert_eq!(tab_at(area, mid, layout.y), Some(i));
            x += w + layout.gap;
        }
    }

    #[test]
    fn tab_at_compact_mode_is_not_clickable() {
        // At 24 cols the overlay is tiny and the label bar cannot fit.
        let area = Rect::new(0, 0, 24, 12);
        assert!(tab_layout(area).compact);
        let row = tab_layout(area).y;
        assert_eq!(tab_at(area, 0, row), None);
    }

    #[test]
    fn split_blocks_never_splits_sections_when_they_fit() {
        let bindings = default_bindings();
        let groups = build_grouped(&bindings);
        let blocks = build_section_blocks(TABS[0].sections, &groups, 30, 20);
        let total: usize = blocks.iter().map(|b| b.len()).sum();
        // Everything fits in one wide column, with one blank between blocks.
        let one = split_blocks(&blocks, 1, 1000);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].len(), total + blocks.len() - 1);
        // Two columns with room to spare: the second stays empty.
        let two = split_blocks(&blocks, 2, 1000);
        assert!(two[1].is_empty());
    }

    #[test]
    fn render_adapts_column_count_to_content() {
        // On a tall terminal a tab that fits needs no scrolling.
        let (mut buf, area) = make_buf(120, 60);
        let bindings = default_bindings();
        render(area, &mut buf, 3, 0, &bindings);
        let text = dump(area, &buf);
        assert!(text.contains("Panels & Pickers"));
        assert!(text.contains("Sidebar"));
        // Both sections appear without a down-scroll hint.
        assert!(!text.contains(" \u{2193} "), "tab should fit at 120x60");
    }

    #[test]
    fn render_all_tabs_do_not_panic_at_narrow_widths() {
        let bindings = default_bindings();
        for tab in 0..NUM_TABS {
            for w in 20..=80 {
                let (mut buf, area) = make_buf(w, 40);
                render(area, &mut buf, tab, 0, &bindings);
            }
        }
    }

    #[test]
    fn render_out_of_range_tab_is_clamped() {
        let (mut buf, area) = make_buf(120, 40);
        let bindings = default_bindings();
        render(area, &mut buf, NUM_TABS, 0, &bindings);
        render(area, &mut buf, usize::MAX, 0, &bindings);
        let text = dump(area, &buf);
        // Clamp lands on the last tab.
        assert!(
            text.contains("View & App"),
            "clamped tab should render last tab"
        );
    }
}
