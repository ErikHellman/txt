use ratatui::style::Color;

use crate::config::Theme;

/// All configurable colors for the editor UI and syntax highlighting.
///
/// Computed once per frame from `Config::theme` and threaded into sub-renderers.
/// Non-themed UI chrome (overlay borders, mode badges, git gutter marks) keeps
/// its hardcoded colors.
pub struct ThemeColors {
    // ── Syntax highlights ─────────────────────────────────────────────────────
    pub syn_keyword: Color,
    pub syn_string: Color,
    pub syn_comment: Color,
    pub syn_number: Color,
    pub syn_type: Color,
    pub syn_function: Color,
    pub syn_attribute: Color,
    pub syn_punctuation: Color,
    pub syn_heading: Color,
    pub syn_link: Color,
    pub syn_emphasis: Color,
    pub syn_codeblock: Color,
    // ── Editor text area ──────────────────────────────────────────────────────
    pub text: Color,
    pub selection_bg: Color,
    pub line_num_cur: Color,
    // ── Status bar ────────────────────────────────────────────────────────────
    pub statusbar_bg: Color,
    pub statusbar_modified_fg: Color,
    // ── Sidebar ───────────────────────────────────────────────────────────────
    pub sidebar_bg: Color,
    pub sidebar_fg: Color,
    pub sidebar_dir_fg: Color,
    pub sidebar_sel_bg: Color,
    // ── Fuzzy picker / command palette ────────────────────────────────────────
    pub picker_bg: Color,
    pub picker_sel_bg: Color,
}

impl ThemeColors {
    pub fn for_theme(theme: &Theme) -> Self {
        match theme {
            Theme::Default => Self::default_theme(),
            Theme::Monokai => Self::monokai(),
            Theme::Gruvbox => Self::gruvbox(),
            Theme::Nord => Self::nord(),
        }
    }

    // ── Default (VS Code Dark+) ───────────────────────────────────────────────

    fn default_theme() -> Self {
        Self {
            syn_keyword: Color::Rgb(197, 134, 192),
            syn_string: Color::Rgb(206, 145, 120),
            syn_comment: Color::Rgb(106, 153, 85),
            syn_number: Color::Rgb(181, 206, 168),
            syn_type: Color::Rgb(78, 201, 176),
            syn_function: Color::Rgb(220, 220, 170),
            syn_attribute: Color::Rgb(156, 220, 254),
            syn_punctuation: Color::DarkGray,
            syn_heading: Color::Cyan,
            syn_link: Color::Rgb(78, 201, 176),
            syn_emphasis: Color::White,
            syn_codeblock: Color::DarkGray,
            text: Color::White,
            selection_bg: Color::Rgb(60, 80, 120),
            line_num_cur: Color::Yellow,
            statusbar_bg: Color::Rgb(40, 40, 60),
            statusbar_modified_fg: Color::Rgb(255, 150, 50),
            sidebar_bg: Color::Rgb(20, 20, 35),
            sidebar_fg: Color::Rgb(200, 200, 200),
            sidebar_dir_fg: Color::Rgb(130, 170, 230),
            sidebar_sel_bg: Color::Rgb(60, 60, 100),
            picker_bg: Color::Rgb(25, 25, 40),
            picker_sel_bg: Color::Rgb(60, 80, 140),
        }
    }

    // ── Monokai Classic ───────────────────────────────────────────────────────

    fn monokai() -> Self {
        Self {
            syn_keyword: Color::Rgb(249, 38, 114),      // #f92672 hot pink
            syn_string: Color::Rgb(230, 219, 116),      // #e6db74 yellow
            syn_comment: Color::Rgb(117, 113, 94),      // #75715e gray-brown
            syn_number: Color::Rgb(174, 129, 255),      // #ae81ff purple
            syn_type: Color::Rgb(166, 226, 46),         // #a6e22e bright green
            syn_function: Color::Rgb(166, 226, 46),     // #a6e22e bright green
            syn_attribute: Color::Rgb(249, 38, 114),    // #f92672 pink
            syn_punctuation: Color::Rgb(248, 248, 242), // #f8f8f2 near-white
            syn_heading: Color::Rgb(102, 217, 239),     // #66d9ef cyan
            syn_link: Color::Rgb(102, 217, 239),        // #66d9ef cyan
            syn_emphasis: Color::Rgb(248, 248, 242),    // #f8f8f2 near-white
            syn_codeblock: Color::Rgb(117, 113, 94),    // #75715e gray-brown
            text: Color::Rgb(248, 248, 242),            // #f8f8f2
            selection_bg: Color::Rgb(73, 72, 62),       // #49483e
            line_num_cur: Color::Rgb(230, 219, 116),    // yellow
            statusbar_bg: Color::Rgb(39, 40, 34),       // #272822
            statusbar_modified_fg: Color::Rgb(249, 38, 114), // pink
            sidebar_bg: Color::Rgb(39, 40, 34),         // #272822
            sidebar_fg: Color::Rgb(248, 248, 242),      // #f8f8f2
            sidebar_dir_fg: Color::Rgb(102, 217, 239),  // #66d9ef cyan
            sidebar_sel_bg: Color::Rgb(73, 72, 62),     // #49483e
            picker_bg: Color::Rgb(39, 40, 34),          // #272822
            picker_sel_bg: Color::Rgb(73, 72, 62),      // #49483e
        }
    }

    // ── Gruvbox Dark ──────────────────────────────────────────────────────────

    fn gruvbox() -> Self {
        Self {
            syn_keyword: Color::Rgb(251, 73, 52),           // #fb4934 red
            syn_string: Color::Rgb(184, 187, 38),           // #b8bb26 yellow-green
            syn_comment: Color::Rgb(146, 131, 116),         // #928374 gray
            syn_number: Color::Rgb(211, 134, 155),          // #d3869b pink
            syn_type: Color::Rgb(142, 192, 124),            // #8ec07c green
            syn_function: Color::Rgb(250, 189, 47),         // #fabd2f yellow
            syn_attribute: Color::Rgb(131, 165, 152),       // #83a598 teal
            syn_punctuation: Color::Rgb(168, 153, 132),     // #a89984 warm gray
            syn_heading: Color::Rgb(131, 165, 152),         // #83a598 teal
            syn_link: Color::Rgb(131, 165, 152),            // #83a598 teal
            syn_emphasis: Color::Rgb(213, 196, 161),        // #d5c4a1 cream
            syn_codeblock: Color::Rgb(168, 153, 132),       // #a89984 warm gray
            text: Color::Rgb(235, 219, 178),                // #ebdbb2
            selection_bg: Color::Rgb(80, 73, 69),           // dark warm
            line_num_cur: Color::Rgb(250, 189, 47),         // yellow
            statusbar_bg: Color::Rgb(50, 48, 47),           // #32302f
            statusbar_modified_fg: Color::Rgb(251, 73, 52), // red
            sidebar_bg: Color::Rgb(29, 32, 33),             // #1d2021
            sidebar_fg: Color::Rgb(213, 196, 161),          // #d5c4a1
            sidebar_dir_fg: Color::Rgb(131, 165, 152),      // teal
            sidebar_sel_bg: Color::Rgb(80, 73, 69),         // dark warm
            picker_bg: Color::Rgb(29, 32, 33),              // #1d2021
            picker_sel_bg: Color::Rgb(80, 73, 69),          // dark warm
        }
    }

    // ── Nord ──────────────────────────────────────────────────────────────────

    fn nord() -> Self {
        Self {
            syn_keyword: Color::Rgb(129, 161, 193),   // #81a1c1 nord9 blue
            syn_string: Color::Rgb(163, 190, 140),    // #a3be8c nord14 green
            syn_comment: Color::Rgb(76, 86, 106),     // #4c566a nord3 dark
            syn_number: Color::Rgb(180, 142, 173),    // #b48ead nord15 purple
            syn_type: Color::Rgb(136, 192, 208),      // #88c0d0 nord8 light blue
            syn_function: Color::Rgb(143, 188, 187),  // #8fbcbb nord7 teal
            syn_attribute: Color::Rgb(235, 203, 139), // #ebcb8b nord13 yellow
            syn_punctuation: Color::Rgb(76, 86, 106), // nord3 dark
            syn_heading: Color::Rgb(136, 192, 208),   // #88c0d0 nord8 light blue
            syn_link: Color::Rgb(136, 192, 208),      // #88c0d0 nord8 light blue
            syn_emphasis: Color::Rgb(216, 222, 233),  // #d8dee9 nord4
            syn_codeblock: Color::Rgb(76, 86, 106),   // nord3 dark
            text: Color::Rgb(216, 222, 233),          // #d8dee9 nord4
            selection_bg: Color::Rgb(67, 76, 94),     // #434c5e nord2
            line_num_cur: Color::Rgb(129, 161, 193),  // nord9 blue
            statusbar_bg: Color::Rgb(46, 52, 64),     // #2e3440 nord0
            statusbar_modified_fg: Color::Rgb(235, 203, 139), // yellow
            sidebar_bg: Color::Rgb(36, 41, 51),
            sidebar_fg: Color::Rgb(216, 222, 233),     // nord4
            sidebar_dir_fg: Color::Rgb(136, 192, 208), // nord8
            sidebar_sel_bg: Color::Rgb(67, 76, 94),    // nord2
            picker_bg: Color::Rgb(46, 52, 64),         // nord0
            picker_sel_bg: Color::Rgb(67, 76, 94),     // nord2
        }
    }
}
