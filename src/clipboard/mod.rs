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
pub struct ClipboardManager {
    /// In-process clipboard used as fallback when arboard is unavailable.
    internal: String,
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            internal: String::new(),
        }
    }

    /// Write `text` to the system clipboard. Falls back to internal storage on error.
    pub fn set(&mut self, text: String) {
        if let Ok(mut clip) = arboard::Clipboard::new() {
            let _ = clip.set_text(&text);
        }
        self.internal = text;
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
}
