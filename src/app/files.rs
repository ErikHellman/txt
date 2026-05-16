use std::time::{Duration, Instant};

use crate::config::add_to_recent_files;
use crate::watcher::FileWatcher;

use super::util::read_rss_kb;
use super::{AppState, InputMode, SidebarState};

impl AppState {
    /// Persist `last_seen_version = current` so the same overlay isn't shown
    /// again until the next minor/major upgrade.
    pub(super) fn record_version_seen(&mut self) {
        let v = env!("CARGO_PKG_VERSION").to_string();
        if self.config.last_seen_version.as_deref() != Some(v.as_str()) {
            self.config.last_seen_version = Some(v);
            self.config.save();
        }
    }
    pub(super) fn close_tab(&mut self) {
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
    pub(super) fn save_active(&mut self) {
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
            // Persist undo history (opt-in) so Ctrl+Z still works after a
            // restart. Silent on I/O failure.
            if self.config.persistent_undo {
                let tab = self.editor.active();
                if let Some(path) = &tab.path {
                    let content = tab.buffer.to_string();
                    let snap = tab.buffer.history_snapshot();
                    crate::buffer::persistent_undo::save(&self.workspace, path, &content, &snap);
                }
            }
        } else {
            self.input_mode = InputMode::SaveAsPath(String::new());
        }
    }
    /// Owned copy of the row-0 gutter badge string ("↑X.Y.Z") that
    /// `editor_view::render` overlays when a newer release is available. Used
    /// by the layout helpers to widen the gutter width consistently across
    /// renderer and mouse hit-testing.
    pub fn version_badge(&self) -> Option<String> {
        self.version_check.newer_version().map(|v| format!("↑{v}"))
    }
    /// Build a `Session` snapshot of the current editor state and write it
    /// to `<workspace>/.txt/session.json`. Called once on clean shutdown
    /// when `restore_session = true`. Buffers without a saved path are
    /// skipped — they have nothing to reopen on the next launch.
    pub(super) fn save_session(&self) {
        use crate::session::{Session, TabState};
        let mut tabs = Vec::new();
        let mut active_in_session = 0usize;
        let active_idx = self.editor.active_idx;
        for (i, tab) in self.editor.tabs.iter().enumerate() {
            let Some(path) = &tab.path else {
                continue;
            };
            if i <= active_idx {
                active_in_session = tabs.len();
            }
            tabs.push(TabState {
                path: path.clone(),
                cursor_byte: tab.buffer.cursors.primary().byte_offset,
                viewport_top: tab.viewport.scroll_row,
            });
        }
        let session = Session {
            tabs,
            active: active_in_session,
            sidebar_open: self.sidebar.is_some(),
        };
        session.save(&self.workspace);
    }
    /// Open every tab listed in `session` and restore cursor positions /
    /// viewport tops. Files that no longer exist on disk are skipped.
    /// Returns `true` when at least one tab was restored.
    pub fn restore_from_session(&mut self, session: crate::session::Session) -> bool {
        let mut restored_any = false;
        for tab in &session.tabs {
            if !tab.path.exists() {
                continue;
            }
            if self.editor.open_tab(tab.path.clone()).is_ok() {
                let buf = &mut self.editor.active_mut().buffer;
                let bound = tab.cursor_byte.min(buf.rope().len_bytes());
                *buf.cursors.primary_mut() =
                    crate::buffer::cursor::Cursor::from_byte_offset(buf.rope(), bound);
                self.editor.active_mut().viewport.scroll_row = tab.viewport_top;
                restored_any = true;
            }
        }
        if restored_any && session.active < self.editor.tabs.len() {
            self.editor.active_idx = session.active;
        }
        if session.sidebar_open && self.sidebar.is_none() {
            self.sidebar = Some(SidebarState::new());
        }
        restored_any
    }
    /// Called after a file is opened or saved — updates recent files, git gutter,
    /// installs a file watcher, and notifies the LSP server.
    /// Attempt to load persistent undo history for the active tab. No-op
    /// when the feature is disabled, the buffer has no path, or the
    /// on-disk hash doesn't match the buffer content.
    pub fn try_load_persistent_undo_for_active(&mut self) {
        if !self.config.persistent_undo {
            return;
        }
        let path = match self.editor.active().path.clone() {
            Some(p) => p,
            None => return,
        };
        let content = self.editor.active().buffer.to_string();
        if let Some(snap) = crate::buffer::persistent_undo::load(&self.workspace, &path, &content) {
            self.editor.active_mut().buffer.restore_history(snap);
        }
    }
    pub(super) fn after_file_open_or_save(&mut self) {
        if let Some(path) = self.editor.active().path.clone() {
            add_to_recent_files(&path, &self.workspace.clone());
            self.file_watcher = FileWatcher::new(&path);
        }
        // Persistent undo: a freshly opened buffer has an empty history, so
        // we use that as the cue that this is an `open` event (not a
        // `save`). This avoids the load clobbering the in-memory history
        // that we are about to persist on save.
        if !self.editor.active().buffer.can_undo() {
            self.try_load_persistent_undo_for_active();
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
    pub(super) fn reload_active_file(&mut self) {
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
}
