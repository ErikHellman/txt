//! First-launch welcome overlay. Shown once when no config file has yet been
//! written, then dismissed and never shown again (the dismissal records the
//! current version into `Config::last_seen_version`).

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

const TITLE: &str = " Welcome to txt ";
const FOOTER: &str = " Press Enter or Esc to continue ";

/// Returns the welcome content as a list of (style_key, text) pairs.
///
/// `style_key` selects rendering style: "h" = heading, "s" = section, "b" =
/// bullet, "k" = key hint, "" = body, "_" = blank line.
fn lines(version: &str) -> Vec<(&'static str, String)> {
    vec![
        ("h", format!("txt {version}")),
        (
            "",
            "A fast, keyboard-driven terminal text editor.".to_string(),
        ),
        ("_", String::new()),
        (
            "",
            "Editing should feel like thought — no modal ceremony,".to_string(),
        ),
        (
            "",
            "no plugin setup, no configuration required to start.".to_string(),
        ),
        ("_", String::new()),
        ("s", "What's inside".to_string()),
        (
            "b",
            "Syntax highlighting (tree-sitter, 17+ languages)".to_string(),
        ),
        (
            "b",
            "LSP — completions, hover docs, go-to-definition".to_string(),
        ),
        ("b", "Multi-cursor & AST-aware selection".to_string()),
        ("b", "Git gutter & fuzzy file picker".to_string()),
        ("b", "File sidebar with full mouse support".to_string()),
        (
            "b",
            "Configurable keymaps (Default / VS Code / IntelliJ)".to_string(),
        ),
        ("_", String::new()),
        ("s", "A few keys to start".to_string()),
        ("k", "F1          Show all key bindings".to_string()),
        ("k", "Ctrl+,      Open settings".to_string()),
        ("k", "Ctrl+P      Fuzzy file picker".to_string()),
        ("k", "Ctrl+S      Save".to_string()),
        ("k", "Ctrl+Q      Quit".to_string()),
    ]
}

/// Render the welcome overlay as a centered floating panel.
pub fn render(area: Rect, buf: &mut TermBuffer, scroll: usize, version: &str) {
    if area.width < 24 || area.height < 8 {
        return;
    }

    let entries = lines(version);

    let bg = Color::Rgb(18, 26, 44);
    let border = Color::Rgb(80, 120, 180);
    let border_style = Style::default().bg(bg).fg(border);
    let title_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(220, 230, 255))
        .add_modifier(Modifier::BOLD);
    let heading_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(140, 200, 255))
        .add_modifier(Modifier::BOLD);
    let section_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(120, 160, 220))
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().bg(bg).fg(Color::Rgb(210, 215, 230));
    let bullet_style = Style::default().bg(bg).fg(Color::Rgb(190, 200, 220));
    let key_style = Style::default().bg(bg).fg(Color::Rgb(160, 220, 200));

    const INNER_W: u16 = 56;
    const OVERLAY_W: u16 = INNER_W + 2;

    let overlay_w = OVERLAY_W.min(area.width);
    let overlay_h = area.height.saturating_sub(2).max(8).min(area.height);

    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 2;
    let overlay = Rect::new(ox, oy, overlay_w, overlay_h);

    // Background fill.
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }
    super::overlay_chrome::draw_border(buf, overlay, border_style);

    // Title centered on the top border.
    let tx = overlay.x + overlay.width.saturating_sub(TITLE.len() as u16) / 2;
    buf.set_string(tx, overlay.y, TITLE, title_style);

    // Separator beneath the title row.
    let sep_y = overlay.y + 2;
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, sep_y, "\u{2500}", border_style);
    }

    // Footer hint centered on the bottom border.
    let fy = overlay.y + overlay.height.saturating_sub(1);
    let fx = overlay.x + overlay.width.saturating_sub(FOOTER.len() as u16) / 2;
    buf.set_string(fx, fy, FOOTER, border_style);

    // Content area: between top chrome (border + title + separator) and bottom border.
    let content_x = overlay.x + 2;
    let content_w = overlay.width.saturating_sub(4) as usize;
    let content_start_y = overlay.y + 3;
    let content_end_y = overlay.y + overlay.height.saturating_sub(1);
    let visible_rows = content_end_y.saturating_sub(content_start_y) as usize;

    let max_scroll = entries.len().saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    for (row_idx, (kind, text)) in entries.iter().skip(scroll).enumerate() {
        let cy = content_start_y + row_idx as u16;
        if cy >= content_end_y {
            break;
        }
        let (style, prefix) = match *kind {
            "h" => (heading_style, ""),
            "s" => (section_style, ""),
            "b" => (bullet_style, "  • "),
            "k" => (key_style, "  "),
            "_" => continue,
            _ => (body_style, ""),
        };
        let line = format!("{prefix}{text}");
        let display = truncate_to(&line, content_w);
        buf.set_string(content_x, cy, &display, style);
    }

    // Scroll indicators.
    if scroll > 0 {
        let ind_x = overlay.x + overlay.width.saturating_sub(5);
        buf.set_string(ind_x, overlay.y, " \u{2191} ", border_style);
    }
    let entries_shown = visible_rows.min(entries.len().saturating_sub(scroll));
    if scroll + entries_shown < entries.len() {
        let ind_x = overlay.x + overlay.width.saturating_sub(5);
        buf.set_string(ind_x, fy, " \u{2193} ", border_style);
    }
}

fn truncate_to(s: &str, max: usize) -> String {
    crate::ui::text_utils::truncate_to_width(s, max).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_include_version() {
        let l = lines("0.3.0");
        let head = l.first().expect("has heading");
        assert_eq!(head.0, "h");
        assert!(head.1.contains("0.3.0"));
    }

    #[test]
    fn render_skips_tiny_area() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = TermBuffer::empty(area);
        render(area, &mut buf, 0, "0.3.0");
        let all_blank = buf.content().iter().all(|c| c.symbol() == " ");
        assert!(all_blank);
    }

    #[test]
    fn render_does_not_panic_on_normal_area() {
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = TermBuffer::empty(area);
        render(area, &mut buf, 0, "0.3.0");
        let has_border = buf
            .content()
            .iter()
            .any(|c| c.symbol() == "\u{256d}" || c.symbol() == "\u{2500}");
        assert!(has_border);
    }

    #[test]
    fn render_with_scroll_does_not_panic() {
        let area = Rect::new(0, 0, 100, 12);
        let mut buf = TermBuffer::empty(area);
        render(area, &mut buf, 1000, "0.3.0");
    }
}
