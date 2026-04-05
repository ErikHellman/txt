use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::{
    clipboard::ClipboardManager,
    config::{Config, KeymapPreset, Theme, add_to_recent_files, load_recent_files},
    editor::Editor,
    editor::viewport::screen_pos_to_byte_offset,
    git::GitGutter,
    input::{
        InputHandler,
        action::{Direction, EditorAction, ScrollDir},
        keybinding::KeyBindings,
    },
    search::SearchState,
    ui,
    ui::command_palette::CommandPaletteState,
    ui::editor_view::gutter_width,
    watcher::FileWatcher,
};

/// The scroll amount for a single scroll-wheel tick or Ctrl+Up/Down.
const SCROLL_LINES: usize = 3;

/// Sidebar width in terminal columns.
pub const SIDEBAR_WIDTH: u16 = 28;

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
    pub fn new() -> Self {
        let mut all_files = Vec::new();
        for entry in ignore::WalkBuilder::new(".")
            .hidden(false)
            .git_ignore(true)
            .build()
            .flatten()
        {
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

        scored.sort_by(|a, b| b.0.cmp(&a.0));
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
    pub root: PathBuf,
}

impl SidebarState {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut state = Self {
            entries: Vec::new(),
            selected: 0,
            root: root.clone(),
        };
        state.load_root();
        state
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
                root: dir.clone(),
            };
            tmp.entries_from_dir(&dir, depth + 1, false);
            children.extend(tmp.entries);
            let insert_at = idx + 1;
            for (i, child) in children.into_iter().enumerate() {
                self.entries.insert(insert_at + i, child);
            }
        }
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
            return;
        }
        self.selected = old_selected.min(self.entries.len().saturating_sub(1));
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

// ── AppState ──────────────────────────────────────────────────────────────────

/// All mutable application state. Renderers receive `&AppState` (read-only).
pub struct AppState {
    pub editor: Editor,
    pub clipboard: ClipboardManager,
    pub input_mode: InputMode,
    pub fuzzy_picker: Option<FuzzyPickerState>,
    pub sidebar: Option<SidebarState>,
    pub sidebar_focused: bool,
    saved_sidebar: Option<SidebarState>,
    pub sidebar_clipboard: Option<SidebarClipboard>,
    pub search_state: Option<SearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub show_help: bool,
    pub help_scroll: usize,
    pub show_settings: bool,
    pub settings_cursor: usize,
    pub lsp_picker: Option<LspPickerState>,
    pub completion: Option<CompletionState>,
    pub hover: Option<HoverState>,
    pub references_list: Option<ReferencesListState>,
    pub git_gutter: Option<GitGutter>,
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
    /// Active LSP server connection (None when LSP is disabled or unavailable).
    pub lsp: Option<crate::lsp::LspRegistry>,
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
}

impl AppState {
    pub fn new(editor: Editor, workspace: PathBuf) -> Self {
        let config = Config::load();
        let lsp_config = crate::lsp::config::WorkspaceLspConfig::load(&workspace);
        let lsp = if lsp_config.is_active() {
            crate::lsp::LspRegistry::start(&lsp_config, &workspace).ok()
        } else {
            None
        };
        let mut state = Self {
            editor,
            clipboard: ClipboardManager::new(),
            input_mode: InputMode::Normal,
            fuzzy_picker: None,
            sidebar: None,
            sidebar_focused: false,
            saved_sidebar: None,
            sidebar_clipboard: None,
            search_state: None,
            command_palette: None,
            show_help: false,
            help_scroll: 0,
            show_settings: false,
            settings_cursor: 0,
            lsp_picker: None,
            completion: None,
            hover: None,
            references_list: None,
            git_gutter: None,
            config,
            input: InputHandler::new(),
            workspace,
            should_quit: false,
            confirm_quit: false,
            confirm_delete: None,
            auto_save_timer: None,
            file_watcher: None,
            lsp_config,
            lsp,
            status_error: None,
            lsp_dirty_since: None,
            lsp_change_sent: false,
            term_width: 80,
            term_height: 24,
            memory_rss_kb: 0,
            memory_last_checked: Instant::now(),
        };
        // Apply config to initial buffer.
        if state.config.word_wrap {
            state.editor.active_mut().viewport.word_wrap = true;
        }
        // Compute git gutter for the initial file (if any).
        state.refresh_git_gutter();
        state
    }

    // ── Main dispatch ────────────────────────────────────────────────────────

    pub fn update(&mut self, action: EditorAction, terminal_height: u16) {
        self.term_height = terminal_height;

        // Clear transient status error on any user interaction.
        self.status_error = None;

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

        // Delete confirmation mode
        if self.confirm_delete.is_some() {
            self.handle_confirm_delete(action);
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
        if !self.input_mode.is_normal() {
            self.handle_modal_input(action);
            return;
        }

        // Sidebar focus — intercept navigation when sidebar has focus
        if self.sidebar_focused && self.handle_sidebar_input(&action) {
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

            // ── Text insertion ────────────────────────────────────────
            EditorAction::InsertChar(c) => {
                if self.editor.active().buffer.cursors.is_multi() {
                    self.editor.active_mut().buffer.multi_insert_char(c);
                } else {
                    self.editor.active_mut().buffer.insert_char(c);
                }
            }
            EditorAction::InsertNewline => {
                self.editor.active_mut().buffer.insert_newline();
            }
            EditorAction::InsertTab => {
                let tab_size = self.config.tab_size;
                if self.editor.active().buffer.cursors.is_multi() {
                    let spaces = " ".repeat(tab_size.max(1));
                    self.editor.active_mut().buffer.multi_insert_str(&spaces);
                } else {
                    self.editor.active_mut().buffer.insert_tab(tab_size);
                }
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
                if let Some(offset) = self.screen_to_byte(col, row) {
                    self.editor
                        .active_mut()
                        .buffer
                        .move_cursor_to(offset, false);
                }
            }
            EditorAction::MouseDrag { col, row } => {
                if let Some(offset) = self.screen_to_byte(col, row) {
                    self.editor.active_mut().buffer.move_cursor_to(offset, true);
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
                self.fuzzy_picker = Some(FuzzyPickerState::new());
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
            EditorAction::TriggerCompletion => {
                self.trigger_completion();
            }
            EditorAction::ShowHover => {
                self.trigger_hover();
            }
            EditorAction::GoToDefinition => {
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
            | EditorAction::Unhandled => {}
        }

        // Dismiss hover on any action.
        self.hover = None;

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

    // ── Modal input handling ─────────────────────────────────────────────────

    fn handle_modal_input(&mut self, action: EditorAction) {
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
                    | InputMode::Rename(s) => {
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
                    | InputMode::Rename(s) => {
                        s.pop();
                    }
                    InputMode::Normal => {}
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
                    InputMode::Normal => {}
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
            EditorAction::ToggleHelp | EditorAction::CloseSearch => {
                self.show_help = false;
                true
            }
            _ => false,
        }
    }

    // ── Settings overlay input handling ──────────────────────────────────────

    /// Handle input while the settings overlay is open.
    /// Returns `true` if the action was consumed, `false` to let it fall through.
    fn handle_settings(&mut self, action: &EditorAction) -> bool {
        const NUM_ROWS: usize = 5;
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
            3 => {
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
            4 => {
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

        // Apply new config.
        self.lsp_config = new_config;

        // Start new server if enabled.
        if self.lsp_config.is_active() {
            self.lsp = crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
        }
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
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(sb) = &mut self.sidebar {
                    sb.move_down();
                }
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
            _ => false,
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
            Some(ConfirmDelete::DirConfirmed(path)) => {
                if action == EditorAction::InsertNewline {
                    let _ = std::fs::remove_dir_all(&path);
                    self.refresh_sidebar();
                }
            }
            None => {}
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
            text_edits.sort_by(|a, b| b.0.cmp(&a.0));
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
        // Clear stale state from all buffers.
        for tab in &mut self.editor.tabs {
            tab.lsp_state.diagnostics.clear();
            tab.lsp_state.semantic_tokens = None;
        }
        // Start fresh if config is active.
        if self.lsp_config.is_active() {
            self.lsp = crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
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

    fn screen_to_byte(&self, col: u16, row: u16) -> Option<usize> {
        let editor_area_y: u16 = if self.editor.tab_count() > 1 { 1 } else { 0 };
        // If sidebar is open the editor area starts further right; don't click into sidebar.
        let sidebar_offset: u16 = if self.sidebar.is_some() {
            SIDEBAR_WIDTH + 1
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
                SIDEBAR_WIDTH + 1
            } else {
                0
            };
            let gutter = gutter_width(state.editor.active().buffer.len_lines());
            let text_w = term_width.saturating_sub(gutter + 1 + sidebar_w) as usize;
            state.editor.active_mut().scroll_to_cursor(text_h, text_w);

            // Check for external file changes (non-blocking).
            state.poll_file_watcher();
            state.poll_auto_save();
            state.refresh_memory();

            // Drain pending LSP server updates (non-blocking).
            state.poll_lsp_updates();

            // Flush debounced LSP notifications (didChange, semantic tokens).
            state.flush_lsp_debounce();

            terminal.draw(|frame| ui::render(&state, frame))?;

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
