pub mod capabilities;
pub mod client;
pub mod config;
pub mod protocol;
pub mod transport;
pub mod types;

use std::path::Path;
use std::sync::mpsc;

use anyhow::Result;

use self::client::{LspClient, LspUpdate};
use self::config::WorkspaceLspConfig;

// ── LspRegistry ──────────────────────────────────────────────────────────────

/// Manages the active LSP server connection for the workspace.
///
/// Created when `WorkspaceLspConfig::is_active()` returns true. Owns the
/// `LspClient` and the update channel that the main event loop polls each frame.
pub struct LspRegistry {
    client: LspClient,
    update_rx: mpsc::Receiver<LspUpdate>,
    /// Track restart attempts for crash recovery.
    restart_count: u32,
}

const MAX_RESTARTS: u32 = 3;

impl LspRegistry {
    /// Create a new registry and spawn the configured LSP server.
    pub fn start(config: &WorkspaceLspConfig, workspace: &Path) -> Result<Self> {
        let entry = config
            .active_server()
            .ok_or_else(|| anyhow::anyhow!("no active LSP server in config"))?;

        let (update_tx, update_rx) = mpsc::channel();
        let client = LspClient::spawn(entry, workspace, update_tx)?;

        Ok(Self {
            client,
            update_rx,
            restart_count: 0,
        })
    }

    /// Non-blocking drain of all pending LSP updates.
    ///
    /// Call this once per frame in the event loop (like `poll_file_watcher()`).
    pub fn poll(&mut self) -> Vec<LspUpdate> {
        let mut updates = Vec::new();
        while let Ok(update) = self.update_rx.try_recv() {
            updates.push(update);
        }
        updates
    }

    /// Access the client to send requests/notifications.
    pub fn client(&self) -> &LspClient {
        &self.client
    }

    /// Mutable access to the client (for requests that need &mut).
    pub fn client_mut(&mut self) -> &mut LspClient {
        &mut self.client
    }

    /// Whether the server is initialized and ready for requests.
    pub fn is_ready(&self) -> bool {
        self.client.initialized
    }

    /// Whether we've exceeded the restart limit after crashes.
    pub fn restart_exhausted(&self) -> bool {
        self.restart_count >= MAX_RESTARTS
    }

    /// Attempt to restart the server after a crash.
    pub fn try_restart(&mut self, config: &WorkspaceLspConfig, workspace: &Path) -> Result<()> {
        if self.restart_exhausted() {
            anyhow::bail!("LSP restart limit ({}) exceeded", MAX_RESTARTS);
        }

        let entry = config
            .active_server()
            .ok_or_else(|| anyhow::anyhow!("no active LSP server in config"))?;

        let (update_tx, update_rx) = mpsc::channel();
        let client = LspClient::spawn(entry, workspace, update_tx)?;

        self.client = client;
        self.update_rx = update_rx;
        self.restart_count += 1;

        Ok(())
    }
}
