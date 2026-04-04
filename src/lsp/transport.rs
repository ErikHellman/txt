use std::io::{BufRead, BufReader, Read, Write};

use anyhow::{Context, Result, bail};
use serde_json::Value;

// ── Content-Length framed reader ─────────────────────────────────────────────

/// Read a single LSP message from a stream using Content-Length framing.
///
/// The LSP base protocol uses HTTP-style headers terminated by `\r\n\r\n`,
/// followed by a JSON body of exactly `Content-Length` bytes.
pub fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    // Parse headers.
    let mut content_length: Option<usize> = None;
    let mut header_buf = String::new();

    loop {
        header_buf.clear();
        let bytes_read = reader.read_line(&mut header_buf)?;
        if bytes_read == 0 {
            bail!("EOF while reading LSP headers");
        }
        let line = header_buf.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            // Empty line = end of headers.
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .context("invalid Content-Length value")?,
            );
        }
        // Ignore other headers (e.g. Content-Type).
    }

    let len = content_length.context("missing Content-Length header")?;

    // Read the body.
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body).context("invalid JSON in LSP body")?;
    Ok(value)
}

// ── Content-Length framed writer ─────────────────────────────────────────────

/// Write a single LSP message to a stream with Content-Length framing.
pub fn write_message(writer: &mut impl Write, value: &Value) -> Result<()> {
    let body = serde_json::to_string(value)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Convenience: serialize a typed message and write it with Content-Length.
pub fn write_json<T: serde::Serialize>(writer: &mut impl Write, msg: &T) -> Result<()> {
    let value = serde_json::to_value(msg)?;
    write_message(writer, &value)
}

/// Convenience: read a message and wrap the stream in a BufReader if needed.
#[allow(dead_code)]
pub fn read_message_from<R: Read>(stream: &mut BufReader<R>) -> Result<Value> {
    read_message(stream)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_simple_message() {
        let original = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "test"});

        // Write
        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, &original).unwrap();

        // Read
        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_message(&mut reader).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn round_trip_with_body() {
        let original = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///test.rs",
                "diagnostics": [
                    {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                     "message": "error here", "severity": 1}
                ]
            }
        });

        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, &original).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_message(&mut reader).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn multiple_messages() {
        let msg1 = serde_json::json!({"id": 1});
        let msg2 = serde_json::json!({"id": 2});

        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, &msg1).unwrap();
        write_message(&mut buf, &msg2).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let d1 = read_message(&mut reader).unwrap();
        let d2 = read_message(&mut reader).unwrap();

        assert_eq!(d1["id"], 1);
        assert_eq!(d2["id"], 2);
    }

    #[test]
    fn eof_returns_error() {
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn missing_content_length_returns_error() {
        // Just header terminator, no Content-Length
        let data = b"\r\n";
        let mut reader = BufReader::new(Cursor::new(data.to_vec()));
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn handles_extra_headers() {
        let body = r#"{"test":true}"#;
        let msg = format!(
            "Content-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
            body.len(),
            body
        );
        let mut reader = BufReader::new(Cursor::new(msg.into_bytes()));
        let val = read_message(&mut reader).unwrap();
        assert_eq!(val["test"], true);
    }

    #[test]
    fn write_json_typed() {
        use crate::lsp::protocol::RequestMessage;
        let req = RequestMessage::new(42, "test/method", None);
        let mut buf: Vec<u8> = Vec::new();
        write_json(&mut buf, &req).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_message(&mut reader).unwrap();
        assert_eq!(decoded["id"], 42);
        assert_eq!(decoded["method"], "test/method");
    }
}
