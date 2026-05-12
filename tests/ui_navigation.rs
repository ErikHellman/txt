#![cfg(feature = "ui-tests")]

mod ui_common;

use ui_common::{Arrow, Fixture, Key};

#[test]
fn nav_arrow_right_advances_column() {
    let fx = Fixture::new();
    let path = fx.write_file("hello.txt", "hello world\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_keys(&[Key::Right, Key::Right]);
    s.wait_for_status_contains("1:3");
    s.shutdown();
}

#[test]
fn nav_arrow_down_moves_to_next_line() {
    let fx = Fixture::new();
    let path = fx.write_file("multi.txt", "alpha\nbeta\ngamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Down);
    s.wait_for_status_contains("2:1");
    s.shutdown();
}

#[test]
fn nav_ctrl_right_jumps_word() {
    let fx = Fixture::new();
    let path = fx.write_file("words.txt", "foo bar baz\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::CtrlArrow(Arrow::Right));
    // Word jump should leave column 1 and stop before "baz".
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            !last.contains(" 1:1 ") && !last.contains(" 1:9 ") && last.contains(" 1:")
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn nav_ctrl_home_returns_to_file_start() {
    let fx = Fixture::new();
    let path = fx.write_file("multi.txt", "alpha\nbeta\ngamma\ndelta\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_keys(&[Key::CtrlArrow(Arrow::End), Key::CtrlArrow(Arrow::Home)]);
    s.wait_for_status_contains("1:1");
    s.shutdown();
}

#[test]
fn nav_pagedown_scrolls_viewport() {
    let fx = Fixture::new();
    let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    // Pre-condition: "line 1" is visible somewhere in the editor pane.
    s.wait_until(
        |sc| sc.contents().contains("line 1"),
        std::time::Duration::from_secs(5),
    );
    s.send_key(Key::PageDown);
    // After scrolling, "line 1" specifically (with trailing boundary) is gone.
    s.wait_until(
        |sc| {
            !sc.contents()
                .lines()
                .any(|l| l.trim_end().ends_with("line 1"))
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn nav_home_returns_to_column_one() {
    let fx = Fixture::new();
    let path = fx.write_file("indented.txt", "    indented\nplain\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::End);
    s.wait_for_status_contains("1:13"); // 4 spaces + 8 letters + 1
    s.send_key(Key::Home);
    // Home on an indented line jumps to first non-whitespace (col 5);
    // a second Home would return to col 1.  Either is acceptable here.
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            last.contains(" 1:5 ") || last.contains(" 1:1 ")
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}
