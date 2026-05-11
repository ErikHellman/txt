//! Code-folding state for a single buffer.
//!
//! Folds are addressed by their **start line number** (0-indexed). Each line
//! number is paired with its current end-line so the editor can hide every
//! row in between. After every reparse, [`FoldState::refresh`] re-derives
//! candidates from the tree-sitter parse tree and reconciles the user's
//! existing folded set with the new candidate ranges so the active folds
//! stay in sync as the buffer is edited.

use std::collections::BTreeMap;

use ropey::Rope;

use crate::buffer::cursor::ByteRange;

/// Per-buffer fold state.
#[derive(Default)]
pub struct FoldState {
    /// Every line range that *could* be folded, derived from the parse tree
    /// on the most recent refresh. Keyed by start line; value is end line.
    candidates: BTreeMap<usize, usize>,
    /// Subset of `candidates` whose start lines are currently folded.
    folded: BTreeMap<usize, usize>,
}

impl FoldState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-derive candidates from `byte_ranges` (typically the output of
    /// [`crate::syntax::SyntaxHost::fold_ranges`]) and drop any folded entry
    /// whose start line no longer maps to a candidate.
    pub fn refresh(&mut self, rope: &Rope, byte_ranges: &[ByteRange]) {
        let mut new_candidates = BTreeMap::new();
        let total_lines = rope.len_lines();
        for r in byte_ranges {
            let s = r.start.min(rope.len_bytes());
            let e = r.end.min(rope.len_bytes());
            let start_line = rope.byte_to_line(s);
            let end_line = rope.byte_to_line(e);
            if end_line > start_line && start_line < total_lines {
                // Keep the *largest* end-line if multiple ranges share the same start.
                new_candidates
                    .entry(start_line)
                    .and_modify(|v| {
                        if end_line > *v {
                            *v = end_line;
                        }
                    })
                    .or_insert(end_line);
            }
        }
        // Reconcile the folded set with the new candidate map.
        self.folded
            .retain(|start, _| new_candidates.contains_key(start));
        for (start, end) in self.folded.iter_mut() {
            if let Some(&new_end) = new_candidates.get(start) {
                *end = new_end;
            }
        }
        self.candidates = new_candidates;
    }

    /// Toggle the fold whose start line is `line`. If `line` isn't itself a
    /// fold-start, fall back to the innermost fold that contains `line`.
    /// Returns `true` when the toggle took effect.
    pub fn toggle_at_line(&mut self, line: usize) -> bool {
        // Innermost candidate containing `line`.
        let target = self
            .candidates
            .iter()
            .filter(|(s, e)| **s <= line && line <= **e)
            .max_by_key(|(s, _)| **s)
            .map(|(s, e)| (*s, *e));
        match target {
            Some((s, e)) => {
                if self.folded.remove(&s).is_some() {
                    return true;
                }
                self.folded.insert(s, e);
                true
            }
            None => false,
        }
    }

    /// Fold every candidate.
    pub fn fold_all(&mut self) {
        self.folded = self.candidates.clone();
    }

    /// Unfold every candidate.
    pub fn unfold_all(&mut self) {
        self.folded.clear();
    }

    /// True when `line` lies strictly *inside* (not on the start line of) any
    /// currently folded range. Used by the renderer to skip hidden rows.
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.folded
            .iter()
            .any(|(&start, &end)| line > start && line <= end)
    }

    /// True when `line` is the start of a currently folded range — the
    /// gutter should show a `▸` chevron.
    pub fn is_fold_start_folded(&self, line: usize) -> bool {
        self.folded.contains_key(&line)
    }

    /// True when `line` is a candidate fold start (whether currently folded
    /// or not) — the gutter can show a faint `▾` chevron.
    pub fn is_fold_start_candidate(&self, line: usize) -> bool {
        self.candidates.contains_key(&line)
    }

    /// End-line of the folded range starting at `line`, if it's folded.
    pub fn folded_end_line(&self, line: usize) -> Option<usize> {
        self.folded.get(&line).copied()
    }

    /// Total count of currently folded regions — for tests.
    #[allow(dead_code)]
    pub fn folded_count(&self) -> usize {
        self.folded.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn rope_with_lines(n: usize) -> Rope {
        let mut s = String::new();
        for i in 0..n {
            s.push_str(&format!("line {i}\n"));
        }
        Rope::from_str(&s)
    }

    fn byte_range_for_lines(rope: &Rope, start_line: usize, end_line: usize) -> ByteRange {
        let s = rope.line_to_byte(start_line);
        // Use start of end_line's next line so the range spans through end_line.
        let next = (end_line + 1).min(rope.len_lines());
        let e = if next == rope.len_lines() {
            rope.len_bytes()
        } else {
            rope.line_to_byte(next).saturating_sub(1)
        };
        ByteRange::new(s, e)
    }

    #[test]
    fn refresh_promotes_candidates_only() {
        let rope = rope_with_lines(10);
        let ranges = vec![byte_range_for_lines(&rope, 1, 5)];
        let mut fs = FoldState::new();
        fs.refresh(&rope, &ranges);
        assert!(fs.is_fold_start_candidate(1));
        assert!(!fs.is_fold_start_folded(1));
        assert_eq!(fs.folded_count(), 0);
    }

    #[test]
    fn toggle_folds_and_unfolds() {
        let rope = rope_with_lines(10);
        let ranges = vec![byte_range_for_lines(&rope, 2, 6)];
        let mut fs = FoldState::new();
        fs.refresh(&rope, &ranges);
        assert!(fs.toggle_at_line(2));
        assert!(fs.is_fold_start_folded(2));
        assert!(fs.is_line_hidden(3));
        assert!(fs.is_line_hidden(6));
        assert!(!fs.is_line_hidden(2));
        assert!(!fs.is_line_hidden(7));
        // Toggle again — unfolded.
        assert!(fs.toggle_at_line(2));
        assert!(!fs.is_fold_start_folded(2));
    }

    #[test]
    fn fold_all_and_unfold_all() {
        let rope = rope_with_lines(10);
        let ranges = vec![
            byte_range_for_lines(&rope, 0, 3),
            byte_range_for_lines(&rope, 5, 8),
        ];
        let mut fs = FoldState::new();
        fs.refresh(&rope, &ranges);
        fs.fold_all();
        assert_eq!(fs.folded_count(), 2);
        fs.unfold_all();
        assert_eq!(fs.folded_count(), 0);
    }

    #[test]
    fn refresh_drops_stale_folds() {
        let rope = rope_with_lines(10);
        let r1 = byte_range_for_lines(&rope, 1, 5);
        let mut fs = FoldState::new();
        fs.refresh(&rope, &[r1]);
        fs.toggle_at_line(1);
        assert!(fs.is_fold_start_folded(1));
        // Reparse with no candidate at line 1 — the fold should be dropped.
        fs.refresh(&rope, &[]);
        assert!(!fs.is_fold_start_folded(1));
    }

    #[test]
    fn toggle_inside_range_folds_the_outer() {
        let rope = rope_with_lines(20);
        let outer = byte_range_for_lines(&rope, 1, 18);
        let inner = byte_range_for_lines(&rope, 5, 10);
        let mut fs = FoldState::new();
        fs.refresh(&rope, &[outer, inner]);
        // Cursor on line 7: the innermost candidate (line 5..10) folds.
        assert!(fs.toggle_at_line(7));
        assert!(fs.is_fold_start_folded(5));
        assert!(!fs.is_fold_start_folded(1));
    }
}
