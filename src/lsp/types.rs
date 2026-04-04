use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::buffer::cursor::ByteRange;

// ── LSP domain types ─────────────────────────────────────────────────────────

/// LSP Position: 0-based line, 0-based UTF-16 code-unit offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// LSP Range: two positions (inclusive start, exclusive end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// LSP Location: a URI + range.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: LspRange,
}

/// Diagnostic severity levels.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// A single diagnostic message from the LSP server (converted to byte offsets).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnostic {
    pub range: ByteRange,
    pub severity: DiagSeverity,
    pub message: String,
    pub source: Option<String>,
}

/// A text edit to apply to a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: LspRange,
    pub new_text: String,
}

/// A workspace edit that may span multiple files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceEdit {
    /// Map from file URI to list of edits.
    #[serde(default)]
    pub changes: std::collections::HashMap<String, Vec<TextEdit>>,
}

/// Completion item kind (subset of LSP spec).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Keyword = 14,
    Snippet = 15,
    Constant = 21,
}

/// A single completion suggestion.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<CompletionItemKind>,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
    pub filter_text: Option<String>,
    pub text_edit: Option<TextEdit>,
}

/// Code action from the server.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    pub title: String,
    pub edit: Option<WorkspaceEdit>,
}

/// Decoded semantic token with absolute byte positions.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTokenSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub token_type: u32,
    pub modifiers: u32,
}

// ── Position conversion ──────────────────────────────────────────────────────

/// Convert an LSP position (line, UTF-16 code-unit offset) to a byte offset
/// in the rope. Returns `None` if the position is out of range.
#[allow(dead_code)]
pub fn lsp_position_to_byte_offset(rope: &Rope, pos: LspPosition) -> Option<usize> {
    let line_idx = pos.line as usize;
    if line_idx >= rope.len_lines() {
        return None;
    }

    let line_start_char = rope.line_to_char(line_idx);
    let line = rope.line(line_idx);
    let line_len_chars = line.len_chars();

    // Walk chars in the line, counting UTF-16 code units until we reach
    // the target column.
    let mut utf16_offset: u32 = 0;
    let mut char_idx: usize = 0;

    while char_idx < line_len_chars && utf16_offset < pos.character {
        let ch = rope.char(line_start_char + char_idx);
        utf16_offset += ch.len_utf16() as u32;
        char_idx += 1;
    }

    let target_char = line_start_char + char_idx;
    Some(rope.char_to_byte(target_char))
}

/// Convert a byte offset in the rope to an LSP position (line, UTF-16 offset).
pub fn byte_offset_to_lsp_position(rope: &Rope, byte_offset: usize) -> LspPosition {
    let byte_offset = byte_offset.min(rope.len_bytes());
    let char_idx = rope.byte_to_char(byte_offset);
    let line_idx = rope.char_to_line(char_idx);
    let line_start_char = rope.line_to_char(line_idx);

    // Count UTF-16 code units from line start to char_idx.
    let mut utf16_offset: u32 = 0;
    for ci in line_start_char..char_idx {
        let ch = rope.char(ci);
        utf16_offset += ch.len_utf16() as u32;
    }

    LspPosition {
        line: line_idx as u32,
        character: utf16_offset,
    }
}

/// Convert an LSP range to a `ByteRange`. Returns `None` if either endpoint
/// is out of range.
#[allow(dead_code)]
pub fn lsp_range_to_byte_range(rope: &Rope, range: LspRange) -> Option<ByteRange> {
    let start = lsp_position_to_byte_offset(rope, range.start)?;
    let end = lsp_position_to_byte_offset(rope, range.end)?;
    Some(ByteRange { start, end })
}

/// Convert a `ByteRange` to an LSP range.
#[allow(dead_code)]
pub fn byte_range_to_lsp_range(rope: &Rope, range: ByteRange) -> LspRange {
    LspRange {
        start: byte_offset_to_lsp_position(rope, range.start),
        end: byte_offset_to_lsp_position(rope, range.end),
    }
}

/// Convert a file path to a `file://` URI.
pub fn path_to_uri(path: &std::path::Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", canonical.display())
}

/// Convert a `file://` URI to a `PathBuf`.
#[allow(dead_code)]
pub fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    uri.strip_prefix("file://").map(std::path::PathBuf::from)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_position_conversion() {
        let rope = Rope::from_str("hello\nworld\n");
        // "world" starts at line 1, char 0
        let pos = LspPosition {
            line: 1,
            character: 3,
        };
        let byte = lsp_position_to_byte_offset(&rope, pos).unwrap();
        // "hel" on line 0 is 6 bytes, "wor" is 3 bytes -> byte 9
        assert_eq!(byte, 9);
        let back = byte_offset_to_lsp_position(&rope, byte);
        assert_eq!(back, pos);
    }

    #[test]
    fn multibyte_utf8_position_conversion() {
        // 'a' = 1 byte, 1 UTF-16 code unit
        // 'ä' (U+00E4) = 2 bytes UTF-8, 1 UTF-16 code unit
        // '€' (U+20AC) = 3 bytes UTF-8, 1 UTF-16 code unit
        let rope = Rope::from_str("aä€\n");
        // After 'a' and 'ä': UTF-16 offset = 2, byte offset = 3
        let pos = LspPosition {
            line: 0,
            character: 2,
        };
        let byte = lsp_position_to_byte_offset(&rope, pos).unwrap();
        assert_eq!(byte, 3); // 'a' (1 byte) + 'ä' (2 bytes) = 3
        let back = byte_offset_to_lsp_position(&rope, byte);
        assert_eq!(back, pos);
    }

    #[test]
    fn emoji_surrogate_pair_position() {
        // '😀' (U+1F600) = 4 bytes UTF-8, 2 UTF-16 code units (surrogate pair)
        let rope = Rope::from_str("a😀b\n");
        // After 'a' + '😀': UTF-16 offset = 1 + 2 = 3, byte offset = 1 + 4 = 5
        let pos = LspPosition {
            line: 0,
            character: 3,
        };
        let byte = lsp_position_to_byte_offset(&rope, pos).unwrap();
        assert_eq!(byte, 5); // 'a' (1) + '😀' (4) = 5
        let back = byte_offset_to_lsp_position(&rope, byte);
        assert_eq!(back, pos);
    }

    #[test]
    fn position_at_line_end() {
        let rope = Rope::from_str("abc\ndef\n");
        let pos = LspPosition {
            line: 0,
            character: 3,
        };
        let byte = lsp_position_to_byte_offset(&rope, pos).unwrap();
        assert_eq!(byte, 3);
    }

    #[test]
    fn position_past_end_of_line_clamps() {
        let rope = Rope::from_str("ab\ncd\n");
        let pos = LspPosition {
            line: 0,
            character: 100,
        };
        let byte = lsp_position_to_byte_offset(&rope, pos).unwrap();
        // Clamps to end of line (past 'b' and '\n' = byte 3)
        // The char walk stops at line end, so char_idx = 3 (past 'a','b','\n')
        assert!(byte <= 3);
    }

    #[test]
    fn out_of_range_line_returns_none() {
        let rope = Rope::from_str("abc\n");
        let pos = LspPosition {
            line: 5,
            character: 0,
        };
        assert!(lsp_position_to_byte_offset(&rope, pos).is_none());
    }

    #[test]
    fn lsp_range_conversion() {
        let rope = Rope::from_str("hello\nworld\n");
        let range = LspRange {
            start: LspPosition {
                line: 0,
                character: 1,
            },
            end: LspPosition {
                line: 0,
                character: 4,
            },
        };
        let br = lsp_range_to_byte_range(&rope, range).unwrap();
        assert_eq!(br.start, 1);
        assert_eq!(br.end, 4);
    }

    #[test]
    fn path_uri_roundtrip() {
        let path = std::env::temp_dir().join("test.rs");
        let uri = path_to_uri(&path);
        assert!(uri.starts_with("file:"));
        let back = uri_to_path(&uri).unwrap();
        // Compare canonicalized forms since path_to_uri canonicalizes.
        let expected = path.canonicalize().unwrap_or(path);
        assert_eq!(back, expected);
    }

    #[test]
    fn diag_severity_ordering() {
        assert!(DiagSeverity::Error < DiagSeverity::Warning);
        assert!(DiagSeverity::Warning < DiagSeverity::Information);
        assert!(DiagSeverity::Information < DiagSeverity::Hint);
    }
}
