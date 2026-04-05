use serde_json::Value;

// ── Client capabilities ──────────────────────────────────────────────────────

/// Build the `ClientCapabilities` JSON sent during `initialize`.
///
/// Declares which features txt supports receiving from the server.
pub fn client_capabilities() -> Value {
    serde_json::json!({
        "textDocument": {
            "synchronization": {
                "dynamicRegistration": false,
                "didSave": true
            },
            "completion": {
                "completionItem": {
                    "snippetSupport": false,
                    "deprecatedSupport": true,
                    "labelDetailsSupport": true
                },
                "completionItemKind": {
                    "valueSet": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25]
                }
            },
            "hover": {
                "contentFormat": ["plaintext"]
            },
            "definition": {
                "dynamicRegistration": false
            },
            "references": {
                "dynamicRegistration": false
            },
            "rename": {
                "dynamicRegistration": false,
                "prepareSupport": true
            },
            "codeAction": {
                "dynamicRegistration": false,
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": ["quickfix", "refactor", "source"]
                    }
                }
            },
            "publishDiagnostics": {
                "relatedInformation": false,
                "tagSupport": { "valueSet": [1, 2] }
            },
            "semanticTokens": {
                "dynamicRegistration": false,
                "requests": {
                    "full": true,
                    "range": false
                },
                "tokenTypes": [
                    "namespace", "type", "class", "enum", "interface",
                    "struct", "typeParameter", "parameter", "variable",
                    "property", "enumMember", "event", "function", "method",
                    "macro", "keyword", "modifier", "comment", "string",
                    "number", "regexp", "operator", "decorator"
                ],
                "tokenModifiers": [
                    "declaration", "definition", "readonly", "static",
                    "deprecated", "abstract", "async", "modification",
                    "documentation", "defaultLibrary"
                ],
                "formats": ["relative"],
                "multilineTokenSupport": false,
                "overlappingTokenSupport": false
            }
        },
        "window": {
            "workDoneProgress": false
        }
    })
}

// ── Server capabilities (parsed subset) ──────────────────────────────────────

/// Parsed subset of `ServerCapabilities` that we actually use.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub completion_provider: bool,
    pub completion_trigger_chars: Vec<char>,
    pub hover_provider: bool,
    pub definition_provider: bool,
    pub references_provider: bool,
    pub rename_provider: bool,
    pub code_action_provider: bool,
    pub semantic_tokens_provider: bool,
    pub text_document_sync_kind: TextDocumentSyncKind,
}

/// How the server wants document changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDocumentSyncKind {
    None = 0,
    #[default]
    Full = 1,
    Incremental = 2,
}

/// Parse `ServerCapabilities` from the `initialize` response result.
pub fn parse_server_capabilities(result: &Value) -> ServerCapabilities {
    let caps = result.get("capabilities").unwrap_or(result);
    let mut sc = ServerCapabilities::default();

    // Completion
    if let Some(cp) = caps.get("completionProvider") {
        sc.completion_provider = true;
        if let Some(triggers) = cp.get("triggerCharacters").and_then(|v| v.as_array()) {
            for t in triggers {
                if let Some(s) = t.as_str() {
                    for ch in s.chars() {
                        sc.completion_trigger_chars.push(ch);
                    }
                }
            }
        }
    }

    // Hover
    sc.hover_provider = caps
        .get("hoverProvider")
        .map(|v| v.as_bool().unwrap_or(v.is_object()))
        .unwrap_or(false);

    // Definition
    sc.definition_provider = caps
        .get("definitionProvider")
        .map(|v| v.as_bool().unwrap_or(v.is_object()))
        .unwrap_or(false);

    // References
    sc.references_provider = caps
        .get("referencesProvider")
        .map(|v| v.as_bool().unwrap_or(v.is_object()))
        .unwrap_or(false);

    // Rename
    sc.rename_provider = caps
        .get("renameProvider")
        .map(|v| v.as_bool().unwrap_or(v.is_object()))
        .unwrap_or(false);

    // Code actions
    sc.code_action_provider = caps
        .get("codeActionProvider")
        .map(|v| v.as_bool().unwrap_or(v.is_object()))
        .unwrap_or(false);

    // Semantic tokens
    sc.semantic_tokens_provider = caps.get("semanticTokensProvider").is_some();

    // Text document sync
    if let Some(sync) = caps.get("textDocumentSync") {
        if let Some(kind) = sync.as_u64() {
            sc.text_document_sync_kind = match kind {
                0 => TextDocumentSyncKind::None,
                2 => TextDocumentSyncKind::Incremental,
                _ => TextDocumentSyncKind::Full,
            };
        } else if let Some(kind) = sync.get("change").and_then(|v| v.as_u64()) {
            sc.text_document_sync_kind = match kind {
                0 => TextDocumentSyncKind::None,
                2 => TextDocumentSyncKind::Incremental,
                _ => TextDocumentSyncKind::Full,
            };
        }
    }

    sc
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_capabilities_is_valid_json() {
        let caps = client_capabilities();
        assert!(caps.get("textDocument").is_some());
        assert!(caps["textDocument"]["completion"].is_object());
    }

    #[test]
    fn parse_rust_analyzer_style_capabilities() {
        let result = serde_json::json!({
            "capabilities": {
                "completionProvider": {
                    "triggerCharacters": [".", ":", "<"],
                    "resolveProvider": true
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true,
                "renameProvider": { "prepareProvider": true },
                "codeActionProvider": { "codeActionKinds": ["quickfix"] },
                "semanticTokensProvider": {
                    "full": true,
                    "legend": { "tokenTypes": [], "tokenModifiers": [] }
                },
                "textDocumentSync": {
                    "openClose": true,
                    "change": 2
                }
            }
        });

        let sc = parse_server_capabilities(&result);
        assert!(sc.completion_provider);
        assert_eq!(sc.completion_trigger_chars, vec!['.', ':', '<']);
        assert!(sc.hover_provider);
        assert!(sc.definition_provider);
        assert!(sc.references_provider);
        assert!(sc.rename_provider);
        assert!(sc.code_action_provider);
        assert!(sc.semantic_tokens_provider);
        assert_eq!(
            sc.text_document_sync_kind,
            TextDocumentSyncKind::Incremental
        );
    }

    #[test]
    fn parse_minimal_capabilities() {
        let result = serde_json::json!({
            "capabilities": {
                "textDocumentSync": 1
            }
        });

        let sc = parse_server_capabilities(&result);
        assert!(!sc.completion_provider);
        assert!(!sc.hover_provider);
        assert_eq!(sc.text_document_sync_kind, TextDocumentSyncKind::Full);
    }

    #[test]
    fn parse_empty_capabilities() {
        let result = serde_json::json!({});
        let sc = parse_server_capabilities(&result);
        assert!(!sc.completion_provider);
        assert!(!sc.hover_provider);
    }
}
