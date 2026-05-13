#![cfg(feature = "ui-tests")]

//! Tier 3.5 — diff-aware navigation and partial revert.

mod ui_common;

use std::process::Command;

use ui_common::{Fixture, Key, SessionOptions, TxtSession};

/// Build a git fixture: init a repo, write & commit `original`, then
/// overwrite the file with `modified` so the buffer differs from HEAD.
/// Returns the (fixture, path) pair ready for `TxtSession::launch`.
fn git_fixture(original: &str, modified: &str) -> (Fixture, std::path::PathBuf) {
    let fx = Fixture::new();
    let ws = fx.workspace_path();
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&ws)
            .status()
            .expect("git invoke");
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "tester"]);
    run(&["config", "commit.gpgsign", "false"]);

    let path = fx.write_file("hunk.txt", original);
    run(&["add", "hunk.txt"]);
    run(&["commit", "-q", "-m", "initial"]);
    // Now diverge: overwrite the file in the workspace, but do not commit.
    std::fs::write(&path, modified).expect("write modified");
    (fx, path)
}

fn launch_with_git(fx: &Fixture, path: &std::path::Path) -> TxtSession {
    // Use the path *relative to the workspace* so that the subprocess `git
    // show HEAD:<path>` lookup matches the entry in the index.
    let rel = path
        .strip_prefix(fx.workspace_path())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let session = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path())
            .allow_git()
            .arg(rel),
    );
    session.wait_for_first_paint();
    session
}

#[test]
fn next_hunk_jumps_to_changed_line() {
    let original = "line1\nline2\nline3\nline4\nline5\n";
    let modified = "line1\nCHANGED\nline3\nline4\nline5\n";
    let (fx, path) = git_fixture(original, modified);
    let mut s = launch_with_git(&fx, &path);
    s.wait_for_status_contains("hunk.txt");
    // Cursor starts at 1:1. Alt+] should jump to line 2.
    s.send_key(Key::Alt(']'));
    s.wait_for_status_contains("2:1");
    s.shutdown();
}

#[test]
fn status_bar_shows_hunk_index() {
    let original = "a\nb\nc\nd\ne\nf\n";
    let modified = "a\nB\nc\nD\ne\nf\n";
    let (fx, path) = git_fixture(original, modified);
    let mut s = launch_with_git(&fx, &path);
    s.wait_for_status_contains("hunk.txt");
    s.send_key(Key::Alt(']')); // first hunk
    s.wait_for_status_contains("hunk 1/2");
    s.send_key(Key::Alt(']')); // second hunk
    s.wait_for_status_contains("hunk 2/2");
    s.shutdown();
}

#[test]
fn revert_hunk_replaces_with_head_content() {
    let original = "alpha\nbeta\ngamma\n";
    let modified = "alpha\nBETA-CHANGED\ngamma\n";
    let (fx, path) = git_fixture(original, modified);
    let mut s = launch_with_git(&fx, &path);
    s.wait_for_status_contains("hunk.txt");
    s.send_key(Key::Alt(']'));
    s.wait_for_status_contains("hunk 1/1");
    // Revert the hunk back to its HEAD content.
    s.send_key(Key::CtrlShift('U'));
    // Buffer should now show "beta" instead of "BETA-CHANGED".
    s.wait_until(
        |sc| {
            let body = sc.contents();
            body.contains("beta") && !body.contains("BETA-CHANGED")
        },
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn peek_overlay_opens_and_dismisses() {
    let original = "first\nsecond\nthird\n";
    let modified = "first\nSECOND-CHANGED\nthird\n";
    let (fx, path) = git_fixture(original, modified);
    let mut s = launch_with_git(&fx, &path);
    s.wait_for_status_contains("hunk.txt");
    s.send_key(Key::Alt(']'));
    s.wait_for_status_contains("hunk 1/1");
    s.send_key(Key::Alt('h'));
    s.wait_for_screen_contains("HEAD (peek)");
    // HEAD line should appear.
    s.wait_for_screen_contains("second");
    // Toggling again closes the peek.
    s.send_key(Key::Alt('h'));
    s.wait_until(
        |sc| !sc.contents().contains("HEAD (peek)"),
        std::time::Duration::from_secs(5),
    );
    s.shutdown();
}
