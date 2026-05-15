# src/buffer — rope, cursors, undo

The rope and its cursors live here. Everything outside `src/buffer/` must go through `Buffer`'s public API; reach for the underlying `ropey::Rope` directly only inside this directory.

## Critical invariants (also in root CLAUDE.md)

- **Byte offsets only.** Every cursor/range/edit is in byte offsets, never char offsets. Round-trip via `rope.byte_to_char()` / `rope.char_to_byte()` only when ropey forces it.
- **Char boundaries.** `Cursor::byte_offset` must always be a valid UTF-8 char boundary.
- **Sorted cursors.** `MultiCursor::cursors` is sorted by `byte_offset`; overlapping selections merge on insert.
- **Selection normalisation.** `Selection::anchor` is fixed, `active` moves. Range ops always normalise to `start ≤ end`.

Breaking any of these will not produce a compile error but will silently corrupt cursor placement or selections.

## Edit + history coupling

Every rope mutation must be recorded in the undo history *and* the pending-edits queue (so off-buffer trackers — marks, jumps, fold ranges — can adjust their byte offsets). The helpers that do both atomically:

- **`Buffer::recorded_insert(at, text)`** — rope `insert` + `EditCommand::Insert`.
- **`Buffer::recorded_delete(start, end)`** — rope `delete` + `EditCommand::Delete`. Returns the deleted text.
- **`Buffer::recorded_replace(start, end, new_text)`** — rope `replace` + `EditCommand::Replace`. Returns the replaced text.

Always use these. Calling `rope_edit::insert` (or `::delete`, `::replace`) directly is reserved for the undo/redo apply path in `Buffer::apply_command_*`, which deliberately bypasses recording to avoid double-undo entries.

## Batches

Multi-op edits that should undo as a single entry must wrap the recordings in a batch. The closure form is the only safe one — it cannot leak an open batch on early return:

```rust
self.in_batch(|buf| {
    buf.recorded_delete(start, end);
    buf.recorded_insert(start, &replacement);
});
```

`begin_batch` / `commit_batch` still exist on `Buffer` (public) for external callers in `app.rs`, but new code inside `Buffer` should use `in_batch`. The historical `BatchGuard` struct was removed — it was a no-op stub with no real RAII.

## When extending

- Adding a new editing operation? Reach for `recorded_*` and `in_batch`. Do not duplicate `record(&mut self.history, &mut self.pending_edits, ...)` calls.
- Adding a new cursor mutation? Re-normalise (`MultiCursor::normalize()` does sort + dedup) after the mutation so the sorted/non-overlap invariant holds.
- Adding a new undo-able command type? Update `EditCommand`, the apply/inverse paths in `Buffer::apply_command_forward` and `Buffer::apply_command_inverse`, and add a `recorded_*` helper that ties it to history.
