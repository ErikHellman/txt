# Changelog

## v0.7.0

- Fix laggy scrolling on large files — holding a scroll key or spinning the wheel could visibly freeze the editor and stall queued keystrokes because every scroll event walked the whole buffer to sum grapheme widths. Reverts the viewport-centre scroll cap and horizontal mouse-wheel handling added in v0.5.0; the longest line and last line are once again allowed to scroll past the viewport edge, and horizontal mouse-wheel events are ignored. The v0.6.0 input-drain loop and no-op-scroll filter are kept.
- Widen the F1 help overlay into a 4-column layout grouped by category, stretching to the terminal width (capped at 160 columns); long key combos and descriptions now wrap on whitespace and slash boundaries instead of being truncated

## v0.6.0

- Add per-workspace session persistence — when `restore_session = true`, a clean shutdown writes the open tabs (with cursor and viewport) to `<workspace>/.txt/session.json` and re-opens them on next launch; a positional file argument suppresses the restore
- Add persistent undo across sessions — when `persistent_undo = true`, every save writes the `UndoStack` and an FNV-1a content hash to `<workspace>/.txt/undo/<digest>.json`; Ctrl+Z reaches back across restarts when the on-disk hash still matches
- Add a clipboard ring (Ctrl+Shift+V) — the last 32 copy/cut values are tracked in a bounded in-memory ring; the overlay shows first-line previews, Enter pastes the entry and promotes it to the front, and adjacent duplicates are coalesced
- Add bracket auto-pair and a surround prompt — typing `(`, `[`, `{`, `"`, `'`, `` ` `` with no selection inserts the matching close and steps between them; quotes are suppressed adjacent to word characters (disable with `auto_pair = false`). Alt+' wraps the selection (or word under the cursor) in a chosen delimiter pair in a single undo batch
- Add diff-aware hunk navigation — Alt+] and Alt+[ jump between git hunks (and push to the jump list), Ctrl+Shift+U reverts the hunk under the cursor to its HEAD content in a single undo batch, Alt+H pops up an inline float showing the HEAD lines, and the status bar gains a `hunk i/n` segment inside a hunk
- Add a quickfix list overlay (Alt+1) for LSP diagnostics across every open buffer, sorted by severity then path/line; F8 and Shift+F8 step through entries without opening the overlay
- Highlight trailing whitespace on inactive lines with a subtle red background and surface a `mixed-indent` status-bar segment when a buffer mixes tab- and space-leading lines (disable via `highlight_trailing_whitespace = false` / `warn_mixed_indent = false`)
- Add an async startup version check — a background thread asks the GitHub Releases API for the latest tag and overlays a `↑X.Y.Z` badge on the first gutter row when a newer version exists; failures are silent. `TXT_DISABLE_VERSION_CHECK` keeps tests offline
- Expose every new Tier 3 toggle in the Ctrl+, settings dialog (restore session, persistent undo, auto-pair brackets, highlight trailing whitespace, warn on mixed indent) so users no longer need to hand-edit `config.toml`
- Add Mac- and compact-keyboard-friendly aliases for document/page navigation: Alt+, / Alt+. (file start/end), Alt+< / Alt+> (extend to file start/end), Alt+V / Alt+Shift+V (page up/down); the existing Home/End/PageUp/PageDown bindings remain canonical
- Switch the Kotlin LSP preset to JetBrains' `kotlin-lsp`
- Drain queued input events per frame and drop no-op vertical scrolls so mouse-wheel bursts and held Ctrl+Up/Down no longer leave the editor a frame behind or queue keystrokes behind a burst
- Fix a help-overlay panic when the key/description column was truncated inside a multibyte character (e.g. the en-dash in `Ctrl+M then a–z`)
- Group the new Tier 3 help entries under their proper sections (surround under Editing, hunk actions under Git, quickfix under LSP)

## v0.5.0

- Add keyboard macros: Ctrl+Shift+R prompts for a slot (a–z) and toggles recording, Ctrl+Alt+R replays a slot inside a single undo batch
- Add TextMate-style snippets with `$1` / `${1:default}` / `$0` tab stops, lazy-loaded per language from `~/.config/txt/snippets/<lang>.toml`; Tab expands a known prefix and cycles stops while a session is active
- Add named marks and a workspace-wide jump list (persisted under `<workspace>/.txt/`): Ctrl+M then a–z sets a mark, Ctrl+' then a–z jumps to it, Alt+Left/Alt+Right walk the jump list (also pushed to before Go To Definition)
- Add tree-sitter–driven code folding with a fold gutter and `▸ N lines` markers: Ctrl+Shift+[ toggles, Alt+0 folds all, Alt+Shift+0 unfolds all
- Add Ctrl+Shift+O symbols-in-file picker — fuzzy-match functions, classes, and other declarations in the current buffer
- Add a sticky header row and status-bar breadcrumb showing the enclosing function/class/module while scrolling (configurable via `sticky_header`)
- Render indent guides at each tab stop and configurable column rulers (`rulers = [80, 120]` in config)
- Honor `.editorconfig` per buffer for indent style/width, line endings, final newline, and trailing-whitespace trimming; the resolved indent is shown in the status bar
- Add `hide_git_folder` (default on) and `hide_dot_folders` settings that prune dot-prefixed directories from the Go To File picker and project-wide search
- Switch tabs by clicking them in the tab bar
- Replace the empty placeholder `[No Name]` tab when opening a file instead of stacking a new tab beside it
- Select a word on double-click in the editor
- Handle horizontal mouse-wheel scrolling
- Cap mouse and keyboard scrolling at the viewport centre so the last line and longest line stay visible
- Expand the LSP server picker with Kotlin, C#, and Java entries, sort it alphabetically by language, and add a notice that servers must be installed externally
- Fix Alt+Left / Alt+Right not walking the jump list
- Fix the sticky header tracking the cursor instead of the scroll position
- Fix mouse selection landing on the wrong character when word wrap is active

## v0.4.2

- Fix crash when arrow-navigating up or down into a line containing wide multi-byte characters (e.g. box-drawing `─` in YAML comments): the cursor's preferred display column was being applied as a byte offset, landing inside a UTF-8 char and panicking on the next render

## v0.4.1

- Fix Ctrl+1..9 tab shortcuts on AZERTY keyboards — the v0.4.0 fix matched a key event the terminal never sends; this version maps the actual top-row glyphs (`& é " ' ( - è _ ç`) to tabs 1..9
- Surface six existing shortcuts in the F1 help overlay that were already bound but undocumented: Shift+Home/End and Ctrl+Shift+Home/End and Shift+PageUp/PageDown (extend selection), Ctrl+Up/Down (scroll viewport), Ctrl+F4 (close tab), and Esc (close search)

## v0.4.0

- Add a git operations dialog (Ctrl+Shift+G) for staging, committing, pushing, and pulling without leaving the editor
- Show the current git branch in the status bar
- Add code formatting: live indent rules while typing and integration with external formatters
- Add F5 to refresh the sidebar file tree
- Set the terminal title to `txt` and the active filename so windows are easy to identify
- Block editor-mutating actions while the sidebar is focused so keystrokes don't accidentally edit the buffer
- Fix Ctrl+1..9 tab shortcuts on AZERTY keyboards
- Fix syntax-highlighting drift caused by a stale tree-sitter tree

## v0.3.1

- Show a welcome panel on first launch and a "What's new" panel after a minor or major upgrade, listing every CHANGELOG section newer than the last dismissed version; patch bumps stay silent
- Scroll the help overlay with the mouse wheel
- Surface the F1 Help hint as a dedicated, brighter segment in the status bar

## v0.3.0

- Add mouse navigation to the sidebar: click to expand/collapse folders or open files, scroll-wheel to scroll independently of the selection, drag the separator column to resize
- Add syntax highlighting for HTML, CSS, Shell, TypeScript/TSX, C#, Java, Go, Kotlin, Groovy, YAML, Properties, and TOML
- Allow scrolling past the cursor: mouse-wheel and Ctrl+Up/Down now move the viewport freely; the cursor stays put and snaps back into view on the next edit or cursor move
- Prompt before launching LSP binaries (trust on first use): SHA-256 hashes are stored in `~/.config/txt/trusted_binaries.json`, and changed binaries re-prompt; mise/asdf shims are canonicalized so toolchain bumps don't re-trigger the prompt
- Bound the undo stack at 1000 entries so long sessions don't grow memory without limit
- Cap search match collection at 10,000 ranges and show truncation in the count (`1/10000+`) to keep pathological queries from stalling the UI
- Add Go To Matching Bracket (Alt+M) — jump the cursor between paired `{}`, `()`, `[]`
- Add Center Viewport On Cursor (Alt+L) — re-center the view without moving the cursor

## v0.2.2

- Fix multi-cursor mode: secondary cursors now render and move correctly
- Fix multi-cursor editing: typing or deleting with multiple cursors no longer produces garbled text

## v0.2.1

- Add markdown syntax highlighting support
- Added customizable keymaps. 3 pre-installed (TXT, VS Code, and IntelliJ IDEA)
- Added kill line action. Copies the line to the paste buffer
- Lots of minor bug fixes and improvments

## v0.1.2

- Bug fixes and improvements

## v0.1.1

- Bug fixes and improvements

## v0.1.0

- Bug fixes and improvements

## v0.0.6

- Initial release

## v0.0.5

- Initial release

## v0.0.4

- Initial release

## v0.0.3

- Initial release

## v0.0.2

- Initial release

## v0.0.1

- Initial release
