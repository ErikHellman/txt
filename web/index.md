---
layout: default
title: txt — fast terminal text editor
---

<div class="hero">
  <h1>txt</h1>
  <p class="tagline">A fast, keyboard-driven terminal text editor for engineers.</p>
  <div class="hero-links">
    <a class="btn btn-primary" href="#install">Install</a>
    <a class="btn btn-secondary" href="https://github.com/ErikHellman/txt">View on GitHub</a>
  </div>
</div>

<video src="txt-editor-demo.mp4" autoplay loop muted playsinline style="width:100%;border-radius:8px;border:1px solid var(--border);margin-bottom:3rem;display:block;"></video>

## Why txt

Most terminal editors demand a ceremony before you can edit: configuring plugins, learning a modal system, or fighting with a setup that takes hours to get right. GUI editors are the opposite problem — they are powerful, but reaching for the mouse to edit a config file or fix a typo breaks your flow.

I built `txt` because I spend most of my time in the terminal, and switching to a different tool just to make a quick edit always felt like friction I did not want. The editors that live in the terminal either have steep learning curves built around modal paradigms, or they are too limited to be genuinely useful.

The design principle behind `txt` is simple: **editing should feel like thought**. The editor opens immediately, keystrokes are predictable and discoverable, and the cursor lands exactly where you expect. There is no modal confusion, no mandatory configuration, and no plugin ecosystem to navigate before you can open your first file.

`txt` is not trying to replace your main IDE. It is the editor you reach for when you just need to *edit something* — and you want to stay in the terminal to do it.

## Features

- **Syntax highlighting** for Rust, Python, JavaScript, and JSON via tree-sitter
- **Git gutter** showing added, modified, and deleted lines inline
- **Fuzzy file picker** for fast navigation across large projects
- **Multi-cursor editing** — add cursors above or below with Alt+Shift+Up/Down
- **AST-aware selection** — expand and contract selections along the syntax tree
- **Language Server Protocol** — completions, hover docs, go-to-definition, and find references
- **Find & replace** with regex and case-sensitive modes
- **File sidebar** with full file management (create, rename, delete, move)
- **File watching** — automatically reloads files changed by external tools

## Install {#install}

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ErikHellman/txt/main/install.sh | sh
```

Installs the latest release binary to `~/.local/bin`. Make sure that directory is on your `PATH`.

### Windows

```powershell
irm https://raw.githubusercontent.com/ErikHellman/txt/main/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\txt` and adds it to your `PATH` automatically.

### Build from source

Requires [Rust](https://rustup.rs) 1.88 or newer.

```sh
git clone https://github.com/ErikHellman/txt.git
cd txt
cargo build --release
```

The binary will be at `./target/release/txt`. Copy it anywhere on your `PATH`.

## Usage

Open a file or a directory:

```sh
txt file.txt
txt .
```

Opening a directory brings up the file sidebar automatically. Press `?` at any time to see the full list of key bindings.

### Key bindings

| Key | Action |
|-----|--------|
| `Ctrl+S` | Save file |
| `Ctrl+Q` | Quit |
| `Ctrl+P` | Fuzzy file picker |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+F` | Find / replace |
| `Ctrl+W` | Expand selection to enclosing AST node |
| `Ctrl+Shift+W` | Shrink selection |
| `Alt+Shift+Up/Down` | Add cursor above / below |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Ctrl+,` | Open settings |
| `?` | Show all key bindings |

## License

`txt` is dual-licensed under **[MIT](https://github.com/ErikHellman/txt/blob/main/LICENSE-MIT)** and **[Apache 2.0](https://github.com/ErikHellman/txt/blob/main/LICENSE-APACHE)**. You may choose either license.

## Bug reports and feature requests

Found a bug or have an idea for a feature? Open an issue on GitHub:

**[github.com/ErikHellman/txt/issues](https://github.com/ErikHellman/txt/issues)**

For bugs, please include your operating system, the version of `txt` (run `txt --version`), and the steps needed to reproduce the problem.
