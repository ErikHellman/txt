//! Git operations dialog. A single overlay with a [`GitScreen`] state machine
//! exposing the common workflow: view status, stage files, commit, push/pull,
//! switch/create branches, and manage stashes.
//!
//! Modeled on [`crate::ui::command_palette`]: an `Option<GitDialogState>`
//! lives on `AppState`, the renderer is a centered float, and input is
//! routed through `AppState::handle_git_dialog`.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::git::ops::{BranchEntry, StashEntry, StatusEntry};
use crate::theme::ThemeColors;

// ── Menu items ───────────────────────────────────────────────────────────────

/// Top-level operations shown on the menu screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Status,
    Stage,
    Commit,
    Push,
    Pull,
    Branches,
    Stashes,
}

impl MenuItem {
    pub const ALL: &'static [MenuItem] = &[
        MenuItem::Status,
        MenuItem::Stage,
        MenuItem::Commit,
        MenuItem::Push,
        MenuItem::Pull,
        MenuItem::Branches,
        MenuItem::Stashes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Status => "Status",
            MenuItem::Stage => "Stage / Unstage files",
            MenuItem::Commit => "Commit…",
            MenuItem::Push => "Push",
            MenuItem::Pull => "Pull",
            MenuItem::Branches => "Branches…",
            MenuItem::Stashes => "Stashes…",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            MenuItem::Status => "show working tree status",
            MenuItem::Stage => "Space toggles, Enter applies",
            MenuItem::Commit => "type a message, Enter to confirm",
            MenuItem::Push => "git push",
            MenuItem::Pull => "git pull",
            MenuItem::Branches => "switch / create / delete",
            MenuItem::Stashes => "apply / pop / drop / push",
        }
    }
}

// ── Confirm operations ───────────────────────────────────────────────────────

/// Operations that need a y/n confirmation before running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmOp {
    DropStash(usize),
    DeleteBranch(String),
}

impl ConfirmOp {
    pub fn prompt(&self) -> String {
        match self {
            ConfirmOp::DropStash(i) => format!("Drop stash@{{{}}}? (y/n)", i),
            ConfirmOp::DeleteBranch(name) => format!("Delete branch '{}'? (y/n)", name),
        }
    }
}

// ── Screens ──────────────────────────────────────────────────────────────────

/// One screen of the dialog. Each screen owns the data it displays so the
/// renderer never needs to hit the disk.
#[derive(Debug, Clone)]
pub enum GitScreen {
    Menu {
        selected: usize,
    },
    Status {
        output: String,
        scroll: usize,
    },
    Stage {
        entries: Vec<StatusEntry>,
        checked: Vec<bool>,
        selected: usize,
    },
    Branches {
        entries: Vec<BranchEntry>,
        selected: usize,
    },
    Stashes {
        entries: Vec<StashEntry>,
        selected: usize,
    },
    Confirm {
        op: ConfirmOp,
    },
    /// Read-only output display after running an operation. `Esc` returns to
    /// the previous screen.
    Output {
        /// Operation name shown as a body header (e.g. "Push", "Commit").
        title: String,
        body: String,
        scroll: usize,
    },
}

// ── State ────────────────────────────────────────────────────────────────────

/// Mutable state for the git operations overlay.
#[derive(Debug, Clone)]
pub struct GitDialogState {
    pub screen: GitScreen,
    /// Stack of previous screens, for `Esc`-to-back navigation.
    pub history: Vec<GitScreen>,
    /// Latest error message, rendered as a banner above the body. Cleared on
    /// successful operations and on screen transitions that aren't error
    /// recoveries.
    pub last_error: Option<String>,
}

impl GitDialogState {
    pub fn new() -> Self {
        Self {
            screen: GitScreen::Menu { selected: 0 },
            history: Vec::new(),
            last_error: None,
        }
    }

    /// Navigate to a new screen, pushing the current screen onto the history.
    pub fn push(&mut self, next: GitScreen) {
        let current = std::mem::replace(&mut self.screen, next);
        self.history.push(current);
        self.last_error = None;
    }

    /// Return to the previous screen if any. Returns `false` when the dialog
    /// should be closed (already at the menu with no history).
    pub fn pop(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            self.screen = prev;
            self.last_error = None;
            true
        } else {
            false
        }
    }

    /// Replace the active screen without touching history. Used when a
    /// completed operation lands on a fresh result screen.
    pub fn replace(&mut self, next: GitScreen) {
        self.screen = next;
    }

    pub fn set_error(&mut self, err: impl Into<String>) {
        self.last_error = Some(err.into());
    }

    #[allow(dead_code)]
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
}

impl Default for GitDialogState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers used by the dispatch handler ─────────────────────────────────────

impl GitScreen {
    /// Number of selectable rows in the body.
    pub fn list_len(&self) -> usize {
        match self {
            GitScreen::Menu { .. } => MenuItem::ALL.len(),
            GitScreen::Stage { entries, .. } => entries.len(),
            GitScreen::Branches { entries, .. } => entries.len(),
            GitScreen::Stashes { entries, .. } => entries.len(),
            _ => 0,
        }
    }

    /// Move the highlighted row up if applicable.
    pub fn move_up(&mut self) {
        let sel = self.selected_mut();
        if let Some(s) = sel
            && *s > 0
        {
            *s -= 1;
        }
    }

    /// Move the highlighted row down if applicable.
    pub fn move_down(&mut self) {
        let len = self.list_len();
        if let Some(s) = self.selected_mut()
            && *s + 1 < len
        {
            *s += 1;
        }
    }

    /// Scroll the read-only screens by `delta` rows (positive = down).
    pub fn scroll_by(&mut self, delta: isize) {
        match self {
            GitScreen::Status { scroll, .. } | GitScreen::Output { scroll, .. } => {
                if delta < 0 {
                    *scroll = scroll.saturating_sub((-delta) as usize);
                } else {
                    *scroll = scroll.saturating_add(delta as usize);
                }
            }
            _ => {}
        }
    }

    fn selected_mut(&mut self) -> Option<&mut usize> {
        match self {
            GitScreen::Menu { selected }
            | GitScreen::Stage { selected, .. }
            | GitScreen::Branches { selected, .. }
            | GitScreen::Stashes { selected, .. } => Some(selected),
            _ => None,
        }
    }

    /// Title shown at the top of the overlay.
    pub fn title(&self) -> &'static str {
        match self {
            GitScreen::Menu { .. } => " Git ",
            GitScreen::Status { .. } => " Git · Status ",
            GitScreen::Stage { .. } => " Git · Stage / Unstage ",
            GitScreen::Branches { .. } => " Git · Branches ",
            GitScreen::Stashes { .. } => " Git · Stashes ",
            GitScreen::Confirm { .. } => " Git · Confirm ",
            GitScreen::Output { .. } => " Git · Output ",
        }
    }

    /// Footer hints — short, comma-separated.
    pub fn hint(&self) -> &'static str {
        match self {
            GitScreen::Menu { .. } => "↑↓ navigate  ·  Enter select  ·  Esc close",
            GitScreen::Stage { .. } => "Space toggle  ·  Enter stage/unstage  ·  Esc back",
            GitScreen::Branches { .. } => "Enter switch  ·  n new  ·  d delete  ·  Esc back",
            GitScreen::Stashes { .. } => "Enter apply  ·  p pop  ·  d drop  ·  n push  ·  Esc back",
            GitScreen::Status { .. } | GitScreen::Output { .. } => "↑↓ scroll  ·  Esc back",
            GitScreen::Confirm { .. } => "y confirm  ·  n cancel",
        }
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

const OVERLAY_W: u16 = 70;
const OVERLAY_H: u16 = 22;

pub fn render(state: &GitDialogState, theme: &ThemeColors, area: Rect, buf: &mut TermBuffer) {
    if area.width < 30 || area.height < 8 {
        return;
    }

    let ow = OVERLAY_W.min(area.width);
    let oh = OVERLAY_H.min(area.height);
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    let bg = theme.picker_bg;
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 100, 160));
    let title_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 200, 255))
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));
    let selected_style = Style::default()
        .bg(theme.picker_sel_bg)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(120, 140, 170));
    let error_style = Style::default()
        .bg(Color::Rgb(140, 30, 30))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    // Background fill.
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    draw_border(buf, overlay, border_style);

    // Title.
    let title = state.screen.title();
    let tx = overlay.x + overlay.width.saturating_sub(title.len() as u16) / 2;
    buf.set_string(tx, overlay.y, title, title_style);

    let inner_w = overlay.width.saturating_sub(2) as usize;
    let mut row = overlay.y + 1;
    let body_bottom = overlay.y + overlay.height - 2; // leave 1 for hint

    // Optional error banner.
    if let Some(err) = &state.last_error
        && row < body_bottom
    {
        let line = clip(&format!(" ! {} ", err), inner_w);
        for x in overlay.x + 1..overlay.x + overlay.width - 1 {
            buf.set_string(x, row, " ", error_style);
        }
        buf.set_string(overlay.x + 1, row, &line, error_style);
        row += 1;
    }

    // Body.
    let mut ctx = BodyCtx {
        buf,
        overlay,
        start_y: row,
        end_y: body_bottom,
        normal: body_style,
        sel: selected_style,
    };
    match &state.screen {
        GitScreen::Menu { selected } => ctx.render_menu(*selected),
        GitScreen::Status { output, scroll } => ctx.render_text(output, *scroll),
        GitScreen::Output {
            title,
            body,
            scroll,
        } => {
            let header_y = ctx.start_y;
            if header_y < ctx.end_y {
                let header = format!(" {} ", title);
                let line = clip(&header, (ctx.overlay.width - 2) as usize);
                let title_local = Style::default()
                    .bg(theme.picker_bg)
                    .fg(Color::Rgb(140, 200, 140))
                    .add_modifier(Modifier::BOLD);
                ctx.buf
                    .set_string(ctx.overlay.x + 1, header_y, &line, title_local);
            }
            ctx.start_y = (header_y + 1).min(ctx.end_y);
            ctx.render_text(body, *scroll);
        }
        GitScreen::Stage {
            entries,
            checked,
            selected,
        } => ctx.render_stage(entries, checked, *selected),
        GitScreen::Branches { entries, selected } => ctx.render_branches(entries, *selected),
        GitScreen::Stashes { entries, selected } => ctx.render_stashes(entries, *selected),
        GitScreen::Confirm { op } => ctx.render_confirm(&op.prompt()),
    }

    // Footer hint.
    let hint = state.screen.hint();
    let hint_y = overlay.y + overlay.height - 1;
    let hx = overlay.x + overlay.width.saturating_sub(hint.len() as u16) / 2;
    let line = clip(hint, inner_w);
    buf.set_string(hx.max(overlay.x + 1), hint_y, &line, hint_style);
}

/// Layout + style context used by the body sub-renderers. Bundles parameters
/// that would otherwise inflate every helper signature.
struct BodyCtx<'a> {
    buf: &'a mut TermBuffer,
    overlay: Rect,
    start_y: u16,
    end_y: u16,
    normal: Style,
    sel: Style,
}

impl BodyCtx<'_> {
    fn inner_w(&self) -> usize {
        (self.overlay.width - 2) as usize
    }

    fn fill_row(&mut self, y: u16, style: Style) {
        for x in self.overlay.x + 1..self.overlay.x + self.overlay.width - 1 {
            self.buf.set_string(x, y, " ", style);
        }
    }

    fn render_menu(&mut self, selected: usize) {
        for (i, item) in MenuItem::ALL.iter().enumerate() {
            let y = self.start_y + i as u16;
            if y >= self.end_y {
                break;
            }
            let is_sel = i == selected;
            let style = if is_sel { self.sel } else { self.normal };
            self.fill_row(y, style);
            let marker = if is_sel { ">" } else { " " };
            let line = format!(" {} {:<22}  {}", marker, item.label(), item.hint());
            let line = clip(&line, self.inner_w());
            self.buf.set_string(self.overlay.x + 1, y, &line, style);
        }
    }

    fn render_text(&mut self, text: &str, scroll: usize) {
        let inner_w = self.inner_w();
        let lines: Vec<&str> = text.lines().collect();
        let visible_rows = self.end_y.saturating_sub(self.start_y) as usize;
        let max_scroll = lines.len().saturating_sub(visible_rows);
        let scroll = scroll.min(max_scroll);

        for (row_off, line) in lines.iter().skip(scroll).take(visible_rows).enumerate() {
            let y = self.start_y + row_off as u16;
            let display = clip(line, inner_w);
            self.buf
                .set_string(self.overlay.x + 1, y, &display, self.normal);
        }

        if lines.is_empty() && self.start_y < self.end_y {
            self.buf
                .set_string(self.overlay.x + 2, self.start_y, "(no output)", self.normal);
        }
    }

    fn render_stage(&mut self, entries: &[StatusEntry], checked: &[bool], selected: usize) {
        if entries.is_empty() {
            if self.start_y < self.end_y {
                self.buf.set_string(
                    self.overlay.x + 2,
                    self.start_y,
                    "(working tree clean)",
                    self.normal,
                );
            }
            return;
        }
        let inner_w = self.inner_w();
        let visible_rows = self.end_y.saturating_sub(self.start_y) as usize;
        let scroll = if selected >= visible_rows {
            selected - visible_rows + 1
        } else {
            0
        };

        for (row_off, idx) in (scroll..entries.len()).take(visible_rows).enumerate() {
            let y = self.start_y + row_off as u16;
            let entry = &entries[idx];
            let is_sel = idx == selected;
            let style = if is_sel { self.sel } else { self.normal };
            self.fill_row(y, style);
            let mark = if checked.get(idx).copied().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            let cursor = if is_sel { ">" } else { " " };
            let xy = format!("{}{}", entry.index, entry.worktree);
            let line = format!(
                " {} {} {} {}",
                cursor,
                mark,
                xy,
                entry.path.to_string_lossy()
            );
            let line = clip(&line, inner_w);
            self.buf.set_string(self.overlay.x + 1, y, &line, style);
        }
    }

    fn render_branches(&mut self, entries: &[BranchEntry], selected: usize) {
        if entries.is_empty() {
            if self.start_y < self.end_y {
                self.buf.set_string(
                    self.overlay.x + 2,
                    self.start_y,
                    "(no branches)",
                    self.normal,
                );
            }
            return;
        }
        let inner_w = self.inner_w();
        let visible_rows = self.end_y.saturating_sub(self.start_y) as usize;
        let scroll = if selected >= visible_rows {
            selected - visible_rows + 1
        } else {
            0
        };

        for (row_off, idx) in (scroll..entries.len()).take(visible_rows).enumerate() {
            let y = self.start_y + row_off as u16;
            let entry = &entries[idx];
            let is_sel = idx == selected;
            let style = if is_sel { self.sel } else { self.normal };
            self.fill_row(y, style);
            let cursor = if is_sel { ">" } else { " " };
            let marker = if entry.current { "*" } else { " " };
            let line = format!(" {} {} {}", cursor, marker, entry.name);
            let line = clip(&line, inner_w);
            self.buf.set_string(self.overlay.x + 1, y, &line, style);
        }
    }

    fn render_stashes(&mut self, entries: &[StashEntry], selected: usize) {
        if entries.is_empty() {
            if self.start_y < self.end_y {
                self.buf.set_string(
                    self.overlay.x + 2,
                    self.start_y,
                    "(no stashes)",
                    self.normal,
                );
            }
            return;
        }
        let inner_w = self.inner_w();
        let visible_rows = self.end_y.saturating_sub(self.start_y) as usize;
        let scroll = if selected >= visible_rows {
            selected - visible_rows + 1
        } else {
            0
        };

        for (row_off, idx) in (scroll..entries.len()).take(visible_rows).enumerate() {
            let y = self.start_y + row_off as u16;
            let entry = &entries[idx];
            let is_sel = idx == selected;
            let style = if is_sel { self.sel } else { self.normal };
            self.fill_row(y, style);
            let cursor = if is_sel { ">" } else { " " };
            let line = format!(" {} stash@{{{}}}: {}", cursor, entry.index, entry.message);
            let line = clip(&line, inner_w);
            self.buf.set_string(self.overlay.x + 1, y, &line, style);
        }
    }

    fn render_confirm(&mut self, prompt: &str) {
        if self.start_y >= self.end_y {
            return;
        }
        let line = clip(prompt, self.inner_w());
        let cy = (self.start_y + self.end_y) / 2;
        let cx = self.overlay.x + self.overlay.width.saturating_sub(line.len() as u16) / 2;
        self.buf
            .set_string(cx.max(self.overlay.x + 1), cy, &line, self.normal);
    }
}

// ── Drawing helpers ──────────────────────────────────────────────────────────

fn draw_border(buf: &mut TermBuffer, area: Rect, style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);

    buf.set_string(x0, y0, "╭", style);
    buf.set_string(x1, y0, "╮", style);
    buf.set_string(x0, y1, "╰", style);
    buf.set_string(x1, y1, "╯", style);
    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "─", style);
        buf.set_string(x, y1, "─", style);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "│", style);
        buf.set_string(x1, y, "│", style);
    }
}

fn clip(s: &str, width: usize) -> String {
    if s.len() <= width {
        s.to_string()
    } else {
        let mut end = width;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_starts_at_menu() {
        let s = GitDialogState::new();
        assert!(matches!(s.screen, GitScreen::Menu { selected: 0 }));
        assert!(s.history.is_empty());
    }

    #[test]
    fn push_pop_round_trip() {
        let mut s = GitDialogState::new();
        s.push(GitScreen::Status {
            output: "ok".into(),
            scroll: 0,
        });
        assert!(matches!(s.screen, GitScreen::Status { .. }));
        assert_eq!(s.history.len(), 1);
        assert!(s.pop());
        assert!(matches!(s.screen, GitScreen::Menu { .. }));
        // Popping the menu (no history left) returns false → caller closes the dialog.
        assert!(!s.pop());
    }

    #[test]
    fn move_up_down_clamps() {
        let mut s = GitScreen::Menu { selected: 0 };
        s.move_up();
        if let GitScreen::Menu { selected } = s {
            assert_eq!(selected, 0);
        }
        for _ in 0..50 {
            s.move_down();
        }
        if let GitScreen::Menu { selected } = s {
            assert_eq!(selected, MenuItem::ALL.len() - 1);
        }
    }

    #[test]
    fn confirm_op_prompt_includes_target() {
        let p = ConfirmOp::DropStash(2).prompt();
        assert!(p.contains("2"));
        let p = ConfirmOp::DeleteBranch("feature/x".into()).prompt();
        assert!(p.contains("feature/x"));
    }

    #[test]
    fn set_and_clear_error() {
        let mut s = GitDialogState::new();
        s.set_error("nope");
        assert_eq!(s.last_error.as_deref(), Some("nope"));
        s.clear_error();
        assert!(s.last_error.is_none());
    }

    #[test]
    fn render_does_not_panic_on_each_screen() {
        let theme = crate::theme::ThemeColors::for_theme(&crate::config::Theme::Default);
        let area = Rect::new(0, 0, 100, 40);

        let screens = vec![
            GitScreen::Menu { selected: 0 },
            GitScreen::Status {
                output: "M file.rs".into(),
                scroll: 0,
            },
            GitScreen::Stage {
                entries: vec![],
                checked: vec![],
                selected: 0,
            },
            GitScreen::Branches {
                entries: vec![],
                selected: 0,
            },
            GitScreen::Stashes {
                entries: vec![],
                selected: 0,
            },
            GitScreen::Confirm {
                op: ConfirmOp::DropStash(0),
            },
            GitScreen::Output {
                title: "".into(),
                body: "done".into(),
                scroll: 0,
            },
        ];
        for screen in screens {
            let s = GitDialogState {
                screen,
                history: Vec::new(),
                last_error: None,
            };
            let mut buf = TermBuffer::empty(area);
            render(&s, &theme, area, &mut buf);
        }
    }
}
