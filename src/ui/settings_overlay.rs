use ratatui::{
    buffer::Buffer as TermBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::app::AppState;
use crate::config::Config;
use crate::ui::overlay_chrome::{draw_border, draw_h_separator, render_centered_header};

// ── Layout constants ──────────────────────────────────────────────────────────

const OVERLAY_W: u16 = 50;
// Rows: top border + header + separator + N settings + separator + hint + bottom border
pub(crate) const NUM_SETTINGS: usize = 12;
const OVERLAY_H: u16 = 3 + NUM_SETTINGS as u16 + 3;

/// Render the settings overlay centered in `area`.
pub fn render(state: &AppState, area: Rect, buf: &mut TermBuffer) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    // ── Styles ────────────────────────────────────────────────────────────────
    let bg = Color::Rgb(18, 22, 40);
    let border_style = Style::default().bg(bg).fg(Color::Rgb(80, 100, 160));
    let header_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(200, 200, 255))
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().bg(bg).fg(Color::Rgb(200, 200, 220));
    let selected_bg = Color::Rgb(40, 55, 110);
    let dim_style = Style::default().bg(bg).fg(Color::Rgb(100, 100, 120));
    let on_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(120, 220, 120))
        .add_modifier(Modifier::BOLD);
    let theme_style = Style::default()
        .bg(bg)
        .fg(Color::Rgb(255, 200, 80))
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().bg(bg).fg(Color::Rgb(100, 110, 150));
    let path_style = Style::default().bg(bg).fg(Color::Rgb(110, 115, 145));

    // ── Dimensions ────────────────────────────────────────────────────────────
    let ow = OVERLAY_W.min(area.width);
    let oh = OVERLAY_H.min(area.height);
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    // ── Background fill ───────────────────────────────────────────────────────
    for y in overlay.y..overlay.y + overlay.height {
        for x in overlay.x..overlay.x + overlay.width {
            buf.set_string(x, y, " ", Style::default().bg(bg));
        }
    }

    // ── Border ────────────────────────────────────────────────────────────────
    draw_border(buf, overlay, border_style);

    // ── Header ────────────────────────────────────────────────────────────────
    render_centered_header(buf, overlay, overlay.y, " Settings ", header_style);

    // ── Path subtitle (row 1, y+1) ────────────────────────────────────────────
    let inner_w_u16 = overlay.width.saturating_sub(4);
    let path_str = Config::config_path()
        .map(|p| {
            // Shorten home directory to ~
            if let Some(home) = dirs::home_dir()
                && let Ok(rel) = p.strip_prefix(&home)
            {
                return format!("~/{}", rel.display());
            }
            p.display().to_string()
        })
        .unwrap_or_else(|| "(unavailable)".to_string());
    let truncated_path: String = path_str.chars().take(inner_w_u16 as usize).collect();
    let px = overlay.x
        + overlay
            .width
            .saturating_sub(truncated_path.len() as u16 + 4)
            / 2
        + 2;
    buf.set_string(px, overlay.y + 1, &truncated_path, path_style);

    // Separator after header (row 2, y+2)
    draw_h_separator(buf, overlay, overlay.y + 2, border_style);

    // ── Setting rows (y+3 .. y+3+NUM_SETTINGS) ───────────────────────────────
    let inner_w = overlay.width.saturating_sub(2) as usize;
    let label_w: usize = 30;

    let settings: [(&str, SettingValue); NUM_SETTINGS] = [
        (
            "Confirm exit",
            SettingValue::Bool(state.config.confirm_exit),
        ),
        ("Auto save", SettingValue::Bool(state.config.auto_save)),
        (
            "Show whitespace",
            SettingValue::Bool(state.config.show_whitespace),
        ),
        (
            "Highlight trailing whitespace",
            SettingValue::Bool(state.config.highlight_trailing_whitespace),
        ),
        (
            "Warn on mixed indent",
            SettingValue::Bool(state.config.warn_mixed_indent),
        ),
        (
            "Auto-pair brackets",
            SettingValue::Bool(state.config.auto_pair),
        ),
        (
            "Hide .git folder",
            SettingValue::Bool(state.config.hide_git_folder),
        ),
        (
            "Hide dot folders",
            SettingValue::Bool(state.config.hide_dot_folders),
        ),
        (
            "Restore session",
            SettingValue::Bool(state.config.restore_session),
        ),
        (
            "Persistent undo",
            SettingValue::Bool(state.config.persistent_undo),
        ),
        (
            "Color theme",
            SettingValue::Enum(state.config.theme.display_name()),
        ),
        (
            "Keybinding preset",
            SettingValue::Enum(state.config.keymap_preset.display_name()),
        ),
    ];

    for (i, (label, value)) in settings.iter().enumerate() {
        let row_y = overlay.y + 3 + i as u16;
        let selected = state.settings_cursor == i;
        let row_bg = if selected { selected_bg } else { bg };

        // Fill row background
        for x in overlay.x + 1..overlay.x + overlay.width.saturating_sub(1) {
            buf.set_string(x, row_y, " ", Style::default().bg(row_bg));
        }

        let cx = overlay.x + 2;
        let lbl_style = Style::default().bg(row_bg).fg(if selected {
            Color::White
        } else {
            Color::Rgb(200, 200, 220)
        });

        // Label
        let padded = format!("{:<width$}", label, width = label_w);
        let display: String = padded.chars().take(inner_w.saturating_sub(8)).collect();
        buf.set_string(cx, row_y, &display, lbl_style);

        // Value — right-aligned within the row
        let value_str = match value {
            SettingValue::Bool(true) => "[ON] ".to_string(),
            SettingValue::Bool(false) => "[OFF]".to_string(),
            SettingValue::Enum(name) => format!("‹ {} ›", name),
        };
        let value_x = (overlay.x + overlay.width.saturating_sub(1))
            .saturating_sub(value_str.len() as u16 + 1);
        let val_style = match value {
            SettingValue::Bool(true) => on_style.bg(row_bg),
            SettingValue::Bool(false) => dim_style.bg(row_bg),
            SettingValue::Enum(_) => theme_style.bg(row_bg),
        };
        if value_x > cx {
            buf.set_string(value_x, row_y, &value_str, val_style);
        }

        _ = label_style; // suppress unused warning
    }

    // Separator before hint (y + 3 + NUM_SETTINGS)
    let sep_y = overlay.y + 3 + NUM_SETTINGS as u16;
    draw_h_separator(buf, overlay, sep_y, border_style);

    // ── Hint row ──────────────────────────────────────────────────────────────
    let hint = "Space/Enter: toggle  ·  ←/→: cycle  ·  Esc: close";
    let hint_y = sep_y + 1;
    let hint_x = overlay.x + overlay.width.saturating_sub(hint.len() as u16 + 2) / 2 + 1;
    let truncated: String = hint.chars().take(inner_w).collect();
    buf.set_string(hint_x, hint_y, &truncated, hint_style);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

enum SettingValue<'a> {
    Bool(bool),
    Enum(&'a str),
}
