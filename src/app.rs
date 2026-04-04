use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::{
    clipboard::ClipboardManager,
    config::{Config, Theme, add_to_recent_files, load_recent_files},
    editor::Editor,
    editor::viewport::screen_pos_to_byte_offset,
    git::GitGutter,
    input::{
        InputHandler,
        action::{Direction, EditorAction, ScrollDir},
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
        self.entries_from_dir(&self.root.clone(), 0, true);
    }

    /// Append entries for a directory at `depth`. If `expand` is false, only
    /// add the directory entry itself (collapsed).
    fn entries_from_dir(&mut self, dir: &PathBuf, depth: usize, _expand: bool) {
        let mut children: Vec<(PathBuf, bool)> = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let is_dir = path.is_dir();
                // Skip hidden files at depth 0 (but show deeper)
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if depth == 0 && name.starts_with('.') {
                    continue;
                }
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
    pub search_state: Option<SearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub show_help: bool,
    pub help_scroll: usize,
    pub show_settings: bool,
    pub settings_cursor: usize,
    pub git_gutter: Option<GitGutter>,
    pub config: Config,
    pub workspace: PathBuf,
    pub should_quit: bool,
    pub confirm_quit: bool,
    /// Active file watcher for the current buffer (replaced on each file open/save).
    file_watcher: Option<FileWatcher>,
    pub term_width: u16,
    pub term_height: u16,
}

impl AppState {
    pub fn new(editor: Editor, workspace: PathBuf) -> Self {
        let config = Config::load();
        let mut state = Self {
            editor,
            clipboard: ClipboardManager::new(),
            input_mode: InputMode::Normal,
            fuzzy_picker: None,
            sidebar: None,
            sidebar_focused: false,
            saved_sidebar: None,
            search_state: None,
            command_palette: None,
            show_help: false,
            help_scroll: 0,
            show_settings: false,
            settings_cursor: 0,
            git_gutter: None,
            config,
            workspace,
            should_quit: false,
            confirm_quit: false,
            file_watcher: None,
            term_width: 80,
            term_height: 24,
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

        // Help overlay — intercept navigation keys for scrolling
        if self.show_help && self.handle_help(&action) {
            return;
        }

        // Settings overlay — intercept navigation and edits
        if self.show_settings && self.handle_settings(&action) {
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

        // Search / replace bar — captured input (navigation still falls through)
        if self.search_state.is_some() && self.handle_search_input(action.clone()) {
            return;
        }
        // Navigation actions fall through to normal dispatch below.

        // Status-bar modal input modes
        if !self.input_mode.is_normal() {
            self.handle_modal_input(action);
            return;
        }

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
                if self.editor.active().buffer.modified {
                    self.confirm_quit = true;
                } else {
                    self.should_quit = true;
                }
            }
            EditorAction::ForceQuit => {
                self.should_quit = true;
            }
            EditorAction::Unhandled => {}
        }

        // Re-parse the active buffer if it was modified this action.
        if self.editor.active().buffer.modified {
            self.editor.active_mut().reparse();
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
                    InputMode::OpenFilePath(s) | InputMode::SaveAsPath(s) => {
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
                    | InputMode::SaveAsPath(s) => {
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
        const NUM_ROWS: usize = 4;
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
            _ => {}
        }
        self.config.save();
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
            EditorAction::ToggleSidebar => {
                // Ctrl+B: save state and close.
                self.saved_sidebar = self.sidebar.take();
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
            _ => false,
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
        if self.editor.active().buffer.modified {
            self.confirm_quit = true; // reuse confirm for "discard changes?"
        } else {
            self.editor.close_active_tab();
        }
    }

    fn save_active(&mut self) {
        if self.editor.active().path.is_some() {
            let _ = self.editor.active_mut().save();
            self.after_file_open_or_save();
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
    /// and installs a file watcher for the new path.
    fn after_file_open_or_save(&mut self) {
        if let Some(path) = self.editor.active().path.clone() {
            add_to_recent_files(&path, &self.workspace.clone());
            self.file_watcher = FileWatcher::new(&path);
        }
        self.refresh_git_gutter();
    }

    /// Poll the file watcher; if the file changed externally, reload automatically.
    pub fn poll_file_watcher(&mut self) {
        if let Some(watcher) = &self.file_watcher
            && watcher.poll()
        {
            self.reload_active_file();
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

pub struct App {
    input: InputHandler,
}

impl App {
    pub fn new() -> Self {
        Self {
            input: InputHandler::new(),
        }
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

            terminal.draw(|frame| ui::render(&state, frame))?;

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            let action = match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => self.input.handle_key(k),
                Event::Mouse(m) => self.input.handle_mouse(m),
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
