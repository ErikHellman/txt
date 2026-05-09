# Incremental syntax parsing — design plan

## Status

The current implementation in `src/syntax/mod.rs::reparse_rope` always performs
a **full reparse** by passing `None` as the previous tree to
`Parser::parse`. This is correct but does not benefit from tree-sitter's
incremental re-parse, which can be much faster for large files. Full reparse is
the right default until the buffer can correctly describe edits to tree-sitter.

This document describes the proper incremental-parsing fix that should replace
the full-reparse path once the supporting infrastructure is in place.

## Background — why the previous incremental code was wrong

A previous version of `reparse_rope` did roughly:

```rust
let old_tree = self.tree.take();
self.tree = self.parser.parse(source.as_bytes(), old_tree.as_ref());
```

Tree-sitter's incremental API requires every byte change to be reported to the
old tree via `Tree::edit(&InputEdit { … })` *before* it is passed back into
`Parser::parse`. Without that step, tree-sitter reuses cached subtrees whose
recorded `start_byte`/`end_byte` still match the **old** source. Highlighting
then renders at byte offsets that no longer correspond to the same content,
and the error grows with every keystroke. The visible symptom was: "highlight
colours shift with every character typed". Markdown happened to escape this
because the previous code force-disabled the incremental path for it (citing
"bus errors with incremental").

Because no part of the buffer recorded edits, the `old_tree` argument was
unconditionally stale. Until edits are tracked, full reparse is the only
correct option.

## Goal

Restore incremental parsing so that:

- Highlight spans always reflect the current source content.
- Re-parse cost is proportional to the size of the edited region, not the
  whole file.
- Multi-cursor edits, paste, undo/redo, large indent operations, and
  `save_as`-driven language switches all behave correctly.

## What tree-sitter needs

For each contiguous edit, tree-sitter needs an `InputEdit`:

```rust
pub struct InputEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_position: Point,    // row, column (in *bytes*)
    pub old_end_position: Point,
    pub new_end_position: Point,
}
```

`Point::row` is a 0-based line index; `Point::column` is the byte offset within
that line (not codepoint, not display column). All three positions must be
consistent with the rope **at the moment they describe**: `start_*` is shared
between old and new state, `old_end_*` is from the rope *before* the edit,
`new_end_*` is from the rope *after* the edit.

Then, before each call to `Parser::parse(new_source, Some(&old_tree))`, every
edit since the previous successful parse must have been applied via
`old_tree.edit(&edit)`. Order matters when there are multiple edits.

## Proposed design

### 1. Buffer records InputEdits

`Buffer` (in `src/buffer/mod.rs`) is the single place where the rope is
mutated. Add an internal queue of pending edits drained on every reparse:

```rust
pub struct Buffer {
    rope: Rope,
    // … existing fields …
    pending_edits: Vec<tree_sitter::InputEdit>,
}
```

Every mutation method (`insert_str`, `insert_char`, `insert_newline`,
`insert_tab`, `delete_backward`, `delete_forward`, `delete_range`,
`duplicate_line`, `move_line_up`, `move_line_down`, undo/redo's
`apply_inverse` / `apply_forward`, …) must:

1. Compute `start_byte`, `old_end_byte`, `new_end_byte` from the rope.
2. Compute the corresponding `Point`s using the **rope as it is at the time**
   each end position is taken (i.e. `old_end_position` is read *before* the
   rope is modified, `new_end_position` is read *after*).
3. Push the resulting `InputEdit` onto `pending_edits`.

A small helper such as `Buffer::record_replace(start_byte, old_end_byte,
new_text: &str)` should encapsulate this so individual mutators do not each
re-derive the math.

Multi-cursor edits should push one `InputEdit` per cursor, in the order they
are applied to the rope. The simplest invariant is: edits are pushed in the
exact order in which the rope was mutated. As long as that order matches the
order they will be replayed against `old_tree`, tree-sitter's positions stay
consistent.

### 2. SyntaxHost consumes the queue

`SyntaxHost::reparse_rope` becomes:

```rust
pub fn reparse_rope(&mut self, rope: &Rope, edits: &[InputEdit]) {
    if self.language == Lang::Unknown {
        self.tree = None;
        return;
    }
    let source = rope.to_string();
    let old_tree = self.tree.as_mut().map(|t| {
        for e in edits { t.edit(e); }
        &*t
    });
    self.tree = self.parser.parse(source.as_bytes(), old_tree);
}
```

`BufferHandle::reparse` (and any other call site) drains `pending_edits` and
passes the slice. After a successful reparse the queue is cleared.

If parsing fails (`parse` returns `None`), the queue should also be cleared,
and the next reparse must run from `None` — a partial edit history applied to
a fresh tree is meaningless.

### 3. Cases that must still bypass incremental parse

- **Initial open** (`BufferHandle::from_path`): no previous tree exists; pass
  `&[]` and `None`.
- **Language change** (`BufferHandle::save_as` when extension changes,
  `SyntaxHost::set_language`): the existing tree belongs to a different
  grammar and must be discarded.
- **Markdown**: keep the current full-reparse override until the underlying
  bus-error issue with `tree-sitter-md` incremental is investigated and
  fixed upstream or worked around.
- **External rope replacement** (e.g. file reload from disk): no edit history
  exists for the new content; clear pending edits and reparse from `None`.
- **Undo/redo**: each undone or redone command must push an `InputEdit`
  describing its forward effect on the rope. As long as that holds, the
  incremental path keeps working through history navigation.

### 4. Tests

The minimum bar:

- A test that performs a sequence of inserts and deletes through `Buffer`,
  then calls `reparse`, then asserts that
  `host.highlight_spans(...)` returns spans whose byte ranges correspond to
  tokens at their *current* positions in the rope. The existing
  `spans_correct_after_reparse_with_new_content` test in `src/syntax/mod.rs`
  is a precursor; it only exercises full reparse via the temporary
  `None`-old-tree path.
- A test that verifies multi-cursor edits leave the tree consistent
  (e.g. insert `"foo"` at three cursors, reparse, check span text).
- A test that round-trips through `undo`, asserting span correctness at every
  step.
- A property-style test that fuzzes random edit sequences and asserts the
  tree's reported byte ranges match a freshly-parsed reference tree.

### 5. Phase 7 alignment

The existing comment in `reparse_rope` mentions a future migration to
`Parser::parse_with` + rope chunk callbacks (avoiding the `rope.to_string()`
allocation) and an async background worker. Both compose cleanly with the
incremental design above:

- `parse_with` takes the same `old_tree` parameter, so the edit-tracking
  story is unchanged.
- The async worker should consume `(Rope snapshot, edits since last parse)`
  pairs. The buffer's `pending_edits` queue maps directly onto that channel
  payload.

## Summary

The temporary fix (always full reparse) trades parsing speed for correctness.
The proper incremental fix is straightforward but requires touching every
buffer mutator to record `InputEdit`s and threading them into `reparse_rope`.
It should be implemented alongside (or just before) the Phase 7 move to async
parsing, so the work is done once for both the perf cleanup and the
correctness fix.
