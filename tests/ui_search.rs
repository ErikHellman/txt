#![cfg(feature = "ui-tests")]

mod ui_common;

use std::time::Duration;

use ui_common::{Fixture, Key};

#[test]
fn search_ctrl_f_opens_bar() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "alpha\nbeta\ngamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('f'));
    // Search bar renders the "Find:" prompt.
    s.wait_until(|sc| sc.contents().contains("Find:"), Duration::from_secs(5));
    s.shutdown();
}

#[test]
fn search_typed_query_jumps_cursor_to_match() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "alpha\nbeta\nneedle\ngamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('f'));
    s.wait_until(|sc| sc.contents().contains("Find:"), Duration::from_secs(5));
    s.send_text("needle");
    // Cursor now sits on line 3 (the needle).
    s.wait_for_status_contains("3:");
    s.shutdown();
}

#[test]
fn search_f3_advances_to_next_match() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "needle\nbeta\nneedle\ngamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('f'));
    s.wait_until(|sc| sc.contents().contains("Find:"), Duration::from_secs(5));
    s.send_text("needle");
    s.wait_for_status_contains("1:");
    s.send_key(Key::F(3));
    s.wait_for_status_contains("3:");
    s.shutdown();
}

#[test]
fn search_f3_wraps_after_last_match() {
    // Shift+F3 (SearchPrev) is unreachable through crossterm 0.29's legacy
    // parser because `\x1b[1;2R` is dispatched as a cursor-position report
    // instead of a key event.  Verify the next/wrap path instead.
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "needle\nbeta\nneedle\ngamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('f'));
    s.wait_until(|sc| sc.contents().contains("Find:"), Duration::from_secs(5));
    s.send_text("needle");
    s.send_key(Key::F(3)); // 1 → 3
    s.wait_for_status_contains("3:");
    s.send_key(Key::F(3)); // 3 → wrap back to 1
    s.wait_for_status_contains("1:");
    s.shutdown();
}

#[test]
fn search_esc_closes_bar() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "alpha\nbeta\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('f'));
    s.wait_until(|sc| sc.contents().contains("Find:"), Duration::from_secs(5));
    s.send_key(Key::Esc);
    s.wait_until(
        |sc| !sc.contents().contains("Find:"),
        Duration::from_secs(5),
    );
    s.shutdown();
}
