//! Encode `Key` values as the byte sequences a terminal sends to an
//! application.  Used by the PTY harness to drive the real `txt` binary.
//!
//! Crossterm pushes `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES`
//! at startup (see `src/main.rs`), so the binary parses both legacy xterm
//! sequences *and* the kitty CSI-u protocol.  We send legacy where it
//! suffices and reach for kitty only for combinations that can't be
//! expressed otherwise (Ctrl+Shift+letter, Ctrl+digit, Shift+F3).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    /// Modifier with Up/Down/Left/Right/Home/End.  Use [`Key::Ctrl`] for
    /// `Ctrl+letter` style chords.
    CtrlArrow(Arrow),
    ShiftArrow(Arrow),
    Ctrl(char),
    /// Ctrl+Shift+letter via the kitty CSI-u sequence.
    CtrlShift(char),
    Alt(char),
    /// Ctrl+digit via the kitty CSI-u sequence; ASCII '0'..='9'.
    CtrlDigit(char),
    /// Shift+F<N> via the legacy xterm modifier encoding (mod=2).
    ShiftF(u8),
    /// Ctrl+Backspace (kitty CSI-u, codepoint 127).
    CtrlBackspace,
    /// Ctrl+Delete (kitty CSI-u, functional codepoint 57426).
    CtrlDelete,
    /// Ctrl+PageUp (legacy xterm modifier encoding).
    CtrlPageUp,
    /// Ctrl+PageDown (legacy xterm modifier encoding).
    CtrlPageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

impl Arrow {
    fn final_byte(self) -> u8 {
        match self {
            Arrow::Up => b'A',
            Arrow::Down => b'B',
            Arrow::Right => b'C',
            Arrow::Left => b'D',
            Arrow::Home => b'H',
            Arrow::End => b'F',
        }
    }
}

pub fn key_to_bytes(k: Key) -> Vec<u8> {
    match k {
        Key::Char(c) => {
            let mut s = String::new();
            s.push(c);
            s.into_bytes()
        }
        Key::Enter => b"\r".to_vec(),
        Key::Esc => b"\x1b".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace => b"\x7f".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::F(n) => f_key(n),
        Key::CtrlArrow(a) => format!("\x1b[1;5{}", a.final_byte() as char).into_bytes(),
        Key::ShiftArrow(a) => format!("\x1b[1;2{}", a.final_byte() as char).into_bytes(),
        Key::Ctrl(c) => ctrl_byte(c),
        Key::CtrlShift(c) => {
            // kitty CSI-u: \x1b[<codepoint>;<modifiers>u   mods = ctrl|shift = 5|1+1 = ?
            // kitty modifier formula: 1 + (shift)1 + (alt)2 + (ctrl)4 = 6 for ctrl+shift
            let code = c.to_ascii_lowercase() as u32;
            format!("\x1b[{code};6u").into_bytes()
        }
        Key::Alt(c) => {
            let mut v = vec![0x1b];
            let mut s = String::new();
            s.push(c);
            v.extend_from_slice(s.as_bytes());
            v
        }
        Key::CtrlDigit(c) => {
            assert!(c.is_ascii_digit(), "CtrlDigit expects ASCII '0'..='9'");
            let code = c as u32;
            format!("\x1b[{code};5u").into_bytes()
        }
        Key::ShiftF(n) => shift_f_key(n),
        Key::CtrlBackspace => b"\x1b[127;5u".to_vec(),
        Key::CtrlDelete => b"\x1b[57426;5u".to_vec(),
        Key::CtrlPageUp => b"\x1b[5;5~".to_vec(),
        Key::CtrlPageDown => b"\x1b[6;5~".to_vec(),
    }
}

fn ctrl_byte(c: char) -> Vec<u8> {
    // Ctrl+letter is encoded as a single C0 control byte; everything else
    // uses kitty CSI-u because crossterm's legacy parser maps `\x1C..=\x1F`
    // onto digits, not punctuation.
    let lc = c.to_ascii_lowercase();
    match lc {
        'a'..='z' => vec![(lc as u8) - b'a' + 1],
        _ => {
            let code = lc as u32;
            format!("\x1b[{code};5u").into_bytes()
        }
    }
}

fn f_key(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => panic!("F{n} not supported"),
    }
}

fn shift_f_key(n: u8) -> Vec<u8> {
    // F1..F4 with the leading `1` parameter omitted to avoid crossterm's
    // CPR (cursor-position-report) dispatch on `\x1b[1;2R` and `\x1b[1;2P`.
    // Crossterm's `parse_csi_modifier_key_code` accepts the leading semicolon.
    match n {
        1 => b"\x1b[;2P".to_vec(),
        2 => b"\x1b[;2Q".to_vec(),
        3 => b"\x1b[;2R".to_vec(),
        4 => b"\x1b[;2S".to_vec(),
        5 => b"\x1b[15;2~".to_vec(),
        6 => b"\x1b[17;2~".to_vec(),
        7 => b"\x1b[18;2~".to_vec(),
        8 => b"\x1b[19;2~".to_vec(),
        9 => b"\x1b[20;2~".to_vec(),
        10 => b"\x1b[21;2~".to_vec(),
        11 => b"\x1b[23;2~".to_vec(),
        12 => b"\x1b[24;2~".to_vec(),
        _ => panic!("Shift+F{n} not supported"),
    }
}
