//! The screen drawn: each row of the grid one shaped line, its looks as
//! runs, a cell wide each; backgrounds as rectangles under it; the caret.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use super::{Cells, TerminalView, ansi};

/// What one paint puts on screen.
pub(super) struct Screen {
    line_height: Pixels,
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    rows: Vec<(Point<Pixels>, ShapedLine)>,
    caret: Option<Caret>,
    caret_color: Hsla,
    focused: bool,
}

struct Caret {
    bounds: Bounds<Pixels>,
    shape: CursorShape,
    /// On a block: the character under it, in the background's colour.
    under: Option<ShapedLine>,
}

/// The look of a cell's text; cells of one look share a run.
#[derive(Clone, PartialEq)]
struct Look {
    color: Hsla,
    bold: bool,
    italic: bool,
    underline: Option<bool>,
    strike: bool,
}

/// The palette: the theme's, the 256 colours and a program's own (OSC 4).
struct Palette<'a> {
    set: &'a Colors,
    foreground: Hsla,
    background: Hsla,
    dark: bool,
}

impl Palette<'_> {
    fn of(&self, color: Color) -> Hsla {
        let index = match color {
            Color::Spec(rgb) => return Hsla::from(Rgba { r: f32::from(rgb.r) / 255., g: f32::from(rgb.g) / 255., b: f32::from(rgb.b) / 255., a: 1. }),
            Color::Named(named) => named as usize,
            Color::Indexed(index) => usize::from(index),
        };
        if let Some(rgb) = self.set[index] {
            return self.of(Color::Spec(rgb));
        }
        let dim = NamedColor::DimBlack as usize;
        match index {
            0..=255 => rgb(ansi(index as u8, self.dark)).into(),
            i if i == NamedColor::Background as usize => self.background,
            i if (dim..dim + 8).contains(&i) => Hsla::from(rgb(ansi((i - dim) as u8, self.dark))).opacity(0.7),
            i if i == NamedColor::DimForeground as usize => self.foreground.opacity(0.7),
            _ => self.foreground,
        }
    }
}

/// The text a cell shows, and its look; its background, if not the window's.
fn cell_look(cell: &Cell, palette: &Palette) -> (Look, Option<Hsla>) {
    let flags = cell.flags;
    let (fg, bg) = match flags.contains(Flags::INVERSE) {
        true => (cell.bg, cell.fg),
        false => (cell.fg, cell.bg),
    };
    let mut color = palette.of(fg);
    if flags.contains(Flags::DIM) {
        color = color.opacity(0.7);
    }
    let look = Look {
        color,
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES).then(|| flags.contains(Flags::UNDERCURL)),
        strike: flags.contains(Flags::STRIKEOUT),
    };
    let background = (bg != Color::Named(NamedColor::Background)).then(|| palette.of(bg));
    (look, background)
}

fn run(look: &Look, len: usize, base: &Font) -> TextRun {
    TextRun {
        len,
        font: Font {
            weight: if look.bold { FontWeight::BOLD } else { base.weight },
            style: if look.italic { FontStyle::Italic } else { FontStyle::Normal },
            ..base.clone()
        },
        color: look.color,
        background_color: None,
        underline: look.underline.map(|wavy| UnderlineStyle { color: Some(look.color), thickness: px(1.), wavy }),
        strikethrough: look.strike.then(|| StrikethroughStyle { color: Some(look.color), thickness: px(1.) }),
    }
}

/// One row's text and runs as they are built, column by column.
#[derive(Default)]
struct Row {
    text: String,
    runs: Vec<(Look, usize)>,
    /// Bytes up to the last cell that shows something: blanks after it go.
    shown: usize,
    runs_shown: usize,
}

impl Row {
    fn push(&mut self, text: &str, look: Look, visible: bool) {
        self.text.push_str(text);
        match self.runs.last_mut() {
            Some((last, len)) if *last == look => *len += text.len(),
            _ => self.runs.push((look, text.len())),
        }
        if visible {
            (self.shown, self.runs_shown) = (self.text.len(), self.runs.len());
        }
    }
}

/// Lays the grid out in `bounds`, the view sized to it first.
pub(super) fn layout(view: &Entity<TerminalView>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) -> Screen {
    let theme = cx.theme();
    let (foreground, background, selected, dark) = (theme.foreground, theme.background, theme.selection, theme.mode.is_dark());
    let (font, size) = (font(theme.mono_font_family.clone()), theme.mono_font_size);
    let texts = window.text_system().clone();
    // A cell: an "m" wide, the line 1.3 times the font.
    let advance = texts.advance(texts.resolve_font(&font), size, 'm').map_or(size * 0.6, |advance| advance.width);
    let cell = gpui_kit::size(advance, (size * 1.3).round());
    let cells = Cells {
        columns: ((bounds.size.width / cell.width) as usize).max(2),
        lines: ((bounds.size.height / cell.height) as usize).max(1),
    };
    view.update(cx, |view, _| view.fit(bounds.origin, cell, cells, (foreground, background, dark)));
    let focused = view.read(cx).focus.is_focused(window);
    let term = view.read(cx).term.clone();
    let term = term.lock();
    let content = term.renderable_content();
    let palette = Palette { set: content.colors, foreground, background, dark };
    let corner = |line: usize, column: usize| point(bounds.origin.x + cell.width * column as f32, bounds.origin.y + cell.height * line as f32);
    let shown_lines = term.screen_lines();
    let mut rows: Vec<Row> = (0..shown_lines).map(|_| Row::default()).collect();
    let mut backgrounds: Vec<(usize, usize, usize, Hsla)> = Vec::new();
    let scrolled = content.display_offset as i32;
    for indexed in content.display_iter {
        let Some(line) = usize::try_from(indexed.point.line.0 + scrolled).ok().filter(|line| *line < shown_lines) else {
            continue;
        };
        let column = indexed.point.column.0;
        let cell = indexed.cell;
        let (look, own) = cell_look(cell, &palette);
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        let fill = match content.selection {
            Some(range) if range.contains(indexed.point) => Some(selected),
            _ => own,
        };
        if let Some(fill) = fill {
            let span = if wide { 2 } else { 1 };
            match backgrounds.last_mut() {
                Some((at, _, end, color)) if *at == line && *end == column && *color == fill => *end += span,
                _ => backgrounds.push((line, column, column + span, fill)),
            }
        }
        // A blank keeps each column a glyph: a wide character's glyph reaches
        // into its second cell; combining marks stay on their character.
        let blank = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN;
        let text: String = match cell.flags.intersects(blank) {
            true => " ".into(),
            false => std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied()).collect(),
        };
        let visible = text != " " || look.underline.is_some() || look.strike;
        rows[line].push(&text, look, visible);
    }
    let backgrounds = backgrounds
        .into_iter()
        .map(|(line, start, end, color)| (Bounds::new(corner(line, start), gpui_kit::size(cell.width * (end - start) as f32, cell.height)), color))
        .collect();
    let rows = rows
        .into_iter()
        .enumerate()
        .filter(|(_, row)| row.shown > 0)
        .map(|(line, mut row)| {
            row.text.truncate(row.shown);
            row.runs.truncate(row.runs_shown);
            // The last run ends where the shown text does.
            let before: usize = row.runs.iter().rev().skip(1).map(|(_, len)| len).sum();
            if let Some((_, len)) = row.runs.last_mut() {
                *len = row.shown - before;
            }
            let runs: Vec<TextRun> = row.runs.iter().map(|(look, len)| run(look, *len, &font)).collect();
            (corner(line, 0), texts.shape_line(row.text.into(), size, &runs, Some(cell.width)))
        })
        .collect();
    let cursor = content.cursor;
    let caret = usize::try_from(cursor.point.line.0 + scrolled)
        .ok()
        .filter(|line| *line < shown_lines && cursor.shape != CursorShape::Hidden)
        .map(|line| {
            let under = &term.grid()[cursor.point];
            let width = if under.flags.contains(Flags::WIDE_CHAR) { 2. } else { 1. };
            let bounds = Bounds::new(corner(line, cursor.point.column.0), gpui_kit::size(cell.width * width, cell.height));
            let under = (cursor.shape == CursorShape::Block && focused && under.c != ' ').then(|| {
                let look = Look { color: background, bold: false, italic: false, underline: None, strike: false };
                texts.shape_line(under.c.to_string().into(), size, &[run(&look, under.c.len_utf8(), &font)], Some(cell.width))
            });
            Caret { bounds, shape: cursor.shape, under }
        });
    Screen { line_height: cell.height, backgrounds, rows, caret, caret_color: foreground, focused }
}

pub(super) fn paint(screen: Screen, view: Entity<TerminalView>, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    for (bounds, color) in screen.backgrounds {
        window.paint_quad(fill(bounds, color));
    }
    for (origin, row) in screen.rows {
        let _ = row.paint(origin, screen.line_height, TextAlign::Left, None, window, cx);
    }
    let caret = screen.caret.as_ref().map_or(Bounds::default(), |caret| caret.bounds);
    if let Some(Caret { bounds, shape, under }) = screen.caret {
        let color = screen.caret_color;
        let bar = |origin: Point<Pixels>, size: Size<Pixels>| fill(Bounds::new(origin, size), color);
        match shape {
            // Not focused, any caret is a frame.
            _ if !screen.focused => window.paint_quad(outline(bounds, color, BorderStyle::Solid)),
            CursorShape::HollowBlock => window.paint_quad(outline(bounds, color, BorderStyle::Solid)),
            CursorShape::Beam => window.paint_quad(bar(bounds.origin, gpui_kit::size(px(2.), bounds.size.height))),
            CursorShape::Underline => window.paint_quad(bar(point(bounds.origin.x, bounds.bottom() - px(2.)), gpui_kit::size(bounds.size.width, px(2.)))),
            _ => {
                window.paint_quad(fill(bounds, color));
                if let Some(under) = under {
                    let _ = under.paint(bounds.origin, screen.line_height, TextAlign::Left, None, window, cx);
                }
            }
        }
    }
    window.handle_input(focus, Typing { view, caret }, cx);
}

/// Typed text, an input method's too, goes to the program.
struct Typing {
    view: Entity<TerminalView>,
    /// Where an input method puts its window.
    caret: Bounds<Pixels>,
}

impl InputHandler for Typing {
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut App) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<std::ops::Range<usize>> {
        None
    }

    fn text_for_range(&mut self, _: std::ops::Range<usize>, _: &mut Option<std::ops::Range<usize>>, _: &mut Window, _: &mut App) -> Option<String> {
        None
    }

    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: &mut Window, cx: &mut App) {
        self.view.update(cx, |view, _| view.type_bytes(text.as_bytes().to_vec()));
    }

    // shortcut: an input method's text being composed is not shown, only what it gives.
    fn replace_and_mark_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, _: &str, _: Option<std::ops::Range<usize>>, _: &mut Window, _: &mut App) {}

    fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}

    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, _: &mut Window, _: &mut App) -> Option<Bounds<Pixels>> {
        Some(self.caret)
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut App) -> Option<usize> {
        None
    }

    // A held key repeats, as in a terminal.
    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}
