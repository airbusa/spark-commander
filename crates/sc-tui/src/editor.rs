//! sedit, and scmd's F4: an editor as Midnight Commander's mcedit, over the
//! core's text (`edit::buffer`). mcedit's keys, the common ones beside them:
//! F2 or Ctrl+S saves, F3 marks a block (F5 copies it to the caret, F6 moves
//! it there, F8 deletes it), F7 finds, F4 replaces, F9 its menus, F10 quits.

use std::io;
use std::ops::Range;
use std::path::PathBuf;
use std::time::SystemTime;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use sc_core::edit::buffer::Buffer;
use sc_core::edit::syntax::{Open, Syntax};
use sc_core::edit::text_edit::{self, TextCase, TextEdit};
use sc_core::edit::text_find::{Finder, Options};
use sc_core::edit::{self, Loaded};
use sc_core::settings::{EditorSettings, Settings};
use sc_core::t;
use sc_core::viewer::Query;
use sc_core::viewer::origin::{self, Upload};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::dialog::Form;
use crate::menu::{Entry, Menus, Picked};
use crate::skin::skin;
use crate::{App, Purpose, ui};

#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    Save,
    SaveAs,
    Reload,
    Close,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Find,
    Replace,
    FindNext,
    FindPrevious,
    GoToLine,
    SelectAll,
    Mark,
    CopyBlock,
    MoveBlock,
    DeleteBlock,
    Case(TextCase),
    Duplicate,
    MoveUp,
    MoveDown,
    DeleteLines,
    /// Descending when true.
    Sort(bool),
    Trim,
    Comment,
    Indent,
    Outdent,
    LineNumbers,
    Whitespace,
    TabWidth(usize),
    HardTabs,
    Wrap,
}

/// What the editor's dialogs ask.
pub enum Asked {
    /// Not text: shown anyway, or the editor closes.
    Unfit,
    Find,
    Replace,
    GoToLine,
    /// Save As's name; true: the editor closes once saved.
    SaveAs(bool),
    Overwrite(PathBuf, bool),
    /// What was not text saved as text.
    SaveAnyway(bool),
    /// Save, don't save, or stay.
    Close,
    Reload,
}

/// Letters Alt has a command on: Alt+R redo, Alt+L go to line, Alt+N line numbers, Alt+Z word wrap.
const ALT_TAKEN: [char; 4] = ['r', 'l', 'n', 'z'];

pub struct Editor {
    /// None: a new file, named when first saved.
    path: Option<PathBuf>,
    /// A server's or an archive's file goes by its own name, and goes back
    /// there once the editor closes (the copy's time when opened).
    title: Option<String>,
    upload: Option<(Upload, Option<SystemTime>)>,
    pub buffer: Buffer,
    /// The file's language's colours, and what each line starts with open, as far as read.
    syntax: Option<Syntax>,
    opens: Vec<Open>,
    /// Its lines end in CRLF: saved so again.
    crlf: bool,
    /// Not text: why, and the text shown on request; shown, saving asks first.
    unfit: Option<(&'static str, String)>,
    forced: bool,
    options: EditorSettings,
    /// F3: moves take the selection along; F3 again leaves it a block, which
    /// stays while the caret moves, as mcedit's.
    marking: bool,
    block: Option<Range<usize>>,
    /// The column Up and Down keep.
    column: Option<usize>,
    /// The first line in sight, and which of its rows when it is wrapped.
    top: usize,
    top_row: usize,
    left: usize,
    /// The text's columns at the last frame, which wrapped lines fit.
    room: usize,
    /// The caret brought in sight at the next frame.
    follow: bool,
    /// Where the last frame put the text, after the line numbers.
    body: Rect,
    gutter: u16,
    pub menus: Menus<Act>,
    /// Replace's last.
    replace_with: String,
}

impl Editor {
    /// `path`'s text, a new file's where there is none.
    pub fn open(path: Option<PathBuf>, title: Option<String>, options: EditorSettings) -> io::Result<Editor> {
        let loaded = match &path {
            Some(path) => match edit::load_text(path, options.limit_mb) {
                Err(err) if err.kind() == io::ErrorKind::NotFound => Loaded::Text(String::new()),
                loaded => loaded?,
            },
            None => Loaded::Text(String::new()),
        };
        let (text, unfit) = match loaded {
            Loaded::Text(text) => (text, None),
            Loaded::Unfit { why, anyway } => (String::new(), Some((why, anyway))),
        };
        let mut editor = Editor {
            path,
            title,
            upload: None,
            buffer: Buffer::default(),
            syntax: None,
            opens: Vec::new(),
            crlf: false,
            unfit,
            forced: false,
            options,
            marking: false,
            block: None,
            column: None,
            top: 0,
            top_row: 0,
            left: 0,
            room: 0,
            follow: true,
            body: Rect::default(),
            gutter: 0,
            menus: Menus::default(),
            replace_with: String::new(),
        };
        editor.set_text(text);
        Ok(editor)
    }

    fn set_text(&mut self, text: String) {
        self.crlf = text.contains("\r\n");
        let found = text_edit::detect_indent(&text);
        (self.options.tab_size, self.options.hard_tabs) = found.unwrap_or((self.options.tab_size, self.options.hard_tabs));
        self.buffer = Buffer::new(if self.crlf { text.replace("\r\n", "\n") } else { text });
        self.set_language();
        (self.marking, self.block, self.column, self.follow) = (false, None, None, true);
        (self.top, self.top_row, self.left) = (0, 0, 0);
    }

    fn name(&self) -> String {
        match (&self.title, &self.path) {
            (Some(title), _) => title.clone(),
            (None, Some(path)) => path.display().to_string(),
            (None, None) => t!("scmd.editor.untitled"),
        }
    }

    fn tab(&self) -> usize {
        self.options.tab_size.max(1)
    }

    /// F3's block, else the selection.
    fn selected(&self) -> Range<usize> {
        self.block.clone().unwrap_or_else(|| self.buffer.selection())
    }

    /// The caret's line and column.
    fn place(&self) -> (usize, usize) {
        let line = self.buffer.line_of(self.buffer.cursor);
        let start = self.buffer.line_range(line).start;
        (line, columns(&self.buffer.text()[start..self.buffer.cursor], self.tab()))
    }

    /// Where column `column` of line `line` is, its rows not counted.
    fn line_offset(&self, line: usize, column: usize) -> usize {
        let range = self.buffer.line_range(line.min(self.buffer.lines() - 1));
        range.start + offset_at(&self.buffer.text()[range], 0, column, self.tab())
    }

    /// Where each of line `line`'s rows starts in it: one row, or, wrapped, as many as fit.
    fn rows(&self, line: usize) -> Vec<usize> {
        match self.options.wrap && self.room > 0 {
            true => breaks(self.buffer.line(line), self.room, self.tab()),
            false => vec![0],
        }
    }

    /// The caret's line, its row in the line, and its column in the row.
    fn spot(&self) -> (usize, usize, usize) {
        let (line, cursor) = (self.buffer.line_of(self.buffer.cursor), self.buffer.cursor);
        let start = self.buffer.line_range(line).start;
        let rows = self.rows(line);
        let row = rows.partition_point(|&at| at <= cursor - start) - 1;
        let text = &self.buffer.text()[start..cursor];
        (line, row, columns(text, self.tab()) - columns(&text[..rows[row]], self.tab()))
    }

    /// Where column `column` of a line's row is; on a row that wraps, at most its last character.
    fn offset(&self, line: usize, row: usize, column: usize) -> usize {
        let (range, rows) = (self.buffer.line_range(line), self.rows(line));
        let text = &self.buffer.text()[range.clone()];
        let at = offset_at(text, rows[row], column, self.tab());
        match rows.get(row + 1) {
            Some(&next) if at >= next => range.start + self.buffer.before(range.start + next) - range.start,
            _ => range.start + at,
        }
    }

    /// `by` rows on from a line's row, stopping at the text's ends.
    fn step(&self, (mut line, mut row): (usize, usize), by: isize) -> (usize, usize) {
        let last = self.buffer.lines() - 1;
        for _ in 0..by.unsigned_abs() {
            match by < 0 {
                true if row > 0 => row -= 1,
                true if line > 0 => (line, row) = (line - 1, self.rows(line - 1).len() - 1),
                false if row + 1 < self.rows(line).len() => row += 1,
                false if line < last => (line, row) = (line + 1, 0),
                _ => break,
            }
        }
        (line, row)
    }

    /// The text moved `by` rows, the caret left where it is.
    fn scroll(&mut self, by: isize) {
        (self.top, self.top_row) = self.step((self.top, self.top_row), by);
    }

    /// The caret to `to`; Shift, or F3's marking, takes the selection along.
    fn go(&mut self, to: usize, select: bool) {
        match select || self.marking {
            true => {
                self.buffer.anchor.get_or_insert(self.buffer.cursor);
                if select {
                    self.block = None;
                }
            }
            false => self.buffer.anchor = None,
        }
        (self.buffer.cursor, self.column, self.follow) = (to, None, true);
    }

    /// Up and Down by rows, keeping the column they started from.
    fn vertical(&mut self, by: isize, select: bool) {
        let (line, row, column) = self.spot();
        let column = self.column.unwrap_or(column);
        let (line, row) = self.step((line, row), by);
        let to = self.offset(line, row, column);
        self.go(to, select);
        self.column = Some(column);
    }

    fn changed(&mut self) {
        (self.block, self.marking, self.column, self.follow) = (None, false, None, true);
    }

    /// `text` in place of the selection; F3's block is let be.
    pub fn type_text(&mut self, text: &str, typing: bool) {
        let range = self.buffer.selection();
        self.buffer.replace(range, text, typing);
        self.changed();
    }

    fn apply(&mut self, edit: Option<TextEdit>) {
        if let Some(edit) = edit {
            self.buffer.apply(edit);
            self.changed();
        }
    }

    /// Enter: the line's indent, up to the caret, carried to the next.
    fn newline(&mut self) {
        let at = self.buffer.selection().start;
        let start = self.buffer.line_range(self.buffer.line_of(at)).start;
        let indent: String = self.buffer.text()[start..at].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.type_text(&format!("\n{indent}"), false);
    }

    /// Tab: lines selected are indented; else a tab, or spaces to the next stop.
    fn tab_key(&mut self) {
        let selected = self.selected();
        if self.block.is_some() || self.buffer.text()[selected].contains('\n') {
            return self.run(Act::Indent);
        }
        let spaces = self.tab() - self.place().1 % self.tab();
        let text = if self.options.hard_tabs { "\t".to_owned() } else { " ".repeat(spaces) };
        self.type_text(&text, true);
    }

    /// Backspace and Delete: the selection, else a character.
    fn delete(&mut self, forward: bool) {
        let selection = self.buffer.selection();
        let range = match (selection.is_empty(), forward) {
            (false, _) => selection.clone(),
            (true, true) => selection.start..self.buffer.after(selection.start),
            (true, false) => self.buffer.before(selection.start)..selection.start,
        };
        if !range.is_empty() {
            self.buffer.replace(range, "", selection.is_empty());
            self.changed();
        }
    }

    /// F3: marking starts at the caret; again, what it marked stays a block.
    fn mark(&mut self) {
        match self.marking {
            true => {
                let selection = self.buffer.selection();
                self.block = (!selection.is_empty()).then_some(selection);
                (self.buffer.anchor, self.marking) = (None, false);
            }
            false => (self.block, self.buffer.anchor, self.marking) = (None, Some(self.buffer.cursor), true),
        }
    }

    /// Esc: the marking, block or selection let go; false when there was none.
    fn let_go(&mut self) -> bool {
        let had = self.marking || self.block.is_some() || !self.buffer.selection().is_empty();
        (self.marking, self.block, self.buffer.anchor) = (false, None, None);
        had
    }

    /// F5: a copy of the block at the caret, the copy the block; the caret stays before it.
    fn copy_block(&mut self) {
        let (block, at) = (self.selected(), self.buffer.cursor);
        if block.is_empty() || (block.start < at && at < block.end) {
            return;
        }
        let copy = self.buffer.text()[block].to_owned();
        self.buffer.replace(at..at, &copy, false);
        self.changed();
        (self.buffer.cursor, self.block) = (at, Some(at..at + copy.len()));
    }

    /// F6: the block moved to the caret, in one step.
    fn move_block(&mut self) {
        let (block, at) = (self.selected(), self.buffer.cursor);
        if block.is_empty() || (block.start <= at && at <= block.end) {
            return;
        }
        let text = self.buffer.text();
        let moved = &text[block.clone()];
        let (range, new, start) = match at < block.start {
            true => (at..block.end, format!("{moved}{}", &text[at..block.start]), at),
            false => (block.start..at, format!("{}{moved}", &text[block.end..at]), at - moved.len()),
        };
        let length = moved.len();
        self.buffer.replace(range, &new, false);
        self.changed();
        self.buffer.cursor = start;
        self.block = Some(start..start + length);
    }

    /// The editor's own commands: on the text, and the View menu's tabs.
    fn run(&mut self, act: Act) {
        let (text, selected) = (self.buffer.text(), self.selected());
        let edit = match act {
            Act::Case(case) => text_edit::change_case(text, selected.clone(), case),
            Act::Duplicate => Some(text_edit::duplicate_lines(text, selected.clone())),
            Act::MoveUp | Act::MoveDown => text_edit::move_lines(text, selected.clone(), act == Act::MoveUp),
            Act::DeleteLines => Some(text_edit::delete_lines(text, selected.clone())),
            Act::Sort(descending) => Some(text_edit::sort_lines(text, selected.clone(), descending)),
            Act::Trim => text_edit::trim_trailing_whitespace(text, selected.clone()),
            Act::Comment => self.comment().map(|prefix| text_edit::toggle_comment(text, selected.clone(), prefix)),
            Act::Indent => {
                let unit = if self.options.hard_tabs { "\t".to_owned() } else { " ".repeat(self.tab()) };
                Some(text_edit::indent_lines(text, selected.clone(), &unit))
            }
            Act::Outdent => text_edit::outdent_lines(text, selected.clone(), self.tab()),
            _ => None,
        };
        if edit.is_some() {
            return self.apply(edit);
        }
        match act {
            Act::Undo if self.buffer.undo() => self.changed(),
            Act::Redo if self.buffer.redo() => self.changed(),
            Act::SelectAll => {
                (self.marking, self.block, self.follow) = (false, None, true);
                self.buffer.select(0..self.buffer.text().len());
            }
            Act::Mark => self.mark(),
            Act::CopyBlock => self.copy_block(),
            Act::MoveBlock => self.move_block(),
            Act::DeleteBlock if !selected.is_empty() => {
                self.buffer.replace(selected, "", false);
                self.changed();
            }
            Act::TabWidth(width) => self.options.tab_size = width,
            Act::HardTabs => self.options.hard_tabs = !self.options.hard_tabs,
            _ => {}
        }
    }

    fn language(&self) -> &'static str {
        self.path.as_deref().map_or("text", text_edit::language_of)
    }

    /// By the file's name; again when saved as another.
    fn set_language(&mut self) {
        (self.syntax, self.opens) = (Syntax::of(self.language()), vec![Open::Nothing]);
    }

    /// What starts a line comment in the file's language.
    fn comment(&self) -> Option<&'static str> {
        text_edit::line_comment(self.language())
    }

    /// What the lines up to `last` start with open, read on from where the text last changed.
    fn read_opens(&mut self, last: usize) {
        if let Some(at) = self.buffer.take_changed() {
            self.opens.truncate(self.buffer.line_of(at) + 1);
        }
        let Some(syntax) = self.syntax else { return };
        while self.opens.len() <= last.min(self.buffer.lines() - 1) {
            let line = self.opens.len() - 1;
            let next = syntax.next(self.buffer.line(line), self.opens[line]);
            self.opens.push(next);
        }
    }

    /// Moves, typing, F9.
    fn key(&mut self, key: &KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let (cursor, end) = (self.buffer.cursor, self.buffer.text().len());
        let page = self.body.height.max(1) as usize;
        let line = self.buffer.line_range(self.buffer.line_of(cursor));
        let to = match key.code {
            KeyCode::Left if ctrl => self.buffer.word_before(cursor),
            KeyCode::Right if ctrl => self.buffer.word_after(cursor),
            KeyCode::Left => self.buffer.before(cursor),
            KeyCode::Right => self.buffer.after(cursor),
            KeyCode::Up => return self.vertical(-1, shift),
            KeyCode::Down => return self.vertical(1, shift),
            KeyCode::PageUp | KeyCode::Home if ctrl => 0,
            KeyCode::PageDown | KeyCode::End if ctrl => end,
            KeyCode::PageUp | KeyCode::PageDown => {
                let by = if key.code == KeyCode::PageUp { -(page as isize) } else { page as isize };
                self.scroll(by);
                return self.vertical(by, shift);
            }
            // The line's text first, then its start.
            KeyCode::Home => {
                let text = &self.buffer.text()[line.clone()];
                let first = line.start + text.len() - text.trim_start_matches([' ', '\t']).len();
                if cursor == first { line.start } else { first }
            }
            KeyCode::End => line.end,
            KeyCode::Enter => return self.newline(),
            KeyCode::Tab => return self.tab_key(),
            KeyCode::BackTab => return self.run(Act::Outdent),
            KeyCode::Backspace => return self.delete(false),
            KeyCode::Delete => return self.delete(true),
            KeyCode::F(9) => {
                self.make_menus();
                return self.menus.open(0);
            }
            KeyCode::Char(c) if alt => {
                self.make_menus();
                self.menus.open_by_letter(c);
                return;
            }
            KeyCode::Char(c) if !ctrl => return self.type_text(c.encode_utf8(&mut [0; 4]), true),
            _ => return,
        };
        self.go(to, shift);
    }

    /// F9's menus as they are now, the window editor's.
    fn make_menus(&mut self) {
        let (o, text) = (&self.options, self.unfit.is_none());
        let item = |label: String, key: &str, act: Act, works: bool, on: bool| Entry::item(label, key, act, works, on);
        let on = |label: String, key: &str, act: Act| Entry::item(label, key, act, text, false);
        let file = vec![
            on(t!("editor.file.save"), "F2", Act::Save),
            on(t!("editor.file.save_as"), "F12", Act::SaveAs),
            Entry::line(),
            item(t!("editor.file.reload"), "", Act::Reload, text && self.path.is_some(), false),
            Entry::line(),
            item(t!("editor.file.close"), "F10", Act::Close, true, false),
        ];
        let edit = vec![
            on(t!("editor.edit.undo"), "Ctrl+Z", Act::Undo),
            on(t!("editor.edit.redo"), "Alt+R", Act::Redo),
            Entry::line(),
            on(t!("editor.edit.cut"), "Ctrl+X", Act::Cut),
            on(t!("editor.edit.copy"), "Ctrl+C", Act::Copy),
            on(t!("editor.edit.paste"), "Ctrl+V", Act::Paste),
            Entry::line(),
            on(t!("editor.edit.find"), "F7", Act::Find),
            on(t!("editor.edit.replace"), "F4", Act::Replace),
            on(t!("editor.edit.find_next"), "Shift+F7", Act::FindNext),
            on(t!("editor.edit.find_previous"), "", Act::FindPrevious),
            Entry::line(),
            on(t!("editor.edit.go_to_line"), "Alt+L", Act::GoToLine),
        ];
        let selection = vec![
            on(t!("editor.selection.select_all"), "Ctrl+A", Act::SelectAll),
            Entry::line(),
            on(t!("editor.selection.uppercase"), "", Act::Case(TextCase::Upper)),
            on(t!("editor.selection.lowercase"), "", Act::Case(TextCase::Lower)),
            on(t!("editor.selection.title_case"), "", Act::Case(TextCase::Title)),
            Entry::line(),
            on(t!("editor.selection.duplicate_lines"), "", Act::Duplicate),
            on(t!("editor.selection.move_lines_up"), "Alt+Up", Act::MoveUp),
            on(t!("editor.selection.move_lines_down"), "Alt+Down", Act::MoveDown),
            on(t!("editor.selection.delete_lines"), "Ctrl+Y", Act::DeleteLines),
            Entry::line(),
            on(t!("editor.selection.sort_lines_ascending"), "", Act::Sort(false)),
            on(t!("editor.selection.sort_lines_descending"), "", Act::Sort(true)),
            on(t!("editor.selection.trim_trailing_whitespace"), "", Act::Trim),
            Entry::line(),
            item(t!("editor.selection.toggle_line_comment"), "", Act::Comment, text && self.comment().is_some(), false),
            on(t!("editor.selection.indent"), "Tab", Act::Indent),
            on(t!("editor.selection.outdent"), "Shift+Tab", Act::Outdent),
        ];
        let mut view = vec![
            item(t!("editor.view.word_wrap"), "Alt+Z", Act::Wrap, true, o.wrap),
            item(t!("editor.view.line_numbers"), "Alt+N", Act::LineNumbers, true, o.line_numbers),
            item(t!("editor.view.whitespace"), "", Act::Whitespace, true, o.whitespace),
            Entry::line(),
        ];
        for width in [2, 4, 8] {
            view.push(item(t!("editor.view.tab_width", width = width), "", Act::TabWidth(width), true, o.tab_size == width));
        }
        view.push(item(t!("editor.view.indent_with_tabs"), "", Act::HardTabs, true, o.hard_tabs));
        let menus = vec![
            (t!("editor.menu.file"), file),
            (t!("editor.menu.edit"), edit),
            (t!("editor.menu.selection"), selection),
            (t!("editor.menu.view"), view),
        ];
        self.menus.fill(menus, &ALT_TAKEN);
    }
}

/// Keys that are commands, mcedit's and the common ones.
fn act_of(key: &KeyEvent) -> Option<Act> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    Some(match key.code {
        KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
            'z' if shift => Act::Redo,
            'z' | 'u' => Act::Undo,
            's' => Act::Save,
            'c' => Act::Copy,
            'x' => Act::Cut,
            'v' => Act::Paste,
            'a' => Act::SelectAll,
            'f' => Act::Find,
            'y' => Act::DeleteLines,
            _ => return None,
        },
        KeyCode::Char('r') if alt => Act::Redo,
        KeyCode::Char('l') if alt => Act::GoToLine,
        KeyCode::Char('n') if alt => Act::LineNumbers,
        KeyCode::Char('z') if alt => Act::Wrap,
        KeyCode::Up if alt => Act::MoveUp,
        KeyCode::Down if alt => Act::MoveDown,
        KeyCode::Insert if ctrl => Act::Copy,
        KeyCode::Insert if shift => Act::Paste,
        KeyCode::Delete if shift => Act::Cut,
        KeyCode::F(2) if shift => Act::SaveAs,
        KeyCode::F(2) => Act::Save,
        KeyCode::F(12) => Act::SaveAs,
        KeyCode::F(3) => Act::Mark,
        KeyCode::F(4) => Act::Replace,
        KeyCode::F(5) => Act::CopyBlock,
        KeyCode::F(6) => Act::MoveBlock,
        KeyCode::F(7) if shift => Act::FindNext,
        KeyCode::F(17) => Act::FindNext,
        KeyCode::F(7) => Act::Find,
        KeyCode::F(8) => Act::DeleteBlock,
        KeyCode::F(10) => Act::Close,
        _ => return None,
    })
}

/// Columns `c` takes at column `column`: a tab to its stop, a control character its symbol's one.
fn width(c: char, column: usize, tab: usize) -> usize {
    match c {
        '\t' => tab - column % tab,
        c if c.is_control() => 1,
        c => c.width().unwrap_or(0),
    }
}

/// The columns a line's `text` takes from its start.
fn columns(text: &str, tab: usize) -> usize {
    text.chars().fold(0, |column, c| column + width(c, column, tab))
}

/// Where `column` columns on from `from` falls in `line`: the character over it, else the line's end.
fn offset_at(line: &str, from: usize, column: usize, tab: usize) -> usize {
    let mut at = columns(&line[..from], tab);
    let target = at + column;
    for (ix, c) in line[from..].char_indices() {
        let wide = width(c, at, tab);
        if at + wide > target {
            return from + ix;
        }
        at += wide;
    }
    line.len()
}

/// Where a line's rows start when it wraps at `room` columns: after a space where
/// there is one in the row, else where the row is full.
fn breaks(line: &str, room: usize, tab: usize) -> Vec<usize> {
    let mut starts = vec![0];
    // The row's first column, and the place and column after the row's last space.
    let (mut column, mut first, mut space) = (0, 0, None);
    for (ix, c) in line.char_indices() {
        let wide = width(c, column, tab);
        let start = *starts.last().expect("a row");
        if column + wide - first > room && ix > start {
            let (at, at_column) = space.filter(|&(at, _)| at > start).unwrap_or((ix, column));
            starts.push(at);
            (first, space) = (at_column, None);
        }
        column += wide;
        if c == ' ' || c == '\t' {
            space = Some((ix + c.len_utf8(), column));
        }
    }
    starts
}

/// The 10 keys, mcedit's.
fn keys() -> [String; 10] {
    [
        String::new(),
        t!("scmd.editor.key_bar.save"),
        t!("scmd.editor.key_bar.mark"),
        t!("scmd.editor.key_bar.replace"),
        t!("scmd.key_bar.copy"),
        t!("scmd.key_bar.move"),
        t!("scmd.viewer.key_bar.search"),
        t!("scmd.key_bar.delete"),
        t!("scmd.key_bar.menu"),
        t!("scmd.key_bar.quit"),
    ]
}

/// A menu word as a dialog's title: its dots left out.
fn titled(label: String) -> String {
    label.trim_end_matches(['.', '…']).to_owned()
}

impl App {
    /// The editor on `path`; a server's copy goes back once it closes.
    pub fn open_editor(&mut self, path: Option<PathBuf>, title: Option<String>, upload: Option<Upload>) {
        let name = title.clone().or_else(|| path.as_ref().map(|path| path.display().to_string())).unwrap_or_default();
        match Editor::open(path.clone(), title, self.settings.editor.clone()) {
            Ok(mut editor) => {
                let before = path.as_deref().and_then(|path| origin::modified(path).ok().flatten());
                editor.upload = upload.map(|upload| (upload, before));
                self.show_editor(editor);
            }
            Err(err) => self.say(format!("{name}: {err}"), true),
        }
    }

    /// A file that is not text asks first whether to show it anyway.
    pub fn show_editor(&mut self, editor: Editor) {
        if let Some((why, _)) = &editor.unfit {
            let form = Form::new(editor.name())
                .note(&t!("editor.unfit.text", why = *why))
                .buttons(vec![t!("editor.unfit.open_anyway"), t!("editor.dialog.cancel")]);
            self.form = Some((form, Purpose::Edit(Asked::Unfit)));
        }
        self.editor = Some(editor);
    }

    /// Whether the editor is open on a file not shown: closing a dialog then closes it.
    pub fn editor_unfit(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.unfit.is_some())
    }

    pub fn editor_key(&mut self, key: KeyEvent) {
        let Some(editor) = self.editor.as_mut() else { return };
        if editor.menus.is_open() {
            if let Some(Picked::Command(act)) = editor.menus.key(&key) {
                self.act(act);
            }
            return;
        }
        let act = match key.code {
            KeyCode::Esc if editor.let_go() => return,
            KeyCode::Esc => Some(Act::Close),
            _ => act_of(&key),
        };
        match act {
            Some(act) => self.act(act),
            None if editor.unfit.is_some() => {}
            None => editor.key(&key),
        }
    }

    pub fn editor_mouse(&mut self, mouse: MouseEvent) {
        let Some(editor) = self.editor.as_mut() else { return };
        if editor.menus.is_open() {
            if let Some(Picked::Command(act)) = editor.menus.mouse(&mouse) {
                self.act(act);
            }
            return;
        }
        let body = editor.body;
        let at = || {
            let row = mouse.row.clamp(body.y, body.bottom().max(body.y + 1) - 1) - body.y;
            let column = mouse.column.saturating_sub(body.x + editor.gutter) as usize;
            let (line, row) = editor.step((editor.top, editor.top_row), row as isize);
            editor.offset(line, row, editor.left + column)
        };
        match mouse.kind {
            MouseEventKind::ScrollUp => editor.scroll(-3),
            MouseEventKind::ScrollDown => editor.scroll(3),
            MouseEventKind::Down(MouseButton::Left) if body.contains(Position::new(mouse.column, mouse.row)) => {
                let at = at();
                (editor.marking, editor.block, editor.column, editor.follow) = (false, None, None, true);
                (editor.buffer.cursor, editor.buffer.anchor) = (at, Some(at));
            }
            MouseEventKind::Drag(MouseButton::Left) if editor.buffer.anchor.is_some() => {
                (editor.buffer.cursor, editor.follow) = (at(), true);
            }
            MouseEventKind::Up(MouseButton::Left) if editor.buffer.anchor == Some(editor.buffer.cursor) => editor.buffer.anchor = None,
            _ => {}
        }
    }

    fn act(&mut self, act: Act) {
        let Some(editor) = self.editor.as_mut() else { return };
        let view = matches!(act, Act::Close | Act::LineNumbers | Act::Whitespace | Act::Wrap | Act::TabWidth(_) | Act::HardTabs);
        if editor.unfit.is_some() && !view {
            return;
        }
        match act {
            Act::Save => self.save_editor(false),
            Act::SaveAs => self.ask_save_as(false),
            Act::Reload => self.reload_editor(true),
            Act::Close => self.close_editor(),
            Act::Cut | Act::Copy => self.copy_out(act == Act::Cut),
            Act::Paste => {
                let text = self.clipboard.clone();
                editor.type_text(&text, false);
            }
            Act::Find => self.editor_find(false),
            Act::Replace => self.editor_find(true),
            Act::FindNext | Act::FindPrevious => {
                let query = self.searches().query.clone();
                match query.text.is_empty() {
                    true => self.editor_find(false),
                    false => self.find_in_editor(&query, query.backwards != (act == Act::FindPrevious)),
                }
            }
            Act::GoToLine => {
                let form = Form::new(titled(t!("editor.go_to_line.title"))).text(&t!("editor.go_to_line.label"), "", Vec::new());
                self.form = Some((form, Purpose::Edit(Asked::GoToLine)));
            }
            Act::LineNumbers | Act::Whitespace | Act::Wrap => {
                let options = &mut editor.options;
                match act {
                    Act::LineNumbers => options.line_numbers = !options.line_numbers,
                    Act::Whitespace => options.whitespace = !options.whitespace,
                    _ => options.wrap = !options.wrap,
                }
                let kept = (options.line_numbers, options.whitespace, options.wrap);
                (editor.top_row, editor.follow) = (0, true);
                self.keep_view(kept);
            }
            act => editor.run(act),
        }
    }

    /// Line numbers, whitespace and word wrap stay for the next file, the window's editor's too.
    fn keep_view(&mut self, (numbers, whitespace, wrap): (bool, bool, bool)) {
        let saved = Settings::load().and_then(|mut fresh| {
            (fresh.editor.line_numbers, fresh.editor.whitespace, fresh.editor.wrap) = (numbers, whitespace, wrap);
            fresh.save()
        });
        let kept = &mut self.settings.editor;
        match saved {
            Ok(()) => (kept.line_numbers, kept.whitespace, kept.wrap) = (numbers, whitespace, wrap),
            Err(err) => self.say(t!("main.error.save_settings", err = err), true),
        }
    }

    /// Copy and Cut: to scmd's own clipboard, and the terminal's (OSC 52) where it takes it.
    fn copy_out(&mut self, cut: bool) {
        let Some(editor) = self.editor.as_mut() else { return };
        let range = editor.selected();
        if range.is_empty() {
            return;
        }
        let text = editor.buffer.text()[range.clone()].to_owned();
        if cut {
            editor.buffer.replace(range, "", false);
            editor.changed();
        }
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&text);
        let mut out = io::stdout();
        let _ = io::Write::write_all(&mut out, format!("\x1b]52;c;{encoded}\x07").as_bytes()).and_then(|()| io::Write::flush(&mut out));
        self.clipboard = text;
    }

    /// F2: written as it was read, CRLF kept; a new file is named first.
    fn save_editor(&mut self, then_close: bool) {
        let Some(editor) = self.editor.as_ref() else { return };
        let Some(path) = editor.path.clone() else { return self.ask_save_as(then_close) };
        if editor.forced {
            let form = Form::new(t!("editor.save_anyway.title"))
                .note(&t!("editor.save_anyway.text"))
                .buttons(vec![t!("editor.save_anyway.button"), t!("editor.dialog.cancel")]);
            return self.form = Some((form, Purpose::Edit(Asked::SaveAnyway(then_close))));
        }
        self.write_editor(path, then_close);
    }

    fn write_editor(&mut self, path: PathBuf, then_close: bool) {
        let Some(editor) = self.editor.as_mut() else { return };
        let text = editor.buffer.text();
        let written = match editor.crlf {
            true => edit::save_text(&path, &text.replace('\n', "\r\n")),
            false => edit::save_text(&path, text),
        };
        // shortcut: a file only an administrator may write is refused here; the window's editor offers to save it as one.
        if let Err(err) = written {
            return self.say(t!("editor.save.failed", path = path.display(), err = err), true);
        }
        if editor.path.as_ref() != Some(&path) {
            // Another file now: the server's is saved back no more.
            (editor.path, editor.title, editor.upload) = (Some(path), None, None);
            editor.set_language();
        }
        editor.forced = false;
        editor.buffer.saved();
        if then_close {
            self.editor_closed();
        }
    }

    fn ask_save_as(&mut self, then_close: bool) {
        let Some(editor) = self.editor.as_ref() else { return };
        let name = editor.path.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        let form = Form::new(titled(t!("editor.file.save_as"))).text(&t!("scmd.editor.file_name"), &name, Vec::new());
        self.form = Some((form, Purpose::Edit(Asked::SaveAs(then_close))));
    }

    /// F10: changes not saved ask first.
    fn close_editor(&mut self) {
        let Some(editor) = self.editor.as_ref() else { return };
        if !editor.buffer.modified() {
            return self.editor_closed();
        }
        let form = Form::new(t!("editor.close.title"))
            .note(&t!("editor.close.unsaved", name = editor.name()))
            .buttons(vec![t!("editor.close.save"), t!("editor.close.discard"), t!("editor.dialog.cancel")]);
        self.form = Some((form, Purpose::Edit(Asked::Close)));
    }

    /// A server's or an archive's copy goes back if it changed; the panels show the file as it is now.
    pub fn editor_closed(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        let Some(path) = editor.path else { return };
        if let Some((upload, before)) = editor.upload {
            self.send_back(path.clone(), before, upload);
        }
        if let Some(folder) = path.parent() {
            for side in &mut self.sides {
                side.tabs.reload_folder(folder);
            }
        }
    }

    /// The file again from the disk; changes not saved ask first.
    fn reload_editor(&mut self, ask: bool) {
        let Some(editor) = self.editor.as_mut() else { return };
        let Some(path) = editor.path.clone() else { return };
        if ask && editor.buffer.modified() {
            let form = Form::new(t!("editor.reload.title"))
                .note(&t!("editor.reload.text"))
                .buttons(vec![t!("editor.reload.button"), t!("editor.dialog.cancel")]);
            return self.form = Some((form, Purpose::Edit(Asked::Reload)));
        }
        match edit::load_text(&path, editor.options.limit_mb) {
            Ok(Loaded::Text(text)) => editor.set_text(text),
            Ok(Loaded::Unfit { why, .. }) => self.say(t!("editor.reload.unfit", why = why), true),
            Err(err) => self.say(t!("editor.reload.failed", err = err), true),
        }
    }

    /// F7 and F4: Find, or Replace; a selection in one line is what they look for.
    fn editor_find(&mut self, replace: bool) {
        let Some(editor) = self.editor.as_ref() else { return };
        let selected = &editor.buffer.text()[editor.selected()];
        let seed = (!selected.is_empty() && !selected.contains('\n')).then(|| selected.to_owned());
        let with = editor.replace_with.clone();
        let searches = self.searches();
        let (query, history) = (searches.query.clone(), searches.history.clone());
        let text = seed.unwrap_or(query.text);
        let form = match replace {
            false => Form::new(t!("scmd.find.title")).text(&t!("scmd.find.search_for"), &text, history),
            true => Form::new(t!("scmd.editor.replace.title"))
                .text(&t!("scmd.find.search_for"), &text, history)
                .text(&t!("scmd.editor.replace.with"), &with, Vec::new()),
        };
        let form = form
            .check(&t!("scmd.find.whole_words"), query.whole_words)
            .check(&t!("scmd.find.case_sensitive"), query.case_sensitive)
            .check(&t!("scmd.find.regex"), query.regex);
        let form = if replace { form } else { form.check(&t!("scmd.find.backwards"), query.backwards) };
        self.form = Some((form, Purpose::Edit(if replace { Asked::Replace } else { Asked::Find })));
    }

    pub fn editor_answered(&mut self, asked: Asked, form: &Form) {
        match asked {
            Asked::Unfit if form.button() == 0 => {
                if let Some(editor) = self.editor.as_mut()
                    && let Some((_, text)) = editor.unfit.take()
                {
                    editor.set_text(text);
                    editor.forced = true;
                }
            }
            Asked::Unfit => self.editor_closed(),
            Asked::Find | Asked::Replace => {
                let replace = matches!(asked, Asked::Replace);
                let checks = if replace { 2 } else { 1 };
                let query = Query {
                    text: form.value(0).to_owned(),
                    whole_words: form.checked(checks),
                    case_sensitive: form.checked(checks + 1),
                    regex: form.checked(checks + 2),
                    backwards: !replace && form.checked(checks + 3),
                    hex: false,
                };
                if query.text.is_empty() {
                    return;
                }
                let searches = self.searches();
                searches.add(query.clone());
                if let Err(err) = searches.save() {
                    self.say(t!("scmd.error.save_searches", err = err), true);
                }
                match replace {
                    true => self.replace_all(&query, form.value(1)),
                    false => self.find_in_editor(&query, query.backwards),
                }
            }
            Asked::GoToLine => {
                let Some(editor) = self.editor.as_mut() else { return };
                let mut numbers = form.value(0).split(':').map(|part| part.trim().parse::<usize>().ok());
                if let Some(Some(line)) = numbers.next() {
                    let column = numbers.next().flatten().unwrap_or(1);
                    let to = editor.line_offset(line.saturating_sub(1), column.saturating_sub(1));
                    editor.go(to, false);
                }
            }
            Asked::SaveAs(then_close) => {
                let typed = form.value(0).trim();
                if typed.is_empty() {
                    return;
                }
                let path = sc_core::command::resolve(typed, &std::env::current_dir().unwrap_or_default());
                let ours = self.editor.as_ref().and_then(|editor| editor.path.as_ref()) == Some(&path);
                match path.exists() && !ours {
                    true => {
                        let name = path.display().to_string();
                        let form = Form::new(titled(t!("editor.file.save_as")))
                            .note(&t!("scmd.editor.exists", name = name))
                            .buttons(vec![t!("transfer.answer.overwrite"), t!("editor.dialog.cancel")]);
                        self.form = Some((form, Purpose::Edit(Asked::Overwrite(path, then_close))));
                    }
                    false => self.write_editor(path, then_close),
                }
            }
            Asked::Overwrite(path, then_close) if form.button() == 0 => self.write_editor(path, then_close),
            Asked::SaveAnyway(then_close) if form.button() == 0 => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.forced = false;
                }
                self.save_editor(then_close);
            }
            Asked::Close => match form.button() {
                0 => self.save_editor(true),
                1 => self.editor_closed(),
                _ => {}
            },
            Asked::Reload if form.button() == 0 => self.reload_editor(false),
            _ => {}
        }
    }

    fn finder(&mut self, query: &Query) -> Option<Finder> {
        let options = Options { match_case: query.case_sensitive, whole_word: query.whole_words, regex: query.regex, preserve_case: false };
        match Finder::new(&query.text, options)? {
            Ok(finder) => Some(finder),
            Err(err) => {
                self.say(err, true);
                None
            }
        }
    }

    /// The next match from the selection on, or the one before it; past the end, from the other end.
    fn find_in_editor(&mut self, query: &Query, backwards: bool) {
        let Some(finder) = self.finder(query) else { return };
        let Some(editor) = self.editor.as_mut() else { return };
        let (text, selected) = (editor.buffer.text(), editor.selected());
        let found = match backwards {
            false => finder.next(text, selected.end).or_else(|| finder.next(text, 0)),
            true => finder.previous(text, selected.start).or_else(|| finder.previous(text, text.len())),
        };
        match found {
            Some(found) => {
                (editor.marking, editor.block, editor.follow) = (false, None, true);
                editor.buffer.select(found);
            }
            None => self.say(t!("scmd.find.not_found", text = query.text), false),
        }
    }

    /// Every match in the selection, else in the text, replaced in one step.
    fn replace_all(&mut self, query: &Query, with: &str) {
        let Some(finder) = self.finder(query) else { return };
        let Some(editor) = self.editor.as_mut() else { return };
        editor.replace_with = with.to_owned();
        let text = editor.buffer.text();
        let scope = Some(editor.selected()).filter(|selected| !selected.is_empty()).unwrap_or(0..text.len());
        let found = finder.matches(text, scope.clone());
        let Some(first) = found.first().map(|found| found.start) else {
            return self.say(t!("scmd.find.not_found", text = query.text), false);
        };
        let (mut changed, mut from) = (String::new(), scope.start);
        for each in &found {
            changed.push_str(&text[from..each.start]);
            changed.push_str(&finder.replacement(text, scope.clone(), each.clone(), with));
            from = each.end;
        }
        changed.push_str(&text[from..scope.end]);
        editor.buffer.replace(scope, &changed, false);
        editor.changed();
        editor.buffer.cursor = first;
        self.say(t!("scmd.editor.replace.done", count = found.len()), false);
    }
}

/// The whole screen: the file's name and the caret's place, the text, the keys.
pub fn draw(frame: &mut Frame, editor: &mut Editor, caret: bool) {
    let area = frame.area();
    if area.height < 3 {
        return;
    }
    let skin = skin();
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    let lines = editor.buffer.lines();
    let gutter = if editor.options.line_numbers { lines.to_string().len() as u16 + 1 } else { 0 };
    let (room, height) = (body.width.saturating_sub(gutter) as usize, body.height as usize);
    (editor.body, editor.gutter, editor.room) = (body, gutter, room);
    let tab = editor.tab();
    let wrap = editor.options.wrap;
    editor.top = editor.top.min(lines - 1);
    editor.top_row = editor.top_row.min(editor.rows(editor.top).len() - 1);
    let (line, column) = editor.place();
    let (_, caret_row, caret_column) = editor.spot();
    if editor.follow {
        editor.follow = false;
        let (caret, top) = ((line, caret_row), (editor.top, editor.top_row));
        let mut below = 0;
        while below < height && editor.step(top, below as isize) < caret {
            below += 1;
        }
        if caret < top {
            (editor.top, editor.top_row) = caret;
        } else if below >= height {
            (editor.top, editor.top_row) = editor.step(caret, 1 - height as isize);
        }
        editor.left = match wrap {
            true => 0,
            false => editor.left.clamp((caret_column + 1).saturating_sub(room), caret_column),
        };
    }
    if !wrap {
        editor.top = editor.top.min(lines.saturating_sub(height));
    }
    editor.read_opens(editor.top + height);
    let buf = frame.buffer_mut();
    buf.set_style(body, skin.editor);
    let marked = editor.selected();
    let text = editor.buffer.text();
    let (mut n, mut row, mut caret_y) = (editor.top, editor.top_row, None);
    for y in body.y..body.bottom() {
        if n >= lines {
            break;
        }
        if gutter > 0 && row == 0 {
            let number = format!("{:>width$} ", n + 1, width = gutter as usize - 1);
            buf.set_string(body.x, y, number, skin.editor_line_numbers);
        }
        if (n, row) == (line, caret_row) {
            caret_y = Some(y);
        }
        let range = editor.buffer.line_range(n);
        let rows = editor.rows(n);
        let (from, to) = (rows[row], rows.get(row + 1).copied().unwrap_or(range.len()));
        let kinds = editor.syntax.map(|syntax| syntax.line(&text[range.clone()], editor.opens[n]).0).unwrap_or_default();
        let mut kinds = kinds.iter().peekable();
        let mut spans: Vec<Span> = Vec::new();
        let mut at = columns(&text[range.start..range.start + from], tab);
        let shown = at + editor.left..at + editor.left + room;
        for (ix, c) in text[range.start + from..range.start + to].char_indices() {
            let ix = from + ix;
            while kinds.next_if(|(kind, _)| kind.end <= ix).is_some() {}
            let coloured = match kinds.peek() {
                Some((kind, which)) if kind.start <= ix => skin.editor.patch(skin.syntax[*which as usize]),
                _ => skin.editor,
            };
            let wide = width(c, at, tab);
            let (start, end) = (at, at + wide);
            at = end;
            if end <= shown.start {
                continue;
            }
            if start >= shown.end {
                break;
            }
            let (mut cells, style) = match c {
                '\t' if editor.options.whitespace => (format!("→{}", " ".repeat(wide - 1)), skin.editor_whitespace),
                '\t' => (" ".repeat(wide), coloured),
                ' ' if editor.options.whitespace => ("·".into(), skin.editor_whitespace),
                c if c.is_control() => {
                    let symbol = char::from_u32(0x2400 + c as u32).filter(|_| c < ' ').unwrap_or('␡');
                    (symbol.into(), skin.editor_nonprintable)
                }
                c => (c.into(), coloured),
            };
            // A wide character cut by an edge: blanks for its part in sight.
            if start < shown.start || end > shown.end {
                cells = " ".repeat(end.min(shown.end) - start.max(shown.start));
            }
            let style = if marked.contains(&(range.start + ix)) { skin.editor_marked } else { style };
            match spans.last_mut() {
                Some(span) if span.style == style => span.content.to_mut().push_str(&cells),
                _ => spans.push(Span::styled(cells, style)),
            }
        }
        // A selected line end: a cell of its own.
        if to == range.len() && marked.contains(&range.end) && range.end < text.len() && shown.contains(&at) {
            spans.push(Span::styled(" ", skin.editor_marked));
        }
        buf.set_line(body.x + gutter, y, &Line::from(spans), room as u16);
        row += 1;
        if row == rows.len() {
            (n, row) = (n + 1, 0);
        }
    }

    let top = Rect::new(area.x, area.y, area.width, 1);
    match editor.menus.is_open() {
        true => editor.menus.draw(buf, top, area),
        false => {
            editor.menus.hidden();
            let count = text[marked].chars().count();
            let (line, column) = (line + 1, column + 1);
            let place = match count {
                0 => t!("editor.status.position", line = line, column = column),
                count => t!("editor.status.position_selected", line = line, column = column, count = count),
            };
            let indent = match editor.options.hard_tabs {
                true => t!("editor.status.tab_size", size = tab),
                false => t!("editor.status.spaces", size = tab),
            };
            let ends = if editor.crlf { "CRLF" } else { "LF" };
            let right = format!(" {place} · {indent} · {ends} ");
            buf.set_style(top, skin.status);
            let mark = if editor.buffer.modified() { "*" } else { "" };
            let room = (area.width as usize).saturating_sub(right.width() + 1);
            buf.set_string(area.x + 1, area.y, ui::tail(&format!("{mark}{}", editor.name()), room), skin.status);
            buf.set_string(area.right().saturating_sub(right.width() as u16), area.y, &right, skin.status);
        }
    }
    ui::key_bar(buf, Rect::new(area.x, area.bottom() - 1, area.width, 1), &keys());
    let column = caret_column.checked_sub(editor.left).filter(|column| *column < room);
    if let (true, false, Some(y), Some(column)) = (caret, editor.menus.is_open(), caret_y, column) {
        frame.set_cursor_position(Position::new(body.x + gutter + column as u16, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::session::Session;

    #[test]
    fn wraps_after_spaces_else_where_full() {
        assert_eq!(breaks("one two three", 8, 4), [0, 8]);
        assert_eq!(breaks("abcdefghij", 4, 4), [0, 4, 8]);
        assert_eq!(breaks("a\tbcdef", 6, 4), [0, 2]);
        assert_eq!(breaks("short", 8, 4), [0]);
    }

    #[test]
    fn marks_copies_indents_undoes_and_saves() {
        let dir = std::env::temp_dir().join(format!("sc-tui-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "one\r\n\ttwo\r\n").unwrap();
        let mut app = App::new(Settings::default(), &Session::default(), std::iter::empty());
        app.open_editor(Some(file.clone()), None, None);
        let mut press = |code, modifiers| app.key(KeyEvent::new(code, modifiers));
        let none = KeyModifiers::NONE;
        // F3 marks the first line, F5 copies it to the end.
        for code in [KeyCode::F(3), KeyCode::Down, KeyCode::F(3), KeyCode::Down, KeyCode::Down, KeyCode::F(5)] {
            press(code, none);
        }
        // Enter keeps the tab.
        for code in [KeyCode::Up, KeyCode::End, KeyCode::Enter, KeyCode::Char('x')] {
            press(code, none);
        }
        press(KeyCode::Char('z'), KeyModifiers::CONTROL);
        press(KeyCode::F(2), none);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\r\n\ttwo\r\n\t\r\none\r\n");
        press(KeyCode::F(10), none);
        assert!(app.editor.is_none(), "saved: F10 closes at once");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
