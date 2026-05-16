use std::path::PathBuf;

/// Modes that capture keyboard input for a status-bar prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    /// Ctrl+G: "Go to line: {input}"
    JumpToLine(String),
    /// Ctrl+O: "Open: {input}"
    OpenFilePath(String),
    /// Ctrl+Shift+S: "Save as: {input}"
    SaveAsPath(String),
    /// F2 (sidebar): "Rename: {input}" — carries (original_path, current_input).
    RenamePath(PathBuf, String),
    /// Ctrl+Shift+N (sidebar): "New folder: {input}" — carries (parent_dir, current_input).
    NewFolderName(PathBuf, String),
    /// F2: "Rename: {input}" (LSP rename symbol)
    Rename(String),
    /// Git dialog → "Commit message: {input}". On submit, runs `git commit -m`.
    GitCommitMessage(String),
    /// Git dialog → "New branch: {input}". On submit, runs `git checkout -b`.
    GitNewBranch(String),
    /// Git dialog → "Stash message: {input}" (optional). On submit, runs `git stash push`.
    GitStashMessage(String),
    /// Ctrl+Alt+\ → "Shell filter (selection): {cmd}". On Enter, runs the
    /// command via `sh -c` with the selection on stdin and replaces the
    /// selection with the captured stdout.
    ShellFilter(String),
    /// "Align on character: " — `apply_align_on` is invoked with the first
    /// non-empty character submitted (Enter takes the first char, or aborts
    /// if empty).
    AlignChar(String),
    /// Ctrl+M: "Mark: " — auto-submits on the next typed alphabetic char.
    SetMarkChar,
    /// Ctrl+': "Jump to mark: " — auto-submits on the next typed char.
    JumpToMarkChar,
    /// "Record macro into slot: " — next char names the slot a–z.
    RecordMacroChar,
    /// "Replay macro from slot: " — next char names the slot a–z.
    ReplayMacroChar,
    /// Alt+': "Surround with: " — wraps the current selection in a chosen
    /// delimiter pair on the next typed character.
    SurroundChar,
}

impl InputMode {
    pub fn is_normal(&self) -> bool {
        matches!(self, InputMode::Normal)
    }

    /// Mutable access to the prompt buffer for string-carrying variants.
    ///
    /// Used by `handle_modal_input` to share a single InsertChar/DeleteBackward
    /// code path across every text-entry prompt. Returns `None` for variants
    /// that don't store an editable string (Normal, *Char variants).
    pub fn string_mut(&mut self) -> Option<&mut String> {
        match self {
            InputMode::JumpToLine(s)
            | InputMode::OpenFilePath(s)
            | InputMode::SaveAsPath(s)
            | InputMode::RenamePath(_, s)
            | InputMode::NewFolderName(_, s)
            | InputMode::Rename(s)
            | InputMode::GitCommitMessage(s)
            | InputMode::GitNewBranch(s)
            | InputMode::GitStashMessage(s)
            | InputMode::ShellFilter(s)
            | InputMode::AlignChar(s) => Some(s),
            InputMode::Normal
            | InputMode::SetMarkChar
            | InputMode::JumpToMarkChar
            | InputMode::RecordMacroChar
            | InputMode::ReplayMacroChar
            | InputMode::SurroundChar => None,
        }
    }
}
