use std::time::{Duration, Instant};

use crate::input::action::{Direction, EditorAction, ScrollDir};
use crate::ui::git_dialog::GitDialogState;

use super::{AppState, DiffPeekState, InputMode};

impl AppState {
    pub(super) fn open_git_dialog(&mut self) {
        if !crate::git::ops::is_repo(&self.workspace) {
            self.status_error = Some("Not a git repository".into());
            return;
        }
        self.git_dialog = Some(GitDialogState::new());
    }
    /// Drive the git dialog. Captures all input until `Esc` from the menu
    /// closes the overlay. Routes nav keys to `GitScreen::move_*`, and
    /// handles per-screen actions inline.
    pub(super) fn handle_git_dialog(&mut self, action: EditorAction) {
        use crate::ui::git_dialog::{ConfirmOp, GitScreen, MenuItem};

        // Quit always closes the dialog without doing anything.
        if matches!(action, EditorAction::Quit | EditorAction::ForceQuit) {
            self.git_dialog = None;
            return;
        }

        // Esc / CloseSearch: step back; if no history, close the dialog.
        if matches!(action, EditorAction::CloseSearch) {
            let close = match self.git_dialog.as_mut() {
                Some(d) => !d.pop(),
                None => true,
            };
            if close {
                self.git_dialog = None;
            }
            return;
        }

        // Navigation keys are uniform across most screens.
        match action {
            EditorAction::MoveCursor(Direction::Up) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.move_up();
                    d.screen.scroll_by(-1);
                }
                return;
            }
            EditorAction::MoveCursor(Direction::Down) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.move_down();
                    d.screen.scroll_by(1);
                }
                return;
            }
            EditorAction::MoveCursorPage(Direction::Up) | EditorAction::Scroll(ScrollDir::Up) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.scroll_by(-5);
                }
                return;
            }
            EditorAction::MoveCursorPage(Direction::Down)
            | EditorAction::Scroll(ScrollDir::Down) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.screen.scroll_by(5);
                }
                return;
            }
            _ => {}
        }

        // Per-screen handling. We snapshot the current screen kind so we can
        // borrow self mutably below for I/O without holding a borrow on
        // `self.git_dialog`.
        let screen_kind = match self.git_dialog.as_ref() {
            Some(d) => d.screen.clone(),
            None => return,
        };

        match (screen_kind, action) {
            // ── Menu ──
            (GitScreen::Menu { selected }, EditorAction::InsertNewline) => {
                let item = MenuItem::ALL[selected];
                self.git_open_menu_item(item);
            }

            // ── Stage ──
            (
                GitScreen::Stage { entries, .. },
                EditorAction::InsertChar(' ') | EditorAction::InsertChar('x'),
            ) => {
                if let Some(GitDialogState {
                    screen:
                        GitScreen::Stage {
                            checked, selected, ..
                        },
                    ..
                }) = self.git_dialog.as_mut()
                    && let Some(slot) = checked.get_mut(*selected)
                    && !entries.is_empty()
                {
                    *slot = !*slot;
                }
            }
            (
                GitScreen::Stage {
                    entries, checked, ..
                },
                EditorAction::InsertNewline,
            ) => {
                self.git_apply_stage(&entries, &checked);
            }

            // ── Branches ──
            (GitScreen::Branches { entries, selected }, EditorAction::InsertNewline) => {
                if let Some(branch) = entries.get(selected) {
                    if branch.current {
                        if let Some(d) = self.git_dialog.as_mut() {
                            d.set_error("Already on this branch");
                        }
                    } else {
                        let name = branch.name.clone();
                        match crate::git::ops::checkout(&self.workspace, &name) {
                            Ok(out) => {
                                self.git_after_branch_change();
                                self.set_git_output("Checkout", out);
                            }
                            Err(e) => self.set_git_error(e),
                        }
                    }
                }
            }
            (GitScreen::Branches { .. }, EditorAction::InsertChar('n')) => {
                self.input_mode = InputMode::GitNewBranch(String::new());
            }
            (GitScreen::Branches { entries, selected }, EditorAction::InsertChar('d')) => {
                if let Some(branch) = entries.get(selected) {
                    if branch.current {
                        if let Some(d) = self.git_dialog.as_mut() {
                            d.set_error("Cannot delete the current branch");
                        }
                    } else if let Some(d) = self.git_dialog.as_mut() {
                        let op = ConfirmOp::DeleteBranch(branch.name.clone());
                        d.push(GitScreen::Confirm { op });
                    }
                }
            }

            // ── Stashes ──
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertNewline) => {
                if let Some(s) = entries.get(selected) {
                    let idx = s.index;
                    match crate::git::ops::stash_apply(&self.workspace, idx) {
                        Ok(out) => self.set_git_output("Stash apply", out),
                        Err(e) => self.set_git_error(e),
                    }
                }
            }
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertChar('p')) => {
                if let Some(s) = entries.get(selected) {
                    let idx = s.index;
                    match crate::git::ops::stash_pop(&self.workspace, idx) {
                        Ok(out) => {
                            self.git_after_branch_change();
                            self.set_git_output("Stash pop", out);
                        }
                        Err(e) => self.set_git_error(e),
                    }
                }
            }
            (GitScreen::Stashes { entries, selected }, EditorAction::InsertChar('d')) => {
                if let Some(s) = entries.get(selected)
                    && let Some(d) = self.git_dialog.as_mut()
                {
                    let op = ConfirmOp::DropStash(s.index);
                    d.push(GitScreen::Confirm { op });
                }
            }
            (GitScreen::Stashes { .. }, EditorAction::InsertChar('n')) => {
                self.input_mode = InputMode::GitStashMessage(String::new());
            }

            // ── Confirm (y/n) ──
            (GitScreen::Confirm { op }, EditorAction::InsertChar('y' | 'Y')) => {
                self.git_run_confirm(op);
            }
            (GitScreen::Confirm { .. }, EditorAction::InsertChar('n' | 'N')) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    d.pop();
                }
            }

            _ => {}
        }
    }
    pub(super) fn git_open_menu_item(&mut self, item: crate::ui::git_dialog::MenuItem) {
        use crate::ui::git_dialog::{GitScreen, MenuItem};

        match item {
            MenuItem::Status => match crate::git::ops::status_summary(&self.workspace) {
                Ok(out) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        d.push(GitScreen::Status {
                            output: out,
                            scroll: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Stage => match crate::git::ops::status(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        let checked = vec![false; entries.len()];
                        d.push(GitScreen::Stage {
                            entries,
                            checked,
                            selected: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Commit => {
                self.input_mode = InputMode::GitCommitMessage(String::new());
            }
            MenuItem::Push => match crate::git::ops::push(&self.workspace) {
                Ok(out) => self.set_git_output("Push", out),
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Pull => match crate::git::ops::pull(&self.workspace) {
                Ok(out) => {
                    self.git_after_branch_change();
                    self.set_git_output("Pull", out);
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Branches => match crate::git::ops::branches(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        let selected = entries.iter().position(|b| b.current).unwrap_or(0);
                        d.push(GitScreen::Branches { entries, selected });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
            MenuItem::Stashes => match crate::git::ops::stashes(&self.workspace) {
                Ok(entries) => {
                    if let Some(d) = self.git_dialog.as_mut() {
                        d.push(GitScreen::Stashes {
                            entries,
                            selected: 0,
                        });
                    }
                }
                Err(e) => self.set_git_error(e),
            },
        }
    }
    /// Apply staging based on the user's checked/unchecked selections.
    /// Files that are currently staged become `git reset` targets; files that
    /// are unstaged or untracked become `git add` targets.
    pub(super) fn git_apply_stage(
        &mut self,
        entries: &[crate::git::ops::StatusEntry],
        checked: &[bool],
    ) {
        let mut to_add: Vec<&std::path::Path> = Vec::new();
        let mut to_reset: Vec<&std::path::Path> = Vec::new();
        for (entry, &is_checked) in entries.iter().zip(checked.iter()) {
            if !is_checked {
                continue;
            }
            if entry.is_staged() {
                to_reset.push(&entry.path);
            } else {
                to_add.push(&entry.path);
            }
        }

        if to_add.is_empty() && to_reset.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Nothing selected");
            }
            return;
        }

        if let Err(e) = crate::git::ops::add(&self.workspace, &to_add) {
            self.set_git_error(e);
            return;
        }
        if let Err(e) = crate::git::ops::reset(&self.workspace, &to_reset) {
            self.set_git_error(e);
            return;
        }

        // Refresh the stage screen with new statuses.
        match crate::git::ops::status(&self.workspace) {
            Ok(entries) => {
                if let Some(d) = self.git_dialog.as_mut() {
                    let checked = vec![false; entries.len()];
                    d.replace(crate::ui::git_dialog::GitScreen::Stage {
                        entries,
                        checked,
                        selected: 0,
                    });
                }
            }
            Err(e) => self.set_git_error(e),
        }
        self.refresh_git_gutter();
    }
    pub(super) fn git_finish_commit(&mut self, message: &str) {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Empty commit message — cancelled");
            }
            return;
        }
        match crate::git::ops::commit(&self.workspace, trimmed) {
            Ok(out) => {
                self.set_git_output("Commit", out);
                self.refresh_git_gutter();
            }
            Err(e) => self.set_git_error(e),
        }
    }
    pub(super) fn git_finish_new_branch(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            if let Some(d) = self.git_dialog.as_mut() {
                d.set_error("Empty branch name — cancelled");
            }
            return;
        }
        match crate::git::ops::create_branch(&self.workspace, trimmed) {
            Ok(out) => {
                self.git_after_branch_change();
                self.set_git_output("Create branch", out);
            }
            Err(e) => self.set_git_error(e),
        }
    }
    pub(super) fn git_finish_stash_push(&mut self, message: &str) {
        let trimmed = message.trim();
        let msg = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
        match crate::git::ops::stash_push(&self.workspace, msg) {
            Ok(out) => {
                self.git_after_branch_change();
                self.set_git_output("Stash push", out);
            }
            Err(e) => self.set_git_error(e),
        }
    }
    pub(super) fn git_run_confirm(&mut self, op: crate::ui::git_dialog::ConfirmOp) {
        use crate::ui::git_dialog::ConfirmOp;
        // Pop the confirm screen first so the result lands on the prior screen.
        if let Some(d) = self.git_dialog.as_mut() {
            d.pop();
        }
        match op {
            ConfirmOp::DropStash(idx) => match crate::git::ops::stash_drop(&self.workspace, idx) {
                Ok(out) => {
                    // Refresh the stash list under us.
                    if let Ok(entries) = crate::git::ops::stashes(&self.workspace)
                        && let Some(d) = self.git_dialog.as_mut()
                    {
                        d.replace(crate::ui::git_dialog::GitScreen::Stashes {
                            entries,
                            selected: 0,
                        });
                    }
                    self.set_git_output("Stash drop", out);
                }
                Err(e) => self.set_git_error(e),
            },
            ConfirmOp::DeleteBranch(name) => {
                match crate::git::ops::delete_branch(&self.workspace, &name) {
                    Ok(out) => {
                        if let Ok(entries) = crate::git::ops::branches(&self.workspace)
                            && let Some(d) = self.git_dialog.as_mut()
                        {
                            let selected = entries.iter().position(|b| b.current).unwrap_or(0);
                            d.replace(crate::ui::git_dialog::GitScreen::Branches {
                                entries,
                                selected,
                            });
                        }
                        self.set_git_output("Delete branch", out);
                    }
                    Err(e) => self.set_git_error(e),
                }
            }
        }
    }
    pub(super) fn set_git_output(&mut self, title: &str, body: String) {
        if let Some(d) = self.git_dialog.as_mut() {
            d.push(crate::ui::git_dialog::GitScreen::Output {
                title: title.into(),
                body,
                scroll: 0,
            });
        }
    }
    pub(super) fn set_git_error(&mut self, err: String) {
        if let Some(d) = self.git_dialog.as_mut() {
            d.set_error(err);
        }
    }
    /// Called after operations that may change which file is on disk under
    /// the active buffer (checkout, pull, stash pop, new branch). Refreshes
    /// the gutter; the existing file watcher will pick up content changes.
    pub(super) fn git_after_branch_change(&mut self) {
        self.refresh_git_gutter();
    }
    /// Recompute the git gutter for the currently active buffer (if it has a path).
    /// Move the cursor to the start of the next (`step == 1`) or previous
    /// (`step == -1`) git hunk. No-op when there are no hunks or git is off.
    pub(super) fn jump_to_relative_hunk(&mut self, step: i32) {
        let Some(gutter) = self.git_gutter.as_ref() else {
            return;
        };
        let hunks = gutter.hunks();
        if hunks.is_empty() {
            self.status_error = Some("No git hunks in this buffer".into());
            return;
        }
        let cur_line = self.editor.active().buffer.cursors.primary().line;
        let target = if step > 0 {
            hunks
                .iter()
                .find(|h| h.start_line > cur_line)
                .copied()
                .unwrap_or(hunks[0])
        } else {
            hunks
                .iter()
                .rev()
                .find(|h| h.end_line < cur_line)
                .copied()
                .unwrap_or(*hunks.last().unwrap())
        };
        self.push_current_to_jump_list();
        let rope = self.editor.active().buffer.rope().clone();
        let line = target.start_line.min(rope.len_lines().saturating_sub(1));
        let cursor = crate::buffer::cursor::Cursor::from_line_col(&rope, line, 0);
        *self.editor.active_mut().buffer.cursors.primary_mut() = cursor;
    }
    /// Replace the hunk containing the cursor with its HEAD content.
    pub(super) fn revert_hunk_at_cursor(&mut self) {
        let Some(gutter) = self.git_gutter.as_ref() else {
            self.status_error = Some("No git gutter available".into());
            return;
        };
        let cur_line = self.editor.active().buffer.cursors.primary().line;
        let hunk_opt = gutter
            .hunks()
            .into_iter()
            .find(|h| cur_line >= h.start_line && cur_line <= h.end_line);
        let Some(hunk) = hunk_opt else {
            self.status_error = Some("Cursor is not inside a hunk".into());
            return;
        };
        let Some(path) = self.editor.active().path.clone() else {
            self.status_error = Some("Save the file before reverting a hunk".into());
            return;
        };
        let head_content = match crate::git::fetch_head_content(&path) {
            Some(c) => c,
            None => {
                self.status_error = Some("File is not tracked in HEAD".into());
                return;
            }
        };
        // Recompute the hunk's HEAD-side line span by re-running the same
        // diff that produced the gutter. We need to find the HEAD lines that
        // map to this hunk's [start_line, end_line] range in the current
        // buffer.
        let current_full = self.editor.active().buffer.to_string();
        let head_lines: Vec<&str> = head_content.lines().collect();
        let current_lines: Vec<&str> = current_full.lines().collect();
        let (head_start, head_end) = crate::git::head_range_for_hunk(
            &head_lines,
            &current_lines,
            hunk.start_line,
            hunk.end_line,
        );

        // Replace the buffer slice [hunk.start_line, hunk.end_line] with the
        // HEAD content slice [head_start, head_end].
        let replacement = if head_start <= head_end && head_end < head_lines.len() {
            let mut s = head_lines[head_start..=head_end].join("\n");
            // Keep the trailing newline so we don't merge the next line into
            // the hunk's last line.
            s.push('\n');
            s
        } else {
            // Pure deletion in HEAD — replacement is empty (matches: hunk
            // exists in current but has no HEAD counterpart).
            String::new()
        };

        let buf = &mut self.editor.active_mut().buffer;
        let start_byte = buf.line_start_byte(hunk.start_line);
        let end_line = (hunk.end_line + 1).min(buf.len_lines());
        let end_byte = buf.line_start_byte(end_line);
        buf.begin_batch();
        buf.delete_range(start_byte, end_byte);
        if !replacement.is_empty() {
            buf.cursors.primary_mut().byte_offset = start_byte;
            buf.insert_str(&replacement);
        }
        buf.commit_batch();
        self.refresh_git_gutter();
    }
    /// Toggle the inline diff-peek float for the hunk under the cursor.
    pub(super) fn toggle_diff_peek(&mut self) {
        if self.diff_peek.is_some() {
            self.diff_peek = None;
            return;
        }
        let Some(gutter) = self.git_gutter.as_ref() else {
            return;
        };
        let cur_line = self.editor.active().buffer.cursors.primary().line;
        let Some(hunk) = gutter
            .hunks()
            .into_iter()
            .find(|h| cur_line >= h.start_line && cur_line <= h.end_line)
        else {
            self.status_error = Some("Cursor is not inside a hunk".into());
            return;
        };
        let Some(path) = self.editor.active().path.clone() else {
            return;
        };
        let Some(head_content) = crate::git::fetch_head_content(&path) else {
            self.status_error = Some("File is not tracked in HEAD".into());
            return;
        };
        let current_full = self.editor.active().buffer.to_string();
        let head_lines: Vec<&str> = head_content.lines().collect();
        let current_lines: Vec<&str> = current_full.lines().collect();
        let (head_start, head_end) = crate::git::head_range_for_hunk(
            &head_lines,
            &current_lines,
            hunk.start_line,
            hunk.end_line,
        );
        let lines: Vec<String> = if head_start <= head_end && head_end < head_lines.len() {
            head_lines[head_start..=head_end]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            vec!["(no HEAD content for this hunk)".into()]
        };
        self.diff_peek = Some(DiffPeekState {
            head_lines: lines,
            anchor_line: cur_line,
        });
    }
    pub(super) fn refresh_git_gutter(&mut self) {
        let path = self.editor.active().path.clone();
        if let Some(path) = path {
            let content = self.editor.active().buffer.to_string();
            self.git_gutter = crate::git::gutter_for_path(&path, &content);
        } else {
            self.git_gutter = None;
        }
    }
    /// Refresh the cached git branch (throttled to every 2 seconds).
    ///
    /// Picks up branch changes made outside the editor (e.g. `git checkout`
    /// from another terminal).
    pub fn refresh_git_branch(&mut self) {
        if self.git_branch_last_checked.elapsed() >= Duration::from_secs(2) {
            self.git_branch = crate::git::current_branch(&self.workspace);
            self.git_branch_last_checked = Instant::now();
        }
    }
}
