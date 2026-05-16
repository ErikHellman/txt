use std::path::PathBuf;

/// State for the references list overlay.
pub struct ReferencesListState {
    pub items: Vec<ReferenceItem>,
    pub selected: usize,
}

/// State for the inline diff-peek float (`Alt+H`). Lists the HEAD lines for
/// the hunk under the cursor.
pub struct DiffPeekState {
    pub head_lines: Vec<String>,
    /// Cursor line at the time the peek was opened — used to anchor the float
    /// near the cursor.
    #[allow(dead_code)]
    pub anchor_line: usize,
}

/// State for the clipboard-ring picker overlay (`Ctrl+Shift+V`).
pub struct ClipboardRingState {
    /// Snapshot of the ring at the time the overlay was opened. Most recent
    /// entry first. Mutated only via `selected`; the underlying
    /// `ClipboardManager` is not touched until the user confirms a pick.
    pub entries: Vec<String>,
    pub selected: usize,
}

/// A single reference location.
pub struct ReferenceItem {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub context: String,
}
