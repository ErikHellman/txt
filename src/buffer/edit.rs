//! Low-level rope editing operations. These functions mutate the rope directly
//! and return the information needed to record an undo command.
//!
//! All byte offsets must be valid char boundaries in the rope.

use ropey::Rope;

/// Insert a string at a byte offset.
///
/// Returns the number of bytes inserted (== `text.len()`).
pub fn insert(rope: &mut Rope, byte_offset: usize, text: &str) -> usize {
    debug_assert!(byte_offset <= rope.len_bytes(), "byte_offset out of bounds");
    let char_offset = rope.byte_to_char(byte_offset);
    rope.insert(char_offset, text);
    text.len()
}

/// Delete bytes in `[start, end)`.
///
/// Returns the deleted text (needed to record the undo command).
pub fn delete(rope: &mut Rope, start: usize, end: usize) -> String {
    debug_assert!(start <= end, "start > end");
    debug_assert!(end <= rope.len_bytes(), "end out of bounds");
    let start_char = rope.byte_to_char(start);
    let end_char = rope.byte_to_char(end);
    let deleted: String = rope.slice(start_char..end_char).chars().collect();
    rope.remove(start_char..end_char);
    deleted
}

/// Replace bytes in `[start, end)` with `new_text`.
///
/// Returns the old text that was replaced.
pub fn replace(rope: &mut Rope, start: usize, end: usize, new_text: &str) -> String {
    let old = delete(rope, start, end);
    insert(rope, start, new_text);
    old
}

/// Returns the byte offset of the previous grapheme cluster boundary before
/// `byte_offset`. Returns 0 if already at the start.
pub fn prev_grapheme_boundary(rope: &Rope, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 0;
    }
    // Convert to char offset, step back one char, convert back to byte.
    // For correct grapheme handling we'd need to collect the line into a string;
    // for now we step back one Unicode scalar value (sufficient for most cases).
    let char_offset = rope.byte_to_char(byte_offset);
    let prev_char = char_offset.saturating_sub(1);
    rope.char_to_byte(prev_char)
}

/// Returns the byte offset of the next grapheme cluster boundary after
/// `byte_offset`. Returns `rope.len_bytes()` if already at the end.
pub fn next_grapheme_boundary(rope: &Rope, byte_offset: usize) -> usize {
    if byte_offset >= rope.len_bytes() {
        return rope.len_bytes();
    }
    let char_offset = rope.byte_to_char(byte_offset);
    let next_char = (char_offset + 1).min(rope.len_chars());
    rope.char_to_byte(next_char)
}

/// Find the byte offset of the start of the previous word from `byte_offset`.
pub fn prev_word_boundary(rope: &Rope, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 0;
    }
    let char_offset = rope.byte_to_char(byte_offset);
    // Collect chars before the cursor into a small string to find word boundary.
    let prefix: String = rope.chars_at(0).take(char_offset).collect();
    let mut idx = prefix.len();
    // Skip any trailing whitespace
    while idx > 0 && prefix[..idx].ends_with(|c: char| c.is_whitespace()) {
        idx -= prefix[..idx]
            .chars()
            .next_back()
            .map_or(1, |c| c.len_utf8());
    }
    // Skip word chars
    while idx > 0 && prefix[..idx].ends_with(|c: char| !c.is_whitespace()) {
        idx -= prefix[..idx]
            .chars()
            .next_back()
            .map_or(1, |c| c.len_utf8());
    }
    idx
}

/// Find the byte offset of the start of the next word from `byte_offset`.
pub fn next_word_boundary(rope: &Rope, byte_offset: usize) -> usize {
    let len = rope.len_bytes();
    if byte_offset >= len {
        return len;
    }
    let char_offset = rope.byte_to_char(byte_offset);
    let suffix: String = rope.chars_at(char_offset).collect();
    let mut idx = 0;
    // Skip non-whitespace (current word)
    while idx < suffix.len() && !suffix[idx..].starts_with(|c: char| c.is_whitespace()) {
        idx += suffix[idx..].chars().next().map_or(1, |c| c.len_utf8());
    }
    // Skip whitespace
    while idx < suffix.len() && suffix[idx..].starts_with(|c: char| c.is_whitespace()) {
        idx += suffix[idx..].chars().next().map_or(1, |c| c.len_utf8());
    }
    byte_offset + idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    #[test]
    fn insert_at_start() {
        let mut r = rope("world");
        insert(&mut r, 0, "hello ");
        assert_eq!(r.to_string(), "hello world");
    }

    #[test]
    fn insert_at_end() {
        let mut r = rope("hello");
        insert(&mut r, 5, " world");
        assert_eq!(r.to_string(), "hello world");
    }

    #[test]
    fn insert_mid() {
        let mut r = rope("helo");
        insert(&mut r, 3, "l");
        assert_eq!(r.to_string(), "hello");
    }

    #[test]
    fn insert_unicode() {
        let mut r = rope("hi");
        insert(&mut r, 2, " 😀");
        assert_eq!(r.to_string(), "hi 😀");
    }

    #[test]
    fn delete_range() {
        let mut r = rope("hello world");
        let deleted = delete(&mut r, 5, 11);
        assert_eq!(r.to_string(), "hello");
        assert_eq!(deleted, " world");
    }

    #[test]
    fn delete_unicode() {
        // "😀" is 4 bytes
        let mut r = rope("hi 😀 there");
        let deleted = delete(&mut r, 3, 7); // delete the emoji
        assert_eq!(deleted, "😀");
        assert_eq!(r.to_string(), "hi  there");
    }

    #[test]
    fn replace_text() {
        let mut r = rope("hello world");
        let old = replace(&mut r, 6, 11, "Rust");
        assert_eq!(old, "world");
        assert_eq!(r.to_string(), "hello Rust");
    }

    #[test]
    fn prev_grapheme_ascii() {
        let r = rope("hello");
        assert_eq!(prev_grapheme_boundary(&r, 3), 2);
        assert_eq!(prev_grapheme_boundary(&r, 0), 0);
    }

    #[test]
    fn next_grapheme_ascii() {
        let r = rope("hello");
        assert_eq!(next_grapheme_boundary(&r, 2), 3);
        assert_eq!(next_grapheme_boundary(&r, 5), 5);
    }

    #[test]
    fn prev_grapheme_emoji() {
        // "😀" is 4 bytes; after it, prev should go back 4 bytes (one char)
        let r = rope("a😀b");
        // byte layout: 'a'=0, '😀'=1..4, 'b'=5
        assert_eq!(prev_grapheme_boundary(&r, 5), 1);
    }

    #[test]
    fn word_boundary_forward() {
        let r = rope("hello world foo");
        assert_eq!(next_word_boundary(&r, 0), 6); // after "hello "
        assert_eq!(next_word_boundary(&r, 6), 12); // after "world "
    }

    #[test]
    fn word_boundary_backward() {
        let r = rope("hello world foo");
        assert_eq!(prev_word_boundary(&r, 11), 6); // back to "world"
        assert_eq!(prev_word_boundary(&r, 6), 0); // back to "hello"
    }

    #[test]
    fn large_file_insert_performance() {
        // Create a large rope (~10 MB of text) and verify insert is fast.
        // This is a correctness check; the actual timing assertion lives in benches/.
        let line = "a".repeat(99) + "\n"; // 100 bytes per line
        let content = line.repeat(100_000); // 10 MB
        let mut r = Rope::from_str(&content);
        let mid = r.len_bytes() / 2;
        insert(&mut r, mid, "INSERTED");
        assert!(r.to_string().contains("INSERTED"));
    }
}
