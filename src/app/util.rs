use std::path::PathBuf;

use crate::input::action::{Direction, EditorAction, ScrollDir};

/// The scroll amount for a single scroll-wheel tick or Ctrl+Up/Down.
pub(super) const SCROLL_LINES: usize = 3;

/// Translate an action into a scroll-position update for a centred-overlay
/// scroll counter. Returns `true` if the action was a scroll input that the
/// caller should consume; on `false` the caller can run its own match arms.
///
/// One line per arrow tick, ten lines per page key, `SCROLL_LINES` per
/// mouse-wheel tick. Saturates at zero; the upper bound is clamped by the
/// renderer.
pub(super) fn scroll_action(action: &EditorAction, scroll: &mut usize) -> bool {
    match action {
        EditorAction::MoveCursor(Direction::Up) => {
            *scroll = scroll.saturating_sub(1);
            true
        }
        EditorAction::MoveCursor(Direction::Down) => {
            *scroll = scroll.saturating_add(1);
            true
        }
        EditorAction::MoveCursorPage(Direction::Up) => {
            *scroll = scroll.saturating_sub(10);
            true
        }
        EditorAction::MoveCursorPage(Direction::Down) => {
            *scroll = scroll.saturating_add(10);
            true
        }
        EditorAction::MouseScroll { dir, .. } => {
            match dir {
                ScrollDir::Up => *scroll = scroll.saturating_sub(SCROLL_LINES),
                ScrollDir::Down => *scroll = scroll.saturating_add(SCROLL_LINES),
                _ => {}
            }
            true
        }
        _ => false,
    }
}

// ── Platform-specific RSS memory reading ─────────────────────────────────────

#[cfg(target_os = "linux")]
pub(super) fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.trim().trim_end_matches(" kB").trim().parse().ok();
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub(super) fn read_rss_kb() -> Option<u64> {
    use std::mem;

    const MACH_TASK_BASIC_INFO: u32 = 20;

    type TaskT = u32;
    type TaskFlavorT = u32;
    type TaskInfoT = u32;
    type MachMsgTypeNumberT = u32;
    type KernReturnT = i32;

    unsafe extern "C" {
        fn mach_task_self() -> TaskT;
        fn task_info(
            target_task: TaskT,
            flavor: TaskFlavorT,
            task_info_out: *mut TaskInfoT,
            task_info_outCnt: *mut MachMsgTypeNumberT,
        ) -> KernReturnT;
    }

    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [u32; 2],
        system_time: [u32; 2],
        policy: i32,
        suspend_count: i32,
    }

    unsafe {
        let mut info: MachTaskBasicInfo = mem::zeroed();
        let mut count = (mem::size_of::<MachTaskBasicInfo>() / mem::size_of::<u32>()) as u32;
        let ret = task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut TaskInfoT,
            &mut count,
        );
        if ret == 0 {
            Some(info.resident_size / 1024)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn read_rss_kb() -> Option<u64> {
    use std::mem;

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            ppsmemCounters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        page_file_usage: usize,
        peak_page_file_usage: usize,
    }

    unsafe {
        let mut pmc: ProcessMemoryCounters = mem::zeroed();
        pmc.cb = mem::size_of::<ProcessMemoryCounters>() as u32;
        let ret = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut pmc,
            mem::size_of::<ProcessMemoryCounters>() as u32,
        );
        if ret != 0 {
            Some(pmc.working_set_size as u64 / 1024)
        } else {
            None
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(super) fn read_rss_kb() -> Option<u64> {
    None
}

// ── Free helpers for LSP ─────────────────────────────────────────────────────

/// Parse an LSP `{ line, character }` JSON value into our `LspPosition`.
pub(super) fn parse_lsp_position(
    val: Option<&serde_json::Value>,
) -> Option<crate::lsp::types::LspPosition> {
    let obj = val?;
    Some(crate::lsp::types::LspPosition {
        line: obj.get("line")?.as_u64()? as u32,
        character: obj.get("character")?.as_u64()? as u32,
    })
}

/// Compare two paths, canonicalizing to handle symlinks / relative paths.
pub(super) fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Map an LSP completion item kind number to a short label.
pub(super) fn completion_kind_label(kind: u64) -> &'static str {
    match kind {
        1 => "txt",
        2 => "fn ",
        3 => "fn ",
        4 => "new",
        5 => "fld",
        6 => "var",
        7 => "cls",
        8 => "ifc",
        9 => "mod",
        10 => "prp",
        14 => "kw ",
        15 => "snp",
        21 => "cst",
        _ => "   ",
    }
}

/// Extract plain text from an LSP hover contents value.
pub(super) fn extract_hover_text(contents: &serde_json::Value) -> String {
    // Can be a string, a { kind, value } MarkupContent, or an array.
    if let Some(s) = contents.as_str() {
        return s.to_string();
    }
    if let Some(value) = contents.get("value").and_then(|v| v.as_str()) {
        return value.to_string();
    }
    if let Some(arr) = contents.as_array() {
        return arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.get("value").and_then(|v| v.as_str()).map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

/// Parse LSP Location or Location[] into (path, line, col) tuples.
pub(super) fn parse_locations(value: &serde_json::Value) -> Vec<(PathBuf, usize, usize)> {
    let locs = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if value.is_object() {
        vec![value.clone()]
    } else {
        return Vec::new();
    };

    locs.iter()
        .filter_map(|loc| {
            let uri = loc.get("uri")?.as_str()?;
            let path = crate::lsp::types::uri_to_path(uri)?;
            let range = loc.get("range")?;
            let start = range.get("start")?;
            let line = start.get("line")?.as_u64()? as usize;
            let col = start.get("character")?.as_u64()? as usize;
            Some((path, line, col))
        })
        .collect()
}

/// Extract the word under the cursor at a byte offset.
pub(super) fn extract_word_at(text: &str, byte_offset: usize) -> String {
    let bytes = text.as_bytes();
    let mut start = byte_offset;
    let mut end = byte_offset;
    while start > 0 && ((bytes[start - 1] as char).is_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    while end < bytes.len() && ((bytes[end] as char).is_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    text[start..end].to_string()
}
