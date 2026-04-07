/// Every user-initiated operation is expressed as an `EditorAction`.
///
/// This is a flat enum (not trait objects) so it is zero-allocation,
/// pattern-matchable, and serializable in future phases.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    // ── Text insertion ────────────────────────────────────────────────
    InsertChar(char),
    InsertNewline,
    InsertTab,

    // ── Deletion ──────────────────────────────────────────────────────
    DeleteBackward,
    DeleteForward,
    DeleteWordBackward,
    DeleteWordForward,
    /// Delete from the cursor to the end of the line. If already at the end of
    /// the line, delete the newline character (joining with the next line).
    KillLine,

    // ── Cursor movement ───────────────────────────────────────────────
    MoveCursor(Direction),
    MoveCursorWord(Direction),
    MoveCursorHome,
    MoveCursorEnd,
    MoveCursorFileStart,
    MoveCursorFileEnd,
    MoveCursorPage(Direction),

    // ── Selection (same movements with Shift held) ────────────────────
    ExtendSelection(Direction),
    ExtendSelectionWord(Direction),
    ExtendSelectionHome,
    ExtendSelectionEnd,
    ExtendSelectionFileStart,
    ExtendSelectionFileEnd,
    ExtendSelectionPage(Direction),
    SelectAll,

    // ── Scrolling (without moving the cursor) ─────────────────────────
    Scroll(ScrollDir),

    // ── AST-aware selection (tree-sitter) ────────────────────────────
    /// Ctrl+W: expand selection to the next enclosing AST node.
    AstExpandSelection,
    /// Ctrl+Shift+W: contract selection back to the previous one.
    AstContractSelection,

    // ── Clipboard ────────────────────────────────────────────────────
    Copy,
    Cut,
    /// Paste the given text at the cursor, replacing any selection.
    Paste(String),
    /// Copy a file reference to the clipboard. In the editor, copies
    /// `path:line,col`; when the sidebar is focused, copies only the file path.
    CopyFileReference,

    // ── Edit operations ───────────────────────────────────────────────
    Undo,
    Redo,
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,

    // ── Mouse ─────────────────────────────────────────────────────────
    /// Left-click: place cursor (absolute terminal coordinates).
    MouseClick {
        col: u16,
        row: u16,
    },
    /// Left-button drag: extend selection (absolute terminal coordinates).
    MouseDrag {
        col: u16,
        row: u16,
    },

    // ── Search / replace ─────────────────────────────────────────────
    /// Open the find bar (Ctrl+F).
    OpenSearch,
    /// Open the find+replace bar (Ctrl+H).
    OpenReplace,
    /// Jump to the next search match (F3 / Enter while search bar focused).
    SearchNext,
    /// Jump to the previous search match (Shift+F3).
    SearchPrev,
    /// Close the find/replace bar (Esc).
    CloseSearch,
    /// Replace the current match and advance (Enter in replace field).
    #[allow(dead_code)]
    SearchReplaceOne,
    /// Replace all matches in a single undo batch (Ctrl+A in replace field).
    #[allow(dead_code)]
    SearchReplaceAll,
    /// Toggle regex mode while the search bar is active (Alt+R).
    SearchToggleRegex,
    /// Toggle case-sensitive mode while the search bar is active (Alt+C).
    SearchToggleCaseSensitive,
    /// Select all occurrences of the current selection or search query (Ctrl+Shift+L).
    SelectAllOccurrences,

    // ── File / tab management ─────────────────────────────────────────
    /// Create a new empty buffer in a new tab.
    NewFile,
    /// Open a new empty tab (alias for NewFile).
    NewTab,
    /// Close the active tab.
    CloseTab,
    /// Switch to the next tab (wraps around).
    NextTab,
    /// Switch to the previous tab (wraps around).
    PrevTab,
    /// Switch to a specific tab by 0-based index.
    GoToTab(usize),
    /// Save the active file (prompts Save As if unnamed).
    SaveFile,
    /// Save the active file to a new path.
    SaveFileAs,
    /// Open a file by path (prompts in status bar).
    OpenFile,
    /// Jump to a specific line number (prompts in status bar).
    JumpToLine,
    /// Open the fuzzy file picker overlay (Ctrl+P).
    OpenFuzzyPicker,
    /// Toggle the file tree sidebar visibility (Ctrl+Shift+B).
    ToggleSidebar,
    /// Focus-jump between the editor and the sidebar (Ctrl+B).
    /// Opens the sidebar if it is closed, then focuses it.
    /// If the sidebar is already focused, returns focus to the editor.
    FocusSidebar,

    // ── View / UI toggles ─────────────────────────────────────────────
    /// Open the recent-files picker (Ctrl+R).
    OpenRecentFiles,
    /// Reload the configuration from disk.
    ReloadConfig,
    /// Toggle the help overlay (F1).
    ToggleHelp,
    /// Open the settings overlay (Ctrl+,).
    OpenSettings,
    /// Toggle line comment for the current line(s) (Ctrl+/).
    ToggleLineComment,
    /// Toggle word wrap (Alt+Z).
    ToggleWordWrap,

    // ── Column edit mode (multi-cursor) ──────────────────────────────
    /// Alt+Shift+Up: spawn a cursor on the line above at the same display column.
    SpawnCursorUp,
    /// Alt+Shift+Down: spawn a cursor on the line below at the same display column.
    SpawnCursorDown,

    // ── Command palette / buffer switcher ────────────────────────────
    /// Open the command palette (Ctrl+Shift+P).
    OpenCommandPalette,
    /// Open the open-buffer switcher (Ctrl+Shift+E).
    OpenBufferSwitcher,
    /// Open the LSP server configuration overlay (Ctrl+L).
    OpenLspConfig,

    // ── LSP features ─────────────────────────────────────────────────
    /// Trigger code completion (Ctrl+Space).
    TriggerCompletion,
    /// Show hover info at cursor (Ctrl+K).
    ShowHover,
    /// Go to definition (F12).
    GoToDefinition,
    /// Find references (Shift+F12).
    FindReferences,
    /// Rename symbol (F2).
    RenameSymbol,
    /// Code actions / quick fix (Ctrl+.).
    CodeAction,
    /// Restart the LSP server (command palette).
    #[allow(dead_code)]
    LspRestart,
    /// Stop the LSP server (command palette).
    #[allow(dead_code)]
    LspStop,

    // ── App lifecycle ─────────────────────────────────────────────────
    /// Quit the editor. The app will confirm if there are unsaved changes.
    Quit,
    #[allow(dead_code)]
    ForceQuit,

    // ── Sidebar file operations ───────────────────────────────────────
    /// Rename the selected file/directory in the sidebar (F2).
    #[allow(dead_code)]
    SidebarRename,
    /// Create a new folder in the sidebar (Ctrl+Shift+N).
    SidebarNewFolder,

    // ── Placeholder for unrecognised / unimplemented keys ─────────────
    Unhandled,
}

/// Convert an `EditorAction` to its canonical snake_case name for keybinding config.
///
/// Returns `None` for non-remappable actions (InsertChar, InsertNewline, InsertTab,
/// GoToTab, MouseClick, MouseDrag, Unhandled) and dead-code variants only used
/// internally (SearchReplaceOne, SearchReplaceAll, LspRestart, LspStop, ForceQuit,
/// SidebarRename).
pub fn action_to_name(action: &EditorAction) -> Option<&'static str> {
    Some(match action {
        // Deletion
        EditorAction::DeleteBackward => "delete_backward",
        EditorAction::DeleteForward => "delete_forward",
        EditorAction::DeleteWordBackward => "delete_word_backward",
        EditorAction::DeleteWordForward => "delete_word_forward",
        EditorAction::KillLine => "kill_line",
        // Cursor movement
        EditorAction::MoveCursor(Direction::Up) => "move_cursor_up",
        EditorAction::MoveCursor(Direction::Down) => "move_cursor_down",
        EditorAction::MoveCursor(Direction::Left) => "move_cursor_left",
        EditorAction::MoveCursor(Direction::Right) => "move_cursor_right",
        EditorAction::MoveCursorWord(Direction::Left) => "move_cursor_word_left",
        EditorAction::MoveCursorWord(Direction::Right) => "move_cursor_word_right",
        EditorAction::MoveCursorHome => "move_cursor_home",
        EditorAction::MoveCursorEnd => "move_cursor_end",
        EditorAction::MoveCursorFileStart => "move_cursor_file_start",
        EditorAction::MoveCursorFileEnd => "move_cursor_file_end",
        EditorAction::MoveCursorPage(Direction::Up) => "move_cursor_page_up",
        EditorAction::MoveCursorPage(Direction::Down) => "move_cursor_page_down",
        // Selection
        EditorAction::ExtendSelection(Direction::Up) => "extend_selection_up",
        EditorAction::ExtendSelection(Direction::Down) => "extend_selection_down",
        EditorAction::ExtendSelection(Direction::Left) => "extend_selection_left",
        EditorAction::ExtendSelection(Direction::Right) => "extend_selection_right",
        EditorAction::ExtendSelectionWord(Direction::Left) => "extend_selection_word_left",
        EditorAction::ExtendSelectionWord(Direction::Right) => "extend_selection_word_right",
        EditorAction::ExtendSelectionHome => "extend_selection_home",
        EditorAction::ExtendSelectionEnd => "extend_selection_end",
        EditorAction::ExtendSelectionFileStart => "extend_selection_file_start",
        EditorAction::ExtendSelectionFileEnd => "extend_selection_file_end",
        EditorAction::ExtendSelectionPage(Direction::Up) => "extend_selection_page_up",
        EditorAction::ExtendSelectionPage(Direction::Down) => "extend_selection_page_down",
        EditorAction::SelectAll => "select_all",
        // Scrolling
        EditorAction::Scroll(ScrollDir::Up) => "scroll_up",
        EditorAction::Scroll(ScrollDir::Down) => "scroll_down",
        // AST selection
        EditorAction::AstExpandSelection => "ast_expand_selection",
        EditorAction::AstContractSelection => "ast_contract_selection",
        // Clipboard
        EditorAction::Copy => "copy",
        EditorAction::Cut => "cut",
        EditorAction::Paste(_) => "paste",
        EditorAction::CopyFileReference => "copy_file_reference",
        // Edit operations
        EditorAction::Undo => "undo",
        EditorAction::Redo => "redo",
        EditorAction::DuplicateLine => "duplicate_line",
        EditorAction::MoveLineUp => "move_line_up",
        EditorAction::MoveLineDown => "move_line_down",
        // Search / replace
        EditorAction::OpenSearch => "open_search",
        EditorAction::OpenReplace => "open_replace",
        EditorAction::SearchNext => "search_next",
        EditorAction::SearchPrev => "search_prev",
        EditorAction::CloseSearch => "close_search",
        EditorAction::SearchToggleRegex => "search_toggle_regex",
        EditorAction::SearchToggleCaseSensitive => "search_toggle_case_sensitive",
        EditorAction::SelectAllOccurrences => "select_all_occurrences",
        // File / tab management
        EditorAction::NewFile => "new_file",
        EditorAction::NewTab => "new_tab",
        EditorAction::CloseTab => "close_tab",
        EditorAction::NextTab => "next_tab",
        EditorAction::PrevTab => "prev_tab",
        EditorAction::SaveFile => "save_file",
        EditorAction::SaveFileAs => "save_file_as",
        EditorAction::OpenFile => "open_file",
        EditorAction::JumpToLine => "jump_to_line",
        EditorAction::OpenFuzzyPicker => "open_fuzzy_picker",
        EditorAction::ToggleSidebar => "toggle_sidebar",
        EditorAction::FocusSidebar => "focus_sidebar",
        // View / UI toggles
        EditorAction::OpenRecentFiles => "open_recent_files",
        EditorAction::ReloadConfig => "reload_config",
        EditorAction::ToggleHelp => "toggle_help",
        EditorAction::OpenSettings => "open_settings",
        EditorAction::ToggleLineComment => "toggle_line_comment",
        EditorAction::ToggleWordWrap => "toggle_word_wrap",
        // Multi-cursor
        EditorAction::SpawnCursorUp => "spawn_cursor_up",
        EditorAction::SpawnCursorDown => "spawn_cursor_down",
        // Command palette / buffer switcher
        EditorAction::OpenCommandPalette => "open_command_palette",
        EditorAction::OpenBufferSwitcher => "open_buffer_switcher",
        EditorAction::OpenLspConfig => "open_lsp_config",
        // LSP features
        EditorAction::TriggerCompletion => "trigger_completion",
        EditorAction::ShowHover => "show_hover",
        EditorAction::GoToDefinition => "go_to_definition",
        EditorAction::FindReferences => "find_references",
        EditorAction::RenameSymbol => "rename_symbol",
        EditorAction::CodeAction => "code_action",
        // App lifecycle
        EditorAction::Quit => "quit",
        // Sidebar
        EditorAction::SidebarNewFolder => "sidebar_new_folder",
        // Non-remappable
        _ => return None,
    })
}

/// Convert a snake_case action name back to an `EditorAction`.
///
/// Returns `None` for unknown names.
pub fn action_from_name(name: &str) -> Option<EditorAction> {
    Some(match name {
        // Deletion
        "delete_backward" => EditorAction::DeleteBackward,
        "delete_forward" => EditorAction::DeleteForward,
        "delete_word_backward" => EditorAction::DeleteWordBackward,
        "delete_word_forward" => EditorAction::DeleteWordForward,
        "kill_line" => EditorAction::KillLine,
        // Cursor movement
        "move_cursor_up" => EditorAction::MoveCursor(Direction::Up),
        "move_cursor_down" => EditorAction::MoveCursor(Direction::Down),
        "move_cursor_left" => EditorAction::MoveCursor(Direction::Left),
        "move_cursor_right" => EditorAction::MoveCursor(Direction::Right),
        "move_cursor_word_left" => EditorAction::MoveCursorWord(Direction::Left),
        "move_cursor_word_right" => EditorAction::MoveCursorWord(Direction::Right),
        "move_cursor_home" => EditorAction::MoveCursorHome,
        "move_cursor_end" => EditorAction::MoveCursorEnd,
        "move_cursor_file_start" => EditorAction::MoveCursorFileStart,
        "move_cursor_file_end" => EditorAction::MoveCursorFileEnd,
        "move_cursor_page_up" => EditorAction::MoveCursorPage(Direction::Up),
        "move_cursor_page_down" => EditorAction::MoveCursorPage(Direction::Down),
        // Selection
        "extend_selection_up" => EditorAction::ExtendSelection(Direction::Up),
        "extend_selection_down" => EditorAction::ExtendSelection(Direction::Down),
        "extend_selection_left" => EditorAction::ExtendSelection(Direction::Left),
        "extend_selection_right" => EditorAction::ExtendSelection(Direction::Right),
        "extend_selection_word_left" => EditorAction::ExtendSelectionWord(Direction::Left),
        "extend_selection_word_right" => EditorAction::ExtendSelectionWord(Direction::Right),
        "extend_selection_home" => EditorAction::ExtendSelectionHome,
        "extend_selection_end" => EditorAction::ExtendSelectionEnd,
        "extend_selection_file_start" => EditorAction::ExtendSelectionFileStart,
        "extend_selection_file_end" => EditorAction::ExtendSelectionFileEnd,
        "extend_selection_page_up" => EditorAction::ExtendSelectionPage(Direction::Up),
        "extend_selection_page_down" => EditorAction::ExtendSelectionPage(Direction::Down),
        "select_all" => EditorAction::SelectAll,
        // Scrolling
        "scroll_up" => EditorAction::Scroll(ScrollDir::Up),
        "scroll_down" => EditorAction::Scroll(ScrollDir::Down),
        // AST selection
        "ast_expand_selection" => EditorAction::AstExpandSelection,
        "ast_contract_selection" => EditorAction::AstContractSelection,
        // Clipboard
        "copy" => EditorAction::Copy,
        "cut" => EditorAction::Cut,
        "paste" => EditorAction::Paste(String::new()),
        "copy_file_reference" => EditorAction::CopyFileReference,
        // Edit operations
        "undo" => EditorAction::Undo,
        "redo" => EditorAction::Redo,
        "duplicate_line" => EditorAction::DuplicateLine,
        "move_line_up" => EditorAction::MoveLineUp,
        "move_line_down" => EditorAction::MoveLineDown,
        // Search / replace
        "open_search" => EditorAction::OpenSearch,
        "open_replace" => EditorAction::OpenReplace,
        "search_next" => EditorAction::SearchNext,
        "search_prev" => EditorAction::SearchPrev,
        "close_search" => EditorAction::CloseSearch,
        "search_toggle_regex" => EditorAction::SearchToggleRegex,
        "search_toggle_case_sensitive" => EditorAction::SearchToggleCaseSensitive,
        "select_all_occurrences" => EditorAction::SelectAllOccurrences,
        // File / tab management
        "new_file" => EditorAction::NewFile,
        "new_tab" => EditorAction::NewTab,
        "close_tab" => EditorAction::CloseTab,
        "next_tab" => EditorAction::NextTab,
        "prev_tab" => EditorAction::PrevTab,
        "save_file" => EditorAction::SaveFile,
        "save_file_as" => EditorAction::SaveFileAs,
        "open_file" => EditorAction::OpenFile,
        "jump_to_line" => EditorAction::JumpToLine,
        "open_fuzzy_picker" => EditorAction::OpenFuzzyPicker,
        "toggle_sidebar" => EditorAction::ToggleSidebar,
        "focus_sidebar" => EditorAction::FocusSidebar,
        // View / UI toggles
        "open_recent_files" => EditorAction::OpenRecentFiles,
        "reload_config" => EditorAction::ReloadConfig,
        "toggle_help" => EditorAction::ToggleHelp,
        "open_settings" => EditorAction::OpenSettings,
        "toggle_line_comment" => EditorAction::ToggleLineComment,
        "toggle_word_wrap" => EditorAction::ToggleWordWrap,
        // Multi-cursor
        "spawn_cursor_up" => EditorAction::SpawnCursorUp,
        "spawn_cursor_down" => EditorAction::SpawnCursorDown,
        // Command palette / buffer switcher
        "open_command_palette" => EditorAction::OpenCommandPalette,
        "open_buffer_switcher" => EditorAction::OpenBufferSwitcher,
        "open_lsp_config" => EditorAction::OpenLspConfig,
        // LSP features
        "trigger_completion" => EditorAction::TriggerCompletion,
        "show_hover" => EditorAction::ShowHover,
        "go_to_definition" => EditorAction::GoToDefinition,
        "find_references" => EditorAction::FindReferences,
        "rename_symbol" => EditorAction::RenameSymbol,
        "code_action" => EditorAction::CodeAction,
        // App lifecycle
        "quit" => EditorAction::Quit,
        // Sidebar
        "sidebar_new_folder" => EditorAction::SidebarNewFolder,
        _ => return None,
    })
}

/// Cardinal directions used for cursor and selection movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Directions for viewport scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDir {
    Up,
    Down,
    #[allow(dead_code)]
    Left,
    #[allow(dead_code)]
    Right,
    #[allow(dead_code)]
    HalfPageUp,
    #[allow(dead_code)]
    HalfPageDown,
}
