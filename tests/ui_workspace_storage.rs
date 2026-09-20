#![cfg(feature = "ui-tests")]

//! `workspace_storage` — where per-workspace state (session, marks, recents,
//! undo, lsp.toml, formatters.toml) lives: in the workspace (default,
//! `<workspace>/.txt/`), globally under `~/.config/txt/workspaces/<sha256>/`,
//! or disabled entirely.

mod ui_common;

use sha2::{Digest, Sha256};
use ui_common::{Fixture, Key, SessionOptions, TxtSession};

/// The directory name the binary derives for a workspace in global mode:
/// lowercase-hex SHA-256 of the canonical workspace path.
fn global_workspaces_dir(fx: &Fixture) -> std::path::PathBuf {
    let workspace = fx.workspace_path();
    let canonical = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.clone());
    let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let key: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    fx.config_path().join("workspaces").join(key)
}

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
fn global_storage_session_lands_in_config_dir() {
    let fx = Fixture::new();
    fx.append_config("restore_session = true\nworkspace_storage = \"global\"\n");
    let path = fx.write_file("global.txt", "one\ntwo\nthree\n");

    let s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("global.txt");
    s1.shutdown();

    // Session must be written under the hashed global directory…
    assert!(
        global_workspaces_dir(&fx).join("session.json").exists(),
        "session.json should live under config_dir/workspaces/<hash>/"
    );
    // …and NOT inside the workspace.
    assert!(
        !fx.workspace_path().join(".txt").exists(),
        "no .txt directory should be created in the workspace in global mode"
    );

    // Relaunch: the globally stored session must restore the tab.
    let s2 = launch_with(&fx, &[]);
    s2.wait_for_status_contains("global.txt");
    s2.shutdown();
}

#[test]
fn disabled_storage_writes_nothing_and_restores_nothing() {
    let fx = Fixture::new();
    fx.append_config("restore_session = true\nworkspace_storage = \"disabled\"\n");
    let path = fx.write_file("ephemeral.txt", "one\n");

    let s1 = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s1.wait_for_first_paint();
    s1.wait_for_status_contains("ephemeral.txt");
    s1.shutdown();

    // No per-workspace state anywhere.
    assert!(
        !fx.workspace_path().join(".txt").exists(),
        ".txt directory must not be created when workspace storage is disabled"
    );
    assert!(
        !fx.config_path().join("workspaces").exists(),
        "workspaces dir must not be created when workspace storage is disabled"
    );

    // And nothing can be restored on the next launch.
    let s2 = launch_with(&fx, &[]);
    s2.wait_for_first_paint();
    let screen = s2.screen();
    assert!(
        !screen.contents().contains("ephemeral.txt"),
        "disabled storage must not restore a session; got:\n{}",
        screen.contents()
    );
    s2.shutdown();
}

#[test]
fn settings_overlay_lists_workspace_storage_toggle() {
    let fx = Fixture::new();
    let path = fx.write_file("settings.txt", "x\n");
    let mut s = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    s.wait_for_first_paint();
    s.wait_for_status_contains("settings.txt");
    s.send_key(Key::Ctrl(','));
    s.wait_for_screen_contains("Settings");
    s.wait_for_screen_contains("Workspace storage");

    // Walk down to the workspace-storage row (last row) — the hint should
    // surface the restart-only semantics.
    const NUM_SETTINGS: usize = 13;
    for _ in 0..NUM_SETTINGS - 1 {
        s.send_key(Key::Down);
    }
    s.wait_for_screen_contains("takes effect on next start");

    // Cycle the value with Right and confirm it persists to config.toml.
    s.send_key(Key::Right);
    s.wait_for_screen_contains("Global (~/.config/txt)");
    s.shutdown();
    let cfg = std::fs::read_to_string(fx.config_path().join("config.toml")).unwrap();
    assert!(
        cfg.contains("workspace_storage = \"global\""),
        "config.toml should record the cycled value; got:\n{cfg}"
    );
}
