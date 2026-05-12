#![cfg(feature = "ui-tests")]

mod ui_common;

use std::time::Duration;

use ui_common::{Fixture, Key};

#[test]
fn save_ctrl_s_writes_file_and_clears_modified() {
    let fx = Fixture::new();
    let path = fx.write_file("note.txt", "hello\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('X'));
    s.wait_for_status_contains("[+]");
    s.send_key(Key::Ctrl('s'));
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            !last.contains("[+]")
        },
        Duration::from_secs(5),
    );
    let on_disk = std::fs::read_to_string(&path).expect("read back saved file");
    assert!(
        on_disk.starts_with("Xhello"),
        "saved file did not contain the edit: {on_disk:?}"
    );
    s.shutdown();
}

#[test]
fn save_as_prompts_in_status_bar() {
    let fx = Fixture::new();
    let path = fx.write_file("note.txt", "abc\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::CtrlShift('s'));
    s.wait_for_status_contains("Save as:");
    s.shutdown();
}

#[test]
fn save_as_writes_to_new_path() {
    let fx = Fixture::new();
    let path = fx.write_file("orig.txt", "abc\n");
    let new_path = fx.workspace_path().join("renamed.txt");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::CtrlShift('s'));
    s.wait_for_status_contains("Save as:");
    s.send_text(new_path.to_str().expect("utf-8 path"));
    s.send_key(Key::Enter);
    s.wait_until(
        |sc| sc.contents().contains("renamed.txt"),
        Duration::from_secs(5),
    );
    assert!(new_path.exists(), "save-as did not create {new_path:?}");
    s.shutdown();
}

#[test]
fn quit_dirty_buffer_shows_confirm_prompt() {
    let fx = Fixture::new();
    // confirm_exit defaults to false; opt in via the config file.
    let cfg = fx.config_path().join("config.toml");
    std::fs::write(&cfg, "last_seen_version = \"X\"\nconfirm_exit = true\n").unwrap();
    let path = fx.write_file("a.txt", "abc\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('Z'));
    s.wait_for_status_contains("[+]");
    s.send_key(Key::Ctrl('q'));
    s.wait_for_status_contains("Quit anyway?");
    s.send_key(Key::Char('n'));
    // After cancelling the prompt the editor is back to normal.
    s.wait_until(
        |sc| {
            let (rows, cols) = sc.size();
            let last = sc.contents_between(rows - 1, 0, rows - 1, cols);
            !last.contains("Quit anyway?")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn quit_confirm_y_exits_cleanly() {
    let fx = Fixture::new();
    let cfg = fx.config_path().join("config.toml");
    std::fs::write(&cfg, "last_seen_version = \"X\"\nconfirm_exit = true\n").unwrap();
    let path = fx.write_file("a.txt", "abc\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Char('Z'));
    s.wait_for_status_contains("[+]");
    s.send_key(Key::Ctrl('q'));
    s.wait_for_status_contains("Quit anyway?");
    s.send_key(Key::Char('y'));
    s.shutdown();
    // shutdown() returns only after the child exits cleanly.
}
