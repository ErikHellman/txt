use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Watches a single file for external modifications using the OS file-notification
/// API.  Events are delivered over a `std::sync::mpsc` channel so the main event
/// loop can poll without blocking.
pub struct FileWatcher {
    /// Kept alive to prevent the watcher thread from terminating.
    _watcher: RecommendedWatcher,
    rx: Receiver<bool>,
}

impl FileWatcher {
    /// Start watching `path`.  Returns `None` if the OS watcher could not be
    /// created (e.g. the file doesn't exist yet, or the platform is unsupported).
    pub fn new(path: &Path) -> Option<Self> {
        let (tx, rx) = channel::<bool>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    let _ = tx.send(true);
                }
            }
        })
        .ok()?;
        watcher.watch(path, RecursiveMode::NonRecursive).ok()?;
        Some(Self { _watcher: watcher, rx })
    }

    /// Non-blocking check: returns `true` if at least one modification event has
    /// arrived since the last call.  Drains the entire channel so spurious
    /// duplicate events don't accumulate.
    pub fn poll(&self) -> bool {
        let mut changed = false;
        while self.rx.try_recv().is_ok() {
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    #[ignore = "timing-sensitive: FSEvents on macOS batches events with variable latency"]
    fn watcher_detects_write() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        let watcher = FileWatcher::new(&path).expect("watcher creation failed");

        // Give the watcher time to register before we write.
        std::thread::sleep(std::time::Duration::from_millis(100));

        tmp.write_all(b"hello").unwrap();
        tmp.flush().unwrap();

        // FSEvents on macOS can have up to ~2s latency; retry every 100ms for up to 3s.
        let detected = (0..30).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            watcher.poll()
        });
        assert!(detected, "expected a change event within 3s");
        // Second poll should see no new events.
        assert!(!watcher.poll(), "channel should be drained");
    }

    #[test]
    fn watcher_no_spurious_events() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        let watcher = FileWatcher::new(&path).expect("watcher creation failed");
        std::thread::sleep(std::time::Duration::from_millis(50));
        // No writes — poll should return false.
        assert!(!watcher.poll());
    }
}
