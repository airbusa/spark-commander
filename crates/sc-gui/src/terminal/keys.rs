//! The bytes a key sends to the program in the terminal, as xterm's control
//! sequences specify them (invisible-island.net/xterm/ctlseqs): PC-style
//! function keys, the cursor keys' normal and application modes, control
//! characters, and Alt as a leading ESC.

use alacritty_terminal::term::TermMode;
use gpui_kit::Keystroke;

/// How a key that has no character of its own is sent.
enum Sequence {
    /// The cursor keys, Home and End: `CSI x`, or `SS3 x` in application mode.
    Cursor(u8),
    /// F1 to F4: `SS3 x` alone, `CSI 1;m x` with modifiers.
    Function(u8),
    /// The editing keys and F5 on: `CSI n ~`.
    Numbered(u8),
}

fn sequence(name: &str) -> Option<Sequence> {
    use Sequence::*;
    Some(match name {
        "up" => Cursor(b'A'),
        "down" => Cursor(b'B'),
        "right" => Cursor(b'C'),
        "left" => Cursor(b'D'),
        "home" => Cursor(b'H'),
        "end" => Cursor(b'F'),
        "f1" => Function(b'P'),
        "f2" => Function(b'Q'),
        "f3" => Function(b'R'),
        "f4" => Function(b'S'),
        "insert" => Numbered(2),
        "delete" => Numbered(3),
        "pageup" => Numbered(5),
        "pagedown" => Numbered(6),
        _ => {
            // F5 to F20, numbered with gaps where the VT220 had other keys.
            const NUMBERS: [u8; 16] = [15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34];
            let n: usize = name.strip_prefix('f')?.parse().ok()?;
            Numbered(*NUMBERS.get(n.checked_sub(5)?)?)
        }
    })
}

/// What `key` sends in `mode`; none for a key that types text (that comes
/// through the input method) or that sends nothing.
pub fn bytes(key: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let held = key.modifiers;
    // The system's own shortcuts (⌘, the Windows key) are no terminal input.
    if held.platform {
        return None;
    }
    let (shift, alt, ctrl) = (held.shift, held.alt, held.control);
    let alone = !shift && !alt && !ctrl;
    let fixed: Option<&[u8]> = match key.key.as_str() {
        "enter" if alone => Some(b"\r"),
        "enter" if shift && !alt && !ctrl => Some(b"\n"),
        "enter" if alt && !shift && !ctrl => Some(b"\x1b\r"),
        "tab" if alone => Some(b"\t"),
        "tab" if shift && !alt && !ctrl => Some(b"\x1b[Z"),
        "escape" if alone => Some(b"\x1b"),
        "backspace" | "back" if !alt && !ctrl => Some(b"\x7f"),
        "backspace" if ctrl && !alt && !shift => Some(b"\x08"),
        "backspace" if alt && !ctrl && !shift => Some(b"\x1b\x7f"),
        "space" if ctrl && !alt && !shift => Some(b"\0"),
        "enter" | "tab" | "escape" | "backspace" | "back" | "space" => return None,
        _ => None,
    };
    if let Some(fixed) = fixed {
        return Some(fixed.to_vec());
    }
    if let Some(sequence) = sequence(&key.key) {
        // xterm's modifier parameter: 1, plus 1 for Shift, 2 for Alt, 4 for Ctrl.
        let modifier = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
        let application = mode.contains(TermMode::APP_CURSOR);
        return Some(match (sequence, alone) {
            (Sequence::Cursor(x), true) if application => vec![0x1b, b'O', x],
            (Sequence::Cursor(x), true) => vec![0x1b, b'[', x],
            (Sequence::Function(x), true) => vec![0x1b, b'O', x],
            (Sequence::Cursor(x) | Sequence::Function(x), false) => format!("\x1b[1;{modifier}{}", x as char).into_bytes(),
            (Sequence::Numbered(n), true) => format!("\x1b[{n}~").into_bytes(),
            (Sequence::Numbered(n), false) => format!("\x1b[{n};{modifier}~").into_bytes(),
        });
    }
    let mut chars = key.key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if !c.is_ascii() {
        return None;
    }
    if ctrl && !alt {
        return control(c).map(|byte| vec![byte]);
    }
    // Alt as Meta: ESC before the key. On macOS Option types characters instead.
    if alt && !cfg!(target_os = "macos") {
        let after = match (shift, ctrl) {
            (true, _) => c.to_ascii_uppercase() as u8,
            (false, true) if c.is_ascii_lowercase() => control(c)?,
            (false, true) => return None,
            (false, false) => c as u8,
        };
        return Some(vec![0x1b, after]);
    }
    None
}

/// The control character Ctrl makes of `c`: Ctrl+A is 1, Ctrl+[ is ESC,
/// Ctrl+? is DEL; none for others.
fn control(c: char) -> Option<u8> {
    match c.to_ascii_uppercase() {
        '?' => Some(0x7f),
        c @ '@'..='_' => Some(c as u8 & 0x1f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Modifiers;

    fn sent(text: &str, mode: TermMode) -> Option<Vec<u8>> {
        bytes(&Keystroke::parse(text).unwrap(), mode)
    }

    #[test]
    fn sends_as_xterm() {
        let normal = TermMode::NONE;
        assert_eq!(sent("enter", normal).unwrap(), b"\r");
        assert_eq!(sent("shift-enter", normal).unwrap(), b"\n");
        assert_eq!(sent("alt-enter", normal).unwrap(), b"\x1b\r");
        assert_eq!(sent("shift-tab", normal).unwrap(), b"\x1b[Z");
        assert_eq!(sent("backspace", normal).unwrap(), b"\x7f");
        assert_eq!(sent("ctrl-backspace", normal).unwrap(), b"\x08");
        assert_eq!(sent("alt-backspace", normal).unwrap(), b"\x1b\x7f");
        assert_eq!(sent("ctrl-space", normal).unwrap(), b"\0");
        assert_eq!(sent("up", normal).unwrap(), b"\x1b[A");
        assert_eq!(sent("up", TermMode::APP_CURSOR).unwrap(), b"\x1bOA");
        assert_eq!(sent("end", TermMode::APP_CURSOR).unwrap(), b"\x1bOF");
        assert_eq!(sent("ctrl-left", normal).unwrap(), b"\x1b[1;5D");
        assert_eq!(sent("shift-alt-home", normal).unwrap(), b"\x1b[1;4H");
        assert_eq!(sent("f1", normal).unwrap(), b"\x1bOP");
        assert_eq!(sent("ctrl-f4", normal).unwrap(), b"\x1b[1;5S");
        assert_eq!(sent("f5", normal).unwrap(), b"\x1b[15~");
        assert_eq!(sent("f12", normal).unwrap(), b"\x1b[24~");
        assert_eq!(sent("f20", normal).unwrap(), b"\x1b[34~");
        assert_eq!(sent("shift-f10", normal).unwrap(), b"\x1b[21;2~");
        assert_eq!(sent("pagedown", normal).unwrap(), b"\x1b[6~");
        assert_eq!(sent("ctrl-delete", normal).unwrap(), b"\x1b[3;5~");
        assert_eq!(sent("ctrl-a", normal).unwrap(), [1]);
        assert_eq!(sent("ctrl-z", normal).unwrap(), [26]);
        assert_eq!(sent("ctrl-[", normal).unwrap(), [0x1b]);
        assert_eq!(sent("ctrl-?", normal).unwrap(), [0x7f]);
        assert_eq!(sent("a", normal), None, "typed text comes as text");
        assert_eq!(sent("f21", normal), None);
        if !cfg!(target_os = "macos") {
            assert_eq!(sent("alt-b", normal).unwrap(), b"\x1bb");
            assert_eq!(sent("alt-shift-b", normal).unwrap(), b"\x1bB");
            assert_eq!(sent("ctrl-alt-c", normal).unwrap(), [0x1b, 3]);
        }
        let platform = Keystroke { modifiers: Modifiers { platform: true, ..Modifiers::default() }, ..Keystroke::parse("up").unwrap() };
        assert_eq!(bytes(&platform, normal), None);
    }
}
