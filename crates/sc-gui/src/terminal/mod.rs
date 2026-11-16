//! The terminal under the panels: a shell on a pseudo-terminal, its screen
//! kept by alacritty_terminal (Apache 2.0); drawn by `draw`, its keys and
//! mouse put into the bytes xterm's control sequences specify by `keys` and
//! `mouse`.

mod draw;
mod keys;
mod mouse;
pub mod pane;

use std::borrow::Cow;
use std::io;
use std::path::Path;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::*;

/// The shell has ended.
pub struct Exited;

actions!(terminal, [SendTab, SendBackTab, Interrupt, CopySelection, Paste, NewTerminalTab, CloseTerminalTab, NextTerminalTab, PreviousTerminalTab]);

/// New tab, close tab, next and previous: Konsole's and GNOME Terminal's
/// keys, Terminal.app's on macOS.
#[cfg(not(target_os = "macos"))]
const TAB_KEYS: [&str; 4] = ["ctrl-shift-t", "ctrl-shift-w", "ctrl-pagedown", "ctrl-pageup"];
#[cfg(target_os = "macos")]
const TAB_KEYS: [&str; 4] = ["cmd-t", "cmd-w", "cmd-shift-]", "cmd-shift-["];

/// Keys the terminal keeps for itself, whatever the app binds them to.
pub fn own_key(key: &str) -> bool {
    const KEPT: [&str; 9] = ["tab", "shift-tab", "ctrl-c", "cmd-c", "ctrl-shift-c", "ctrl-shift-v", "cmd-v", "ctrl-insert", "shift-insert"];
    let key = key.replace("secondary-", if cfg!(target_os = "macos") { "cmd-" } else { "ctrl-" });
    TAB_KEYS.contains(&key.as_str()) || KEPT.contains(&key.as_str())
}

/// The terminal's keys; Tab and Ctrl+C, which the app's root would take
/// (focus, copy), go to the shell.
pub fn bind_keys(cx: &mut App) {
    let here = Some("Terminal");
    let [new, close, next, previous] = TAB_KEYS;
    let mut keys = vec![
        KeyBinding::new("tab", SendTab, here),
        KeyBinding::new("shift-tab", SendBackTab, here),
        // Copy and paste as terminals have them; the right-click menu shows the last bound.
        KeyBinding::new("ctrl-insert", CopySelection, here),
        KeyBinding::new("shift-insert", Paste, here),
        KeyBinding::new(new, NewTerminalTab, here),
        KeyBinding::new(close, CloseTerminalTab, here),
        KeyBinding::new(next, NextTerminalTab, here),
        KeyBinding::new(previous, PreviousTerminalTab, here),
    ];
    match cfg!(target_os = "macos") {
        true => keys.extend([KeyBinding::new("cmd-c", CopySelection, here), KeyBinding::new("cmd-v", Paste, here)]),
        false => keys.extend([
            KeyBinding::new("ctrl-c", Interrupt, here),
            KeyBinding::new("ctrl-shift-c", CopySelection, here),
            KeyBinding::new("ctrl-shift-v", Paste, here),
        ]),
    }
    // Windows Terminal's: Ctrl+V pastes (and Ctrl+C copies a selection, `interrupt`).
    if cfg!(windows) {
        keys.push(KeyBinding::new("ctrl-v", Paste, here));
    }
    cx.bind_keys(keys);
}

/// alacritty's events, carried to the view's thread.
#[derive(Clone)]
struct Events(UnboundedSender<TermEvent>);

impl EventListener for Events {
    fn send_event(&self, event: TermEvent) {
        let _ = self.0.unbounded_send(event);
    }
}

/// The grid's size in cells.
#[derive(Clone, Copy, PartialEq)]
struct Cells {
    columns: usize,
    lines: usize,
}

impl Dimensions for Cells {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

fn window_size(cells: Cells, cell: Size<Pixels>) -> WindowSize {
    WindowSize {
        num_lines: cells.lines as u16,
        num_cols: cells.columns as u16,
        cell_width: f32::from(cell.width) as u16,
        cell_height: f32::from(cell.height) as u16,
    }
}

pub struct TerminalView {
    term: Arc<FairMutex<Term<Events>>>,
    pty: EventLoopSender,
    pub focus: FocusHandle,
    /// What the shell calls itself ("user@host: ~/src").
    pub title: Option<String>,
    cells: Cells,
    /// A cell's size, and the first cell's corner on screen.
    cell: Size<Pixels>,
    origin: Point<Pixels>,
    /// The theme's foreground, background and darkness, told to programs
    /// that ask for their colours.
    theme: (Hsla, Hsla, bool),
    /// The wheel's movement short of a line, kept for the next.
    wheel_rest: f32,
    /// The cell a move was reported at last: each is reported once.
    last_report: Option<(usize, usize)>,
    /// The pty's descriptor and the shell's process, to tell whether it
    /// waits at its prompt.
    #[cfg(unix)]
    pty_fd: i32,
    #[cfg(unix)]
    shell_pid: i32,
    /// The shell's process group as it first stood in front: under macOS's
    /// login it is not the started process's.
    #[cfg(unix)]
    shell_group: Option<i32>,
    _events: Task<()>,
    _focus: [Subscription; 2],
}

impl EventEmitter<Exited> for TerminalView {}

impl Drop for TerminalView {
    fn drop(&mut self) {
        // Its loop ends and the pty hangs up: the shell gets SIGHUP.
        let _ = self.pty.send(Msg::Shutdown);
    }
}

impl TerminalView {
    /// The user's shell in `dir`, or `program`.
    pub fn open(dir: &Path, program: Option<(String, Vec<String>)>, window: &mut Window, cx: &mut App) -> io::Result<Entity<Self>> {
        let (cells, cell) = (Cells { columns: 80, lines: 24 }, size(px(8.), px(16.)));
        let (sender, mut received) = unbounded();
        let events = Events(sender);
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &cells, events.clone())));
        let options = tty::Options {
            shell: program.map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: Some(dir.to_owned()),
            env: [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")].map(|(name, value)| (name.to_owned(), value.to_owned())).into(),
            #[cfg(windows)]
            escape_args: true,
            ..Default::default()
        };
        let pty = tty::new(&options, window_size(cells, cell), 0)?;
        #[cfg(unix)]
        let (pty_fd, shell_pid) = (std::os::fd::AsRawFd::as_raw_fd(pty.file()), pty.child().id() as i32);
        let reader = EventLoop::new(term.clone(), events, pty, false, false)?;
        let pty = reader.channel();
        reader.spawn();
        Ok(cx.new(|cx| {
            let focus = cx.focus_handle();
            let carried = cx.spawn(async move |this, cx| {
                while let Some(event) = received.next().await {
                    if this.update(cx, |this: &mut Self, cx| this.heard(event, cx)).is_err() {
                        return;
                    }
                }
            });
            let gained = cx.on_focus_in(&focus, window, |this, _, cx| this.focus_changed(true, cx));
            let lost = cx.on_focus_out(&focus, window, |this, _, _, cx| this.focus_changed(false, cx));
            Self {
                term,
                pty,
                focus,
                title: None,
                cells,
                cell,
                origin: Point::default(),
                theme: (Hsla::default(), Hsla::default(), true),
                wheel_rest: 0.,
                last_report: None,
                #[cfg(unix)]
                pty_fd,
                #[cfg(unix)]
                shell_pid,
                #[cfg(unix)]
                shell_group: None,
                _events: carried,
                _focus: [gained, lost],
            }
        }))
    }

    /// `text` as if typed: the command line's commands come so.
    pub fn send(&mut self, text: &str) {
        self.type_bytes(text.as_bytes().to_vec());
    }

    /// The shell wrote something, as its first prompt: lines sent before it would show twice.
    pub fn started(&self) -> bool {
        self.term.lock().grid().cursor.point != GridPoint::default()
    }

    /// Whether the shell waits at its prompt, nothing running in front of it;
    /// none where that cannot be told (Windows).
    pub fn idle(&self) -> Option<bool> {
        #[cfg(unix)]
        // SAFETY: asks the pty, which the event loop keeps open, who is in front.
        return Some(unsafe { libc::tcgetpgrp(self.pty_fd) } == self.shell_group.unwrap_or(self.shell_pid));
        #[cfg(not(unix))]
        None
    }

    fn heard(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        match event {
            TermEvent::Wakeup => {
                #[cfg(unix)]
                if self.shell_group.is_none() {
                    // SAFETY: asks the pty, which the event loop keeps open, who is in front.
                    let front = unsafe { libc::tcgetpgrp(self.pty_fd) };
                    self.shell_group = (front > 0).then_some(front);
                }
                cx.notify();
            }
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => cx.notify(),
            TermEvent::Title(title) => {
                self.title = Some(title).filter(|title| !title.trim().is_empty());
                cx.notify();
            }
            TermEvent::ResetTitle => {
                self.title = None;
                cx.notify();
            }
            // A program's own answers, and OSC 52's clipboard.
            TermEvent::PtyWrite(text) => self.write(text.into_bytes()),
            TermEvent::ClipboardStore(_, text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            TermEvent::ClipboardLoad(_, answer) => {
                let text = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
                self.write(answer(&text).into_bytes());
            }
            TermEvent::ColorRequest(index, answer) => {
                let rgb = self.term.lock().colors()[index].unwrap_or_else(|| self.theme_rgb(index));
                self.write(answer(rgb).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(answer) => self.write(answer(window_size(self.cells, self.cell)).into_bytes()),
            TermEvent::Exit | TermEvent::ChildExit(_) => cx.emit(Exited),
            TermEvent::Bell => {}
        }
    }

    /// Colour `index` as drawn when no program set it.
    fn theme_rgb(&self, index: usize) -> Rgb {
        let (foreground, background, dark) = self.theme;
        let color: Hsla = match index {
            0..=255 => rgb(ansi(index as u8, dark)).into(),
            i if i == NamedColor::Background as usize => background,
            _ => foreground,
        };
        let Rgba { r, g, b, .. } = color.into();
        let byte = |part: f32| (part * 255.).round() as u8;
        Rgb { r: byte(r), g: byte(g), b: byte(b) }
    }

    fn write(&self, bytes: Vec<u8>) {
        let _ = self.pty.send(Msg::Input(Cow::Owned(bytes)));
    }

    /// What the user sends: the view back at the newest lines, the selection gone.
    fn type_bytes(&mut self, bytes: Vec<u8>) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Bottom);
        term.selection = None;
        drop(term);
        self.write(bytes);
    }

    fn focus_changed(&mut self, focused: bool, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        term.is_focused = focused;
        // Told to a program that asked (DECSET 1004).
        let tell = term.mode().contains(TermMode::FOCUS_IN_OUT);
        drop(term);
        if tell {
            self.write(if focused { b"\x1b[I" } else { b"\x1b[O" }.to_vec());
        }
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    fn scrolled_back(&self) -> bool {
        self.term.lock().grid().display_offset() != 0
    }

    fn scroll(&mut self, to: Scroll, cx: &mut Context<Self>) {
        self.term.lock().scroll_display(to);
        cx.notify();
    }

    fn selected(&self) -> Option<String> {
        self.term.lock().selection_to_string().filter(|text| !text.is_empty())
    }

    fn copy(&self, cx: &mut App) {
        if let Some(text) = self.selected() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Ctrl+C interrupts; on Windows a selection is copied instead, as Windows Terminal does.
    fn interrupt(&mut self, _: &Interrupt, _: &mut Window, cx: &mut Context<Self>) {
        if cfg!(windows) && self.selected().is_some() {
            self.copy(cx);
            self.term.lock().selection = None;
            return cx.notify();
        }
        self.type_bytes(vec![0x03]);
    }

    /// `text` pasted: marked as pasted where the program asked (DECSET 2004),
    /// with no ESC that could end the mark early; else line ends as Enter.
    fn paste(&mut self, text: &str) {
        let bytes = match self.mode().contains(TermMode::BRACKETED_PASTE) {
            true => format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")),
            false => text.replace("\r\n", "\r").replace('\n', "\r"),
        };
        self.type_bytes(bytes.into_bytes());
    }

    fn paste_clipboard(&mut self, cx: &mut App) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste(&text);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let held = key.modifiers;
        let name = key.key.to_ascii_lowercase();
        let only = |shift: bool, control: bool| held.shift == shift && held.control == control && !held.alt;
        // The history: Shift+PageUp/PageDown pages it, Ctrl+End leaves it.
        if (name == "pageup" || name == "pagedown") && held.shift && !self.mode().contains(TermMode::ALT_SCREEN) {
            self.scroll(if name == "pageup" { Scroll::PageUp } else { Scroll::PageDown }, cx);
        } else if name == "end" && only(false, true) && self.scrolled_back() {
            self.scroll(Scroll::Bottom, cx);
        } else if event.prefer_character_input && key.key_char.is_some() {
            // A character (AltGr's too) comes through the input method.
            return;
        } else if let Some(bytes) = keys::bytes(key, self.mode()) {
            self.type_bytes(bytes);
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// The column and line on screen under `position`, and the half of the cell it is in.
    fn cell_under(&self, position: Point<Pixels>) -> (usize, usize, Side) {
        let x = ((position.x - self.origin.x) / self.cell.width).max(0.);
        let y = ((position.y - self.origin.y) / self.cell.height).max(0.);
        let side = if x.fract() > 0.5 { Side::Right } else { Side::Left };
        ((x as usize).min(self.cells.columns - 1), (y as usize).min(self.cells.lines - 1), side)
    }

    /// The grid's point at the screen's `column` and `line`, the history scrolled or not.
    fn grid_point(term: &Term<Events>, column: usize, line: usize) -> GridPoint {
        let back = term.grid().display_offset() as i32;
        GridPoint::new(Line(line as i32 - back), Column(column))
    }

    /// `event` reported to the program, if it asked for the mouse; Shift
    /// keeps the mouse ours (to select).
    fn report(&mut self, event: mouse::Event, position: Point<Pixels>, held: Modifiers) -> bool {
        if held.shift {
            return false;
        }
        let (column, line, _) = self.cell_under(position);
        let Some(bytes) = mouse::report(event, column, line, held, self.mode()) else {
            return false;
        };
        self.write(bytes);
        true
    }

    /// A right click goes to a program that takes the mouse (mc), not to the menu.
    fn right_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Right {
            window.focus(&self.focus, cx);
            if self.report(mouse::Event::Button(MouseButton::Right, true), event.position, event.modifiers) {
                cx.stop_propagation();
            }
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.report(mouse::Event::Button(event.button, true), event.position, event.modifiers) {
            return;
        }
        if event.button == MouseButton::Left {
            // A word on a double click, lines on a triple.
            let kind = match event.click_count {
                0 | 1 => SelectionType::Simple,
                2 => SelectionType::Semantic,
                _ => SelectionType::Lines,
            };
            let (column, line, side) = self.cell_under(event.position);
            let mut term = self.term.lock();
            let at = Self::grid_point(&term, column, line);
            term.selection = Some(Selection::new(kind, at, side));
            drop(term);
            cx.notify();
        }
        // X11's way: the middle button pastes what was selected last, anywhere.
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if event.button == MouseButton::Middle
            && let Some(text) = cx.read_from_primary().and_then(|item| item.text())
        {
            self.paste(&text);
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.report(mouse::Event::Button(event.button, false), event.position, event.modifiers) {
            return;
        }
        // A selection made is X11's to paste with the middle button.
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if event.button == MouseButton::Left
            && let Some(text) = self.selected()
        {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
        let _ = cx;
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (column, line, side) = self.cell_under(event.position);
        if self.last_report != Some((column, line)) && self.report(mouse::Event::Move(event.pressed_button), event.position, event.modifiers) {
            self.last_report = Some((column, line));
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let mut term = self.term.lock();
        let at = Self::grid_point(&term, column, line);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(at, side);
            drop(term);
            cx.notify();
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        // A touchpad's pixels add up to whole lines.
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y as i32,
            ScrollDelta::Pixels(delta) => {
                self.wheel_rest += delta.y / self.cell.height;
                let whole = self.wheel_rest.trunc();
                self.wheel_rest -= whole;
                whole as i32
            }
        };
        if lines == 0 {
            return;
        }
        let mode = self.mode();
        if mode.intersects(TermMode::MOUSE_MODE) && !event.modifiers.shift {
            for _ in 0..lines.unsigned_abs() {
                self.report(mouse::Event::Wheel(lines > 0), event.position, event.modifiers);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // A full-screen program (less, an editor) scrolls by its arrow keys.
            let arrow: &[u8] = if lines > 0 { b"\x1bOA" } else { b"\x1bOB" };
            self.write(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            self.scroll(Scroll::Delta(lines), cx);
        }
    }

    /// The grid sized to the space drawn in; the pty told when it changes.
    fn fit(&mut self, origin: Point<Pixels>, cell: Size<Pixels>, cells: Cells, theme: (Hsla, Hsla, bool)) {
        (self.origin, self.cell, self.theme) = (origin, cell, theme);
        if self.cells != cells {
            self.cells = cells;
            self.term.lock().resize(cells);
            let _ = self.pty.send(Msg::Resize(window_size(cells, cell)));
        }
    }
}

/// The 256 colours: the first 16 as VS Code's terminal has them, then
/// xterm's 6×6×6 cube and its 24 greys.
fn ansi(index: u8, dark: bool) -> u32 {
    const DARK: [u32; 16] = [
        0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea,
        0xd670d6, 0x29b8db, 0xe5e5e5,
    ];
    const LIGHT: [u32; 16] = [
        0x000000, 0xcd3131, 0x00bc00, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555, 0x666666, 0xcd3131, 0x14ce14, 0xb5ba00, 0x0451a5,
        0xbc05bc, 0x0598bc, 0xa5a5a5,
    ];
    match index {
        0..16 => (if dark { DARK } else { LIGHT })[usize::from(index)],
        16..232 => {
            let step = |level: u8| if level == 0 { 0 } else { 55 + 40 * u32::from(level) };
            let cube = index - 16;
            step(cube / 36) << 16 | step(cube / 6 % 6) << 8 | step(cube % 6)
        }
        _ => {
            let grey = 8 + 10 * u32::from(index - 232);
            grey << 16 | grey << 8 | grey
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (view, focus) = (cx.entity(), self.focus.clone());
        let (drawn, menu_view, menu_focus) = (view.clone(), view.downgrade(), focus.clone());
        let theme = cx.theme();
        // Back in the history: a way to the newest lines.
        let to_bottom = self.scrolled_back().then(|| {
            let button = div()
                .id("jump-to-bottom")
                .px_2()
                .py_0p5()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .text_sm()
                .cursor_pointer()
                .child("Jump to bottom (Ctrl+End) ↓")
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.scroll(Scroll::Bottom, cx);
                }));
            div().absolute().bottom_2().left_0().right_0().flex().justify_center().child(button)
        });
        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .px_1()
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &SendTab, _, _| this.type_bytes(b"\t".to_vec())))
            .on_action(cx.listener(|this, _: &SendBackTab, _, _| this.type_bytes(b"\x1b[Z".to_vec())))
            .on_action(cx.listener(Self::interrupt))
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste_clipboard(cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .capture_any_mouse_down(cx.listener(Self::right_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .child(
                canvas(move |bounds, window, cx| draw::layout(&view, bounds, window, cx), move |_, screen, window, cx| draw::paint(screen, drawn, &focus, window, cx))
                    .size_full(),
            )
            .children(to_bottom)
            .context_menu(move |menu, _, cx| {
                let selected = menu_view.read_with(cx, |this, _| this.selected().is_some()).unwrap_or(false);
                menu.action_context(menu_focus.clone())
                    .menu_with_disabled("Copy", Box::new(CopySelection), !selected)
                    .menu("Paste", Box::new(Paste))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::ansi;

    #[test]
    fn colours_256() {
        assert_eq!(ansi(1, true), 0xcd3131);
        assert_eq!(ansi(16, true), 0x000000);
        assert_eq!(ansi(231, true), 0xffffff);
        assert_eq!(ansi(196, true), 0xff0000);
        assert_eq!(ansi(232, true), 0x080808);
        assert_eq!(ansi(255, true), 0xeeeeee);
    }
}
