//! F9's menus, the window's (the core's `MENUS`), or sedit's: their names on
//! the top line, each dropping down under its name, as Midnight Commander's.
//! Every menu and item has its letter: Alt+letter opens a menu, the letter
//! alone picks in it.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, Clear, Widget};
use sc_core::menu::{Command, Item, MENUS, label, menu_label, mnemonics};
use unicode_width::UnicodeWidthStr;

use crate::skin::skin;

pub struct Entry<A = Command> {
    label: String,
    hot: Option<usize>,
    /// Its key, as `Ctrl+D`.
    key: String,
    /// An option that is on.
    checked: bool,
    does: Does<A>,
}

enum Does<A> {
    /// The command, and whether scmd does it yet (else it is dimmed).
    Run(A, bool),
    Workspace(usize),
    Open(Vec<Entry<A>>),
    Line,
}

impl<A> Entry<A> {
    pub fn item(label: String, key: &str, does: A, works: bool, checked: bool) -> Self {
        Entry { label, hot: None, key: key.into(), checked, does: Does::Run(does, works) }
    }

    pub fn line() -> Self {
        Entry { label: String::new(), hot: None, key: String::new(), checked: false, does: Does::Line }
    }
}

/// What was chosen.
pub enum Picked<A = Command> {
    Command(A),
    Workspace(usize),
}

/// What the menus need from the app as they are made.
pub struct Making<'a> {
    pub works: &'a dyn Fn(Command) -> bool,
    pub key_of: &'a dyn Fn(Command) -> String,
    /// Commands the window has and scmd leaves out.
    pub hidden: &'a [Command],
    pub workspaces: &'a [String],
    /// Letters Alt already has a command on.
    pub taken: &'a [char],
}

pub struct Menus<A = Command> {
    top: Vec<(String, Option<usize>, Vec<Entry<A>>)>,
    /// The open menu, and the item at each level: its own, then its open submenus'.
    open: Option<(usize, Vec<usize>)>,
    /// Where the last frame drew the names, and each open level's items (and the first shown).
    names: Vec<Rect>,
    lists: Vec<(Rect, usize)>,
}

impl<A> Default for Menus<A> {
    fn default() -> Self {
        Menus { top: Vec::new(), open: None, names: Vec::new(), lists: Vec::new() }
    }
}

impl Menus<Command> {
    /// Made anew before they open: the workspaces' names change.
    pub fn make(&mut self, making: &Making) {
        let menus = MENUS.iter().map(|menu| (menu_label(menu.label), entries(menu.items, making))).collect();
        self.fill(menus, making.taken);
    }
}

impl<A: Copy> Menus<A> {
    /// These menus, each item lettered; `taken`: letters Alt has a command on.
    pub fn fill(&mut self, menus: Vec<(String, Vec<Entry<A>>)>, taken: &[char]) {
        let names = mnemonics(&menus.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), taken);
        let menus = names.into_iter().zip(menus).map(|((name, hot), (_, entries))| (name, hot, lettered(entries)));
        self.top = menus.collect();
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    pub fn open(&mut self, at: usize) {
        let first = self.top.get(at).map_or(0, |(.., entries)| next(entries, usize::MAX, 1));
        self.open = Some((at, vec![first]));
    }

    /// Alt+letter: the menu with that letter, if one has it.
    pub fn open_by_letter(&mut self, letter: char) -> bool {
        let found = self.top.iter().position(|(name, hot, _)| is_letter(name, *hot, letter));
        found.map(|at| self.open(at)).is_some()
    }

    /// Not drawn (the viewer has the screen): nothing of them to click.
    pub fn hidden(&mut self) {
        self.names.clear();
        self.lists.clear();
    }

    /// Whether `at` is on a menu's name.
    pub fn on_bar(&self, at: Position) -> bool {
        self.names.iter().any(|name| name.contains(at))
    }

    /// The items of open level `level`.
    fn level(&self, level: usize) -> &[Entry<A>] {
        let Some((at, path)) = &self.open else { return &[] };
        let mut entries = &self.top[*at].2[..];
        for &ix in &path[..level] {
            match entries.get(ix).map(|entry| &entry.does) {
                Some(Does::Open(sub)) => entries = sub,
                _ => return &[],
            }
        }
        entries
    }

    pub fn key(&mut self, key: &KeyEvent) -> Option<Picked<A>> {
        let (at, path) = self.open.clone()?;
        let depth = path.len() - 1;
        let here = path[depth];
        let count = self.top.len();
        let switch = |this: &mut Self, by: usize| this.open((at + by) % count);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc if depth > 0 => self.back(),
            KeyCode::Esc | KeyCode::F(9) | KeyCode::F(10) => self.close(),
            KeyCode::Left if depth > 0 => self.back(),
            KeyCode::Left => switch(self, count - 1),
            KeyCode::Right if matches!(self.level(depth).get(here), Some(Entry { does: Does::Open(_), .. })) => return self.pick(depth, here),
            KeyCode::Right => switch(self, 1),
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
                let (from, by) = match key.code {
                    KeyCode::Up => (here, -1),
                    KeyCode::Down => (here, 1),
                    KeyCode::Home => (usize::MAX, 1),
                    _ => (usize::MAX, -1),
                };
                let to = next(self.level(depth), from, by);
                self.set(depth, to);
            }
            KeyCode::Enter => return self.pick(depth, here),
            KeyCode::Char(c) if alt => {
                self.open_by_letter(c);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let found = self.level(depth).iter().position(|entry| is_letter(&entry.label, entry.hot, c));
                return found.and_then(|ix| self.pick(depth, ix));
            }
            _ => {}
        }
        None
    }

    fn back(&mut self) {
        if let Some((_, path)) = self.open.as_mut() {
            path.pop();
        }
    }

    fn set(&mut self, depth: usize, ix: usize) {
        if let Some((_, path)) = self.open.as_mut() {
            path.truncate(depth + 1);
            path[depth] = ix;
        }
    }

    /// Item `ix` of level `depth` chosen: a submenu opens, a command closes the menus.
    fn pick(&mut self, depth: usize, ix: usize) -> Option<Picked<A>> {
        let picked = match &self.level(depth).get(ix)?.does {
            Does::Run(command, _) => Picked::Command(*command),
            Does::Workspace(ix) => Picked::Workspace(*ix),
            Does::Open(sub) => {
                let first = next(sub, usize::MAX, 1);
                self.set(depth, ix);
                if let Some((_, path)) = self.open.as_mut() {
                    path.push(first);
                }
                return None;
            }
            Does::Line => return None,
        };
        self.close();
        Some(picked)
    }

    /// A click: on a name opens (or closes) its menu, on an item picks it, elsewhere closes them.
    pub fn mouse(&mut self, mouse: &MouseEvent) -> Option<Picked<A>> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return None;
        }
        let at = Position::new(mouse.column, mouse.row);
        if let Some(ix) = self.names.iter().position(|name| name.contains(at)) {
            match self.open.as_ref().is_some_and(|(open, _)| *open == ix) {
                true => self.close(),
                false => self.open(ix),
            }
            return None;
        }
        let Some(depth) = self.lists.iter().rposition(|(list, _)| list.contains(at)) else {
            self.close();
            return None;
        };
        let (list, top) = self.lists[depth];
        self.pick(depth, top + (mouse.row - list.y) as usize)
    }

    /// The names on `bar`; the open menus over `screen`.
    pub fn draw(&mut self, buf: &mut Buffer, bar: Rect, screen: Rect) {
        let skin = skin();
        buf.set_style(bar, skin.menu);
        self.names.clear();
        let open = self.open.as_ref().map(|(at, _)| *at);
        let mut x = bar.x + 1;
        for (ix, (name, hot, _)) in self.top.iter().enumerate() {
            let width = (name.width() as u16 + 2).min(bar.right().saturating_sub(x));
            let area = Rect::new(x, bar.y, width, 1);
            let (style, hot_style) = match open == Some(ix) {
                true => (skin.menu_selected, skin.menu_hot_selected),
                false => (skin.menu, skin.menu_hot),
            };
            buf.set_style(area, style);
            write(buf, area.x + 1, area.y, area.width.saturating_sub(1), name, *hot, style, hot_style);
            self.names.push(area);
            x = area.right() + 1;
        }
        self.lists.clear();
        let Some((at, path)) = self.open.clone() else { return };
        let mut parent: Option<(Rect, usize)> = None;
        for depth in 0..path.len() {
            let entries = self.level(depth);
            if entries.is_empty() {
                break;
            }
            let inner = entries.iter().map(|entry| entry.label.width() + entry.key.width() + 4).max().unwrap_or(0).max(12) as u16;
            // Under the bar; a long menu scrolls.
            let below = bar.y + 1;
            let room = screen.bottom().saturating_sub(below);
            let (width, height) = ((inner + 2).min(screen.width), (entries.len() as u16 + 2).min(room));
            let shown = height.saturating_sub(2) as usize;
            let top = (path[depth] + 1).saturating_sub(shown);
            let (x, y) = match parent {
                None => (self.names[at].x.saturating_sub(1), below),
                Some((parent, parent_top)) => {
                    let y = parent.y + (path[depth - 1] - parent_top) as u16;
                    let right = parent.right();
                    // Right of it, else left; else over its right end, its names in sight.
                    match (right + width <= screen.right(), parent.x >= width) {
                        (true, _) => (right, y),
                        (false, true) => (parent.x - width, y),
                        (false, false) => (screen.right().saturating_sub(width), y),
                    }
                }
            };
            let x = x.min(screen.right().saturating_sub(width));
            let y = y.min(screen.bottom().saturating_sub(height)).max(below);
            let area = Rect::new(x, y, width, height);
            Clear.render(area, buf);
            Block::bordered().border_set(skin.frame).style(skin.menu).render(area, buf);
            let list = area.inner(ratatui::layout::Margin::new(1, 1));
            for (ix, entry) in entries.iter().enumerate().skip(top).take(shown) {
                let row = Rect::new(list.x, list.y + (ix - top) as u16, list.width, 1);
                if let Does::Line = entry.does {
                    let [horiz, left, right, _] = skin.split;
                    buf.set_string(area.x, row.y, left, skin.menu);
                    buf.set_string(row.x, row.y, horiz.repeat(row.width as usize), skin.menu);
                    buf.set_string(area.right() - 1, row.y, right, skin.menu);
                    continue;
                }
                let works = !matches!(entry.does, Does::Run(_, false));
                let (style, hot_style) = match (path[depth] == ix, works) {
                    (true, _) => (skin.menu_selected, skin.menu_hot_selected),
                    (false, true) => (skin.menu, skin.menu_hot),
                    // Its letter kept, as in the window; picked, it says it is not there yet.
                    (false, false) => (skin.menu_inactive, skin.menu_inactive.add_modifier(ratatui::style::Modifier::UNDERLINED)),
                };
                buf.set_style(row, style);
                let key = match entry.does {
                    Does::Open(_) => "▸",
                    _ => &entry.key,
                };
                let key_x = row.right().saturating_sub(key.width() as u16 + 1);
                if entry.checked {
                    buf.set_string(row.x, row.y, "✓", style);
                }
                write(buf, row.x + 1, row.y, key_x.saturating_sub(row.x + 2), &entry.label, entry.hot, style, hot_style);
                buf.set_string(key_x, row.y, key, style);
            }
            self.lists.push((list, top));
            parent = Some((area, top));
        }
    }
}

/// The menu's items as scmd shows them: the window's own left out, what scmd
/// does not do yet dimmed.
fn entries(items: &[Item], making: &Making) -> Vec<Entry> {
    let entry = |label: String, key: String, does: Does<Command>| Entry { label, hot: None, key, checked: false, does };
    let mut made: Vec<Entry> = Vec::new();
    for item in items {
        match *item {
            Item::Command { command, .. } | Item::History { command, .. } if !making.hidden.contains(&command) => {
                made.push(entry(label(command), (making.key_of)(command), Does::Run(command, (making.works)(command))));
            }
            Item::Submenu { label, items } => made.push(entry(menu_label(label), String::new(), Does::Open(entries(items, making)))),
            Item::Workspaces { label, items } => {
                let each = making.workspaces.iter().enumerate().map(|(ix, name)| entry(name.clone(), String::new(), Does::Workspace(ix)));
                let mut sub: Vec<Entry> = each.chain([entry(String::new(), String::new(), Does::Line)]).collect();
                sub.extend(entries(items, making));
                made.push(entry(menu_label(label), String::new(), Does::Open(sub)));
            }
            Item::Separator => made.push(entry(String::new(), String::new(), Does::Line)),
            _ => {}
        }
    }
    made
}

/// No line first, last or twice; a letter for each, a submenu's too.
fn lettered<A>(made: Vec<Entry<A>>) -> Vec<Entry<A>> {
    let line = |entry: &Entry<A>| matches!(entry.does, Does::Line);
    let mut kept: Vec<Entry<A>> = Vec::new();
    for mut entry in made {
        if let Does::Open(sub) = entry.does {
            entry.does = Does::Open(lettered(sub));
        }
        if !(line(&entry) && kept.last().is_none_or(line)) {
            kept.push(entry);
        }
    }
    if kept.last().is_some_and(line) {
        kept.pop();
    }
    let hot = mnemonics(&kept.iter().map(|entry| entry.label.as_str()).collect::<Vec<_>>(), &[]);
    for (entry, (shown, at)) in kept.iter_mut().zip(hot) {
        (entry.label, entry.hot) = (shown, at);
    }
    kept
}

/// The next item from `from` going `by` (from the end, `usize::MAX`), lines skipped, round.
fn next<A>(entries: &[Entry<A>], from: usize, by: isize) -> usize {
    let count = entries.len() as isize;
    if count == 0 {
        return 0;
    }
    let mut ix = match from {
        usize::MAX if by > 0 => -1,
        usize::MAX => count,
        from => from as isize,
    };
    for _ in 0..count {
        ix = (ix + by).rem_euclid(count);
        if !matches!(entries[ix as usize].does, Does::Line) {
            return ix as usize;
        }
    }
    0
}

fn is_letter(label: &str, hot: Option<usize>, letter: char) -> bool {
    let lower = |c: char| c.to_lowercase().next();
    hot.and_then(|at| label.chars().nth(at)).is_some_and(|hot| lower(hot) == lower(letter))
}

/// `text` from `x`, at most `width` columns, its letter at `hot` in `hot_style`.
#[allow(clippy::too_many_arguments)]
fn write(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, hot: Option<usize>, style: ratatui::style::Style, hot_style: ratatui::style::Style) {
    buf.set_stringn(x, y, text, width as usize, style);
    let Some((before, letter)) = hot.and_then(|at| Some((text.chars().take(at).collect::<String>(), text.chars().nth(at)?))) else { return };
    let at = before.width() as u16;
    if at < width {
        buf.set_stringn(x + at, y, letter.to_string(), (width - at) as usize, hot_style);
    }
}
