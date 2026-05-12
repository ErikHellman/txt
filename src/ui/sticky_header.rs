//! Sticky header row at the top of the editor pane.
//!
//! Displays the enclosing function/class/module of the cursor's position,
//! computed from the tree-sitter parse tree via
//! [`crate::syntax::SyntaxHost::enclosing_named_path`]. Renders nothing when
//! the path is empty so no row is reserved unnecessarily.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::syntax::EnclosingSymbol;

/// Format the breadcrumb path as `kind name › kind name › …`. Returns an
/// empty string for an empty path.
pub fn format_path(path: &[EnclosingSymbol]) -> String {
    let mut out = String::new();
    for (i, sym) in path.iter().enumerate() {
        if i > 0 {
            out.push_str(" › ");
        }
        out.push_str(sym.kind);
        out.push(' ');
        out.push_str(&sym.name);
    }
    out
}

/// Render one row showing the enclosing-symbol breadcrumb. The caller is
/// responsible for shrinking the editor area by 1 row when this is drawn.
pub fn render(path: &[EnclosingSymbol], area: Rect, buf: &mut TermBuffer) {
    if area.width == 0 || area.height == 0 || path.is_empty() {
        return;
    }
    let bg = Color::Rgb(22, 26, 40);
    let fg = Color::Rgb(180, 190, 220);
    let dim = Color::Rgb(110, 130, 170);
    let bar_style = Style::default().bg(bg).fg(fg);

    // Background fill for the header row.
    for x in area.x..area.x + area.width {
        buf.set_string(x, area.y, " ", bar_style);
    }

    let label = format_path(path);
    let max = (area.width as usize).saturating_sub(2);
    let truncated = truncate_left_keep_right(&label, max);
    let prefix_style = bar_style.add_modifier(Modifier::ITALIC).fg(dim);
    buf.set_string(area.x, area.y, " ", prefix_style);
    buf.set_string(
        area.x + 1,
        area.y,
        &truncated,
        bar_style.add_modifier(Modifier::BOLD),
    );
}

/// Truncate `s` to fit in `max_chars` columns, keeping the rightmost
/// (innermost) part of the breadcrumb. Falls back to a left truncate if the
/// string already fits.
fn truncate_left_keep_right(s: &str, max_chars: usize) -> String {
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

    fn sym(kind: &'static str, name: &str) -> EnclosingSymbol {
        EnclosingSymbol {
            kind,
            name: name.into(),
        }
    }

    #[test]
    fn format_path_joins_with_chevron() {
        let path = vec![sym("impl", "Bar"), sym("fn", "baz")];
        assert_eq!(format_path(&path), "impl Bar › fn baz");
    }

    #[test]
    fn format_path_empty_yields_empty_string() {
        assert_eq!(format_path(&[]), "");
    }

    #[test]
    fn truncate_left_keep_right_preserves_innermost() {
        let s = "very long path › fn baz";
        let out = truncate_left_keep_right(s, 10);
        assert!(out.starts_with('…'), "out={out:?}");
        assert!(out.ends_with("fn baz"), "out={out:?}");
    }

    #[test]
    fn truncate_left_no_op_when_fits() {
        assert_eq!(truncate_left_keep_right("fn baz", 20), "fn baz");
    }

    #[test]
    fn render_skips_empty_path() {
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = TermBuffer::empty(area);
        let initial = buf.cell((0, 0)).unwrap().symbol().to_string();
        render(&[], area, &mut buf);
        // No writes for an empty path — the cell content is unchanged.
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), initial);
    }

    #[test]
    fn render_writes_path_when_non_empty() {
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = TermBuffer::empty(area);
        render(&[sym("fn", "main")], area, &mut buf);
        // The full breadcrumb starts at column 1 (after the leading space pad).
        let mut text = String::new();
        for x in 0..40 {
            text.push_str(buf.cell((x, 0)).unwrap().symbol());
        }
        assert!(text.contains("fn main"), "rendered text was {text:?}");
    }
}
