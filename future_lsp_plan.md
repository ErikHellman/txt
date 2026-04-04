# Future LSP Plan

## What LSP enables

**High-value features:**
- **Diagnostics** — real compiler errors/warnings with precise ranges (beyond tree-sitter parse errors)
- **Code completion** — context-aware suggestions, auto-import
- **Go to definition / Find references** — cross-file navigation
- **Hover** — type info and documentation under cursor
- **Rename** — project-wide symbol renaming
- **Code actions** — quick fixes, refactors

**Secondary features:**
- **Semantic highlighting** — more accurate than tree-sitter (LSP knows types, not just syntax)
- **Signature help** — parameter hints while typing function calls
- **Inlay hints** — inline type annotations

---

## Memory impact

### External process memory (outside txt's heap)

Language servers run as child processes. txt manages the connection but the RAM cost is entirely theirs:

| Server | Typical RAM usage |
|---|---|
| `rust-analyzer` | 500MB–2GB for a real Rust project |
| `typescript-language-server` | 300–800MB |
| `pyright` | 200–500MB |

### txt's own additional memory per-tab

| What | Estimate | Notes |
|---|---|---|
| `diagnostics: Vec<Diagnostic>` | ~0–50KB | 0–200 items × ~200 bytes each |
| `version: u64` | 8 bytes | Document version for incremental sync |
| Semantic token cache | 50–500KB | ~32 bytes × up to 15k tokens for a large file |
| Completion list | ~20–100KB | Ephemeral — only while popup is open |
| Hover text | ~1–5KB | Ephemeral — held only while displayed |

Semantic tokens are the real trade-off. If cached per-buffer (needed for snappy rendering), 20 open tabs of large files could add ~5MB just for token caches. Mitigation: evict on tab close or only cache the active tab.

### Infrastructure overhead (AppState level)

- One `LspClient` handle per language: minimal (channel pair + process handle)
- Pending request map `HashMap<u64, Callback>`: negligible
- Single dedicated LSP thread: no runtime overhead beyond the thread stack (~8MB default, can be sized down)
- If Tokio were used instead: ~1–2MB runtime overhead (probably overkill)

---

## Architectural changes

### Async model

The current synchronous event loop is the hardest part to adapt. LSP is inherently async (request → response latency: 10ms–2s depending on server load). Best fit for the existing architecture:

**Dedicated LSP thread per server** — receives JSON-RPC messages over the server's stdout, pushes updates into a shared `Mutex<PendingLspUpdates>` that the render loop drains each frame. This:
- Fits the existing sync render loop without introducing Tokio
- Aligns naturally with Phase 7's planned async parsing worker
- Keeps LSP failures isolated from the render path

### Data model changes

`BufferHandle` gains an `LspState` field alongside the existing `SyntaxHost`:

```rust
pub struct LspState {
    pub version: u64,
    pub diagnostics: Vec<Diagnostic>,
    pub semantic_tokens: Option<Vec<SemanticToken>>, // optional cache
}

pub struct Diagnostic {
    pub range: ByteRange,
    pub severity: DiagnosticSeverity, // Error, Warning, Info, Hint
    pub message: String,
    pub source: Option<String>,
}
```

`AppState` gains a server registry:

```rust
pub lsp_servers: HashMap<Lang, LspClient>,
```

Where `LspClient` wraps the child process handle and a send channel.

### UI changes

| Location | Change |
|---|---|
| Gutter (`ui/editor_view.rs`) | Diagnostic severity icons alongside git marks |
| Status bar (`ui/status_bar.rs`) | Error/warning counts for the active buffer |
| New overlay | Completion popup (follows `src/ui/fuzzy_picker.rs` pattern) |
| New overlay | Hover popup (small float near cursor) |
| New overlay | References list (reuse fuzzy picker or new panel) |

### Input routing

Following the existing priority chain in `AppState::update()`, completion popup and hover overlays should be inserted between the fuzzy picker handler and the search handler — both must return `bool` so global actions (Quit, ToggleHelp) are never swallowed.

---

## Implementation phases (suggested)

1. **LSP process management** — spawn/restart servers, JSON-RPC send/receive on a background thread, connect to `AppState` via channel
2. **Diagnostics** — `textDocument/publishDiagnostics` push handler, gutter rendering, status bar counts
3. **Completion** — `textDocument/completion` request on trigger characters, popup overlay
4. **Hover** — `textDocument/hover` on cursor dwell, floating overlay
5. **Go to definition / References** — `textDocument/definition`, `textDocument/references`, open result in new tab or overlay list
6. **Rename** — `textDocument/rename`, apply `WorkspaceEdit` across tabs
7. **Semantic highlighting** — `textDocument/semanticTokens/full`, replace tree-sitter spans for supported languages
8. **Inlay hints / Signature help** — quality-of-life additions once core is stable
