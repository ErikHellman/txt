//! Post-upgrade changelog overlay. Shows every `## v…` section in the
//! embedded `CHANGELOG.md` whose version is newer than the user's previously
//! seen version.

use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::config::parse_full_version;

const CHANGELOG_SOURCE: &str = include_str!("../../CHANGELOG.md");
const TITLE: &str = " What's new ";
const FOOTER: &str = " Press Enter or Esc to continue ";

/// One section parsed from `CHANGELOG.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub version: String,
    pub bullets: Vec<String>,
}

/// Parse the embedded changelog into ordered sections (newest first, as written).
pub fn parse_changelog(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push(Section {
                version: rest.trim().to_string(),
                bullets: Vec::new(),
            });
        } else if let Some(rest) = trimmed.strip_prefix("- ")
            && let Some(last) = out.last_mut()
        {
            last.bullets.push(rest.to_string());
        }
        // Other lines (top-level header, blank lines) are ignored.
    }
    out
}

/// Filter `sections` to those strictly newer than `last_seen` by full
/// `(major, minor, patch)`. Sections with unparseable versions are skipped.
pub fn sections_since(sections: &[Section], last_seen: &str) -> Vec<Section> {
    let last = parse_full_version(last_seen);
    sections
        .iter()
        .filter(|s| match (parse_full_version(&s.version), last) {
            (Some(sv), Some(lv)) => sv > lv,
            _ => false,
        })
        .cloned()
        .collect()
}

/// Convenience: parse the embedded changelog and filter to sections newer
/// than `last_seen`. Returns an empty vec if `last_seen` doesn't parse.
pub fn relevant_sections(last_seen: &str) -> Vec<Section> {
    sections_since(&parse_changelog(CHANGELOG_SOURCE), last_seen)
}

/// Render the changelog overlay as a centered floating panel.
pub fn render(area: Rect, buf: &mut TermBuffer, scroll: usize, sections: &[Section]) {
    if area.width < 24 || area.height < 8 {
        return;
    }

    let bg = Color::Rgb(22, 28, 44);
    let border = Color::Rgb(80, 130, 170);
    let border_style = Style::default().bg(bg).fg(border);
    let title_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(220, 230, 255))
        .add_modifier(Modifier::BOLD);
    let version_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(140, 200, 255))
        .add_modifier(Modifier::BOLD);
    let bullet_style = Style::default().bg(bg).fg(Color::Rgb(210, 215, 230));

    const INNER_W: u16 = 70;
    const OVERLAY_W: u16 = INNER_W + 2;

    let overlay_w = OVERLAY_W.min(area.width);
    let overlay_h = area.height.saturating_sub(2).max(8).min(area.height);

    let ox = area.x + area.width.saturating_sub(overlay_w) / 2;
    let oy = area.y + area.height.saturating_sub(overlay_h) / 2;
    let overlay = Rect::new(ox, oy, overlay_w, overlay_h);

    // Background.
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }
    super::overlay_chrome::draw_border(buf, overlay, border_style);

    // Title centered on top border.
    let tx = overlay.x + overlay.width.saturating_sub(TITLE.len() as u16) / 2;
    buf.set_string(tx, overlay.y, TITLE, title_style);

    // Separator.
    let sep_y = overlay.y + 2;
    for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
        buf.set_string(x, sep_y, "\u{2500}", border_style);
    }

    // Footer.
    let fy = overlay.y + overlay.height.saturating_sub(1);
    let fx = overlay.x + overlay.width.saturating_sub(FOOTER.len() as u16) / 2;
    buf.set_string(fx, fy, FOOTER, border_style);

    // Build display lines. Each version becomes a header followed by indented
    // wrapped bullets and a trailing blank line for separation.
    let content_x = overlay.x + 2;
    let content_w = overlay.width.saturating_sub(4) as usize;
    let content_start_y = overlay.y + 3;
    let content_end_y = overlay.y + overlay.height.saturating_sub(1);

    let lines = build_lines(sections, content_w);
    let visible_rows = content_end_y.saturating_sub(content_start_y) as usize;
    let max_scroll = lines.len().saturating_sub(visible_rows);
    let scroll = scroll.min(max_scroll);

    for (row_idx, (kind, text)) in lines.iter().skip(scroll).enumerate() {
        let cy = content_start_y + row_idx as u16;
        if cy >= content_end_y {
            break;
        }
        let style = match *kind {
            'v' => version_style,
            _ => bullet_style,
        };
        buf.set_string(content_x, cy, text, style);
    }

    if scroll > 0 {
        let ind_x = overlay.x + overlay.width.saturating_sub(5);
        buf.set_string(ind_x, overlay.y, " \u{2191} ", border_style);
    }
    let entries_shown = visible_rows.min(lines.len().saturating_sub(scroll));
    if scroll + entries_shown < lines.len() {
        let ind_x = overlay.x + overlay.width.saturating_sub(5);
        buf.set_string(ind_x, fy, " \u{2193} ", border_style);
    }
}

/// Lay out the sections into renderable lines. Each line is `(kind, text)`
/// where kind is `'v'` for a version header and `'b'` for a (possibly wrapped)
/// bullet line.
fn build_lines(sections: &[Section], width: usize) -> Vec<(char, String)> {
    let mut out: Vec<(char, String)> = Vec::new();
    for (i, s) in sections.iter().enumerate() {
        if i > 0 {
            out.push(('b', String::new())); // blank line between versions
        }
        out.push(('v', s.version.clone()));
        for bullet in &s.bullets {
            let prefix = "  • ";
            let cont = "    ";
            let wrapped = wrap_with_indent(bullet, width, prefix.len());
            for (j, chunk) in wrapped.iter().enumerate() {
                let ind = if j == 0 { prefix } else { cont };
                out.push(('b', format!("{ind}{chunk}")));
            }
        }
    }
    out
}

/// Word-wrap `text` so each chunk plus an `indent_w`-character indent fits
/// inside `width` columns. Falls back to a single chunk if `width` is too
/// small to make progress.
fn wrap_with_indent(text: &str, width: usize, indent_w: usize) -> Vec<String> {
    let max = width.saturating_sub(indent_w).max(8);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
            continue;
        }
        if current.chars().count() + 1 + word.chars().count() <= max {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_extracts_sections_and_bullets() {
        let txt = "# Changelog\n\n## v0.3.0\n\n- one\n- two\n\n## v0.2.2\n\n- three\n";
        let s = parse_changelog(txt);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].version, "v0.3.0");
        assert_eq!(s[0].bullets, vec!["one", "two"]);
        assert_eq!(s[1].version, "v0.2.2");
        assert_eq!(s[1].bullets, vec!["three"]);
    }

    #[test]
    fn embedded_changelog_parses() {
        let s = parse_changelog(CHANGELOG_SOURCE);
        assert!(!s.is_empty(), "embedded changelog should produce sections");
        // First section should match the current crate version we ship.
        assert!(s[0].version.starts_with('v'));
    }

    #[test]
    fn sections_since_includes_all_newer_versions_including_patches() {
        let txt = "## v0.3.0\n- a\n## v0.2.2\n- b\n## v0.2.1\n- c\n";
        let s = parse_changelog(txt);

        // From 0.2.0: every newer section, including patch bumps under the
        // same minor (0.2.1, 0.2.2) plus the next minor (0.3.0).
        let after = sections_since(&s, "0.2.0");
        let versions: Vec<_> = after.iter().map(|s| s.version.clone()).collect();
        assert_eq!(versions, vec!["v0.3.0", "v0.2.2", "v0.2.1"]);

        // From 0.2.1: skip 0.2.1 itself, include 0.2.2 and 0.3.0.
        let after = sections_since(&s, "0.2.1");
        let versions: Vec<_> = after.iter().map(|s| s.version.clone()).collect();
        assert_eq!(versions, vec!["v0.3.0", "v0.2.2"]);

        // From the latest: nothing newer.
        let none = sections_since(&s, "0.3.0");
        assert!(none.is_empty());
    }

    #[test]
    fn wrap_with_indent_respects_width() {
        let chunks = wrap_with_indent("one two three four five six", 16, 4);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 12, "chunk too long: {chunk:?}");
        }
    }

    #[test]
    fn render_does_not_panic() {
        let sections = vec![Section {
            version: "v0.3.0".to_string(),
            bullets: vec!["something happened".to_string()],
        }];
        let area = Rect::new(0, 0, 100, 20);
        let mut buf = TermBuffer::empty(area);
        render(area, &mut buf, 0, &sections);
    }

    #[test]
    fn render_skips_tiny_area() {
        let sections = vec![];
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = TermBuffer::empty(area);
        render(area, &mut buf, 0, &sections);
    }
}
