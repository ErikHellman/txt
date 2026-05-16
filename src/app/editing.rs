use super::AppState;

impl AppState {
    /// Run `command` (via `sh -c`) with the current selection on stdin and
    /// replace the selection with the captured stdout. Single undo entry.
    pub(super) fn apply_shell_filter(&mut self, command: &str) {
        if self.config.disable_shell_filter {
            self.status_error = Some("Shell filter disabled by config".to_string());
            return;
        }
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return;
        }
        let buf = &self.editor.active().buffer;
        let primary = buf.cursors.primary();
        let range = match primary.selection {
            Some(s) => s.as_byte_range(),
            None => return,
        };
        if range.is_empty() {
            return;
        }
        let selection_text = {
            let rope = buf.rope();
            let cs = rope.byte_to_char(range.start);
            let ce = rope.byte_to_char(range.end);
            rope.slice(cs..ce).to_string()
        };

        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = match Command::new("sh")
            .arg("-c")
            .arg(trimmed)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                self.status_error = Some(format!("filter spawn failed: {e}"));
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(selection_text.as_bytes());
            // Drop stdin to signal EOF to the child.
        }
        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                self.status_error = Some(format!("filter wait failed: {e}"));
                return;
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed_err = stderr.trim();
            self.status_error = Some(if trimmed_err.is_empty() {
                format!("filter exited {}", output.status)
            } else {
                format!("filter: {trimmed_err}")
            });
            return;
        }
        let new_text = String::from_utf8_lossy(&output.stdout).to_string();
        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        buf.move_cursor_to(range.start, false);
        buf.move_cursor_to(range.end, true);
        buf.insert_str(&new_text);
        buf.commit_batch();
    }
    /// Short status-bar label showing the active indent style, e.g.
    /// `"spaces:4"` or `"tabs:8"`.
    pub fn indent_label(&self) -> String {
        let (indent, _) = self.indent_for_active();
        match indent.style {
            crate::formatting::IndentStyle::Tabs => format!("tabs:{}", indent.width),
            crate::formatting::IndentStyle::Spaces => format!("spaces:{}", indent.width),
        }
    }
    /// Resolve the live indent rules for the active buffer's language,
    /// merging project + global config and falling back to the legacy
    /// `tab_size` and built-in defaults.
    ///
    /// Per-buffer `.editorconfig` overrides win over every config layer.
    pub(super) fn indent_for_active(
        &self,
    ) -> (
        crate::formatting::IndentConfig,
        crate::formatting::IndentRules,
    ) {
        let active = self.editor.active();
        let lang = active.syntax.language;
        let resolver = crate::formatting::FormattingResolver {
            global: &self.config.formatting,
            project: self.project_fmt.as_ref(),
            legacy_tab_size: self.config.tab_size,
        };
        let mut indent = resolver.indent(lang);
        if let Some(style) = active.editorconfig.indent_style {
            indent.style = style;
        }
        if let Some(width) = active.editorconfig.effective_width()
            && width > 0
        {
            indent.width = width;
        }
        (indent, crate::formatting::IndentRules::for_lang(lang))
    }
    /// Run the configured external formatter for the active buffer's
    /// language and replace the buffer atomically. On any error, the buffer
    /// is left untouched and `status_error` is set.
    pub(super) fn format_buffer(&mut self) {
        let lang = self.editor.active().syntax.language;
        let path = self.editor.active().path.clone();
        let resolver = crate::formatting::FormattingResolver {
            global: &self.config.formatting,
            project: self.project_fmt.as_ref(),
            legacy_tab_size: self.config.tab_size,
        };
        let fc = match resolver.formatter(lang) {
            Some(f) => f,
            None => {
                let name = lang.name();
                let display = if name.is_empty() { "this file" } else { name };
                self.status_error = Some(format!("No formatter configured for {display}"));
                return;
            }
        };

        let input = self.editor.active().buffer.to_string();
        let (saved_line, saved_col) = {
            let c = self.editor.active().buffer.cursors.primary();
            (c.line, c.col)
        };

        match crate::formatting::run_formatter(&fc, &input, path.as_deref()) {
            Ok(out) if out == input => {
                // No-op format — leave the buffer alone, no undo entry.
            }
            Ok(out) => {
                let buf = &mut self.editor.active_mut().buffer;
                buf.begin_batch();
                let len = buf.rope().len_bytes();
                buf.delete_range(0, len);
                buf.move_cursor_to(0, false);
                buf.insert_str(&out);
                buf.commit_batch();
                // Restore cursor by clamped (line, col).
                let new_cursor =
                    crate::buffer::cursor::Cursor::from_line_col(buf.rope(), saved_line, saved_col);
                buf.move_cursor_to(new_cursor.byte_offset, false);
                self.editor.active_mut().reparse();
            }
            Err(e) => {
                self.status_error = Some(format!("Formatter failed: {e}"));
            }
        }
    }
    pub(super) fn toggle_line_comment(&mut self) {
        let prefix = match self.editor.active().syntax.comment_prefix() {
            Some(p) => p,
            None => return, // language has no line comment syntax
        };
        let cursor_line = self.editor.active().buffer.cursors.primary().line;
        let line_str = self.editor.active().buffer.line_str(cursor_line);
        let trimmed = line_str.trim_start();
        let leading_spaces = line_str.len() - trimmed.len();
        let already_commented = trimmed.starts_with(prefix);

        let buf = &mut self.editor.active_mut().buffer;
        let line_start = buf
            .rope()
            .char_to_byte(buf.rope().line_to_char(cursor_line));

        buf.begin_batch();
        if already_commented {
            // Remove the comment prefix.
            let comment_start = line_start + leading_spaces;
            let comment_end = comment_start + prefix.len();
            buf.delete_range(comment_start, comment_end);
        } else {
            // Insert the comment prefix at the start of the line.
            buf.move_cursor_to(line_start, false);
            buf.insert_str(prefix);
        }
        buf.commit_batch();
    }
    /// Try to handle `c` as a bracket-pair insertion or skip-on-close.
    /// Returns `true` when this method consumed the keystroke; the caller
    /// must skip the normal insert path. Caller already ensured the buffer
    /// is single-cursor and `config.auto_pair` is on.
    pub(super) fn try_auto_pair(&mut self, c: char) -> bool {
        // Pair table — opener → closer. Symmetric pairs (`"` etc.) close
        // with the same character.
        let pair_for = |ch: char| -> Option<char> {
            match ch {
                '(' => Some(')'),
                '[' => Some(']'),
                '{' => Some('}'),
                '"' => Some('"'),
                '\'' => Some('\''),
                '`' => Some('`'),
                _ => None,
            }
        };
        let close_for = |ch: char| -> bool { matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`') };

        let buf = &mut self.editor.active_mut().buffer;
        let cursor = buf.cursors.primary();
        if cursor.has_selection() {
            return false;
        }
        let at = cursor.byte_offset;
        let rope = buf.rope();
        let next_char = if at < rope.len_bytes() {
            let ch_idx = rope.byte_to_char(at);
            rope.chars_at(ch_idx).next()
        } else {
            None
        };
        let prev_char = if at > 0 {
            let ch_idx = rope.byte_to_char(at);
            // Walk one char back.
            let mut iter = rope.chars_at(ch_idx);
            iter.prev()
        } else {
            None
        };

        // Skip-on-close: typing `)` over an existing `)` just advances.
        if close_for(c) && next_char == Some(c) {
            let new_off = at + c.len_utf8();
            buf.move_cursor_to(new_off, false);
            return true;
        }

        // Auto-open: insert `(` `)` and step back. Symmetric pairs need extra
        // care to avoid pairing a closing quote with itself when the cursor
        // is right after a word character (e.g. `let's` → don't pair the
        // apostrophe).
        if let Some(close) = pair_for(c) {
            let is_symmetric = c == close;
            if is_symmetric {
                // Don't auto-pair quotes when adjacent to a word char on
                // either side (prevents the `let's` case and contractions).
                let is_word = |ch: Option<char>| match ch {
                    Some(c) => c.is_alphanumeric() || c == '_',
                    None => false,
                };
                if is_word(prev_char) || is_word(next_char) {
                    return false;
                }
            }
            let mut pair = String::with_capacity(c.len_utf8() + close.len_utf8());
            pair.push(c);
            pair.push(close);
            buf.insert_str(&pair);
            // Step the cursor back one char so the user types inside the pair.
            let after = buf.cursors.primary().byte_offset;
            let target = after.saturating_sub(close.len_utf8());
            buf.move_cursor_to(target, false);
            return true;
        }
        false
    }
    /// Wrap the active selection (or word at the cursor) in the delimiter
    /// pair chosen by `ch`. Symmetric punctuation (e.g. `"`, `'`) uses the
    /// same character on both sides.
    pub(super) fn apply_surround(&mut self, ch: char) {
        let pair: Option<(String, String)> = match ch {
            '(' | ')' => Some(("(".into(), ")".into())),
            '[' | ']' => Some(("[".into(), "]".into())),
            '{' | '}' => Some(("{".into(), "}".into())),
            '<' | '>' => Some(("<".into(), ">".into())),
            '"' | '\'' | '`' => Some((ch.to_string(), ch.to_string())),
            c if c.is_ascii_punctuation() => Some((c.to_string(), c.to_string())),
            _ => None,
        };
        let Some((open, close)) = pair else {
            self.status_error = Some(format!("No surround pair for '{ch}'"));
            return;
        };
        let ok = self
            .editor
            .active_mut()
            .buffer
            .surround_selection(&open, &close);
        if !ok {
            self.status_error = Some("No selection or word to surround".into());
        }
    }
    pub(super) fn selected_text(&self) -> Option<String> {
        let cursor = self.editor.active().buffer.cursors.primary();
        if !cursor.has_selection() {
            return None;
        }
        let range = cursor.selection_bytes();
        let start = self.editor.active().buffer.rope().byte_to_char(range.start);
        let end = self.editor.active().buffer.rope().byte_to_char(range.end);
        Some(
            self.editor
                .active()
                .buffer
                .rope()
                .slice(start..end)
                .to_string(),
        )
    }
}
