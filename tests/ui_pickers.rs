#![cfg(feature = "ui-tests")]

mod ui_common;

use std::time::Duration;

use ui_common::{Fixture, Key};

#[test]
fn picker_ctrl_p_opens_fuzzy_picker() {
    let fx = Fixture::new();
    fx.write_file("alpha.txt", "a\n");
    fx.write_file("beta.txt", "b\n");
    fx.write_file("gamma.txt", "c\n");
    let mut s = fx.launch_empty();
    s.send_key(Key::Ctrl('p'));
    // The picker overlay shows the file list with the workspace files.
    s.wait_until(
        |sc| {
            let body = sc.contents();
            body.contains("alpha.txt") && body.contains("beta.txt")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn picker_fuzzy_filters_on_typed_query() {
    let fx = Fixture::new();
    fx.write_file("alpha.txt", "a\n");
    fx.write_file("beta.txt", "b\n");
    fx.write_file("gamma.txt", "c\n");
    let mut s = fx.launch_empty();
    s.send_key(Key::Ctrl('p'));
    s.wait_until(
        |sc| sc.contents().contains("alpha.txt"),
        Duration::from_secs(5),
    );
    s.send_text("bet");
    // After typing "bet", only beta.txt should remain in the visible list.
    s.wait_until(
        |sc| {
            let body = sc.contents();
            body.contains("beta.txt") && !body.contains("gamma.txt")
        },
        Duration::from_secs(5),
    );
    s.shutdown();
}

#[test]
fn picker_fuzzy_enter_opens_selected_file() {
    let fx = Fixture::new();
    fx.write_file("alpha.txt", "alpha contents\n");
    let mut s = fx.launch_empty();
    s.send_key(Key::Ctrl('p'));
    s.wait_until(
        |sc| sc.contents().contains("alpha.txt"),
        Duration::from_secs(5),
    );
    s.send_text("alp");
    s.send_key(Key::Enter);
    // Status bar now shows the opened file plus a cursor at 1:1.
    s.wait_for_status_contains("alpha.txt");
    s.wait_for_status_contains("1:1");
    s.shutdown();
}

#[test]
fn picker_ctrl_g_prompts_for_line() {
    let fx = Fixture::new();
    let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('g'));
    s.wait_for_status_contains("Go to [line:col]:");
    s.shutdown();
}

#[test]
fn picker_ctrl_g_jumps_to_typed_line() {
    let fx = Fixture::new();
    let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let path = fx.write_file("long.txt", &body);
    let mut s = fx.open(&path);
    s.wait_for_status_contains("1:1");
    s.send_key(Key::Ctrl('g'));
    s.wait_for_status_contains("Go to [line:col]:");
    s.send_text("25");
    s.send_key(Key::Enter);
    s.wait_for_status_contains("25:1");
    s.shutdown();
}

#[test]
fn quickfix_shows_no_diagnostics_message_without_lsp() {
    // With LSP disabled in the harness, opening the quickfix list must
    // report "No diagnostics" via the status bar instead of opening the
    // overlay.
    let fx = Fixture::new();
    let path = fx.write_file("nodiag.txt", "fn main() {}\n");
    let mut s = fx.open(&path);
    s.wait_for_status_contains("nodiag.txt");
    s.send_key(Key::Alt('1'));
    s.wait_for_status_contains("No diagnostics");
    s.shutdown();
}
