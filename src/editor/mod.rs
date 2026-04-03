pub mod tab;
pub mod viewport;

use std::path::PathBuf;

use crate::editor::tab::{BufferHandle, BufferId};

/// Manages all open tabs and tracks which one is active.
pub struct Editor {
    pub tabs: Vec<BufferHandle>,
    pub active_idx: usize,
    next_id: BufferId,
}

impl Editor {
    /// Create an editor with one empty unnamed tab.
    pub fn new() -> Self {
        let handle = BufferHandle::new_empty(0);
        Self {
            tabs: vec![handle],
            active_idx: 0,
            next_id: 1,
        }
    }

    /// Create an editor with one tab opened from `path`.
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let handle = BufferHandle::from_path(0, path)?;
        Ok(Self {
            tabs: vec![handle],
            active_idx: 0,
            next_id: 1,
        })
    }

    // ── Active tab accessors ──────────────────────────────────────────────

    pub fn active(&self) -> &BufferHandle {
        &self.tabs[self.active_idx]
    }

    pub fn active_mut(&mut self) -> &mut BufferHandle {
        &mut self.tabs[self.active_idx]
    }

    // ── Tab management ────────────────────────────────────────────────────

    /// Open a new empty tab and make it active.
    pub fn new_tab(&mut self) -> BufferId {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(BufferHandle::new_empty(id));
        self.active_idx = self.tabs.len() - 1;
        id
    }

    /// Open `path` in a new tab. If the path is already open, switch to it.
    /// Returns `Err` only if the file cannot be read.
    pub fn open_tab(&mut self, path: PathBuf) -> anyhow::Result<()> {
        // Switch to an existing tab if the file is already open.
        if let Some(idx) = self.tabs.iter().position(|t| t.path.as_deref() == Some(&path)) {
            self.active_idx = idx;
            return Ok(());
        }
        let id = self.next_id;
        self.next_id += 1;
        let handle = BufferHandle::from_path(id, path)?;
        self.tabs.push(handle);
        self.active_idx = self.tabs.len() - 1;
        Ok(())
    }

    /// Close the active tab. If it is the last tab, replaces it with an empty one.
    /// Returns `true` if the closed tab had unsaved changes (caller may want to warn).
    pub fn close_active_tab(&mut self) -> bool {
        let had_changes = self.tabs[self.active_idx].buffer.modified;

        if self.tabs.len() == 1 {
            // Keep exactly one tab — replace with empty.
            let id = self.next_id;
            self.next_id += 1;
            self.tabs[0] = BufferHandle::new_empty(id);
            self.active_idx = 0;
        } else {
            self.tabs.remove(self.active_idx);
            if self.active_idx >= self.tabs.len() {
                self.active_idx = self.tabs.len() - 1;
            }
        }

        had_changes
    }

    /// Switch to the next tab (wraps around).
    pub fn next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.active_idx = (self.active_idx + 1) % self.tabs.len();
    }

    /// Switch to the previous tab (wraps around).
    pub fn prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.active_idx = self.active_idx
            .checked_sub(1)
            .unwrap_or(self.tabs.len() - 1);
    }

    /// Switch to tab by 0-based index. No-op if out of range.
    pub fn go_to_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_idx = idx;
        }
    }

    /// Number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns display names for all open buffers (for the buffer switcher).
    pub fn buffer_names(&self) -> Vec<(usize, String)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (i, t.display_name()))
            .collect()
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
