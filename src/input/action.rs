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
    /// Toggle the file tree sidebar (Ctrl+B).
    ToggleSidebar,

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

    // ── App lifecycle ─────────────────────────────────────────────────
    /// Quit the editor. The app will confirm if there are unsaved changes.
    Quit,
    #[allow(dead_code)]
    ForceQuit,

    // ── Placeholder for unrecognised / unimplemented keys ─────────────
    Unhandled,
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
