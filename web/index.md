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
  <p class="hero-version" hidden>
    Latest release:
    <a id="latest-version" href="https://github.com/ErikHellman/txt/releases/latest"></a>
  </p>
</div>

<video src="txt-editor-demo.mp4" autoplay loop muted playsinline style="width:100%;border-radius:8px;border:1px solid var(--border);margin-bottom:3rem;display:block;"></video>

## Why txt

Most terminal editors demand a ceremony before you can edit: configuring plugins, learning a modal system, or fighting with a setup that takes hours to get right. GUI editors are the opposite problem — they are powerful, but reaching for the mouse to edit a config file or fix a typo breaks your flow.

I built `txt` because I spend most of my time in the terminal, and switching to a different tool just to make a quick edit always felt like friction I did not want. The editors that live in the terminal either have steep learning curves built around modal paradigms, or they are too limited to be genuinely useful.

The design principle behind `txt` is simple: **editing should feel like thought**. The editor opens immediately, keystrokes are predictable and discoverable, and the cursor lands exactly where you expect. There is no modal confusion, no mandatory configuration, and no plugin ecosystem to navigate before you can open your first file.

`txt` is not trying to replace your main IDE. It is the editor you reach for when you just need to *edit something* — and you want to stay in the terminal to do it.

## Features

- **Syntax highlighting** via tree-sitter for Rust, Python, JavaScript, TypeScript / TSX, Go, Java, Kotlin, C#, Groovy, Shell, HTML, CSS, JSON, YAML, TOML, Properties, and Markdown (with embedded code blocks)
- **Code folding** — tree-sitter–driven folds with a fold gutter and `▸ N lines` markers; `Ctrl+Shift+[` toggles, `Alt+0` / `Alt+Shift+0` fold and unfold everything
- **Symbols in current file** — `Ctrl+Shift+O` opens a fuzzy picker over every function, class, and declaration in the buffer
- **Sticky scope header** — the enclosing function, class, or module stays pinned at the top of the editor pane while you scroll, with a faint breadcrumb in the status bar
- **Git integration** — inline gutter for added/modified/deleted lines, the current branch in the status bar, and a built-in operations dialog (`Ctrl+Shift+G`) for staging, committing, pushing, and pulling
- **Fuzzy file picker** for fast navigation across large projects
- **Multi-cursor editing** — add cursors above or below with `Alt+Shift+Up/Down`
- **AST-aware selection** — expand and contract selections along the syntax tree
- **Language Server Protocol** — completions, hover docs, go-to-definition, and find references, with a trust-on-first-use prompt before any LSP binary is launched (see [Configuring LSP servers](#configuring-lsp-servers))
- **Snippets** — TextMate-style placeholders with tab stops, loaded per language from `~/.config/txt/snippets/<lang>.toml`
- **Keyboard macros** — record edit sequences into named slots a–z (`Ctrl+Shift+R`) and replay them inside a single undo step (`Ctrl+Alt+R`)
- **Marks and jump list** — set workspace-wide named marks with `Ctrl+M` then a–z, jump back with `Ctrl+'`, and walk the history with `Alt+Left` / `Alt+Right`; everything is persisted under `<workspace>/.txt/`
- **Code formatting** — live indent rules while typing and one-keystroke formatting via external tools (`rustfmt`, `prettier`, etc.) configured per language
- **`.editorconfig` support** — per-file indent style and width, line endings, final newline, and trailing-whitespace trimming, picked up automatically
- **Indent guides and column rulers** at configurable columns (e.g. `rulers = [80, 120]`)
- **Find & replace** with regex and case-sensitive modes
- **File sidebar** with full file management (create, rename, delete, move), `.git` and dot-folders hidden by default, and full mouse support — click to open, scroll to browse, drag the separator to resize
- **Mouse-friendly editor** — click tabs to switch, double-click to select a word, horizontal wheel-scroll, and free vertical scroll past the cursor (snaps back on the next edit)
- **Configurable keybindings** — drop a `~/.config/txt/keybindings.toml`, or pick a preset for VS Code or IntelliJ IDEA from the settings UI
- **File watching** — automatically reloads files changed by external tools

## Install {#install}

### Homebrew (macOS and Linux)

```sh
brew tap ErikHellman/tap
brew install txt
```

Update later with `brew upgrade txt`.

### macOS and Linux (curl)

```sh
curl -fsSL https://raw.githubusercontent.com/ErikHellman/txt/main/install.sh | sh
```

Installs the latest release binary to `~/.local/bin`. Make sure that directory is on your `PATH`. Re-run the same command to update.

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

Opening a directory brings up the file sidebar automatically. Press `F1` at any time to see the full list of key bindings.

### Key bindings

| Key | Action |
|-----|--------|
| `Ctrl+S` | Save file |
| `Ctrl+Q` | Quit |
| `Ctrl+P` | Fuzzy file picker |
| `Ctrl+Shift+O` | Symbols in current file |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+F` | Find |
| `Ctrl+H` | Find & replace |
| `Ctrl+W` | Expand selection to enclosing AST node |
| `Ctrl+Shift+W` | Shrink selection |
| `Ctrl+Shift+[` | Toggle code fold at cursor |
| `Alt+Shift+Up/Down` | Add cursor above / below |
| `Alt+M` | Jump to matching bracket |
| `Alt+L` | Center the viewport on the cursor |
| `Alt+Left` / `Alt+Right` | Walk the jump list |
| `Ctrl+Z` | Undo |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Redo |
| `Ctrl+Shift+G` | Git operations dialog |
| `Ctrl+L` | Configure LSP server |
| `Ctrl+,` | Open settings |
| `F1` | Show all key bindings |

The default `txt` keymap is shown above. VS Code and IntelliJ IDEA presets are available from the settings UI (`Ctrl+,`), and a `~/.config/txt/keybindings.toml` overrides individual bindings.

## Configuring LSP servers

LSP is per-workspace and opt-in. The fastest way to get started is the LSP picker (`Ctrl+L`, or *Configure LSP server* in the command palette): pick a language and `txt` writes the selection to `<workspace>/.txt/lsp.toml` for you. Install the server binary from upstream first — `txt` does not download language servers — and the first time the binary is launched you will be asked to approve it. The SHA-256 of approved binaries is cached in `~/.config/txt/trusted_binaries.json`; changed binaries re-prompt.

### Built-in server presets

The picker ships with presets for the following servers — click through to the upstream project for installation instructions:

- **Rust** — [rust-analyzer](https://rust-analyzer.github.io/)
- **Go** — [gopls](https://github.com/golang/tools/tree/master/gopls)
- **TypeScript / JavaScript** — [typescript-language-server](https://github.com/typescript-language-server/typescript-language-server)
- **Kotlin** — [kotlin-lsp](https://github.com/Kotlin/kotlin-lsp)
- **Java** — [Eclipse JDT Language Server (jdtls)](https://github.com/eclipse-jdtls/eclipse.jdt.ls)
- **C#** — [OmniSharp](https://github.com/OmniSharp/omnisharp-roslyn)

C/C++ (clangd), Python (pyright), Lua (lua-language-server), and Zig (zls) are also in the picker.

### Editing `lsp.toml` directly

For anything beyond the defaults — passing extra flags, pointing at a custom binary, or providing `initializationOptions` — edit `<workspace>/.txt/lsp.toml`:

```toml
enabled = true
server  = "rust-analyzer"

# Plain server with no extra arguments.
[servers.rust-analyzer]
command = "rust-analyzer"

# Pass extra arguments via the `args` array.
[servers.gopls]
command = "gopls"
args    = ["serve", "-rpc.trace"]

# Use a custom binary path and forward an initializationOptions JSON
# payload during the LSP handshake.
[servers.pyright]
command      = "/opt/homebrew/bin/pyright-langserver"
args         = ["--stdio"]
init_options = { pythonPath = "/usr/bin/python3" }

# Multiple servers can live side-by-side; the `server` key at the top
# of the file selects which one is active.
[servers.typescript-language-server]
command = "typescript-language-server"
args    = ["--stdio"]
```

The fields are:

- **`enabled`** — master switch. When `false` (the default), no LSP server is launched and `txt` uses tree-sitter alone.
- **`server`** — which `[servers.<name>]` entry to activate. Switch servers by editing this single line.
- **`[servers.<name>]`** — one table per server. `command` is the executable name or absolute path, `args` is the argument list, and `init_options` is an optional JSON value sent as `initializationOptions` during the LSP handshake.

## License

`txt` is dual-licensed under **[MIT](https://github.com/ErikHellman/txt/blob/main/LICENSE-MIT)** and **[Apache 2.0](https://github.com/ErikHellman/txt/blob/main/LICENSE-APACHE)**. You may choose either license.

## Bug reports and feature requests

Found a bug or have an idea for a feature? Open an issue on GitHub:

**[github.com/ErikHellman/txt/issues](https://github.com/ErikHellman/txt/issues)**

For bugs, please include your operating system, the version of `txt` (run `txt --version`), and the steps needed to reproduce the problem.
