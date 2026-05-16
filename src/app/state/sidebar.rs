use std::path::PathBuf;

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
    pub(crate) fn root_is_selected(&self) -> bool {
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
pub(crate) fn copy_target_path(
    source: &std::path::Path,
    dest_dir: &std::path::Path,
) -> Option<PathBuf> {
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

/// State of an in-progress sidebar separator-drag.
#[derive(Debug, Clone, Copy)]
pub struct SidebarDrag {
    /// Column the user pressed the mouse button on (always the separator x).
    pub start_col: u16,
    /// Width the sidebar had when the drag began.
    pub start_width: u16,
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
