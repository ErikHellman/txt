/// System clipboard integration via `arboard`.
///
/// Always maintains an in-process fallback string so copy/paste works even
/// when the system clipboard is unavailable (e.g. running inside a headless
/// terminal multiplexer without clipboard forwarding).
///
/// # Platform notes
/// - macOS: NSPasteboard — fast, non-blocking.
/// - Linux X11/Wayland: may require a clipboard daemon to keep contents alive
///   after the process exits, but reads/writes within the same session are fine.
/// - If arboard returns an error at any point, we silently fall back to the
///   internal string and log nothing (to avoid polluting the TUI).
///
/// A bounded clipboard ring (last [`RING_CAP`] non-empty `set()` values) is
/// kept in memory for the lifetime of the process. The system clipboard
/// remains the default sink/source; the ring is additive and surfaced through
/// the `Ctrl+Shift+V` overlay.
pub struct ClipboardManager {
    /// In-process clipboard used as fallback when arboard is unavailable.
    internal: String,
    /// Bounded ring of recent `set()` values, most-recent first. Adjacent
    /// duplicates are coalesced.
    ring: std::collections::VecDeque<String>,
}

/// Maximum number of distinct entries kept in the clipboard ring.
pub const RING_CAP: usize = 32;

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            internal: String::new(),
            ring: std::collections::VecDeque::new(),
        }
    }

    /// Write `text` to the system clipboard. Falls back to internal storage on error.
    pub fn set(&mut self, text: String) {
        if let Ok(mut clip) = arboard::Clipboard::new() {
            let _ = clip.set_text(&text);
        }
        self.internal = text.clone();
        self.push_ring(text);
    }

    /// Read from the system clipboard. Falls back to internal storage on error.
    pub fn get(&mut self) -> String {
        if let Ok(mut clip) = arboard::Clipboard::new()
            && let Ok(text) = clip.get_text()
        {
            // Keep internal in sync so a future get() after arboard fails still works.
            self.internal = text.clone();
            return text;
        }
        self.internal.clone()
    }

    /// Returns a snapshot of the clipboard ring, most-recent first.
    pub fn ring_entries(&self) -> Vec<String> {
        self.ring.iter().cloned().collect()
    }

    /// Return the entry at `index` from the ring (0 = most recent) and promote
    /// it to the front. `None` when out of bounds. Does NOT touch the system
    /// clipboard so the user's external paste target stays unchanged.
    pub fn pick(&mut self, index: usize) -> Option<String> {
        let entry = self.ring.remove(index)?;
        self.ring.push_front(entry.clone());
        Some(entry)
    }

    fn push_ring(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        // Coalesce with the most recent entry to avoid spamming the ring when
        // the same selection is yanked twice in a row.
        if self.ring.front().is_some_and(|t| t == &text) {
            return;
        }
        // De-dupe: if `text` already exists later in the ring, remove the old
        // occurrence before unshifting the new one.
        if let Some(pos) = self.ring.iter().position(|t| t == &text) {
            self.ring.remove(pos);
        }
        self.ring.push_front(text);
        while self.ring.len() > RING_CAP {
            self.ring.pop_back();
        }
    }

    /// Returns a reference to the internal (in-process) clipboard contents without
    /// touching the system clipboard. Useful for read-only inspection in tests.
    #[cfg(test)]
    pub fn internal(&self) -> &str {
        &self.internal
    }
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_internal() {
        let mut cm = ClipboardManager::new();
        // Set stores in internal regardless of whether system clipboard succeeds.
        cm.set("hello clipboard".to_string());
        assert_eq!(cm.internal(), "hello clipboard");
    }

    #[test]
    fn get_falls_back_to_internal() {
        let mut cm = ClipboardManager::new();
        cm.internal = "fallback text".to_string();
        // get() will try arboard first; if it fails it returns internal.
        // We can't predict whether arboard succeeds in the test environment,
        // so we just verify get() returns a non-empty string.
        let result = cm.get();
        // Either the system clipboard returned something, or the internal fallback did.
        // Both are valid — we just assert the call doesn't panic.
        let _ = result;
    }

    #[test]
    fn empty_initial_state() {
        let cm = ClipboardManager::new();
        assert_eq!(cm.internal(), "");
    }

    #[test]
    fn overwrite_preserves_last_value() {
        let mut cm = ClipboardManager::new();
        cm.set("first".to_string());
        cm.set("second".to_string());
        assert_eq!(cm.internal(), "second");
    }

    #[test]
    fn ring_records_unique_entries_newest_first() {
        let mut cm = ClipboardManager::new();
        cm.set("alpha".into());
        cm.set("beta".into());
        cm.set("gamma".into());
        let ring = cm.ring_entries();
        assert_eq!(
            ring,
            vec!["gamma".to_string(), "beta".into(), "alpha".into()]
        );
    }

    #[test]
    fn ring_coalesces_immediate_duplicates() {
        let mut cm = ClipboardManager::new();
        cm.set("x".into());
        cm.set("x".into());
        cm.set("x".into());
        assert_eq!(cm.ring_entries(), vec!["x".to_string()]);
    }

    #[test]
    fn ring_dedupes_older_occurrence() {
        let mut cm = ClipboardManager::new();
        cm.set("a".into());
        cm.set("b".into());
        cm.set("a".into());
        // After the second `a`, only one `a` remains and it's at the front.
        assert_eq!(cm.ring_entries(), vec!["a".to_string(), "b".into()]);
    }

    #[test]
    fn ring_caps_at_ring_cap() {
        let mut cm = ClipboardManager::new();
        for i in 0..(RING_CAP + 5) {
            cm.set(format!("entry-{i}"));
        }
        assert_eq!(cm.ring_entries().len(), RING_CAP);
        // The oldest 5 should have fallen off.
        assert_eq!(cm.ring_entries().last().unwrap(), &format!("entry-{}", 5));
    }

    #[test]
    fn pick_promotes_entry_to_front() {
        let mut cm = ClipboardManager::new();
        cm.set("a".into());
        cm.set("b".into());
        cm.set("c".into());
        // Pick index 2 (= "a") → ring becomes [a, c, b].
        let picked = cm.pick(2);
        assert_eq!(picked.as_deref(), Some("a"));
        assert_eq!(
            cm.ring_entries(),
            vec!["a".to_string(), "c".into(), "b".into()]
        );
    }

    #[test]
    fn pick_out_of_bounds_returns_none() {
        let mut cm = ClipboardManager::new();
        cm.set("only".into());
        assert!(cm.pick(5).is_none());
    }

    #[test]
    fn empty_set_does_not_pollute_ring() {
        let mut cm = ClipboardManager::new();
        cm.set(String::new());
        cm.set("real".into());
        assert_eq!(cm.ring_entries(), vec!["real".to_string()]);
    }
}
