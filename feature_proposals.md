# Feature Proposals — `txt` v0.4.1 → next

A comparative review of `txt` against neighbouring terminal and GUI editors, and a prioritised list of features that would sharpen its identity as a *fast, precise text editor* — explicitly excluding a built‑in terminal and any AI/coding‑assistant integration.

## 1. Where `txt` stands today

`txt` already covers the spine of a modern editor:

- **Buffer**: rope‑based (`ropey`), bounded undo (1000 entries), externally‑watched reload preserving cursor.
- **Selection model**: multi‑cursor with merged overlapping ranges, AST‑aware expand/contract via tree‑sitter (`Ctrl+W` / `Ctrl+Shift+W`), `SelectAllOccurrences`.
- **Navigation**: word / line / file / page motions, jump‑to‑line, jump‑to‑matching‑bracket, viewport scroll without moving cursor, recenter (`Alt+L`).
- **Find/Replace**: regex + case toggles, live match count capped at 10 000, replace‑all in one undo entry — but *single‑file only*.
- **Pickers & overlays**: fuzzy file picker (nucleo), command palette, buffer switcher, recent files, settings, help, welcome / what's‑new.
- **Project**: file sidebar with mouse + keyboard ops (rename, copy, cut, paste, new folder, delete with confirmation), refresh.
- **Code intelligence**: 16 tree‑sitter grammars, optional LSP (completion, hover, definition, references, rename, code actions) with trust‑on‑first‑use binary hashing, external formatter integration, indent rules.
- **Git**: gutter (added / modified / deleted), branch in status bar, full stage/commit/push/pull dialog (`Ctrl+Shift+G`).
- **Customisation**: 4 themes, 3 pre‑built keymaps (TXT, VS Code, IntelliJ), TOML config + Ctrl+, settings overlay.
- **Robust input routing**: explicit modal priority chain, AZERTY‑aware tab shortcuts, configurable bindings.

This is already a serious editor for the niche described in the README — *"reviewing a diff, editing a config file, or making a targeted change."*

## 2. How it compares

| Capability | `txt` | Helix | Kakoune | micro | Neovim | VS Code |
|---|---|---|---|---|---|---|
| Rope buffer | ✅ | ✅ | rope‑like | gap buffer | ✅ | ✅ |
| Tree‑sitter highlighting | ✅ | ✅ | external | ✅ | ✅ | semantic tokens |
| AST‑aware selection | ✅ expand/contract | ✅ object motions | ✅ object motions | ❌ | textobjects plugin | symbol nav |
| Multi‑cursor (column) | line above/below only | ✅ rich | ✅ rich | ✅ + click | block visual | ✅ + Alt‑click + Ctrl+D add‑next |
| Project‑wide find/replace | ❌ | ✅ via `:rg` | ❌ | ✅ | telescope/grep plugins | ✅ |
| Snippets | ❌ | LSP only | ❌ | ✅ plugin | ✅ many | ✅ |
| Macros (record/replay) | ❌ | ✅ | ✅ | ✅ | ✅ q/@ | ✅ |
| Marks / bookmarks | ❌ | ✅ `mx` / `'x` | ✅ | ✅ | ✅ | ✅ |
| Jump list | ❌ | ✅ | ✅ | ❌ | ✅ Ctrl‑O / Ctrl‑I | ✅ Alt+←/→ |
| Code folding | ❌ | tree‑sitter folds | ❌ | manual | ✅ | ✅ |
| Sticky scroll / breadcrumbs | ❌ | ❌ | ❌ | ❌ | plugin | ✅ |
| Indent guides | ❌ | ✅ | ❌ | ✅ | plugin | ✅ |
| Filter selection through shell | ❌ | ✅ `\|` | ✅ `\|` | ❌ | `!` | ❌ |
| Sort / case / dedupe lines | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Increment number under cursor | ❌ | ❌ | ✅ | ❌ | ✅ Ctrl‑A/X | ❌ |
| EditorConfig support | ❌ | ✅ | plugin | ✅ | plugin | ✅ |
| Persistent sessions | ❌ | partial | ❌ | partial | ✅ | ✅ |
| Persistent undo | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| Read clipboard ring / registers | system clipboard | ✅ registers | ✅ registers | system | ✅ registers | ❌ |
| LSP | ✅ | ✅ | external | ✅ plugin | ✅ | ✅ |
| Built‑in terminal | ❌ (intentional) | ❌ | ❌ | ✅ | ✅ | ✅ |

The pattern is clear: `txt` is closer to **Helix in scope** than to micro, but it is missing several day‑to‑day editing primitives that Helix, Kakoune and Vim users reach for constantly. Those are the gaps worth closing.

## 3. Proposed features, prioritised

### Tier 1 — biggest editing leverage, modest scope ✅ shipped

All five Tier 1 features have been implemented on branch
`claude/implement-tier-1-features-Rydjl` (one commit per feature).

These are the changes most likely to make a daily user say *"I can't go back to the version without this."*

#### 1.1 Project‑wide search and replace ✅ implemented

The single largest gap. `Ctrl+Shift+F` should open a project‑wide search overlay backed by ripgrep semantics (or a re‑use of the `ignore` crate, already a dependency, plus `regex`). Results render as a scrollable, fuzzy‑filterable list grouped by file with surrounding context lines; `Enter` opens at the match, `Tab` toggles a replace field, `Ctrl+A` performs an all‑file batched replace with a single transactional undo per buffer.

Why it matters: every other gap on this list can be worked around. This one cannot, and it's the reason most users still drop out to `rg \| sed` or to a heavier editor.

#### 1.2 Add‑cursor‑on‑next‑match (the "Ctrl+D" motion) ✅ implemented

`SelectAllOccurrences` exists but is one‑shot. The far more useful pattern is the iterative one popularised by Sublime / VS Code:

- `Ctrl+D` — if no selection, select word under cursor; otherwise add a cursor at the next occurrence and select it.
- `Ctrl+K Ctrl+D` — skip the current match and move to the next.
- `Ctrl+U` — undo the last added cursor.

This composes with the existing `MultiCursor` machinery and would reuse the search subsystem. Combined with column/box selection (1.3) it covers ~90 % of the multi‑cursor cases the current editor cannot express.

#### 1.3 Box / column selection ✅ implemented

Block selection at arbitrary rectangles (e.g. `Alt+drag` with the mouse, or a new `Alt+Shift+arrows` mode that extends a rectangular region). Pasting a multi‑line clipboard distributes one line per cursor — this works once `MultiCursor` has co‑linear cursors at distinct columns.

#### 1.4 Filter selection through an external command ✅ implemented

Vim's `!` and Helix's `|`: pipe the selection to `sort`, `jq`, `prettier`, `column -t`, `awk`, etc., and replace it with stdout. A status‑bar prompt that runs the command via `std::process::Command` and writes back atomically is ~150 lines, gated by a `disable_shell_filter` config flag for restricted environments.

This is the single feature that moves `txt` into the *power* category without a plugin system, because every Unix user already has a personal toolbox.

#### 1.5 Built‑in line transforms ✅ implemented

Ten compact actions exposed on the command palette and remappable:

| Command | Effect |
|---|---|
| `sort_lines_asc` / `sort_lines_desc` | Stable sort of selected lines |
| `dedupe_lines` | Remove adjacent duplicates within selection |
| `reverse_lines` | Reverse line order in selection |
| `to_upper` / `to_lower` / `to_title` | Case conversion of selection |
| `trim_trailing_whitespace` | Whole buffer or selection |
| `join_lines` | `Ctrl+J` — vim‑style line join |
| `align_on` | Align rows of selection on a chosen character (`=`, `:`, `,`) |
| `increment_number` / `decrement_number` | `Ctrl+A` / `Ctrl+X` on number under cursor; with multiple cursors, generates an arithmetic sequence — instant numbered‑list creation |
| `convert_indent` | Tabs ↔ spaces using `tab_size` |
| `convert_eol` | LF / CRLF normalisation, exposed in status bar |

Each is a few lines on top of the existing `Buffer` API and trivially composes with multi‑cursor.

### Tier 2 — daily quality‑of‑life

#### 2.1 Snippets

A simple TextMate‑style snippet engine (`prefix → body` with `$1`, `$2`, `${1:default}`, `$0` final cursor, no transformations needed). Snippets live in `~/.config/txt/snippets/<lang>.toml`, are filtered by tree‑sitter language id, and surface in the existing completion popup. Tab cycles tab‑stops; Esc collapses to `$0`. Roughly the scope of 800 lines including parser, expander, and tab‑stop tracker.

Worth noting: this dovetails with LSP completions because LSP servers already return snippet bodies in the same syntax — `txt` only needs the runtime.

#### 2.2 Macros (record / replay)

Modal‑editor staple: `Ctrl+Shift+R` starts recording into a slot (`a`–`z`); `Ctrl+Shift+R` again stops. `Ctrl+R` replays. With multi‑cursor and the new `Ctrl+D` motion, macros recorded once apply trivially across an entire file.

Implementation: an `EditorAction` queue tee'd into a `Vec<EditorAction>` while recording, then re‑fed through `update()`. `MouseClick`, `MouseDrag`, and `Unhandled` are skipped. ~200 lines.

#### 2.3 Marks / bookmarks and a jump list

Two related features that share storage:

- **Anonymous jump list**: every cursor‑moving navigation (`GoToDefinition`, `JumpToLine`, click, `Ctrl+P` open) pushes the previous position. `Alt+←` / `Alt+→` walk the history, capped at e.g. 100 entries, persisted per workspace next to `recents.json`.
- **Named marks**: `Ctrl+M` followed by `a`–`z` records a mark; `Ctrl+'` followed by the same key jumps to it. Marks survive file edits because they track byte offsets through a small offset‑rebase table on every edit (the rope already has the data).

Both are cheap and pay back constantly when navigating a multi‑file change.

#### 2.4 Code folding driven by tree‑sitter

Folding ranges are queryable from the existing parse tree (`@fold` captures or balanced node kinds). UI: chevrons in the gutter, `Ctrl+Shift+[` / `Ctrl+Shift+]` to fold/unfold, `Ctrl+K Ctrl+0` fold all. Folded ranges are stored on the `BufferHandle` as a `Vec<Range<usize>>`; `editor_view.rs` already iterates lines for rendering, so the visible‑line walk just skips folded interiors and emits a "..." marker.

#### 2.5 Sticky header / breadcrumbs row

When the cursor is inside a function/class/section, render the enclosing parent on the first row of the editor pane. The information is one tree‑sitter ancestor walk; the cost is one row of viewport. Breadcrumbs in the status bar (`Buffer › Mod › Fn`) are a free byproduct.

#### 2.6 Indent guides and column rulers

Render light vertical lines at every indent multiple, and optional vertical rulers at user‑configured columns (e.g. `rulers = [80, 120]`). Both are render‑only changes in `editor_view.rs` and read‑only fields in config. Indent guides catch indentation bugs in YAML/Python instantly.

#### 2.7 EditorConfig support

`.editorconfig` is the de facto cross‑editor convention. The `editorconfig` Rust crate is small; on file open, override per‑buffer `tab_size`, `indent_style`, `end_of_line`, `insert_final_newline`, `trim_trailing_whitespace`. This is one of the cheapest features to ship and significantly reduces config friction in mixed‑language repos.

#### 2.8 Symbols in file (Ctrl+Shift+O)

A fuzzy picker over all named symbols in the active buffer. The data source is tree‑sitter (`@local.definition.*` captures or per‑grammar definition queries), so it works without an LSP server. The picker is the existing `fuzzy_picker.rs` populated from a different source.

A natural extension is a workspace‑wide symbol index built on file open and refreshed by the existing watcher — same UI keyed off `Ctrl+T`.

### Tier 3 — power features that round out the editor

#### 3.1 Persistent sessions per workspace

Save open tabs, cursor positions, fold state, marks, and sidebar expansion to `<workspace>/.txt/session.json` and restore on `txt .` in the same directory. Opt‑in via config to keep "starts instantly" honest.

#### 3.2 Persistent undo

`Buffer::history` already serialises cleanly. Writing it to `<workspace>/.txt/undo/<file-hash>.bin` on save, and reloading on open, gives Vim‑style cross‑session undo. Bounded by the existing 1000‑entry cap so disk use stays predictable.

#### 3.3 Clipboard ring (registers without modal pain)

A bounded ring of the last *N* yanks/cuts. `Ctrl+Shift+V` opens a small picker showing each entry's first line; `Enter` pastes. The system clipboard remains the default target — the ring is purely additive and lives only in memory.

#### 3.4 Surround / change‑pair

A non‑modal version of vim‑surround:
- `Ctrl+'` followed by a delimiter wraps the current selection in `''`, `""`, `()`, `[]`, `{}`, `<>`, or HTML tags.
- With AST selection (`Ctrl+W` already exists) this turns "wrap this expression in parens" into two keystrokes.

Bracket auto‑close on insertion (`(`→`()`) with smart skip on the matching close is the obvious companion. Both are toggle‑able in config because half the audience hates auto‑pairs.

#### 3.5 Diff‑aware navigation and partial revert

The git gutter already classifies each line. Add:
- `Alt+]` / `Alt+[` — jump to next / previous changed hunk.
- `Ctrl+Shift+U` — revert the hunk at the cursor (replace with `HEAD` content). Exists in `git_dialog.rs` philosophy already.
- Inline diff peek: `Ctrl+Shift+D` opens a small float showing the `HEAD` version of the surrounding lines.

#### 3.6 Trailing whitespace, mixed indentation, tabs visualisation

Already half‑there via `show_whitespace`. Add a subtle red highlight for trailing whitespace at end of line, and a one‑line warning in the status bar when a file mixes tabs and spaces. `format_on_save = false` config keeps it opt‑in.

#### 3.7 Quickfix / location list

A general "list of file:line:col entries with messages" pane reusable for:
- LSP diagnostics across the workspace.
- Project‑wide search results (1.1).
- `cargo check` / `eslint` / arbitrary command output parsed with a configurable regex.

Bound to `Alt+1` / `Alt+2`. Even without auto‑running the build, *opening* the cargo output of a previous run and stepping through errors is a large productivity win for the Rust audience this editor implicitly targets.

### Tier 4 — nice to have

- **Spelling check** for `*.md`, `*.txt`, comments — one buffer pass against `hunspell` or a pure‑Rust dictionary, opt‑in.
- **Soft wrap with indent continuation** (current word‑wrap is hard‑broken at viewport edge).
- **Diff mode** (`txt --diff a b`) using a side‑by‑side two‑column layout reusing existing buffer/viewport code.
- **Read‑only mode toggle** so editing keystrokes display a status‑bar warning instead of mutating.
- **Last‑edit jump** (`Ctrl+Shift+M`, vim `gi`) — single position remembered per buffer.
- **Render whitespace on selection only**, not whole buffer — common compromise.
- **Configurable status bar segments** (path, eol, encoding, indent, branch, position, mode) and a permanent column ruler.
- **`txt --pipe`** mode that reads stdin into a scratch buffer for one‑shot edits inside shell pipelines.

## 4. Suggested ordering

A pragmatic 5‑release roadmap that keeps each release shippable on its own:

| Release | Theme | Headline features |
|---|---|---|
| v0.5 | "Find everywhere" ✅ shipped | 1.1 project search/replace, 1.5 line transforms, 1.4 shell filter |
| v0.6 | "Multi‑cursor upgrade" | 1.2 add‑next ✅, 1.3 box selection ✅, 3.3 clipboard ring |
| v0.7 | "Navigate" | 2.3 marks + jump list, 2.8 file/workspace symbols, 3.5 diff navigation |
| v0.8 | "Rhythm" | 2.2 macros, 2.1 snippets, 3.4 surround/auto‑pairs |
| v0.9 | "Polish" | 2.4 folding, 2.5 sticky header, 2.6 indent guides + rulers, 2.7 EditorConfig, 3.1 sessions, 3.2 persistent undo |

## 5. Constraints to respect

- **Do not introduce a built‑in terminal or AI coding integration** — both are explicitly out of scope.
- **Do not soften the no‑plugin posture.** Every feature above is a first‑party action; none requires a runtime extension API.
- **Do not regress startup time.** Snippets, EditorConfig, persistent sessions, and persistent undo all touch disk; load them lazily after the first frame so cold launch stays at the current ~10 ms target.
- **Keep `unicode-width` pinned to `=0.2.2`.** None of the proposed features should bump it.
- **Stay byte‑offset‑clean.** Marks, folds, and the jump list track byte offsets and rebase through `Buffer::edit` deltas, identical to how cursors do today.
- **Every overlay handler must return `bool`** so global actions (`Quit`, `ToggleHelp`) keep working — the existing routing chain in `AppState::update()` is the contract for new modes.

## 6. What to skip, even though other editors have it

- **Plugin / extension system.** It would compromise the "starts instantly, no daemons" promise and grow the maintenance surface unboundedly.
- **GUI mode.** Out of project scope; the TUI is the value.
- **Modal Vim/Helix editing as a primary mode.** The remappable‑keymap system is the right boundary; users who want modal can keep building keymaps. A first‑party modal mode is a different editor.
- **Workspace settings (`.txt/config.toml`).** EditorConfig is the right cross‑editor solution for per‑project formatting. A second config layer is a maintenance trap.
- **Built‑in linters / debuggers / test runners.** These belong upstream; the quickfix list (3.7) is enough to consume their output.
