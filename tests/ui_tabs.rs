#![cfg(feature = "ui-tests")]

mod ui_common;

use std::time::Duration;

use ui_common::{Fixture, Key};

#[test]
fn tabs_ctrl_n_opens_new_tab() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "alpha\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('n'));
    // The new tab is untitled; the tab bar shows two tabs.  Either filename
    // marker or an "[No Name]" label is sufficient evidence.
    s.wait_until(
        |sc| {
            let content = sc.contents();
            content.contains("a.txt") && content.contains("[No Name]")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn tabs_ctrl_w_closes_active_tab() {
    let fx = Fixture::new();
    let path = fx.write_file("a.txt", "alpha\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('n')); // open [No Name]
    s.wait_until(
        |sc| sc.contents().contains("[No Name]"),
        Duration::from_secs(5),
    );
    s.send_key(Key::Ctrl('w')); // close [No Name]
    s.wait_until(
        |sc| !sc.contents().contains("[No Name]"),
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn tabs_ctrl_pageup_returns_to_previous_tab() {
    let fx = Fixture::new();
    let path_a = fx.write_file("a.txt", "alpha\n");
    let _path_b = fx.write_file("b.txt", "beta\n");
    let mut s = fx.open(&path_a);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('n')); // creates [No Name], active
    s.wait_until(
        |sc| sc.contents().contains("[No Name]"),
        Duration::from_secs(5),
    );
    // Ctrl+PageUp = PrevTab.  From the second tab, we should land on `a.txt`.
    s.send_key(Key::CtrlPageUp);
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            last.contains("a.txt")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn tabs_ctrl_digit_jumps_to_tab() {
    let fx = Fixture::new();
    let path_a = fx.write_file("a.txt", "alpha\n");
    let mut s = fx.open(&path_a);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('n')); // tab 2: [No Name]
    s.wait_until(
        |sc| sc.contents().contains("[No Name]"),
        Duration::from_secs(5),
    );
    s.send_key(Key::CtrlDigit('1')); // jump to tab 1
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            last.contains("a.txt")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}
