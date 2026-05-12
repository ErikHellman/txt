//! Active snippet session — tracks tab-stop byte ranges as the user edits.
//!
//! A session is installed on a buffer right after a snippet body is
//! inserted. The session owns one [`StopRange`] per index `$1`, `$2`, … plus
//! an optional `$0`. Each range is rebased on every buffer edit so that
//! advancing to the next stop always lands on the user-typed content.

use crate::buffer::cursor::ByteRange;
use crate::buffer::history::EditCommand;
use crate::snippet::{ParsedBody, Segment};

/// One tab-stop entry within an active session.
#[derive(Debug, Clone)]
pub struct StopRange {
    pub index: u32,
    pub range: ByteRange,
}

/// Active snippet session installed on a buffer after expansion.
#[derive(Debug)]
pub struct SnippetSession {
    /// Every tab stop sorted by `index`. The `$0` stop, if present, is the
    /// last entry.
    stops: Vec<StopRange>,
    /// Index into `stops` of the currently selected stop.
    current: usize,
}

/// Output of [`expand_into_buffer`]: the rendered snippet text and a session
/// ready to install on the buffer.
pub struct Expansion {
    pub text: String,
    pub session: Option<SnippetSession>,
}

impl SnippetSession {
    /// Translate a parsed snippet body and the byte offset at which it will
    /// be inserted into a string to insert and the session that tracks the
    /// tab stops within it.
    ///
    /// Returns `Expansion` with `session = None` when the body contains no
    /// tab stops at all (a plain literal snippet).
    pub fn expand_at(body: &ParsedBody, insert_at: usize) -> Expansion {
        let mut text = String::new();
        let mut stops: Vec<StopRange> = Vec::new();
        let mut current_offset = insert_at;
        for segment in &body.segments {
            match segment {
                Segment::Literal(s) => {
                    text.push_str(s);
                    current_offset += s.len();
                }
                Segment::Stop { index, default } => {
                    let start = current_offset;
                    text.push_str(default);
                    current_offset += default.len();
                    let end = current_offset;
                    stops.push(StopRange {
                        index: *index,
                        range: ByteRange::new(start, end),
                    });
                }
            }
        }
        if stops.is_empty() {
            return Expansion {
                text,
                session: None,
            };
        }
        // Sort so $1, $2, … come before $0 (the final-stop convention).
        stops.sort_by_key(|s| if s.index == 0 { u32::MAX } else { s.index });
        let session = SnippetSession { stops, current: 0 };
        Expansion {
            text,
            session: Some(session),
        }
    }

    /// Byte range of the current tab stop.
    pub fn current_range(&self) -> ByteRange {
        self.stops[self.current].range
    }

    /// Whether the current stop is `$0` — the conventional final cursor.
    #[allow(dead_code)]
    pub fn current_is_final(&self) -> bool {
        self.stops[self.current].index == 0
    }

    /// Advance to the next tab stop. Returns `true` if a next stop existed.
    pub fn next_stop(&mut self) -> bool {
        if self.current + 1 < self.stops.len() {
            self.current += 1;
            true
        } else {
            false
        }
    }

    /// Move to the previous tab stop.
    pub fn prev_stop(&mut self) -> bool {
        if self.current > 0 {
            self.current -= 1;
            true
        } else {
            false
        }
    }

    /// Apply an [`EditCommand`] to every tab-stop range so the session keeps
    /// tracking the user's typing within stops. Stops fully consumed by a
    /// deletion are dropped; the session is considered "done" when no stops
    /// remain.
    pub fn rebase(&mut self, cmd: &EditCommand) {
        let mut drop = Vec::new();
        for (i, stop) in self.stops.iter_mut().enumerate() {
            match rebase_range(stop.range, cmd) {
                Some(r) => stop.range = r,
                None => drop.push(i),
            }
        }
        for i in drop.into_iter().rev() {
            self.stops.remove(i);
            if self.current >= i && self.current > 0 {
                self.current -= 1;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// Test-only inspection helpers.
    #[allow(dead_code)]
    pub fn current_index(&self) -> u32 {
        self.stops[self.current].index
    }
    #[allow(dead_code)]
    pub fn stop_count(&self) -> usize {
        self.stops.len()
    }
}

/// Rebase a byte range against a single edit command.
fn rebase_range(range: ByteRange, cmd: &EditCommand) -> Option<ByteRange> {
    match cmd {
        EditCommand::Insert { at, text } => {
            let added = text.len();
            // Inside the range: extend it.
            if *at >= range.start && *at <= range.end {
                return Some(ByteRange::new(range.start, range.end + added));
            }
            // Before the range: shift both ends.
            if *at < range.start {
                return Some(ByteRange::new(range.start + added, range.end + added));
            }
            // After the range: untouched.
            Some(range)
        }
        EditCommand::Delete { start, end, .. } => {
            let removed = end - start;
            if *end <= range.start {
                return Some(ByteRange::new(range.start - removed, range.end - removed));
            }
            if *start >= range.end {
                return Some(range);
            }
            // Overlap — clip the range.
            let new_start = (*start).min(range.start);
            let inside_left = range.start.saturating_sub(*start);
            let inside_right = range.end.min(*end);
            let outside_right_len = range.end - inside_right;
            let new_end = new_start + outside_right_len + inside_left;
            if new_end == new_start {
                return None;
            }
            Some(ByteRange::new(new_start, new_end))
        }
        EditCommand::Replace {
            start,
            end,
            new_text,
            ..
        } => {
            // Treat as delete + insert at the same location.
            let removed = end - start;
            let added = new_text.len();
            if *end <= range.start {
                return Some(ByteRange::new(
                    range.start + added - removed,
                    range.end + added - removed,
                ));
            }
            if *start >= range.end {
                return Some(range);
            }
            // Overlap — collapse to the replacement span.
            Some(ByteRange::new(*start, start + added))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippet::parse_body;

    #[test]
    fn expand_at_collects_stops_in_order() {
        let body = parse_body("for $1 in $2 { $0 }");
        let exp = SnippetSession::expand_at(&body, 100);
        let session = exp.session.expect("has stops");
        assert_eq!(session.stop_count(), 3);
        assert_eq!(session.current_index(), 1);
        assert_eq!(exp.text, "for  in  {  }");
    }

    #[test]
    fn expand_with_defaults_includes_default_text() {
        let body = parse_body("for ${1:i} in ${2:xs} {}");
        let exp = SnippetSession::expand_at(&body, 0);
        assert_eq!(exp.text, "for i in xs {}");
        let session = exp.session.unwrap();
        // First stop covers "i" — 4..5
        let r1 = session.current_range();
        assert_eq!(r1.start, 4);
        assert_eq!(r1.end, 5);
    }

    #[test]
    fn next_and_prev_walk_stops() {
        let body = parse_body("$1 $2 $0");
        let mut session = SnippetSession::expand_at(&body, 0).session.unwrap();
        assert_eq!(session.current_index(), 1);
        assert!(session.next_stop());
        assert_eq!(session.current_index(), 2);
        assert!(session.next_stop());
        assert!(session.current_is_final());
        assert!(!session.next_stop());
        session.prev_stop();
        assert_eq!(session.current_index(), 2);
    }

    #[test]
    fn rebase_after_insert_inside_stop_grows_range() {
        let body = parse_body("${1:i}");
        let mut session = SnippetSession::expand_at(&body, 10).session.unwrap();
        // Stop covers 10..11 ("i").
        session.rebase(&EditCommand::Insert {
            at: 11,
            text: "tem".into(),
        });
        let r = session.current_range();
        assert_eq!(r.start, 10);
        assert_eq!(r.end, 14);
    }

    #[test]
    fn rebase_after_insert_before_shifts_range() {
        let body = parse_body("${1:i}");
        let mut session = SnippetSession::expand_at(&body, 10).session.unwrap();
        session.rebase(&EditCommand::Insert {
            at: 5,
            text: "abc".into(),
        });
        let r = session.current_range();
        assert_eq!(r.start, 13);
        assert_eq!(r.end, 14);
    }
}
