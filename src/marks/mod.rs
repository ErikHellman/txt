//! Named marks and a workspace-scoped jump list.
//!
//! Both features track byte offsets that travel with buffer edits via
//! [`rebase_after_edit`]. Persistence mirrors the recent-files pattern:
//! marks live in `<workspace>/.txt/marks.json`, jumps in
//! `<workspace>/.txt/jumps.json`. Both files are silent on I/O errors.
//!
//! Marks survive the buffer being closed and reopened because they are
//! addressed by file path (canonical). The jump list is bounded at
//! [`JUMP_LIST_CAP`] so disk usage stays predictable.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::buffer::history::EditCommand;

/// Hard cap for the jump list. Old entries fall off the front as new ones
/// arrive.
pub const JUMP_LIST_CAP: usize = 100;

// ── Marks ────────────────────────────────────────────────────────────────

/// Named marks per file, keyed by canonical file path and mark character.
#[derive(Default)]
pub struct NamedMarks {
    /// Inner map: file path → mark char → byte offset.
    by_path: HashMap<PathBuf, BTreeMap<char, usize>>,
}

#[derive(Serialize, Deserialize)]
struct MarksOnDisk {
    /// Flat array of `{ path, char, offset }` entries (JSON can't key by char).
    marks: Vec<MarkEntry>,
}

#[derive(Serialize, Deserialize)]
struct MarkEntry {
    path: String,
    char: char,
    offset: usize,
}

impl NamedMarks {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, path: &Path, ch: char, offset: usize) {
        self.by_path
            .entry(canonical(path))
            .or_default()
            .insert(ch, offset);
    }

    pub fn get(&self, path: &Path, ch: char) -> Option<usize> {
        self.by_path.get(&canonical(path))?.get(&ch).copied()
    }

    /// Apply a single [`EditCommand`] to every mark stored for `path` so
    /// offsets stay valid after a buffer edit.
    pub fn rebase_after_edit(&mut self, path: &Path, cmd: &EditCommand) {
        let key = canonical(path);
        let Some(file_marks) = self.by_path.get_mut(&key) else {
            return;
        };
        let mut drop_keys: Vec<char> = Vec::new();
        for (ch, off) in file_marks.iter_mut() {
            match rebase_offset(*off, cmd) {
                Some(o) => *off = o,
                None => drop_keys.push(*ch),
            }
        }
        for ch in drop_keys {
            file_marks.remove(&ch);
        }
        if file_marks.is_empty() {
            self.by_path.remove(&key);
        }
    }

    pub fn load(workspace: &Path) -> Self {
        let path = workspace.join(".txt").join("marks.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        let disk: MarksOnDisk = match serde_json::from_str(&text) {
            Ok(d) => d,
            Err(_) => return Self::default(),
        };
        let mut out = Self::default();
        for entry in disk.marks {
            out.by_path
                .entry(PathBuf::from(entry.path))
                .or_default()
                .insert(entry.char, entry.offset);
        }
        out
    }

    pub fn save(&self, workspace: &Path) {
        let mut marks = Vec::new();
        for (path, file_marks) in &self.by_path {
            for (ch, off) in file_marks {
                marks.push(MarkEntry {
                    path: path.to_string_lossy().into_owned(),
                    char: *ch,
                    offset: *off,
                });
            }
        }
        let disk = MarksOnDisk { marks };
        let dir = workspace.join(".txt");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string(&disk) {
            let _ = std::fs::write(dir.join("marks.json"), text);
        }
    }
}

// ── Jump list ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpEntry {
    pub path: PathBuf,
    pub byte_offset: usize,
}

#[derive(Default)]
pub struct JumpList {
    pub entries: VecDeque<JumpEntry>,
    /// Position within `entries` for back/forward navigation. Equal to
    /// `entries.len()` means "past the newest entry" — i.e. forward is empty.
    pub cursor: usize,
}

impl JumpList {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a new jump destination. Truncates any forward entries past the
    /// current cursor (standard "new branch when stepping back" behaviour)
    /// and caps the list at [`JUMP_LIST_CAP`].
    pub fn push(&mut self, entry: JumpEntry) {
        // Drop the most recent if it's identical (avoid a streak of dupes).
        if let Some(latest) = self.entries.get(self.cursor.saturating_sub(1))
            && latest == &entry
        {
            return;
        }
        self.entries.truncate(self.cursor);
        self.entries.push_back(entry);
        while self.entries.len() > JUMP_LIST_CAP {
            self.entries.pop_front();
        }
        self.cursor = self.entries.len();
    }

    /// Return the previous entry, moving the cursor back one step.
    pub fn back(&mut self) -> Option<JumpEntry> {
        if self.cursor <= 1 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor - 1).cloned()
    }

    /// Return the next entry forward, moving the cursor forward one step.
    pub fn forward(&mut self) -> Option<JumpEntry> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor - 1).cloned()
    }

    /// Rebase every entry whose path matches `path` against `cmd`.
    pub fn rebase_after_edit(&mut self, path: &Path, cmd: &EditCommand) {
        let key = canonical(path);
        let mut drops: Vec<usize> = Vec::new();
        for (idx, entry) in self.entries.iter_mut().enumerate() {
            if canonical(&entry.path) != key {
                continue;
            }
            match rebase_offset(entry.byte_offset, cmd) {
                Some(o) => entry.byte_offset = o,
                None => drops.push(idx),
            }
        }
        for idx in drops.into_iter().rev() {
            self.entries.remove(idx);
            if self.cursor > idx {
                self.cursor -= 1;
            }
        }
    }

    pub fn load(workspace: &Path) -> Self {
        let path = workspace.join(".txt").join("jumps.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        let entries: Vec<JumpEntry> = serde_json::from_str(&text).unwrap_or_default();
        let cursor = entries.len();
        Self {
            entries: entries.into(),
            cursor,
        }
    }

    pub fn save(&self, workspace: &Path) {
        let entries: Vec<&JumpEntry> = self.entries.iter().collect();
        let dir = workspace.join(".txt");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string(&entries) {
            let _ = std::fs::write(dir.join("jumps.json"), text);
        }
    }
}

// ── Offset rebasing ──────────────────────────────────────────────────────

/// Rebase a single byte offset against an `EditCommand`. Returns `None` when
/// the offset was inside a deletion and therefore no longer valid.
fn rebase_offset(off: usize, cmd: &EditCommand) -> Option<usize> {
    match cmd {
        EditCommand::Insert { at, text } => {
            // Inserts at or before our offset push us forward.
            if off >= *at {
                Some(off + text.len())
            } else {
                Some(off)
            }
        }
        EditCommand::Delete { start, end, .. } => {
            if off <= *start {
                Some(off)
            } else if off >= *end {
                Some(off - (end - start))
            } else {
                None
            }
        }
        EditCommand::Replace {
            start,
            end,
            old_text,
            new_text,
        } => {
            let _ = old_text;
            let removed = end - start;
            let added = new_text.len();
            if off <= *start {
                Some(off)
            } else if off >= *end {
                let adjusted = off + added;
                Some(adjusted.saturating_sub(removed))
            } else {
                // Inside the replaced range — collapse to the start.
                Some(*start)
            }
        }
    }
}

/// Canonicalise a path, falling back to its given form on error. Used so two
/// representations of the same file map to the same key.
fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn rebase_insert_after_offset_shifts_offset() {
        let cmd = EditCommand::Insert {
            at: 10,
            text: "xyz".into(),
        };
        assert_eq!(rebase_offset(15, &cmd), Some(18));
        assert_eq!(rebase_offset(10, &cmd), Some(13));
        assert_eq!(rebase_offset(5, &cmd), Some(5));
    }

    #[test]
    fn rebase_delete_drops_offsets_inside() {
        let cmd = EditCommand::Delete {
            start: 5,
            end: 10,
            deleted: "12345".into(),
        };
        assert_eq!(rebase_offset(3, &cmd), Some(3));
        assert_eq!(rebase_offset(5, &cmd), Some(5));
        assert_eq!(rebase_offset(7, &cmd), None);
        assert_eq!(rebase_offset(10, &cmd), Some(5));
        assert_eq!(rebase_offset(15, &cmd), Some(10));
    }

    #[test]
    fn named_marks_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut marks = NamedMarks::new();
        marks.set(&p("/some/file.rs"), 'a', 100);
        marks.set(&p("/some/file.rs"), 'b', 200);
        marks.save(dir.path());
        let loaded = NamedMarks::load(dir.path());
        // Note: canonicalize() will fail for non-existent paths, so the
        // stored key falls back to the input path.
        let v = loaded.get(&p("/some/file.rs"), 'a');
        assert_eq!(v, Some(100));
        assert_eq!(loaded.get(&p("/some/file.rs"), 'b'), Some(200));
    }

    #[test]
    fn named_marks_rebase_after_insert() {
        let mut marks = NamedMarks::new();
        marks.set(&p("/tmp/a.rs"), 'a', 50);
        marks.rebase_after_edit(
            &p("/tmp/a.rs"),
            &EditCommand::Insert {
                at: 10,
                text: "abcde".into(),
            },
        );
        assert_eq!(marks.get(&p("/tmp/a.rs"), 'a'), Some(55));
    }

    #[test]
    fn named_marks_rebase_drops_marks_inside_deletion() {
        let mut marks = NamedMarks::new();
        marks.set(&p("/tmp/a.rs"), 'a', 50);
        marks.rebase_after_edit(
            &p("/tmp/a.rs"),
            &EditCommand::Delete {
                start: 30,
                end: 80,
                deleted: "x".repeat(50),
            },
        );
        assert_eq!(marks.get(&p("/tmp/a.rs"), 'a'), None);
    }

    #[test]
    fn jump_list_push_and_navigate() {
        let mut jl = JumpList::new();
        jl.push(JumpEntry {
            path: p("/a"),
            byte_offset: 0,
        });
        jl.push(JumpEntry {
            path: p("/b"),
            byte_offset: 10,
        });
        jl.push(JumpEntry {
            path: p("/c"),
            byte_offset: 20,
        });
        assert_eq!(
            jl.back(),
            Some(JumpEntry {
                path: p("/b"),
                byte_offset: 10
            })
        );
        assert_eq!(
            jl.back(),
            Some(JumpEntry {
                path: p("/a"),
                byte_offset: 0
            })
        );
        assert_eq!(jl.back(), None);
        assert_eq!(
            jl.forward(),
            Some(JumpEntry {
                path: p("/b"),
                byte_offset: 10
            })
        );
    }

    #[test]
    fn jump_list_dedupes_consecutive_pushes() {
        let mut jl = JumpList::new();
        let e = JumpEntry {
            path: p("/a"),
            byte_offset: 0,
        };
        jl.push(e.clone());
        jl.push(e.clone());
        assert_eq!(jl.entries.len(), 1);
    }

    #[test]
    fn jump_list_back_after_push_drops_forward() {
        let mut jl = JumpList::new();
        jl.push(JumpEntry {
            path: p("/a"),
            byte_offset: 0,
        });
        jl.push(JumpEntry {
            path: p("/b"),
            byte_offset: 10,
        });
        jl.back();
        jl.push(JumpEntry {
            path: p("/c"),
            byte_offset: 30,
        });
        assert_eq!(jl.entries.len(), 2);
        assert_eq!(jl.entries.back().unwrap().path, p("/c"));
    }

    #[test]
    fn jump_list_caps_at_limit() {
        let mut jl = JumpList::new();
        for i in 0..(JUMP_LIST_CAP + 5) {
            jl.push(JumpEntry {
                path: p("/x"),
                byte_offset: i,
            });
        }
        assert_eq!(jl.entries.len(), JUMP_LIST_CAP);
    }
}
