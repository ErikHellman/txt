use std::path::PathBuf;

use crate::config::{KeymapPreset, Theme};
use crate::input::action::{Direction, EditorAction};
use crate::input::keybinding::KeyBindings;

use super::util::scroll_action;
use super::{AppState, ConfirmDelete, InputMode, LSP_SERVER_OPTIONS};

impl AppState {
    pub(super) fn handle_modal_input(&mut self, action: EditorAction) {
        // Mark prompts auto-complete on the first typed character.
        if let EditorAction::InsertChar(c) = action {
            match self.input_mode {
                InputMode::SetMarkChar => {
                    self.input_mode = InputMode::Normal;
                    let handle = self.editor.active();
                    if let Some(path) = handle.path.clone() {
                        let off = handle.buffer.cursors.primary().byte_offset;
                        self.marks.set(&path, c, off);
                        self.marks.save(&self.workspace);
                    } else {
                        self.status_error = Some("Save the file before setting a mark".into());
                    }
                    return;
                }
                InputMode::JumpToMarkChar => {
                    self.input_mode = InputMode::Normal;
                    let active_path = self.editor.active().path.clone();
                    if let Some(path) = active_path
                        && let Some(off) = self.marks.get(&path, c)
                    {
                        self.push_current_to_jump_list();
                        let handle = self.editor.active_mut();
                        let rope = handle.buffer.rope();
                        let bound = off.min(rope.len_bytes());
                        *handle.buffer.cursors.primary_mut() =
                            crate::buffer::cursor::Cursor::from_byte_offset(rope, bound);
                        handle.buffer.cursors.collapse_to_primary();
                    } else {
                        self.status_error = Some(format!("No mark named {c}"));
                    }
                    return;
                }
                InputMode::RecordMacroChar => {
                    self.input_mode = InputMode::Normal;
                    self.macros.start_recording(c);
                    return;
                }
                InputMode::ReplayMacroChar => {
                    self.input_mode = InputMode::Normal;
                    self.replay_macro_slot(c);
                    return;
                }
                InputMode::SurroundChar => {
                    self.input_mode = InputMode::Normal;
                    self.apply_surround(c);
                    return;
                }
                _ => {}
            }
        }
        // Mutate the input string for typing/backspace without accessing other fields.
        match action {
            EditorAction::InsertChar(c) => {
                // JumpToLine only accepts digits and `:`; every other
                // string-carrying mode accepts any char.
                let allow = !matches!(&self.input_mode, InputMode::JumpToLine(_))
                    || c.is_ascii_digit()
                    || c == ':';
                if allow && let Some(s) = self.input_mode.string_mut() {
                    s.push(c);
                }
                return;
            }
            EditorAction::DeleteBackward => {
                if let Some(s) = self.input_mode.string_mut() {
                    s.pop();
                }
                return;
            }
            _ => {}
        }

        // For Enter / Esc: take ownership of the mode to free the borrow before
        // accessing self.editor or self.input_mode again.
        match action {
            EditorAction::InsertNewline => {
                let mode = std::mem::replace(&mut self.input_mode, InputMode::Normal);
                match mode {
                    InputMode::JumpToLine(input) => {
                        let (line_str, col_str) = match input.split_once(':') {
                            Some((l, c)) => (l, Some(c)),
                            None => (input.as_str(), None),
                        };
                        if let Ok(n) = line_str.parse::<usize>() {
                            self.push_current_to_jump_list();
                            let line = n.saturating_sub(1); // 1-based input
                            let buf = &mut self.editor.active_mut().buffer;
                            let target = {
                                let rope = buf.rope();
                                let clamped_line = line.min(rope.len_lines().saturating_sub(1));
                                let line_start_char = rope.line_to_char(clamped_line);
                                let target_char =
                                    match col_str.and_then(|s| s.parse::<usize>().ok()) {
                                        Some(col_n) => {
                                            let col = col_n.saturating_sub(1); // 1-based → 0-based
                                            let line_char_len = rope.line(clamped_line).len_chars();
                                            // Exclude trailing newline when clamping the column.
                                            let line_content = if line_char_len > 0
                                                && rope.char(line_start_char + line_char_len - 1)
                                                    == '\n'
                                            {
                                                line_char_len - 1
                                            } else {
                                                line_char_len
                                            };
                                            line_start_char + col.min(line_content)
                                        }
                                        None => line_start_char,
                                    };
                                rope.char_to_byte(target_char)
                            };
                            buf.move_cursor_to(target, false);
                        }
                    }
                    InputMode::OpenFilePath(input) => {
                        let path = PathBuf::from(input.trim());
                        self.push_current_to_jump_list();
                        let _ = self.editor.open_tab(path);
                        self.after_file_open_or_save();
                    }
                    InputMode::SaveAsPath(input) => {
                        let path = PathBuf::from(input.trim());
                        let _ = self.editor.active_mut().save_as(path);
                        self.after_file_open_or_save();
                    }
                    InputMode::RenamePath(original, input) => {
                        let new_name = input.trim();
                        // Validate: must be a plain filename (no path separators or ..).
                        let mut components = std::path::Path::new(new_name).components();
                        let is_plain_name = matches!(
                            (components.next(), components.next()),
                            (Some(std::path::Component::Normal(_)), None)
                        );
                        if is_plain_name && let Some(parent) = original.parent() {
                            let new_path = parent.join(new_name);
                            if !new_path.exists() && std::fs::rename(&original, &new_path).is_ok() {
                                self.refresh_sidebar();
                            }
                        }
                    }
                    InputMode::NewFolderName(parent, input) => {
                        let name = input.trim();
                        let mut components = std::path::Path::new(name).components();
                        let is_plain_name = matches!(
                            (components.next(), components.next()),
                            (Some(std::path::Component::Normal(_)), None)
                        );
                        if is_plain_name {
                            let new_dir = parent.join(name);
                            if !new_dir.exists() && std::fs::create_dir(&new_dir).is_ok() {
                                self.refresh_sidebar();
                            }
                        }
                    }
                    InputMode::Rename(input) => {
                        if !input.is_empty() {
                            self.send_rename(&input);
                        }
                    }
                    InputMode::GitCommitMessage(input) => {
                        self.git_finish_commit(&input);
                    }
                    InputMode::GitNewBranch(input) => {
                        self.git_finish_new_branch(&input);
                    }
                    InputMode::GitStashMessage(input) => {
                        self.git_finish_stash_push(&input);
                    }
                    InputMode::ShellFilter(input) => {
                        self.apply_shell_filter(&input);
                    }
                    InputMode::AlignChar(input) => {
                        if let Some(c) = input.chars().next() {
                            self.editor.active_mut().buffer.align_on(c);
                        }
                    }
                    InputMode::Normal
                    | InputMode::SetMarkChar
                    | InputMode::JumpToMarkChar
                    | InputMode::RecordMacroChar
                    | InputMode::ReplayMacroChar
                    | InputMode::SurroundChar => {}
                }
            }
            EditorAction::Quit | EditorAction::Unhandled => {
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
    }
    pub(super) fn handle_fuzzy_picker(&mut self, action: EditorAction) {
        if self.fuzzy_picker.is_none() {
            return;
        }
        match action {
            EditorAction::InsertChar(c) => {
                if let Some(picker) = &mut self.fuzzy_picker {
                    let mut q = picker.query.clone();
                    q.push(c);
                    picker.update_query(q);
                }
            }
            EditorAction::DeleteBackward => {
                if let Some(picker) = &mut self.fuzzy_picker {
                    let mut q = picker.query.clone();
                    q.pop();
                    picker.update_query(q);
                }
            }
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(picker) = &mut self.fuzzy_picker {
                    picker.move_up();
                }
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(picker) = &mut self.fuzzy_picker {
                    picker.move_down();
                }
            }
            EditorAction::InsertNewline => {
                // Extract path before closing picker to avoid borrow conflict with self.editor.
                let path = self
                    .fuzzy_picker
                    .as_ref()
                    .and_then(|p| p.selected_path().cloned());
                self.fuzzy_picker = None;
                if let Some(path) = path {
                    self.push_current_to_jump_list();
                    let _ = self.editor.open_tab(path);
                    self.after_file_open_or_save();
                }
            }
            EditorAction::Quit | EditorAction::CloseSearch | EditorAction::Unhandled => {
                self.fuzzy_picker = None;
            }
            _ => {}
        }
    }
    pub(super) fn handle_symbol_picker(&mut self, action: EditorAction) {
        if self.symbol_picker.is_none() {
            return;
        }
        match action {
            EditorAction::InsertChar(c) => {
                if let Some(picker) = &mut self.symbol_picker {
                    let mut q = picker.query.clone();
                    q.push(c);
                    picker.update_query(q);
                }
            }
            EditorAction::DeleteBackward => {
                if let Some(picker) = &mut self.symbol_picker {
                    let mut q = picker.query.clone();
                    q.pop();
                    picker.update_query(q);
                }
            }
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(picker) = &mut self.symbol_picker {
                    picker.move_up();
                }
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(picker) = &mut self.symbol_picker {
                    picker.move_down();
                }
            }
            EditorAction::InsertNewline => {
                let target = self
                    .symbol_picker
                    .as_ref()
                    .and_then(|p| p.selected_symbol().cloned());
                self.symbol_picker = None;
                if let Some(sym) = target {
                    self.push_current_to_jump_list();
                    let handle = self.editor.active_mut();
                    let rope = handle.buffer.rope();
                    let bound = sym.byte_range.start.min(rope.len_bytes());
                    *handle.buffer.cursors.primary_mut() =
                        crate::buffer::cursor::Cursor::from_byte_offset(rope, bound);
                    handle.buffer.cursors.collapse_to_primary();
                }
            }
            EditorAction::Quit | EditorAction::CloseSearch | EditorAction::Unhandled => {
                self.symbol_picker = None;
            }
            _ => {}
        }
    }
    /// Handle input while the welcome overlay is visible. Any "OK" key
    /// (Enter / Esc / F1 / Ctrl+Q-style ToggleHelp) dismisses it; arrows and
    /// the mouse wheel scroll its content. Returns `true` to consume.
    pub(super) fn handle_welcome(&mut self, action: &EditorAction) -> bool {
        if scroll_action(action, &mut self.welcome_scroll) {
            return true;
        }
        match action {
            EditorAction::InsertNewline | EditorAction::CloseSearch => {
                self.show_welcome = false;
                self.record_version_seen();
                true
            }
            // Don't let typing leak through to the editor while the modal is up.
            EditorAction::InsertChar(_) | EditorAction::InsertTab => true,
            _ => false,
        }
    }
    /// Handle input while the changelog overlay is visible. Same dismiss /
    /// scroll semantics as the welcome overlay.
    pub(super) fn handle_changelog(&mut self, action: &EditorAction) -> bool {
        if scroll_action(action, &mut self.changelog_scroll) {
            return true;
        }
        match action {
            EditorAction::InsertNewline | EditorAction::CloseSearch => {
                self.show_changelog = false;
                self.record_version_seen();
                true
            }
            EditorAction::InsertChar(_) | EditorAction::InsertTab => true,
            _ => false,
        }
    }
    /// Handle input while the help overlay is visible.
    /// Returns `true` if the action was consumed (caller should `return`).
    pub(super) fn handle_help(&mut self, action: &EditorAction) -> bool {
        if scroll_action(action, &mut self.help_scroll) {
            return true;
        }
        match action {
            EditorAction::MoveCursorFileStart => {
                self.help_scroll = 0;
                true
            }
            EditorAction::MoveCursorFileEnd => {
                self.help_scroll = usize::MAX; // clamped in render
                true
            }
            EditorAction::ToggleHelp | EditorAction::CloseSearch => {
                self.show_help = false;
                true
            }
            // Swallow all text-insertion actions so they don't reach the editor.
            EditorAction::InsertChar(_) | EditorAction::InsertNewline | EditorAction::InsertTab => {
                true
            }
            _ => false,
        }
    }
    /// Handle input while the settings overlay is open.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    pub(super) fn handle_settings(&mut self, action: &EditorAction) -> bool {
        const NUM_ROWS: usize = crate::ui::settings_overlay::NUM_SETTINGS;
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                self.settings_cursor = self.settings_cursor.saturating_sub(1);
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                self.settings_cursor = (self.settings_cursor + 1).min(NUM_ROWS - 1);
                true
            }
            EditorAction::InsertChar(' ') | EditorAction::InsertNewline => {
                self.toggle_setting(true);
                true
            }
            EditorAction::MoveCursor(Direction::Right) => {
                self.toggle_setting(true);
                true
            }
            EditorAction::MoveCursor(Direction::Left) => {
                self.toggle_setting(false);
                true
            }
            // Let Quit / Escape close the overlay but fall through so Quit still quits.
            EditorAction::OpenSettings | EditorAction::CloseSearch => {
                self.show_settings = false;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.show_settings = false;
                false
            }
            // Swallow all text-insertion actions so they don't reach the editor.
            EditorAction::InsertChar(_) | EditorAction::InsertTab => true,
            _ => false,
        }
    }
    /// Toggle or cycle the setting at `settings_cursor`. `forward` controls
    /// direction for enum settings; booleans always flip.
    pub(super) fn toggle_setting(&mut self, forward: bool) {
        match self.settings_cursor {
            0 => self.config.confirm_exit = !self.config.confirm_exit,
            1 => self.config.auto_save = !self.config.auto_save,
            2 => self.config.show_whitespace = !self.config.show_whitespace,
            3 => {
                self.config.highlight_trailing_whitespace =
                    !self.config.highlight_trailing_whitespace;
            }
            4 => self.config.warn_mixed_indent = !self.config.warn_mixed_indent,
            5 => self.config.auto_pair = !self.config.auto_pair,
            6 => self.config.hide_git_folder = !self.config.hide_git_folder,
            7 => self.config.hide_dot_folders = !self.config.hide_dot_folders,
            8 => self.config.restore_session = !self.config.restore_session,
            9 => self.config.persistent_undo = !self.config.persistent_undo,
            10 => {
                let all = Theme::ALL;
                let idx = all
                    .iter()
                    .position(|t| t == &self.config.theme)
                    .unwrap_or(0);
                let next = if forward {
                    (idx + 1) % all.len()
                } else {
                    (idx + all.len() - 1) % all.len()
                };
                self.config.theme = all[next].clone();
            }
            11 => {
                let all = KeymapPreset::ALL;
                let idx = all
                    .iter()
                    .position(|p| p == &self.config.keymap_preset)
                    .unwrap_or(0);
                let next = if forward {
                    (idx + 1) % all.len()
                } else {
                    (idx + all.len() - 1) % all.len()
                };
                self.config.keymap_preset = all[next].clone();
                KeyBindings::apply_preset(&self.config.keymap_preset);
                self.input.reload_keybindings();
            }
            _ => {}
        }
        self.config.save();
    }
    /// Handle input while the LSP config picker is open.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    pub(super) fn handle_lsp_picker(&mut self, action: &EditorAction) -> bool {
        let num_rows = 1 + LSP_SERVER_OPTIONS.len();
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(picker) = &mut self.lsp_picker {
                    picker.selected = picker.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(picker) = &mut self.lsp_picker {
                    picker.selected = (picker.selected + 1).min(num_rows - 1);
                }
                true
            }
            EditorAction::InsertChar(' ') | EditorAction::InsertNewline => {
                self.apply_lsp_picker_selection();
                self.lsp_picker = None;
                true
            }
            EditorAction::OpenLspConfig | EditorAction::CloseSearch => {
                self.lsp_picker = None;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.lsp_picker = None;
                false
            }
            _ => false,
        }
    }
    /// Write the selected LSP config to `<workspace>/.txt/lsp.toml` and reload.
    pub(super) fn apply_lsp_picker_selection(&mut self) {
        let selected = match &self.lsp_picker {
            Some(p) => p.selected,
            None => return,
        };

        use crate::lsp::config::{LspServerEntry, WorkspaceLspConfig};
        use std::collections::HashMap;

        let new_config = if selected == 0 {
            // Disabled
            WorkspaceLspConfig::default()
        } else {
            let (_language, name, command, args) = LSP_SERVER_OPTIONS[selected - 1];
            let mut servers = HashMap::new();
            servers.insert(
                name.to_string(),
                LspServerEntry {
                    command: command.to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                    init_options: None,
                },
            );
            WorkspaceLspConfig {
                enabled: true,
                server: Some(name.to_string()),
                servers,
            }
        };

        // Write config file.
        let txt_dir = self.workspace.join(".txt");
        let _ = std::fs::create_dir_all(&txt_dir);
        if let Ok(text) = toml::to_string(&new_config) {
            let _ = std::fs::write(txt_dir.join("lsp.toml"), text);
        }

        // Tear down existing LSP connection if any.
        self.lsp = None;
        self.pending_lsp_approval = None;

        // Apply new config.
        self.lsp_config = new_config;

        // Start new server if enabled — routed through the trust gate.
        self.request_lsp_start();
    }
    /// Handle input while a delete confirmation is active.
    pub(super) fn handle_confirm_delete(&mut self, action: EditorAction) {
        let state = self.confirm_delete.take();
        match state {
            Some(ConfirmDelete::File(path)) => match action {
                EditorAction::InsertChar('y') | EditorAction::InsertChar('Y') => {
                    let _ = std::fs::remove_file(&path);
                    self.refresh_sidebar();
                }
                _ => {} // Any other key cancels.
            },
            Some(ConfirmDelete::Dir(path)) => match action {
                EditorAction::InsertChar('y') | EditorAction::InsertChar('Y') => {
                    // Move to second confirmation step.
                    self.confirm_delete = Some(ConfirmDelete::DirConfirmed(path));
                }
                _ => {} // Any other key cancels.
            },
            Some(ConfirmDelete::DirConfirmed(path)) if action == EditorAction::InsertNewline => {
                let _ = std::fs::remove_dir_all(&path);
                self.refresh_sidebar();
            }
            Some(ConfirmDelete::DirConfirmed(_)) | None => {}
        }
    }
    pub(super) fn handle_command_palette(&mut self, action: EditorAction) {
        match action {
            EditorAction::InsertChar(c) => {
                if let Some(p) = &mut self.command_palette {
                    let mut q = p.query.clone();
                    q.push(c);
                    p.update_query(q);
                }
            }
            EditorAction::DeleteBackward => {
                if let Some(p) = &mut self.command_palette {
                    let mut q = p.query.clone();
                    q.pop();
                    p.update_query(q);
                }
            }
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(p) = &mut self.command_palette {
                    p.move_up();
                }
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(p) = &mut self.command_palette {
                    p.move_down();
                }
            }
            EditorAction::InsertNewline => {
                // Execute the selected command.
                let dispatched = self
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.execute_selected());
                self.command_palette = None;
                if let Some(action) = dispatched {
                    // Guard: don't re-open palette from itself.
                    if !matches!(action, EditorAction::OpenCommandPalette) {
                        let th = self.term_height;
                        self.update(action, th);
                    }
                }
            }
            EditorAction::Quit | EditorAction::Unhandled | EditorAction::CloseSearch => {
                self.command_palette = None;
            }
            _ => {}
        }
    }
}
