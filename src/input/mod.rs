pub mod action;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::input::action::{Direction, EditorAction, ScrollDir};

/// Maps raw terminal events to `EditorAction` values.
pub struct InputHandler;

impl InputHandler {
    pub fn new() -> Self {
        Self
    }

    /// Translate a `crossterm` `KeyEvent` into an `EditorAction`.
    pub fn handle_key(&self, event: KeyEvent) -> EditorAction {
        let mods = event.modifiers;
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let shift = mods.contains(KeyModifiers::SHIFT);
        let alt = mods.contains(KeyModifiers::ALT);

        match event.code {
            // ── Function keys ─────────────────────────────────────────
            KeyCode::F(1) => EditorAction::ToggleHelp,
            KeyCode::F(3) if shift => EditorAction::SearchPrev,
            KeyCode::F(3) => EditorAction::SearchNext,
            KeyCode::Esc => EditorAction::CloseSearch,

            // ── Alt combinations ──────────────────────────────────────
            KeyCode::Char('r') if alt && !ctrl => EditorAction::SearchToggleRegex,
            KeyCode::Char('c') if alt && !ctrl => EditorAction::SearchToggleCaseSensitive,
            KeyCode::Char('z') if alt && !ctrl => EditorAction::ToggleWordWrap,

            // ── Printable characters ──────────────────────────────────
            KeyCode::Char(c) if ctrl && !shift && !alt => self.handle_ctrl_char(c),
            KeyCode::Char(c) if ctrl && shift => self.handle_ctrl_shift_char(c),
            KeyCode::Char(c) if !ctrl => EditorAction::InsertChar(c),

            // ── Whitespace ────────────────────────────────────────────
            KeyCode::Enter => EditorAction::InsertNewline,
            KeyCode::Tab => EditorAction::InsertTab,
            KeyCode::BackTab => EditorAction::Unhandled, // Shift+Tab — future dedent

            // ── Deletion ──────────────────────────────────────────────
            KeyCode::Backspace if ctrl => EditorAction::DeleteWordBackward,
            KeyCode::Backspace => EditorAction::DeleteBackward,
            KeyCode::Delete if ctrl => EditorAction::DeleteWordForward,
            KeyCode::Delete => EditorAction::DeleteForward,

            // ── Arrow keys ────────────────────────────────────────────
            KeyCode::Up if shift && alt => EditorAction::SpawnCursorUp,
            KeyCode::Down if shift && alt => EditorAction::SpawnCursorDown,
            KeyCode::Up if alt => EditorAction::MoveLineUp,
            KeyCode::Down if alt => EditorAction::MoveLineDown,
            KeyCode::Up if ctrl && shift => EditorAction::ExtendSelectionPage(Direction::Up),
            KeyCode::Down if ctrl && shift => EditorAction::ExtendSelectionPage(Direction::Down),
            KeyCode::Left if ctrl && shift => EditorAction::ExtendSelectionWord(Direction::Left),
            KeyCode::Right if ctrl && shift => EditorAction::ExtendSelectionWord(Direction::Right),
            KeyCode::Up if shift => EditorAction::ExtendSelection(Direction::Up),
            KeyCode::Down if shift => EditorAction::ExtendSelection(Direction::Down),
            KeyCode::Left if shift => EditorAction::ExtendSelection(Direction::Left),
            KeyCode::Right if shift => EditorAction::ExtendSelection(Direction::Right),
            KeyCode::Up if ctrl => EditorAction::Scroll(ScrollDir::Up),
            KeyCode::Down if ctrl => EditorAction::Scroll(ScrollDir::Down),
            KeyCode::Left if ctrl => EditorAction::MoveCursorWord(Direction::Left),
            KeyCode::Right if ctrl => EditorAction::MoveCursorWord(Direction::Right),
            KeyCode::Up => EditorAction::MoveCursor(Direction::Up),
            KeyCode::Down => EditorAction::MoveCursor(Direction::Down),
            KeyCode::Left => EditorAction::MoveCursor(Direction::Left),
            KeyCode::Right => EditorAction::MoveCursor(Direction::Right),

            // ── Home / End ────────────────────────────────────────────
            KeyCode::Home if ctrl && shift => EditorAction::ExtendSelectionFileStart,
            KeyCode::End if ctrl && shift => EditorAction::ExtendSelectionFileEnd,
            KeyCode::Home if shift => EditorAction::ExtendSelectionHome,
            KeyCode::End if shift => EditorAction::ExtendSelectionEnd,
            KeyCode::Home if ctrl => EditorAction::MoveCursorFileStart,
            KeyCode::End if ctrl => EditorAction::MoveCursorFileEnd,
            KeyCode::Home => EditorAction::MoveCursorHome,
            KeyCode::End => EditorAction::MoveCursorEnd,

            // ── Page Up / Down ────────────────────────────────────────
            KeyCode::PageUp if ctrl => EditorAction::PrevTab,
            KeyCode::PageDown if ctrl => EditorAction::NextTab,
            KeyCode::PageUp if shift => EditorAction::ExtendSelectionPage(Direction::Up),
            KeyCode::PageDown if shift => EditorAction::ExtendSelectionPage(Direction::Down),
            KeyCode::PageUp => EditorAction::MoveCursorPage(Direction::Up),
            KeyCode::PageDown => EditorAction::MoveCursorPage(Direction::Down),

            _ => EditorAction::Unhandled,
        }
    }

    /// Handle Ctrl+<letter> shortcuts.
    fn handle_ctrl_char(&self, c: char) -> EditorAction {
        match c {
            'q' | 'Q' => EditorAction::Quit,
            'z' | 'Z' => EditorAction::Undo,
            'y' | 'Y' => EditorAction::Redo,
            'd' | 'D' => EditorAction::DuplicateLine,
            'a' | 'A' => EditorAction::SelectAll,
            'w' | 'W' => EditorAction::AstExpandSelection,
            'c' | 'C' => EditorAction::Copy,
            'x' | 'X' => EditorAction::Cut,
            // Ctrl+V is handled via a Paste action; the clipboard read happens in app.rs
            // so the action carries the actual text.
            'v' | 'V' => EditorAction::Paste(String::new()), // text filled in by app
            // Phase 5: file / tab management
            's' | 'S' => EditorAction::SaveFile,
            'n' | 'N' => EditorAction::NewFile,
            'o' | 'O' => EditorAction::OpenFile,
            't' | 'T' => EditorAction::NewTab,
            'g' | 'G' => EditorAction::JumpToLine,
            'b' | 'B' => EditorAction::ToggleSidebar,
            'p' | 'P' => EditorAction::OpenFuzzyPicker,
            'f' | 'F' => EditorAction::OpenSearch,
            'h' | 'H' => EditorAction::OpenReplace,
            'l' | 'L' => EditorAction::OpenLspConfig,
            'r' | 'R' => EditorAction::OpenRecentFiles,
            '/' => EditorAction::ToggleLineComment,
            ',' => EditorAction::OpenSettings,
            '[' => EditorAction::PrevTab,
            ']' => EditorAction::NextTab,
            // Ctrl+1..9: switch to tab by index
            '1' => EditorAction::GoToTab(0),
            '2' => EditorAction::GoToTab(1),
            '3' => EditorAction::GoToTab(2),
            '4' => EditorAction::GoToTab(3),
            '5' => EditorAction::GoToTab(4),
            '6' => EditorAction::GoToTab(5),
            '7' => EditorAction::GoToTab(6),
            '8' => EditorAction::GoToTab(7),
            '9' => EditorAction::GoToTab(8),
            _ => EditorAction::Unhandled,
        }
    }

    /// Handle Ctrl+Shift+<letter> shortcuts.
    fn handle_ctrl_shift_char(&self, c: char) -> EditorAction {
        match c.to_ascii_lowercase() {
            'z' => EditorAction::Redo,
            'w' => EditorAction::AstContractSelection,
            's' => EditorAction::SaveFileAs,
            'l' => EditorAction::SelectAllOccurrences,
            'p' => EditorAction::OpenCommandPalette,
            'e' => EditorAction::OpenBufferSwitcher,
            _ => EditorAction::Unhandled,
        }
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

    const IH: InputHandler = InputHandler;

    #[test]
    fn printable_char() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::Char('a'))),
            EditorAction::InsertChar('a')
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::Char('Z'))),
            EditorAction::InsertChar('Z')
        );
    }

    #[test]
    fn enter_and_tab() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::Enter)),
            EditorAction::InsertNewline
        );
        assert_eq!(IH.handle_key(plain(KeyCode::Tab)), EditorAction::InsertTab);
    }

    #[test]
    fn backspace_and_delete() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::Backspace)),
            EditorAction::DeleteBackward
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::Delete)),
            EditorAction::DeleteForward
        );
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Backspace)),
            EditorAction::DeleteWordBackward
        );
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Delete)),
            EditorAction::DeleteWordForward
        );
    }

    #[test]
    fn arrow_keys() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::Up)),
            EditorAction::MoveCursor(Direction::Up)
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::Down)),
            EditorAction::MoveCursor(Direction::Down)
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::Left)),
            EditorAction::MoveCursor(Direction::Left)
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::Right)),
            EditorAction::MoveCursor(Direction::Right)
        );
    }

    #[test]
    fn shift_arrows_extend_selection() {
        assert_eq!(
            IH.handle_key(shift(KeyCode::Right)),
            EditorAction::ExtendSelection(Direction::Right)
        );
        assert_eq!(
            IH.handle_key(shift(KeyCode::Left)),
            EditorAction::ExtendSelection(Direction::Left)
        );
    }

    #[test]
    fn ctrl_arrows_word_move() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Left)),
            EditorAction::MoveCursorWord(Direction::Left)
        );
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Right)),
            EditorAction::MoveCursorWord(Direction::Right)
        );
    }

    #[test]
    fn ctrl_shift_arrows_word_select() {
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Left)),
            EditorAction::ExtendSelectionWord(Direction::Left)
        );
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Right)),
            EditorAction::ExtendSelectionWord(Direction::Right)
        );
    }

    #[test]
    fn home_end() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::Home)),
            EditorAction::MoveCursorHome
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::End)),
            EditorAction::MoveCursorEnd
        );
        assert_eq!(
            IH.handle_key(shift(KeyCode::Home)),
            EditorAction::ExtendSelectionHome
        );
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Home)),
            EditorAction::MoveCursorFileStart
        );
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::End)),
            EditorAction::MoveCursorFileEnd
        );
    }

    #[test]
    fn undo_redo() {
        assert_eq!(IH.handle_key(ctrl(KeyCode::Char('z'))), EditorAction::Undo);
        assert_eq!(IH.handle_key(ctrl(KeyCode::Char('y'))), EditorAction::Redo);
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Char('Z'))),
            EditorAction::Redo
        );
    }

    #[test]
    fn quit() {
        assert_eq!(IH.handle_key(ctrl(KeyCode::Char('q'))), EditorAction::Quit);
    }

    #[test]
    fn move_line_alt_arrows() {
        assert_eq!(IH.handle_key(alt(KeyCode::Up)), EditorAction::MoveLineUp);
        assert_eq!(
            IH.handle_key(alt(KeyCode::Down)),
            EditorAction::MoveLineDown
        );
    }

    #[test]
    fn page_up_down() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::PageUp)),
            EditorAction::MoveCursorPage(Direction::Up)
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::PageDown)),
            EditorAction::MoveCursorPage(Direction::Down)
        );
    }

    #[test]
    fn ast_selection_shortcuts() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('w'))),
            EditorAction::AstExpandSelection
        );
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Char('W'))),
            EditorAction::AstContractSelection
        );
    }

    #[test]
    fn help_and_search_f_keys() {
        assert_eq!(
            IH.handle_key(plain(KeyCode::F(1))),
            EditorAction::ToggleHelp
        );
        assert_eq!(
            IH.handle_key(plain(KeyCode::F(3))),
            EditorAction::SearchNext
        );
        assert_eq!(
            IH.handle_key(shift(KeyCode::F(3))),
            EditorAction::SearchPrev
        );
    }

    #[test]
    fn word_wrap_and_search_toggles() {
        let alt_z = key(KeyCode::Char('z'), KeyModifiers::ALT);
        assert_eq!(IH.handle_key(alt_z), EditorAction::ToggleWordWrap);
        let alt_r = key(KeyCode::Char('r'), KeyModifiers::ALT);
        assert_eq!(IH.handle_key(alt_r), EditorAction::SearchToggleRegex);
        let alt_c = key(KeyCode::Char('c'), KeyModifiers::ALT);
        assert_eq!(
            IH.handle_key(alt_c),
            EditorAction::SearchToggleCaseSensitive
        );
    }

    #[test]
    fn command_palette_and_buffer_switcher() {
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Char('P'))),
            EditorAction::OpenCommandPalette
        );
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Char('E'))),
            EditorAction::OpenBufferSwitcher
        );
    }

    #[test]
    fn toggle_line_comment() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('/'))),
            EditorAction::ToggleLineComment
        );
    }

    #[test]
    fn open_recent_files() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('r'))),
            EditorAction::OpenRecentFiles
        );
    }

    #[test]
    fn open_lsp_config() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('l'))),
            EditorAction::OpenLspConfig
        );
    }

    #[test]
    fn save_file_shortcuts() {
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('s'))),
            EditorAction::SaveFile
        );
        assert_eq!(
            IH.handle_key(ctrl_shift(KeyCode::Char('S'))),
            EditorAction::SaveFileAs
        );
    }

    #[test]
    fn clipboard_shortcuts() {
        assert_eq!(IH.handle_key(ctrl(KeyCode::Char('c'))), EditorAction::Copy);
        assert_eq!(IH.handle_key(ctrl(KeyCode::Char('x'))), EditorAction::Cut);
        // Paste returns Paste("") from the input handler; app fills in the text.
        assert_eq!(
            IH.handle_key(ctrl(KeyCode::Char('v'))),
            EditorAction::Paste(String::new())
        );
    }

    #[test]
    fn mouse_click_and_drag() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            IH.handle_mouse(click),
            EditorAction::MouseClick { col: 10, row: 5 }
        );

        let drag = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 15,
            row: 6,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            IH.handle_mouse(drag),
            EditorAction::MouseDrag { col: 15, row: 6 }
        );
    }

    #[test]
    fn mouse_scroll() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let up = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(IH.handle_mouse(up), EditorAction::Scroll(ScrollDir::Up));
    }

    #[test]
    fn column_edit_spawn_cursor() {
        let alt_shift_up = key(KeyCode::Up, KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(IH.handle_key(alt_shift_up), EditorAction::SpawnCursorUp);

        let alt_shift_down = key(KeyCode::Down, KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(IH.handle_key(alt_shift_down), EditorAction::SpawnCursorDown);
    }
}
