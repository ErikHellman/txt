#![cfg(feature = "ui-tests")]

mod ui_common;

use ui_common::{Arrow, Fixture, Key};

/// A long file scrolls down when Ctrl+Down is pressed enough times.
#[test]
fn scroll_ctrl_down_moves_viewport() {
    let fx = Fixture::new();
    let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let s = fx.open(&path);
    s.wait_for_status_contains("1:1");

    // Pre-condition: "line 1" is visible on its own row.
    s.wait_until(
        |sc| {
            sc.contents()
                .lines()
                .any(|l| l.trim_end().ends_with("line 1"))
        },
        std::time::Duration::from_secs(5),
    );

    // Ctrl+Down scrolls the viewport (default `SCROLL_LINES` = 3 per press).
    // 20 presses moves the viewport ~60 lines down, well past the first row.
    let mut s = s;
    for _ in 0..20 {
        s.send_key(Key::CtrlArrow(Arrow::Down));
    }
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

/// A flood of scroll events at the end of the file is drained without
/// freezing other input: the very next keystroke after the spam still
/// takes effect promptly.
///
/// Without coalescing and no-op detection this would push the editor
/// many render cycles behind and Ctrl+Home would feel "stuck" until the
/// queue drains.
#[test]
fn scroll_burst_past_end_does_not_block_other_keys() {
    let fx = Fixture::new();
    let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");

    // Send hundreds of Ctrl+Down at once. 200 lines / 3-per-scroll → the
    // viewport saturates after ~67 events; the rest hit the no-op path
    // in `AppState::scroll_action_is_no_op`.
    let keys: Vec<Key> = std::iter::repeat(Key::CtrlArrow(Arrow::Down))
        .take(300)
        .collect();
    s.send_keys(&keys);

    // The follow-up keystroke must register without being starved by the
    // backlog of dropped scrolls.
    s.send_key(Key::CtrlArrow(Arrow::Home));
    s.wait_for_status_contains("1:1");
    s.wait_until(
        |sc| {
            sc.contents()
                .lines()
                .any(|l| l.trim_end().ends_with("line 1"))
        },
        std::time::Duration::from_secs(5),
    );

    s.shutdown();
}

/// Scrolling up when already at the top is a no-op and never moves the
/// cursor either (`Scroll` is independent from cursor motion).
#[test]
fn scroll_up_at_top_is_a_no_op() {
    let fx = Fixture::new();
    let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");

    for _ in 0..50 {
        s.send_key(Key::CtrlArrow(Arrow::Up));
    }
    // Cursor must still be on line 1, column 1.
    s.assert_status_contains("1:1");
    // "line 1" remains visible.
    s.wait_until(
        |sc| {
            sc.contents()
                .lines()
                .any(|l| l.trim_end().ends_with("line 1"))
        },
        std::time::Duration::from_secs(5),
    );

    s.shutdown();
}
