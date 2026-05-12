#![cfg(feature = "ui-tests")]

//! Tier 3.1 — persistent sessions per workspace.

mod ui_common;

use ui_common::{Fixture, Key, SessionOptions, TxtSession};

fn launch_with(fx: &Fixture, args: &[String]) -> TxtSession {
    let mut opts = SessionOptions::new(fx.workspace_path(), fx.config_path());
    for a in args {
        opts = opts.arg(a.clone());
    }
    let session = TxtSession::launch(opts);
    session.wait_for_first_paint();
    session
}

#[test]
fn session_round_trip_reopens_tab_at_cursor() {
    let fx = Fixture::new();
    fx.append_config("restore_session = true\n");
    let path = fx.write_file("kept.txt", "line one\nline two\nline three\n");

    // Launch with the file as a positional arg, move the cursor to line 2,
    // then quit cleanly.
    let mut s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("kept.txt");
    s1.send_keys(&[Key::Down]);
    s1.wait_for_status_contains("2:1");
    s1.shutdown();

    // Re-launch without any positional arg → session.json should reopen the
    // saved tab and restore the cursor.
    let s2 = launch_with(&fx, &[]);
    s2.wait_for_status_contains("kept.txt");
    s2.wait_for_status_contains("2:1");
    s2.shutdown();
}

#[test]
fn session_not_restored_when_disabled() {
    // Without `restore_session = true`, even a saved session is ignored.
    let fx = Fixture::new();
    let path = fx.write_file("ignored.txt", "x\n");

    let s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("ignored.txt");
    s1.shutdown();

    // Relaunch with no positional arg, with `restore_session = false`
    // (the default).
    let s2 = launch_with(&fx, &[]);
    s2.wait_for_first_paint();
    let screen = s2.screen();
    assert!(
        !screen.contents().contains("ignored.txt"),
        "session should NOT have been restored; got screen:\n{}",
        screen.contents()
    );
    s2.shutdown();
}

#[test]
fn session_skipped_when_positional_file_is_given() {
    // Even with `restore_session = true`, providing a positional file
    // should NOT also reopen prior tabs (would be surprising).
    let fx = Fixture::new();
    fx.append_config("restore_session = true\n");
    let saved = fx.write_file("saved.txt", "a\n");
    let other = fx.write_file("other.txt", "b\n");

    // Save a session containing saved.txt.
    let s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(saved.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("saved.txt");
    s1.shutdown();

    // Launch with a different positional file.
    let s2 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(other.to_string_lossy()),
    );
    s2.wait_for_first_paint();
    s2.wait_for_status_contains("other.txt");
    // saved.txt should NOT be reopened.
    let screen = s2.screen();
    assert!(
        !screen.contents().contains("saved.txt"),
        "explicit positional arg should suppress session restore; screen:\n{}",
        screen.contents()
    );
    s2.shutdown();
}
