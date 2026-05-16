use crate::editor::viewport::{screen_pos_to_byte_offset, screen_pos_to_line_display_col};
use crate::input::action::{EditorAction, ScrollDir};
use crate::ui::editor_view::effective_gutter_width;

use super::AppState;

impl AppState {
    /// Hit-test the tab strip. Returns the tab index when `(col, row)`
    /// lands on a rendered tab label.
    pub(super) fn tab_bar_tab_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.tab_bar_area?;
        crate::ui::tab_bar::tab_at(&self.editor, area, col, row)
    }
    /// Returns `true` when `action` is a vertical scroll that the viewport
    /// (or sidebar) can't honour because it's already at the corresponding
    /// edge. Used by the run loop to drop end-of-buffer wheel spam before
    /// it ever reaches `update` and triggers a redraw.
    ///
    /// Horizontal and non-scroll actions always return `false` — the
    /// horizontal case would need an O(n) max-line-width scan per event,
    /// which is more expensive than letting `update` handle it normally.
    pub(crate) fn scroll_action_is_no_op(&self, action: &EditorAction) -> bool {
        let dir = match action {
            EditorAction::Scroll(d) => *d,
            EditorAction::MouseScroll { dir, col, row } => {
                if self.point_in_sidebar(*col, *row) {
                    return self.sidebar_scroll_is_no_op(*dir);
                }
                *dir
            }
            _ => return false,
        };

        let tab = self.editor.active();
        let vp = &tab.viewport;
        let total_lines = tab.buffer.len_lines();
        let max_row = total_lines.saturating_sub(1);

        match dir {
            ScrollDir::Up | ScrollDir::HalfPageUp => vp.scroll_row == 0,
            ScrollDir::Down | ScrollDir::HalfPageDown => vp.scroll_row >= max_row,
            ScrollDir::Left | ScrollDir::Right => false,
        }
    }
    /// Whether a wheel-scroll over the sidebar would be a no-op (already at
    /// top or already past the end of the entry list).
    pub(super) fn sidebar_scroll_is_no_op(&self, dir: ScrollDir) -> bool {
        let sb = match self.sidebar.as_ref() {
            Some(sb) => sb,
            None => return true,
        };
        let h = self.sidebar_area.map(|r| r.height as usize).unwrap_or(0);
        let max = if h == 0 || sb.entries.len() <= h {
            0
        } else {
            sb.entries.len() - h
        };
        match dir {
            ScrollDir::Up => sb.scroll_offset == 0,
            ScrollDir::Down => sb.scroll_offset >= max,
            _ => true,
        }
    }
    /// Returns true if the given screen point is inside the sidebar's
    /// entry-list area (excludes the separator column).
    pub(super) fn point_in_sidebar(&self, col: u16, row: u16) -> bool {
        match self.sidebar_area {
            Some(area) => {
                col >= area.x
                    && col < area.x + self.sidebar_width
                    && row >= area.y
                    && row < area.y + area.height
            }
            None => false,
        }
    }
    /// Returns true if the given screen point is on the sidebar's separator
    /// column (the 1-column-wide vertical bar between sidebar and editor).
    pub(super) fn point_on_separator(&self, col: u16, row: u16) -> bool {
        match self.sidebar_area {
            Some(area) => {
                col == area.x + self.sidebar_width && row >= area.y && row < area.y + area.height
            }
            None => false,
        }
    }
    /// Map a screen `row` inside the sidebar to the corresponding entry index.
    /// Returns `None` if the row is outside the sidebar or past the last entry.
    pub(super) fn sidebar_entry_at(&self, row: u16) -> Option<usize> {
        let area = self.sidebar_area?;
        if row < area.y || row >= area.y + area.height {
            return None;
        }
        let sb = self.sidebar.as_ref()?;
        let screen_row = (row - area.y) as usize;
        let idx = sb.scroll_offset + screen_row;
        if idx < sb.entries.len() {
            Some(idx)
        } else {
            None
        }
    }
    /// Sync the sidebar's `scroll_offset` so the selected entry remains visible.
    /// Called after any keyboard navigation that may move `selected` off-screen.
    pub(super) fn ensure_sidebar_selected_visible(&mut self) {
        let h = self.sidebar_area.map(|r| r.height as usize).unwrap_or(0);
        if let Some(sb) = &mut self.sidebar {
            sb.ensure_selected_visible(h);
        }
    }
    pub(super) fn screen_to_byte(&self, col: u16, row: u16) -> Option<usize> {
        let (adjusted_col, editor_area_y, gutter_cols, text_width) =
            self.screen_mouse_geometry(col)?;
        Some(screen_pos_to_byte_offset(
            adjusted_col,
            row,
            editor_area_y,
            gutter_cols,
            text_width,
            &self.editor.active().buffer,
            &self.editor.active().viewport,
        ))
    }
    /// Convert a screen position into `(line, display_col)` for box selection.
    /// Returns `None` if the click landed in the sidebar.
    pub(super) fn screen_to_line_col(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let (adjusted_col, editor_area_y, gutter_cols, text_width) =
            self.screen_mouse_geometry(col)?;
        Some(screen_pos_to_line_display_col(
            adjusted_col,
            row,
            editor_area_y,
            gutter_cols,
            text_width,
            &self.editor.active().buffer,
            &self.editor.active().viewport,
        ))
    }
    /// Shared geometry for mouse-to-buffer conversion: returns
    /// `(adjusted_col, editor_area_y, gutter_cols, text_width)`, or `None`
    /// when the click is inside the sidebar. `text_width` matches the column
    /// width the renderer uses for word wrap.
    pub(super) fn screen_mouse_geometry(&self, col: u16) -> Option<(u16, u16, u16, u16)> {
        let editor_area_y: u16 = if self.editor.tab_count() > 1 { 1 } else { 0 };
        let sidebar_offset: u16 = if self.sidebar.is_some() {
            self.sidebar_width + 1
        } else {
            0
        };
        if self.sidebar.is_some() && col < sidebar_offset {
            return None;
        }
        let adjusted_col = col.saturating_sub(sidebar_offset);
        let handle = self.editor.active();
        let total_lines = handle.buffer.len_lines();
        let label = self.version_badge();
        let gutter = effective_gutter_width(total_lines, label.as_deref());
        let git_col_w: u16 = if self.git_gutter.is_some() {
            crate::ui::editor_view::GIT_GUTTER_W
        } else {
            0
        };
        let diag_col_w: u16 = if !handle.lsp_state.diagnostics.is_empty() {
            crate::ui::editor_view::DIAG_GUTTER_W
        } else {
            0
        };
        let has_folds = (0..total_lines).any(|i| handle.folds.is_fold_start_candidate(i));
        let fold_col_w: u16 = if has_folds {
            crate::ui::editor_view::FOLD_GUTTER_W
        } else {
            0
        };
        let gutter_cols =
            git_col_w + diag_col_w + fold_col_w + gutter + crate::ui::editor_view::GUTTER_PAD;
        let editor_area_w = self.term_width.saturating_sub(sidebar_offset);
        let text_width = editor_area_w.saturating_sub(gutter_cols);
        Some((adjusted_col, editor_area_y, gutter_cols, text_width))
    }
}
