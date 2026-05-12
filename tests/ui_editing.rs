#![cfg(feature = "ui-tests")]

mod ui_common;

use ui_common::{Fixture, Key};

#[test]
fn edit_insert_character_shows_modified() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "hi\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('x'));
    s.wait_for_status_contains("[+]");
    s.shutdown();
}

#[test]
fn edit_backspace_removes_char() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "abc\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_keys(&[Key::End, Key::Backspace]);
    s.wait_until(
        |sc| sc.contents().contains("ab") && !contains_word(&sc.contents(), "abc"),
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn edit_ctrl_backspace_removes_word() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "foo bar\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_keys(&[Key::End, Key::CtrlBackspace]);
    // "bar" deleted; cursor lands at column 5 (after "foo ").
    s.wait_for_status_contains("1:5");
    s.shutdown();
}

#[test]
fn edit_delete_removes_forward() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "abc\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Delete);
    s.wait_until(
        |sc| {
            let body = sc.contents();
            body.contains("bc") && !body.lines().any(|l| l.trim_end() == "abc")
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn edit_undo_restores_buffer() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "hi\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('X'));
    s.wait_for_status_contains("[+]");
    s.send_key(Key::Ctrl('z'));
    // After undo, modified indicator clears.
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            !last.contains("[+]")
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn edit_redo_reapplies_change() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "hi\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('X'));
    s.send_key(Key::Ctrl('z'));
    s.send_key(Key::Ctrl('y'));
    s.wait_for_status_contains("[+]");
    s.shutdown();
}

#[test]
fn edit_tab_inserts_indent() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "x\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Tab);
    // Tab size defaults to 4 spaces; cursor advances to column 5.
    s.wait_for_status_contains("1:5");
    s.shutdown();
}

#[test]
fn edit_ctrl_slash_toggles_line_comment() {
    let fx = Fixture::new();
    let path = fx.write_file("a.rs", "let x = 1;\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('/'));
    s.wait_until(
        |sc| sc.contents().contains("// let"),
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

fn contains_word(haystack: &str, word: &str) -> bool {
    haystack
        .lines()
        .any(|l| l.split(|c: char| !c.is_alphanumeric()).any(|w| w == word))
}

// ── Tier 3 — 3.6 trailing-whitespace + mixed indent ─────────────────────

#[test]
fn mixed_indent_segment_appears_in_status_bar() {
    // File mixes tab- and space-leading lines → status bar must contain the
    // "mixed-indent" segment.
    let fx = Fixture::new();
    let path = fx.write_file("mix.txt", "\tfoo\n  bar\n");
    let s = fx.open(&path);
    s.wait_for_status_contains("mixed-indent");
    s.shutdown();
}

#[test]
fn mixed_indent_absent_for_pure_spaces() {
    let fx = Fixture::new();
    let path = fx.write_file("clean.txt", "  foo\n  bar\n");
    let s = fx.open(&path);
    s.wait_for_status_contains("clean.txt");
    let screen = s.screen();
    let (rows, cols) = screen.size();
    let last = screen.contents_between(rows - 1, 0, rows - 1, cols);
    assert!(
        !last.contains("mixed-indent"),
        "status bar should not flag pure-space file, got: {last:?}"
    );
    s.shutdown();
}

// ── Tier 3 — 3.2 persistent undo ─────────────────────────────────────────

#[test]
fn persistent_undo_survives_relaunch() {
    use ui_common::{SessionOptions, TxtSession};
    let fx = Fixture::new();
    // Enable persistent undo via the seeded config.
    fx.append_config("persistent_undo = true\n");
    let path = fx.write_file("pu.txt", "hello\n");

    // Edit, save, quit.
    let mut s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("pu.txt");
    s1.send_keys(&[Key::End, Key::Char('!'), Key::Ctrl('s')]);
    s1.wait_until(
        |sc| sc.contents().contains("hello!"),
        std::time::Duration::from_secs(5),
    );
    s1.shutdown();

    // Relaunch the same file → press Ctrl+Z → should restore "hello".
    let mut s2 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s2.wait_for_first_paint();
    s2.wait_for_status_contains("pu.txt");
    s2.send_key(Key::Ctrl('z'));
    s2.wait_until(
        |sc| {
            let body = sc.contents();
            body.contains("hello") && !body.contains("hello!")
        },
        std::time::Duration::from_secs(5),
    );
    s2.shutdown();
}

// ── Tier 3 — 3.4 auto-pair + surround ────────────────────────────────────

#[test]
fn auto_pair_inserts_matching_close_paren() {
    let fx = Fixture::new();
    let path = fx.write_file("ap.txt", "\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('('));
    // After "(", cursor should still be at col 2 (sitting inside the pair),
    // but the buffer should contain "()" on line 1.
    s.wait_until(
        |sc| sc.contents().contains("()"),
        std::time::Duration::from_secs(5),
    );
    // Status bar should show 1:2 (inside the pair) rather than 1:3.
    s.assert_status_contains("1:2");
    s.shutdown();
}

#[test]
fn auto_pair_skip_over_existing_close() {
    let fx = Fixture::new();
    let path = fx.write_file("skip.txt", "\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    // Type "(" → auto-pair creates "(|)", then type ")" — should just move past it.
    s.send_keys(&[Key::Char('('), Key::Char(')')]);
    s.wait_until(
        |sc| sc.contents().contains("()"),
        std::time::Duration::from_secs(5),
    );
    // Cursor now should be just after the close (col 3).
    s.assert_status_contains("1:3");
    s.shutdown();
}

#[test]
fn surround_wraps_selection_in_quotes() {
    let fx = Fixture::new();
    let path = fx.write_file("sur.txt", "hello\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    // Select "hello" and trigger surround → "
    s.send_keys(&[
        Key::Home,
        Key::ShiftArrow(ui_common::Arrow::End),
        Key::Alt('\''),
        Key::Char('"'),
    ]);
    s.wait_until(
        |sc| sc.contents().contains("\"hello\""),
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn trailing_whitespace_highlight_renders() {
    // Trailing spaces on a line other than the cursor's must show with the
    // configured background colour. The vt100 parser exposes per-cell bg
    // colour so we check the colour of a trailing-space cell.
    use ratatui::style::Color;
    let fx = Fixture::new();
    let path = fx.write_file("tws.txt", "first   \nsecond\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("tws.txt");
    // Move cursor to line 2 so line 1's trailing whitespace is highlighted.
    s.send_key(Key::Down);
    s.wait_for_status_contains("2:1");
    let screen = s.screen();
    // Inspect cell at row 0 (file content row), col after "first" (5..8).
    // The actual on-screen column depends on the gutter width; we look for a
    // non-default bg colour somewhere on row 0 between cols 5 and 30.
    let (_rows, cols) = screen.size();
    let row0 = 0;
    let mut found_red_bg = false;
    for c in 0..cols {
        if let Some(cell) = screen.cell(row0, c)
            && let vt100::Color::Rgb(r, _g, _b) = cell.bgcolor()
            && r > 80
            && r < 160
        {
            // Our trailing-ws style uses Rgb(110, 30, 30) — accept that range.
            found_red_bg = true;
            break;
        }
        // suppress unused-import warning in non-matching builds
        let _ = Color::Reset;
    }
    assert!(
        found_red_bg,
        "expected a red-bg trailing-ws cell on row 0, screen:\n{}",
        screen.contents()
    );
    s.shutdown();
}
