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
    config::{Config, load_recent_files},
    editor::Editor,
    git::GitGutter,
    input::{
        InputHandler,
        action::{Direction, EditorAction, ScrollDir},
    },
    search::SearchState,
    ui,
    ui::command_palette::CommandPaletteState,
    ui::editor_view::effective_gutter_width,
    ui::git_dialog::GitDialogState,
    watcher::FileWatcher,
};

/// Default sidebar width in terminal columns. The active width is stored on
/// `AppState::sidebar_width` so the user can resize by dragging the separator.
pub const DEFAULT_SIDEBAR_WIDTH: u16 = 28;

/// Minimum width the sidebar can be resized to (just enough to show short names).
pub const MIN_SIDEBAR_WIDTH: u16 = 12;

/// Maximum width the sidebar can be resized to in absolute columns. Also clamped
/// to half the terminal width so the editor stays usable.
pub const MAX_SIDEBAR_WIDTH: u16 = 80;

/// Maximum delay between two left-clicks at the same screen cell for the
/// second click to register as a double-click (which selects the word under
/// the cursor).
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

mod completion;
mod editing;
mod files;
mod git;
mod lsp;
mod modal;
mod mouse;
mod search;
mod sidebar;
mod snippets;
pub mod state;
mod util;

pub use state::*;
use util::SCROLL_LINES;

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
    /// Time and screen position of the most recent left-click in the editor
    /// area. A subsequent click at the same `(col, row)` within
    /// `DOUBLE_CLICK_INTERVAL` is treated as a double-click and selects the
    /// word at the click position.
    last_click: Option<(Instant, u16, u16)>,
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
    /// `Ctrl+Shift+V` overlay listing the last N clipboard entries.
    pub clipboard_ring: Option<ClipboardRingState>,
    /// `Alt+H` float showing HEAD content for the hunk at the cursor.
    pub diff_peek: Option<DiffPeekState>,
    /// `Alt+1` overlay listing every LSP diagnostic across the workspace.
    pub quickfix: Option<crate::quickfix::QuickfixState>,
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
    /// Background "is there a newer release?" checker. Always present, but
    /// stays inert when `TXT_DISABLE_VERSION_CHECK` is set (used by tests).
    pub version_check: crate::version_check::VersionChecker,
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
            last_click: None,
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
            clipboard_ring: None,
            diff_peek: None,
            quickfix: None,
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
            version_check: crate::version_check::VersionChecker::spawn(),
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

        // Floating pickers own input whenever they are open, regardless of
        // whether the sidebar or the editor has focus. They must be checked
        // before the sidebar so that typing reaches the picker rather than
        // being swallowed by the sidebar's catch-all.

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

        // Sidebar focus — intercept navigation when sidebar has focus
        if self.sidebar_focused && self.handle_sidebar_input(&action) {
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

        // Clipboard ring picker — captured input
        if self.clipboard_ring.is_some() && self.handle_clipboard_ring(&action) {
            return;
        }

        // Quickfix list — captured input
        if self.quickfix.is_some() && self.handle_quickfix_input(&action) {
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
                // Auto-pair: when the user types one of the configured open
                // delimiters with no active selection, also insert the
                // matching close delimiter and step the cursor back one byte
                // so they keep typing inside the pair. Skip-on-close: when
                // the user types a close delimiter and the next char already
                // is that close, just advance the cursor instead of
                // inserting a duplicate.
                let auto_paired = self.config.auto_pair
                    && !self.editor.active().buffer.cursors.is_multi()
                    && self.try_auto_pair(c);
                if !auto_paired {
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
                    self.last_click = None;
                } else if self.point_on_separator(col, row) {
                    // Begin a sidebar resize drag.
                    self.sidebar_drag = Some(SidebarDrag {
                        start_col: col,
                        start_width: self.sidebar_width,
                    });
                    self.last_click = None;
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
                    self.last_click = None;
                } else {
                    // Click in the editor area: defocus the sidebar (if focused)
                    // and move the cursor to the click target. A second click
                    // at the same cell within DOUBLE_CLICK_INTERVAL selects the
                    // word under the cursor instead of just moving.
                    self.sidebar_focused = false;
                    if let Some(offset) = self.screen_to_byte(col, row) {
                        let is_double_click = self.last_click.is_some_and(|(t, c, r)| {
                            c == col && r == row && t.elapsed() <= DOUBLE_CLICK_INTERVAL
                        });
                        if is_double_click {
                            let span = crate::buffer::cursor::word_span_at(
                                self.editor.active().buffer.rope(),
                                offset,
                            );
                            let buf = &mut self.editor.active_mut().buffer;
                            if let Some((start, end)) = span {
                                buf.move_cursor_to(start, false);
                                buf.move_cursor_to(end, true);
                            } else {
                                buf.move_cursor_to(offset, false);
                            }
                            // Reset so a third click starts a fresh single-click sequence.
                            self.last_click = None;
                        } else {
                            self.editor
                                .active_mut()
                                .buffer
                                .move_cursor_to(offset, false);
                            self.last_click = Some((Instant::now(), col, row));
                        }
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
                        vp.scroll_col = vp.scroll_col.saturating_add(4);
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
            EditorAction::BeginSurround => {
                self.input_mode = InputMode::SurroundChar;
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
            EditorAction::OpenClipboardRing => {
                let entries = self.clipboard.ring_entries();
                if !entries.is_empty() {
                    self.clipboard_ring = Some(ClipboardRingState {
                        entries,
                        selected: 0,
                    });
                }
            }
            EditorAction::NextHunk => {
                self.jump_to_relative_hunk(1);
            }
            EditorAction::PrevHunk => {
                self.jump_to_relative_hunk(-1);
            }
            EditorAction::RevertHunkAtCursor => {
                self.revert_hunk_at_cursor();
            }
            EditorAction::PeekHeadAtCursor => {
                self.toggle_diff_peek();
            }
            EditorAction::OpenQuickfix => {
                let entries = crate::quickfix::collect_lsp_diagnostics(&self.editor);
                if entries.is_empty() {
                    self.status_error = Some("No diagnostics".into());
                } else {
                    self.quickfix = Some(crate::quickfix::QuickfixState::new(entries));
                }
            }
            EditorAction::QuickfixNext => {
                self.quickfix_step(1);
            }
            EditorAction::QuickfixPrev => {
                self.quickfix_step(-1);
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

    // ── Modal input handling ─────────────────────────────────────────────────

    // ── Fuzzy picker input handling ──────────────────────────────────────────

    // ── Project search overlay input handling ────────────────────────────────

    // ── Search input handling ────────────────────────────────────────────────

    // ── Search helpers ────────────────────────────────────────────────────────

    // ── Help overlay input handling ───────────────────────────────────────────

    // ── Settings overlay input handling ──────────────────────────────────────

    // ── LSP picker input handling ────────────────────────────────────────────

    // ── Git operations dialog ────────────────────────────────────────────────

    // ── Sidebar input handling ────────────────────────────────────────────────

    // ── Command palette input handling ───────────────────────────────────────

    // ── Code formatting ───────────────────────────────────────────────────────

    // ── Line comment toggle ───────────────────────────────────────────────────

    // ── File helpers ─────────────────────────────────────────────────────────

    // ── LSP polling ──────────────────────────────────────────────────────────

    /// How long to wait after the last edit before sending `didChange`.
    const LSP_DEBOUNCE: Duration = Duration::from_millis(100);
    /// How long to wait after the last edit before re-requesting semantic tokens.
    const SEMANTIC_TOKEN_DEBOUNCE: Duration = Duration::from_millis(300);

    // ── Completion ───────────────────────────────────────────────────────────

    // ── Hover ────────────────────────────────────────────────────────────────

    // ── Go to Definition ─────────────────────────────────────────────────────

    // ── Find References ──────────────────────────────────────────────────────

    // ── Rename ───────────────────────────────────────────────────────────────

    // ── Code Action ──────────────────────────────────────────────────────────

    // ── Semantic Tokens ──────────────────────────────────────────────────────

    // ── LSP restart/stop ──────────────────────────────────────────────────────

    // ── Coordinate helpers ───────────────────────────────────────────────────
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
        pending_session: Option<crate::session::Session>,
    ) -> Result<()> {
        let mut state = AppState::new(editor, workspace);
        if open_sidebar {
            state.sidebar = Some(SidebarState::new());
            state.sidebar_focused = true;
        }
        // Honour `restore_session = true`: re-open the previously saved
        // tabs before painting the first frame. Skips silently when no
        // session is pending.
        if let Some(session) = pending_session {
            state.restore_from_session(session);
            state.refresh_git_gutter();
        }
        // Load persistent undo for every tab that was opened during
        // startup (either via the positional CLI arg or session restore).
        // Walk by index so we can borrow `state` mutably across iterations;
        // restore the original active index when done.
        let saved_active = state.editor.active_idx;
        for i in 0..state.editor.tabs.len() {
            state.editor.active_idx = i;
            state.try_load_persistent_undo_for_active();
        }
        state.editor.active_idx = saved_active.min(state.editor.tabs.len().saturating_sub(1));

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
            let badge = state.version_badge();
            let gutter =
                effective_gutter_width(state.editor.active().buffer.len_lines(), badge.as_deref());
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
            state.version_check.poll();

            // Drain pending LSP server updates (non-blocking).
            state.poll_lsp_updates();

            // Flush debounced LSP notifications (didChange, semantic tokens).
            state.flush_lsp_debounce();

            terminal.draw(|frame| ui::render(&mut state, frame))?;

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            // Drain everything already queued up before the next draw. A
            // mouse-wheel spin or held arrow key can generate input faster
            // than a single frame can render, so without this the editor
            // falls one render behind every event and visibly "freezes"
            // while it catches up — keystrokes queued behind a scroll
            // burst then take ages to register.
            //
            // A scroll the viewport can't honour (already at the top or
            // bottom) is dropped before it reaches `update`, so a held
            // wheel at end-of-file costs nothing.
            loop {
                let action = match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => state.input.handle_key(k),
                    Event::Mouse(m) => state.input.handle_mouse(m),
                    Event::Resize(_, _) => EditorAction::Unhandled,
                    _ => EditorAction::Unhandled,
                };
                if !state.scroll_action_is_no_op(&action) {
                    state.update(action, term_height);
                    if state.should_quit {
                        break;
                    }
                }
                if !event::poll(Duration::from_millis(0))? {
                    break;
                }
            }

            if state.should_quit {
                break;
            }
        }

        // Persist the session for the next launch when the user has opted
        // in. Silent on I/O failures.
        if state.config.restore_session {
            state.save_session();
        }

        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
