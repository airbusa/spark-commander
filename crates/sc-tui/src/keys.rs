//! Terminal keys in the core's key syntax (`ctrl-shift-f4`), so the
//! terminal takes the same bindings as the window.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventState, KeyModifiers};

pub fn name(key: &KeyEvent) -> Option<String> {
    let mut shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let keypad = key.state.contains(KeyEventState::KEYPAD);
    let base = match key.code {
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char('+') if keypad => "add".into(),
        KeyCode::Char('-') if keypad => "subtract".into(),
        KeyCode::Char('*') if keypad => "multiply".into(),
        KeyCode::Char('/') if keypad => "divide".into(),
        KeyCode::Char(c) if c.is_ascii_uppercase() => {
            shift = true;
            c.to_ascii_lowercase().to_string()
        }
        // The character says the shift already: `+`, not `shift-=`.
        KeyCode::Char(c) => {
            shift = false;
            c.to_string()
        }
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "escape".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => {
            shift = true;
            "tab".into()
        }
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        _ => return None,
    };
    let mut name = String::new();
    for (on, prefix) in [
        (key.modifiers.contains(KeyModifiers::CONTROL), "ctrl-"),
        (key.modifiers.contains(KeyModifiers::ALT), "alt-"),
        (shift, "shift-"),
    ] {
        if on {
            name.push_str(prefix);
        }
    }
    name.push_str(&base);
    Some(name)
}

/// A key as menus show it: `ctrl-shift-f4`, `Ctrl+Shift+F4`.
pub fn shown(key: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = key;
    while let Some((modifier, after)) = rest.split_once('-').filter(|(modifier, after)| !after.is_empty() && ["ctrl", "alt", "shift", "cmd"].contains(modifier)) {
        parts.push(format!("{}{}", modifier[..1].to_uppercase(), &modifier[1..]));
        rest = after;
    }
    let base = match rest {
        "pageup" => "PgUp".to_owned(),
        "pagedown" => "PgDn".to_owned(),
        "escape" => "Esc".to_owned(),
        "add" => "Num +".to_owned(),
        "subtract" => "Num -".to_owned(),
        "multiply" => "Num *".to_owned(),
        "divide" => "Num /".to_owned(),
        _ => rest.chars().next().map(|first| first.to_uppercase().chain(rest.chars().skip(1)).collect()).unwrap_or_default(),
    };
    parts.push(base);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_keys_as_menus_do() {
        assert_eq!(shown("ctrl-shift-f4"), "Ctrl+Shift+F4");
        assert_eq!(shown("alt-d"), "Alt+D");
        assert_eq!(shown("ctrl--"), "Ctrl+-");
        assert_eq!(shown("pagedown"), "PgDn");
    }

    fn named(code: KeyCode, modifiers: KeyModifiers) -> String {
        name(&KeyEvent::new(code, modifiers)).unwrap()
    }

    #[test]
    fn names_keys_as_the_core_does() {
        assert_eq!(named(KeyCode::F(3), KeyModifiers::NONE), "f3");
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(named(KeyCode::F(4), ctrl_shift), "ctrl-shift-f4");
        assert_eq!(
            named(KeyCode::Char('W'), KeyModifiers::CONTROL),
            "ctrl-shift-w"
        );
        assert_eq!(named(KeyCode::Char('+'), KeyModifiers::SHIFT), "+");
        assert_eq!(named(KeyCode::Char(' '), KeyModifiers::NONE), "space");
        assert_eq!(named(KeyCode::BackTab, KeyModifiers::SHIFT), "shift-tab");
        let mut add = KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE);
        add.state = KeyEventState::KEYPAD;
        assert_eq!(name(&add).unwrap(), "add");
    }
}
