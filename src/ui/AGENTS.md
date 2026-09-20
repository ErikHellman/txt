# src/ui — overlay & rendering helpers

Every `pub fn render(...)` in this directory is invoked from `ui::mod::render` once per frame. Each module owns a self-contained renderer plus any `*State` types referenced by `AppState`. State mutation lives in `src/app.rs`, never here.

## Shared chrome (do not reimplement)

`overlay_chrome.rs` and `text_utils.rs` exist so the family of centred overlays renders consistently. Use them:

- **`overlay_chrome::draw_border`** — rounded `╭╮╰╯` box around any `Rect`.
- **`overlay_chrome::draw_h_separator`** — `├─────┤` row at row `y` inside the border.
- **`overlay_chrome::fill_rect`** — solid background fill before drawing chrome.
- **`overlay_chrome::render_centered_header`** — write a title centred on a row.
- **`text_utils::truncate_to_width`** — grapheme-aware column-budget truncate, returns `&str`. Default choice for visible UI text.
- **`text_utils::truncate_bytes`** — byte-budget truncate snapped to a UTF-8 boundary, returns `&str`. Use only when the budget really is in bytes.
- **`text_utils::truncate_left_keep_right`** — prefix `…` and keep the right side. For paths and breadcrumbs.

If you find yourself opening a `for x in area.x..area.x + area.width` to paint a row, or pushing `╭ ╮ ╰ ╯` into a buffer by hand, stop and use the helper instead. The local copies were removed in the duplication cleanup; reintroducing them is a regression.

## Overlay file conventions

- File name matches the feature (`fuzzy_picker.rs`, `lsp_picker.rs`, …).
- Public surface is the renderer plus any `*State` struct held on `AppState`.
- Render order is decided by `ui::mod::render`. Overlays paint on top of the editor buffer last, in the same priority order as their input handlers in `AppState::update()`.
- Renderers must be pure: read `&State`, write to `&mut TermBuffer`. No I/O, no mutation of `AppState`.

## Tests

Renderers have `#[cfg(test)]` smoke tests that exercise tiny / narrow areas to confirm they don't panic. When adding a new overlay, add at least:

- A `render_does_not_panic_on_narrow_widths` test that sweeps small `Rect`s.
- A `render_skips_tiny_area` test that asserts the bail-out branch.

Visual regression is covered by the PTY suite (`tests/ui_*.rs`) — that lives outside this directory.
