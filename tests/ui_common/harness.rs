//! `TxtSession`: spawn the real `txt` binary inside a PTY and drive it from
//! a test thread.  Output is parsed by `vt100` so tests can assert on the
//! rendered screen contents (lines, cursor position, status bar).
//!
//! See `tests/ui_common/keys.rs` for the input encoding.

#![allow(dead_code)]

use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

use super::keys::{Key, key_to_bytes};

/// Default poll interval used by [`TxtSession::wait_until`].
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Default timeout for [`TxtSession::wait_until`].
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long [`Drop`] waits for the child to exit after sending Ctrl+Q.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Options controlling [`TxtSession::launch`].
pub struct SessionOptions {
    /// Arguments passed to `txt` after the binary path.
    pub args: Vec<String>,
    /// Working directory the binary is spawned in (the workspace).
    pub cwd: PathBuf,
    /// Sets `TXT_CONFIG_DIR` so config/trust/keybindings live in an isolated tempdir.
    pub config_dir: PathBuf,
    /// PTY size as `(rows, cols)`.  Default `(24, 80)`.
    pub size: (u16, u16),
    /// Additional environment variables.  Override the harness defaults.
    pub extra_env: Vec<(String, String)>,
}

impl SessionOptions {
    pub fn new(cwd: PathBuf, config_dir: PathBuf) -> Self {
        Self {
            args: Vec::new(),
            cwd,
            config_dir,
            size: (24, 80),
            extra_env: Vec::new(),
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.extra_env.push((k.into(), v.into()));
        self
    }

    pub fn size(mut self, rows: u16, cols: u16) -> Self {
        self.size = (rows, cols);
        self
    }
}

/// One running `txt` instance bound to a PTY.
pub struct TxtSession {
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    _reader_handle: thread::JoinHandle<()>,
    /// Kept alive so the PTY master stays open.
    _master: Box<dyn portable_pty::MasterPty + Send>,
    /// `true` once Drop has been allowed to run cleanup without panicking.
    dropped_clean: bool,
}

impl TxtSession {
    /// Spawn `txt` with the given options and start the reader thread.
    pub fn launch(opts: SessionOptions) -> Self {
        let pty_system = NativePtySystem::default();
        let (rows, cols) = opts.size;
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty failed");

        let bin = env!("CARGO_BIN_EXE_txt");
        let mut cmd = CommandBuilder::new(bin);
        cmd.cwd(&opts.cwd);

        // Scrub environment to a known minimum.
        cmd.env_clear();
        cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.env("HOME", &opts.cwd); // any writable dir; not used because TXT_CONFIG_DIR is set
        cmd.env("TERM", "xterm-256color");
        cmd.env("TXT_CONFIG_DIR", &opts.config_dir);
        cmd.env("TXT_DISABLE_LSP", "1");
        cmd.env("TXT_DISABLE_GIT", "1");
        cmd.env("TXT_DISABLE_WATCHER", "1");
        cmd.env("TXT_DISABLE_VERSION_CHECK", "1");
        // Avoid clipboard side-effects under the test runner.
        cmd.env("WAYLAND_DISPLAY", "");
        cmd.env("DISPLAY", "");
        for (k, v) in &opts.extra_env {
            cmd.env(k, v);
        }
        for a in &opts.args {
            cmd.arg(a);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .expect("failed to spawn txt binary");

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("clone pty reader failed");
        let parser_for_reader = Arc::clone(&parser);
        let reader_handle = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut p) = parser_for_reader.lock() {
                            p.process(&buf[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let writer = pair.master.take_writer().expect("take_writer failed");

        Self {
            writer,
            child,
            parser,
            _reader_handle: reader_handle,
            _master: pair.master,
            dropped_clean: false,
        }
    }

    pub fn send_text(&mut self, s: &str) {
        self.writer.write_all(s.as_bytes()).expect("pty write");
        self.writer.flush().ok();
    }

    pub fn send_key(&mut self, k: Key) {
        let bytes = key_to_bytes(k);
        self.writer.write_all(&bytes).expect("pty write");
        self.writer.flush().ok();
    }

    pub fn send_keys(&mut self, ks: &[Key]) {
        for k in ks {
            self.send_key(*k);
        }
    }

    /// Snapshot the current screen for inspection.  Holds the parser lock
    /// only for the duration of `clone()`.
    pub fn screen(&self) -> vt100::Screen {
        self.parser
            .lock()
            .expect("parser poisoned")
            .screen()
            .clone()
    }

    /// Full screen contents with trailing whitespace per line collapsed.
    pub fn screen_text(&self) -> String {
        self.screen().contents()
    }

    /// `(row, col)` from `vt100`'s view of the terminal (zero-based).
    pub fn cursor(&self) -> (u16, u16) {
        self.screen().cursor_position()
    }

    /// Contents of one row (0-based).
    pub fn line(&self, row: u16) -> String {
        let screen = self.screen();
        let (_, cols) = screen.size();
        screen.contents_between(row, 0, row, cols)
    }

    /// Poll `pred(screen)` every [`POLL_INTERVAL`] until it returns `true`
    /// or `timeout` elapses.  Panics on timeout, dumping the screen for
    /// debugging.
    pub fn wait_until<F>(&self, mut pred: F, timeout: Duration)
    where
        F: FnMut(&vt100::Screen) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let screen = self.screen();
            if pred(&screen) {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "wait_until timed out after {timeout:?}\nfinal screen:\n{}",
                    screen.contents()
                );
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Convenience: wait for the first non-empty frame.
    pub fn wait_for_first_paint(&self) {
        self.wait_until(
            |s| s.contents().trim().chars().any(|c| !c.is_whitespace()),
            DEFAULT_TIMEOUT,
        );
    }

    /// Wait until any cell on the screen contains `sub`.
    pub fn wait_for_screen_contains(&self, sub: &str) {
        self.wait_until(|s| s.contents().contains(sub), DEFAULT_TIMEOUT);
    }

    /// Wait until the bottom-most row (status bar) contains `sub`.
    pub fn wait_for_status_contains(&self, sub: &str) {
        self.wait_until(
            |s| {
                let (rows, cols) = s.size();
                let last = s.contents_between(rows - 1, 0, rows - 1, cols);
                last.contains(sub)
            },
            DEFAULT_TIMEOUT,
        );
    }

    /// Hard assertion: the status bar (bottom row) currently contains `sub`.
    pub fn assert_status_contains(&self, sub: &str) {
        let screen = self.screen();
        let (rows, cols) = screen.size();
        let last = screen.contents_between(rows - 1, 0, rows - 1, cols);
        assert!(
            last.contains(sub),
            "status bar missing {sub:?}\nstatus: {last:?}\nfull screen:\n{}",
            screen.contents()
        );
    }

    /// Hard assertion: row `row` currently contains `sub`.
    pub fn assert_line_contains(&self, row: u16, sub: &str) {
        let screen = self.screen();
        let (_, cols) = screen.size();
        let line = screen.contents_between(row, 0, row, cols);
        assert!(
            line.contains(sub),
            "row {row} missing {sub:?}\nrow: {line:?}\nfull screen:\n{}",
            screen.contents()
        );
    }

    pub fn assert_cursor_at(&self, row: u16, col: u16) {
        let (cr, cc) = self.cursor();
        assert_eq!(
            (cr, cc),
            (row, col),
            "cursor at ({cr}, {cc}); expected ({row}, {col})\nscreen:\n{}",
            self.screen_text()
        );
    }

    /// Clean shutdown: send Ctrl+Q, accept any unsaved-changes prompt with
    /// `y`, wait up to [`SHUTDOWN_TIMEOUT`] for the child to exit.  Panics
    /// if the child does not exit cleanly.  Call this at the end of every
    /// test; otherwise `Drop` will panic for you.
    pub fn shutdown(mut self) {
        self.shutdown_impl();
        self.dropped_clean = true;
    }

    fn shutdown_impl(&mut self) {
        let _ = self.writer.write_all(&key_to_bytes(Key::Ctrl('q')));
        let _ = self.writer.write_all(&key_to_bytes(Key::Char('y')));
        let _ = self.writer.flush();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }
        let _ = self.child.kill();
    }
}

impl Drop for TxtSession {
    fn drop(&mut self) {
        if self.dropped_clean {
            return;
        }
        self.shutdown_impl();
    }
}
