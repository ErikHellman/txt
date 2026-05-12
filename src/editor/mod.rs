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
    ///
    /// If the editor currently has a single empty, unsaved tab, that tab is
    /// replaced by the newly opened file rather than opening alongside it.
    pub fn open_tab(&mut self, path: PathBuf) -> anyhow::Result<()> {
        // Switch to an existing tab if the file is already open.
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.path.as_deref() == Some(&path))
        {
            self.active_idx = idx;
            return Ok(());
        }
        let id = self.next_id;
        self.next_id += 1;
        let handle = BufferHandle::from_path(id, path)?;
        if self.tabs.len() == 1 && is_empty_unsaved(&self.tabs[0]) {
            self.tabs[0] = handle;
            self.active_idx = 0;
        } else {
            self.tabs.push(handle);
            self.active_idx = self.tabs.len() - 1;
        }
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
        self.active_idx = self
            .active_idx
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

/// A tab is "empty and unsaved" when it has no associated file, no edits, and
/// no content — the state of the placeholder tab created on startup or after
/// closing the last tab.
fn is_empty_unsaved(handle: &BufferHandle) -> bool {
    handle.path.is_none() && !handle.buffer.modified && handle.buffer.rope().len_bytes() == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, contents: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn open_tab_replaces_initial_empty_tab() {
        let dir = write_temp("a.txt", "hello");
        let path = dir.path().join("a.txt");

        let mut editor = Editor::new();
        assert_eq!(editor.tab_count(), 1);
        assert!(editor.active().path.is_none());

        editor.open_tab(path.clone()).unwrap();

        assert_eq!(
            editor.tab_count(),
            1,
            "empty placeholder should be replaced"
        );
        assert_eq!(editor.active_idx, 0);
        assert_eq!(editor.active().path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn open_tab_keeps_modified_empty_tab() {
        let dir = write_temp("a.txt", "hello");
        let path = dir.path().join("a.txt");

        let mut editor = Editor::new();
        // Mark the placeholder as modified — user typed and erased, e.g.
        editor.active_mut().buffer.modified = true;

        editor.open_tab(path).unwrap();

        assert_eq!(
            editor.tab_count(),
            2,
            "modified placeholder must not be discarded"
        );
        assert_eq!(editor.active_idx, 1);
    }

    #[test]
    fn open_tab_keeps_non_empty_placeholder() {
        let dir = write_temp("a.txt", "hello");
        let path = dir.path().join("a.txt");

        let mut editor = Editor::new();
        // Placeholder has content even though `modified` is somehow false.
        editor.active_mut().buffer = crate::buffer::Buffer::from_str("typed text");

        editor.open_tab(path).unwrap();

        assert_eq!(editor.tab_count(), 2);
    }

    #[test]
    fn open_tab_adds_alongside_when_multiple_tabs_open() {
        let dir = write_temp("a.txt", "hello");
        let dir2 = write_temp("b.txt", "world");
        let a = dir.path().join("a.txt");
        let b = dir2.path().join("b.txt");

        let mut editor = Editor::new();
        editor.open_tab(a).unwrap();
        // Now one tab with `a.txt`. Add an explicit empty tab so we have 2.
        editor.new_tab();
        assert_eq!(editor.tab_count(), 2);

        editor.open_tab(b).unwrap();

        assert_eq!(editor.tab_count(), 3, "additional tabs left untouched");
    }

    #[test]
    fn open_tab_switches_to_existing_without_replacing_placeholder() {
        let dir = write_temp("a.txt", "hello");
        let path = dir.path().join("a.txt");

        let mut editor = Editor::new();
        editor.open_tab(path.clone()).unwrap();
        // Add a fresh empty tab and make it active.
        editor.new_tab();
        assert_eq!(editor.tab_count(), 2);
        assert_eq!(editor.active_idx, 1);

        // Re-opening an already-open file should switch to it, leaving the
        // empty tab in place.
        editor.open_tab(path).unwrap();

        assert_eq!(editor.tab_count(), 2);
        assert_eq!(editor.active_idx, 0);
    }
}
