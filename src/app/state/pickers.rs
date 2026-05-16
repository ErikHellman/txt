use std::path::PathBuf;

use crate::search::project::ProjectSearchResults;

// ── Fuzzy picker ─────────────────────────────────────────────────────────────

pub struct FuzzyPickerState {
    pub query: String,
    /// All files in the project directory (populated once on open).
    pub all_files: Vec<PathBuf>,
    /// Scored and sorted (score DESC) indices into `all_files`.
    pub filtered: Vec<(u32, usize)>,
    /// Currently highlighted row (0-based within `filtered`).
    pub selected: usize,
}

impl FuzzyPickerState {
    /// Build by walking the current directory with `ignore` (respects .gitignore).
    /// `hide_git` skips the `.git` directory; `hide_dot` skips every dot-prefixed
    /// directory (and implies `hide_git`).
    pub fn new(hide_git: bool, hide_dot: bool) -> Self {
        let mut all_files = Vec::new();
        let mut builder = ignore::WalkBuilder::new(".");
        builder.hidden(false).git_ignore(true);
        crate::search::apply_hidden_dir_filters(&mut builder, hide_git, hide_dot);
        for entry in builder.build().flatten() {
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                // Strip the leading "./" for display clarity.
                let p = entry.into_path();
                let p = p.strip_prefix("./").map(PathBuf::from).unwrap_or(p);
                all_files.push(p);
            }
        }
        all_files.sort();
        let n = all_files.len().min(200); // show first 200 unfiltered
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files,
            filtered,
            selected: 0,
        }
    }

    /// Re-score the file list against the current query using nucleo.
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;

        if self.query.is_empty() {
            let n = self.all_files.len().min(200);
            self.filtered = (0..n).map(|i| (0u32, i)).collect();
            return;
        }

        use nucleo::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo::{Config, Matcher, Utf32String};

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);

        let mut scored: Vec<(u32, usize)> = self
            .all_files
            .iter()
            .enumerate()
            .filter_map(|(idx, path)| {
                let s = path.to_string_lossy();
                let haystack = Utf32String::from(s.as_ref());
                pattern
                    .score(haystack.slice(..), &mut matcher)
                    .map(|sc| (sc, idx))
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.truncate(200);
        self.filtered = scored;
    }

    /// Build a picker pre-populated with an explicit path list (for recent files).
    pub fn from_paths(paths: Vec<PathBuf>) -> Self {
        let n = paths.len().min(200);
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files: paths,
            filtered,
            selected: 0,
        }
    }

    /// Build a picker pre-populated with open buffer names (for buffer switcher).
    /// The `all_files` list stores synthetic paths using the buffer display name.
    pub fn from_buffers(names: Vec<(usize, String)>) -> Self {
        let all_files: Vec<PathBuf> = names.iter().map(|(_, name)| PathBuf::from(name)).collect();
        let n = all_files.len();
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_files,
            filtered,
            selected: 0,
        }
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.filtered
            .get(self.selected)
            .map(|(_, idx)| &self.all_files[*idx])
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }
}

// ── Symbol-in-file picker ────────────────────────────────────────────────────

/// State for the Ctrl+Shift+O symbol-in-file picker. Mirrors
/// [`FuzzyPickerState`] but scores against symbol names rather than file
/// paths and tracks each symbol's byte range so `Enter` can jump the cursor.
pub struct SymbolPickerState {
    pub query: String,
    /// All symbols collected from the active buffer's parse tree.
    pub all_symbols: Vec<crate::syntax::Symbol>,
    /// Scored and sorted (score DESC) indices into `all_symbols`.
    pub filtered: Vec<(u32, usize)>,
    /// Currently highlighted row.
    pub selected: usize,
}

impl SymbolPickerState {
    /// Build with the symbols already collected from the active buffer.
    pub fn new(symbols: Vec<crate::syntax::Symbol>) -> Self {
        let n = symbols.len();
        let filtered = (0..n).map(|i| (0u32, i)).collect();
        Self {
            query: String::new(),
            all_symbols: symbols,
            filtered,
            selected: 0,
        }
    }

    /// Re-score against the current query using nucleo. Empty query shows
    /// every symbol in source order.
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;

        if self.query.is_empty() {
            let n = self.all_symbols.len();
            self.filtered = (0..n).map(|i| (0u32, i)).collect();
            return;
        }

        use nucleo::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo::{Config, Matcher, Utf32String};

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);

        let mut scored: Vec<(u32, usize)> = self
            .all_symbols
            .iter()
            .enumerate()
            .filter_map(|(idx, sym)| {
                let haystack = Utf32String::from(sym.name.as_str());
                pattern
                    .score(haystack.slice(..), &mut matcher)
                    .map(|sc| (sc, idx))
            })
            .collect();
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        self.filtered = scored;
    }

    pub fn selected_symbol(&self) -> Option<&crate::syntax::Symbol> {
        self.filtered
            .get(self.selected)
            .map(|(_, idx)| &self.all_symbols[*idx])
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }
}

// ── Project search overlay ───────────────────────────────────────────────────

/// State for the project-wide search-and-replace overlay (Ctrl+Shift+F).
pub struct ProjectSearchState {
    pub query: String,
    pub replace_text: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub show_replace: bool,
    pub focus_replace: bool,
    pub results: ProjectSearchResults,
    /// Index into `results.matches` for the highlighted row.
    pub selected: usize,
}

impl ProjectSearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replace_text: String::new(),
            is_regex: false,
            case_sensitive: false,
            show_replace: false,
            focus_replace: false,
            results: ProjectSearchResults::default(),
            selected: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.results.matches.is_empty() && self.selected + 1 < self.results.matches.len() {
            self.selected += 1;
        }
    }
}

impl Default for ProjectSearchState {
    fn default() -> Self {
        Self::new()
    }
}

// ── LSP picker ───────────────────────────────────────────────────────────────

/// Built-in LSP server definitions the user can choose from.
/// Sorted alphabetically by language display name.
pub const LSP_SERVER_OPTIONS: &[(&str, &str, &str, &[&str])] = &[
    // (language display, server key written to lsp.toml, command, args)
    ("C#", "omnisharp", "omnisharp", &["-lsp"]),
    ("C/C++", "clangd", "clangd", &[]),
    ("Go", "gopls", "gopls", &["serve"]),
    ("Java", "jdtls", "jdtls", &[]),
    ("Kotlin", "kotlin-lsp", "kotlin-lsp", &[]),
    ("Lua", "lua-language-server", "lua-language-server", &[]),
    ("Python", "pyright", "pyright-langserver", &["--stdio"]),
    ("Rust", "rust-analyzer", "rust-analyzer", &[]),
    (
        "TypeScript",
        "typescript-language-server",
        "typescript-language-server",
        &["--stdio"],
    ),
    ("Zig", "zls", "zls", &[]),
];

/// State for the LSP configuration picker overlay.
pub struct LspPickerState {
    /// Currently highlighted row. 0 = Disabled, 1..=N = server options.
    pub selected: usize,
}

impl LspPickerState {
    pub fn new(lsp_config: &crate::lsp::config::WorkspaceLspConfig) -> Self {
        // Pre-select the currently active server, or 0 (Disabled).
        let selected = if !lsp_config.is_active() {
            0
        } else {
            lsp_config
                .server
                .as_deref()
                .and_then(|key| {
                    LSP_SERVER_OPTIONS
                        .iter()
                        .position(|(_, name, _, _)| *name == key)
                        .map(|i| i + 1)
                })
                .unwrap_or(0)
        };
        Self { selected }
    }

    pub fn num_rows(&self) -> usize {
        1 + LSP_SERVER_OPTIONS.len() // "Disabled" + servers
    }
}
