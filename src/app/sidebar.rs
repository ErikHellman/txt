use crate::input::action::{Direction, EditorAction};

use super::state::sidebar::copy_target_path;
use super::{AppState, ConfirmDelete, InputMode, SidebarClipboard};

impl AppState {
    /// Handle input while the sidebar is focused.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    pub(super) fn handle_sidebar_input(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(sb) = &mut self.sidebar {
                    sb.move_up();
                }
                self.ensure_sidebar_selected_visible();
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(sb) = &mut self.sidebar {
                    sb.move_down();
                }
                self.ensure_sidebar_selected_visible();
                true
            }
            EditorAction::InsertNewline => {
                // Enter: open file or expand/collapse directory.
                let selected_path = self
                    .sidebar
                    .as_ref()
                    .and_then(|sb| sb.selected_path().cloned());
                if let Some(path) = selected_path {
                    if path.is_dir() {
                        if let Some(sb) = &mut self.sidebar {
                            sb.toggle_selected();
                        }
                    } else {
                        self.push_current_to_jump_list();
                        let _ = self.editor.open_tab(path);
                        self.after_file_open_or_save();
                        self.sidebar_focused = false;
                    }
                }
                true
            }
            EditorAction::InsertChar(' ') | EditorAction::MoveCursor(Direction::Right) => {
                // Space / Right: open file (stay in sidebar) or expand/collapse directory.
                let entry = self
                    .sidebar
                    .as_ref()
                    .and_then(|sb| sb.entries.get(sb.selected))
                    .map(|e| (e.path.clone(), e.is_dir));
                if let Some((path, is_dir)) = entry {
                    if is_dir {
                        if let Some(sb) = &mut self.sidebar {
                            sb.toggle_selected();
                        }
                    } else {
                        let _ = self.editor.open_tab(path);
                        self.after_file_open_or_save();
                        // intentionally keep sidebar_focused = true
                    }
                }
                true
            }
            EditorAction::MoveCursor(Direction::Left) => {
                // Left: move to parent directory and collapse it.
                if let Some(sb) = &mut self.sidebar {
                    sb.move_to_parent_and_collapse();
                }
                self.ensure_sidebar_selected_visible();
                true
            }
            EditorAction::FocusSidebar => {
                // Ctrl+B while sidebar focused: jump back to editor, sidebar stays open.
                self.sidebar_focused = false;
                true
            }
            EditorAction::CopyFileReference => {
                // Copy just the file path (no cursor location) when in sidebar.
                let selected_path = self
                    .sidebar
                    .as_ref()
                    .and_then(|sb| sb.selected_path().cloned());
                if let Some(path) = selected_path {
                    let reference = path
                        .strip_prefix(&self.workspace)
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    self.clipboard.set(reference);
                }
                true
            }
            EditorAction::CloseSearch => {
                // Esc: return focus to the editor without closing the sidebar.
                self.sidebar_focused = false;
                true
            }
            EditorAction::Copy => {
                // Ctrl+C: copy file path to sidebar clipboard (not root).
                let sel = self.sidebar.as_ref();
                if sel.map(|sb| !sb.root_is_selected()).unwrap_or(false)
                    && let Some(path) = sel.and_then(|sb| sb.selected_path().cloned())
                {
                    self.sidebar_clipboard = Some(SidebarClipboard {
                        path,
                        is_cut: false,
                    });
                }
                true
            }
            EditorAction::Cut => {
                // Ctrl+X: cut file path to sidebar clipboard (not root).
                let sel = self.sidebar.as_ref();
                if sel.map(|sb| !sb.root_is_selected()).unwrap_or(false)
                    && let Some(path) = sel.and_then(|sb| sb.selected_path().cloned())
                {
                    self.sidebar_clipboard = Some(SidebarClipboard { path, is_cut: true });
                }
                true
            }
            EditorAction::Paste(_) => {
                // Ctrl+V: paste (move or copy) the file from sidebar clipboard.
                self.sidebar_paste();
                true
            }
            EditorAction::DeleteForward => {
                // Delete key: delete the selected file/directory (not root).
                let is_root = self
                    .sidebar
                    .as_ref()
                    .map(|sb| sb.root_is_selected())
                    .unwrap_or(true);
                if !is_root
                    && let Some(path) = self
                        .sidebar
                        .as_ref()
                        .and_then(|sb| sb.selected_path().cloned())
                {
                    if path.is_dir() {
                        self.confirm_delete = Some(ConfirmDelete::Dir(path));
                    } else {
                        self.confirm_delete = Some(ConfirmDelete::File(path));
                    }
                }
                true
            }
            EditorAction::RenameSymbol | EditorAction::SidebarRename => {
                // F2: rename the selected file/directory (not root).
                let is_root = self
                    .sidebar
                    .as_ref()
                    .map(|sb| sb.root_is_selected())
                    .unwrap_or(true);
                if !is_root
                    && let Some(path) = self
                        .sidebar
                        .as_ref()
                        .and_then(|sb| sb.selected_path().cloned())
                {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    self.input_mode = InputMode::RenamePath(path, name);
                }
                true
            }
            EditorAction::SidebarNewFolder => {
                // Ctrl+Shift+N: create a new folder in the selected location.
                let parent = self.sidebar.as_ref().and_then(|sb| {
                    sb.entries.get(sb.selected).map(|e| {
                        if e.is_dir {
                            e.path.clone()
                        } else {
                            e.path.parent().unwrap_or(&sb.root).to_path_buf()
                        }
                    })
                });
                if let Some(parent) = parent {
                    self.input_mode = InputMode::NewFolderName(parent, String::new());
                }
                true
            }
            EditorAction::SidebarRefresh => {
                self.refresh_sidebar();
                true
            }
            // Global actions that don't touch editor content are allowed to
            // fall through to the main dispatcher.
            EditorAction::Quit
            | EditorAction::ForceQuit
            | EditorAction::ToggleHelp
            | EditorAction::ToggleSidebar
            | EditorAction::OpenSettings
            | EditorAction::OpenCommandPalette
            | EditorAction::OpenFuzzyPicker
            | EditorAction::OpenRecentFiles
            | EditorAction::OpenLspConfig
            | EditorAction::OpenGitDialog
            | EditorAction::ReloadConfig
            | EditorAction::ToggleWordWrap
            | EditorAction::OpenFile
            | EditorAction::SaveFile
            | EditorAction::SaveFileAs
            | EditorAction::NewFile
            | EditorAction::NewTab
            | EditorAction::CloseTab
            | EditorAction::NextTab
            | EditorAction::PrevTab
            | EditorAction::GoToTab(_)
            | EditorAction::MouseClick { .. }
            | EditorAction::MouseDrag { .. }
            | EditorAction::MouseUp { .. }
            | EditorAction::MouseScroll { .. }
            | EditorAction::Unhandled => false,
            // Swallow everything else so editor content / cursor / search /
            // LSP state isn't affected while the sidebar has focus.
            _ => true,
        }
    }
    /// Paste from the sidebar clipboard into the currently selected location.
    pub(super) fn sidebar_paste(&mut self) {
        let clip = match &self.sidebar_clipboard {
            Some(c) => c,
            None => return,
        };
        let dest_dir = match self.sidebar.as_ref() {
            Some(sb) => match sb.entries.get(sb.selected) {
                Some(entry) if entry.is_dir => entry.path.clone(),
                Some(entry) => entry.path.parent().unwrap_or(&sb.root).to_path_buf(),
                None => return,
            },
            None => return,
        };
        if clip.is_cut {
            // Move: rename source into dest directory with collision check.
            let source = clip.path.clone();
            if let Some(name) = source.file_name() {
                let new_path = dest_dir.join(name);
                if new_path.exists() {
                    return; // Don't overwrite existing files.
                }
                if std::fs::rename(&source, &new_path).is_ok() {
                    // Only consume clipboard on success.
                    self.sidebar_clipboard = None;
                }
            }
        } else {
            // Copy: only files (not directories).
            let source = clip.path.clone();
            if source.is_file()
                && let Some(new_path) = copy_target_path(&source, &dest_dir)
            {
                let _ = std::fs::copy(&source, &new_path);
            }
            // Clipboard is kept so user can paste again.
        }
        self.refresh_sidebar();
    }
    /// Refresh the sidebar entries after a file operation.
    pub(super) fn refresh_sidebar(&mut self) {
        if let Some(sb) = &mut self.sidebar {
            sb.refresh();
        }
    }
}
