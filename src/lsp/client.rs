use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use serde_json::Value;

use super::capabilities::{ServerCapabilities, client_capabilities, parse_server_capabilities};
use super::config::LspServerEntry;
use super::protocol::{IncomingMessage, NotificationMessage, RequestMessage, classify_incoming};
use super::transport;

// ── Outbound messages (main thread → writer thread) ──────────────────────────

/// Messages the main thread can send to the LSP writer thread.
pub enum OutboundMessage {
    Request(RequestMessage),
    Notification(NotificationMessage),
    Shutdown,
}

// ── LspUpdate (reader thread → main thread) ──────────────────────────────────

/// Updates pushed from the reader thread to the main event loop.
#[derive(Debug)]
#[allow(dead_code)]
pub enum LspUpdate {
    /// Server capabilities after successful initialize handshake.
    Initialized(ServerCapabilities),
    /// Diagnostics pushed by `textDocument/publishDiagnostics`.
    Diagnostics {
        uri: String,
        diagnostics: Vec<Value>,
    },
    /// Response to a completion request.
    Completion { request_id: u64, items: Vec<Value> },
    /// Response to a hover request.
    Hover {
        request_id: u64,
        contents: Option<Value>,
    },
    /// Response to a definition request.
    Definition { request_id: u64, locations: Value },
    /// Response to a references request.
    References { request_id: u64, locations: Value },
    /// Response to a rename request.
    Rename {
        request_id: u64,
        edit: Option<Value>,
    },
    /// Response to a code action request.
    CodeActions {
        request_id: u64,
        actions: Vec<Value>,
    },
    /// Semantic tokens response.
    SemanticTokens { uri: String, data: Vec<u32> },
    /// The server process has exited (crash or normal shutdown).
    ServerExited,
    /// An error occurred on the LSP connection.
    Error(String),
}

// ── LspClient ────────────────────────────────────────────────────────────────

/// Manages a single LSP server: child process, reader/writer threads, channels.
pub struct LspClient {
    /// Channel to send outbound messages to the writer thread.
    out_tx: mpsc::Sender<OutboundMessage>,
    /// Next request ID (monotonically increasing).
    next_id: u64,
    /// The reader thread handle (detached on drop — kept alive as long as client lives).
    _reader_handle: thread::JoinHandle<()>,
    /// The writer thread handle (detached on drop — kept alive as long as client lives).
    _writer_handle: thread::JoinHandle<()>,
    /// The child process (killed on drop if still running).
    child: Child,
    /// Whether the initialize handshake has completed.
    pub initialized: bool,
    /// Server capabilities (populated after initialize response).
    pub capabilities: ServerCapabilities,
}

impl LspClient {
    /// Spawn the LSP server process and start reader/writer threads.
    ///
    /// Returns `(LspClient, Receiver<LspUpdate>)`.
    pub fn spawn(
        entry: &LspServerEntry,
        workspace_root: &Path,
        update_tx: mpsc::Sender<LspUpdate>,
    ) -> Result<Self> {
        let mut child = Command::new(&entry.command)
            .args(&entry.args)
            .current_dir(workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn LSP server: {}", entry.command))?;

        let stdin = child.stdin.take().context("no stdin on LSP child")?;
        let stdout = child.stdout.take().context("no stdout on LSP child")?;

        // Writer thread: receives OutboundMessages, writes them as JSON-RPC.
        let (out_tx, out_rx) = mpsc::channel::<OutboundMessage>();
        let writer_handle = thread::Builder::new()
            .name("lsp-writer".into())
            .spawn(move || {
                let mut writer = BufWriter::new(stdin);
                while let Ok(msg) = out_rx.recv() {
                    let result = match &msg {
                        OutboundMessage::Request(req) => transport::write_json(&mut writer, req),
                        OutboundMessage::Notification(notif) => {
                            transport::write_json(&mut writer, notif)
                        }
                        OutboundMessage::Shutdown => break,
                    };
                    if result.is_err() {
                        break;
                    }
                }
            })
            .context("failed to spawn LSP writer thread")?;

        // Reader thread: reads JSON-RPC messages, classifies, sends LspUpdates.
        let reader_update_tx = update_tx.clone();
        let reader_handle = thread::Builder::new()
            .name("lsp-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let value = match transport::read_message(&mut reader) {
                        Ok(v) => v,
                        Err(_) => {
                            // EOF or I/O error — server exited or crashed.
                            let _ = reader_update_tx.send(LspUpdate::ServerExited);
                            break;
                        }
                    };

                    let update = match classify_incoming(&value) {
                        Some(IncomingMessage::Response(resp)) => {
                            dispatch_response(resp.id, resp.result, resp.error)
                        }
                        Some(IncomingMessage::Notification(notif)) => {
                            dispatch_notification(&notif.method, notif.params)
                        }
                        None => None,
                    };

                    if let Some(u) = update
                        && reader_update_tx.send(u).is_err()
                    {
                        break; // Main thread gone.
                    }
                }
            })
            .context("failed to spawn LSP reader thread")?;

        let mut client = Self {
            out_tx,
            next_id: 1,
            _reader_handle: reader_handle,
            _writer_handle: writer_handle,
            child,
            initialized: false,
            capabilities: ServerCapabilities::default(),
        };

        // Send initialize request.
        client.send_initialize(workspace_root, entry.init_options.clone())?;

        Ok(client)
    }

    // ── Request helpers ──────────────────────────────────────────────────

    /// Allocate a new request ID and send a request.
    pub fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let req = RequestMessage::new(id, method, params);
        self.out_tx
            .send(OutboundMessage::Request(req))
            .context("LSP writer channel closed")?;
        Ok(id)
    }

    /// Send a notification (no response expected).
    pub fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = NotificationMessage::new(method, params);
        self.out_tx
            .send(OutboundMessage::Notification(notif))
            .context("LSP writer channel closed")?;
        Ok(())
    }

    // ── LSP lifecycle ────────────────────────────────────────────────────

    fn send_initialize(
        &mut self,
        workspace_root: &Path,
        init_options: Option<Value>,
    ) -> Result<u64> {
        let root_uri = super::types::path_to_uri(workspace_root);
        let mut params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": client_capabilities(),
        });
        if let Some(opts) = init_options {
            params["initializationOptions"] = opts;
        }
        self.send_request("initialize", Some(params))
    }

    /// Called when the initialize response arrives. Sends `initialized` notification.
    #[allow(dead_code)]
    pub fn complete_initialization(&mut self, result: &Value) {
        self.capabilities = parse_server_capabilities(result);
        self.initialized = true;
        let _ = self.send_notification("initialized", Some(serde_json::json!({})));
    }

    // ── Document lifecycle notifications ─────────────────────────────────

    /// Send `textDocument/didOpen`.
    pub fn did_open(&self, uri: &str, language_id: &str, version: u64, text: &str) -> Result<()> {
        self.send_notification(
            "textDocument/didOpen",
            Some(serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text,
                }
            })),
        )
    }

    /// Send `textDocument/didChange` (full sync).
    #[allow(dead_code)]
    pub fn did_change(&self, uri: &str, version: u64, text: &str) -> Result<()> {
        self.send_notification(
            "textDocument/didChange",
            Some(serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "version": version,
                },
                "contentChanges": [{ "text": text }]
            })),
        )
    }

    /// Send `textDocument/didSave`.
    #[allow(dead_code)]
    pub fn did_save(&self, uri: &str) -> Result<()> {
        self.send_notification(
            "textDocument/didSave",
            Some(serde_json::json!({
                "textDocument": { "uri": uri }
            })),
        )
    }

    /// Send `textDocument/didClose`.
    #[allow(dead_code)]
    pub fn did_close(&self, uri: &str) -> Result<()> {
        self.send_notification(
            "textDocument/didClose",
            Some(serde_json::json!({
                "textDocument": { "uri": uri }
            })),
        )
    }

    // ── Feature requests ─────────────────────────────────────────────────

    /// Request code completion at the given position.
    #[allow(dead_code)]
    pub fn request_completion(&mut self, uri: &str, line: u32, character: u32) -> Result<u64> {
        self.send_request(
            "textDocument/completion",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            })),
        )
    }

    /// Request hover info at the given position.
    #[allow(dead_code)]
    pub fn request_hover(&mut self, uri: &str, line: u32, character: u32) -> Result<u64> {
        self.send_request(
            "textDocument/hover",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            })),
        )
    }

    /// Request go-to-definition at the given position.
    #[allow(dead_code)]
    pub fn request_definition(&mut self, uri: &str, line: u32, character: u32) -> Result<u64> {
        self.send_request(
            "textDocument/definition",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            })),
        )
    }

    /// Request find-references at the given position.
    #[allow(dead_code)]
    pub fn request_references(&mut self, uri: &str, line: u32, character: u32) -> Result<u64> {
        self.send_request(
            "textDocument/references",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true },
            })),
        )
    }

    /// Request rename at the given position with a new name.
    #[allow(dead_code)]
    pub fn request_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<u64> {
        self.send_request(
            "textDocument/rename",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name,
            })),
        )
    }

    /// Request code actions for the given range.
    #[allow(dead_code)]
    pub fn request_code_action(&mut self, uri: &str, range: serde_json::Value) -> Result<u64> {
        self.send_request(
            "textDocument/codeAction",
            Some(serde_json::json!({
                "textDocument": { "uri": uri },
                "range": range,
                "context": { "diagnostics": [] },
            })),
        )
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Send shutdown request, then exit notification.
        let _ = self.send_request("shutdown", None);
        let _ = self.send_notification("exit", None);
        let _ = self.out_tx.send(OutboundMessage::Shutdown);

        // Give the server a moment to exit gracefully, then kill.
        let _ = self
            .child
            .try_wait()
            .ok()
            .flatten()
            .or_else(|| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.child.try_wait().ok().flatten()
            })
            .or_else(|| {
                let _ = self.child.kill();
                self.child.wait().ok()
            });
    }
}

// ── Response/notification dispatch ───────────────────────────────────────────

/// Map a response to an `LspUpdate` based on the request ID.
///
/// The first request (ID 1) is always `initialize`. For other requests, we
/// inspect the result shape to determine the response type.
fn dispatch_response(
    id: u64,
    result: Option<Value>,
    error: Option<super::protocol::RpcError>,
) -> Option<LspUpdate> {
    if let Some(err) = error {
        return Some(LspUpdate::Error(format!(
            "LSP error (id={}): {} (code {})",
            id, err.message, err.code
        )));
    }

    let result = result?;

    // ID 1 is always the initialize handshake.
    if id == 1 {
        return Some(LspUpdate::Initialized(parse_server_capabilities(&result)));
    }

    // For other responses, we return a generic structure. The caller (LspRegistry)
    // tracks pending request IDs to know what each response means.
    // For now, we try to detect the shape:
    if result.is_null() {
        return None;
    }

    // Completion response: items array or { items: [...] }
    if let Some(items) = result.as_array() {
        return Some(LspUpdate::Completion {
            request_id: id,
            items: items.clone(),
        });
    }
    if let Some(items) = result.get("items").and_then(|v| v.as_array()) {
        return Some(LspUpdate::Completion {
            request_id: id,
            items: items.clone(),
        });
    }

    // Hover response: { contents: ... }
    if result.get("contents").is_some() {
        return Some(LspUpdate::Hover {
            request_id: id,
            contents: result.get("contents").cloned(),
        });
    }

    // WorkspaceEdit response (rename): { changes: {...} } or { documentChanges: [...] }
    if result.get("changes").is_some() || result.get("documentChanges").is_some() {
        return Some(LspUpdate::Rename {
            request_id: id,
            edit: Some(result),
        });
    }

    // Semantic tokens response: { data: [u32...] }
    if let Some(data) = result.get("data").and_then(|v| v.as_array()) {
        let nums: Vec<u32> = data
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u32))
            .collect();
        return Some(LspUpdate::SemanticTokens {
            uri: String::new(), // URI will be matched by the caller
            data: nums,
        });
    }

    // Default: treat as definition/references (Location or Location[])
    Some(LspUpdate::Definition {
        request_id: id,
        locations: result,
    })
}

/// Map a server notification to an `LspUpdate`.
fn dispatch_notification(method: &str, params: Option<Value>) -> Option<LspUpdate> {
    match method {
        "textDocument/publishDiagnostics" => {
            let params = params?;
            let uri = params.get("uri")?.as_str()?.to_string();
            let diagnostics = params
                .get("diagnostics")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            Some(LspUpdate::Diagnostics { uri, diagnostics })
        }
        _ => None, // Ignore unknown notifications.
    }
}
