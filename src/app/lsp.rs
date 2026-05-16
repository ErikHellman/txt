use crate::input::action::EditorAction;

use super::util::{extract_word_at, parse_lsp_position, same_file};
use super::{AppState, InputMode, LspApprovalReason, PendingLspApproval};

impl AppState {
    /// Flush debounced LSP notifications if enough idle time has passed.
    /// Called once per frame in the event loop.
    pub fn flush_lsp_debounce(&mut self) {
        let Some(dirty_since) = self.lsp_dirty_since else {
            return;
        };
        let elapsed = dirty_since.elapsed();

        // After 100ms idle, send the buffered didChange (one full-buffer copy).
        if elapsed >= Self::LSP_DEBOUNCE && !self.lsp_change_sent {
            self.send_lsp_did_change();
            self.lsp_change_sent = true;
        }

        // After 300ms idle, re-request semantic tokens and clear the timer.
        if elapsed >= Self::SEMANTIC_TOKEN_DEBOUNCE {
            self.request_semantic_tokens_for_active();
            self.lsp_dirty_since = None;
            self.lsp_change_sent = false;
        }
    }
    /// Non-blocking drain of pending LSP updates. Called once per frame.
    pub fn poll_lsp_updates(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        let updates = registry.poll();
        for update in updates {
            self.apply_lsp_update(update);
        }
    }
    pub(super) fn apply_lsp_update(&mut self, update: crate::lsp::client::LspUpdate) {
        use crate::lsp::client::LspUpdate;
        match update {
            LspUpdate::Initialized(caps) => {
                if let Some(registry) = &mut self.lsp {
                    registry.client_mut().capabilities = caps;
                    registry.client_mut().initialized = true;
                    let _ = registry
                        .client()
                        .send_notification("initialized", Some(serde_json::json!({})));
                }
                // Send didOpen for all currently open buffers.
                self.notify_lsp_did_open_all();
                // Request semantic tokens for the active buffer.
                self.request_semantic_tokens_for_active();
            }
            LspUpdate::Diagnostics { uri, diagnostics } => {
                self.apply_diagnostics(&uri, &diagnostics);
            }
            LspUpdate::ServerExited => {
                // If the binary on disk is still the one the user approved,
                // use the existing in-place restart path (preserves restart_count
                // so a crash loop self-terminates after MAX_RESTARTS).
                // If the binary changed (or vanished), tear down and route
                // through the approval gate so the user is re-prompted.
                let trusted = self.lsp_binary_still_trusted();

                if !trusted {
                    self.lsp = None;
                    self.status_error =
                        Some("LSP binary changed since approval; re-prompting".into());
                    self.request_lsp_start();
                    return;
                }

                let config = self.lsp_config.clone();
                let workspace = self.workspace.clone();
                let mut disable_lsp = false;

                if let Some(registry) = &mut self.lsp
                    && (registry.restart_exhausted()
                        || registry.try_restart(&config, &workspace).is_err())
                {
                    disable_lsp = true;
                }

                if disable_lsp {
                    self.lsp = None;
                    self.status_error =
                        Some("LSP server exited unexpectedly (restart limit reached)".into());
                } else {
                    self.status_error = Some("LSP server exited, restarting…".into());
                }
            }
            LspUpdate::Completion { items, .. } => {
                self.apply_completion_response(items);
            }
            LspUpdate::Hover { contents, .. } => {
                self.apply_hover_response(contents);
            }
            LspUpdate::Definition { locations, .. } => {
                self.apply_definition_response(locations);
            }
            LspUpdate::References { locations, .. } => {
                self.apply_references_response(locations);
            }
            LspUpdate::Rename { edit, .. } => {
                if let Some(edit) = edit {
                    self.apply_workspace_edit(&edit);
                }
            }
            LspUpdate::CodeActions { actions, .. } => {
                let _ = actions; // TODO: show code action picker
            }
            LspUpdate::SemanticTokens { uri, data } => {
                self.apply_semantic_tokens(&uri, &data);
            }
            LspUpdate::Error(msg) => {
                self.status_error = Some(msg);
            }
        }
    }
    pub(super) fn notify_lsp_did_open_all(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        for tab in &self.editor.tabs {
            if let Some(path) = &tab.path {
                let uri = crate::lsp::types::path_to_uri(path);
                let lang_id = tab.syntax.language.name().to_lowercase();
                let text = tab.buffer.rope().to_string();
                let _ = registry
                    .client()
                    .did_open(&uri, &lang_id, tab.lsp_state.version, &text);
            }
        }
    }
    /// Send `textDocument/didOpen` for a single buffer.
    #[allow(dead_code)]
    pub(super) fn notify_lsp_did_open(&self, handle: &crate::editor::tab::BufferHandle) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        if let Some(path) = &handle.path {
            let uri = crate::lsp::types::path_to_uri(path);
            let lang_id = handle.syntax.language.name().to_lowercase();
            let text = handle.buffer.rope().to_string();
            let _ = registry
                .client()
                .did_open(&uri, &lang_id, handle.lsp_state.version, &text);
        }
    }
    /// Send `textDocument/didChange` for the active buffer (full sync).
    /// Version must already be bumped before calling this.
    pub(super) fn send_lsp_did_change(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        if let Some(path) = &handle.path {
            let uri = crate::lsp::types::path_to_uri(path);
            let version = handle.lsp_state.version;
            let text = handle.buffer.rope().to_string();
            let _ = registry.client().did_change(&uri, version, &text);
        }
    }
    /// Send `textDocument/didSave` for the active buffer.
    pub(super) fn notify_lsp_did_save(&self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        if let Some(path) = &self.editor.active().path {
            let uri = crate::lsp::types::path_to_uri(path);
            let _ = registry.client().did_save(&uri);
        }
    }
    /// Send `textDocument/didClose` for a buffer by path.
    pub(super) fn notify_lsp_did_close(&self, path: &std::path::Path) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() {
            return;
        }
        let uri = crate::lsp::types::path_to_uri(path);
        let _ = registry.client().did_close(&uri);
    }
    /// Convert raw diagnostic JSON from the server to byte-offset `LspDiagnostic`s
    /// and store them on the matching buffer.
    pub(super) fn apply_diagnostics(&mut self, uri: &str, raw_diagnostics: &[serde_json::Value]) {
        use crate::lsp::types::{DiagSeverity, LspDiagnostic, lsp_position_to_byte_offset};

        let path = match crate::lsp::types::uri_to_path(uri) {
            Some(p) => p,
            None => return,
        };

        // Find the buffer that matches this URI.
        let tab = self
            .editor
            .tabs
            .iter_mut()
            .find(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
        let Some(tab) = tab else { return };

        let rope = tab.buffer.rope();
        let mut diagnostics = Vec::with_capacity(raw_diagnostics.len());

        for raw in raw_diagnostics {
            let range = match raw.get("range") {
                Some(r) => r,
                None => continue,
            };
            let start = match parse_lsp_position(range.get("start")) {
                Some(pos) => match lsp_position_to_byte_offset(rope, pos) {
                    Some(b) => b,
                    None => continue,
                },
                None => continue,
            };
            let end = match parse_lsp_position(range.get("end")) {
                Some(pos) => match lsp_position_to_byte_offset(rope, pos) {
                    Some(b) => b,
                    None => continue,
                },
                None => continue,
            };
            let severity = match raw.get("severity").and_then(|v| v.as_u64()) {
                Some(1) => DiagSeverity::Error,
                Some(2) => DiagSeverity::Warning,
                Some(3) => DiagSeverity::Information,
                _ => DiagSeverity::Hint,
            };
            let message = raw
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let source = raw.get("source").and_then(|v| v.as_str()).map(String::from);

            diagnostics.push(LspDiagnostic {
                range: crate::buffer::cursor::ByteRange { start, end },
                severity,
                message,
                source,
            });
        }

        tab.lsp_state.diagnostics = diagnostics;
    }
    pub(super) fn trigger_rename(&mut self) {
        let Some(registry) = &self.lsp else { return };
        if !registry.is_ready() || !registry.client().capabilities.rename_provider {
            return;
        }
        // Enter rename modal: prompt for new name.
        let handle = self.editor.active();
        let cursor = handle.buffer.cursors.primary();
        // Extract word under cursor as the default name.
        let rope = handle.buffer.rope();
        let byte = cursor.byte_offset;
        let text = rope.to_string();
        let word = extract_word_at(&text, byte);
        self.input_mode = InputMode::Rename(word);
    }
    /// Send rename request after the user confirms the new name.
    pub(super) fn send_rename(&mut self, new_name: &str) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );

        let _ = registry
            .client_mut()
            .request_rename(&uri, pos.line, pos.character, new_name);
    }
    pub(super) fn apply_workspace_edit(&mut self, edit: &serde_json::Value) {
        let changes = match edit.get("changes").and_then(|v| v.as_object()) {
            Some(c) => c,
            None => return,
        };

        for (uri, edits) in changes {
            let path = match crate::lsp::types::uri_to_path(uri) {
                Some(p) => p,
                None => continue,
            };
            let edits = match edits.as_array() {
                Some(e) => e,
                None => continue,
            };

            // Find or open the tab.
            let tab_idx = self
                .editor
                .tabs
                .iter()
                .position(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
            let tab_idx = match tab_idx {
                Some(i) => i,
                None => continue, // Skip files not open.
            };

            // Collect and sort edits in reverse order to avoid offset shifting.
            let mut text_edits: Vec<(usize, usize, String)> = Vec::new();
            let rope = self.editor.tabs[tab_idx].buffer.rope();
            for e in edits {
                let range = match e.get("range") {
                    Some(r) => r,
                    None => continue,
                };
                let start = match parse_lsp_position(range.get("start")) {
                    Some(pos) => {
                        crate::lsp::types::lsp_position_to_byte_offset(rope, pos).unwrap_or(0)
                    }
                    None => continue,
                };
                let end = match parse_lsp_position(range.get("end")) {
                    Some(pos) => {
                        crate::lsp::types::lsp_position_to_byte_offset(rope, pos).unwrap_or(0)
                    }
                    None => continue,
                };
                let new_text = e
                    .get("newText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                text_edits.push((start, end, new_text));
            }

            // Apply in reverse byte order.
            text_edits.sort_by_key(|b| std::cmp::Reverse(b.0));
            let tab = &mut self.editor.tabs[tab_idx];
            for (start, end, new_text) in &text_edits {
                let rope = tab.buffer.rope();
                let start_char = rope.byte_to_char(*start);
                let end_char = rope.byte_to_char(*end);
                tab.buffer.delete_range(start_char, end_char);
                tab.buffer.insert_str(new_text);
            }
        }
    }
    pub(super) fn trigger_code_action(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.code_action_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let cursor = handle.buffer.cursors.primary();
        let pos = crate::lsp::types::byte_offset_to_lsp_position(
            handle.buffer.rope(),
            cursor.byte_offset,
        );
        let range = serde_json::json!({
            "start": { "line": pos.line, "character": pos.character },
            "end": { "line": pos.line, "character": pos.character },
        });

        let _ = registry.client_mut().request_code_action(&uri, range);
    }
    pub(super) fn apply_semantic_tokens(&mut self, uri: &str, data: &[u32]) {
        let path = match crate::lsp::types::uri_to_path(uri) {
            Some(p) => p,
            None => return,
        };
        let tab = self
            .editor
            .tabs
            .iter_mut()
            .find(|t| t.path.as_ref().is_some_and(|p| same_file(p, &path)));
        let Some(tab) = tab else { return };

        let rope = tab.buffer.rope();
        let tokens = crate::lsp::types::decode_semantic_tokens(data, rope);
        tab.lsp_state.semantic_tokens = Some(tokens);
    }
    /// Request semantic tokens for the active buffer.
    pub(super) fn request_semantic_tokens_for_active(&mut self) {
        let Some(registry) = &mut self.lsp else {
            return;
        };
        if !registry.is_ready() || !registry.client().capabilities.semantic_tokens_provider {
            return;
        }
        let handle = self.editor.active();
        let Some(path) = &handle.path else { return };
        let uri = crate::lsp::types::path_to_uri(path);
        let _ = registry.client_mut().send_request(
            "textDocument/semanticTokens/full",
            Some(serde_json::json!({
                "textDocument": { "uri": uri }
            })),
        );
    }
    pub(super) fn lsp_restart(&mut self) {
        // Tear down existing connection.
        self.lsp = None;
        self.pending_lsp_approval = None;
        // Clear stale state from all buffers.
        for tab in &mut self.editor.tabs {
            tab.lsp_state.diagnostics.clear();
            tab.lsp_state.semantic_tokens = None;
        }
        // Start fresh if config is active — routed through the trust gate.
        self.request_lsp_start();
    }
    /// Trust-gated entry point for spawning the LSP server.
    ///
    /// Resolves the configured binary, hashes it, and consults the user-global
    /// trust store. If the binary is approved, spawns directly. If unknown or
    /// the hash has changed, sets `pending_lsp_approval` so the approval
    /// overlay opens on the next frame.
    pub fn request_lsp_start(&mut self) {
        if std::env::var_os("TXT_DISABLE_LSP").is_some() {
            return;
        }
        if !self.lsp_config.is_active() {
            return;
        }
        let entry = match self.lsp_config.active_server() {
            Some(e) => e.clone(),
            None => return,
        };
        let server_name = self.lsp_config.server.clone().unwrap_or_default();

        let resolved = match crate::lsp::resolve::resolve_binary(&entry.command) {
            Ok(r) => r,
            Err(e) => {
                self.status_error = Some(format!("LSP: {e}"));
                return;
            }
        };
        let hash = match crate::lsp::resolve::hash_file(&resolved.canonical_path) {
            Ok(h) => h,
            Err(e) => {
                self.status_error = Some(format!("LSP: {e}"));
                return;
            }
        };

        let store = crate::lsp::trust::TrustStore::load();
        match store.check(&resolved.canonical_path, &hash) {
            crate::lsp::trust::TrustDecision::Approved => {
                self.lsp = crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
            }
            crate::lsp::trust::TrustDecision::Unknown => {
                self.pending_lsp_approval = Some(PendingLspApproval {
                    server_name,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    display_path: resolved.display_path,
                    canonical_path: resolved.canonical_path,
                    hash,
                    reason: LspApprovalReason::FirstLaunch,
                });
            }
            crate::lsp::trust::TrustDecision::HashMismatch { previous_hash } => {
                self.pending_lsp_approval = Some(PendingLspApproval {
                    server_name,
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    display_path: resolved.display_path,
                    canonical_path: resolved.canonical_path,
                    hash,
                    reason: LspApprovalReason::BinaryChanged { previous_hash },
                });
            }
        }
    }
    /// Whether the currently configured LSP binary still matches its trust-store
    /// entry. Used by the crash-recovery path to decide between an in-place
    /// restart and re-routing through the approval gate.
    pub(super) fn lsp_binary_still_trusted(&self) -> bool {
        let entry = match self.lsp_config.active_server() {
            Some(e) => e,
            None => return false,
        };
        let resolved = match crate::lsp::resolve::resolve_binary(&entry.command) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let hash = match crate::lsp::resolve::hash_file(&resolved.canonical_path) {
            Ok(h) => h,
            Err(_) => return false,
        };
        matches!(
            crate::lsp::trust::TrustStore::load().check(&resolved.canonical_path, &hash),
            crate::lsp::trust::TrustDecision::Approved
        )
    }
    /// Handle input while the LSP-binary approval overlay is shown.
    /// Returns `true` if the action was consumed (always, while modal is open).
    pub(super) fn handle_lsp_approval(&mut self, action: &EditorAction) -> bool {
        match action {
            EditorAction::InsertChar('y') | EditorAction::InsertChar('Y') => {
                if let Some(pending) = self.pending_lsp_approval.take() {
                    let mut store = crate::lsp::trust::TrustStore::load();
                    store.approve(
                        pending.canonical_path,
                        pending.hash,
                        Some(pending.server_name),
                    );
                    store.save();
                    self.lsp =
                        crate::lsp::LspRegistry::start(&self.lsp_config, &self.workspace).ok();
                    self.status_error = Some(
                        "LSP approved; recorded in ~/.config/txt/trusted_binaries.json".into(),
                    );
                }
                true
            }
            EditorAction::InsertChar('n')
            | EditorAction::InsertChar('N')
            | EditorAction::Quit
            | EditorAction::CloseSearch => {
                self.pending_lsp_approval = None;
                self.status_error = Some("LSP not started.".into());
                true
            }
            // Capture every other input while the modal is up.
            _ => true,
        }
    }
}
