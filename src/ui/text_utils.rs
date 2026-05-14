//! Shared text-truncation helpers used by overlay and status-bar renderers.
//!
//! Three flavours exist because callers have different budgets:
//!
//! - [`truncate_to_width`] respects grapheme clusters and `unicode-width`
//!   display columns. Use this whenever the budget is in *terminal cells*,
//!   which is almost always the right call for visible UI text.
//! - [`truncate_bytes`] is a cheap byte-budget cut that snaps back to a UTF-8
//!   char boundary. Use only when the budget really is in bytes (e.g. when
//!   serialising into a fixed-width field).
//! - [`truncate_left_keep_right`] preserves the trailing portion (with a
//!   leading `…`). Useful for paths and breadcrumbs where the rightmost
//!   segment carries the most information.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Truncate `s` to at most `max_width` terminal columns, respecting grapheme
/// boundaries. The result is always a valid `&str` slice.
pub fn truncate_to_width(s: &str, max_width: usize) -> &str {
    let mut width = 0usize;
    let mut end = 0usize;
    for (idx, g) in s.grapheme_indices(true) {
        let gw = UnicodeWidthStr::width(g);
        if width + gw > max_width {
            return &s[..end];
        }
        width += gw;
        end = idx + g.len();
    }
    s
}

/// Truncate `s` to at most `max_bytes` bytes, snapping back to the nearest
/// UTF-8 char boundary so the result is always valid UTF-8.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Truncate `s` to fit in `max_chars` columns, keeping the rightmost
/// (innermost) part. When the string already fits, the full string is
/// returned; otherwise an ellipsis `…` is prefixed to the kept suffix.
pub fn truncate_left_keep_right(s: &str, max_chars: usize) -> String {
    let len = s.chars().count();
    if len <= max_chars || max_chars == 0 {
        return s.chars().take(max_chars).collect();
    }
    let want = max_chars.saturating_sub(1);
    let skip = len - want;
    let tail: String = s.chars().skip(skip).collect();
    let mut out = String::with_capacity(tail.len() + 1);
    out.push('…');
    out.push_str(&tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_truncation_handles_wide_chars() {
        // CJK characters are 2 columns wide each.
        assert_eq!(truncate_to_width("日本語", 4), "日本");
        assert_eq!(truncate_to_width("日本語", 5), "日本");
        assert_eq!(truncate_to_width("日本語", 6), "日本語");
        assert_eq!(truncate_to_width("abc", 2), "ab");
        assert_eq!(truncate_to_width("abc", 100), "abc");
    }

    #[test]
    fn byte_truncation_snaps_to_char_boundary() {
        // "é" is two bytes; trimming inside it must back off.
        assert_eq!(truncate_bytes("café", 3), "caf");
        assert_eq!(truncate_bytes("café", 4), "caf");
        assert_eq!(truncate_bytes("café", 5), "café");
        assert_eq!(truncate_bytes("abc", 10), "abc");
    }

    #[test]
    fn left_truncation_prefixes_ellipsis() {
        assert_eq!(truncate_left_keep_right("hello", 10), "hello");
        assert_eq!(truncate_left_keep_right("hello", 3), "…lo");
        assert_eq!(truncate_left_keep_right("hello", 0), "");
    }
}
