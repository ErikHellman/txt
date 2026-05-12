#![cfg(feature = "ui-tests")]

//! Tier 3.3 — clipboard ring overlay (Ctrl+Shift+V).

mod ui_common;

use ui_common::{Fixture, Key};

#[test]
fn clipboard_ring_overlay_opens_and_pastes_older_entry() {
    // Plan: open a file, select-and-copy "alpha", then select-and-copy
    // "beta". Open the clipboard ring (Ctrl+Shift+V) — "beta" should be at
    // the top. Move down once to highlight "alpha" and press Enter; the
    // ring closes and the pasted text appears in the buffer.
    let fx = Fixture::new();
    let path = fx.write_file("ring.txt", "alpha beta gamma\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("ring.txt");

    // Select "alpha" (cols 1..6) and copy.
    s.send_keys(&[
        Key::Home,
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::Ctrl('c'),
    ]);

    // Move past the space to "beta" (cols 7..11) and copy.
    s.send_keys(&[
        Key::Right, // skip space
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::ShiftArrow(ui_common::Arrow::Right),
        Key::Ctrl('c'),
    ]);

    // Move to a fresh insertion point at end-of-line and open the ring.
    s.send_keys(&[Key::End, Key::CtrlShift('V')]);
    s.wait_for_screen_contains("Clipboard ring");

    // Two entries should be visible: "beta" (most recent) and "alpha".
    s.wait_for_screen_contains("alpha");

    // Highlight the older entry and paste.
    s.send_keys(&[Key::Down, Key::Enter]);

    // The original line is unchanged because we landed at End; what was
    // pasted is appended. After paste, screen should contain "gammaalpha"
    // (no space, deliberately — tests the ring → paste round trip, not
    // formatting).
    s.wait_until(
        |sc| sc.contents().contains("gammaalpha"),
        std::time::Duration::from_secs(5),
    );

    s.shutdown();
}

#[test]
fn clipboard_ring_overlay_dismisses_on_escape() {
    let fx = Fixture::new();
    let path = fx.write_file("ring2.txt", "hello\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("ring2.txt");

    // Copy "hello".
    s.send_keys(&[
        Key::Home,
        Key::ShiftArrow(ui_common::Arrow::End),
        Key::Ctrl('c'),
    ]);

    s.send_key(Key::CtrlShift('V'));
    s.wait_for_screen_contains("Clipboard ring");
    s.send_key(Key::Esc);
    // After dismiss, the header should be gone.
    s.wait_until(
        |sc| !sc.contents().contains("Clipboard ring"),
        std::time::Duration::from_secs(5),
    );

    s.shutdown();
}

#[test]
fn clipboard_ring_silent_when_empty() {
    let fx = Fixture::new();
    let path = fx.write_file("empty.txt", "x\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("empty.txt");
    // Without any prior copy, the ring is empty — the overlay must NOT open.
    s.send_key(Key::CtrlShift('V'));
    // Give the binary time to (not) draw the overlay.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let screen = s.screen();
    assert!(
        !screen.contents().contains("Clipboard ring"),
        "overlay should not open with an empty ring; got:\n{}",
        screen.contents()
    );
    s.shutdown();
}
