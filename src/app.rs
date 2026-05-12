use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::SetTitle;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use crate::{
    clipboard::ClipboardManager,
    config::{Config, KeymapPreset, Theme, add_to_recent_files, load_recent_files},
    editor::Editor,
    editor::viewport::{screen_pos_to_byte_offset, screen_pos_to_line_display_col},
    git::GitGutter,
    input::{
        InputHandler,
        action::{Direction, EditorAction, ScrollDir},
        keybinding::KeyBindings,
    },
    search::{SearchState, project::ProjectSearchResults},
    ui,
    ui::command_palette::CommandPaletteState,
    ui::editor_view::gutter_width,
    ui::git_dialog::GitDialogState,
    watcher::FileWatcher,
};

/// The scroll amount for a single scroll-wheel tick or Ctrl+Up/Down.
const SCROLL_LINES: usize = 3;

/// Default sidebar width in terminal columns. The active width is stored on
/// `AppState::sidebar_width` so the user can resize by dragging the separator.
pub const DEFAULT_SIDEBAR_WIDTH: u16 = 28;

/// Minimum width the sidebar can be resized to (just enough to show short names).
pub const MIN_SIDEBAR_WIDTH: u16 = 12;

/// Maximum width the sidebar can be resized to in absolute columns. Also clamped
/// to half the terminal width so the editor stays usable.
pub const MAX_SIDEBAR_WIDTH: u16 = 80;

// ── Modal input mode ─────────────────────────────────────────────────────────

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
}

impl InputMode {
    pub fn is_normal(&self) -> bool {
        matches!(self, InputMode::Normal)
    }
}

// ── Fuzzy picker state ────────────────────────────────────────────────────────

pub struct FuzzyPickerState {
    pub query: String,
    /// All files in the project directory (populated once on open).
    pub all_files: Vec<PathBuf>,
    /// Scored and sorted (score DESC) indices into `all_files`.
    pub filtered: Vec<(u32, usize)>,
    /// Currently highlighted row (0-based within `filtered`).
    pub selected: usize,
}

impl FuzzyPickerState {
    /// Build by walking the current directory with `ignore` (respects .gitignore).
    /// `hide_git` skips the `.git` directory; `hide_dot` skips every dot-prefixed
    /// directory (and implies `hide_git`).
    pub fn new(hide_git: bool, hide_dot: bool) -> Self {
        let mut all_files = Vec::new();
        let mut builder = ignore::WalkBuilder::new(".");
        builder.hidden(false).git_ignore(true);
        crate::search::apply_hidden_dir_filters(&mut builder, hide_git, hide_dot);
        for entry in builder.build().flatten() {
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                // Strip the leading "./" for display clarity.
                let p = entry.into_path();
                let p = p.strip_prefix("./").map(PathBuf::from).unwrap_or(p);
                all_files.push(p);
            }
        }
        all_files.sort();
        let n = all_files.len().min(200); // show first 200 unfiltered
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files,
            filtered,
            selected: 0,
        }
    }

    /// Re-score the file list against the current query using nucleo.
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;

        if self.query.is_empty() {
            let n = self.all_files.len().min(200);
            self.filtered = (0..n).map(|i| (0u32, i)).collect();
            return;
        }

        use nucleo::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo::{Config, Matcher, Utf32String};

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);

        let mut scored: Vec<(u32, usize)> = self
            .all_files
            .iter()
            .enumerate()
            .filter_map(|(idx, path)| {
                let s = path.to_string_lossy();
                let haystack = Utf32String::from(s.as_ref());
                pattern
                    .score(haystack.slice(..), &mut matcher)
                    .map(|sc| (sc, idx))
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.truncate(200);
        self.filtered = scored;
    }

    /// Build a picker pre-populated with an explicit path list (for recent files).
    pub fn from_paths(paths: Vec<PathBuf>) -> Self {
        let n = paths.len().min(200);
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files: paths,
            filtered,
            selected: 0,
        }
    }

    /// Build a picker pre-populated with open buffer names (for buffer switcher).
    /// The `all_files` list stores synthetic paths using the buffer display name.
    pub fn from_buffers(names: Vec<(usize, String)>) -> Self {
        let all_files: Vec<PathBuf> = names.iter().map(|(_, name)| PathBuf::from(name)).collect();
        let n = all_files.len();
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files,
            filtered,
            selected: 0,
        }
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.filtered
            .get(self.selected)
            .map(|(_, idx)| &self.all_files[*idx])
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }
}

// ── Symbol-in-file picker state ──────────────────────────────────────────

/// State for the Ctrl+Shift+O symbol-in-file picker. Mirrors
/// [`FuzzyPickerState`] but scores against symbol names rather than file
/// paths and tracks each symbol's byte range so `Enter` can jump the cursor.
pub struct SymbolPickerState {
    pub query: String,
    /// All symbols collected from the active buffer's parse tree.
    pub all_symbols: Vec<crate::syntax::Symbol>,
    /// Scored and sorted (score DESC) indices into `all_symbols`.
    pub filtered: Vec<(u32, usize)>,
    /// Currently highlighted row.
    pub selected: usize,
}

impl SymbolPickerState {
    /// Build with the symbols already collected from the active buffer.
    pub fn new(symbols: Vec<crate::syntax::Symbol>) -> Self {
        let n = symbols.len();
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_symbols: symbols,
            filtered,
            selected: 0,
        }
    }

    /// Re-score against the current query using nucleo. Empty query shows
    /// every symbol in source order.
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;

        if self.query.is_empty() {
            let n = self.all_symbols.len();
            self.filtered = (0..n).map(|i| (0u32, i)).collect();
            return;
        }

        use nucleo::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo::{Config, Matcher, Utf32String};

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);

        let mut scored: Vec<(u32, usize)> = self
            .all_symbols
            .iter()
            .enumerate()
            .filter_map(|(idx, sym)| {
                let haystack = Utf32String::from(sym.name.as_str());
                pattern
                    .score(haystack.slice(..), &mut matcher)
                    .map(|sc| (sc, idx))
            })
            .collect();
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        self.filtered = scored;
    }

    pub fn selected_symbol(&self) -> Option<&crate::syntax::Symbol> {
        self.filtered
            .get(self.selected)
            .map(|(_, idx)| &self.all_symbols[*idx])
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }
}

// ── Project search overlay state ───────────────────────────────────────────

/// State for the project-wide search-and-replace overlay (Ctrl+Shift+F).
pub struct ProjectSearchState {
    pub query: String,
    pub replace_text: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub show_replace: bool,
    pub focus_replace: bool,
    pub results: ProjectSearchResults,
    /// Index into `results.matches` for the highlighted row.
    pub selected: usize,
}

impl ProjectSearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replace_text: String::new(),
            is_regex: false,
            case_sensitive: false,
            show_replace: false,
            focus_replace: false,
            results: ProjectSearchResults::default(),
            selected: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.results.matches.is_empty() && self.selected + 1 < self.results.matches.len() {
            self.selected += 1;
        }
    }
}

impl Default for ProjectSearchState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Sidebar / file tree ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
/// Tracks the state of a sidebar delete confirmation.
pub enum ConfirmDelete {
    /// Deleting a file — waiting for Y/N.
    File(PathBuf),
    /// Deleting a directory — first step, waiting for Y/N.
    Dir(PathBuf),
    /// Deleting a directory — user pressed Y, now waiting for Enter to confirm.
    DirConfirmed(PathBuf),
}

/// Tracks a file that has been cut or copied in the sidebar.
pub struct SidebarClipboard {
    pub path: PathBuf,
    pub is_cut: bool, // true = move, false = copy
}

pub struct TreeEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

pub struct SidebarState {
    pub entries: Vec<TreeEntry>,
    pub selected: usize,
    /// Index of the first visible entry. Independent from `selected`:
    /// scroll-wheel and resize move this without touching `selected`,
    /// while keyboard navigation calls `ensure_selected_visible` to keep
    /// the selection on screen.
    pub scroll_offset: usize,
    pub root: PathBuf,
}

impl SidebarState {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut state = Self {
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            root: root.clone(),
        };
        state.load_root();
        state
    }

    /// Scroll the visible window by `delta_lines` rows (positive = down).
    /// Selection is unchanged. Clamped to `[0, max_scroll]` where
    /// `max_scroll` keeps at least one row visible when `viewport_rows > 0`.
    pub fn scroll_by(&mut self, delta_lines: isize, viewport_rows: usize) {
        let max_scroll = if viewport_rows == 0 || self.entries.len() <= viewport_rows {
            0
        } else {
            self.entries.len() - viewport_rows
        };
        let new = (self.scroll_offset as isize + delta_lines).max(0) as usize;
        self.scroll_offset = new.min(max_scroll);
    }

    /// Adjust `scroll_offset` so the currently-selected entry is on-screen.
    /// No-op when `viewport_rows == 0`.
    pub fn ensure_selected_visible(&mut self, viewport_rows: usize) {
        if viewport_rows == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport_rows {
            self.scroll_offset = self.selected + 1 - viewport_rows;
        }
    }

    /// Clamp `scroll_offset` so it doesn't point past the end of `entries`.
    /// Called after operations that shrink or change the entry list.
    fn clamp_scroll(&mut self) {
        let max = self.entries.len().saturating_sub(1);
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }

    /// Load the top-level entries of the root directory.
    fn load_root(&mut self) {
        self.entries.clear();
        // Root node is always present and always expanded; it cannot be collapsed.
        self.entries.push(TreeEntry {
            path: self.root.clone(),
            depth: 0,
            is_dir: true,
            expanded: true,
        });
        self.entries_from_dir(&self.root.clone(), 1, true);
    }

    /// Append entries for a directory at `depth`. If `expand` is false, only
    /// add the directory entry itself (collapsed).
    fn entries_from_dir(&mut self, dir: &PathBuf, depth: usize, _expand: bool) {
        let mut children: Vec<(PathBuf, bool)> = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let is_dir = path.is_dir();
                children.push((path, is_dir));
            }
        }
        // Sort: dirs first, then files, both alphabetically.
        children.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.file_name().cmp(&b.0.file_name()))
        });
        for (path, is_dir) in children {
            self.entries.push(TreeEntry {
                path,
                depth,
                is_dir,
                expanded: false,
            });
        }
    }

    /// Toggle expand/collapse of the selected directory entry.
    #[allow(dead_code)]
    pub fn toggle_selected(&mut self) {
        let idx = self.selected;
        if idx >= self.entries.len() {
            return;
        }
        let entry = &self.entries[idx];
        if !entry.is_dir {
            return;
        }
        // Root cannot be collapsed.
        if entry.path == self.root {
            return;
        }
        if entry.expanded {
            // Collapse: remove all children (entries with depth > entry.depth
            // that follow this entry and have higher depth).
            let depth = entry.depth;
            self.entries[idx].expanded = false;
            let start = idx + 1;
            let end = self.entries[start..]
                .iter()
                .position(|e| e.depth <= depth)
                .map(|p| start + p)
                .unwrap_or(self.entries.len());
            self.entries.drain(start..end);
        } else {
            // Expand: load children and insert after this entry.
            let dir = self.entries[idx].path.clone();
            let depth = self.entries[idx].depth;
            self.entries[idx].expanded = true;
            let mut children: Vec<TreeEntry> = Vec::new();
            let mut tmp = Self {
                entries: Vec::new(),
                selected: 0,
                scroll_offset: 0,
                root: dir.clone(),
            };
            tmp.entries_from_dir(&dir, depth + 1, false);
            children.extend(tmp.entries);
            let insert_at = idx + 1;
            for (i, child) in children.into_iter().enumerate() {
                self.entries.insert(insert_at + i, child);
            }
        }
        self.clamp_scroll();
    }

    /// Collapse the directory entry at `idx` (if expanded), removing its children.
    fn collapse_at(&mut self, idx: usize) {
        if idx >= self.entries.len() || !self.entries[idx].is_dir || !self.entries[idx].expanded {
            return;
        }
        // Root cannot be collapsed.
        if self.entries[idx].path == self.root {
            return;
        }
        let depth = self.entries[idx].depth;
        self.entries[idx].expanded = false;
        let start = idx + 1;
        let end = self.entries[start..]
            .iter()
            .position(|e| e.depth <= depth)
            .map(|p| start + p)
            .unwrap_or(self.entries.len());
        self.entries.drain(start..end);
        self.clamp_scroll();
    }

    /// Move selection to the nearest ancestor directory and collapse it.
    /// Does nothing if the selected entry is already at depth 0.
    pub fn move_to_parent_and_collapse(&mut self) {
        let idx = self.selected;
        let depth = match self.entries.get(idx) {
            Some(e) => e.depth,
            None => return,
        };
        if depth == 0 {
            return;
        }
        if let Some(parent_idx) = self.entries[..idx]
            .iter()
            .rposition(|e| e.depth == depth - 1)
        {
            self.selected = parent_idx;
            self.collapse_at(parent_idx);
        }
    }

    /// Expand the directory entry at `idx` without affecting `self.selected`.
    fn expand_dir_at(&mut self, idx: usize) {
        if self.entries[idx].expanded || !self.entries[idx].is_dir {
            return;
        }
        let dir = self.entries[idx].path.clone();
        let depth = self.entries[idx].depth;
        self.entries[idx].expanded = true;
        let mut tmp = Self {
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            root: dir.clone(),
        };
        tmp.entries_from_dir(&dir, depth + 1, false);
        let insert_at = idx + 1;
        for (i, child) in tmp.entries.into_iter().enumerate() {
            self.entries.insert(insert_at + i, child);
        }
    }

    /// Expand all ancestor directories leading to `target` and select it.
    /// Does nothing if `target` is not under `self.root`.
    pub fn expand_to_path(&mut self, target: &std::path::Path) {
        // Relative paths (e.g. from the fuzzy picker) are resolved against root.
        let abs_target = if target.is_absolute() {
            target.to_path_buf()
        } else {
            self.root.join(target)
        };
        let Ok(relative) = abs_target.strip_prefix(&self.root) else {
            return;
        };
        let mut current = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for (i, component) in components.iter().enumerate() {
            current = current.join(component);
            let is_last = i == components.len() - 1;
            if let Some(idx) = self.entries.iter().position(|e| e.path == current) {
                if is_last {
                    self.selected = idx;
                } else {
                    self.expand_dir_at(idx);
                }
            } else {
                break;
            }
        }
    }

    #[allow(dead_code)]
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    #[allow(dead_code)]
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    #[allow(dead_code)]
    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.entries.get(self.selected).map(|e| &e.path)
    }

    /// Returns true if the root directory itself is currently selected.
    fn root_is_selected(&self) -> bool {
        self.entries
            .get(self.selected)
            .map(|e| e.path == self.root)
            .unwrap_or(false)
    }

    /// Reload the sidebar, preserving expanded directories and selection by path.
    pub fn refresh(&mut self) {
        let expanded: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.is_dir && e.expanded)
            .map(|e| e.path.clone())
            .collect();
        let old_path = self.selected_path().cloned();
        let old_selected = self.selected;
        self.load_root();
        // Re-expand previously expanded directories.
        for path in &expanded {
            if let Some(idx) = self
                .entries
                .iter()
                .position(|e| &e.path == path && e.is_dir)
                && !self.entries[idx].expanded
            {
                self.selected = idx;
                self.toggle_selected();
            }
        }
        // Restore selection by path if possible, otherwise clamp the old index.
        if let Some(ref old) = old_path
            && let Some(idx) = self.entries.iter().position(|e| &e.path == old)
        {
            self.selected = idx;
            self.clamp_scroll();
            return;
        }
        self.selected = old_selected.min(self.entries.len().saturating_sub(1));
        self.clamp_scroll();
    }
}

/// Generate a copy target path with a `-N` suffix (before the extension).
/// Returns `None` if no suitable name can be found within 1000 attempts.
fn copy_target_path(source: &std::path::Path, dest_dir: &std::path::Path) -> Option<PathBuf> {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = source.extension().and_then(|e| e.to_str());
    for n in 1..1000 {
        let name = match ext {
            Some(e) => format!("{}-{}.{}", stem, n, e),
            None => format!("{}-{}", stem, n),
        };
        let candidate = dest_dir.join(&name);
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

// ── LSP picker state ─────────────────────────────────────────────────────────

/// State for the code completion popup.
pub struct CompletionState {
    /// All items received from the server.
    pub items: Vec<CompletionItemEntry>,
    /// Indices into `items` after prefix filtering.
    pub filtered: Vec<usize>,
    /// Currently highlighted row in `filtered`.
    pub selected: usize,
    /// Byte offset where completion was triggered (start of the prefix).
    pub anchor_byte: usize,
    /// Line of the trigger position (for popup positioning).
    #[allow(dead_code)]
    pub anchor_line: usize,
    /// Display column of the trigger position.
    #[allow(dead_code)]
    pub anchor_col: usize,
}

/// A single completion item (simplified from LSP).
pub struct CompletionItemEntry {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub filter_text: String,
    pub kind_label: &'static str,
}

impl CompletionState {
    pub fn new(anchor_byte: usize, anchor_line: usize, anchor_col: usize) -> Self {
        Self {
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            anchor_byte,
            anchor_line,
            anchor_col,
        }
    }

    /// Re-filter items against the typed prefix.
    pub fn filter(&mut self, prefix: &str) {
        let lower_prefix = prefix.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.filter_text.to_lowercase().contains(&lower_prefix))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    /// Get the currently selected item, if any.
    pub fn selected_item(&self) -> Option<&CompletionItemEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.items.get(i))
    }
}

/// State for the hover info popup.
pub struct HoverState {
    pub content: String,
    #[allow(dead_code)]
    pub anchor_line: usize,
    #[allow(dead_code)]
    pub anchor_col: usize,
}

/// State for the references list overlay.
pub struct ReferencesListState {
    pub items: Vec<ReferenceItem>,
    pub selected: usize,
}

/// A single reference location.
pub struct ReferenceItem {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub context: String,
}

/// Why the user is being prompted to approve an LSP binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspApprovalReason {
    /// We've never seen this binary before.
    FirstLaunch,
    /// We have an entry for this path, but the hash on disk has changed.
    BinaryChanged { previous_hash: String },
}

/// State for the LSP-binary approval overlay.
#[derive(Debug, Clone)]
pub struct PendingLspApproval {
    /// Server identifier from the active LSP config (e.g. `"rust-analyzer"`).
    pub server_name: String,
    /// Raw command from the config (may be a bare name or a path).
    pub command: String,
    /// Command-line args from the config.
    pub args: Vec<String>,
    /// Path returned by `which` (or the absolute path the user gave).
    pub display_path: PathBuf,
    /// Canonicalized path that gets hashed and recorded in the trust store.
    pub canonical_path: PathBuf,
    /// SHA-256 of the binary contents, lowercase hex.
    pub hash: String,
    /// What triggered the prompt.
    pub reason: LspApprovalReason,
}

/// Built-in LSP server definitions the user can choose from.
pub const LSP_SERVER_OPTIONS: &[(&str, &str, &[&str])] = &[
    // (display name / key, command, args)
    ("rust-analyzer", "rust-analyzer", &[]),
    ("pyright", "pyright-langserver", &["--stdio"]),
    (
        "typescript-language-server",
        "typescript-language-server",
        &["--stdio"],
    ),
    ("clangd", "clangd", &[]),
    ("gopls", "gopls", &["serve"]),
    ("lua-language-server", "lua-language-server", &[]),
    ("zls", "zls", &[]),
];

/// State for the LSP configuration picker overlay.
pub struct LspPickerState {
    /// Currently highlighted row. 0 = Disabled, 1..=N = server options.
    pub selected: usize,
}

impl LspPickerState {
    pub fn new(lsp_config: &crate::lsp::config::WorkspaceLspConfig) -> Self {
        // Pre-select the currently active server, or 0 (Disabled).
        let selected = if !lsp_config.is_active() {
            0
        } else {
            lsp_config
                .server
                .as_deref()
                .and_then(|key| {
                    LSP_SERVER_OPTIONS
                        .iter()
                        .position(|(name, _, _)| *name == key)
                        .map(|i| i + 1)
                })
                .unwrap_or(0)
        };
        Self { selected }
    }

    pub fn num_rows(&self) -> usize {
        1 + LSP_SERVER_OPTIONS.len() // "Disabled" + servers
    }
}

/// State of an in-progress sidebar separator-drag.
#[derive(Debug, Clone, Copy)]
pub struct SidebarDrag {
    /// Column the user pressed the mouse button on (always the separator x).
    pub start_col: u16,
    /// Width the sidebar had when the drag began.
    pub start_width: u16,
}

// ── AppState ──────────────────────────────────────────────────────────────────

/// All mutable application state.
pub struct AppState {
    pub editor: Editor,
    pub clipboard: ClipboardManager,
    pub input_mode: InputMode,
    pub fuzzy_picker: Option<FuzzyPickerState>,
    pub symbol_picker: Option<SymbolPickerState>,
    pub marks: crate::marks::NamedMarks,
    pub jumps: crate::marks::JumpList,
    /// Lazy-loaded snippet store, populated per language on first use.
    pub snippets: crate::snippet::SnippetStore,
    /// In-memory keyboard macro state (recording, slots, replay flag).
    pub macros: crate::macros::MacroState,
    pub sidebar: Option<SidebarState>,
    pub sidebar_focused: bool,
    /// Current sidebar width in columns (excluding the 1-col separator).
    /// Mutated when the user drags the separator.
    pub sidebar_width: u16,
    /// Outer sidebar rect (including the separator column) from the most recent
    /// frame. Used by mouse-event routing to translate `(col, row)` into either
    /// an entry hit, a separator hit, or a fall-through to the editor.
    pub sidebar_area: Option<Rect>,
    /// Tab-bar rect from the most recent frame, or `None` when the strip is
    /// not shown (single tab). Used by mouse-event routing to hit-test tab
    /// labels.
    pub tab_bar_area: Option<Rect>,
    /// Active separator-drag, if any.
    pub sidebar_drag: Option<SidebarDrag>,
    /// Active Alt+drag box-select anchor, in (line, display_col).
    /// Set on `BoxDragStart`, used by `BoxDragUpdate`, cleared on `BoxDragEnd`.
    pub box_drag_anchor: Option<(usize, usize)>,
    saved_sidebar: Option<SidebarState>,
    pub sidebar_clipboard: Option<SidebarClipboard>,
    pub search_state: Option<SearchState>,
    pub project_search: Option<ProjectSearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub show_help: bool,
    pub help_scroll: usize,
    pub show_settings: bool,
    pub settings_cursor: usize,
    /// First-launch welcome overlay. Set in `AppState::new` when no config
    /// file exists on disk; cleared (and version persisted) on dismissal.
    pub show_welcome: bool,
    pub welcome_scroll: usize,
    /// Post-upgrade changelog overlay. Set in `AppState::new` when the
    /// previously seen version is older than the current one (by minor or
    /// major). The sections to render are computed once at startup.
    pub show_changelog: bool,
    pub changelog_scroll: usize,
    pub changelog_sections: Vec<crate::ui::changelog_overlay::Section>,
    pub lsp_picker: Option<LspPickerState>,
    pub completion: Option<CompletionState>,
    pub hover: Option<HoverState>,
    pub references_list: Option<ReferencesListState>,
    pub git_gutter: Option<GitGutter>,
    pub git_dialog: Option<GitDialogState>,
    pub config: Config,
    pub input: InputHandler,
    pub workspace: PathBuf,
    pub should_quit: bool,
    pub confirm_quit: bool,
    pub confirm_delete: Option<ConfirmDelete>,
    /// Debounce timer for auto-save: reset on every edit, fires after 1 s of inactivity.
    auto_save_timer: Option<std::time::Instant>,
    /// Active file watcher for the current buffer (replaced on each file open/save).
    file_watcher: Option<FileWatcher>,
    /// Per-workspace LSP configuration (loaded from `<workspace>/.txt/lsp.toml`).
    pub lsp_config: crate::lsp::config::WorkspaceLspConfig,
    /// Per-workspace formatting overrides (loaded from
    /// `<workspace>/.txt/formatters.toml`). `None` when the file is missing
    /// or unparseable; the global config still applies.
    pub project_fmt: Option<crate::formatting::FormattingConfig>,
    /// Active LSP server connection (None when LSP is disabled or unavailable).
    pub lsp: Option<crate::lsp::LspRegistry>,
    /// Pending trust-on-first-use approval for the LSP binary, if any.
    pub pending_lsp_approval: Option<PendingLspApproval>,
    /// Transient error message shown in the status bar (cleared on next user action).
    pub status_error: Option<String>,
    /// When the buffer was last edited — used to debounce `didChange` notifications
    /// and semantic token re-requests so we don't send the full buffer on every keystroke.
    lsp_dirty_since: Option<Instant>,
    /// Whether `didChange` has been sent for the current dirty period (but semantic
    /// tokens haven't been re-requested yet).
    lsp_change_sent: bool,
    pub term_width: u16,
    pub term_height: u16,
    pub memory_rss_kb: u64,
    memory_last_checked: Instant,
    /// Currently checked-out git branch in `workspace`, if any. Refreshed
    /// on a 2 s cadence so external `git checkout`s are picked up live.
    pub git_branch: Option<String>,
    git_branch_last_checked: Instant,
}

impl AppState {
    pub fn new(editor: Editor, workspace: PathBuf) -> Self {
        let config_existed = Config::config_file_exists();
        let mut config = Config::load();

        // Decide what onboarding overlay (if any) to show on this launch:
        //   - No config file at all → first run → welcome.
        //   - Config exists with a previously seen version older than the
        //     current minor/major → changelog.
        //   - Config exists but no version recorded (pre-existing user from
        //     before this feature) → silently catch them up.
        let current_version = env!("CARGO_PKG_VERSION");
        let mut show_welcome = false;
        let mut show_changelog = false;
        let mut changelog_sections = Vec::new();
        match (&config.last_seen_version, config_existed) {
            (None, false) => {
                show_welcome = true;
            }
            (None, true) => {
                config.last_seen_version = Some(current_version.to_string());
                config.save();
            }
            (Some(prev), _) => {
                if crate::config::is_minor_or_major_upgrade(prev, current_version) {
                    let sections = crate::ui::changelog_overlay::relevant_sections(prev);
                    if !sections.is_empty() {
                        changelog_sections = sections;
                        show_changelog = true;
                    } else {
                        // Nothing parseable since last time — just bump.
                        config.last_seen_version = Some(current_version.to_string());
                        config.save();
                    }
                }
            }
        }

        let lsp_config = crate::lsp::config::WorkspaceLspConfig::load(&workspace);
        let project_fmt = crate::formatting::project::load(&workspace);
        let git_branch = crate::git::current_branch(&workspace);
        let mut state = Self {
            editor,
            clipboard: ClipboardManager::new(),
            input_mode: InputMode::Normal,
            fuzzy_picker: None,
            symbol_picker: None,
            marks: crate::marks::NamedMarks::load(&workspace),
            jumps: crate::marks::JumpList::load(&workspace),
            snippets: crate::snippet::SnippetStore::new(),
            macros: crate::macros::MacroState::new(),
            sidebar: None,
            sidebar_focused: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_area: None,
            tab_bar_area: None,
            sidebar_drag: None,
            box_drag_anchor: None,
            saved_sidebar: None,
            sidebar_clipboard: None,
            search_state: None,
            project_search: None,
            command_palette: None,
            show_help: false,
            help_scroll: 0,
            show_settings: false,
            settings_cursor: 0,
            show_welcome,
            welcome_scroll: 0,
            show_changelog,
            changelog_scroll: 0,
            changelog_sections,
            lsp_picker: None,
            completion: None,
            hover: None,
            references_list: None,
            git_gutter: None,
            git_dialog: None,
            config,
            input: InputHandler::new(),
            workspace,
            should_quit: false,
            confirm_quit: false,
            confirm_delete: None,
            auto_save_timer: None,
            file_watcher: None,
            lsp_config,
            project_fmt,
            lsp: None,
            pending_lsp_approval: None,
            status_error: None,
            lsp_dirty_since: None,
            lsp_change_sent: false,
            term_width: 80,
            term_height: 24,
            memory_rss_kb: 0,
            memory_last_checked: Instant::now(),
            git_branch,
            git_branch_last_checked: Instant::now(),
        };
        // Apply config to initial buffer.
        if state.config.word_wrap {
            state.editor.active_mut().viewport.word_wrap = true;
        }
        // Compute git gutter for the initial file (if any).
        state.refresh_git_gutter();
        // Trust-gated LSP launch (may set `pending_lsp_approval` for a first-frame prompt).
        state.request_lsp_start();
        state
    }

    // ── Main dispatch ────────────────────────────────────────────────────────

    pub fn update(&mut self, action: EditorAction, terminal_height: u16) {
        self.term_height = terminal_height;

        // Clear transient status error on any user interaction.
        self.status_error = None;

        // Append to the in-progress macro recording (if any). The state's
        // own `replaying` flag suppresses re-recording during playback.
        if crate::macros::is_recordable(&action) {
            self.macros.append(&action);
        }

        // Capture undo depth before dispatch so we can detect actual buffer edits below.
        let pre_undo_depth = self.editor.active().buffer.undo_depth();

        // Quit confirmation mode
        if self.confirm_quit {
            match action {
                EditorAction::InsertChar('y') | EditorAction::InsertChar('Y') => {
                    self.should_quit = true;
                }
                EditorAction::InsertChar('n')
                | EditorAction::InsertChar('N')
                | EditorAction::Quit => {
                    self.confirm_quit = false;
                }
                _ => {
                    self.confirm_quit = false;
                }
            }
            return;
        }

        // LSP-binary trust approval — security decision, captures all input.
        if self.pending_lsp_approval.is_some() {
            self.handle_lsp_approval(&action);
            return;
        }

        // Delete confirmation mode
        if self.confirm_delete.is_some() {
            self.handle_confirm_delete(action);
            return;
        }

        // Welcome overlay — first-launch onboarding, captures most input.
        if self.show_welcome && self.handle_welcome(&action) {
            return;
        }

        // Changelog overlay — post-upgrade summary, captures most input.
        if self.show_changelog && self.handle_changelog(&action) {
            return;
        }

        // Help overlay — intercept navigation keys for scrolling
        if self.show_help && self.handle_help(&action) {
            return;
        }

        // Settings overlay — intercept navigation and edits
        if self.show_settings && self.handle_settings(&action) {
            return;
        }

        // LSP config picker — intercept navigation and selection
        if self.lsp_picker.is_some() && self.handle_lsp_picker(&action) {
            return;
        }

        // Modal input (status-bar prompts) — must come before sidebar so that
        // rename / new-folder prompts receive Enter/typing even while sidebar is focused.
        // Also takes precedence over the git dialog so InputMode::Git* prompts
        // (commit message, new branch, stash message) own input while the
        // dialog stays open behind them.
        if !self.input_mode.is_normal() {
            self.handle_modal_input(action);
            return;
        }

        // Git operations dialog — captures all input.
        if self.git_dialog.is_some() {
            self.handle_git_dialog(action);
            return;
        }

        // Sidebar focus — intercept navigation when sidebar has focus
        if self.sidebar_focused && self.handle_sidebar_input(&action) {
            return;
        }

        // Project search overlay — captured input
        if self.project_search.is_some() && self.handle_project_search(action.clone()) {
            return;
        }

        // Command palette — captured input
        if self.command_palette.is_some() {
            self.handle_command_palette(action);
            return;
        }

        // Fuzzy picker — captured input
        if self.fuzzy_picker.is_some() {
            self.handle_fuzzy_picker(action);
            return;
        }

        // Symbol picker — captured input
        if self.symbol_picker.is_some() {
            self.handle_symbol_picker(action);
            return;
        }

        // Completion popup — partially captured (chars fall through to editing)
        if self.completion.is_some() && self.handle_completion_input(&action) {
            return;
        }

        // References list — captured input
        if self.references_list.is_some() && self.handle_references_input(&action) {
            return;
        }

        // Search / replace bar — captured input (navigation still falls through)
        if self.search_state.is_some() && self.handle_search_input(action.clone()) {
            return;
        }
        // Navigation actions fall through to normal dispatch below.

        // Normal editing
        let text_h = (terminal_height as usize).saturating_sub(1);
        let clears_ast = !matches!(
            action,
            EditorAction::AstExpandSelection | EditorAction::AstContractSelection
        );
        if clears_ast {
            self.editor.active_mut().syntax.clear_selection_history();
        }

        match action {
            // ── AST-aware selection ───────────────────────────────────
            EditorAction::AstExpandSelection => {
                let current = self
                    .editor
                    .active()
                    .buffer
                    .cursors
                    .primary()
                    .selection_bytes();
                let new_range = self.editor.active_mut().syntax.expand_selection(current);
                if let Some(r) = new_range {
                    self.editor
                        .active_mut()
                        .buffer
                        .move_cursor_to(r.start, false);
                    self.editor.active_mut().buffer.move_cursor_to(r.end, true);
                } else if self.editor.active().syntax.language
                    == crate::syntax::language::Lang::Unknown
                {
                    self.close_tab();
                }
            }
            EditorAction::AstContractSelection => {
                let prev = self.editor.active_mut().syntax.contract_selection();
                if let Some(r) = prev {
                    if r.is_empty() {
                        self.editor
                            .active_mut()
                            .buffer
                            .move_cursor_to(r.start, false);
                    } else {
                        self.editor
                            .active_mut()
                            .buffer
                            .move_cursor_to(r.start, false);
                        self.editor.active_mut().buffer.move_cursor_to(r.end, true);
                    }
                }
            }
            EditorAction::GoToMatchingBracket => {
                let buf = &mut self.editor.active_mut().buffer;
                let cursor_byte = buf.cursors.primary().byte_offset;
                if let Some((open_b, close_b)) =
                    crate::buffer::edit::find_matching_bracket(buf.rope(), cursor_byte)
                {
                    // Jump to the opposite bracket from the one currently under cursor.
                    let target = if cursor_byte == open_b {
                        close_b
                    } else {
                        open_b
                    };
                    buf.move_cursor_to(target, false);
                }
            }

            // ── Text insertion ────────────────────────────────────────
            EditorAction::InsertChar(c) => {
                let (indent, rules) = self.indent_for_active();
                if self.editor.active().buffer.cursors.is_multi() {
                    self.editor.active_mut().buffer.multi_insert_char(c);
                } else {
                    self.editor
                        .active_mut()
                        .buffer
                        .insert_char_with_indent(c, &indent, rules);
                }
            }
            EditorAction::InsertNewline => {
                let (indent, rules) = self.indent_for_active();
                self.editor
                    .active_mut()
                    .buffer
                    .insert_newline(&indent, rules);
            }
            EditorAction::InsertTab => {
                if self.editor.active().snippet_session.is_some() {
                    self.snippet_advance(true);
                    return;
                }
                // Try to expand a snippet whose prefix matches the word
                // immediately before the cursor. Falls through to normal
                // tab-indent behaviour when no snippet is found.
                if self.try_expand_snippet_silently() {
                    return;
                }
                let (indent, _) = self.indent_for_active();
                // Multi-line selection → indent every touched line.
                let buf = &self.editor.active().buffer;
                let primary = buf.cursors.primary();
                let multi_line_selection = primary.has_selection() && {
                    let r = primary.selection_bytes();
                    let rope = buf.rope();
                    rope.char_to_line(rope.byte_to_char(r.start))
                        != rope.char_to_line(rope.byte_to_char(r.end))
                };
                if multi_line_selection {
                    self.editor.active_mut().buffer.indent_lines(&indent);
                } else if self.editor.active().buffer.cursors.is_multi() {
                    self.editor
                        .active_mut()
                        .buffer
                        .multi_insert_str(&indent.one_level());
                } else {
                    self.editor.active_mut().buffer.insert_tab(&indent);
                }
            }
            EditorAction::IndentSelection => {
                let (indent, _) = self.indent_for_active();
                self.editor.active_mut().buffer.indent_lines(&indent);
            }
            EditorAction::DedentSelection => {
                if self.editor.active().snippet_session.is_some() {
                    self.snippet_advance(false);
                    return;
                }
                let (indent, _) = self.indent_for_active();
                self.editor.active_mut().buffer.dedent_lines(&indent);
            }
            EditorAction::FormatBuffer => {
                self.format_buffer();
            }

            // ── Deletion ──────────────────────────────────────────────
            EditorAction::DeleteBackward => {
                if self.editor.active().buffer.cursors.is_multi() {
                    self.editor.active_mut().buffer.multi_delete_backward();
                } else {
                    self.editor.active_mut().buffer.delete_backward();
                }
            }
            EditorAction::DeleteForward => {
                if self.editor.active().buffer.cursors.is_multi() {
                    self.editor.active_mut().buffer.multi_delete_forward();
                } else {
                    self.editor.active_mut().buffer.delete_forward();
                }
            }
            EditorAction::DeleteWordBackward => {
                let buf = &mut self.editor.active_mut().buffer;
                let at = buf.cursors.primary().byte_offset;
                let prev = crate::buffer::edit::prev_word_boundary(buf.rope(), at);
                buf.delete_range(prev, at);
            }
            EditorAction::DeleteWordForward => {
                let buf = &mut self.editor.active_mut().buffer;
                let at = buf.cursors.primary().byte_offset;
                let next = crate::buffer::edit::next_word_boundary(buf.rope(), at);
                buf.delete_range(at, next);
            }
            EditorAction::KillLine => {
                let (at, end, killed) = {
                    let buf = &mut self.editor.active_mut().buffer;
                    let at = buf.cursors.primary().byte_offset;
                    let rope = buf.rope();
                    let char_at = rope.byte_to_char(at);
                    let line_idx = rope.char_to_line(char_at);
                    let line_start_char = rope.line_to_char(line_idx);
                    let line_end_char = line_start_char + rope.line(line_idx).len_chars();
                    // len_chars() includes the trailing '\n'; strip it to get content end.
                    let content_end_char = if line_end_char > line_start_char
                        && rope.char(line_end_char - 1) == '\n'
                    {
                        line_end_char - 1
                    } else {
                        line_end_char
                    };
                    let content_end_byte = rope.char_to_byte(content_end_char);
                    let end = if at == content_end_byte {
                        // At end of line content: delete the newline to join lines.
                        crate::buffer::edit::next_grapheme_boundary(rope, at)
                    } else {
                        content_end_byte
                    };
                    let killed = if end > at {
                        rope.slice(rope.byte_to_char(at)..rope.byte_to_char(end))
                            .to_string()
                    } else {
                        String::new()
                    };
                    (at, end, killed)
                };
                if !killed.is_empty() {
                    self.clipboard.set(killed);
                }
                if end > at {
                    self.editor.active_mut().buffer.delete_range(at, end);
                }
            }

            // ── Clipboard ────────────────────────────────────────────
            EditorAction::Copy => {
                if let Some(text) = self.selected_text() {
                    self.clipboard.set(text);
                }
            }
            EditorAction::CopyFileReference => {
                if let Some(path) = self.editor.active().path.as_ref() {
                    let buf = &self.editor.active().buffer;
                    let cursor = buf.cursors.primary();
                    let rope = buf.rope();
                    let line_start_byte = rope.char_to_byte(rope.line_to_char(cursor.line));
                    let char_col = rope.byte_to_char(line_start_byte + cursor.col)
                        - rope.line_to_char(cursor.line);
                    let relative = path
                        .strip_prefix(&self.workspace)
                        .unwrap_or(path)
                        .display()
                        .to_string();
                    let reference = format!("{}:{},{}", relative, cursor.line + 1, char_col + 1);
                    self.clipboard.set(reference);
                }
            }
            EditorAction::Cut => {
                if let Some(text) = self.selected_text() {
                    self.clipboard.set(text);
                    let range = self
                        .editor
                        .active()
                        .buffer
                        .cursors
                        .primary()
                        .selection_bytes();
                    self.editor
                        .active_mut()
                        .buffer
                        .delete_range(range.start, range.end);
                }
            }
            EditorAction::Paste(text) => {
                let content = if text.is_empty() {
                    self.clipboard.get()
                } else {
                    text
                };
                if !content.is_empty() {
                    self.editor.active_mut().buffer.insert_str(&content);
                }
            }

            // ── Cursor movement ───────────────────────────────────────
            EditorAction::MoveCursor(dir) => match dir {
                Direction::Left => self.editor.active_mut().buffer.move_cursor_left(false),
                Direction::Right => self.editor.active_mut().buffer.move_cursor_right(false),
                Direction::Up => self.editor.active_mut().buffer.move_cursor_up(false),
                Direction::Down => self.editor.active_mut().buffer.move_cursor_down(false),
            },
            EditorAction::MoveCursorWord(dir) => match dir {
                Direction::Left => self.editor.active_mut().buffer.move_cursor_word_left(false),
                Direction::Right => self
                    .editor
                    .active_mut()
                    .buffer
                    .move_cursor_word_right(false),
                _ => {}
            },
            EditorAction::MoveCursorHome => self.editor.active_mut().buffer.move_cursor_home(false),
            EditorAction::MoveCursorEnd => self.editor.active_mut().buffer.move_cursor_end(false),
            EditorAction::MoveCursorFileStart => self
                .editor
                .active_mut()
                .buffer
                .move_cursor_file_start(false),
            EditorAction::MoveCursorFileEnd => {
                self.editor.active_mut().buffer.move_cursor_file_end(false)
            }
            EditorAction::MoveCursorPage(dir) => {
                let lines = text_h.max(1);
                match dir {
                    Direction::Up => {
                        for _ in 0..lines {
                            self.editor.active_mut().buffer.move_cursor_up(false);
                        }
                    }
                    Direction::Down => {
                        for _ in 0..lines {
                            self.editor.active_mut().buffer.move_cursor_down(false);
                        }
                    }
                    _ => {}
                }
            }

            // ── Selection ─────────────────────────────────────────────
            EditorAction::ExtendSelection(dir) => match dir {
                Direction::Left => self.editor.active_mut().buffer.move_cursor_left(true),
                Direction::Right => self.editor.active_mut().buffer.move_cursor_right(true),
                Direction::Up => self.editor.active_mut().buffer.move_cursor_up(true),
                Direction::Down => self.editor.active_mut().buffer.move_cursor_down(true),
            },
            EditorAction::ExtendSelectionWord(dir) => match dir {
                Direction::Left => self.editor.active_mut().buffer.move_cursor_word_left(true),
                Direction::Right => self.editor.active_mut().buffer.move_cursor_word_right(true),
                _ => {}
            },
            EditorAction::ExtendSelectionHome => {
                self.editor.active_mut().buffer.move_cursor_home(true)
            }
            EditorAction::ExtendSelectionEnd => {
                self.editor.active_mut().buffer.move_cursor_end(true)
            }
            EditorAction::ExtendSelectionFileStart => {
                self.editor.active_mut().buffer.move_cursor_file_start(true)
            }
            EditorAction::ExtendSelectionFileEnd => {
                self.editor.active_mut().buffer.move_cursor_file_end(true)
            }
            EditorAction::ExtendSelectionPage(dir) => {
                let lines = text_h.max(1);
                match dir {
                    Direction::Up => {
                        for _ in 0..lines {
                            self.editor.active_mut().buffer.move_cursor_up(true);
                        }
                    }
                    Direction::Down => {
                        for _ in 0..lines {
                            self.editor.active_mut().buffer.move_cursor_down(true);
                        }
                    }
                    _ => {}
                }
            }
            EditorAction::SelectAll => self.editor.active_mut().buffer.select_all(),

            // ── Mouse ─────────────────────────────────────────────────
            EditorAction::MouseClick { col, row } => {
                if let Some(idx) = self.tab_bar_tab_at(col, row) {
                    self.editor.go_to_tab(idx);
                    self.sidebar_focused = false;
                } else if self.point_on_separator(col, row) {
                    // Begin a sidebar resize drag.
                    self.sidebar_drag = Some(SidebarDrag {
                        start_col: col,
                        start_width: self.sidebar_width,
                    });
                } else if self.point_in_sidebar(col, row) {
                    if let Some(idx) = self.sidebar_entry_at(row) {
                        self.sidebar_focused = true;
                        let h = self.sidebar_area.map(|r| r.height as usize).unwrap_or(0);
                        let selected = if let Some(sb) = &mut self.sidebar {
                            sb.selected = idx;
                            sb.ensure_selected_visible(h);
                            sb.entries
                                .get(sb.selected)
                                .map(|e| (e.path.clone(), e.is_dir))
                        } else {
                            None
                        };
                        if let Some((path, is_dir)) = selected {
                            if is_dir {
                                if let Some(sb) = &mut self.sidebar {
                                    sb.toggle_selected();
                                }
                            } else {
                                let _ = self.editor.open_tab(path);
                                self.after_file_open_or_save();
                                // Single click on a file moves focus to the editor.
                                self.sidebar_focused = false;
                            }
                        }
                    }
                } else {
                    // Click in the editor area: defocus the sidebar (if focused)
                    // and move the cursor to the click target.
                    self.sidebar_focused = false;
                    if let Some(offset) = self.screen_to_byte(col, row) {
                        self.editor
                            .active_mut()
                            .buffer
                            .move_cursor_to(offset, false);
                    }
                }
            }
            EditorAction::MouseDrag { col, row } => {
                if let Some(drag) = self.sidebar_drag {
                    // Resize the sidebar based on cursor delta from drag start.
                    let max = (self.term_width as i32 / 2).min(MAX_SIDEBAR_WIDTH as i32);
                    let min = MIN_SIDEBAR_WIDTH as i32;
                    let delta = col as i32 - drag.start_col as i32;
                    let new_w = (drag.start_width as i32 + delta).clamp(min, max.max(min)) as u16;
                    self.sidebar_width = new_w;
                } else if let Some(offset) = self.screen_to_byte(col, row) {
                    self.editor.active_mut().buffer.move_cursor_to(offset, true);
                }
            }
            EditorAction::MouseUp { .. } => {
                // End any in-progress separator drag. No editor action required.
                self.sidebar_drag = None;
            }
            EditorAction::BoxDragStart { col, row } => {
                if let Some((line, dcol)) = self.screen_to_line_col(col, row) {
                    self.box_drag_anchor = Some((line, dcol));
                    // Collapse any existing multi-cursor; start a new box.
                    self.editor.active_mut().buffer.collapse_cursors();
                    self.editor
                        .active_mut()
                        .buffer
                        .set_box_cursors(line, dcol, line, dcol);
                }
            }
            EditorAction::BoxDragUpdate { col, row } => {
                if let (Some((al, ac)), Some((cl, cc))) =
                    (self.box_drag_anchor, self.screen_to_line_col(col, row))
                {
                    self.editor
                        .active_mut()
                        .buffer
                        .set_box_cursors(al, ac, cl, cc);
                }
            }
            EditorAction::BoxDragEnd { .. } => {
                self.box_drag_anchor = None;
            }
            EditorAction::BoxSelectExtend(dir) => {
                self.editor.active_mut().buffer.extend_box_selection(dir);
            }
            EditorAction::FilterSelection => {
                if self.config.disable_shell_filter {
                    self.status_error = Some("Shell filter disabled by config".to_string());
                } else if self.selected_text().is_some() {
                    self.input_mode = InputMode::ShellFilter(String::new());
                } else {
                    self.status_error = Some("Filter requires a non-empty selection".to_string());
                }
            }

            // ── Line transforms ───────────────────────────────────────
            EditorAction::SortLinesAsc => {
                self.editor.active_mut().buffer.sort_lines(false);
            }
            EditorAction::SortLinesDesc => {
                self.editor.active_mut().buffer.sort_lines(true);
            }
            EditorAction::DedupeLines => {
                self.editor.active_mut().buffer.dedupe_lines();
            }
            EditorAction::ReverseLines => {
                self.editor.active_mut().buffer.reverse_lines();
            }
            EditorAction::ToUpper => {
                self.editor.active_mut().buffer.uppercase_selection();
            }
            EditorAction::ToLower => {
                self.editor.active_mut().buffer.lowercase_selection();
            }
            EditorAction::ToTitle => {
                self.editor.active_mut().buffer.titlecase_selection();
            }
            EditorAction::TrimTrailingWhitespace => {
                self.editor.active_mut().buffer.trim_trailing_whitespace();
            }
            EditorAction::JoinLines => {
                self.editor.active_mut().buffer.join_lines();
            }
            EditorAction::IncrementNumber => {
                self.editor.active_mut().buffer.increment_number(1);
            }
            EditorAction::DecrementNumber => {
                self.editor.active_mut().buffer.increment_number(-1);
            }
            EditorAction::ConvertIndentToSpaces => {
                let width = self.config.tab_size.max(1);
                self.editor
                    .active_mut()
                    .buffer
                    .convert_indent_to_spaces(width);
            }
            EditorAction::ConvertIndentToTabs => {
                let width = self.config.tab_size.max(1);
                self.editor
                    .active_mut()
                    .buffer
                    .convert_indent_to_tabs(width);
            }
            EditorAction::ConvertEolLf => {
                self.editor
                    .active_mut()
                    .buffer
                    .convert_eol(crate::buffer::EolStyle::Lf);
            }
            EditorAction::ConvertEolCrlf => {
                self.editor
                    .active_mut()
                    .buffer
                    .convert_eol(crate::buffer::EolStyle::Crlf);
            }
            EditorAction::AlignSelection => {
                self.input_mode = InputMode::AlignChar(String::new());
            }
            EditorAction::MouseScroll { dir, col, row } => {
                if self.point_in_sidebar(col, row) {
                    let h = self.sidebar_area.map(|r| r.height as usize).unwrap_or(0);
                    if let Some(sb) = &mut self.sidebar {
                        let delta: isize = match dir {
                            ScrollDir::Up => -(SCROLL_LINES as isize),
                            ScrollDir::Down => SCROLL_LINES as isize,
                            _ => 0,
                        };
                        sb.scroll_by(delta, h);
                    }
                } else {
                    // Route to the editor scroll just like a keyboard scroll.
                    let total_lines = self.editor.active().buffer.len_lines();
                    let vp = &mut self.editor.active_mut().viewport;
                    match dir {
                        ScrollDir::Up => {
                            vp.scroll_row = vp.scroll_row.saturating_sub(SCROLL_LINES);
                        }
                        ScrollDir::Down => {
                            vp.scroll_row =
                                (vp.scroll_row + SCROLL_LINES).min(total_lines.saturating_sub(1));
                        }
                        _ => {}
                    }
                }
            }

            // ── Scroll ────────────────────────────────────────────────
            EditorAction::Scroll(dir) => {
                let total_lines = self.editor.active().buffer.len_lines();
                let vp = &mut self.editor.active_mut().viewport;
                match dir {
                    ScrollDir::Up => {
                        vp.scroll_row = vp.scroll_row.saturating_sub(SCROLL_LINES);
                    }
                    ScrollDir::Down => {
                        vp.scroll_row =
                            (vp.scroll_row + SCROLL_LINES).min(total_lines.saturating_sub(1));
                    }
                    ScrollDir::Left => {
                        vp.scroll_col = vp.scroll_col.saturating_sub(4);
                    }
                    ScrollDir::Right => {
                        vp.scroll_col += 4;
                    }
                    ScrollDir::HalfPageUp => {
                        vp.scroll_row = vp.scroll_row.saturating_sub(text_h / 2);
                    }
                    ScrollDir::HalfPageDown => {
                        vp.scroll_row =
                            (vp.scroll_row + text_h / 2).min(total_lines.saturating_sub(1));
                    }
                }
            }
            EditorAction::ScrollCursorCenter => {
                let cursor_line = self.editor.active().buffer.cursors.primary().line;
                let half = text_h / 2;
                let vp = &mut self.editor.active_mut().viewport;
                vp.scroll_row = cursor_line.saturating_sub(half);
            }

            // ── Edit ops ──────────────────────────────────────────────
            EditorAction::Undo => {
                self.editor.active_mut().buffer.undo();
            }
            EditorAction::Redo => {
                self.editor.active_mut().buffer.redo();
            }
            EditorAction::DuplicateLine => {
                self.editor.active_mut().buffer.duplicate_line();
            }
            EditorAction::MoveLineUp => {
                self.editor.active_mut().buffer.move_line_up();
            }
            EditorAction::MoveLineDown => {
                self.editor.active_mut().buffer.move_line_down();
            }

            // ── File / tab management ─────────────────────────────────
            EditorAction::NewFile => {
                self.editor.new_tab();
            }
            EditorAction::NewTab => {
                self.editor.new_tab();
            }
            EditorAction::CloseTab => {
                self.close_tab();
            }
            EditorAction::NextTab => {
                self.editor.next_tab();
            }
            EditorAction::PrevTab => {
                self.editor.prev_tab();
            }
            EditorAction::GoToTab(n) => {
                self.editor.go_to_tab(n);
            }
            EditorAction::SaveFile => {
                self.save_active();
            }
            EditorAction::SaveFileAs => {
                self.input_mode = InputMode::SaveAsPath(String::new());
            }
            EditorAction::OpenFile => {
                self.input_mode = InputMode::OpenFilePath(String::new());
            }
            EditorAction::JumpToLine => {
                self.input_mode = InputMode::JumpToLine(String::new());
            }
            EditorAction::OpenFuzzyPicker => {
                self.fuzzy_picker = Some(FuzzyPickerState::new(
                    self.config.hide_git_folder,
                    self.config.hide_dot_folders,
                ));
            }
            EditorAction::OpenSymbolPicker => {
                let active = self.editor.active();
                let symbols = active.syntax.collect_symbols(active.buffer.rope());
                if symbols.is_empty() {
                    self.status_error = Some("No symbols found in this buffer".to_string());
                } else {
                    self.symbol_picker = Some(SymbolPickerState::new(symbols));
                }
            }
            EditorAction::ToggleFoldAtCursor => {
                let active = self.editor.active_mut();
                let line = active.buffer.cursors.primary().line;
                if !active.folds.toggle_at_line(line) {
                    self.status_error = Some("No fold at cursor".to_string());
                }
            }
            EditorAction::FoldAll => {
                self.editor.active_mut().folds.fold_all();
            }
            EditorAction::UnfoldAll => {
                self.editor.active_mut().folds.unfold_all();
            }
            EditorAction::JumpListBack => {
                self.push_current_to_jump_list();
                if let Some(entry) = self.jumps.back() {
                    self.go_to_jump_entry(&entry);
                }
            }
            EditorAction::JumpListForward => {
                if let Some(entry) = self.jumps.forward() {
                    self.go_to_jump_entry(&entry);
                }
            }
            EditorAction::BeginSetMark => {
                self.input_mode = InputMode::SetMarkChar;
            }
            EditorAction::BeginJumpToMark => {
                self.input_mode = InputMode::JumpToMarkChar;
            }
            EditorAction::ExpandSnippetAtCursor => {
                self.expand_snippet_at_cursor();
            }
            EditorAction::SnippetNextStop => {
                self.snippet_advance(true);
            }
            EditorAction::SnippetPrevStop => {
                self.snippet_advance(false);
            }
            EditorAction::SnippetCancel => {
                self.editor.active_mut().snippet_session = None;
            }
            EditorAction::BeginRecordMacro => {
                if self.macros.recording_slot().is_some() {
                    if let Some(slot) = self.macros.stop_recording() {
                        self.status_error = Some(format!("Recorded macro '{slot}'"));
                    }
                } else {
                    self.input_mode = InputMode::RecordMacroChar;
                }
            }
            EditorAction::StopRecordMacro => {
                if let Some(slot) = self.macros.stop_recording() {
                    self.status_error = Some(format!("Recorded macro '{slot}'"));
                }
            }
            EditorAction::BeginReplayMacro => {
                self.input_mode = InputMode::ReplayMacroChar;
            }
            EditorAction::ToggleSidebar => {
                if self.sidebar.is_none() {
                    // Restore saved state or create fresh, then expand to current file.
                    let mut sb = self.saved_sidebar.take().unwrap_or_else(SidebarState::new);
                    if let Some(path) = self.editor.active().path.clone() {
                        sb.expand_to_path(&path);
                    }
                    self.sidebar = Some(sb);
                    self.sidebar_focused = true;
                } else {
                    // Save state and close.
                    self.saved_sidebar = self.sidebar.take();
                    self.sidebar_focused = false;
                }
            }
            EditorAction::FocusSidebar => {
                if self.sidebar.is_none() {
                    // Open and focus.
                    let mut sb = self.saved_sidebar.take().unwrap_or_else(SidebarState::new);
                    if let Some(path) = self.editor.active().path.clone() {
                        sb.expand_to_path(&path);
                    }
                    self.sidebar = Some(sb);
                }
                self.sidebar_focused = true;
            }
            EditorAction::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.help_scroll = 0;
                }
            }
            EditorAction::OpenSettings => {
                self.show_settings = !self.show_settings;
                if self.show_settings {
                    self.settings_cursor = 0;
                }
            }
            EditorAction::OpenLspConfig => {
                if self.lsp_picker.is_some() {
                    self.lsp_picker = None;
                } else {
                    self.lsp_picker = Some(LspPickerState::new(&self.lsp_config));
                }
            }
            EditorAction::OpenGitDialog => {
                self.open_git_dialog();
            }
            EditorAction::TriggerCompletion => {
                self.trigger_completion();
            }
            EditorAction::ShowHover => {
                self.trigger_hover();
            }
            EditorAction::GoToDefinition => {
                self.push_current_to_jump_list();
                self.trigger_go_to_definition();
            }
            EditorAction::FindReferences => {
                self.trigger_find_references();
            }
            EditorAction::RenameSymbol => {
                self.trigger_rename();
            }
            EditorAction::CodeAction => {
                self.trigger_code_action();
            }
            EditorAction::LspRestart => {
                self.lsp_restart();
            }
            EditorAction::LspStop => {
                self.lsp = None;
                // Clear diagnostics and semantic tokens from all buffers.
                for tab in &mut self.editor.tabs {
                    tab.lsp_state.diagnostics.clear();
                    tab.lsp_state.semantic_tokens = None;
                }
            }
            EditorAction::ToggleLineComment => {
                self.toggle_line_comment();
            }
            EditorAction::ToggleWordWrap => {
                let vp = &mut self.editor.active_mut().viewport;
                vp.word_wrap = !vp.word_wrap;
                if vp.word_wrap {
                    vp.scroll_col = 0;
                }
            }
            // ── Column-edit multi-cursor ──────────────────────────────
            EditorAction::SpawnCursorUp => {
                let (top_line, display_col) = {
                    let cursors = &self.editor.active().buffer.cursors;
                    let top = cursors.cursors().iter().map(|c| c.line).min().unwrap_or(0);
                    let dcol = cursors.primary().preferred_col;
                    (top, dcol)
                };
                if top_line > 0 {
                    self.editor
                        .active_mut()
                        .buffer
                        .add_cursor_at_display_col(top_line - 1, display_col);
                }
            }
            EditorAction::SpawnCursorDown => {
                let (bottom_line, display_col) = {
                    let cursors = &self.editor.active().buffer.cursors;
                    let bot = cursors.cursors().iter().map(|c| c.line).max().unwrap_or(0);
                    let dcol = cursors.primary().preferred_col;
                    (bot, dcol)
                };
                let total_lines = self.editor.active().buffer.len_lines();
                if bottom_line + 1 < total_lines {
                    self.editor
                        .active_mut()
                        .buffer
                        .add_cursor_at_display_col(bottom_line + 1, display_col);
                }
            }

            EditorAction::OpenCommandPalette => {
                self.command_palette = Some(CommandPaletteState::new());
            }
            EditorAction::OpenBufferSwitcher => {
                self.fuzzy_picker =
                    Some(FuzzyPickerState::from_buffers(self.editor.buffer_names()));
            }
            EditorAction::OpenRecentFiles => {
                let files = load_recent_files(&self.workspace);
                self.fuzzy_picker = Some(FuzzyPickerState::from_paths(files));
            }
            EditorAction::ReloadConfig => {
                self.config = Config::load();
                self.project_fmt = crate::formatting::project::load(&self.workspace);
                self.input.reload_keybindings();
                for tab in &mut self.editor.tabs {
                    tab.viewport.word_wrap = self.config.word_wrap;
                }
            }

            // ── Search ────────────────────────────────────────────────
            EditorAction::OpenSearch => {
                self.search_state = Some(SearchState::new(false));
                self.recompute_search_and_jump();
            }
            EditorAction::OpenReplace => {
                self.search_state = Some(SearchState::new(true));
                self.recompute_search_and_jump();
            }
            EditorAction::SearchNext => self.search_next(),
            EditorAction::SearchPrev => self.search_prev(),
            EditorAction::CloseSearch => {
                if self.editor.active().snippet_session.is_some() {
                    self.editor.active_mut().snippet_session = None;
                    return;
                }
                self.search_state = None;
                // Esc also collapses column-edit multi-cursor when search is not open.
                if self.editor.active().buffer.cursors.is_multi() {
                    self.editor.active_mut().buffer.collapse_cursors();
                }
            }
            EditorAction::SearchReplaceOne => self.replace_current(),
            EditorAction::SearchReplaceAll => self.replace_all(),
            EditorAction::SearchToggleRegex | EditorAction::SearchToggleCaseSensitive => {}
            EditorAction::SelectAllOccurrences => self.select_all_occurrences(),
            EditorAction::OpenProjectSearch => {
                self.project_search = Some(ProjectSearchState::new());
            }
            EditorAction::AddCursorNextMatch => {
                self.editor.active_mut().buffer.add_cursor_at_next_match();
            }
            EditorAction::SkipCurrentMatch => {
                self.editor.active_mut().buffer.skip_current_match_to_next();
            }
            EditorAction::UndoLastCursor => {
                self.editor.active_mut().buffer.pop_last_cursor();
            }

            // ── App lifecycle ─────────────────────────────────────────
            EditorAction::Quit => {
                if self.editor.active().buffer.modified && self.config.confirm_exit {
                    self.confirm_quit = true;
                } else {
                    self.should_quit = true;
                }
            }
            EditorAction::ForceQuit => {
                self.should_quit = true;
            }
            EditorAction::SidebarRename
            | EditorAction::SidebarNewFolder
            | EditorAction::SidebarRefresh
            | EditorAction::Unhandled => {}
        }

        // Dismiss hover on any action.
        self.hover = None;

        // Forward any buffer edits made this action to marks/jumps so their
        // byte offsets keep tracking after inserts, deletes, and replaces.
        // Drain regardless of `modified` so the queue never grows unbounded.
        let pending = self.editor.active_mut().buffer.drain_pending_edits();
        if !pending.is_empty() {
            if let Some(path) = self.editor.active().path.clone() {
                for cmd in &pending {
                    self.marks.rebase_after_edit(&path, cmd);
                    self.jumps.rebase_after_edit(&path, cmd);
                }
                self.marks.save(&self.workspace);
                self.jumps.save(&self.workspace);
            }
            if let Some(session) = self.editor.active_mut().snippet_session.as_mut() {
                for cmd in &pending {
                    session.rebase(cmd);
                }
                if session.is_empty() {
                    self.editor.active_mut().snippet_session = None;
                }
            }
        }

        // Re-parse the active buffer if it was modified this action.
        if self.editor.active().buffer.modified {
            self.editor.active_mut().reparse();
            // Bump the version immediately but defer the actual didChange send
            // until the debounce timer fires (avoids full-buffer copy per keystroke).
            self.editor.active_mut().lsp_state.version += 1;
            self.lsp_dirty_since = Some(Instant::now());
            self.lsp_change_sent = false;
            // Invalidate semantic tokens (re-requested after debounce).
            self.editor.active_mut().lsp_state.semantic_tokens = None;

            // Re-filter completion popup if open.
            if self.completion.is_some() {
                self.refilter_completion();
            }
        }

        // Reset the auto-save debounce timer only when buffer content actually changed.
        if self.config.auto_save {
            let post_undo_depth = self.editor.active().buffer.undo_depth();
            if self.editor.active().buffer.modified && pre_undo_depth != post_undo_depth {
                self.auto_save_timer = Some(std::time::Instant::now());
            } else if !self.editor.active().buffer.modified {
                // Undo back to saved state — nothing left to auto-save.
                self.auto_save_timer = None;
            }
        }
    }

    /// Run `command` (via `sh -c`) with the current selection on stdin and
    /// replace the selection with the captured stdout. Single undo entry.
    fn apply_shell_filter(&mut self, command: &str) {
        if self.config.disable_shell_filter {
            self.status_error = Some("Shell filter disabled by config".to_string());
            return;
        }
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return;
        }
        let buf = &self.editor.active().buffer;
        let primary = buf.cursors.primary();
        let range = match primary.selection {
            Some(s) => s.as_byte_range(),
            None => return,
        };
        if range.is_empty() {
            return;
        }
        let selection_text = {
            let rope = buf.rope();
            let cs = rope.byte_to_char(range.start);
            let ce = rope.byte_to_char(range.end);
            rope.slice(cs..ce).to_string()
        };

        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = match Command::new("sh")
            .arg("-c")
            .arg(trimmed)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                self.status_error = Some(format!("filter spawn failed: {e}"));
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(selection_text.as_bytes());
            // Drop stdin to signal EOF to the child.
        }
        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                self.status_error = Some(format!("filter wait failed: {e}"));
                return;
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed_err = stderr.trim();
            self.status_error = Some(if trimmed_err.is_empty() {
                format!("filter exited {}", output.status)
            } else {
                format!("filter: {trimmed_err}")
            });
            return;
        }
        let new_text = String::from_utf8_lossy(&output.stdout).to_string();
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.move_cursor_to(range.start, false);
        buf.move_cursor_to(range.end, true);
        buf.insert_str(&new_text);
        buf.commit_batch();
    }

    // ── Modal input handling ─────────────────────────────────────────────────

    fn handle_modal_input(&mut self, action: EditorAction) {
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
                _ => {}
            }
        }
        // Mutate the input string for typing/backspace without accessing other fields.
        match action {
            EditorAction::InsertChar(c) => {
                match &mut self.input_mode {
                    InputMode::JumpToLine(s) if c.is_ascii_digit() || c == ':' => {
                        s.push(c);
                    }
                    InputMode::OpenFilePath(s)
                    | InputMode::SaveAsPath(s)
                    | InputMode::RenamePath(_, s)
                    | InputMode::NewFolderName(_, s)
                    | InputMode::Rename(s)
                    | InputMode::GitCommitMessage(s)
                    | InputMode::GitNewBranch(s)
                    | InputMode::GitStashMessage(s)
                    | InputMode::ShellFilter(s)
                    | InputMode::AlignChar(s) => {
                        s.push(c);
                    }
                    _ => {}
                }
                return;
            }
            EditorAction::DeleteBackward => {
                match &mut self.input_mode {
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
                    | InputMode::AlignChar(s) => {
                        s.pop();
                    }
                    InputMode::Normal
                    | InputMode::SetMarkChar
                    | InputMode::JumpToMarkChar
                    | InputMode::RecordMacroChar
                    | InputMode::ReplayMacroChar => {}
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
                    | InputMode::ReplayMacroChar => {}
                }
            }
            EditorAction::Quit | EditorAction::Unhandled => {
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
    }

    // ── Fuzzy picker input handling ──────────────────────────────────────────

    fn handle_fuzzy_picker(&mut self, action: EditorAction) {
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

    fn handle_symbol_picker(&mut self, action: EditorAction) {
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

    // ── Project search overlay input handling ────────────────────────────────

    /// Handle keyboard input while the project-search overlay is active.
    /// Returns `true` when the action was consumed (do not fall through to the
    /// editor). Global actions like Quit/ToggleHelp fall through.
    fn handle_project_search(&mut self, action: EditorAction) -> bool {
        match action {
            EditorAction::InsertChar(c) => {
                if let Some(ps) = &mut self.project_search {
                    if ps.focus_replace {
                        ps.replace_text.push(c);
                    } else {
                        ps.query.push(c);
                    }
                }
                if !self
                    .project_search
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false)
                {
                    self.recompute_project_search();
                }
                true
            }
            EditorAction::DeleteBackward => {
                if let Some(ps) = &mut self.project_search {
                    if ps.focus_replace {
                        ps.replace_text.pop();
                    } else {
                        ps.query.pop();
                    }
                }
                if !self
                    .project_search
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false)
                {
                    self.recompute_project_search();
                }
                true
            }
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(ps) = &mut self.project_search {
                    ps.move_up();
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(ps) = &mut self.project_search {
                    ps.move_down();
                }
                true
            }
            EditorAction::InsertTab => {
                // Tab toggles focus and reveals the replace input.
                if let Some(ps) = &mut self.project_search {
                    ps.show_replace = true;
                    ps.focus_replace = !ps.focus_replace;
                }
                true
            }
            EditorAction::SearchToggleRegex => {
                if let Some(ps) = &mut self.project_search {
                    ps.is_regex = !ps.is_regex;
                }
                self.recompute_project_search();
                true
            }
            EditorAction::SearchToggleCaseSensitive => {
                if let Some(ps) = &mut self.project_search {
                    ps.case_sensitive = !ps.case_sensitive;
                }
                self.recompute_project_search();
                true
            }
            EditorAction::InsertNewline => {
                let target = self
                    .project_search
                    .as_ref()
                    .and_then(|ps| ps.results.matches.get(ps.selected).cloned());
                if let Some(m) = target {
                    let abs = self.workspace.join(&m.path);
                    self.project_search = None;
                    let _ = self.editor.open_tab(abs);
                    self.after_file_open_or_save();
                    // Move cursor to the start of the match line. Computing the
                    // exact column requires the file's full byte→line table,
                    // which we don't cache; landing on the line is the
                    // standard "go to result" behaviour.
                    let buf = &mut self.editor.active_mut().buffer;
                    let rope = buf.rope();
                    let line_idx = m.line.min(rope.len_lines().saturating_sub(1));
                    let line_start_char = rope.line_to_char(line_idx);
                    let target_byte = rope.char_to_byte(line_start_char);
                    buf.move_cursor_to(target_byte, false);
                }
                true
            }
            EditorAction::SearchReplaceAll => {
                self.project_replace_all();
                true
            }
            EditorAction::CloseSearch | EditorAction::Quit | EditorAction::Unhandled => {
                self.project_search = None;
                true
            }
            // Global actions that should still work while the overlay is open
            // (toggle help, scroll cursor, etc.).
            EditorAction::ToggleHelp => false,
            _ => true,
        }
    }

    fn recompute_project_search(&mut self) {
        let (query, is_regex, case_sensitive) = match &self.project_search {
            Some(ps) => (ps.query.clone(), ps.is_regex, ps.case_sensitive),
            None => return,
        };
        let results = crate::search::project::run(
            &self.workspace,
            &query,
            is_regex,
            case_sensitive,
            self.config.hide_git_folder,
            self.config.hide_dot_folders,
        );
        if let Some(ps) = &mut self.project_search {
            ps.results = results;
            ps.selected = 0;
        }
    }

    fn project_replace_all(&mut self) {
        let (query, replacement, is_regex, case_sensitive) = match &self.project_search {
            Some(ps) if !ps.query.is_empty() && ps.show_replace => (
                ps.query.clone(),
                ps.replace_text.clone(),
                ps.is_regex,
                ps.case_sensitive,
            ),
            _ => return,
        };

        // Collect distinct file paths from results.
        let paths: Vec<PathBuf> = {
            let ps = match &self.project_search {
                Some(p) => p,
                None => return,
            };
            let mut seen = std::collections::HashSet::new();
            ps.results
                .matches
                .iter()
                .filter_map(|m| {
                    let abs = self.workspace.join(&m.path);
                    if seen.insert(abs.clone()) {
                        Some(abs)
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut total_replaced = 0usize;
        for path in &paths {
            // If the file is currently open in a tab, edit it through the
            // buffer so undo history is preserved.
            let open_idx = self
                .editor
                .tabs
                .iter()
                .position(|t| t.path.as_deref() == Some(path.as_path()));
            if let Some(idx) = open_idx {
                let prev_active = self.editor.active_idx;
                self.editor.go_to_tab(idx);
                let buf = &mut self.editor.active_mut().buffer;
                let text = buf.to_string();
                let pattern = crate::search::build_pattern(&query, is_regex, case_sensitive);
                let re = match regex::Regex::new(&pattern) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let ranges: Vec<(usize, usize)> =
                    re.find_iter(&text).map(|m| (m.start(), m.end())).collect();
                if ranges.is_empty() {
                    self.editor.go_to_tab(prev_active);
                    continue;
                }
                buf.begin_batch();
                for (start, end) in ranges.iter().rev() {
                    buf.move_cursor_to(*start, false);
                    buf.move_cursor_to(*end, true);
                    buf.insert_str(&replacement);
                }
                buf.commit_batch();
                total_replaced += ranges.len();
                self.editor.go_to_tab(prev_active);
            } else {
                // Edit on disk.
                if let Ok(n) = crate::search::project::replace_all_in_file(
                    path,
                    &query,
                    is_regex,
                    case_sensitive,
                    &replacement,
                ) {
                    total_replaced += n;
                }
            }
        }

        // Refresh result list and surface a status message.
        self.recompute_project_search();
        self.status_error = Some(format!("Replaced {total_replaced} occurrence(s)"));
    }

    // ── Search input handling ────────────────────────────────────────────────

    /// Handle keyboard input while the search bar is active.
    /// Returns `true` if the action was consumed (should not be processed further).
    fn handle_search_input(&mut self, action: EditorAction) -> bool {
        match &action {
            // Navigation: let these fall through to the normal dispatch.
            EditorAction::MoveCursor(_)
            | EditorAction::MoveCursorWord(_)
            | EditorAction::MoveCursorHome
            | EditorAction::MoveCursorEnd
            | EditorAction::MoveCursorFileStart
            | EditorAction::MoveCursorFileEnd
            | EditorAction::MoveCursorPage(_)
            | EditorAction::Scroll(_)
            | EditorAction::ScrollCursorCenter
            | EditorAction::GoToMatchingBracket
            | EditorAction::MouseClick { .. }
            | EditorAction::MouseDrag { .. } => return false,

            _ => {}
        }

        match action {
            EditorAction::InsertChar(c) => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false);
                if focus_replace {
                    if let Some(ss) = &mut self.search_state {
                        ss.replace_text.push(c);
                    }
                } else {
                    if let Some(ss) = &mut self.search_state {
                        ss.query.push(c);
                    }
                    self.recompute_search_and_jump();
                }
            }
            EditorAction::DeleteBackward => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false);
                if focus_replace {
                    if let Some(ss) = &mut self.search_state {
                        ss.replace_text.pop();
                    }
                } else {
                    if let Some(ss) = &mut self.search_state {
                        ss.query.pop();
                    }
                    self.recompute_search_and_jump();
                }
            }
            EditorAction::InsertNewline => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace && s.show_replace)
                    .unwrap_or(false);
                if focus_replace {
                    self.replace_current();
                } else {
                    self.search_next();
                }
            }
            EditorAction::InsertTab => {
                // Toggle focus between query and replace fields.
                if let Some(ss) = &mut self.search_state
                    && ss.show_replace
                {
                    ss.focus_replace = !ss.focus_replace;
                }
            }
            EditorAction::SearchNext => self.search_next(),
            EditorAction::SearchPrev => self.search_prev(),
            EditorAction::SearchReplaceOne => self.replace_current(),
            EditorAction::SearchReplaceAll => self.replace_all(),
            EditorAction::SearchToggleRegex => {
                if let Some(ss) = &mut self.search_state {
                    ss.is_regex = !ss.is_regex;
                }
                self.recompute_search_and_jump();
            }
            EditorAction::SearchToggleCaseSensitive => {
                if let Some(ss) = &mut self.search_state {
                    ss.case_sensitive = !ss.case_sensitive;
                }
                self.recompute_search_and_jump();
            }
            EditorAction::OpenReplace => {
                if let Some(ss) = &mut self.search_state {
                    ss.show_replace = true;
                    ss.focus_replace = true;
                }
            }
            EditorAction::CloseSearch | EditorAction::Quit => {
                self.search_state = None;
            }
            EditorAction::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.help_scroll = 0;
                }
            }
            _ => {}
        }
        true
    }

    // ── Search helpers ────────────────────────────────────────────────────────

    fn recompute_search_and_jump(&mut self) {
        if self.search_state.is_none() {
            return;
        }
        let text = self.editor.active().buffer.to_string();
        let cursor_offset = self.editor.active().buffer.cursors.primary().byte_offset;
        if let Some(ss) = &mut self.search_state {
            ss.recompute_matches(&text);
            ss.jump_to_nearest(cursor_offset);
        }
        self.select_current_match();
    }

    fn search_next(&mut self) {
        if let Some(ss) = &mut self.search_state {
            ss.next_match();
        }
        self.select_current_match();
    }

    fn search_prev(&mut self) {
        if let Some(ss) = &mut self.search_state {
            ss.prev_match();
        }
        self.select_current_match();
    }

    fn select_current_match(&mut self) {
        let range = self.search_state.as_ref().and_then(|s| s.current_range());
        if let Some(r) = range {
            self.editor
                .active_mut()
                .buffer
                .move_cursor_to(r.start, false);
            self.editor.active_mut().buffer.move_cursor_to(r.end, true);
        }
    }

    fn replace_current(&mut self) {
        let range = self.search_state.as_ref().and_then(|s| s.current_range());
        let replace_text = self
            .search_state
            .as_ref()
            .map(|s| s.replace_text.clone())
            .unwrap_or_default();
        if let Some(r) = range {
            let buf = &mut self.editor.active_mut().buffer;
            buf.begin_batch();
            buf.move_cursor_to(r.start, false);
            buf.move_cursor_to(r.end, true);
            buf.insert_str(&replace_text);
            buf.commit_batch();
        }
        self.recompute_search_and_jump();
    }

    fn replace_all(&mut self) {
        let ranges: Vec<_> = self
            .search_state
            .as_ref()
            .map(|s| s.matches.clone())
            .unwrap_or_default();
        let replace_text = self
            .search_state
            .as_ref()
            .map(|s| s.replace_text.clone())
            .unwrap_or_default();
        if ranges.is_empty() {
            return;
        }

        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        // Replace in reverse order so earlier byte offsets remain valid.
        for r in ranges.iter().rev() {
            buf.move_cursor_to(r.start, false);
            buf.move_cursor_to(r.end, true);
            buf.insert_str(&replace_text);
        }
        buf.commit_batch();

        self.recompute_search_and_jump();
    }

    fn select_all_occurrences(&mut self) {
        // Use the current selection text as the search query (or keep existing query).
        if let Some(selected) = self.selected_text() {
            if self.search_state.is_none() {
                self.search_state = Some(SearchState::new(false));
            }
            if let Some(ss) = &mut self.search_state {
                ss.query = selected;
                ss.case_sensitive = true;
            }
        }
        self.recompute_search_and_jump();
    }

    // ── Help overlay input handling ───────────────────────────────────────────

    /// Persist `last_seen_version = current` so the same overlay isn't shown
    /// again until the next minor/major upgrade.
    fn record_version_seen(&mut self) {
        let v = env!("CARGO_PKG_VERSION").to_string();
        if self.config.last_seen_version.as_deref() != Some(v.as_str()) {
            self.config.last_seen_version = Some(v);
            self.config.save();
        }
    }

    /// Handle input while the welcome overlay is visible. Any "OK" key
    /// (Enter / Esc / F1 / Ctrl+Q-style ToggleHelp) dismisses it; arrows and
    /// the mouse wheel scroll its content. Returns `true` to consume.
    fn handle_welcome(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                self.welcome_scroll = self.welcome_scroll.saturating_sub(1);
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                self.welcome_scroll = self.welcome_scroll.saturating_add(1);
                true
            }
            EditorAction::MoveCursorPage(Direction::Up) => {
                self.welcome_scroll = self.welcome_scroll.saturating_sub(10);
                true
            }
            EditorAction::MoveCursorPage(Direction::Down) => {
                self.welcome_scroll = self.welcome_scroll.saturating_add(10);
                true
            }
            EditorAction::MouseScroll { dir, .. } => {
                match dir {
                    ScrollDir::Up => {
                        self.welcome_scroll = self.welcome_scroll.saturating_sub(SCROLL_LINES);
                    }
                    ScrollDir::Down => {
                        self.welcome_scroll = self.welcome_scroll.saturating_add(SCROLL_LINES);
                    }
                    _ => {}
                }
                true
            }
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
    fn handle_changelog(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                self.changelog_scroll = self.changelog_scroll.saturating_sub(1);
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                self.changelog_scroll = self.changelog_scroll.saturating_add(1);
                true
            }
            EditorAction::MoveCursorPage(Direction::Up) => {
                self.changelog_scroll = self.changelog_scroll.saturating_sub(10);
                true
            }
            EditorAction::MoveCursorPage(Direction::Down) => {
                self.changelog_scroll = self.changelog_scroll.saturating_add(10);
                true
            }
            EditorAction::MouseScroll { dir, .. } => {
                match dir {
                    ScrollDir::Up => {
                        self.changelog_scroll = self.changelog_scroll.saturating_sub(SCROLL_LINES);
                    }
                    ScrollDir::Down => {
                        self.changelog_scroll = self.changelog_scroll.saturating_add(SCROLL_LINES);
                    }
                    _ => {}
                }
                true
            }
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
    fn handle_help(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                self.help_scroll = self.help_scroll.saturating_add(1);
                true
            }
            EditorAction::MoveCursorPage(Direction::Up) => {
                self.help_scroll = self.help_scroll.saturating_sub(10);
                true
            }
            EditorAction::MoveCursorPage(Direction::Down) => {
                self.help_scroll = self.help_scroll.saturating_add(10);
                true
            }
            EditorAction::MoveCursorFileStart => {
                self.help_scroll = 0;
                true
            }
            EditorAction::MoveCursorFileEnd => {
                self.help_scroll = usize::MAX; // clamped in render
                true
            }
            EditorAction::MouseScroll { dir, .. } => {
                match dir {
                    ScrollDir::Up => {
                        self.help_scroll = self.help_scroll.saturating_sub(SCROLL_LINES);
                    }
                    ScrollDir::Down => {
                        self.help_scroll = self.help_scroll.saturating_add(SCROLL_LINES);
                    }
                    _ => {}
                }
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

    // ── Settings overlay input handling ──────────────────────────────────────

    /// Handle input while the settings overlay is open.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    fn handle_settings(&mut self, action: &EditorAction) -> bool {
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
    fn toggle_setting(&mut self, forward: bool) {
        match self.settings_cursor {
            0 => self.config.confirm_exit = !self.config.confirm_exit,
            1 => self.config.auto_save = !self.config.auto_save,
            2 => self.config.show_whitespace = !self.config.show_whitespace,
            3 => self.config.hide_git_folder = !self.config.hide_git_folder,
            4 => self.config.hide_dot_folders = !self.config.hide_dot_folders,
            5 => {
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
            6 => {
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

    // ── LSP picker input handling ────────────────────────────────────────────

    /// Handle input while the LSP config picker is open.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    fn handle_lsp_picker(&mut self, action: &EditorAction) -> bool {
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
    fn apply_lsp_picker_selection(&mut self) {
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
            let (name, command, args) = LSP_SERVER_OPTIONS[selected - 1];
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

    // ── Git operations dialog ────────────────────────────────────────────────

    fn open_git_dialog(&mut self) {
        if !crate::git::ops::is_repo(&self.workspace) {
            self.status_error = Some("Not a git repository".into());
            return;
        }
        self.git_dialog = Some(GitDialogState::new());
    }

    /// Drive the git dialog. Captures all input until `Esc` from the menu
    /// closes the overlay. Routes nav keys to `GitScreen::move_*`, and
    /// handles per-screen actions inline.
    fn handle_git_dialog(&mut self, action: EditorAction) {
        use crate::ui::git_dialog::{ConfirmOp, GitScreen, MenuItem};

        // Quit always closes the dialog without doing anything.
        if matches!(action, EditorAction::Quit | EditorAction::ForceQuit) {
            self.git_dialog = None;
            return;
        }

        // Esc / CloseSearch: step back; if no history, close the dialog.
        if matches!(action, EditorAction::CloseSearch) {
            let close = match self.git_dialog.as_mut() {
                Some(d) => !d.pop(),
                None => true,
            };
            if close {
                self.git_dialog = None;
            }
            return;
        }

        // Navigation keys are uniform across most screens.
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.move_up();
                    d.screen.scroll_by(-1);
                }
                return;
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.move_down();
                    d.screen.scroll_by(1);
                }
                return;
            }
            EditorAction::MoveCursorPage(Direction::Up) | EditorAction::Scroll(ScrollDir::Up) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.scroll_by(-5);
                }
                return;
            }
            EditorAction::MoveCursorPage(Direction::Down)
            | EditorAction::Scroll(ScrollDir::Down) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.scroll_by(5);
                }
                return;
            }
            _ => {}
        }

        // Per-screen handling. We snapshot the current screen kind so we can
        // borrow self mutably below for I/O without holding a borrow on
        // `self.git_dialog`.
        let screen_kind = match self.git_dialog.as_ref() {
            Some(d) => d.screen.clone(),
            None => return,
        };

        match (screen_kind, action) {
            // ── Menu ──
            (GitScreen::Menu { selected }, EditorAction::InsertNewline) => {
                let item = MenuItem::ALL[selected];
                self.git_open_menu_item(item);
            }

            // ── Stage ──
            (
                GitScreen::Stage { entries, .. },
                EditorAction::InsertChar(' ') | EditorAction::InsertChar('x'),
            ) => {
                if let Some(GitDialogState {
                    screen:
                        GitScreen::Stage {
                            checked, selected, ..
                        },
                    ..
                }) = self.git_dialog.as_mut()
                    && let Some(slot) = checked.get_mut(*selected)
                    && !entries.is_empty()
                {
                    *slot = !*slot;
                }
            }
            (
                GitScreen::Stage {
                    entries, checked, ..
                },
                EditorAction::InsertNewline,
            ) => {
                self.git_apply_stage(&entries, &checked);
            }

            // ── Branches ──
            (GitScreen::Branches { entries, selected }, EditorAction::InsertNewline) => {
                if let Some(branch) = entries.get(selected) {
                    if branch.current {
                        if let Some(d) = self.git_dialog.as_mut() {
                            d.set_error("Already on this branch");
                        }
                    } else {
                        let name = branch.name.clone();
                        match crate::git::ops::checkout(&self.workspace, &name) {
                            Ok(out) => {
                                self.git_after_branch_change();
                                self.set_git_output("Checkout", out);
                            }
                            Err(e) => self.set_git_error(e),
                        }
                    }
                }
            }
            (GitScreen::Branches { .. }, EditorAction::InsertChar('n')) => {
                self.input_mode = InputMode::GitNewBranch(String::new());
            }
            (GitScreen::Branches { entries, selected }, EditorAction::InsertChar('d')) => {
                if let Some(branch) = entries.get(selected) {
                    if branch.current {
                        if let Some(d) = self.git_dialog.as_mut() {
                            d.set_error("Cannot delete the current branch");
                        }
                    } else if let Some(d) = self.git_dialog.as_mut() {
                        let op = ConfirmOp::DeleteBranch(branch.name.clone());
                        d.push(GitScreen::Confirm { op });
                    }
                }
            }

            // ── Stashes ──
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertNewline) => {
                if let Some(s) = entries.get(selected) {
                    let idx = s.index;
                    match crate::git::ops::stash_apply(&self.workspace, idx) {
                        Ok(out) => self.set_git_output("Stash apply", out),
                        Err(e) => self.set_git_error(e),
                    }
                }
            }
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertChar('p')) => {
                if let Some(s) = entries.get(selected) {
                    let idx = s.index;
                    match crate::git::ops::stash_pop(&self.workspace, idx) {
                        Ok(out) => {
                            self.git_after_branch_change();
                            self.set_git_output("Stash pop", out);
                        }
                        Err(e) => self.set_git_error(e),
                    }
                }
            }
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertChar('d')) => {
                if let Some(s) = entries.get(selected)
                    && let Some(d) = self.git_dialog.as_mut()
                {
                    let op = ConfirmOp::DropStash(s.index);
                    d.push(GitScreen::Confirm { op });
                }
            }
            (GitScreen::Stashes { .. }, EditorAction::InsertChar('n')) => {
                self.input_mode = InputMode::GitStashMessage(String::new());
            }

            // ── Confirm (y/n) ──
            (GitScreen::Confirm { op }, EditorAction::InsertChar('y' | 'Y')) => {
                self.git_run_confirm(op);
            }
            (GitScreen::Confirm { .. }, EditorAction::InsertChar('n' | 'N')) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.pop();
                }
            }

            _ => {}
        }
    }

    fn git_open_menu_item(&mut self, item: crate::ui::git_dialog::MenuItem) {
        use crate::ui::git_dialog::{GitScreen, MenuItem};

        match item {
            MenuItem::Status => match crate::git::ops::status_summary(&self.workspace) {
                Ok(out) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        d.push(GitScreen::Status {
                            output: out,
                            scroll: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Stage => match crate::git::ops::status(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        let checked = vec![false; entries.len()];
                        d.push(GitScreen::Stage {
                            entries,
                            checked,
                            selected: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Commit => {
                self.input_mode = InputMode::GitCommitMessage(String::new());
            }
            MenuItem::Push => match crate::git::ops::push(&self.workspace) {
                Ok(out) => self.set_git_output("Push", out),
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Pull => match crate::git::ops::pull(&self.workspace) {
                Ok(out) => {
                    self.git_after_branch_change();
                    self.set_git_output("Pull", out);
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Branches => match crate::git::ops::branches(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        let selected = entries.iter().position(|b| b.current).unwrap_or(0);
                        d.push(GitScreen::Branches { entries, selected });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Stashes => match crate::git::ops::stashes(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        d.push(GitScreen::Stashes {
                            entries,
                            selected: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
        }
    }

    /// Apply staging based on the user's checked/unchecked selections.
    /// Files that are currently staged become `git reset` targets; files that
    /// are unstaged or untracked become `git add` targets.
    fn git_apply_stage(&mut self, entries: &[crate::git::ops::StatusEntry], checked: &[bool]) {
        let mut to_add: Vec<&std::path::Path> = Vec::new();
        let mut to_reset: Vec<&std::path::Path> = Vec::new();
        for (entry, &is_checked) in entries.iter().zip(checked.iter()) {
            if !is_checked {
                continue;
            }
            if entry.is_staged() {
                to_reset.push(&entry.path);
            } else {
                to_add.push(&entry.path);
            }
        }

        if to_add.is_empty() && to_reset.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Nothing selected");
            }
            return;
        }

        if let Err(e) = crate::git::ops::add(&self.workspace, &to_add) {
            self.set_git_error(e);
            return;
        }
        if let Err(e) = crate::git::ops::reset(&self.workspace, &to_reset) {
            self.set_git_error(e);
            return;
        }

        // Refresh the stage screen with new statuses.
        match crate::git::ops::status(&self.workspace) {
            Ok(entries) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    let checked = vec![false; entries.len()];
                    d.replace(crate::ui::git_dialog::GitScreen::Stage {
                        entries,
                        checked,
                        selected: 0,
                    });
                }
            }
            Err(e) => self.set_git_error(e),
        }
        self.refresh_git_gutter();
    }

    fn git_finish_commit(&mut self, message: &str) {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Empty commit message — cancelled");
            }
            return;
        }
        match crate::git::ops::commit(&self.workspace, trimmed) {
            Ok(out) => {
                self.set_git_output("Commit", out);
                self.refresh_git_gutter();
            }
            Err(e) => self.set_git_error(e),
        }
    }

    fn git_finish_new_branch(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Empty branch name — cancelled");
            }
            return;
        }
        match crate::git::ops::create_branch(&self.workspace, trimmed) {
            Ok(out) => {
                self.git_after_branch_change();
                self.set_git_output("Create branch", out);
            }
            Err(e) => self.set_git_error(e),
        }
    }

    fn git_finish_stash_push(&mut self, message: &str) {
        let trimmed = message.trim();
        let msg = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
        match crate::git::ops::stash_push(&self.workspace, msg) {
            Ok(out) => {
                self.git_after_branch_change();
                self.set_git_output("Stash push", out);
            }
            Err(e) => self.set_git_error(e),
        }
    }

    fn git_run_confirm(&mut self, op: crate::ui::git_dialog::ConfirmOp) {
        use crate::ui::git_dialog::ConfirmOp;
        // Pop the confirm screen first so the result lands on the prior screen.
        if let Some(d) = self.git_dialog.as_mut() {
            d.pop();
        }
        match op {
            ConfirmOp::DropStash(idx) => match crate::git::ops::stash_drop(&self.workspace, idx) {
                Ok(out) => {
                    // Refresh the stash list under us.
                    if let Ok(entries) = crate::git::ops::stashes(&self.workspace)
                        && let Some(d) = self.git_dialog.as_mut()
                    {
                        d.replace(crate::ui::git_dialog::GitScreen::Stashes {
                            entries,
                            selected: 0,
                        });
                    }
                    self.set_git_output("Stash drop", out);
                }
                Err(e) => self.set_git_error(e),
            },
            ConfirmOp::DeleteBranch(name) => {
                match crate::git::ops::delete_branch(&self.workspace, &name) {
                    Ok(out) => {
                        if let Ok(entries) = crate::git::ops::branches(&self.workspace)
                            && let Some(d) = self.git_dialog.as_mut()
                        {
                            let selected = entries.iter().position(|b| b.current).unwrap_or(0);
                            d.replace(crate::ui::git_dialog::GitScreen::Branches {
                                entries,
                                selected,
                            });
                        }
                        self.set_git_output("Delete branch", out);
                    }
                    Err(e) => self.set_git_error(e),
                }
            }
        }
    }

    fn set_git_output(&mut self, title: &str, body: String) {
        if let Some(d) = self.git_dialog.as_mut() {
            d.push(crate::ui::git_dialog::GitScreen::Output {
                title: title.into(),
                body,
                scroll: 0,
            });
        }
    }

    fn set_git_error(&mut self, err: String) {
        if let Some(d) = self.git_dialog.as_mut() {
            d.set_error(err);
        }
    }

    /// Called after operations that may change which file is on disk under
    /// the active buffer (checkout, pull, stash pop, new branch). Refreshes
    /// the gutter; the existing file watcher will pick up content changes.
    fn git_after_branch_change(&mut self) {
        self.refresh_git_gutter();
    }

    // ── Sidebar input handling ────────────────────────────────────────────────

    /// Handle input while the sidebar is focused.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    fn handle_sidebar_input(&mut self, action: &EditorAction) -> bool {
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
    fn sidebar_paste(&mut self) {
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
    fn refresh_sidebar(&mut self) {
        if let Some(sb) = &mut self.sidebar {
            sb.refresh();
        }
    }

    /// Handle input while a delete confirmation is active.
    fn handle_confirm_delete(&mut self, action: EditorAction) {
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

    // ── Command palette input handling ───────────────────────────────────────

    fn handle_command_palette(&mut self, action: EditorAction) {
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

    // ── Code formatting ───────────────────────────────────────────────────────

    /// Short status-bar label showing the active indent style, e.g.
    /// `"spaces:4"` or `"tabs:8"`.
    pub fn indent_label(&self) -> String {
        let (indent, _) = self.indent_for_active();
        match indent.style {
            crate::formatting::IndentStyle::Tabs => format!("tabs:{}", indent.width),
            crate::formatting::IndentStyle::Spaces => format!("spaces:{}", indent.width),
        }
    }

    /// Resolve the live indent rules for the active buffer's language,
    /// merging project + global config and falling back to the legacy
    /// `tab_size` and built-in defaults.
    ///
    /// Per-buffer `.editorconfig` overrides win over every config layer.
    fn indent_for_active(
        &self,
    ) -> (
        crate::formatting::IndentConfig,
        crate::formatting::IndentRules,
    ) {
        let active = self.editor.active();
        let lang = active.syntax.language;
        let resolver = crate::formatting::FormattingResolver {
            global: &self.config.formatting,
            project: self.project_fmt.as_ref(),
            legacy_tab_size: self.config.tab_size,
        };
        let mut indent = resolver.indent(lang);
        if let Some(style) = active.editorconfig.indent_style {
            indent.style = style;
        }
        if let Some(width) = active.editorconfig.effective_width()
            && width > 0
        {
            indent.width = width;
        }
        (indent, crate::formatting::IndentRules::for_lang(lang))
    }

    /// Run the configured external formatter for the active buffer's
    /// language and replace the buffer atomically. On any error, the buffer
    /// is left untouched and `status_error` is set.
    fn format_buffer(&mut self) {
        let lang = self.editor.active().syntax.language;
        let path = self.editor.active().path.clone();
        let resolver = crate::formatting::FormattingResolver {
            global: &self.config.formatting,
            project: self.project_fmt.as_ref(),
            legacy_tab_size: self.config.tab_size,
        };
        let fc = match resolver.formatter(lang) {
            Some(f) => f,
            None => {
                let name = lang.name();
                let display = if name.is_empty() { "this file" } else { name };
                self.status_error = Some(format!("No formatter configured for {display}"));
                return;
            }
        };

        let input = self.editor.active().buffer.to_string();
        let (saved_line, saved_col) = {
            let c = self.editor.active().buffer.cursors.primary();
            (c.line, c.col)
        };

        match crate::formatting::run_formatter(&fc, &input, path.as_deref()) {
            Ok(out) if out == input => {
                // No-op format — leave the buffer alone, no undo entry.
            }
            Ok(out) => {
                let buf = &mut self.editor.active_mut().buffer;
                buf.begin_batch();
                let len = buf.rope().len_bytes();
                buf.delete_range(0, len);
                buf.move_cursor_to(0, false);
                buf.insert_str(&out);
                buf.commit_batch();
                // Restore cursor by clamped (line, col).
                let new_cursor =
                    crate::buffer::cursor::Cursor::from_line_col(buf.rope(), saved_line, saved_col);
                buf.move_cursor_to(new_cursor.byte_offset, false);
                self.editor.active_mut().reparse();
            }
            Err(e) => {
                self.status_error = Some(format!("Formatter failed: {e}"));
            }
        }
    }

    // ── Line comment toggle ───────────────────────────────────────────────────

    fn toggle_line_comment(&mut self) {
        let prefix = match self.editor.active().syntax.comment_prefix() {
            Some(p) => p,
            None => return, // language has no line comment syntax
        };
        let cursor_line = self.editor.active().buffer.cursors.primary().line;
        let line_str = self.editor.active().buffer.line_str(cursor_line);
        let trimmed = line_str.trim_start();
        let leading_spaces = line_str.len() - trimmed.len();
        let already_commented = trimmed.starts_with(prefix);

        let buf = &mut self.editor.active_mut().buffer;
        let line_start = buf
            .rope()
            .char_to_byte(buf.rope().line_to_char(cursor_line));

        buf.begin_batch();
        if already_commented {
            // Remove the comment prefix.
            let comment_start = line_start + leading_spaces;
            let comment_end = comment_start + prefix.len();
            buf.delete_range(comment_start, comment_end);
        } else {
            // Insert the comment prefix at the start of the line.
            buf.move_cursor_to(line_start, false);
            buf.insert_str(prefix);
        }
        buf.commit_batch();
    }

    // ── File helpers ─────────────────────────────────────────────────────────

    fn close_tab(&mut self) {
        if self.editor.active().buffer.modified && self.config.confirm_exit {
            self.confirm_quit = true; // reuse confirm for "discard changes?"
        } else {
            // Notify LSP before closing.
            if let Some(path) = self.editor.active().path.clone() {
                self.notify_lsp_did_close(&path);
            }
            self.editor.close_active_tab();
        }
    }

    fn save_active(&mut self) {
        if self.editor.active().path.is_some() {
            let _ = self.editor.active_mut().save();
            self.after_file_open_or_save();
            // Flush any pending didChange before sending didSave.
            if self.lsp_dirty_since.is_some() && !self.lsp_change_sent {
                self.send_lsp_did_change();
            }
            self.lsp_dirty_since = None;
            self.lsp_change_sent = false;
            self.notify_lsp_did_save();
        } else {
            self.input_mode = InputMode::SaveAsPath(String::new());
        }
    }

    /// Recompute the git gutter for the currently active buffer (if it has a path).
    fn refresh_git_gutter(&mut self) {
        let path = self.editor.active().path.clone();
        if let Some(path) = path {
            let content = self.editor.active().buffer.to_string();
            self.git_gutter = crate::git::gutter_for_path(&path, &content);
        } else {
            self.git_gutter = None;
        }
    }

    /// Replay the actions stored in macro slot `slot`. Wraps the playback in
    /// a single undo batch so the entire sequence collapses to one undo step,
    /// and sets the replay flag so the actions are not re-recorded if the
    /// user starts a new recording mid-replay.
    fn replay_macro_slot(&mut self, slot: char) {
        let Some(actions) = self.macros.play(slot) else {
            self.status_error = Some(format!("No macro in slot '{slot}'"));
            return;
        };
        let term_h = self.term_height;
        self.macros.set_replaying(true);
        self.editor.active_mut().buffer.begin_batch();
        for action in actions {
            self.update(action, term_h);
        }
        self.editor.active_mut().buffer.commit_batch();
        self.macros.set_replaying(false);
    }

    /// Same logic as `expand_snippet_at_cursor` but silent: returns `false`
    /// when no snippet matches so the caller can fall through to its own
    /// default (e.g. `InsertTab` doing indentation).
    fn try_expand_snippet_silently(&mut self) -> bool {
        let lang_id = self.editor.active().syntax.language.config_key();
        if lang_id.is_empty() {
            return false;
        }
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        let primary = self.editor.active().buffer.cursors.primary();
        if primary.has_selection() {
            return false;
        }
        let rope = self.editor.active().buffer.rope();
        let probe = cursor_byte.saturating_sub(1);
        let Some((wstart, wend)) = crate::buffer::cursor::word_span_at(rope, probe) else {
            return false;
        };
        if wend < cursor_byte {
            return false;
        }
        let prefix: String = rope.byte_slice(wstart..wend).chars().collect();
        let matches = self.snippets.lookup(lang_id, &prefix);
        let Some(snip) = matches.into_iter().next() else {
            return false;
        };
        let parsed = snip.parse_body();
        let exp = crate::snippet::session::SnippetSession::expand_at(&parsed, wstart);
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.delete_range(wstart, wend);
        *buf.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(buf.rope(), wstart);
        buf.cursors.primary_mut().selection = None;
        buf.insert_str(&exp.text);
        buf.commit_batch();
        self.editor.active_mut().snippet_session = exp.session;
        self.snippet_select_current();
        true
    }

    /// Look up the word before the cursor in the active buffer's language
    /// snippet store; if there's a match, delete the word and expand the
    /// snippet in its place.
    fn expand_snippet_at_cursor(&mut self) {
        let lang_id = self.editor.active().syntax.language.config_key();
        if lang_id.is_empty() {
            self.status_error = Some("Snippets need a recognised language".into());
            return;
        }
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        let rope = self.editor.active().buffer.rope();
        let probe = cursor_byte.saturating_sub(1);
        let Some((wstart, wend)) = crate::buffer::cursor::word_span_at(rope, probe) else {
            self.status_error = Some("Place the cursor after a snippet prefix".into());
            return;
        };
        if wend < cursor_byte {
            self.status_error = Some("Place the cursor after a snippet prefix".into());
            return;
        }
        let prefix: String = rope.byte_slice(wstart..wend).chars().collect();
        let matches = self.snippets.lookup(lang_id, &prefix);
        let Some(snip) = matches.into_iter().next() else {
            self.status_error = Some(format!("No snippet named '{prefix}'"));
            return;
        };
        let parsed = snip.parse_body();
        // Compute the expansion text and session at the prefix start position
        // since we're about to delete the prefix.
        let exp = crate::snippet::session::SnippetSession::expand_at(&parsed, wstart);
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.delete_range(wstart, wend);
        // Move the primary cursor to wstart before inserting so insert lands
        // at the right location regardless of prior selection state.
        *buf.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(buf.rope(), wstart);
        buf.cursors.primary_mut().selection = None;
        buf.insert_str(&exp.text);
        buf.commit_batch();
        self.editor.active_mut().snippet_session = exp.session;
        // Jump cursor to the first tab stop, selecting its default text.
        self.snippet_select_current();
    }

    /// Select the byte range of the current snippet tab stop. Called after
    /// expanding or advancing the session.
    fn snippet_select_current(&mut self) {
        let handle = self.editor.active_mut();
        let range = match &handle.snippet_session {
            Some(s) => s.current_range(),
            None => return,
        };
        let len = handle.buffer.rope().len_bytes();
        let bound_start = range.start.min(len);
        let bound_end = range.end.min(len);
        let new_cursor = {
            let rope = handle.buffer.rope();
            crate::buffer::cursor::Cursor::from_byte_offset(rope, bound_end)
        };
        let primary = handle.buffer.cursors.primary_mut();
        *primary = new_cursor;
        if bound_end > bound_start {
            primary.selection = Some(crate::buffer::cursor::Selection {
                anchor: bound_start,
                active: bound_end,
            });
        } else {
            primary.selection = None;
        }
    }

    /// Move the active snippet session forward (`true`) or backward.
    fn snippet_advance(&mut self, forward: bool) {
        let advanced = match self.editor.active_mut().snippet_session.as_mut() {
            Some(s) => {
                if forward {
                    s.next_stop()
                } else {
                    s.prev_stop()
                }
            }
            None => false,
        };
        if advanced {
            self.snippet_select_current();
        } else {
            self.editor.active_mut().snippet_session = None;
        }
    }

    /// Snapshot the active buffer's cursor into the jump list. Used right
    /// before navigation actions (mark jumps, jump-list back) so the
    /// previous cursor position can be returned to.
    fn push_current_to_jump_list(&mut self) {
        let handle = self.editor.active();
        if let Some(path) = handle.path.clone() {
            let byte_offset = handle.buffer.cursors.primary().byte_offset;
            self.jumps
                .push(crate::marks::JumpEntry { path, byte_offset });
        }
    }

    /// Move the cursor to a jump-list entry, switching tabs if necessary.
    fn go_to_jump_entry(&mut self, entry: &crate::marks::JumpEntry) {
        let active_path = self.editor.active().path.clone();
        if active_path.as_ref() != Some(&entry.path) {
            let _ = self.editor.open_tab(entry.path.clone());
            self.after_file_open_or_save();
        }
        let handle = self.editor.active_mut();
        let rope = handle.buffer.rope();
        let bound = entry.byte_offset.min(rope.len_bytes());
        *handle.buffer.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(rope, bound);
        handle.buffer.cursors.collapse_to_primary();
    }

    /// Called after a file is opened or saved — updates recent files, git gutter,
    /// installs a file watcher, and notifies the LSP server.
    fn after_file_open_or_save(&mut self) {
        if let Some(path) = self.editor.active().path.clone() {
            add_to_recent_files(&path, &self.workspace.clone());
            self.file_watcher = FileWatcher::new(&path);
        }
        self.refresh_git_gutter();
        // Notify LSP server that a file was opened.
        let handle = self.editor.active();
        // Avoid borrow conflict by extracting what we need.
        let path = handle.path.clone();
        let lang = handle.syntax.language.name().to_lowercase();
        let version = handle.lsp_state.version;
        let text = handle.buffer.rope().to_string();
        if let Some(registry) = &self.lsp
            && registry.is_ready()
            && let Some(path) = &path
        {
            let uri = crate::lsp::types::path_to_uri(path);
            let _ = registry.client().did_open(&uri, &lang, version, &text);
        }
    }

    /// Poll the file watcher; if the file changed externally, reload automatically.
    pub fn poll_file_watcher(&mut self) {
        if let Some(watcher) = &self.file_watcher
            && watcher.poll()
        {
            self.reload_active_file();
        }
    }

    /// Save the active buffer automatically after 1 second of inactivity (debounced).
    ///
    /// Only saves when `config.auto_save` is enabled and the buffer has a path.
    pub fn poll_auto_save(&mut self) {
        if !self.config.auto_save {
            return;
        }
        if let Some(t) = self.auto_save_timer
            && t.elapsed() >= std::time::Duration::from_secs(1)
            && self.editor.active().path.is_some()
        {
            self.save_active();
            self.auto_save_timer = None;
        }
    }

    /// Update cached RSS memory usage (throttled to every 2 seconds).
    pub fn refresh_memory(&mut self) {
        if self.memory_last_checked.elapsed() >= Duration::from_secs(2) {
            if let Some(kb) = read_rss_kb() {
                self.memory_rss_kb = kb;
            }
            self.memory_last_checked = Instant::now();
        }
    }

    /// Refresh the cached git branch (throttled to every 2 seconds).
    ///
    /// Picks up branch changes made outside the editor (e.g. `git checkout`
    /// from another terminal).
    pub fn refresh_git_branch(&mut self) {
        if self.git_branch_last_checked.elapsed() >= Duration::from_secs(2) {
            self.git_branch = crate::git::current_branch(&self.workspace);
            self.git_branch_last_checked = Instant::now();
        }
    }

    /// Reload the active buffer from disk (used after external modification).
    fn reload_active_file(&mut self) {
        let path = match self.editor.active().path.clone() {
            Some(p) => p,
            None => return,
        };
        if let Ok(text) = std::fs::read_to_string(&path) {
            let handle = self.editor.active_mut();
            let saved_line = handle.buffer.cursors.primary().line;
            let saved_col = handle.buffer.cursors.primary().col;
            handle.buffer = crate::buffer::Buffer::from_str(&text);
            handle.buffer.modified = false;
            let rope = handle.buffer.rope().clone();
            *handle.buffer.cursors.primary_mut() =
                crate::buffer::cursor::Cursor::from_line_col(&rope, saved_line, saved_col);
        }
        self.refresh_git_gutter();
        // Re-install watcher after reload so we don't miss the next change.
        if let Some(path) = self.editor.active().path.clone() {
            self.file_watcher = FileWatcher::new(&path);
        }
    }

    // ── LSP polling ──────────────────────────────────────────────────────────

    /// How long to wait after the last edit before sending `didChange`.
    const LSP_DEBOUNCE: Duration = Duration::from_millis(100);
    /// How long to wait after the last edit before re-requesting semantic tokens.
    const SEMANTIC_TOKEN_DEBOUNCE: Duration = Duration::from_millis(300);

    /// Flush debounced LSP notifications if enough idle time has passed.
    /// Called once per frame in the event loop.
    pub fn flush_lsp_debounce(&mut self) {
        let Some(dirty_since) = self.lsp_dirty_since else {
            return;
        };
        let elapsed = dirty_since.elapsed();

        // After 100ms idle, send the buffered didChange (one full-buffer copy).
        if elapsed >= Self::LSP_DEBOUNCE && !self.lsp_change_sent {
            self.send_lsp_did_change();
            self.lsp_change_sent = true;
        }

        // After 300ms idle, re-request semantic tokens and clear the timer.
        if elapsed >= Self::SEMANTIC_TOKEN_DEBOUNCE {
            self.request_semantic_tokens_for_active();
            self.lsp_dirty_since = None;
            self.lsp_change_sent = false;
        }
    }

    /// Non-blocking drain of pending LSP updates. Called once per frame.
    pub fn poll_lsp_updates(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        let updates = registry.poll();
        for update in updates {
            self.apply_lsp_update(update);
        }
    }

    fn apply_lsp_update(&mut self, update: crate::lsp::client::LspUpdate) {
        use crate::lsp::client::LspUpdate;
        match update {
            LspUpdate::Initialized(caps) => {
                if let Some(registry) = &mut self.lsp {
                    registry.client_mut().capabilities = caps;
                    registry.client_mut().initialized = true;
                    let _ = registry
                        .client()
                        .send_notification("initialized", Some(serde_json::json!({})));
                }
                // Send didOpen for all currently open buffers.
                self.notify_lsp_did_open_all();
                // Request semantic tokens for the active buffer.
                self.request_semantic_tokens_for_active();
            }
            LspUpdate::Diagnostics { uri, diagnostics } => {
                self.apply_diagnostics(&uri, &diagnostics);
            }
            LspUpdate::ServerExited => {
                // If the binary on disk is still the one the user approved,
                // use the existing in-place restart path (preserves restart_count
                // so a crash loop self-terminates after MAX_RESTARTS).
                // If the binary changed (or vanished), tear down and route
                // through the approval gate so the user is re-prompted.
                let trusted = self.lsp_binary_still_trusted();

                if !trusted {
                    self.lsp = None;
                    self.status_error =
                        Some("LSP binary changed since approval; re-prompting".into());
                    self.request_lsp_start();
                    return;
                }

                let config = self.lsp_config.clone();
                let workspace = self.workspace.clone();
                let mut disable_lsp = false;

                if let Some(registry) = &mut self.lsp
                    && (registry.restart_exhausted()
                        || registry.try_restart(&config, &workspace).is_err())
                {
                    disable_lsp = true;
                }

                if disable_lsp {
                    self.lsp = None;
                    self.status_error =
                        Some("LSP server exited unexpectedly (restart limit reached)".into());
                } else {
                    self.status_error = Some("LSP server exited, restarting…".into());
                }
            }
            LspUpdate::Completion { items, .. } => {
                self.apply_completion_response(items);
            }
            LspUpdate::Hover { contents, .. } => {
                self.apply_hover_response(contents);
            }
            LspUpdate::Definition { locations, .. } => {
                self.apply_definition_response(locations);
            }
            LspUpdate::References { locations, .. } => {
                self.apply_references_response(locations);
            }
            LspUpdate::Rename { edit, .. } => {
                if let Some(edit) = edit {
                    self.apply_workspace_edit(&edit);
                }
            }
            LspUpdate::CodeActions { actions, .. } => {
                let _ = actions; // TODO: show code action picker
            }
            LspUpdate::SemanticTokens { uri, data } => {
                self.apply_semantic_tokens(&uri, &data);
            }
            LspUpdate::Error(msg) => {
                self.status_error = Some(msg);
            }
        }
    }

    fn notify_lsp_did_open_all(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        for tab in &self.editor.tabs {
            if let Some(path) = &tab.path {
                let uri = crate::lsp::types::path_to_uri(path);
                let lang_id = tab.syntax.language.name().to_lowercase();
                let text = tab.buffer.rope().to_string();
                let _ = registry
                    .client()
                    .did_open(&uri, &lang_id, tab.lsp_state.version, &text);
            }
        }
    }

    /// Send `textDocument/didOpen` for a single buffer.
    #[allow(dead_code)]
    fn notify_lsp_did_open(&self, handle: &crate::editor::tab::BufferHandle) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        if let Some(path) = &handle.path {
            let uri = crate::lsp::types::path_to_uri(path);
            let lang_id = handle.syntax.language.name().to_lowercase();
            let text = handle.buffer.rope().to_string();
            let _ = registry
                .client()
                .did_open(&uri, &lang_id, handle.lsp_state.version, &text);
        }
    }

    /// Send `textDocument/didChange` for the active buffer (full sync).
    /// Version must already be bumped before calling this.
    fn send_lsp_did_change(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        if let Some(path) = &handle.path {
            let uri = crate::lsp::types::path_to_uri(path);
            let version = handle.lsp_state.version;
            let text = handle.buffer.rope().to_string();
            let _ = registry.client().did_change(&uri, version, &text);
        }
    }

    /// Send `textDocument/didSave` for the active buffer.
    fn notify_lsp_did_save(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        if let Some(path) = &self.editor.active().path {
            let uri = crate::lsp::types::path_to_uri(path);
            let _ = registry.client().did_save(&uri);
        }
    }

    /// Send `textDocument/didClose` for a buffer by path.
    fn notify_lsp_did_close(&self, path: &std::path::Path) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        let uri = crate::lsp::types::path_to_uri(path);
        let _ = registry.client().did_close(&uri);
    }

    /// Convert raw diagnostic JSON from the server to byte-offset `LspDiagnostic`s
    /// and store them on the matching buffer.
    fn apply_diagnostics(&mut self, uri: &str, raw_diagnostics: &[serde_json::Value]) {
        use crate::lsp::types::{DiagSeverity, LspDiagnostic, lsp_position_to_byte_offset};

        let path = match crate::lsp::types::uri_to_path(uri) {
            Some(p) => p,
            None => return,
        };

        // Find the buffer that matches this URI.
        let tab = self
            .editor
            .tabs
            .iter_mut()
            .find(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
        let Some(tab) = tab else { return };

        let rope = tab.buffer.rope();
        let mut diagnostics = Vec::with_capacity(raw_diagnostics.len());

        for raw in raw_diagnostics {
            let range = match raw.get("range") {
                Some(r) => r,
                None => continue,
            };
            let start = match parse_lsp_position(range.get("start")) {
                Some(pos) => match lsp_position_to_byte_offset(rope, pos) {
                    Some(b) => b,
                    None => continue,
                },
                None => continue,
            };
            let end = match parse_lsp_position(range.get("end")) {
                Some(pos) => match lsp_position_to_byte_offset(rope, pos) {
                    Some(b) => b,
                    None => continue,
                },
                None => continue,
            };
            let severity = match raw.get("severity").and_then(|v| v.as_u64()) {
                Some(1) => DiagSeverity::Error,
                Some(2) => DiagSeverity::Warning,
                Some(3) => DiagSeverity::Information,
                _ => DiagSeverity::Hint,
            };
            let message = raw
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let source = raw.get("source").and_then(|v| v.as_str()).map(String::from);

            diagnostics.push(LspDiagnostic {
                range: crate::buffer::cursor::ByteRange { start, end },
                severity,
                message,
                source,
            });
        }

        tab.lsp_state.diagnostics = diagnostics;
    }

    // ── Completion ───────────────────────────────────────────────────────────

    fn trigger_completion(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let anchor_byte = cursor.byte_offset;
        let anchor_line = cursor.line;
        let anchor_col = cursor.col;

        let _ = registry
            .client_mut()
            .request_completion(&uri, pos.line, pos.character);

        self.completion = Some(CompletionState::new(anchor_byte, anchor_line, anchor_col));
    }

    fn handle_completion_input(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(c) = &mut self.completion {
                    c.selected = c.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(c) = &mut self.completion
                    && !c.filtered.is_empty()
                {
                    c.selected = (c.selected + 1).min(c.filtered.len() - 1);
                }
                true
            }
            EditorAction::InsertNewline | EditorAction::InsertTab => {
                self.accept_completion();
                true
            }
            EditorAction::CloseSearch => {
                self.completion = None;
                true
            }
            // Cursor movement dismisses completion.
            EditorAction::MoveCursor(_)
            | EditorAction::MoveCursorWord(_)
            | EditorAction::MoveCursorHome
            | EditorAction::MoveCursorEnd => {
                self.completion = None;
                false // let the movement fall through
            }
            // Characters fall through to editing, then refilter.
            _ => false,
        }
    }

    fn accept_completion(&mut self) {
        let insert_text = match &self.completion {
            Some(c) => match c.selected_item() {
                Some(item) => item.insert_text.clone(),
                None => {
                    self.completion = None;
                    return;
                }
            },
            None => return,
        };
        let anchor = self.completion.as_ref().unwrap().anchor_byte;
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;

        // Delete the typed prefix and insert the completion text.
        if cursor_byte > anchor {
            let rope = self.editor.active().buffer.rope();
            let start_char = rope.byte_to_char(anchor);
            let end_char = rope.byte_to_char(cursor_byte);
            self.editor
                .active_mut()
                .buffer
                .delete_range(start_char, end_char);
        }
        self.editor.active_mut().buffer.insert_str(&insert_text);

        self.completion = None;
    }

    fn refilter_completion(&mut self) {
        let Some(comp) = &mut self.completion else {
            return;
        };
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        if cursor_byte < comp.anchor_byte {
            self.completion = None;
            return;
        }
        let rope = self.editor.active().buffer.rope();
        let start = rope.byte_to_char(comp.anchor_byte);
        let end = rope.byte_to_char(cursor_byte);
        let prefix: String = rope.slice(start..end).chars().collect();
        comp.filter(&prefix);
    }

    fn apply_completion_response(&mut self, items: Vec<serde_json::Value>) {
        let Some(comp) = &mut self.completion else {
            return;
        };
        comp.items = items
            .iter()
            .map(|item| {
                let label = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let detail = item
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let insert_text = item
                    .get("insertText")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&label)
                    .to_string();
                let filter_text = item
                    .get("filterText")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&label)
                    .to_string();
                let kind = item.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
                let kind_label = completion_kind_label(kind);
                CompletionItemEntry {
                    label,
                    detail,
                    insert_text,
                    filter_text,
                    kind_label,
                }
            })
            .collect();
        comp.filtered = (0..comp.items.len()).collect();
        self.refilter_completion();
    }

    // ── Hover ────────────────────────────────────────────────────────────────

    fn trigger_hover(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.hover_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let anchor_line = cursor.line;
        let anchor_col = cursor.col;

        let _ = registry
            .client_mut()
            .request_hover(&uri, pos.line, pos.character);

        // We'll set hover state when the response arrives.
        let _ = (anchor_line, anchor_col);
    }

    fn apply_hover_response(&mut self, contents: Option<serde_json::Value>) {
        let Some(contents) = contents else { return };
        let text = extract_hover_text(&contents);
        if text.is_empty() {
            return;
        }
        let cursor = self.editor.active().buffer.cursors.primary();
        self.hover = Some(HoverState {
            content: text,
            anchor_line: cursor.line,
            anchor_col: cursor.col,
        });
    }

    // ── Go to Definition ─────────────────────────────────────────────────────

    fn trigger_go_to_definition(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.definition_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_definition(&uri, pos.line, pos.character);
    }

    fn apply_definition_response(&mut self, locations: serde_json::Value) {
        let locs = parse_locations(&locations);
        if locs.is_empty() {
            return;
        }
        if locs.len() == 1 {
            self.jump_to_location(&locs[0]);
        } else {
            self.show_references_list(locs);
        }
    }

    // ── Find References ──────────────────────────────────────────────────────

    fn trigger_find_references(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.references_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_references(&uri, pos.line, pos.character);
    }

    fn apply_references_response(&mut self, locations: serde_json::Value) {
        let locs = parse_locations(&locations);
        if locs.is_empty() {
            return;
        }
        self.show_references_list(locs);
    }

    fn show_references_list(&mut self, locs: Vec<(PathBuf, usize, usize)>) {
        let items: Vec<ReferenceItem> = locs
            .into_iter()
            .map(|(path, line, col)| {
                let context = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.lines().nth(line).map(|l| l.trim().to_string()))
                    .unwrap_or_default();
                ReferenceItem {
                    path,
                    line,
                    col,
                    context,
                }
            })
            .collect();
        self.references_list = Some(ReferencesListState { items, selected: 0 });
    }

    fn handle_references_input(&mut self, action: &EditorAction) -> bool {
        let num_items = self
            .references_list
            .as_ref()
            .map(|r| r.items.len())
            .unwrap_or(0);
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(r) = &mut self.references_list {
                    r.selected = r.selected.saturating_sub(1);
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(r) = &mut self.references_list {
                    r.selected = (r.selected + 1).min(num_items.saturating_sub(1));
                }
                true
            }
            EditorAction::InsertNewline => {
                if let Some(r) = &self.references_list
                    && let Some(item) = r.items.get(r.selected)
                {
                    let path = item.path.clone();
                    let line = item.line;
                    let col = item.col;
                    self.references_list = None;
                    self.jump_to_location(&(path, line, col));
                }
                true
            }
            EditorAction::CloseSearch => {
                self.references_list = None;
                true
            }
            EditorAction::Quit | EditorAction::ForceQuit => {
                self.references_list = None;
                false
            }
            _ => false,
        }
    }

    fn jump_to_location(&mut self, loc: &(PathBuf, usize, usize)) {
        let (path, line, col) = loc;

        // Check if file is already open in a tab.
        let existing = self
            .editor
            .tabs
            .iter()
            .position(|t| t.path.as_ref().is_some_and(|p| same_file(p, path)));

        if let Some(idx) = existing {
            self.editor.go_to_tab(idx);
        } else {
            // Open in new tab.
            if self.editor.open_tab(path.clone()).is_err() {
                return;
            }
            self.after_file_open_or_save();
        }

        // Jump cursor to position.
        let rope = self.editor.active().buffer.rope().clone();
        let cursor = crate::buffer::cursor::Cursor::from_line_col(&rope, *line, *col);
        *self.editor.active_mut().buffer.cursors.primary_mut() = cursor;
    }

    // ── Rename ───────────────────────────────────────────────────────────────

    fn trigger_rename(&mut self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() || !registry.client().capabilities.rename_provider {
            return;
        }
        // Enter rename modal: prompt for new name.
        let handle = self.editor.active();
        let cursor = handle.buffer.cursors.primary();
        // Extract word under cursor as the default name.
        let rope = handle.buffer.rope();
        let byte = cursor.byte_offset;
        let text = rope.to_string();
        let word = extract_word_at(&text, byte);
        self.input_mode = InputMode::Rename(word);
    }

    /// Send rename request after the user confirms the new name.
    fn send_rename(&mut self, new_name: &str) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_rename(&uri, pos.line, pos.character, new_name);
    }

    fn apply_workspace_edit(&mut self, edit: &serde_json::Value) {
        let changes = match edit.get("changes").and_then(|v| v.as_object()) {
            Some(c) => c,
            None => return,
        };

        for (uri, edits) in changes {
            let path = match crate::lsp::types::uri_to_path(uri) {
                Some(p) => p,
                None => continue,
            };
            let edits = match edits.as_array() {
                Some(e) => e,
                None => continue,
            };

            // Find or open the tab.
            let tab_idx = self
                .editor
                .tabs
                .iter()
                .position(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
            let tab_idx = match tab_idx {
                Some(i) => i,
                None => continue, // Skip files not open.
            };

            // Collect and sort edits in reverse order to avoid offset shifting.
            let mut text_edits: Vec<(usize, usize, String)> = Vec::new();
            let rope = self.editor.tabs[tab_idx].buffer.rope();
            for e in edits {
                let range = match e.get("range") {
                    Some(r) => r,
                    None => continue,
                };
                let start = match parse_lsp_position(range.get("start")) {
                    Some(pos) => {
                        crate::lsp::types::lsp_position_to_byte_offset(rope, pos).unwrap_or(0)
                    }
                    None => continue,
                };
                let end = match parse_lsp_position(range.get("end")) {
                    Some(pos) => {
                        crate::lsp::types::lsp_position_to_byte_offset(rope, pos).unwrap_or(0)
                    }
                    None => continue,
                };
                let new_text = e
                    .get("newText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                text_edits.push((start, end, new_text));
            }

            // Apply in reverse byte order.
            text_edits.sort_by_key(|b| std::cmp::Reverse(b.0));
            let tab = &mut self.editor.tabs[tab_idx];
            for (start, end, new_text) in &text_edits {
                let rope = tab.buffer.rope();
                let start_char = rope.byte_to_char(*start);
                let end_char = rope.byte_to_char(*end);
                tab.buffer.delete_range(start_char, end_char);
                tab.buffer.insert_str(new_text);
            }
        }
    }

    // ── Code Action ──────────────────────────────────────────────────────────

    fn trigger_code_action(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.code_action_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let range = serde_json::json!({
            "start": { "line": pos.line, "character": pos.character },
            "end": { "line": pos.line, "character": pos.character },
        });

        let _ = registry.client_mut().request_code_action(&uri, range);
    }

    // ── Semantic Tokens ──────────────────────────────────────────────────────

    fn apply_semantic_tokens(&mut self, uri: &str, data: &[u32]) {
        let path = match crate::lsp::types::uri_to_path(uri) {
            Some(p) => p,
            None => return,
        };
        let tab = self
            .editor
            .tabs
            .iter_mut()
            .find(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
        let Some(tab) = tab else { return };

        let rope = tab.buffer.rope();
        let tokens = crate::lsp::types::decode_semantic_tokens(data, rope);
        tab.lsp_state.semantic_tokens = Some(tokens);
    }

    /// Request semantic tokens for the active buffer.
    fn request_semantic_tokens_for_active(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.semantic_tokens_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let _ = registry.client_mut().send_request(
            "textDocument/semanticTokens/full",
            Some(serde_json::json!({
                "textDocument": { "uri": uri }
            })),
        );
    }

    // ── LSP restart/stop ──────────────────────────────────────────────────────

    fn lsp_restart(&mut self) {
        // Tear down existing connection.
        self.lsp = None;
        self.pending_lsp_approval = None;
        // Clear stale state from all buffers.
        for tab in &mut self.editor.tabs {
            tab.lsp_state.diagnostics.clear();
            tab.lsp_state.semantic_tokens = None;
        }
        // Start fresh if config is active — routed through the trust gate.
        self.request_lsp_start();
    }

    /// Trust-gated entry point for spawning the LSP server.
    ///
    /// Resolves the configured binary, hashes it, and consults the user-global
    /// trust store. If the binary is approved, spawns directly. If unknown or
    /// the hash has changed, sets `pending_lsp_approval` so the approval
    /// overlay opens on the next frame.
    pub fn request_lsp_start(&mut self) {
        if !self.lsp_config.is_active() {
            return;
        }
        let entry = match self.lsp_config.active_server() {
            Some(e) => e.clone(),
            None => return,
        };
        let server_name = self.lsp_config.server.clone().unwrap_or_default();

        let resolved = match crate::lsp::resolve::resolve_binary(&entry.command) {
            Ok(r) => r,
            Err(e) => {
                self.status_error = Some(format!("LSP: {e}"));
                return;
            }
        };
        let hash = match crate::lsp::resolve::hash_file(&resolved.canonical_path) {
            Ok(h) => h,
            Err(e) => {
                self.status_error = Some(format!("LSP: {e}"));
                return;
            }
        };

        let store = crate::lsp::trust::TrustStore::load();
        match store.check(&resolved.canonical_path, &hash) {
            crate::lsp::trust::TrustDecision::Approved => {
                self.lsp = crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
            }
            crate::lsp::trust::TrustDecision::Unknown => {
                self.pending_lsp_approval = Some(PendingLspApproval {
                    server_name,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    display_path: resolved.display_path,
                    canonical_path: resolved.canonical_path,
                    hash,
                    reason: LspApprovalReason::FirstLaunch,
                });
            }
            crate::lsp::trust::TrustDecision::HashMismatch { previous_hash } => {
                self.pending_lsp_approval = Some(PendingLspApproval {
                    server_name,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    display_path: resolved.display_path,
                    canonical_path: resolved.canonical_path,
                    hash,
                    reason: LspApprovalReason::BinaryChanged { previous_hash },
                });
            }
        }
    }

    /// Whether the currently configured LSP binary still matches its trust-store
    /// entry. Used by the crash-recovery path to decide between an in-place
    /// restart and re-routing through the approval gate.
    fn lsp_binary_still_trusted(&self) -> bool {
        let entry = match self.lsp_config.active_server() {
            Some(e) => e,
            None => return false,
        };
        let resolved = match crate::lsp::resolve::resolve_binary(&entry.command) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let hash = match crate::lsp::resolve::hash_file(&resolved.canonical_path) {
            Ok(h) => h,
            Err(_) => return false,
        };
        matches!(
            crate::lsp::trust::TrustStore::load().check(&resolved.canonical_path, &hash),
            crate::lsp::trust::TrustDecision::Approved
        )
    }

    /// Handle input while the LSP-binary approval overlay is shown.
    /// Returns `true` if the action was consumed (always, while modal is open).
    fn handle_lsp_approval(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::InsertChar('y') | EditorAction::InsertChar('Y') => {
                if let Some(pending) = self.pending_lsp_approval.take() {
                    let mut store = crate::lsp::trust::TrustStore::load();
                    store.approve(
                        pending.canonical_path,
                        pending.hash,
                        Some(pending.server_name),
                    );
                    store.save();
                    self.lsp =
                        crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
                    self.status_error = Some(
                        "LSP approved; recorded in ~/.config/txt/trusted_binaries.json".into(),
                    );
                }
                true
            }
            EditorAction::InsertChar('n')
            | EditorAction::InsertChar('N')
            | EditorAction::Quit
            | EditorAction::CloseSearch => {
                self.pending_lsp_approval = None;
                self.status_error = Some("LSP not started.".into());
                true
            }
            // Capture every other input while the modal is up.
            _ => true,
        }
    }

    // ── Coordinate helpers ───────────────────────────────────────────────────

    fn selected_text(&self) -> Option<String> {
        let cursor = self.editor.active().buffer.cursors.primary();
        if !cursor.has_selection() {
            return None;
        }
        let range = cursor.selection_bytes();
        let start = self.editor.active().buffer.rope().byte_to_char(range.start);
        let end = self.editor.active().buffer.rope().byte_to_char(range.end);
        Some(
            self.editor
                .active()
                .buffer
                .rope()
                .slice(start..end)
                .to_string(),
        )
    }

    /// Hit-test the tab strip. Returns the tab index when `(col, row)`
    /// lands on a rendered tab label.
    fn tab_bar_tab_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.tab_bar_area?;
        crate::ui::tab_bar::tab_at(&self.editor, area, col, row)
    }

    /// Returns true if the given screen point is inside the sidebar's
    /// entry-list area (excludes the separator column).
    fn point_in_sidebar(&self, col: u16, row: u16) -> bool {
        match self.sidebar_area {
            Some(area) => {
                col >= area.x
                    && col < area.x + self.sidebar_width
                    && row >= area.y
                    && row < area.y + area.height
            }
            None => false,
        }
    }

    /// Returns true if the given screen point is on the sidebar's separator
    /// column (the 1-column-wide vertical bar between sidebar and editor).
    fn point_on_separator(&self, col: u16, row: u16) -> bool {
        match self.sidebar_area {
            Some(area) => {
                col == area.x + self.sidebar_width && row >= area.y && row < area.y + area.height
            }
            None => false,
        }
    }

    /// Map a screen `row` inside the sidebar to the corresponding entry index.
    /// Returns `None` if the row is outside the sidebar or past the last entry.
    fn sidebar_entry_at(&self, row: u16) -> Option<usize> {
        let area = self.sidebar_area?;
        if row < area.y || row >= area.y + area.height {
            return None;
        }
        let sb = self.sidebar.as_ref()?;
        let screen_row = (row - area.y) as usize;
        let idx = sb.scroll_offset + screen_row;
        if idx < sb.entries.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Sync the sidebar's `scroll_offset` so the selected entry remains visible.
    /// Called after any keyboard navigation that may move `selected` off-screen.
    fn ensure_sidebar_selected_visible(&mut self) {
        let h = self.sidebar_area.map(|r| r.height as usize).unwrap_or(0);
        if let Some(sb) = &mut self.sidebar {
            sb.ensure_selected_visible(h);
        }
    }

    fn screen_to_byte(&self, col: u16, row: u16) -> Option<usize> {
        let editor_area_y: u16 = if self.editor.tab_count() > 1 { 1 } else { 0 };
        // If sidebar is open the editor area starts further right; don't click into sidebar.
        let sidebar_offset: u16 = if self.sidebar.is_some() {
            self.sidebar_width + 1
        } else {
            0
        };
        if self.sidebar.is_some() && col < sidebar_offset {
            return None;
        }
        let adjusted_col = col.saturating_sub(sidebar_offset);
        let gutter = gutter_width(self.editor.active().buffer.len_lines());
        let gutter_cols = gutter + 1;
        Some(screen_pos_to_byte_offset(
            adjusted_col,
            row,
            editor_area_y,
            gutter_cols,
            &self.editor.active().buffer,
            &self.editor.active().viewport,
        ))
    }

    /// Convert a screen position into `(line, display_col)` for box selection.
    /// Returns `None` if the click landed in the sidebar.
    fn screen_to_line_col(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let editor_area_y: u16 = if self.editor.tab_count() > 1 { 1 } else { 0 };
        let sidebar_offset: u16 = if self.sidebar.is_some() {
            self.sidebar_width + 1
        } else {
            0
        };
        if self.sidebar.is_some() && col < sidebar_offset {
            return None;
        }
        let adjusted_col = col.saturating_sub(sidebar_offset);
        let gutter = gutter_width(self.editor.active().buffer.len_lines());
        let gutter_cols = gutter + 1;
        Some(screen_pos_to_line_display_col(
            adjusted_col,
            row,
            editor_area_y,
            gutter_cols,
            &self.editor.active().buffer,
            &self.editor.active().viewport,
        ))
    }
}

// ── App (top-level runner) ────────────────────────────────────────────────────

pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub fn run(
        &self,
        mut terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
        editor: Editor,
        open_sidebar: bool,
        workspace: PathBuf,
    ) -> Result<()> {
        let mut state = AppState::new(editor, workspace);
        if open_sidebar {
            state.sidebar = Some(SidebarState::new());
            state.sidebar_focused = true;
        }

        // Only re-anchor the viewport on the cursor when something actually
        // moved the cursor (or the layout changed); pure scroll actions leave
        // both untouched so the cursor is allowed to leave the screen.
        let mut prev_anchor: Option<(usize, usize)> = None; // (active tab, primary byte offset)
        let mut prev_size: Option<(usize, usize)> = None; // (text_h, text_w)
        let mut prev_title: Option<String> = None;

        loop {
            let term_size = terminal.size()?;
            let term_height = term_size.height;
            let term_width = term_size.width;
            state.term_width = term_width;
            state.term_height = term_height;

            // Compute text area for scroll calculations.
            let tab_bar_rows: u16 = if state.editor.tab_count() > 1 { 1 } else { 0 };
            let search_rows: u16 = state
                .search_state
                .as_ref()
                .map(|s| s.bar_height())
                .unwrap_or(0);
            let text_h = term_height.saturating_sub(1 + tab_bar_rows + search_rows) as usize;
            let sidebar_w: u16 = if state.sidebar.is_some() {
                state.sidebar_width + 1
            } else {
                0
            };
            let gutter = gutter_width(state.editor.active().buffer.len_lines());
            let text_w = term_width.saturating_sub(gutter + 1 + sidebar_w) as usize;

            let curr_anchor = (
                state.editor.active_idx,
                state.editor.active().buffer.cursors.primary().byte_offset,
            );
            let curr_size = (text_h, text_w);
            if prev_anchor != Some(curr_anchor) || prev_size != Some(curr_size) {
                state.editor.active_mut().scroll_to_cursor(text_h, text_w);
            }
            prev_anchor = Some(curr_anchor);
            prev_size = Some(curr_size);

            let desired_title = if state.editor.active().path.is_some() {
                format!("txt - {}", state.editor.active().display_name())
            } else {
                "txt".to_string()
            };
            if prev_title.as_deref() != Some(desired_title.as_str()) {
                let _ = execute!(std::io::stdout(), SetTitle(&desired_title));
                prev_title = Some(desired_title);
            }

            // Check for external file changes (non-blocking).
            state.poll_file_watcher();
            state.poll_auto_save();
            state.refresh_memory();
            state.refresh_git_branch();

            // Drain pending LSP server updates (non-blocking).
            state.poll_lsp_updates();

            // Flush debounced LSP notifications (didChange, semantic tokens).
            state.flush_lsp_debounce();

            terminal.draw(|frame| ui::render(&mut state, frame))?;

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            let action = match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => state.input.handle_key(k),
                Event::Mouse(m) => state.input.handle_mouse(m),
                Event::Resize(_, _) => EditorAction::Unhandled,
                _ => EditorAction::Unhandled,
            };

            state.update(action, term_height);

            if state.should_quit {
                break;
            }
        }

        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

// ── Platform-specific RSS memory reading ─────────────────────────────────────

#[cfg(target_os = "linux")]
fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.trim().trim_end_matches(" kB").trim().parse().ok();
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn read_rss_kb() -> Option<u64> {
    use std::mem;

    const MACH_TASK_BASIC_INFO: u32 = 20;

    type TaskT = u32;
    type TaskFlavorT = u32;
    type TaskInfoT = u32;
    type MachMsgTypeNumberT = u32;
    type KernReturnT = i32;

    unsafe extern "C" {
        fn mach_task_self() -> TaskT;
        fn task_info(
            target_task: TaskT,
            flavor: TaskFlavorT,
            task_info_out: *mut TaskInfoT,
            task_info_outCnt: *mut MachMsgTypeNumberT,
        ) -> KernReturnT;
    }

    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [u32; 2],
        system_time: [u32; 2],
        policy: i32,
        suspend_count: i32,
    }

    unsafe {
        let mut info: MachTaskBasicInfo = mem::zeroed();
        let mut count = (mem::size_of::<MachTaskBasicInfo>() / mem::size_of::<u32>()) as u32;
        let ret = task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut TaskInfoT,
            &mut count,
        );
        if ret == 0 {
            Some(info.resident_size / 1024)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn read_rss_kb() -> Option<u64> {
    use std::mem;

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            ppsmemCounters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        page_file_usage: usize,
        peak_page_file_usage: usize,
    }

    unsafe {
        let mut pmc: ProcessMemoryCounters = mem::zeroed();
        pmc.cb = mem::size_of::<ProcessMemoryCounters>() as u32;
        let ret = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut pmc,
            mem::size_of::<ProcessMemoryCounters>() as u32,
        );
        if ret != 0 {
            Some(pmc.working_set_size as u64 / 1024)
        } else {
            None
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn read_rss_kb() -> Option<u64> {
    None
}

// ── Free helpers for LSP ─────────────────────────────────────────────────────

/// Parse an LSP `{ line, character }` JSON value into our `LspPosition`.
fn parse_lsp_position(val: Option<&serde_json::Value>) -> Option<crate::lsp::types::LspPosition> {
    let obj = val?;
    Some(crate::lsp::types::LspPosition {
        line: obj.get("line")?.as_u64()? as u32,
        character: obj.get("character")?.as_u64()? as u32,
    })
}

/// Compare two paths, canonicalizing to handle symlinks / relative paths.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Map an LSP completion item kind number to a short label.
fn completion_kind_label(kind: u64) -> &'static str {
    match kind {
        1 => "txt",
        2 => "fn ",
        3 => "fn ",
        4 => "new",
        5 => "fld",
        6 => "var",
        7 => "cls",
        8 => "ifc",
        9 => "mod",
        10 => "prp",
        14 => "kw ",
        15 => "snp",
        21 => "cst",
        _ => "   ",
    }
}

/// Extract plain text from an LSP hover contents value.
fn extract_hover_text(contents: &serde_json::Value) -> String {
    // Can be a string, a { kind, value } MarkupContent, or an array.
    if let Some(s) = contents.as_str() {
        return s.to_string();
    }
    if let Some(value) = contents.get("value").and_then(|v| v.as_str()) {
        return value.to_string();
    }
    if let Some(arr) = contents.as_array() {
        return arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.get("value").and_then(|v| v.as_str()).map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

/// Parse LSP Location or Location[] into (path, line, col) tuples.
fn parse_locations(value: &serde_json::Value) -> Vec<(PathBuf, usize, usize)> {
    let locs = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if value.is_object() {
        vec![value.clone()]
    } else {
        return Vec::new();
    };

    locs.iter()
        .filter_map(|loc| {
            let uri = loc.get("uri")?.as_str()?;
            let path = crate::lsp::types::uri_to_path(uri)?;
            let range = loc.get("range")?;
            let start = range.get("start")?;
            let line = start.get("line")?.as_u64()? as usize;
            let col = start.get("character")?.as_u64()? as usize;
            Some((path, line, col))
        })
        .collect()
}

/// Extract the word under the cursor at a byte offset.
fn extract_word_at(text: &str, byte_offset: usize) -> String {
    let bytes = text.as_bytes();
    let mut start = byte_offset;
    let mut end = byte_offset;
    while start > 0 && ((bytes[start - 1] as char).is_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    while end < bytes.len() && ((bytes[end] as char).is_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    text[start..end].to_string()
}

#[cfg(test)]
mod sidebar_scroll_tests {
    use super::*;

    fn dummy_entries(n: usize) -> Vec<TreeEntry> {
        (0..n)
            .map(|i| TreeEntry {
                path: PathBuf::from(format!("entry{}", i)),
                depth: 0,
                is_dir: false,
                expanded: false,
            })
            .collect()
    }

    fn state_with(n: usize) -> SidebarState {
        SidebarState {
            entries: dummy_entries(n),
            selected: 0,
            scroll_offset: 0,
            root: PathBuf::from("/"),
        }
    }

    #[test]
    fn scroll_by_clamps_at_zero() {
        let mut sb = state_with(20);
        sb.scroll_offset = 5;
        sb.scroll_by(-100, 10);
        assert_eq!(sb.scroll_offset, 0);
    }

    #[test]
    fn scroll_by_clamps_at_max_keeps_viewport_full() {
        let mut sb = state_with(20);
        // viewport_rows=10 → max_scroll = 20 - 10 = 10
        sb.scroll_by(100, 10);
        assert_eq!(sb.scroll_offset, 10);
    }

    #[test]
    fn scroll_by_no_scroll_when_all_fit() {
        let mut sb = state_with(5);
        sb.scroll_by(100, 10);
        assert_eq!(sb.scroll_offset, 0);
    }

    #[test]
    fn scroll_by_zero_viewport_pins_offset() {
        let mut sb = state_with(20);
        sb.scroll_by(100, 0);
        assert_eq!(sb.scroll_offset, 0);
    }

    #[test]
    fn ensure_selected_visible_scrolls_down_when_below() {
        let mut sb = state_with(50);
        sb.selected = 30;
        sb.scroll_offset = 0;
        sb.ensure_selected_visible(10);
        // selected=30, viewport=10 → scroll_offset = 30 + 1 - 10 = 21
        assert_eq!(sb.scroll_offset, 21);
    }

    #[test]
    fn ensure_selected_visible_scrolls_up_when_above() {
        let mut sb = state_with(50);
        sb.selected = 5;
        sb.scroll_offset = 20;
        sb.ensure_selected_visible(10);
        assert_eq!(sb.scroll_offset, 5);
    }

    #[test]
    fn ensure_selected_visible_no_op_when_already_in_view() {
        let mut sb = state_with(50);
        sb.selected = 25;
        sb.scroll_offset = 20;
        sb.ensure_selected_visible(10);
        assert_eq!(sb.scroll_offset, 20);
    }
}
