use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 message types ──────────────────────────────────────────────

/// A JSON-RPC request (client → server or server → client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMessage {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC response (server → client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC notification (no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ── Builders ─────────────────────────────────────────────────────────────────

impl RequestMessage {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        }
    }
}

impl NotificationMessage {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        }
    }
}

// ── Incoming message classification ──────────────────────────────────────────

/// A decoded message from the server — either a response to our request or a
/// server-initiated notification.
#[derive(Debug)]
pub enum IncomingMessage {
    Response(ResponseMessage),
    Notification(NotificationMessage),
}

/// Classify a raw JSON value into a response or notification.
///
/// Responses have an `id` field. Notifications have a `method` field but no `id`.
pub fn classify_incoming(value: &Value) -> Option<IncomingMessage> {
    if value.get("id").is_some() && value.get("method").is_none() {
        // Response to our request
        let resp: ResponseMessage = serde_json::from_value(value.clone()).ok()?;
        Some(IncomingMessage::Response(resp))
    } else if value.get("method").is_some() && value.get("id").is_none() {
        // Server notification
        let notif: NotificationMessage = serde_json::from_value(value.clone()).ok()?;
        Some(IncomingMessage::Notification(notif))
    } else {
        // Server request (has both id + method) — not handled yet
        None
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization() {
        let req = RequestMessage::new(
            1,
            "initialize",
            Some(serde_json::json!({"capabilities": {}})),
        );
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn notification_serialization() {
        let notif = NotificationMessage::new("initialized", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"method\":\"initialized\""));
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn classify_response() {
        let val = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"capabilities": {}}
        });
        match classify_incoming(&val) {
            Some(IncomingMessage::Response(r)) => {
                assert_eq!(r.id, 1);
                assert!(r.result.is_some());
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn classify_notification() {
        let val = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///test.rs", "diagnostics": []}
        });
        match classify_incoming(&val) {
            Some(IncomingMessage::Notification(n)) => {
                assert_eq!(n.method, "textDocument/publishDiagnostics");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn classify_error_response() {
        let val = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "error": {"code": -32600, "message": "Invalid request"}
        });
        match classify_incoming(&val) {
            Some(IncomingMessage::Response(r)) => {
                assert_eq!(r.id, 5);
                assert!(r.error.is_some());
                assert_eq!(r.error.unwrap().code, -32600);
            }
            _ => panic!("Expected Response with error"),
        }
    }
}
