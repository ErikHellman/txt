use super::AppState;

impl AppState {
    /// Replay the actions stored in macro slot `slot`. Wraps the playback in
    /// a single undo batch so the entire sequence collapses to one undo step,
    /// and sets the replay flag so the actions are not re-recorded if the
    /// user starts a new recording mid-replay.
    pub(super) fn replay_macro_slot(&mut self, slot: char) {
        let Some(actions) = self.macros.play(slot) else {
            self.status_error = Some(format!("No macro in slot '{slot}'"));
            return;
        };
        let term_h = self.term_height;
        self.macros.set_replaying(true);
        self.editor.active_mut().buffer.begin_batch();
        for action in actions {
            self.update(action, term_h);
        }
        self.editor.active_mut().buffer.commit_batch();
        self.macros.set_replaying(false);
    }
    /// Same logic as `expand_snippet_at_cursor` but silent: returns `false`
    /// when no snippet matches so the caller can fall through to its own
    /// default (e.g. `InsertTab` doing indentation).
    pub(super) fn try_expand_snippet_silently(&mut self) -> bool {
        let lang_id = self.editor.active().syntax.language.config_key();
        if lang_id.is_empty() {
            return false;
        }
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        let primary = self.editor.active().buffer.cursors.primary();
        if primary.has_selection() {
            return false;
        }
        let rope = self.editor.active().buffer.rope();
        let probe = cursor_byte.saturating_sub(1);
        let Some((wstart, wend)) = crate::buffer::cursor::word_span_at(rope, probe) else {
            return false;
        };
        if wend < cursor_byte {
            return false;
        }
        let prefix: String = rope.byte_slice(wstart..wend).chars().collect();
        let matches = self.snippets.lookup(lang_id, &prefix);
        let Some(snip) = matches.into_iter().next() else {
            return false;
        };
        let parsed = snip.parse_body();
        let exp = crate::snippet::session::SnippetSession::expand_at(&parsed, wstart);
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.delete_range(wstart, wend);
        *buf.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(buf.rope(), wstart);
        buf.cursors.primary_mut().selection = None;
        buf.insert_str(&exp.text);
        buf.commit_batch();
        self.editor.active_mut().snippet_session = exp.session;
        self.snippet_select_current();
        true
    }
    /// Look up the word before the cursor in the active buffer's language
    /// snippet store; if there's a match, delete the word and expand the
    /// snippet in its place.
    pub(super) fn expand_snippet_at_cursor(&mut self) {
        let lang_id = self.editor.active().syntax.language.config_key();
        if lang_id.is_empty() {
            self.status_error = Some("Snippets need a recognised language".into());
            return;
        }
        let cursor_byte = self.editor.active().buffer.cursors.primary().byte_offset;
        let rope = self.editor.active().buffer.rope();
        let probe = cursor_byte.saturating_sub(1);
        let Some((wstart, wend)) = crate::buffer::cursor::word_span_at(rope, probe) else {
            self.status_error = Some("Place the cursor after a snippet prefix".into());
            return;
        };
        if wend < cursor_byte {
            self.status_error = Some("Place the cursor after a snippet prefix".into());
            return;
        }
        let prefix: String = rope.byte_slice(wstart..wend).chars().collect();
        let matches = self.snippets.lookup(lang_id, &prefix);
        let Some(snip) = matches.into_iter().next() else {
            self.status_error = Some(format!("No snippet named '{prefix}'"));
            return;
        };
        let parsed = snip.parse_body();
        // Compute the expansion text and session at the prefix start position
        // since we're about to delete the prefix.
        let exp = crate::snippet::session::SnippetSession::expand_at(&parsed, wstart);
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.delete_range(wstart, wend);
        // Move the primary cursor to wstart before inserting so insert lands
        // at the right location regardless of prior selection state.
        *buf.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(buf.rope(), wstart);
        buf.cursors.primary_mut().selection = None;
        buf.insert_str(&exp.text);
        buf.commit_batch();
        self.editor.active_mut().snippet_session = exp.session;
        // Jump cursor to the first tab stop, selecting its default text.
        self.snippet_select_current();
    }
    /// Select the byte range of the current snippet tab stop. Called after
    /// expanding or advancing the session.
    pub(super) fn snippet_select_current(&mut self) {
        let handle = self.editor.active_mut();
        let range = match &handle.snippet_session {
            Some(s) => s.current_range(),
            None => return,
        };
        let len = handle.buffer.rope().len_bytes();
        let bound_start = range.start.min(len);
        let bound_end = range.end.min(len);
        let new_cursor = {
            let rope = handle.buffer.rope();
            crate::buffer::cursor::Cursor::from_byte_offset(rope, bound_end)
        };
        let primary = handle.buffer.cursors.primary_mut();
        *primary = new_cursor;
        if bound_end > bound_start {
            primary.selection = Some(crate::buffer::cursor::Selection {
                anchor: bound_start,
                active: bound_end,
            });
        } else {
            primary.selection = None;
        }
    }
    /// Move the active snippet session forward (`true`) or backward.
    pub(super) fn snippet_advance(&mut self, forward: bool) {
        let advanced = match self.editor.active_mut().snippet_session.as_mut() {
            Some(s) => {
                if forward {
                    s.next_stop()
                } else {
                    s.prev_stop()
                }
            }
            None => false,
        };
        if advanced {
            self.snippet_select_current();
        } else {
            self.editor.active_mut().snippet_session = None;
        }
    }
    /// Snapshot the active buffer's cursor into the jump list. Used right
    /// before navigation actions (mark jumps, jump-list back) so the
    /// previous cursor position can be returned to.
    pub(super) fn push_current_to_jump_list(&mut self) {
        let handle = self.editor.active();
        if let Some(path) = handle.path.clone() {
            let byte_offset = handle.buffer.cursors.primary().byte_offset;
            self.jumps
                .push(crate::marks::JumpEntry { path, byte_offset });
        }
    }
    /// Move the cursor to a jump-list entry, switching tabs if necessary.
    pub(super) fn go_to_jump_entry(&mut self, entry: &crate::marks::JumpEntry) {
        let active_path = self.editor.active().path.clone();
        if active_path.as_ref() != Some(&entry.path) {
            let _ = self.editor.open_tab(entry.path.clone());
            self.after_file_open_or_save();
        }
        let handle = self.editor.active_mut();
        let rope = handle.buffer.rope();
        let bound = entry.byte_offset.min(rope.len_bytes());
        *handle.buffer.cursors.primary_mut() =
            crate::buffer::cursor::Cursor::from_byte_offset(rope, bound);
        handle.buffer.cursors.collapse_to_primary();
    }
}
