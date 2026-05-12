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
