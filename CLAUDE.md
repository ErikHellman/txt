# txt — Claude Code Guide

`txt` is a fast terminal text editor written in Rust. It uses **ratatui + crossterm** for TUI rendering, **ropey** for the rope-based text buffer, and **tree-sitter** for syntax highlighting. The architecture is Model-View-Update: `AppState::update()` mutates all state, `ui::render()` draws it every frame.

## Commands

```bash
cargo build                  # debug build
cargo build --release        # optimised release build
cargo test                   # run all unit tests (embedded in modules)
cargo bench                  # criterion benchmarks (benches/buffer.rs)
cargo run -- <file>          # open a file
cargo run -- <directory>     # open a directory — sidebar opens automatically
```

**IMPORTANT: Run `scripts/check.sh` after every code change before finishing.**
It runs the same four checks as CI (fmt, build, clippy, test) and exits on the first failure. Fix all reported issues before committing — `#[allow(dead_code)]` is used deliberately on future API (see below), not as a workaround for real warnings.

## Critical constraints

**IMPORTANT: `unicode-width` is pinned to `=0.2.2`.**
It must exactly match ratatui's internal version. Any version change silently breaks cursor column rendering because display-width calculations diverge. Never bump this dependency.

**IMPORTANT: Use byte offsets, not char offsets, for all rope operations.**
`ropey::Rope` is indexed by byte offset throughout `src/buffer/`. Never do char-index arithmetic directly — always convert via `rope.byte_to_char()` / `rope.char_to_byte()`. Every `Cursor::byte_offset` must be a valid UTF-8 char boundary.

**IMPORTANT: `#[allow(dead_code)]` annotations are intentional.**
Many public methods are future API being built incrementally. Do not remove them. Do not add a crate-level `#![allow(dead_code)]`.

## Architecture

### Key types

| Type | File | Role |
|---|---|---|
| `AppState` | `src/app.rs` | All mutable state: editor, overlays, input mode, sidebar, clipboard |
| `Editor` | `src/editor/mod.rs` | Tab list + active-tab index |
| `BufferHandle` | `src/editor/tab.rs` | One tab: buffer + viewport + path + syntax host |
| `Buffer` | `src/buffer/mod.rs` | Rope + undo stack + multi-cursor |
| `MultiCursor` | `src/buffer/cursor.rs` | Sorted `Vec<Cursor>`; primary cursor drives the viewport |
| `InputHandler` | `src/input/mod.rs` | crossterm events → `EditorAction` |
| `EditorAction` | `src/input/action.rs` | Flat enum of all user operations (zero-allocation, pattern-matchable) |
| `SyntaxHost` | `src/syntax/mod.rs` | tree-sitter parser, parse tree, AST selection history |
| `ui::render` | `src/ui/mod.rs` | Top-level renderer; computes layout, delegates to sub-renderers |

### Input routing priority in `AppState::update()`

Actions are intercepted in this order — higher priority handlers `return` early:

1. `confirm_reload` / `confirm_quit` — y/n prompts
2. `show_help` → `handle_help()` — returns `bool`; unhandled actions fall through
3. `sidebar_focused` → `handle_sidebar_input()` — returns `bool`; unhandled actions fall through
4. `command_palette.is_some()` → `handle_command_palette()` — captures all input
5. `fuzzy_picker.is_some()` → `handle_fuzzy_picker()` — captures all input
6. `search_state.is_some()` → `handle_search_input()` — navigation falls through
7. `!input_mode.is_normal()` → `handle_modal_input()` — status-bar prompts
8. Normal editing dispatch

**When adding a new modal overlay:** make its handler return `bool` so global actions (Quit, ToggleHelp, etc.) are never accidentally swallowed.

## Invariants

- `Cursor::byte_offset` is always a valid UTF-8 char boundary in the rope.
- `MultiCursor::cursors` is always sorted by `byte_offset`; overlapping selections are merged on insert.
- `Selection::anchor` is fixed; `active` moves with the cursor. Always normalised to `start ≤ end` for range ops.
- Syntax highlights are computed only for the visible viewport range — never for the full file.
- Tree-sitter parsing is synchronous and runs after every edit (`SyntaxHost::reparse_rope()`).

## Extension points

**New UI overlay:**
Follow `src/ui/fuzzy_picker.rs` or `src/ui/help_overlay.rs` — centered float rendered last in `ui::render()`, `Option<State>` field on `AppState`, handler that returns `bool` wired into the priority chain above.

**New key binding:**
Add to `src/input/mod.rs` (`handle_key`, `handle_ctrl_char`, or `handle_ctrl_shift_char`) and add a corresponding entry to the `ENTRIES` array in `src/ui/help_overlay.rs`.

**Config & data files** (runtime, not source):
- Config: `~/.config/txt/config.toml`
- Recent files: `~/.config/txt/recent.json`
