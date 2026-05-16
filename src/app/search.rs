use std::path::PathBuf;

use crate::input::action::{Direction, EditorAction};
use crate::search::SearchState;

use super::AppState;

impl AppState {
    /// Handle keyboard input while the project-search overlay is active.
    /// Returns `true` when the action was consumed (do not fall through to the
    /// editor). Global actions like Quit/ToggleHelp fall through.
    pub(super) fn handle_project_search(&mut self, action: EditorAction) -> bool {
        match action {
            EditorAction::InsertChar(c) => {
                if let Some(ps) = &mut self.project_search {
                    if ps.focus_replace {
                        ps.replace_text.push(c);
                    } else {
                        ps.query.push(c);
                    }
                }
                if !self
                    .project_search
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false)
                {
                    self.recompute_project_search();
                }
                true
            }
            EditorAction::DeleteBackward => {
                if let Some(ps) = &mut self.project_search {
                    if ps.focus_replace {
                        ps.replace_text.pop();
                    } else {
                        ps.query.pop();
                    }
                }
                if !self
                    .project_search
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false)
                {
                    self.recompute_project_search();
                }
                true
            }
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(ps) = &mut self.project_search {
                    ps.move_up();
                }
                true
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(ps) = &mut self.project_search {
                    ps.move_down();
                }
                true
            }
            EditorAction::InsertTab => {
                // Tab toggles focus and reveals the replace input.
                if let Some(ps) = &mut self.project_search {
                    ps.show_replace = true;
                    ps.focus_replace = !ps.focus_replace;
                }
                true
            }
            EditorAction::SearchToggleRegex => {
                if let Some(ps) = &mut self.project_search {
                    ps.is_regex = !ps.is_regex;
                }
                self.recompute_project_search();
                true
            }
            EditorAction::SearchToggleCaseSensitive => {
                if let Some(ps) = &mut self.project_search {
                    ps.case_sensitive = !ps.case_sensitive;
                }
                self.recompute_project_search();
                true
            }
            EditorAction::InsertNewline => {
                let target = self
                    .project_search
                    .as_ref()
                    .and_then(|ps| ps.results.matches.get(ps.selected).cloned());
                if let Some(m) = target {
                    let abs = self.workspace.join(&m.path);
                    self.project_search = None;
                    let _ = self.editor.open_tab(abs);
                    self.after_file_open_or_save();
                    // Move cursor to the start of the match line. Computing the
                    // exact column requires the file's full byte→line table,
                    // which we don't cache; landing on the line is the
                    // standard "go to result" behaviour.
                    let buf = &mut self.editor.active_mut().buffer;
                    let rope = buf.rope();
                    let line_idx = m.line.min(rope.len_lines().saturating_sub(1));
                    let line_start_char = rope.line_to_char(line_idx);
                    let target_byte = rope.char_to_byte(line_start_char);
                    buf.move_cursor_to(target_byte, false);
                }
                true
            }
            EditorAction::SearchReplaceAll => {
                self.project_replace_all();
                true
            }
            EditorAction::CloseSearch | EditorAction::Quit | EditorAction::Unhandled => {
                self.project_search = None;
                true
            }
            // Global actions that should still work while the overlay is open
            // (toggle help, scroll cursor, etc.).
            EditorAction::ToggleHelp => false,
            _ => true,
        }
    }
    pub(super) fn recompute_project_search(&mut self) {
        let (query, is_regex, case_sensitive) = match &self.project_search {
            Some(ps) => (ps.query.clone(), ps.is_regex, ps.case_sensitive),
            None => return,
        };
        let results = crate::search::project::run(
            &self.workspace,
            &query,
            is_regex,
            case_sensitive,
            self.config.hide_git_folder,
            self.config.hide_dot_folders,
        );
        if let Some(ps) = &mut self.project_search {
            ps.results = results;
            ps.selected = 0;
        }
    }
    pub(super) fn project_replace_all(&mut self) {
        let (query, replacement, is_regex, case_sensitive) = match &self.project_search {
            Some(ps) if !ps.query.is_empty() && ps.show_replace => (
                ps.query.clone(),
                ps.replace_text.clone(),
                ps.is_regex,
                ps.case_sensitive,
            ),
            _ => return,
        };

        // Collect distinct file paths from results.
        let paths: Vec<PathBuf> = {
            let ps = match &self.project_search {
                Some(p) => p,
                None => return,
            };
            let mut seen = std::collections::HashSet::new();
            ps.results
                .matches
                .iter()
                .filter_map(|m| {
                    let abs = self.workspace.join(&m.path);
                    if seen.insert(abs.clone()) {
                        Some(abs)
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut total_replaced = 0usize;
        for path in &paths {
            // If the file is currently open in a tab, edit it through the
            // buffer so undo history is preserved.
            let open_idx = self
                .editor
                .tabs
                .iter()
                .position(|t| t.path.as_deref() == Some(path.as_path()));
            if let Some(idx) = open_idx {
                let prev_active = self.editor.active_idx;
                self.editor.go_to_tab(idx);
                let buf = &mut self.editor.active_mut().buffer;
                let text = buf.to_string();
                let pattern = crate::search::build_pattern(&query, is_regex, case_sensitive);
                let re = match regex::Regex::new(&pattern) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let ranges: Vec<(usize, usize)> =
                    re.find_iter(&text).map(|m| (m.start(), m.end())).collect();
                if ranges.is_empty() {
                    self.editor.go_to_tab(prev_active);
                    continue;
                }
                buf.begin_batch();
                for (start, end) in ranges.iter().rev() {
                    buf.move_cursor_to(*start, false);
                    buf.move_cursor_to(*end, true);
                    buf.insert_str(&replacement);
                }
                buf.commit_batch();
                total_replaced += ranges.len();
                self.editor.go_to_tab(prev_active);
            } else {
                // Edit on disk.
                if let Ok(n) = crate::search::project::replace_all_in_file(
                    path,
                    &query,
                    is_regex,
                    case_sensitive,
                    &replacement,
                ) {
                    total_replaced += n;
                }
            }
        }

        // Refresh result list and surface a status message.
        self.recompute_project_search();
        self.status_error = Some(format!("Replaced {total_replaced} occurrence(s)"));
    }
    /// Handle keyboard input while the search bar is active.
    /// Returns `true` if the action was consumed (should not be processed further).
    pub(super) fn handle_search_input(&mut self, action: EditorAction) -> bool {
        match &action {
            // Navigation: let these fall through to the normal dispatch.
            EditorAction::MoveCursor(_)
            | EditorAction::MoveCursorWord(_)
            | EditorAction::MoveCursorHome
            | EditorAction::MoveCursorEnd
            | EditorAction::MoveCursorFileStart
            | EditorAction::MoveCursorFileEnd
            | EditorAction::MoveCursorPage(_)
            | EditorAction::Scroll(_)
            | EditorAction::ScrollCursorCenter
            | EditorAction::GoToMatchingBracket
            | EditorAction::MouseClick { .. }
            | EditorAction::MouseDrag { .. } => return false,

            _ => {}
        }

        match action {
            EditorAction::InsertChar(c) => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false);
                if focus_replace {
                    if let Some(ss) = &mut self.search_state {
                        ss.replace_text.push(c);
                    }
                } else {
                    if let Some(ss) = &mut self.search_state {
                        ss.query.push(c);
                    }
                    self.recompute_search_and_jump();
                }
            }
            EditorAction::DeleteBackward => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace)
                    .unwrap_or(false);
                if focus_replace {
                    if let Some(ss) = &mut self.search_state {
                        ss.replace_text.pop();
                    }
                } else {
                    if let Some(ss) = &mut self.search_state {
                        ss.query.pop();
                    }
                    self.recompute_search_and_jump();
                }
            }
            EditorAction::InsertNewline => {
                let focus_replace = self
                    .search_state
                    .as_ref()
                    .map(|s| s.focus_replace && s.show_replace)
                    .unwrap_or(false);
                if focus_replace {
                    self.replace_current();
                } else {
                    self.search_next();
                }
            }
            EditorAction::InsertTab => {
                // Toggle focus between query and replace fields.
                if let Some(ss) = &mut self.search_state
                    && ss.show_replace
                {
                    ss.focus_replace = !ss.focus_replace;
                }
            }
            EditorAction::SearchNext => self.search_next(),
            EditorAction::SearchPrev => self.search_prev(),
            EditorAction::SearchReplaceOne => self.replace_current(),
            EditorAction::SearchReplaceAll => self.replace_all(),
            EditorAction::SearchToggleRegex => {
                if let Some(ss) = &mut self.search_state {
                    ss.is_regex = !ss.is_regex;
                }
                self.recompute_search_and_jump();
            }
            EditorAction::SearchToggleCaseSensitive => {
                if let Some(ss) = &mut self.search_state {
                    ss.case_sensitive = !ss.case_sensitive;
                }
                self.recompute_search_and_jump();
            }
            EditorAction::OpenReplace => {
                if let Some(ss) = &mut self.search_state {
                    ss.show_replace = true;
                    ss.focus_replace = true;
                }
            }
            EditorAction::CloseSearch | EditorAction::Quit => {
                self.search_state = None;
            }
            EditorAction::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.help_scroll = 0;
                }
            }
            _ => {}
        }
        true
    }
    pub(super) fn recompute_search_and_jump(&mut self) {
        if self.search_state.is_none() {
            return;
        }
        let text = self.editor.active().buffer.to_string();
        let cursor_offset = self.editor.active().buffer.cursors.primary().byte_offset;
        if let Some(ss) = &mut self.search_state {
            ss.recompute_matches(&text);
            ss.jump_to_nearest(cursor_offset);
        }
        self.select_current_match();
    }
    pub(super) fn search_next(&mut self) {
        if let Some(ss) = &mut self.search_state {
            ss.next_match();
        }
        self.select_current_match();
    }
    pub(super) fn search_prev(&mut self) {
        if let Some(ss) = &mut self.search_state {
            ss.prev_match();
        }
        self.select_current_match();
    }
    pub(super) fn select_current_match(&mut self) {
        let range = self.search_state.as_ref().and_then(|s| s.current_range());
        if let Some(r) = range {
            self.editor
                .active_mut()
                .buffer
                .move_cursor_to(r.start, false);
            self.editor.active_mut().buffer.move_cursor_to(r.end, true);
        }
    }
    pub(super) fn replace_current(&mut self) {
        let range = self.search_state.as_ref().and_then(|s| s.current_range());
        let replace_text = self
            .search_state
            .as_ref()
            .map(|s| s.replace_text.clone())
            .unwrap_or_default();
        if let Some(r) = range {
            let buf = &mut self.editor.active_mut().buffer;
            buf.begin_batch();
            buf.move_cursor_to(r.start, false);
            buf.move_cursor_to(r.end, true);
            buf.insert_str(&replace_text);
            buf.commit_batch();
        }
        self.recompute_search_and_jump();
    }
    pub(super) fn replace_all(&mut self) {
        let ranges: Vec<_> = self
            .search_state
            .as_ref()
            .map(|s| s.matches.clone())
            .unwrap_or_default();
        let replace_text = self
            .search_state
            .as_ref()
            .map(|s| s.replace_text.clone())
            .unwrap_or_default();
        if ranges.is_empty() {
            return;
        }

        let buf = &mut self.editor.active_mut().buffer;
        buf.begin_batch();
        // Replace in reverse order so earlier byte offsets remain valid.
        for r in ranges.iter().rev() {
            buf.move_cursor_to(r.start, false);
            buf.move_cursor_to(r.end, true);
            buf.insert_str(&replace_text);
        }
        buf.commit_batch();

        self.recompute_search_and_jump();
    }
    pub(super) fn select_all_occurrences(&mut self) {
        // Use the current selection text as the search query (or keep existing query).
        if let Some(selected) = self.selected_text() {
            if self.search_state.is_none() {
                self.search_state = Some(SearchState::new(false));
            }
            if let Some(ss) = &mut self.search_state {
                ss.query = selected;
                ss.case_sensitive = true;
            }
        }
        self.recompute_search_and_jump();
    }
}
