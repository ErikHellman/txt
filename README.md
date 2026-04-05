# txt

[![CI](https://github.com/ErikHellman/txt/actions/workflows/ci.yml/badge.svg)](https://github.com/ErikHellman/txt/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ErikHellman/txt)](https://github.com/ErikHellman/txt/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue)](https://github.com/ErikHellman/txt#license)

A fast terminal text editor for engineers who want to make quick, precise edits without leaving the terminal.

## Why txt

- **Starts instantly.** No bundled language servers, no plugin system, no background daemons. LSP support is available when you need it — bring your own server.
- **No configuration required.** Works out of the box with sensible defaults; a TOML config is available when you want it.
- **Keyboard-driven.** Familiar shortcuts (Ctrl+S, Ctrl+F, Ctrl+Z), fuzzy file picker, multi-cursor editing, and AST-aware selection — all without a mouse.
- **Just editing.** No built-in terminal, no code execution, no extension marketplace. The right tool for reviewing a diff, editing a config file, or making a targeted change alongside an AI coding agent.

## Installation

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ErikHellman/txt/main/install.sh | sh
```

Installs the latest release to `~/.local/bin/txt`. Run the same command to update.

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ErikHellman/txt/main/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\txt\txt.exe` and adds it to your user `PATH`.

### Build from source

Requires Rust 1.88 or later.

```sh
git clone https://github.com/ErikHellman/txt.git
cd txt
cargo build --release
# Binary is at target/release/txt
```

### Manual download

Download the archive for your platform from the [Releases page](https://github.com/ErikHellman/txt/releases), extract the binary, and place it on your `PATH`. Verify integrity with `checksums.txt`:

```sh
sha256sum --check checksums.txt
```

### Homebrew / AUR

> Not yet available.

## Quick start

```sh
txt                  # open an empty buffer
txt file.rs          # open a file
txt .                # open a directory (file sidebar opens automatically)
txt --help           # show CLI reference
```

## Keyboard reference

> Press **F1** inside the editor for a scrollable in-app reference.
>
> All keyboard shortcuts below are the **defaults** and can be customized — see [Configurable keybindings](#configurable-keybindings) below.

### Navigation

| Key | Action |
|-----|--------|
| Arrow keys | Move cursor |
| Ctrl+Left / Ctrl+Right | Jump to previous / next word |
| Home / End | Line start / end |
| Ctrl+Home / Ctrl+End | File start / end |
| PageUp / PageDown | Page up / down |
| Ctrl+Up / Ctrl+Down | Scroll viewport without moving cursor |

### Selection

| Key | Action |
|-----|--------|
| Shift+Arrows | Extend selection character by character |
| Ctrl+Shift+Left / Right | Extend selection word by word |
| Shift+Home / End | Extend to line start / end |
| Ctrl+Shift+Home / End | Extend to file start / end |
| Ctrl+A | Select all |

### Multi-cursor

| Key | Action |
|-----|--------|
| Alt+Shift+Down | Add cursor on the line below |
| Alt+Shift+Up | Add cursor on the line above |
| Esc | Collapse all cursors to the primary cursor |

All editing and navigation operations apply to every cursor simultaneously.

### AST-aware selection

| Key | Action |
|-----|--------|
| Ctrl+W | Expand selection to enclosing syntax node |
| Ctrl+Shift+W | Contract selection back one node |
| Ctrl+Shift+L | Select all occurrences of current selection |

### Editing

| Key | Action |
|-----|--------|
| Ctrl+D | Duplicate current line |
| Alt+Up / Alt+Down | Move line up / down |
| Ctrl+/ | Toggle line comment |
| Ctrl+Backspace | Delete word backward |
| Ctrl+Delete | Delete word forward |

### Clipboard

| Key | Action |
|-----|--------|
| Ctrl+C | Copy selection (or current line if no selection) |
| Ctrl+X | Cut selection (or current line) |
| Ctrl+V | Paste |
| Ctrl+Shift+C | Copy file reference |

### Undo / Redo

| Key | Action |
|-----|--------|
| Ctrl+Z | Undo |
| Ctrl+Y / Ctrl+Shift+Z | Redo |

### File & tabs

| Key | Action |
|-----|--------|
| Ctrl+S | Save |
| Ctrl+Shift+S | Save as |
| Ctrl+N | New file |
| Ctrl+T | New tab |
| Ctrl+O | Open file (path prompt) |
| Ctrl+F4 | Close tab |
| Ctrl+] / Ctrl+PgDn | Next tab |
| Ctrl+[ / Ctrl+PgUp | Previous tab |
| Ctrl+1 – Ctrl+9 | Jump to tab by index |
| Ctrl+G | Jump to line |

### Panels & pickers

| Key | Action |
|-----|--------|
| Ctrl+P | Fuzzy file picker |
| Ctrl+R | Recent files picker |
| Ctrl+B | Focus / open sidebar |
| Ctrl+Shift+B | Toggle sidebar (show/hide) |
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+E | Buffer switcher |

**Sidebar keys** (when sidebar is focused):

| Key | Action |
|-----|--------|
| Up / Down | Navigate entries |
| Space / Right | Open file or expand/collapse directory (stays in sidebar) |
| Left | Collapse current directory and move to its parent |
| Enter | Open file or toggle directory |
| Ctrl+C | Copy file |
| Ctrl+X | Cut file/directory |
| Ctrl+V | Paste |
| F2 | Rename file/directory |
| Delete | Delete file/directory |
| Ctrl+Shift+N | New folder |

### Search & replace

| Key | Action |
|-----|--------|
| Ctrl+F | Open find bar |
| Ctrl+H | Open find & replace bar |
| F3 / Enter | Next match |
| Shift+F3 | Previous match |
| Alt+R | Toggle regex mode |
| Alt+C | Toggle case-sensitive mode |
| Tab | Switch between find and replace fields |
| Enter (in replace field) | Replace current match and advance |
| Ctrl+A (in replace field) | Replace all matches |
| Esc | Close search bar |

### LSP (when active)

> LSP servers must be installed separately on your system before they can be used. Use **Ctrl+L** to configure which server to launch for the current file type.

| Key | Action |
|-----|--------|
| Ctrl+Space | Code completion |
| Ctrl+K | Hover info |
| F12 | Go to definition |
| Shift+F12 | Find references |
| F2 | Rename symbol |
| Ctrl+. | Code action / quick fix |

### View & app

| Key | Action |
|-----|--------|
| Alt+Z | Toggle word wrap |
| F1 | Toggle help overlay |
| Ctrl+, | Open settings |
| Ctrl+L | Configure LSP server |
| Ctrl+Q | Quit |

### Mouse

| Action | Effect |
|--------|--------|
| Click | Place cursor |
| Click and drag | Extend selection |
| Scroll wheel | Scroll viewport |

## Features

### Syntax highlighting

Tree-sitter powered highlighting with AST-aware selection (Ctrl+W / Ctrl+Shift+W). Language is detected from the file extension.

| Language | Extensions |
|----------|------------|
| Rust | `.rs` |
| Python | `.py`, `.pyw` |
| JavaScript | `.js`, `.mjs`, `.cjs` |
| JSON | `.json`, `.jsonc` |

### Git gutter

Lines modified relative to `HEAD` are marked in the gutter:

- **Added** — line is new since HEAD
- **Modified** — line exists in HEAD but has changed
- **Deleted** — a line from HEAD was removed before this position

The gutter is absent for untracked files or repositories where git is unavailable.

### Find & replace

The find bar (Ctrl+F) shows a live match count and highlights all matches. Ctrl+H adds a replace field. Toggle **[Rx]** for regex and **[Cc]** for case-sensitive mode while searching. `Ctrl+A` in the replace field replaces all matches in a single undo step.

### Fuzzy file picker

Ctrl+P opens a fuzzy picker over all files in the current workspace using the [nucleo](https://github.com/helix-editor/nucleo) library. Type to filter, use arrow keys to select, Enter to open, Esc to close.

### Command palette

Ctrl+Shift+P opens a searchable list of all editor commands with their shortcuts. Useful for discovering bindings or running commands without remembering the key.

### File sidebar

Ctrl+B focuses or opens the sidebar. Ctrl+Shift+B toggles it closed entirely. Navigating the sidebar does not disturb the editor — the active pane is always shown in the status bar mode badge. Space or Right opens a file while keeping sidebar focus; Left collapses the current directory and moves to its parent.

### File watching

When a file is modified externally (by another process or an AI agent writing changes), txt detects the change and reloads the buffer automatically, preserving the current cursor position.

### Multi-cursor editing

Alt+Shift+Down/Up adds a cursor on the adjacent line at the same column. All cursor movements, edits, clipboard operations, and deletions apply to every cursor simultaneously. Esc collapses back to the primary cursor.

## Configuration

**Location:** `~/.config/txt/config.toml`

All fields are optional. The file is created with defaults on first save via the settings overlay (Ctrl+,).

```toml
tab_size = 4              # spaces per indent level
word_wrap = false         # enable word wrap by default
confirm_exit = false      # ask before quitting with unsaved changes
auto_save = false         # auto-save after edits (debounced)
show_whitespace = false   # render whitespace glyphs
theme = "default"         # "default" | "monokai" | "gruvbox" | "nord"
keymap_preset = "default" # "default" | "intellij_idea" | "vscode"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `tab_size` | integer | `4` | Spaces per tab / indent |
| `word_wrap` | bool | `false` | Wrap long lines at viewport width |
| `confirm_exit` | bool | `false` | Prompt before quitting with unsaved changes |
| `auto_save` | bool | `false` | Auto-save after edits |
| `show_whitespace` | bool | `false` | Show visible whitespace glyphs |
| `theme` | string | `"default"` | Color theme |
| `keymap_preset` | string | `"default"` | Keybinding preset |

Settings can also be changed interactively via **Ctrl+,** and are written back to `config.toml` immediately.

**Recent files** are stored per-workspace in `<workspace>/.txt/recents.json` (up to 50 entries). This file is local to each project. If you do not want it to appear in Git, add `.txt/` to your project's `.gitignore`.

### Configurable keybindings

**Location:** `~/.config/txt/keybindings.toml`

All keyboard shortcuts are configurable. The keybindings file is created with defaults on first launch. Each line maps an action name to a key combo:

```toml
# Format: action_name = "key+combo"
# Modifiers: ctrl, alt, shift (order doesn't matter)
# Keys: a-z, 0-9, f1-f12, up, down, left, right, home, end,
#       pageup, pagedown, backspace, delete, esc, enter, tab, space

save_file = "ctrl+s"
open_replace = "ctrl+h"
toggle_line_comment = "ctrl+/"
```

To rebind a shortcut, change the key combo string. For example, to use Ctrl+Shift+Q for quit instead of Ctrl+Q:

```toml
quit = "ctrl+shift+q"
```

#### Presets

Three keybinding presets are available — **Default**, **IntelliJ IDEA**, and **VS Code**. Switch between them interactively via **Ctrl+,** (Settings overlay) or set `keymap_preset` in `config.toml`.

Switching presets copies the selected preset file to `keybindings.toml` and reloads immediately. You can further customize the active keybindings after switching.

All preset files are auto-created in `~/.config/txt/` on first launch:

| File | Description |
|------|-------------|
| `keybindings.toml` | Active keybindings (loaded by the editor) |
| `keybindings-default.toml` | Default preset (copy of original defaults) |
| `keybindings-intellij.toml` | IntelliJ IDEA macOS keymap (Cmd→Ctrl, Opt→Alt) |
| `keybindings-vscode.toml` | VS Code macOS keymap (Cmd→Ctrl, Opt→Alt) |

The IntelliJ and VS Code presets translate macOS modifier keys for terminal use: Cmd becomes Ctrl, and Opt becomes Alt. Notable differences from the defaults include word navigation (Alt+Arrow instead of Ctrl+Arrow), find & replace shortcuts, and LSP key bindings. See the preset files for the full mapping.

## Development

### Prerequisites

- Rust 1.88+ (edition 2024) — install via [rustup](https://rustup.rs)

### Build

```sh
cargo build                  # debug build
cargo build --release        # optimised release build
cargo run -- <file>          # run from source, open a file
cargo run -- <directory>     # run from source, open a directory
```

### Test

```sh
cargo test                   # run all unit tests
cargo bench                  # criterion benchmarks (benches/buffer.rs)
```

Fix all compiler warnings before opening a PR — CI treats warnings as errors.

### CI

Every push to `main` and every pull request runs:
1. `cargo fmt --check`
2. `cargo build --all-targets` (with `-D warnings`)
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test --all-targets`

### Architecture

`txt` follows a **Model-View-Update** pattern: `AppState::update()` mutates all state in response to input; `ui::render()` draws the full frame from that state on every tick. There is no retained UI state — the terminal buffer is fully redrawn each frame.

| Type | File | Role |
|------|------|------|
| `AppState` | `src/app.rs` | All mutable state: editor, overlays, input mode, sidebar, clipboard |
| `Editor` | `src/editor/mod.rs` | Tab list + active-tab index |
| `BufferHandle` | `src/editor/tab.rs` | One tab: buffer + viewport + path + syntax host |
| `Buffer` | `src/buffer/mod.rs` | Rope-based text + undo stack + multi-cursor |
| `MultiCursor` | `src/buffer/cursor.rs` | Sorted `Vec<Cursor>`; primary cursor drives the viewport |
| `InputHandler` | `src/input/mod.rs` | crossterm events → `EditorAction` |
| `EditorAction` | `src/input/action.rs` | Flat enum of all user operations |
| `SyntaxHost` | `src/syntax/mod.rs` | tree-sitter parser, parse tree, AST selection history |
| `ui::render` | `src/ui/mod.rs` | Top-level renderer; computes layout, delegates to sub-renderers |

### Key constraints

- **`unicode-width` is pinned to `=0.2.2`** — must match ratatui's internal version exactly. Any version change silently breaks cursor column rendering.
- **Use byte offsets for all rope operations.** `ropey::Rope` is indexed by byte offset throughout `src/buffer/`. Convert with `rope.byte_to_char()` / `rope.char_to_byte()` — never do char-index arithmetic directly.
- **`#[allow(dead_code)]` annotations are intentional.** Many public methods are future API being built incrementally. Do not remove them.

## Platform support

| Platform | Target triple | Archive |
|----------|---------------|---------|
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| Linux x86_64 (static) | `x86_64-unknown-linux-musl` | `.tar.gz` |
| Linux aarch64 (static) | `aarch64-unknown-linux-musl` | `.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `.zip` |

Linux binaries are statically linked against musl libc and run on any distribution without glibc version constraints.

## Releases

Releases are tagged `vMAJOR.MINOR.PATCH` and built automatically by GitHub Actions for all five platforms. See [RELEASES.md](RELEASES.md) for the full release procedure, distribution channels, and install script details.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
