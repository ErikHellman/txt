pub mod action;
pub mod keybinding;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::input::action::{EditorAction, ScrollDir};
use crate::input::keybinding::{KeyBindings, KeyCombo};

/// Maps raw terminal events to `EditorAction` values.
///
/// Uses a configurable `KeyBindings` map loaded from
/// `~/.config/txt/keybindings.toml`.  Non-remappable actions (character
/// insertion, tab-by-index) are checked first via hardcoded logic.
pub struct InputHandler {
    bindings: KeyBindings,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            bindings: KeyBindings::load(),
        }
    }

    /// Build an `InputHandler` with explicit keybindings (for testing).
    #[cfg(test)]
    pub fn with_bindings(bindings: KeyBindings) -> Self {
        Self { bindings }
    }

    /// Reload keybindings from disk (called on `ReloadConfig`).
    pub fn reload_keybindings(&mut self) {
        self.bindings = KeyBindings::load();
    }

    /// Access the keybindings (e.g. for the help overlay).
    pub fn keybindings(&self) -> &KeyBindings {
        &self.bindings
    }

    /// Translate a `crossterm` `KeyEvent` into an `EditorAction`.
    pub fn handle_key(&self, event: KeyEvent) -> EditorAction {
        let mods = event.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let shift = mods.contains(KeyModifiers::SHIFT);
        let alt = mods.contains(KeyModifiers::ALT);

        // ── Phase 1: non-remappable hardcoded actions ────────────────────

        // Ctrl+1..9 → GoToTab (index-parameterized, not configurable)
        if ctrl
            && !shift
            && !alt
            && let KeyCode::Char(c) = event.code
            && let Some(n) = c.to_digit(10)
            && (1..=9).contains(&n)
        {
            return EditorAction::GoToTab(n as usize - 1);
        }

        // Plain printable chars (no ctrl/alt) → InsertChar
        if let KeyCode::Char(c) = event.code
            && !ctrl
            && !alt
        {
            return EditorAction::InsertChar(c);
        }

        // Enter / Tab / BackTab (without modifiers affecting them)
        match event.code {
            KeyCode::Enter if !ctrl && !alt => return EditorAction::InsertNewline,
            KeyCode::Tab if !ctrl && !alt && !shift => return EditorAction::InsertTab,
            KeyCode::BackTab => return EditorAction::Unhandled,
            _ => {}
        }

        // ── Phase 2: configurable keybinding lookup ──────────────────────
        let combo = KeyCombo::from_key_event(&event);
        if let Some(action) = self.bindings.lookup(&combo) {
            return action.clone();
        }

        EditorAction::Unhandled
    }

    /// Translate a mouse event into an `EditorAction`.
    pub fn handle_mouse(&self, event: MouseEvent) -> EditorAction {
        let col = event.column;
        let row = event.row;
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => EditorAction::MouseClick { col, row },
            MouseEventKind::Drag(MouseButton::Left) => EditorAction::MouseDrag { col, row },
            MouseEventKind::ScrollUp => EditorAction::Scroll(ScrollDir::Up),
            MouseEventKind::ScrollDown => EditorAction::Scroll(ScrollDir::Down),
            _ => EditorAction::Unhandled,
        }
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::action::Direction;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn plain(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::CONTROL)
    }

    fn shift(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::SHIFT)
    }

    fn ctrl_shift(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::ALT)
    }

    fn handler() -> InputHandler {
        InputHandler::with_bindings(KeyBindings::defaults())
    }

    #[test]
    fn printable_char() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::Char('a'))),
            EditorAction::InsertChar('a')
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::Char('Z'))),
            EditorAction::InsertChar('Z')
        );
    }

    #[test]
    fn enter_and_tab() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::Enter)),
            EditorAction::InsertNewline
        );
        assert_eq!(ih.handle_key(plain(KeyCode::Tab)), EditorAction::InsertTab);
    }

    #[test]
    fn backspace_and_delete() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::Backspace)),
            EditorAction::DeleteBackward
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::Delete)),
            EditorAction::DeleteForward
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Backspace)),
            EditorAction::DeleteWordBackward
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Delete)),
            EditorAction::DeleteWordForward
        );
    }

    #[test]
    fn arrow_keys() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::Up)),
            EditorAction::MoveCursor(Direction::Up)
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::Down)),
            EditorAction::MoveCursor(Direction::Down)
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::Left)),
            EditorAction::MoveCursor(Direction::Left)
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::Right)),
            EditorAction::MoveCursor(Direction::Right)
        );
    }

    #[test]
    fn shift_arrows_extend_selection() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(shift(KeyCode::Right)),
            EditorAction::ExtendSelection(Direction::Right)
        );
        assert_eq!(
            ih.handle_key(shift(KeyCode::Left)),
            EditorAction::ExtendSelection(Direction::Left)
        );
    }

    #[test]
    fn ctrl_arrows_word_move() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Left)),
            EditorAction::MoveCursorWord(Direction::Left)
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Right)),
            EditorAction::MoveCursorWord(Direction::Right)
        );
    }

    #[test]
    fn ctrl_shift_arrows_word_select() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Left)),
            EditorAction::ExtendSelectionWord(Direction::Left)
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Right)),
            EditorAction::ExtendSelectionWord(Direction::Right)
        );
    }

    #[test]
    fn home_end() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::Home)),
            EditorAction::MoveCursorHome
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::End)),
            EditorAction::MoveCursorEnd
        );
        assert_eq!(
            ih.handle_key(shift(KeyCode::Home)),
            EditorAction::ExtendSelectionHome
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Home)),
            EditorAction::MoveCursorFileStart
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::End)),
            EditorAction::MoveCursorFileEnd
        );
    }

    #[test]
    fn undo_redo() {
        let ih = handler();
        assert_eq!(ih.handle_key(ctrl(KeyCode::Char('z'))), EditorAction::Undo);
        assert_eq!(ih.handle_key(ctrl(KeyCode::Char('y'))), EditorAction::Redo);
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('Z'))),
            EditorAction::Redo
        );
    }

    #[test]
    fn quit() {
        let ih = handler();
        assert_eq!(ih.handle_key(ctrl(KeyCode::Char('q'))), EditorAction::Quit);
    }

    #[test]
    fn move_line_alt_arrows() {
        let ih = handler();
        assert_eq!(ih.handle_key(alt(KeyCode::Up)), EditorAction::MoveLineUp);
        assert_eq!(
            ih.handle_key(alt(KeyCode::Down)),
            EditorAction::MoveLineDown
        );
    }

    #[test]
    fn page_up_down() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::PageUp)),
            EditorAction::MoveCursorPage(Direction::Up)
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::PageDown)),
            EditorAction::MoveCursorPage(Direction::Down)
        );
    }

    #[test]
    fn ast_selection_shortcuts() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('w'))),
            EditorAction::AstExpandSelection
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('W'))),
            EditorAction::AstContractSelection
        );
    }

    #[test]
    fn help_and_search_f_keys() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(plain(KeyCode::F(1))),
            EditorAction::ToggleHelp
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::F(3))),
            EditorAction::SearchNext
        );
        assert_eq!(
            ih.handle_key(shift(KeyCode::F(3))),
            EditorAction::SearchPrev
        );
        assert_eq!(
            ih.handle_key(plain(KeyCode::F(2))),
            EditorAction::RenameSymbol
        );
    }

    #[test]
    fn word_wrap_and_search_toggles() {
        let ih = handler();
        let alt_z = key(KeyCode::Char('z'), KeyModifiers::ALT);
        assert_eq!(ih.handle_key(alt_z), EditorAction::ToggleWordWrap);
        let alt_r = key(KeyCode::Char('r'), KeyModifiers::ALT);
        assert_eq!(ih.handle_key(alt_r), EditorAction::SearchToggleRegex);
        let alt_c = key(KeyCode::Char('c'), KeyModifiers::ALT);
        assert_eq!(
            ih.handle_key(alt_c),
            EditorAction::SearchToggleCaseSensitive
        );
    }

    #[test]
    fn command_palette_and_buffer_switcher() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('P'))),
            EditorAction::OpenCommandPalette
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('E'))),
            EditorAction::OpenBufferSwitcher
        );
    }

    #[test]
    fn toggle_line_comment() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('/'))),
            EditorAction::ToggleLineComment
        );
    }

    #[test]
    fn open_recent_files() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('r'))),
            EditorAction::OpenRecentFiles
        );
    }

    #[test]
    fn open_lsp_config() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('l'))),
            EditorAction::OpenLspConfig
        );
    }

    #[test]
    fn sidebar_shortcuts() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('b'))),
            EditorAction::FocusSidebar
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('B'))),
            EditorAction::ToggleSidebar
        );
    }

    #[test]
    fn save_file_shortcuts() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('s'))),
            EditorAction::SaveFile
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('S'))),
            EditorAction::SaveFileAs
        );
    }

    #[test]
    fn clipboard_shortcuts() {
        let ih = handler();
        assert_eq!(ih.handle_key(ctrl(KeyCode::Char('c'))), EditorAction::Copy);
        assert_eq!(ih.handle_key(ctrl(KeyCode::Char('x'))), EditorAction::Cut);
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('v'))),
            EditorAction::Paste(String::new())
        );
        assert_eq!(
            ih.handle_key(ctrl_shift(KeyCode::Char('C'))),
            EditorAction::CopyFileReference
        );
    }

    #[test]
    fn mouse_click_and_drag() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let ih = handler();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            ih.handle_mouse(click),
            EditorAction::MouseClick { col: 10, row: 5 }
        );

        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 15,
            row: 6,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            ih.handle_mouse(drag),
            EditorAction::MouseDrag { col: 15, row: 6 }
        );
    }

    #[test]
    fn mouse_scroll() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let ih = handler();
        let up = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(ih.handle_mouse(up), EditorAction::Scroll(ScrollDir::Up));
    }

    #[test]
    fn column_edit_spawn_cursor() {
        let ih = handler();
        let alt_shift_up = key(KeyCode::Up, KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(ih.handle_key(alt_shift_up), EditorAction::SpawnCursorUp);

        let alt_shift_down = key(KeyCode::Down, KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(ih.handle_key(alt_shift_down), EditorAction::SpawnCursorDown);
    }

    #[test]
    fn go_to_tab_shortcuts() {
        let ih = handler();
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('1'))),
            EditorAction::GoToTab(0)
        );
        assert_eq!(
            ih.handle_key(ctrl(KeyCode::Char('9'))),
            EditorAction::GoToTab(8)
        );
    }
}
