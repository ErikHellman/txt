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
    /// Indent every line touched by the selection (or the current line) by
    /// one indent level. Bound to Tab when there is a multi-line selection,
    /// and exposed for keymap remapping under the name `indent_selection`.
    IndentSelection,
    /// Dedent every line touched by the selection (or the current line) by
    /// one indent level. Bound to Shift+Tab.
    DedentSelection,
    /// Run the configured external formatter for the active buffer's
    /// language and replace the buffer contents in a single undo entry.
    /// Default keybinding: Ctrl+Shift+I.
    FormatBuffer,

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
    /// Re-center the viewport so the primary cursor is on the middle row.
    /// Does not move the cursor.
    ScrollCursorCenter,

    // ── AST-aware selection (tree-sitter) ────────────────────────────
    /// Ctrl+W: expand selection to the next enclosing AST node.
    AstExpandSelection,
    /// Ctrl+Shift+W: contract selection back to the previous one.
    AstContractSelection,
    /// Jump the primary cursor to the matching bracket (`{}`, `()`, `[]`).
    /// No-op when the cursor is not on a bracket character.
    GoToMatchingBracket,

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
    /// Left-button release. Used to terminate drags (e.g. sidebar resize).
    MouseUp {
        col: u16,
        row: u16,
    },
    /// Mouse-wheel scroll. Carries the cursor coordinates so the handler can
    /// route to the sidebar or editor based on where the wheel was rolled.
    MouseScroll {
        dir: ScrollDir,
        col: u16,
        row: u16,
    },
    /// Alt+left-click: start a box / column selection at the click position.
    BoxDragStart {
        col: u16,
        row: u16,
    },
    /// Alt+left-drag: extend the box selection to the new position.
    BoxDragUpdate {
        col: u16,
        row: u16,
    },
    /// Alt+left-release: finish a box-select drag.
    BoxDragEnd {
        col: u16,
        row: u16,
    },
    /// Ctrl+Alt+arrow: extend the rectangular selection by one cell in `dir`.
    BoxSelectExtend(Direction),
    /// Ctrl+Alt+\: open a status-bar prompt; on Enter, run the command via
    /// `sh -c` with the current selection on stdin and replace the selection
    /// with the captured stdout.
    FilterSelection,

    // ── Line transforms (1.5) ─────────────────────────────────────────
    SortLinesAsc,
    SortLinesDesc,
    DedupeLines,
    ReverseLines,
    ToUpper,
    ToLower,
    ToTitle,
    TrimTrailingWhitespace,
    JoinLines,
    IncrementNumber,
    DecrementNumber,
    ConvertIndentToSpaces,
    ConvertIndentToTabs,
    ConvertEolLf,
    ConvertEolCrlf,
    /// Open a status-bar prompt asking for an alignment character.
    AlignSelection,

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
    /// Open the project-wide search/replace overlay (Ctrl+Shift+F).
    OpenProjectSearch,
    /// Sublime/VS Code "Ctrl+D": expand to surrounding word, or add a cursor
    /// at the next occurrence of the current selection.
    AddCursorNextMatch,
    /// Like `AddCursorNextMatch` but skip the current match instead of
    /// adding to it.
    SkipCurrentMatch,
    /// Undo the most recent `AddCursorNextMatch` (Ctrl+U).
    UndoLastCursor,

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
    /// Open the symbols-in-file picker overlay (Ctrl+Shift+O).
    OpenSymbolPicker,
    /// Toggle the fold at the cursor's line (Ctrl+Shift+[).
    ToggleFoldAtCursor,
    /// Fold every candidate region in the active buffer.
    FoldAll,
    /// Unfold every region in the active buffer.
    UnfoldAll,
    /// Walk one step backward in the workspace-wide jump list (Alt+Left).
    JumpListBack,
    /// Walk one step forward in the workspace-wide jump list (Alt+Right).
    JumpListForward,
    /// Begin a mark prompt; the next typed character (a–z) names the mark.
    BeginSetMark,
    /// Begin a jump-to-mark prompt; the next typed character (a–z) selects.
    BeginJumpToMark,
    /// Look up a snippet whose prefix matches the word before the cursor and
    /// expand it in-place. Bound to Ctrl+J by default.
    ExpandSnippetAtCursor,
    /// While a snippet session is active, advance to the next tab stop.
    SnippetNextStop,
    /// While a snippet session is active, walk back to the previous stop.
    SnippetPrevStop,
    /// Cancel the active snippet session (Esc).
    SnippetCancel,
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
    /// Open the git operations dialog (Ctrl+Shift+G).
    OpenGitDialog,

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
    /// Refresh the sidebar file tree (F5).
    SidebarRefresh,

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
        EditorAction::ScrollCursorCenter => "scroll_cursor_center",
        // AST selection
        EditorAction::AstExpandSelection => "ast_expand_selection",
        EditorAction::AstContractSelection => "ast_contract_selection",
        EditorAction::GoToMatchingBracket => "go_to_matching_bracket",
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
        EditorAction::IndentSelection => "indent_selection",
        EditorAction::DedentSelection => "dedent_selection",
        EditorAction::FormatBuffer => "format_buffer",
        // Search / replace
        EditorAction::OpenSearch => "open_search",
        EditorAction::OpenReplace => "open_replace",
        EditorAction::SearchNext => "search_next",
        EditorAction::SearchPrev => "search_prev",
        EditorAction::CloseSearch => "close_search",
        EditorAction::SearchToggleRegex => "search_toggle_regex",
        EditorAction::SearchToggleCaseSensitive => "search_toggle_case_sensitive",
        EditorAction::SelectAllOccurrences => "select_all_occurrences",
        EditorAction::OpenProjectSearch => "open_project_search",
        EditorAction::AddCursorNextMatch => "add_cursor_next_match",
        EditorAction::SkipCurrentMatch => "skip_current_match",
        EditorAction::UndoLastCursor => "undo_last_cursor",
        EditorAction::BoxSelectExtend(Direction::Up) => "box_select_extend_up",
        EditorAction::BoxSelectExtend(Direction::Down) => "box_select_extend_down",
        EditorAction::BoxSelectExtend(Direction::Left) => "box_select_extend_left",
        EditorAction::BoxSelectExtend(Direction::Right) => "box_select_extend_right",
        EditorAction::FilterSelection => "filter_selection",
        EditorAction::SortLinesAsc => "sort_lines_asc",
        EditorAction::SortLinesDesc => "sort_lines_desc",
        EditorAction::DedupeLines => "dedupe_lines",
        EditorAction::ReverseLines => "reverse_lines",
        EditorAction::ToUpper => "to_upper",
        EditorAction::ToLower => "to_lower",
        EditorAction::ToTitle => "to_title",
        EditorAction::TrimTrailingWhitespace => "trim_trailing_whitespace",
        EditorAction::JoinLines => "join_lines",
        EditorAction::IncrementNumber => "increment_number",
        EditorAction::DecrementNumber => "decrement_number",
        EditorAction::ConvertIndentToSpaces => "convert_indent_to_spaces",
        EditorAction::ConvertIndentToTabs => "convert_indent_to_tabs",
        EditorAction::ConvertEolLf => "convert_eol_lf",
        EditorAction::ConvertEolCrlf => "convert_eol_crlf",
        EditorAction::AlignSelection => "align_selection",
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
        EditorAction::OpenSymbolPicker => "open_symbol_picker",
        EditorAction::ToggleFoldAtCursor => "toggle_fold_at_cursor",
        EditorAction::FoldAll => "fold_all",
        EditorAction::UnfoldAll => "unfold_all",
        EditorAction::JumpListBack => "jump_list_back",
        EditorAction::JumpListForward => "jump_list_forward",
        EditorAction::BeginSetMark => "set_mark",
        EditorAction::BeginJumpToMark => "jump_to_mark",
        EditorAction::ExpandSnippetAtCursor => "expand_snippet",
        EditorAction::SnippetNextStop => "snippet_next_stop",
        EditorAction::SnippetPrevStop => "snippet_prev_stop",
        EditorAction::SnippetCancel => "snippet_cancel",
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
        EditorAction::OpenGitDialog => "open_git_dialog",
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
        EditorAction::SidebarRefresh => "sidebar_refresh",
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
        "scroll_cursor_center" => EditorAction::ScrollCursorCenter,
        // AST selection
        "ast_expand_selection" => EditorAction::AstExpandSelection,
        "ast_contract_selection" => EditorAction::AstContractSelection,
        "go_to_matching_bracket" => EditorAction::GoToMatchingBracket,
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
        "indent_selection" => EditorAction::IndentSelection,
        "dedent_selection" => EditorAction::DedentSelection,
        "format_buffer" => EditorAction::FormatBuffer,
        // Search / replace
        "open_search" => EditorAction::OpenSearch,
        "open_replace" => EditorAction::OpenReplace,
        "search_next" => EditorAction::SearchNext,
        "search_prev" => EditorAction::SearchPrev,
        "close_search" => EditorAction::CloseSearch,
        "search_toggle_regex" => EditorAction::SearchToggleRegex,
        "search_toggle_case_sensitive" => EditorAction::SearchToggleCaseSensitive,
        "select_all_occurrences" => EditorAction::SelectAllOccurrences,
        "open_project_search" => EditorAction::OpenProjectSearch,
        "add_cursor_next_match" => EditorAction::AddCursorNextMatch,
        "skip_current_match" => EditorAction::SkipCurrentMatch,
        "undo_last_cursor" => EditorAction::UndoLastCursor,
        "box_select_extend_up" => EditorAction::BoxSelectExtend(Direction::Up),
        "box_select_extend_down" => EditorAction::BoxSelectExtend(Direction::Down),
        "box_select_extend_left" => EditorAction::BoxSelectExtend(Direction::Left),
        "box_select_extend_right" => EditorAction::BoxSelectExtend(Direction::Right),
        "filter_selection" => EditorAction::FilterSelection,
        "sort_lines_asc" => EditorAction::SortLinesAsc,
        "sort_lines_desc" => EditorAction::SortLinesDesc,
        "dedupe_lines" => EditorAction::DedupeLines,
        "reverse_lines" => EditorAction::ReverseLines,
        "to_upper" => EditorAction::ToUpper,
        "to_lower" => EditorAction::ToLower,
        "to_title" => EditorAction::ToTitle,
        "trim_trailing_whitespace" => EditorAction::TrimTrailingWhitespace,
        "join_lines" => EditorAction::JoinLines,
        "increment_number" => EditorAction::IncrementNumber,
        "decrement_number" => EditorAction::DecrementNumber,
        "convert_indent_to_spaces" => EditorAction::ConvertIndentToSpaces,
        "convert_indent_to_tabs" => EditorAction::ConvertIndentToTabs,
        "convert_eol_lf" => EditorAction::ConvertEolLf,
        "convert_eol_crlf" => EditorAction::ConvertEolCrlf,
        "align_selection" => EditorAction::AlignSelection,
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
        "open_symbol_picker" => EditorAction::OpenSymbolPicker,
        "toggle_fold_at_cursor" => EditorAction::ToggleFoldAtCursor,
        "fold_all" => EditorAction::FoldAll,
        "unfold_all" => EditorAction::UnfoldAll,
        "jump_list_back" => EditorAction::JumpListBack,
        "jump_list_forward" => EditorAction::JumpListForward,
        "set_mark" => EditorAction::BeginSetMark,
        "jump_to_mark" => EditorAction::BeginJumpToMark,
        "expand_snippet" => EditorAction::ExpandSnippetAtCursor,
        "snippet_next_stop" => EditorAction::SnippetNextStop,
        "snippet_prev_stop" => EditorAction::SnippetPrevStop,
        "snippet_cancel" => EditorAction::SnippetCancel,
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
        "open_git_dialog" => EditorAction::OpenGitDialog,
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
        "sidebar_refresh" => EditorAction::SidebarRefresh,
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
