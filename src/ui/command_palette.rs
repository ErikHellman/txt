use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::input::action::EditorAction;

/// A single entry in the command palette.
pub struct CommandEntry {
    pub name: &'static str,
    pub key_hint: &'static str,
    pub action: fn() -> EditorAction,
}

/// The full list of available commands, shown in the command palette.
pub static COMMANDS: &[CommandEntry] = &[
    CommandEntry {
        name: "Save File",
        key_hint: "Ctrl+S",
        action: || EditorAction::SaveFile,
    },
    CommandEntry {
        name: "Save File As…",
        key_hint: "Ctrl+Shift+S",
        action: || EditorAction::SaveFileAs,
    },
    CommandEntry {
        name: "New File",
        key_hint: "Ctrl+N",
        action: || EditorAction::NewFile,
    },
    CommandEntry {
        name: "Open File…",
        key_hint: "Ctrl+O",
        action: || EditorAction::OpenFile,
    },
    CommandEntry {
        name: "New Tab",
        key_hint: "Ctrl+T",
        action: || EditorAction::NewTab,
    },
    CommandEntry {
        name: "Close Tab",
        key_hint: "Ctrl+W",
        action: || EditorAction::CloseTab,
    },
    CommandEntry {
        name: "Next Tab",
        key_hint: "Ctrl+Tab",
        action: || EditorAction::NextTab,
    },
    CommandEntry {
        name: "Previous Tab",
        key_hint: "Ctrl+Shift+Tab",
        action: || EditorAction::PrevTab,
    },
    CommandEntry {
        name: "Fuzzy File Picker",
        key_hint: "Ctrl+P",
        action: || EditorAction::OpenFuzzyPicker,
    },
    CommandEntry {
        name: "Buffer Switcher",
        key_hint: "Ctrl+Shift+E",
        action: || EditorAction::OpenBufferSwitcher,
    },
    CommandEntry {
        name: "Toggle Sidebar",
        key_hint: "Ctrl+B",
        action: || EditorAction::ToggleSidebar,
    },
    CommandEntry {
        name: "Jump to Line…",
        key_hint: "Ctrl+G",
        action: || EditorAction::JumpToLine,
    },
    CommandEntry {
        name: "Find…",
        key_hint: "Ctrl+F",
        action: || EditorAction::OpenSearch,
    },
    CommandEntry {
        name: "Find & Replace…",
        key_hint: "Ctrl+H",
        action: || EditorAction::OpenReplace,
    },
    CommandEntry {
        name: "Select All",
        key_hint: "Ctrl+A",
        action: || EditorAction::SelectAll,
    },
    CommandEntry {
        name: "Select All Occurrences",
        key_hint: "Ctrl+Shift+L",
        action: || EditorAction::SelectAllOccurrences,
    },
    CommandEntry {
        name: "Undo",
        key_hint: "Ctrl+Z",
        action: || EditorAction::Undo,
    },
    CommandEntry {
        name: "Redo",
        key_hint: "Ctrl+Y",
        action: || EditorAction::Redo,
    },
    CommandEntry {
        name: "Duplicate Line",
        key_hint: "Ctrl+D",
        action: || EditorAction::DuplicateLine,
    },
    CommandEntry {
        name: "Toggle Line Comment",
        key_hint: "Ctrl+/",
        action: || EditorAction::ToggleLineComment,
    },
    CommandEntry {
        name: "Toggle Word Wrap",
        key_hint: "Alt+Z",
        action: || EditorAction::ToggleWordWrap,
    },
    CommandEntry {
        name: "Toggle Help",
        key_hint: "F1",
        action: || EditorAction::ToggleHelp,
    },
    CommandEntry {
        name: "Open Recent Files",
        key_hint: "Ctrl+R",
        action: || EditorAction::OpenRecentFiles,
    },
    CommandEntry {
        name: "Reload Config",
        key_hint: "palette",
        action: || EditorAction::ReloadConfig,
    },
    CommandEntry {
        name: "LSP: Configure Server…",
        key_hint: "Ctrl+L",
        action: || EditorAction::OpenLspConfig,
    },
    CommandEntry {
        name: "LSP: Code Completion",
        key_hint: "Ctrl+Space",
        action: || EditorAction::TriggerCompletion,
    },
    CommandEntry {
        name: "LSP: Hover Info",
        key_hint: "Ctrl+K",
        action: || EditorAction::ShowHover,
    },
    CommandEntry {
        name: "LSP: Go to Definition",
        key_hint: "F12",
        action: || EditorAction::GoToDefinition,
    },
    CommandEntry {
        name: "LSP: Find References",
        key_hint: "Shift+F12",
        action: || EditorAction::FindReferences,
    },
    CommandEntry {
        name: "LSP: Rename Symbol…",
        key_hint: "F2",
        action: || EditorAction::RenameSymbol,
    },
    CommandEntry {
        name: "LSP: Code Action",
        key_hint: "Ctrl+.",
        action: || EditorAction::CodeAction,
    },
    CommandEntry {
        name: "LSP: Restart Server",
        key_hint: "palette",
        action: || EditorAction::LspRestart,
    },
    CommandEntry {
        name: "LSP: Stop Server",
        key_hint: "palette",
        action: || EditorAction::LspStop,
    },
    CommandEntry {
        name: "Quit",
        key_hint: "Ctrl+Q",
        action: || EditorAction::Quit,
    },
];

/// Mutable state for the command palette overlay.
pub struct CommandPaletteState {
    pub query: String,
    /// Indices into `COMMANDS` after filtering, sorted by relevance.
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn new() -> Self {
        let filtered = (0..COMMANDS.len()).collect();
        Self {
            query: String::new(),
            filtered,
            selected: 0,
        }
    }

    /// Re-filter commands against the current query (simple substring match).
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
        if self.query.is_empty() {
            self.filtered = (0..COMMANDS.len()).collect();
            return;
        }
        let q = self.query.to_lowercase();
        self.filtered = COMMANDS
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Return the `EditorAction` for the currently selected command, if any.
    pub fn execute_selected(&self) -> Option<EditorAction> {
        let idx = *self.filtered.get(self.selected)?;
        Some((COMMANDS[idx].action)())
    }
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

pub fn render(palette: &CommandPaletteState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 30 || area.height < 6 {
        return;
    }

    let overlay_w = 60u16.min(area.width);
    let max_rows: u16 = 15;
    let overlay_h = (palette.filtered.len() as u16 + 4)
        .min(max_rows)
        .min(area.height);
    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 3; // upper-third
    let overlay = Rect::new(ox, oy, overlay_w, overlay_h);

    let bg = Color::Rgb(22, 26, 46);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 100, 160));
    let input_style = Style::default().bg(Color::Rgb(32, 36, 60)).fg(Color::White);
    let selected_style = Style::default()
        .bg(Color::Rgb(50, 70, 120))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let normal_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));
    let key_style = Style::default().bg(bg).fg(Color::Rgb(120, 160, 120));
    let selected_key_style = Style::default()
        .bg(Color::Rgb(50, 70, 120))
        .fg(Color::Rgb(160, 220, 160));

    // Background fill.
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // Border.
    draw_border(buf, overlay, border_style);

    // Query input row.
    let input_y = overlay.y + 1;
    let prompt = "> ";
    buf.set_string(overlay.x + 1, input_y, prompt, input_style);
    let qx = overlay.x + 1 + prompt.len() as u16;
    let query_display = format!("{}_", palette.query);
    let avail = overlay.width.saturating_sub(2 + prompt.len() as u16) as usize;
    let qstr = pad_clip(&query_display, avail);
    buf.set_string(qx, input_y, &qstr, input_style);

    // Separator.
    let sep_y = overlay.y + 2;
    for x in overlay.x + 1..overlay.x + overlay.width - 1 {
        buf.set_string(x, sep_y, "─", border_style);
    }

    // Command list.
    let list_y = overlay.y + 3;
    let list_rows = (overlay.height as usize).saturating_sub(4);
    // Scroll so selected is visible.
    let scroll = if palette.selected >= list_rows {
        palette.selected - list_rows + 1
    } else {
        0
    };

    for (screen_row, idx_in_filtered) in
        (scroll..palette.filtered.len()).take(list_rows).enumerate()
    {
        let cmd_idx = palette.filtered[idx_in_filtered];
        let cmd = &COMMANDS[cmd_idx];
        let y = list_y + screen_row as u16;
        if y >= overlay.y + overlay.height - 1 {
            break;
        }

        let is_selected = idx_in_filtered == palette.selected;
        let (row_style, kh_style) = if is_selected {
            (selected_style, selected_key_style)
        } else {
            (normal_style, key_style)
        };

        // Fill row background.
        for x in overlay.x + 1..overlay.x + overlay.width - 1 {
            buf.set_string(x, y, " ", row_style);
        }

        // Name.
        let name_w = overlay.width.saturating_sub(22) as usize;
        let name = pad_clip(cmd.name, name_w);
        buf.set_string(overlay.x + 2, y, &name, row_style);

        // Key hint, right-aligned.
        let kh_w = 18usize;
        let kh = pad_clip(cmd.key_hint, kh_w);
        let kh_x = overlay.x + overlay.width - 1 - kh_w as u16;
        buf.set_string(kh_x, y, &kh, kh_style);
    }
}

fn draw_border(buf: &mut TermBuffer, area: Rect, style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);

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

fn pad_clip(s: &str, width: usize) -> String {
    if s.len() >= width {
        let mut end = width;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    } else {
        format!("{:<width$}", s, width = width)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_shows_all_commands() {
        let state = CommandPaletteState::new();
        assert_eq!(state.filtered.len(), COMMANDS.len());
        assert_eq!(state.selected, 0);
        assert!(state.query.is_empty());
    }

    #[test]
    fn filter_by_query() {
        let mut state = CommandPaletteState::new();
        state.update_query("save".to_string());
        // "Save File", "Save File As" should match.
        assert!(state.filtered.len() >= 2);
        for &idx in &state.filtered {
            assert!(
                COMMANDS[idx].name.to_lowercase().contains("save"),
                "unexpected command: {}",
                COMMANDS[idx].name
            );
        }
    }

    #[test]
    fn filter_case_insensitive() {
        let mut state = CommandPaletteState::new();
        state.update_query("QUIT".to_string());
        assert!(!state.filtered.is_empty());
        let idx = state.filtered[0];
        assert!(COMMANDS[idx].name.to_lowercase().contains("quit"));
    }

    #[test]
    fn empty_query_restores_all() {
        let mut state = CommandPaletteState::new();
        state.update_query("undo".to_string());
        state.update_query(String::new());
        assert_eq!(state.filtered.len(), COMMANDS.len());
    }

    #[test]
    fn move_up_and_down() {
        let mut state = CommandPaletteState::new();
        assert_eq!(state.selected, 0);
        state.move_down();
        assert_eq!(state.selected, 1);
        state.move_up();
        assert_eq!(state.selected, 0);
        state.move_up(); // can't go below 0
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn move_down_clamps_at_end() {
        let mut state = CommandPaletteState::new();
        for _ in 0..1000 {
            state.move_down();
        }
        assert_eq!(state.selected, state.filtered.len() - 1);
    }

    #[test]
    fn execute_selected_returns_action() {
        let state = CommandPaletteState::new();
        // First command is "Save File" → SaveFile action.
        let action = state.execute_selected().unwrap();
        assert_eq!(action, EditorAction::SaveFile);
    }

    #[test]
    fn execute_selected_after_filter() {
        let mut state = CommandPaletteState::new();
        state.update_query("quit".to_string());
        let action = state.execute_selected().unwrap();
        assert_eq!(action, EditorAction::Quit);
    }

    #[test]
    fn all_commands_have_non_empty_names() {
        for cmd in COMMANDS {
            assert!(!cmd.name.is_empty(), "command has empty name");
            assert!(
                !cmd.key_hint.is_empty(),
                "command has empty key hint: {}",
                cmd.name
            );
        }
    }

    #[test]
    fn render_does_not_panic() {
        let state = CommandPaletteState::new();
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = TermBuffer::empty(area);
        render(&state, area, &mut buf);
    }
}
