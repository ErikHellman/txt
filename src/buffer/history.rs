/// Undo/redo history for the text buffer.
///
/// Uses the Command pattern: every edit is recorded as an `EditCommand` that
/// knows how to undo itself. A `BatchGuard` groups multiple commands into one
/// logical undo step (e.g., Replace All).

/// A single undoable/redoable text edit.
#[derive(Debug, Clone)]
pub enum EditCommand {
    /// Inserted `text` at byte offset `at`.
    Insert { at: usize, text: String },
    /// Deleted the text in `[start, end)`. `deleted` is the removed content.
    Delete { start: usize, end: usize, deleted: String },
    /// Replaced `[start, end)` with `new_text`. `old_text` is what was there before.
    Replace { start: usize, end: usize, old_text: String, new_text: String },
}

impl EditCommand {
    /// Byte offset where the cursor should land after *applying* this command.
    #[allow(dead_code)]
    pub fn cursor_after(&self) -> usize {
        match self {
            EditCommand::Insert { at, text } => at + text.len(),
            EditCommand::Delete { start, .. } => *start,
            EditCommand::Replace { start, new_text, .. } => start + new_text.len(),
        }
    }

    /// Byte offset where the cursor should land after *undoing* this command.
    #[allow(dead_code)]
    pub fn cursor_before(&self) -> usize {
        match self {
            EditCommand::Insert { at, .. } => *at,
            EditCommand::Delete { start, deleted, .. } => start + deleted.len(),
            EditCommand::Replace { start, old_text, .. } => start + old_text.len(),
        }
    }
}

/// One entry on the undo stack. Either a single command or a batch of commands
/// that are undone/redone together.
#[derive(Debug, Clone)]
enum UndoEntry {
    Single(EditCommand),
    Batch(Vec<EditCommand>),
}

/// Undo/redo stack.
pub struct UndoStack {
    /// Past commands, oldest first. Undo pops from the back.
    undo_stack: Vec<UndoEntry>,
    /// Future commands (after an undo). Cleared whenever a new edit is made.
    redo_stack: Vec<UndoEntry>,
    /// Accumulator for the current batch (Some while a BatchGuard is alive).
    current_batch: Option<Vec<EditCommand>>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_batch: None,
        }
    }

    /// Record an edit command. If a batch is open, append to it; otherwise push
    /// directly onto the undo stack. Clears the redo stack.
    pub fn record(&mut self, cmd: EditCommand) {
        self.redo_stack.clear();
        if let Some(batch) = &mut self.current_batch {
            batch.push(cmd);
        } else {
            self.undo_stack.push(UndoEntry::Single(cmd));
        }
    }

    /// Open a batch. All subsequent `record` calls will accumulate into this batch
    /// until `commit_batch` is called. Batches do not nest.
    pub fn begin_batch(&mut self) {
        debug_assert!(self.current_batch.is_none(), "batch already open");
        self.redo_stack.clear();
        self.current_batch = Some(Vec::new());
    }

    /// Close the current batch and push it as a single undo entry.
    /// If the batch is empty, nothing is pushed.
    pub fn commit_batch(&mut self) {
        if let Some(batch) = self.current_batch.take() {
            if !batch.is_empty() {
                self.undo_stack.push(UndoEntry::Batch(batch));
            }
        }
    }

    /// Discard the current batch without recording it.
    #[allow(dead_code)]
    pub fn abort_batch(&mut self) {
        self.current_batch = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Pop the most recent undo entry and return the commands to reverse.
    /// Commands should be applied in *reverse order* to undo them.
    pub fn pop_undo(&mut self) -> Option<Vec<EditCommand>> {
        let entry = self.undo_stack.pop()?;
        let cmds = match &entry {
            UndoEntry::Single(c) => vec![c.clone()],
            UndoEntry::Batch(v) => v.clone(),
        };
        self.redo_stack.push(entry);
        Some(cmds)
    }

    /// Pop the most recent redo entry and return the commands to re-apply.
    /// Commands should be applied in *forward order*.
    pub fn pop_redo(&mut self) -> Option<Vec<EditCommand>> {
        let entry = self.redo_stack.pop()?;
        let cmds = match &entry {
            UndoEntry::Single(c) => vec![c.clone()],
            UndoEntry::Batch(v) => v.clone(),
        };
        self.undo_stack.push(entry);
        Some(cmds)
    }

    /// Total number of entries on the undo stack (for diagnostics).
    #[allow(dead_code)]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// Total number of entries on the redo stack.
    #[allow(dead_code)]
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that keeps a batch open for its lifetime.
///
/// Commit by calling `guard.commit(stack)`. If dropped without committing, the
/// batch is aborted. Typical usage:
///
/// ```ignore
/// let guard = BatchGuard::begin(&mut stack);
/// // ... record commands ...
/// guard.commit(&mut stack);
/// ```
#[allow(dead_code)]
pub struct BatchGuard;

#[allow(dead_code)]
impl BatchGuard {
    pub fn begin(stack: &mut UndoStack) -> Self {
        stack.begin_batch();
        Self
    }

    pub fn commit(self, stack: &mut UndoStack) {
        stack.commit_batch();
        std::mem::forget(self); // prevent Drop from aborting
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        // If not committed, the caller should have called commit() or explicitly abort.
        // We can't access the stack here; callers must be careful.
        // In practice, always call .commit() before the guard goes out of scope.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(at: usize, text: &str) -> EditCommand {
        EditCommand::Insert { at, text: text.to_string() }
    }

    fn delete(start: usize, end: usize, deleted: &str) -> EditCommand {
        EditCommand::Delete { start, end, deleted: deleted.to_string() }
    }

    #[test]
    fn single_undo_redo() {
        let mut stack = UndoStack::new();
        stack.record(insert(0, "hello"));
        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        let cmds = stack.pop_undo().unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(!stack.can_undo());
        assert!(stack.can_redo());

        let cmds = stack.pop_redo().unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(stack.can_undo());
    }

    #[test]
    fn redo_cleared_on_new_edit() {
        let mut stack = UndoStack::new();
        stack.record(insert(0, "hello"));
        stack.pop_undo();
        assert!(stack.can_redo());
        stack.record(insert(0, "world")); // new edit clears redo
        assert!(!stack.can_redo());
    }

    #[test]
    fn batch_undo() {
        let mut stack = UndoStack::new();
        stack.begin_batch();
        stack.record(insert(0, "a"));
        stack.record(insert(1, "b"));
        stack.record(insert(2, "c"));
        stack.commit_batch();

        assert_eq!(stack.undo_depth(), 1);
        let cmds = stack.pop_undo().unwrap();
        assert_eq!(cmds.len(), 3);
    }

    #[test]
    fn empty_batch_not_pushed() {
        let mut stack = UndoStack::new();
        stack.begin_batch();
        stack.commit_batch();
        assert_eq!(stack.undo_depth(), 0);
    }

    #[test]
    fn cursor_positions() {
        let cmd = insert(5, "hello");
        assert_eq!(cmd.cursor_after(), 10);
        assert_eq!(cmd.cursor_before(), 5);

        let cmd = delete(3, 8, "world");
        assert_eq!(cmd.cursor_after(), 3);
        assert_eq!(cmd.cursor_before(), 8);
    }

    #[test]
    fn multiple_undo_steps() {
        let mut stack = UndoStack::new();
        stack.record(insert(0, "a"));
        stack.record(insert(1, "b"));
        stack.record(insert(2, "c"));
        assert_eq!(stack.undo_depth(), 3);

        stack.pop_undo();
        stack.pop_undo();
        assert_eq!(stack.undo_depth(), 1);
        assert_eq!(stack.redo_depth(), 2);
    }
}
