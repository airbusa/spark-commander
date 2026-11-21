//! Dialogs that ask: lines to type and boxes to tick, framed in the middle.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, Clear, Padding};
use sc_core::t;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::input::Input;
use crate::skin::skin;
use crate::ui::fit;

pub enum Field {
    Text {
        label: String,
        input: Input,
        /// Up and Down bring these back, the latest first.
        history: Vec<String>,
        recalled: Option<usize>,
        /// A password's: shown as dots.
        secret: bool,
    },
    Check {
        label: String,
        on: bool,
    },
    /// Text to read, as a server's fingerprint; never focused.
    Note(String),
}

pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    focus: usize,
    /// A question's answers, and the one Enter gives; none: Enter OK, Esc Cancel.
    buttons: Vec<String>,
    button: usize,
}

#[derive(Debug, PartialEq)]
pub enum Answer {
    Open,
    Ok,
    Cancel,
}

impl Form {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            fields: Vec::new(),
            focus: 0,
            buttons: Vec::new(),
            button: 0,
        }
    }

    /// A question's answers: Left and Right choose, Enter gives `button()`, Esc cancels.
    pub fn buttons(mut self, labels: Vec<String>) -> Self {
        self.buttons = labels;
        self
    }

    pub fn button(&self) -> usize {
        self.button
    }

    /// Into the line being typed, in one line.
    pub fn paste(&mut self, text: &str) {
        if let Some(Field::Text { input, .. }) = self.fields.get_mut(self.focus) {
            input.insert(&text.replace('\n', " "));
        }
    }

    pub fn text(mut self, label: &str, value: &str, history: Vec<String>) -> Self {
        let mut input = Input::default();
        input.insert(value);
        self.fields.push(Field::Text {
            label: label.into(),
            input,
            history,
            recalled: None,
            secret: false,
        });
        self
    }

    /// A password's line.
    pub fn secret(mut self, label: &str) -> Self {
        self = self.text(label, "", Vec::new());
        if let Some(Field::Text { secret, .. }) = self.fields.last_mut() {
            *secret = true;
        }
        self
    }

    pub fn note(mut self, text: &str) -> Self {
        if self.focus == self.fields.len() {
            self.focus += 1;
        }
        self.fields.push(Field::Note(text.into()));
        self
    }

    pub fn check(mut self, label: &str, on: bool) -> Self {
        self.fields.push(Field::Check {
            label: label.into(),
            on,
        });
        self
    }

    /// A line's text; none for a box or a question without fields.
    pub fn value(&self, ix: usize) -> &str {
        match self.fields.get(ix) {
            Some(Field::Text { input, .. }) => input.text(),
            _ => "",
        }
    }

    pub fn checked(&self, ix: usize) -> bool {
        matches!(self.fields[ix], Field::Check { on: true, .. })
    }

    /// Tab and Shift+Tab go round the fields; Up and Down too, but in a
    /// line with history, which they bring back.
    pub fn key(&mut self, key: &KeyEvent) -> Answer {
        // Only a question: Enter or Esc.
        if self.fields.iter().all(|field| matches!(field, Field::Note(_))) {
            let count = self.buttons.len().max(1);
            return match key.code {
                KeyCode::Esc => Answer::Cancel,
                KeyCode::Enter => Answer::Ok,
                KeyCode::Right | KeyCode::Tab => {
                    self.button = (self.button + 1) % count;
                    Answer::Open
                }
                KeyCode::Left | KeyCode::BackTab => {
                    self.button = (self.button + count - 1) % count;
                    Answer::Open
                }
                _ => Answer::Open,
            };
        }
        let count = self.fields.len();
        let fields = &self.fields;
        // The next field there is to focus, notes passed over.
        let step = |focus: usize, by: usize| {
            let mut at = (focus + by) % count;
            while matches!(fields[at], Field::Note(_)) {
                at = (at + by) % count;
            }
            at
        };
        let (forward, back) = (step(self.focus, 1), step(self.focus, count - 1));
        match (&mut self.fields[self.focus], key.code) {
            (_, KeyCode::Esc) => return Answer::Cancel,
            (_, KeyCode::Enter) => return Answer::Ok,
            (_, KeyCode::Tab) => self.focus = forward,
            (_, KeyCode::BackTab) => self.focus = back,
            (
                Field::Text {
                    input,
                    history,
                    recalled,
                    ..
                },
                code @ (KeyCode::Up | KeyCode::Down),
            ) if !history.is_empty() => {
                let at = match (code, *recalled) {
                    (KeyCode::Up, None) => 0,
                    (KeyCode::Up, Some(at)) => (at + 1).min(history.len() - 1),
                    (_, None | Some(0)) => return Answer::Open,
                    (_, Some(at)) => at - 1,
                };
                *recalled = Some(at);
                input.clear();
                input.insert(&history[at]);
            }
            (_, KeyCode::Down) => self.focus = forward,
            (_, KeyCode::Up) => self.focus = back,
            (Field::Check { on, .. }, KeyCode::Char(' ')) => *on = !*on,
            (Field::Text { input, .. }, _) => {
                input.handle(key);
            }
            _ => {}
        }
        Answer::Open
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = 60.min(area.width.saturating_sub(4)).max(20);
        // Within the frame and its padding.
        let room = width.saturating_sub(4) as usize;
        let rows: u16 = self
            .fields
            .iter()
            .map(|field| match field {
                Field::Text { .. } => 2,
                Field::Check { .. } => 1,
                Field::Note(text) => wrap(text, room).len() as u16,
            })
            .sum();
        let height = (rows + 4).min(area.height);
        let at = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width.min(area.width),
            height,
        );
        frame.render_widget(Clear, at);
        let block = Block::bordered()
            .border_set(skin().frame)
            .title(format!(" {} ", self.title))
            .title_style(skin().title)
            .style(skin().dialog)
            .padding(Padding::horizontal(1));
        let inner = block.inner(at);
        frame.render_widget(block, at);
        let room = inner.width as usize;
        let mut y = inner.y;
        let mut cursor = None;
        for (ix, field) in self.fields.iter().enumerate() {
            if y >= inner.bottom() {
                break;
            }
            let focused = ix == self.focus;
            let buf = frame.buffer_mut();
            match field {
                Field::Text { label, input, secret, .. } => {
                    buf.set_string(inner.x, y, fit(label, room), skin().dialog);
                    y += 1;
                    let chars: Vec<char> = match secret {
                        true => input.text().chars().map(|_| '•').collect(),
                        false => input.text().chars().collect(),
                    };
                    let start = input.cursor().saturating_sub(room.saturating_sub(1));
                    let shown: String = chars[start..].iter().collect();
                    buf.set_string(inner.x, y, fit(&shown, room), skin().input);
                    if focused {
                        let before: String = chars[start..input.cursor()].iter().collect();
                        cursor = Some(Position::new(inner.x + before.width() as u16, y));
                    }
                }
                Field::Check { label, on } => {
                    let mark = if *on { "[x]" } else { "[ ]" };
                    let style = if focused { skin().focus } else { skin().dialog };
                    buf.set_string(inner.x, y, fit(&format!("{mark} {label}"), room), style);
                }
                Field::Note(text) => {
                    for line in wrap(text, room) {
                        buf.set_string(inner.x, y, fit(&line, room), skin().dialog);
                        y += 1;
                    }
                    continue;
                }
            }
            y += 1;
        }
        let y = inner.bottom().saturating_sub(1);
        match self.buttons.is_empty() {
            true => {
                let buttons = t!("scmd.dialog.buttons");
                let x = inner.x + inner.width.saturating_sub(buttons.width() as u16) / 2;
                frame.buffer_mut().set_stringn(x, y, &buttons, room, skin().dialog);
            }
            false => {
                let shown: Vec<String> = self.buttons.iter().map(|label| format!("[ {label} ]")).collect();
                let width = shown.iter().map(|button| button.width() + 1).sum::<usize>().saturating_sub(1);
                let mut x = inner.x + inner.width.saturating_sub(width as u16) / 2;
                for (ix, button) in shown.iter().enumerate() {
                    let style = if ix == self.button { skin().focus } else { skin().dialog };
                    let left = (inner.right().saturating_sub(x)) as usize;
                    x += frame.buffer_mut().set_stringn(x, y, button, left, style).0.saturating_sub(x) + 1;
                }
            }
        }
        if let Some(cursor) = cursor {
            frame.set_cursor_position(cursor);
        }
    }
}

/// A list to pick from, its keys under it.
pub struct Chooser {
    pub title: String,
    pub items: Vec<String>,
    pub at: usize,
    /// The open one, checked.
    pub checked: Option<usize>,
    pub keys: String,
    pub list: List,
}

/// What a chooser lists.
pub enum List {
    Workspaces,
    /// Bookmarks, then saved connections.
    Places(Vec<crate::Place>),
}

pub enum Choice {
    Open,
    Pick(usize),
    New,
    Duplicate(usize),
    Rename(usize),
    Delete(usize),
    Cancel,
}

impl Chooser {
    pub fn key(&mut self, key: &KeyEvent) -> Choice {
        let last = self.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => Choice::Cancel,
            KeyCode::Enter => Choice::Pick(self.at),
            KeyCode::Char(digit @ '1'..='9') if (digit as usize - '1' as usize) <= last => Choice::Pick(digit as usize - '1' as usize),
            KeyCode::Up => {
                self.at = self.at.saturating_sub(1);
                Choice::Open
            }
            KeyCode::Down => {
                self.at = (self.at + 1).min(last);
                Choice::Open
            }
            KeyCode::F(7) | KeyCode::Insert => Choice::New,
            KeyCode::F(5) => Choice::Duplicate(self.at),
            KeyCode::F(2) => Choice::Rename(self.at),
            KeyCode::F(8) | KeyCode::Delete => Choice::Delete(self.at),
            _ => Choice::Open,
        }
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let keys = &self.keys;
        // An item after its number and check mark.
        let widest = self.items.iter().map(|item| item.width() + 4).max().unwrap_or(0).max(keys.width());
        let width = (widest as u16 + 4).min(area.width.saturating_sub(2)).max(20);
        let height = (self.items.len() as u16 + 4).min(area.height);
        let at = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width.min(area.width),
            height,
        );
        frame.render_widget(Clear, at);
        let block = Block::bordered()
            .border_set(skin().frame)
            .title(format!(" {} ", self.title))
            .title_style(skin().title)
            .style(skin().dialog)
            .padding(Padding::horizontal(1));
        let inner = block.inner(at);
        frame.render_widget(block, at);
        let room = inner.width as usize;
        let buf = frame.buffer_mut();
        for (ix, item) in self.items.iter().enumerate().take(inner.height.saturating_sub(2) as usize) {
            let mark = if Some(ix) == self.checked { "✓" } else { " " };
            let number = if ix < 9 { format!("{}", ix + 1) } else { " ".into() };
            let style = if ix == self.at { skin().focus } else { skin().dialog };
            buf.set_string(inner.x, inner.y + ix as u16, fit(&format!("{number} {mark} {item}"), room), style);
        }
        buf.set_string(inner.x, inner.bottom().saturating_sub(1), fit(keys, room), skin().dialog);
    }
}

/// `text`'s lines, each broken between words to fit `width` columns (a longer word anywhere).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if !line.is_empty() && line.width() + 1 + word.width() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            for c in word.chars() {
                if line.width() + c.width().unwrap_or(0) > width.max(1) {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(c);
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    #[test]
    fn types_ticks_and_recalls() {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let mut form = Form::new("Find")
            .text("Search for:", "", vec!["last".into(), "older".into()])
            .check("Case sensitive", false);
        form.key(&key(KeyCode::Up));
        form.key(&key(KeyCode::Up));
        assert_eq!(form.value(0), "older");
        form.key(&key(KeyCode::Down));
        assert_eq!(form.value(0), "last");
        form.key(&key(KeyCode::Char('!')));
        form.key(&key(KeyCode::Tab));
        form.key(&key(KeyCode::Char(' ')));
        assert!(form.checked(1));
        assert_eq!(form.key(&key(KeyCode::Enter)), Answer::Ok);
        assert_eq!(form.value(0), "last!");
    }

    #[test]
    fn wraps_between_words() {
        assert_eq!(wrap("The server is\n\nSHA256:abcdefghij", 8), ["The", "server", "is", "", "SHA256:a", "bcdefghi", "j"]);
        let mut form = Form::new("Unknown").note("text").check("Remember", true);
        form.key(&KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!form.checked(1));
    }
}
