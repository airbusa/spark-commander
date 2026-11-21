//! sedit's text: its lines, the caret and the selection's other end, and
//! undo and redo, a run of typing one step.

use std::ops::Range;

use super::text_edit::TextEdit;

/// `removed` at `at` became `inserted`; the caret and the selection's other
/// end before and after.
struct Change {
    at: usize,
    removed: String,
    inserted: String,
    before: (usize, Option<usize>),
    after: (usize, Option<usize>),
}

#[derive(Default)]
pub struct Buffer {
    text: String,
    /// Where each line starts.
    starts: Vec<usize>,
    pub cursor: usize,
    /// The selection's other end.
    pub anchor: Option<usize>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    /// Undo's steps when saved; none once undone past them and changed.
    saved: Option<usize>,
    /// The last change was typing, which more typing joins.
    typing: bool,
    /// Where the text changed first since `take_changed`.
    changed: Option<usize>,
}

impl Buffer {
    pub fn new(text: String) -> Self {
        let mut buffer = Buffer { text, saved: Some(0), ..Default::default() };
        buffer.index();
        buffer
    }

    fn index(&mut self) {
        let ends = memchr::memchr_iter(b'\n', self.text.as_bytes()).map(|at| at + 1);
        self.starts = std::iter::once(0).chain(ends).collect();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    /// Line `line`'s text, its `\n` left out.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let start = self.starts[line];
        let end = self.starts.get(line + 1).map_or(self.text.len(), |next| next - 1);
        start..end
    }

    pub fn line(&self, line: usize) -> &str {
        &self.text[self.line_range(line)]
    }

    pub fn line_of(&self, at: usize) -> usize {
        self.starts.partition_point(|&start| start <= at) - 1
    }

    pub fn selection(&self) -> Range<usize> {
        let other = self.anchor.unwrap_or(self.cursor);
        other.min(self.cursor)..other.max(self.cursor)
    }

    pub fn select(&mut self, range: Range<usize>) {
        self.cursor = range.end;
        self.anchor = (!range.is_empty()).then_some(range.start);
    }

    /// Where the text changed first since asked last: what follows it is to be read again.
    pub fn take_changed(&mut self) -> Option<usize> {
        self.changed.take()
    }

    fn changed_at(&mut self, at: usize) {
        self.changed = Some(self.changed.map_or(at, |first| first.min(at)));
    }

    pub fn modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    pub fn saved(&mut self) {
        (self.saved, self.typing) = (Some(self.undo.len()), false);
    }

    /// `range` becomes `text`, the caret after it; `typing` (a character,
    /// Backspace, Delete) joins the typing just before it, the caret not moved since.
    pub fn replace(&mut self, range: Range<usize>, text: &str, typing: bool) {
        let before = (self.cursor, self.anchor);
        let removed = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), text);
        self.index();
        self.changed_at(range.start);
        (self.cursor, self.anchor) = (range.start + text.len(), None);
        let after = (self.cursor, None);
        self.redo.clear();
        let joins = typing && self.typing && before.1.is_none() && self.saved != Some(self.undo.len());
        let joined = match self.undo.last_mut() {
            Some(last) if joins && last.after.0 == before.0 => match (removed.is_empty(), text.is_empty()) {
                (true, false) if last.removed.is_empty() && last.at + last.inserted.len() == range.start => {
                    last.inserted.push_str(text);
                    true
                }
                // Backspace, then Delete.
                (false, true) if last.inserted.is_empty() && range.end == last.at => {
                    last.removed.insert_str(0, &removed);
                    last.at = range.start;
                    true
                }
                (false, true) if last.inserted.is_empty() && range.start == last.at => {
                    last.removed.push_str(&removed);
                    true
                }
                _ => false,
            },
            _ => false,
        };
        match joined {
            true => self.undo.last_mut().expect("joined the last").after = after,
            false => {
                if self.saved.is_some_and(|saved| saved > self.undo.len()) {
                    self.saved = None;
                }
                self.undo.push(Change { at: range.start, removed, inserted: text.to_owned(), before, after });
            }
        }
        self.typing = typing;
    }

    /// One of `text_edit`'s changes, its selection after it.
    pub fn apply(&mut self, edit: TextEdit) {
        self.replace(edit.range, &edit.text, false);
        self.select(edit.selection);
        if let Some(last) = self.undo.last_mut() {
            last.after = (self.cursor, self.anchor);
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(change) = self.undo.pop() else { return false };
        self.text.replace_range(change.at..change.at + change.inserted.len(), &change.removed);
        self.index();
        self.changed_at(change.at);
        (self.cursor, self.anchor, self.typing) = (change.before.0, change.before.1, false);
        self.redo.push(change);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(change) = self.redo.pop() else { return false };
        self.text.replace_range(change.at..change.at + change.removed.len(), &change.inserted);
        self.index();
        self.changed_at(change.at);
        (self.cursor, self.anchor, self.typing) = (change.after.0, change.after.1, false);
        self.undo.push(change);
        true
    }

    /// The character before `at`'s start.
    pub fn before(&self, at: usize) -> usize {
        self.text[..at].chars().next_back().map_or(at, |c| at - c.len_utf8())
    }

    /// The character at `at`'s end.
    pub fn after(&self, at: usize) -> usize {
        self.text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// Ctrl+Left: the start of the word before `at`, spaces passed over.
    pub fn word_before(&self, at: usize) -> usize {
        let mut chars = self.text[..at].char_indices().rev().skip_while(|(_, c)| c.is_whitespace()).peekable();
        let Some(&(mut start, first)) = chars.peek() else { return 0 };
        for (ix, c) in chars {
            if kind(c) != kind(first) {
                break;
            }
            start = ix;
        }
        start
    }

    /// Ctrl+Right: the end of the word after `at`, spaces passed over.
    pub fn word_after(&self, at: usize) -> usize {
        let mut chars = self.text[at..].char_indices().skip_while(|(_, c)| c.is_whitespace()).peekable();
        let Some(&(_, first)) = chars.peek() else { return self.text.len() };
        let end = chars.find(|&(_, c)| kind(c) != kind(first)).map_or(self.text.len() - at, |(ix, _)| ix);
        at + end
    }
}

/// Words, other marks, spaces: what Ctrl+Left and Ctrl+Right go by.
fn kind(c: char) -> u8 {
    match c {
        c if c.is_alphanumeric() || c == '_' => 0,
        c if c.is_whitespace() => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_undoes_and_knows_it_was_saved() {
        let mut buffer = Buffer::new("one\ntwo".into());
        assert_eq!((buffer.lines(), buffer.line(1), buffer.line_of(4), buffer.line_of(3)), (2, "two", 1, 0));
        buffer.cursor = 3;
        for c in [" ", "a", "b"] {
            buffer.replace(buffer.cursor..buffer.cursor, c, true);
        }
        buffer.replace(buffer.before(buffer.cursor)..buffer.cursor, "", true);
        assert_eq!(buffer.text(), "one a\ntwo");
        assert!(buffer.modified());
        buffer.saved();
        buffer.replace(buffer.cursor..buffer.cursor, "!", true);
        assert!(buffer.modified(), "typing after a save is a step of its own");
        assert!(buffer.undo() && !buffer.modified());
        assert!(buffer.undo() && buffer.undo(), "Backspace a step of its own");
        assert_eq!((buffer.text(), buffer.cursor), ("one\ntwo", 3), "the typing one step");
        assert!(buffer.redo() && buffer.redo() && !buffer.modified());
        buffer.undo();
        buffer.replace(0..3, "1", false);
        assert!(buffer.modified() && !buffer.redo(), "a change after undo: the saved text gone");
        buffer.undo();
        buffer.redo();
        assert!(buffer.modified());
    }

    #[test]
    fn moves_by_words() {
        let buffer = Buffer::new("let x = a_b(1);  ok".into());
        assert_eq!(buffer.word_after(0), 3);
        assert_eq!(buffer.word_after(7), 11);
        assert_eq!(buffer.word_after(11), 12);
        assert_eq!(buffer.word_after(15), 19);
        assert_eq!(buffer.word_before(19), 17);
        assert_eq!(buffer.word_before(17), 13);
        assert_eq!(buffer.word_before(13), 12);
        assert_eq!(buffer.word_before(11), 8);
        assert_eq!(buffer.word_before(3), 0);
    }
}
