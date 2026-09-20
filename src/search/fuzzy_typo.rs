//! Typo-tolerant fuzzy scoring for the sidebar file search.
//!
//! `nucleo` (used by the pickers) matches the query as a subsequence of the
//! haystack: query characters may be skipped, but never match a *wrong*
//! haystack character. That misses everyday misspellings — transposed,
//! doubled or substituted letters. [`score_with_typos`] is the fallback pass
//! used by the sidebar search: it aligns the query against the haystack
//! allowing each aligned pair to mismatch at a fixed penalty, so omissions
//! (plain subsequence) *and* substitutions are tolerated.
//!
//! Scoring is intentionally kept in a different scale from nucleo's. The
//! caller always ranks exact (nucleo) matches above typo matches, so the two
//! scales never mix.

/// Reward for aligning a query char onto the correct haystack char.
const MATCH_BONUS: i32 = 12;
/// Penalty for aligning a query char onto a wrong haystack char.
const SUBST_PENALTY: i32 = -8;
/// Reward when the previous query char aligned to the directly preceding
/// haystack char (a consecutive run in the haystack).
const CONSECUTIVE_BONUS: i32 = 4;
/// Reward when the aligned char sits at a word start (start of the path or
/// right after a separator like `/`, `_`, `-`, `.`).
const WORD_START_BONUS: i32 = 8;
/// Extra reward when the aligned char lies within the basename (the last
/// path component), so basename hits outrank matches buried in ancestor
/// directories.
const BASENAME_BONUS: i32 = 6;

fn is_sep(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ')
}

/// Score `query` against `haystack`, tolerating substitutions and
/// transpositions.
///
/// The query must align, char for char and in order, onto distinct haystack
/// chars; an aligned pair that mismatches costs [`SUBST_PENALTY`] and earns
/// no bonuses. Skipped haystack chars are free (subsequence semantics), so
/// pure omissions match too. Returns `None` when the best possible alignment
/// is entirely wrong (more penalties than rewards), which effectively bounds
/// the tolerated typo rate.
pub fn score_with_typos(query: &str, haystack: &str) -> Option<u32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let h: Vec<char> = haystack.to_lowercase().chars().collect();
    if q.is_empty() || h.is_empty() || q.len() > h.len() {
        return None;
    }

    // Char index of the first char of the last path component.
    let basename_start = {
        let byte = haystack.rfind(['/', '\\']).map_or(0, |i| i + 1);
        haystack[..byte].chars().count()
    };

    let n = h.len();
    // `dp[j]` = best score matching the query prefix consumed so far, with
    // the last aligned haystack char exactly `h[j]`. `i32::MIN` marks
    // unreachable positions.
    let mut dp: Vec<i32> = vec![i32::MIN; n];
    for (j, hc) in h.iter().enumerate() {
        dp[j] = align(q[0], *hc, j, &h, basename_start);
    }

    for qc in q.iter().skip(1) {
        let mut ndp = vec![i32::MIN; n];
        // Running max of dp over positions strictly before j.
        let mut best_prev = i32::MIN;
        for j in 1..n {
            if dp[j - 1] > best_prev {
                best_prev = dp[j - 1];
            }
            if best_prev == i32::MIN {
                continue;
            }
            let mut s = best_prev + align(*qc, h[j], j, &h, basename_start);
            // Consecutive-run approximation: treat the alignment as
            // consecutive when the running max is achieved at j-1 exactly.
            if dp[j - 1] == best_prev {
                s += CONSECUTIVE_BONUS;
            }
            ndp[j] = s;
        }
        dp = ndp;
    }

    // Too many wrong chars drag the best alignment to zero or below: reject.
    dp.into_iter().filter(|s| *s > 0).max().map(|s| s as u32)
}

/// Reward for aligning `qc` onto `hc` — position bonuses only apply to
/// correct alignments so they never cushion misspellings.
fn align(qc: char, hc: char, j: usize, h: &[char], basename_start: usize) -> i32 {
    if qc == hc {
        MATCH_BONUS + position_bonus(j, h, basename_start)
    } else {
        SUBST_PENALTY
    }
}

fn position_bonus(j: usize, h: &[char], basename_start: usize) -> i32 {
    let mut b = 0;
    if j == 0 || is_sep(h[j - 1]) {
        b += WORD_START_BONUS;
    }
    if j >= basename_start {
        b += BASENAME_BONUS;
    }
    b
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_substring_scores_positive() {
        assert!(score_with_typos("rust", "src/rust.rs").is_some());
        assert!(score_with_typos("rust", "src/rust.rs").unwrap() > 0);
    }

    #[test]
    fn substitution_still_matches() {
        // One wrong letter inside "rust".
        assert!(score_with_typos("rast", "src/rust.rs").is_some());
    }

    #[test]
    fn transposition_still_matches() {
        // "hllo" — swapped/wrong middle letters in "hello".
        assert!(score_with_typos("hllo", "src/hello.rs").is_some());
        // Doubled-to-single misspelling of "hello" reversed: query has an
        // extra char the haystack lacks, but the remaining chars align.
        assert!(score_with_typos("hellp", "src/hello.rs").is_some());
    }

    #[test]
    fn pure_omission_matches_like_subsequence() {
        assert!(score_with_typos("hlo", "src/hello.rs").is_some());
    }

    #[test]
    fn completely_wrong_word_is_rejected() {
        assert_eq!(score_with_typos("zzzz", "src/rust.rs"), None);
    }

    #[test]
    fn empty_query_is_rejected() {
        assert_eq!(score_with_typos("", "src/rust.rs"), None);
    }

    #[test]
    fn empty_haystack_is_rejected() {
        assert_eq!(score_with_typos("rust", ""), None);
    }

    #[test]
    fn query_longer_than_haystack_is_rejected() {
        assert_eq!(score_with_typos("abcdef", "abc"), None);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(score_with_typos("RUST", "src/rust.rs").is_some());
        assert!(score_with_typos("rust", "SRC/RSUT.RS").is_some());
    }

    #[test]
    fn basename_hit_outranks_far_position_hit() {
        let basename = score_with_typos("rs", "src/lib.rs").unwrap();
        // Same chars matching early in the directory part instead.
        let prefix = score_with_typos("rs", "reset/src/lib.xs").unwrap();
        assert!(basename > prefix);
    }

    #[test]
    fn exact_outranks_typo() {
        let exact = score_with_typos("rust", "src/rust.rs").unwrap();
        let typo = score_with_typos("rast", "src/rust.rs").unwrap();
        assert!(exact > typo);
    }

    #[test]
    fn separator_chars_act_as_word_starts() {
        // "m" should match the `m` after `_` (word start) for a high score.
        let s = score_with_typos("m", "src/my_mod.rs").unwrap();
        assert!(s > 0);
    }
}
