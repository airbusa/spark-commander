//! Colours and lines from a Midnight Commander skin (.ini): our two, built in,
//! Pastel Night the default, or any of mc's by name ("default", mc's own look).

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

/// Ours, by the names `-S` and the settings take.
const BUILT_IN: [(&str, &str); 2] = [
    ("pastel-night", include_str!("../skins/pastel-night.ini")),
    ("pastel-gray", include_str!("../skins/pastel-gray.ini")),
];

pub struct Skin {
    pub panel: Style,
    /// The cursor's row, the active panel's title and tab.
    pub selected: Style,
    pub marked: Style,
    pub mark_selected: Style,
    /// The column titles.
    pub header: Style,
    /// The other side's open tab.
    pub reverse: Style,
    /// A dialog's line to type in.
    pub input: Style,
    pub dialog: Style,
    pub focus: Style,
    pub title: Style,
    pub error: Style,
    pub menu: Style,
    /// The open menu's name and the item under the cursor; the letters to press; what scmd cannot do yet.
    pub menu_selected: Style,
    pub menu_hot: Style,
    pub menu_hot_selected: Style,
    pub menu_inactive: Style,
    pub key_number: Style,
    pub key_label: Style,
    /// The viewer's top line, its text, and what is found in it.
    pub status: Style,
    pub viewer: Style,
    pub found: Style,
    /// Files' colours over the panel's.
    pub directory: Style,
    pub executable: Style,
    pub archive: Style,
    pub media: Style,
    pub picture: Style,
    /// Panels' and dialogs' frames.
    pub frame: border::Set<'static>,
    /// The line over a panel's status: itself, its ends on the frame, where a column line meets it.
    pub split: [&'static str; 4],
    /// Between columns.
    pub vert: &'static str,
}

impl Default for Skin {
    /// mc's own look, in the terminal's palette.
    fn default() -> Self {
        // Fixed 256-colour shades: themes repaint only the first 16, and Breeze's
        // magenta and yellow are unreadable on its blue.
        let (yellow, pink, green) = (Color::Indexed(228), Color::Indexed(219), Color::Indexed(120));
        let panel = Style::new().fg(Color::Gray).bg(Color::Blue);
        let bar = Style::new().fg(Color::Black).bg(Color::Cyan);
        let dialog = Style::new().fg(Color::Black).bg(Color::Gray);
        Skin {
            panel,
            selected: bar,
            marked: panel.fg(yellow).add_modifier(Modifier::BOLD),
            mark_selected: bar.fg(yellow).add_modifier(Modifier::BOLD),
            header: panel.fg(yellow).add_modifier(Modifier::BOLD),
            reverse: panel.add_modifier(Modifier::REVERSED),
            input: bar,
            dialog,
            focus: bar,
            title: dialog,
            error: Style::new().fg(Color::White).bg(Color::Red),
            menu: bar,
            // Not mc's yellow and white on cyan, hard to read: underlined, and the cursor's dark.
            menu_selected: Style::new().fg(Color::White).bg(Color::Black),
            menu_hot: bar.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            menu_hot_selected: Style::new().fg(yellow).bg(Color::Black).add_modifier(Modifier::BOLD),
            menu_inactive: bar.fg(Color::DarkGray),
            key_number: Style::new().fg(Color::Gray).bg(Color::Black),
            key_label: bar,
            status: bar,
            viewer: panel,
            found: bar,
            directory: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            executable: Style::new().fg(green),
            archive: Style::new().fg(pink),
            media: Style::new(),
            picture: Style::new(),
            frame: border::PLAIN,
            split: ["─", "├", "┤", "┴"],
            vert: "│",
        }
    }
}

static SKIN: OnceLock<Skin> = OnceLock::new();

pub fn skin() -> &'static Skin {
    SKIN.get_or_init(|| Skin::parse(BUILT_IN[0].1, truecolor()))
}

/// Before the first frame.
pub fn set(skin: Skin) {
    let _ = SKIN.set(skin);
}

/// Ours by name ("Pastel Night" too), else mc's `<name>.ini` in its skins
/// folders, or a file's path; "default", mc's look.
pub fn load(name: &str) -> io::Result<Skin> {
    let name = name.trim();
    let name = name.strip_suffix(".ini").unwrap_or(name);
    let ours = name.to_lowercase().replace(' ', "-");
    if ours == "default" {
        return Ok(Skin::default());
    }
    if let Some((_, text)) = BUILT_IN.iter().find(|(built_in, _)| *built_in == ours) {
        return Ok(Skin::parse(text, truecolor()));
    }
    let file = format!("{name}.ini");
    let path = folders().into_iter().map(|folder| folder.join(&file)).chain([PathBuf::from(&file)]).find(|path| path.is_file());
    let text = std::fs::read_to_string(path.ok_or(io::ErrorKind::NotFound)?)?;
    Ok(Skin::parse(&text, truecolor()))
}

/// Where mc looks for skins: the user's, then the system's.
fn folders() -> Vec<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::home_dir().map(|home| home.join(".local/share")));
    let system = ["/etc/mc/skins", "/usr/share/mc/skins", "/usr/local/share/mc/skins", "/opt/homebrew/share/mc/skins"];
    data.map(|data| data.join("mc/skins")).into_iter().chain(system.map(PathBuf::from)).collect()
}

/// As mc asks the terminal; else `#rrggbb` takes the nearest of 256.
fn truecolor() -> bool {
    cfg!(windows) || std::env::var("COLORTERM").is_ok_and(|term| term == "truecolor" || term == "24bit")
}

impl Skin {
    fn parse(text: &str, truecolor: bool) -> Skin {
        let ini = Ini::new(text, truecolor);
        let style = |section, key| ini.style(section, key);
        // A file's colour goes over the panel's: its own background only where it has one.
        let file = |key| ini.own("filehighlight", key);
        let line = |key, or| ini.line(key).unwrap_or(or);
        let (vert, horiz) = (line("dvert", "│"), line("dhoriz", "─"));
        Skin {
            panel: style("core", "_default_"),
            selected: style("core", "selected"),
            marked: style("core", "marked"),
            mark_selected: style("core", "markselect"),
            header: style("core", "header"),
            reverse: style("core", "reverse"),
            input: style("core", "input"),
            dialog: style("dialog", "_default_"),
            focus: style("dialog", "dfocus"),
            title: style("dialog", "dtitle"),
            error: style("error", "_default_"),
            menu: style("menu", "_default_"),
            menu_selected: style("menu", "menusel"),
            menu_hot: style("menu", "menuhot"),
            menu_hot_selected: style("menu", "menuhotsel"),
            menu_inactive: style("menu", "menuinactive"),
            key_number: style("buttonbar", "hotkey"),
            key_label: style("buttonbar", "button"),
            status: style("statusbar", "_default_"),
            viewer: style("viewer", "_default_"),
            found: style("viewer", "viewselected"),
            directory: file("directory"),
            executable: file("executable"),
            archive: file("archive"),
            media: file("media"),
            picture: file("graph"),
            // mc's panels and dialogs have the double set; lines in them the single.
            frame: border::Set {
                top_left: line("dlefttop", "┌"),
                top_right: line("drighttop", "┐"),
                bottom_left: line("dleftbottom", "└"),
                bottom_right: line("drightbottom", "┘"),
                vertical_left: vert,
                vertical_right: vert,
                horizontal_top: horiz,
                horizontal_bottom: horiz,
            },
            split: [line("horiz", "─"), line("dleftmiddle", "├"), line("drightmiddle", "┤"), line("bottommiddle", "┴")],
            vert: line("vert", "│"),
        }
    }
}

/// A skin file's values by section (in lower case) and key.
struct Ini {
    values: HashMap<(String, String), String>,
    truecolor: bool,
}

impl Ini {
    fn new(text: &str, truecolor: bool) -> Ini {
        let mut values = HashMap::new();
        let mut section = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
                section = name.trim().to_lowercase();
            } else if let Some((key, value)) = line.split_once('=').filter(|_| !line.starts_with(['#', ';'])) {
                values.insert((section.clone(), key.trim().to_owned()), value.trim().to_owned());
            }
        }
        Ini { values, truecolor }
    }

    fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.values.get(&(section.to_owned(), key.to_owned())).map(String::as_str)
    }

    /// `fg;bg;attributes`, as mc takes it: a colour not given is the
    /// section's `_default_` one, then `[core]`'s.
    fn style(&self, section: &str, key: &str) -> Style {
        let mut style = self.own(section, key);
        for (section, key) in [(section, "_default_"), ("core", "_default_")] {
            let mut parts = self.get(section, key).unwrap_or_default().split(';');
            let (fg, bg) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
            style.fg = style.fg.or_else(|| self.color(fg, 0));
            style.bg = style.bg.or_else(|| self.color(bg, 0));
        }
        style
    }

    /// What the key itself gives.
    fn own(&self, section: &str, key: &str) -> Style {
        let mut parts = self.get(section, key).unwrap_or_default().split(';');
        let mut style = Style::new();
        style.fg = self.color(parts.next().unwrap_or_default(), 0);
        style.bg = self.color(parts.next().unwrap_or_default(), 0);
        for attribute in parts.next().unwrap_or_default().split('+') {
            style = style.add_modifier(match attribute.trim() {
                "bold" => Modifier::BOLD,
                "underline" => Modifier::UNDERLINED,
                "italic" => Modifier::ITALIC,
                "reverse" => Modifier::REVERSED,
                "blink" => Modifier::SLOW_BLINK,
                _ => Modifier::empty(),
            });
        }
        style
    }

    fn line(&self, key: &str) -> Option<&'static str> {
        let line = self.get("lines", key).filter(|line| !line.is_empty())?;
        // Once a run: the frame's pieces live as long as the skin.
        Some(Box::leak(line.to_owned().into_boxed_str()))
    }

    /// mc's names, its 256 (`color0`, `rgb000`, `gray0`), `#rgb` and `#rrggbb`; an alias's.
    fn color(&self, name: &str, depth: u8) -> Option<Color> {
        let name = name.trim();
        if let Some(alias) = self.get("aliases", name).filter(|_| depth < 8) {
            return self.color(alias, depth + 1);
        }
        let number = |prefix: &str| name.strip_prefix(prefix).and_then(|n| n.parse::<u8>().ok());
        let rgb = |r: u8, g: u8, b: u8| match self.truecolor {
            true => Color::Rgb(r, g, b),
            false => nearest(r, g, b),
        };
        Some(match name {
            "" => return None,
            "default" => Color::Reset,
            "black" => Color::Black,
            "gray" => Color::DarkGray,
            "red" => Color::Red,
            "brightred" => Color::LightRed,
            "green" => Color::Green,
            "brightgreen" => Color::LightGreen,
            "brown" => Color::Yellow,
            "yellow" => Color::LightYellow,
            "blue" => Color::Blue,
            "brightblue" => Color::LightBlue,
            "magenta" => Color::Magenta,
            "brightmagenta" => Color::LightMagenta,
            "cyan" => Color::Cyan,
            "brightcyan" => Color::LightCyan,
            "lightgray" => Color::Gray,
            "white" => Color::White,
            _ if name.starts_with('#') => {
                let hex = &name[1..];
                let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
                match digits[..] {
                    [r, g, b] => rgb(r * 17, g * 17, b * 17),
                    [r1, r2, g1, g2, b1, b2] => rgb(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2),
                    _ => return None,
                }
            }
            _ if number("color").is_some() => Color::Indexed(number("color")?),
            _ if number("gray").is_some_and(|n| n < 24) => Color::Indexed(232 + number("gray")?),
            _ if name.starts_with("rgb") => {
                let digits: Vec<u8> = name[3..].bytes().map(|b| b.wrapping_sub(b'0')).collect();
                match digits[..] {
                    [r, g, b] if r < 6 && g < 6 && b < 6 => Color::Indexed(16 + 36 * r + 6 * g + b),
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

/// The nearest of the 256 colours' cube and grays.
fn nearest(r: u8, g: u8, b: u8) -> Color {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| (0..6).min_by_key(|&ix| LEVELS[ix].abs_diff(v)).unwrap_or(0);
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let gray = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3).saturating_sub(3).div_euclid(10).min(23) as u8;
    let distance = |(x, y, z): (u8, u8, u8)| [(x, r), (y, g), (z, b)].iter().map(|&(a, b)| u32::from(a.abs_diff(b)).pow(2)).sum::<u32>();
    let level_of = 8 + 10 * gray;
    match distance((level_of, level_of, level_of)) < distance((LEVELS[ri], LEVELS[gi], LEVELS[bi])) {
        true => Color::Indexed(232 + gray),
        false => Color::Indexed(16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mc_skins() {
        let night = Skin::parse(BUILT_IN[0].1, true);
        assert_eq!(night.panel, Style::new().fg(Color::Rgb(0xe6, 0xe1, 0xff)).bg(Color::Rgb(0x26, 0x23, 0x3a)));
        // Its background from [core]'s _default_, through the aliases.
        assert_eq!(night.marked.bg, Some(Color::Rgb(0x26, 0x23, 0x3a)));
        assert!(night.header.add_modifier.contains(Modifier::BOLD));
        assert_eq!(night.directory, Style::new().fg(Color::Rgb(0x7e, 0xc8, 0xff)));
        assert_eq!(night.frame.top_left, "╭");
        let gray = Skin::parse(BUILT_IN[1].1, false);
        assert!(matches!(gray.panel.bg, Some(Color::Indexed(_))));

        let mc = Skin::parse("[core]\n_default_ = lightgray;blue\nselected = ;color44\n[dialog]\ndfocus = rgb500;gray5;underline+bold\n", true);
        assert_eq!(mc.selected, Style::new().fg(Color::Gray).bg(Color::Indexed(44)));
        assert_eq!(mc.focus, Style::new().fg(Color::Indexed(196)).bg(Color::Indexed(237)).add_modifier(Modifier::UNDERLINED | Modifier::BOLD));
        assert_eq!(mc.frame.top_left, "┌");
    }

    #[test]
    fn takes_the_nearest_of_256() {
        assert_eq!(nearest(255, 255, 255), Color::Indexed(231));
        assert_eq!(nearest(128, 128, 128), Color::Indexed(244));
        assert_eq!(nearest(255, 0, 0), Color::Indexed(196));
    }
}
