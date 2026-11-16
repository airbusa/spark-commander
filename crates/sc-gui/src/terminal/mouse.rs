//! The mouse reported to a program that asks for it (mc, htop, vim), as
//! xterm's control sequences specify: clicks, drags or every move, and the
//! wheel; in the SGR form, or the old one (UTF-8 coordinates if asked).

use alacritty_terminal::term::TermMode;
use gpui_kit::{Modifiers, MouseButton};

pub enum Event {
    /// Pressed, or released.
    Button(MouseButton, bool),
    /// A move, with the button held if any.
    Move(Option<MouseButton>),
    /// A wheel step: up, or down.
    Wheel(bool),
}

/// The bytes for `event` at the screen's `column` and `line` (from 0);
/// none when the program did not ask for it.
pub fn report(event: Event, column: usize, line: usize, modifiers: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    let asked = mode.intersects(TermMode::MOUSE_MODE);
    let (button, pressed) = match event {
        Event::Button(button, pressed) if asked => (number(button)?, pressed),
        Event::Wheel(up) if asked => (64 + !up as u8, true),
        // Every move, or a move with a button held (a drag), as asked; 32 marks a move.
        Event::Move(held) if mode.contains(TermMode::MOUSE_MOTION) || held.is_some() && mode.contains(TermMode::MOUSE_DRAG) => {
            (32 + held.map_or(Some(3), number)?, true)
        }
        _ => return None,
    };
    // Shift 4, Meta (Alt) 8, Control 16.
    let held = (modifiers.shift as u8) << 2 | (modifiers.alt as u8) << 3 | (modifiers.control as u8) << 4;
    let (column, line) = (column + 1, line + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{};{column};{line}{end}", button | held).into_bytes());
    }
    // The old form tells a release by button 3, not which one.
    let button = if pressed { button } else { 3 };
    let mut sent = vec![0x1b, b'[', b'M', 32 + (button | held)];
    for at in [column, line] {
        sent.extend(coordinate(at, mode.contains(TermMode::UTF8_MOUSE))?);
    }
    Some(sent)
}

fn number(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

/// A coordinate from 1, plus 32: one byte, which ends at 223; in UTF-8 mode
/// a character, which goes to 2047.
fn coordinate(at: usize, utf8: bool) -> Option<Vec<u8>> {
    let value = u32::try_from(at + 32).ok()?;
    match utf8 {
        false if value <= 255 => Some(vec![value as u8]),
        true if value < 0x800 => Some(char::from_u32(value)?.to_string().into_bytes()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_as_asked() {
        let none = Modifiers::default();
        assert_eq!(report(Event::Wheel(true), 0, 0, none, TermMode::NONE), None, "not asked");
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(report(Event::Button(MouseButton::Left, false), 4, 2, none, sgr).unwrap(), b"\x1b[<0;5;3m");
        assert_eq!(report(Event::Wheel(false), 0, 0, Modifiers { control: true, ..none }, sgr).unwrap(), b"\x1b[<81;1;1M");
        assert_eq!(report(Event::Button(MouseButton::Left, true), 0, 0, none, TermMode::MOUSE_REPORT_CLICK).unwrap(), b"\x1b[M !!");
        assert_eq!(report(Event::Button(MouseButton::Right, false), 0, 0, none, TermMode::MOUSE_REPORT_CLICK).unwrap(), b"\x1b[M#!!");
        assert_eq!(report(Event::Move(None), 0, 0, none, TermMode::MOUSE_DRAG), None, "drags only");
        assert_eq!(report(Event::Move(Some(MouseButton::Left)), 0, 0, none, TermMode::MOUSE_DRAG).unwrap(), b"\x1b[M@!!");
        assert_eq!(report(Event::Button(MouseButton::Left, true), 300, 0, none, TermMode::MOUSE_REPORT_CLICK), None, "too far for a byte");
        let utf8 = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        assert_eq!(report(Event::Button(MouseButton::Left, true), 300, 0, none, utf8).unwrap(), "\x1b[M \u{14d}!".as_bytes());
    }
}
