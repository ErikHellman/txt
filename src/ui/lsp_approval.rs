use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::{LspApprovalReason, PendingLspApproval};
use crate::ui::overlay_chrome::{draw_border, draw_h_separator};

const OVERLAY_W: u16 = 72;

/// Render the LSP-binary approval overlay centered in `area`.
pub fn render(pending: &PendingLspApproval, area: Rect, buf: &mut TermBuffer) {
    if area.width < 20 || area.height < 8 {
        return;
    }

    let lines = build_lines(pending, OVERLAY_W as usize);
    // Rows: top border + N content rows + separator + hint + bottom border
    let oh = (lines.len() as u16 + 4).min(area.height);
    let ow = OVERLAY_W.min(area.width);
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    let bg = Color::Rgb(40, 18, 18);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(200, 90, 90));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(255, 220, 220))
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().bg(bg).fg(Color::Rgb(220, 220, 220));
    let label_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(255, 200, 120))
        .add_modifier(Modifier::BOLD);
    let warning_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(255, 130, 130))
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(180, 180, 180));

    // Background fill
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    draw_border(buf, overlay, border_style);

    let header = " Approve LSP server ";
    let hx = overlay.x + overlay.width.saturating_sub(header.len() as u16) / 2;
    buf.set_string(hx, overlay.y, header, header_style);

    // Body rows.
    let inner_w = overlay.width.saturating_sub(4) as usize;
    let body_x = overlay.x + 2;
    for (i, line) in lines.iter().enumerate() {
        let y = overlay.y + 1 + i as u16;
        if y >= overlay.y + overlay.height - 2 {
            break;
        }
        let style = match line.kind {
            LineKind::Label => label_style,
            LineKind::Body => body_style,
            LineKind::Warning => warning_style,
        };
        let truncated: String = line.text.chars().take(inner_w).collect();
        buf.set_string(body_x, y, &truncated, style);
    }

    // Separator above hint.
    let sep_y = overlay.y + overlay.height - 2;
    draw_h_separator(buf, overlay, sep_y, border_style);

    let hint = "[y] approve   [n] reject";
    let hint_y = overlay.y + overlay.height - 1;
    let hint_x = overlay.x + overlay.width.saturating_sub(hint.len() as u16) / 2;
    buf.set_string(hint_x, hint_y, hint, hint_style);
}

#[derive(Clone, Copy)]
enum LineKind {
    Label,
    Body,
    Warning,
}

struct Line {
    kind: LineKind,
    text: String,
}

fn build_lines(p: &PendingLspApproval, max_inner: usize) -> Vec<Line> {
    let mut out = Vec::new();

    let reason_text = match &p.reason {
        LspApprovalReason::FirstLaunch => "first launch".to_string(),
        LspApprovalReason::BinaryChanged { .. } => "binary changed since last approval".to_string(),
    };
    out.push(Line {
        kind: LineKind::Body,
        text: format!("{} ({})", p.server_name, reason_text),
    });
    out.push(Line {
        kind: LineKind::Body,
        text: String::new(),
    });

    out.push(Line {
        kind: LineKind::Label,
        text: "Command:".into(),
    });
    let cmd_line = if p.args.is_empty() {
        p.command.clone()
    } else {
        format!("{} {}", p.command, p.args.join(" "))
    };
    out.push(Line {
        kind: LineKind::Body,
        text: truncate_middle(&cmd_line, max_inner.saturating_sub(2)),
    });

    out.push(Line {
        kind: LineKind::Label,
        text: "Path:".into(),
    });
    out.push(Line {
        kind: LineKind::Body,
        text: truncate_middle(
            &p.display_path.display().to_string(),
            max_inner.saturating_sub(2),
        ),
    });
    if p.canonical_path != p.display_path {
        out.push(Line {
            kind: LineKind::Label,
            text: "  → real:".into(),
        });
        out.push(Line {
            kind: LineKind::Body,
            text: truncate_middle(
                &p.canonical_path.display().to_string(),
                max_inner.saturating_sub(2),
            ),
        });
    }

    out.push(Line {
        kind: LineKind::Label,
        text: "SHA-256:".into(),
    });
    out.push(Line {
        kind: LineKind::Body,
        text: format!("{}…", short_hash(&p.hash)),
    });

    if let LspApprovalReason::BinaryChanged { previous_hash } = &p.reason {
        out.push(Line {
            kind: LineKind::Body,
            text: String::new(),
        });
        out.push(Line {
            kind: LineKind::Warning,
            text: format!("Previously approved hash: {}…", short_hash(previous_hash)),
        });
    }

    out
}

fn short_hash(h: &str) -> String {
    h.chars().take(16).collect()
}

fn truncate_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max || max < 4 {
        return s.to_string();
    }
    let keep = max - 1; // leave room for "…"
    let head = keep / 2;
    let tail = keep - head;
    let chars: Vec<char> = s.chars().collect();
    let head_str: String = chars.iter().take(head).collect();
    let tail_str: String = chars
        .iter()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head_str}…{tail_str}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_middle_keeps_short_strings() {
        assert_eq!(truncate_middle("hello", 10), "hello");
        assert_eq!(truncate_middle("hi", 5), "hi");
    }

    #[test]
    fn truncate_middle_shortens_long_paths() {
        let s = "/very/long/path/to/some/binary/that/is/too/long";
        let t = truncate_middle(s, 20);
        assert!(t.chars().count() <= 20);
        assert!(t.contains('…'));
    }

    #[test]
    fn short_hash_takes_16() {
        let h = "abcd1234ef567890fedcba9876543210";
        assert_eq!(short_hash(h), "abcd1234ef567890");
    }
}
