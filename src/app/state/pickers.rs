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

// ── Sidebar file search ────────────────────────────────────────────────

/// State for the Ctrl+F sidebar file-search overlay. Like
/// [`FuzzyPickerState`] but covers both files and directories, and adds a
/// typo-tolerant fallback pass ([`crate::search::fuzzy_typo`]) over the
/// entries nucleo rejects, so misspelled queries still surface results.
pub struct SidebarSearchState {
    pub query: String,
    /// All files and directories in the project directory (populated once on
    /// open), as `(path, is_dir)`.
    pub all_entries: Vec<(PathBuf, bool)>,
    /// Scored and sorted hits: `(score, index into all_entries)`. Exact
    /// (nucleo) matches always precede typo-fallback matches.
    pub filtered: Vec<(u32, usize)>,
    /// Currently highlighted row (0-based within `filtered`).
    pub selected: usize,
}

const SIDEBAR_SEARCH_CAP: usize = 200;

impl SidebarSearchState {
    /// Build by walking the current directory with `ignore` (respects
    /// .gitignore), collecting files **and** directories.
    pub fn new(hide_git: bool, hide_dot: bool) -> Self {
        let mut all_entries = Vec::new();
        let mut builder = ignore::WalkBuilder::new(".");
        builder.hidden(false).git_ignore(true);
        crate::search::apply_hidden_dir_filters(&mut builder, hide_git, hide_dot);
        for entry in builder.build().flatten() {
            if entry.depth() == 0 {
                // Skip the workspace root itself; the sidebar always shows it.
                continue;
            }
            let Some(ft) = entry.file_type() else {
                continue;
            };
            let is_dir = ft.is_dir();
            if !is_dir && !ft.is_file() {
                continue;
            }
            let p = entry.into_path();
            let p = p.strip_prefix("./").map(PathBuf::from).unwrap_or(p);
            all_entries.push((p, is_dir));
        }
        all_entries.sort();
        Self {
            query: String::new(),
            all_entries,
            filtered: Self::unfiltered(&[]),
            selected: 0,
        }
    }

    /// First `CAP` entry indices with score 0 (shown for an empty query).
    fn unfiltered(entries: &[(PathBuf, bool)]) -> Vec<(u32, usize)> {
        let n = entries.len().min(SIDEBAR_SEARCH_CAP);
        (0..n).map(|i| (0u32, i)).collect()
    }

    /// Build from an explicit entry list (tests / future callers).
    #[cfg(test)]
    pub fn from_entries(entries: Vec<(PathBuf, bool)>) -> Self {
        let filtered = Self::unfiltered(&entries);
        Self {
            query: String::new(),
            all_entries: entries,
            filtered,
            selected: 0,
        }
    }

    /// Re-score the entry list against the current query.
    ///
    /// Pass 1 scores every path with nucleo (fzf-style subsequence matching),
    /// pass 2 runs a typo-tolerant matcher over the entries pass 1 rejected.
    /// Exact matches always rank above typo matches; within a pass, higher
    /// score wins.
    pub fn update_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;

        if self.query.is_empty() {
            self.filtered = Self::unfiltered(&self.all_entries);
            return;
        }

        use nucleo::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo::{Config, Matcher, Utf32String};

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);

        let mut matched = vec![false; self.all_entries.len()];
        let mut exact: Vec<(u32, usize)> = Vec::new();
        for (idx, (path, _)) in self.all_entries.iter().enumerate() {
            let s = path.to_string_lossy();
            let haystack = Utf32String::from(s.as_ref());
            if let Some(score) = pattern.score(haystack.slice(..), &mut matcher) {
                matched[idx] = true;
                exact.push((score, idx));
            }
        }
        exact.sort_by_key(|(score, idx)| (std::cmp::Reverse(*score), *idx));

        let mut typo: Vec<(u32, usize)> = self
            .all_entries
            .iter()
            .enumerate()
            .filter(|(idx, _)| !matched[*idx])
            .filter_map(|(idx, (path, _))| {
                let s = path.to_string_lossy();
                // Score the full path and the basename, take the better.
                let mut score = crate::search::fuzzy_typo::score_with_typos(&self.query, &s);
                if let Some(base) = path.file_name().and_then(|n| n.to_str())
                    && let Some(bs) = crate::search::fuzzy_typo::score_with_typos(&self.query, base)
                {
                    score = Some(score.map_or(bs, |ps| ps.max(bs)));
                }
                score.map(|sc| (sc, idx))
            })
            .collect();
        typo.sort_by_key(|(score, idx)| (std::cmp::Reverse(*score), *idx));

        exact.append(&mut typo);
        exact.truncate(SIDEBAR_SEARCH_CAP);
        self.filtered = exact;
    }

    /// The currently selected `(path, is_dir)` entry, if any.
    pub fn selected_entry(&self) -> Option<&(PathBuf, bool)> {
        self.filtered
            .get(self.selected)
            .map(|(_, idx)| &self.all_entries[*idx])
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

#[cfg(test)]
mod sidebar_search_tests {
    use super::*;

    fn entries() -> Vec<(PathBuf, bool)> {
        vec![
            (PathBuf::from("src"), true),
            (PathBuf::from("src/hello.rs"), false),
            (PathBuf::from("src/main.rs"), false),
            (PathBuf::from("tests"), true),
            (PathBuf::from("README.md"), false),
        ]
    }

    #[test]
    fn empty_query_lists_entries() {
        let mut st = SidebarSearchState::from_entries(entries());
        st.update_query(String::new());
        assert_eq!(st.filtered.len(), entries().len());
    }

    #[test]
    fn directories_are_included_in_results() {
        let mut st = SidebarSearchState::from_entries(entries());
        st.update_query("tests".to_string());
        let hit = st
            .filtered
            .iter()
            .map(|(_, idx)| &st.all_entries[*idx])
            .find(|(p, _)| p.to_string_lossy() == "tests")
            .expect("directory 'tests' should match");
        assert!(hit.1, "should be flagged as a directory");
    }

    #[test]
    fn exact_matches_rank_above_typo_matches() {
        let mut st = SidebarSearchState::from_entries(entries());
        // "helo" is a subsequence of "src/hello.rs"? No: h-e-l-o is a
        // subsequence of "hello". It should also typo-match "main.rs"? No —
        // but it exact(ly subsequence)-matches "src/hello.rs", which must be
        // ranked first.
        st.update_query("helo".to_string());
        let (top_path, _) = st.selected_entry().expect("at least one result");
        assert_eq!(top_path, &PathBuf::from("src/hello.rs"));
    }

    #[test]
    fn typo_fallback_surfaces_misspelling() {
        let mut st = SidebarSearchState::from_entries(entries());
        // "helo.rs" has a wrong omission… use a substitution instead:
        // "jello.rs" (h→j) is not a subsequence of "src/hello.rs" with all
        // chars matched — nucleo rejects it; the typo pass must catch it.
        st.update_query("jello".to_string());
        let found = st
            .filtered
            .iter()
            .any(|(_, idx)| st.all_entries[*idx].0 == *"src/hello.rs");
        assert!(found, "typo'd 'jello' should still find hello.rs");
    }

    #[test]
    fn no_results_for_garbage() {
        let mut st = SidebarSearchState::from_entries(entries());
        st.update_query("zzzz".to_string());
        assert!(st.filtered.is_empty());
    }

    #[test]
    fn selection_moves_within_bounds() {
        let mut st = SidebarSearchState::from_entries(entries());
        st.update_query(String::new());
        st.move_down();
        assert_eq!(st.selected, 1);
        st.move_up();
        st.move_up(); // clamps at 0
        assert_eq!(st.selected, 0);
    }
}
