use regex::Regex;

use crate::buffer::cursor::ByteRange;

/// State for the find / replace bar.
///
/// Matches are recomputed synchronously on every query change. For Phase 6 this is
/// fast enough; a background worker can be added in a later phase if needed.
pub struct SearchState {
    pub query: String,
    pub replace_text: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    /// All match byte-ranges in the current buffer text, in document order.
    pub matches: Vec<ByteRange>,
    /// 0-based index into `matches` for the highlighted "current" match.
    pub current_match: usize,
    /// True when the replace input row is visible (Ctrl+H).
    pub show_replace: bool,
    /// True when keyboard focus is in the replace field (vs. the query field).
    pub focus_replace: bool,
}

impl SearchState {
    pub fn new(show_replace: bool) -> Self {
        Self {
            query: String::new(),
            replace_text: String::new(),
            is_regex: false,
            case_sensitive: false,
            matches: Vec::new(),
            current_match: 0,
            show_replace,
            focus_replace: false,
        }
    }

    // ── Match computation ────────────────────────────────────────────────────

    /// Recompute all match byte-ranges against `text`.
    pub fn recompute_matches(&mut self, text: &str) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }

        let pattern = build_pattern(&self.query, self.is_regex, self.case_sensitive);
        if let Ok(re) = Regex::new(&pattern) {
            for m in re.find_iter(text) {
                self.matches.push(ByteRange::new(m.start(), m.end()));
            }
        }

        // Keep current_match in bounds.
        if self.matches.is_empty() {
            self.current_match = 0;
        } else if self.current_match >= self.matches.len() {
            self.current_match = self.matches.len() - 1;
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = self
                .current_match
                .checked_sub(1)
                .unwrap_or(self.matches.len() - 1);
        }
    }

    pub fn current_range(&self) -> Option<ByteRange> {
        self.matches.get(self.current_match).copied()
    }

    /// Advance `current_match` to the first match whose end is after `byte_offset`.
    pub fn jump_to_nearest(&mut self, byte_offset: usize) {
        if self.matches.is_empty() {
            return;
        }
        let idx = self
            .matches
            .iter()
            .position(|r| r.end > byte_offset)
            .unwrap_or(0);
        self.current_match = idx;
    }

    // ── Layout ────────────────────────────────────────────────────────────────

    /// Number of terminal rows the search bar occupies.
    pub fn bar_height(&self) -> u16 {
        if self.show_replace { 2 } else { 1 }
    }
}

/// Build a `regex` pattern string from the user's query and search flags.
fn build_pattern(query: &str, is_regex: bool, case_sensitive: bool) -> String {
    let base = if is_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if case_sensitive {
        base
    } else {
        format!("(?i){}", base)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_search_finds_all_occurrences() {
        let mut s = SearchState::new(false);
        s.query = "he".to_string();
        s.recompute_matches("hello he");
        assert_eq!(s.matches.len(), 2);
        assert_eq!(s.matches[0], ByteRange::new(0, 2));
        assert_eq!(s.matches[1], ByteRange::new(6, 8));
    }

    #[test]
    fn plain_search_case_insensitive() {
        let mut s = SearchState::new(false);
        s.query = "HELLO".to_string();
        s.case_sensitive = false;
        s.recompute_matches("hello HELLO");
        assert_eq!(s.matches.len(), 2);
    }

    #[test]
    fn plain_search_case_sensitive() {
        let mut s = SearchState::new(false);
        s.query = "Hello".to_string();
        s.case_sensitive = true;
        s.recompute_matches("hello Hello HELLO");
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.matches[0].start, 6);
    }

    #[test]
    fn regex_search() {
        let mut s = SearchState::new(false);
        s.query = r"\d+".to_string();
        s.is_regex = true;
        s.recompute_matches("abc 123 def 456");
        assert_eq!(s.matches.len(), 2);
    }

    #[test]
    fn invalid_regex_produces_no_matches() {
        let mut s = SearchState::new(false);
        s.query = "[invalid".to_string();
        s.is_regex = true;
        s.recompute_matches("test [invalid text");
        assert!(s.matches.is_empty());
    }

    #[test]
    fn empty_query_clears_matches() {
        let mut s = SearchState::new(false);
        s.query = "foo".to_string();
        s.recompute_matches("foo foo");
        assert_eq!(s.matches.len(), 2);
        s.query.clear();
        s.recompute_matches("foo foo");
        assert!(s.matches.is_empty());
    }

    #[test]
    fn next_and_prev_wrap() {
        let mut s = SearchState::new(false);
        s.query = "a".to_string();
        s.recompute_matches("a a a");
        assert_eq!(s.matches.len(), 3);
        s.next_match();
        assert_eq!(s.current_match, 1);
        s.next_match();
        assert_eq!(s.current_match, 2);
        s.next_match();
        assert_eq!(s.current_match, 0); // wraps
        s.prev_match();
        assert_eq!(s.current_match, 2); // wraps back
    }

    #[test]
    fn jump_to_nearest() {
        let mut s = SearchState::new(false);
        s.query = "x".to_string();
        s.recompute_matches("ax bx cx");
        // matches at bytes 1, 4, 7
        s.jump_to_nearest(3);
        assert_eq!(s.current_match, 1); // match at byte 4 is first after offset 3
    }

    #[test]
    fn bar_height() {
        let s1 = SearchState::new(false);
        assert_eq!(s1.bar_height(), 1);
        let s2 = SearchState::new(true);
        assert_eq!(s2.bar_height(), 2);
    }
}
