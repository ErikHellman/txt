#![cfg(feature = "ui-tests")]

mod ui_common;

use std::time::Duration;

use ui_common::{Fixture, SessionOptions, TxtSession};

#[test]
fn harness_launch_first_paint() {
    let fx = Fixture::new();
    let path = fx.write_file("hello.txt", "hello world\n");
    let session = TxtSession::launch(
        SessionOptions::new(fx.workspace_path(), fx.config_path()).arg(path.to_string_lossy()),
    );
    session.wait_for_first_paint();
    // Filename appears somewhere on screen (status bar) within the timeout.
    session.wait_until(
        |s| s.contents().contains("hello.txt"),
        Duration::from_secs(5),
    );
    session.shutdown();
}
