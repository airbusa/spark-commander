use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::settings;

/// Both sides' tabs and the window's layout, kept between runs.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The tabs; the open workspace's.
    #[serde(flatten)]
    pub open: OpenTabs,
    /// Widths of the Name, Ext, Size, Date and Attr columns; Name 0 fills the rest.
    pub columns: Vec<f32>,
    /// The left panel's share of the width.
    pub split: f32,
    /// The sidebar's tab; none while it is hidden.
    pub sidebar: Option<crate::sidebar::SidebarTab>,
    pub sidebar_width: f32,
    /// The panels one above the other.
    pub vertical: bool,
    /// The terminal's share of the window's height; 0 before it was dragged.
    pub terminal_share: f32,
    /// The main window's place and size; none before the first run.
    pub window: Option<WindowPlace>,
    /// Synchronized browsing was on.
    pub sync_browsing: bool,
    /// The command line's last lines, the latest first.
    pub commands: Vec<String>,
}

/// Where the main window was, in logical pixels; maximized, its size before.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPlace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

/// Both panels' tabs and the terminal's: what a workspace keeps.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenTabs {
    pub sides: Vec<SideState>,
    /// The side the keys go to.
    pub active: usize,
    /// The terminal was shown: it opens again, as Krusader's.
    pub terminal: bool,
    /// The terminal's tabs, kept also while it is hidden.
    pub terminals: Vec<TerminalState>,
    pub terminal_tab: usize,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SideState {
    pub tabs: Vec<TabState>,
    pub active: usize,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    /// The folder, or the archive.
    pub path: PathBuf,
    /// The folder inside the archive.
    pub archive_dir: Option<PathBuf>,
    pub locked: bool,
    pub name: Option<String>,
    /// A server's tab: `path` is on it. Restored as a local tab, not reconnected.
    pub url: Option<String>,
    pub view: crate::View,
    pub history: crate::History,
    pub sort: crate::Sort,
}

/// A terminal tab: a shell in its folder, or ssh.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalState {
    /// None for ssh.
    pub dir: Option<PathBuf>,
    /// What runs instead of the shell, with its arguments.
    pub program: Option<(String, Vec<String>)>,
}

/// The window's tabs; the terminal's are its own.
pub const WINDOW: &str = "session.json";
pub const TERMINAL: &str = "session-terminal.json";
pub const WORKSPACES: &str = "workspaces.json";
/// scmd's own, as its tabs are.
pub const WORKSPACES_TERMINAL: &str = "workspaces-terminal.json";

impl Session {
    pub fn load() -> io::Result<Self> {
        Self::load_from(WINDOW)
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(WINDOW)
    }

    pub fn load_from(file: &str) -> io::Result<Self> {
        settings::read_json(file)
    }

    pub fn save_to(&self, file: &str) -> io::Result<()> {
        settings::write_json(file, self)
    }
}

/// Show > Workspaces: named sets of open tabs; changes go to the open one.
/// There is always one.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Workspaces {
    pub list: Vec<Workspace>,
    /// The open one.
    pub current: usize,
}

/// The first workspace's name.
const DEFAULT: &str = "Default";

impl Default for Workspaces {
    fn default() -> Self {
        let first = Workspace { name: DEFAULT.into(), open: OpenTabs::default() };
        Self { list: vec![first], current: 0 }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub name: String,
    #[serde(flatten)]
    pub open: OpenTabs,
}

impl Workspaces {
    pub fn load() -> io::Result<Self> {
        Self::load_from(WORKSPACES)
    }

    pub fn load_from(file: &str) -> io::Result<Self> {
        let mut workspaces: Self = settings::read_json(file)?;
        if workspaces.list.is_empty() {
            workspaces = Self::default();
        }
        // The first one was "Workspace 1" before.
        if workspaces.name_error(DEFAULT, None).is_none()
            && let Some(first) = workspaces.list.iter_mut().find(|workspace| workspace.name == "Workspace 1")
        {
            first.name = DEFAULT.into();
        }
        workspaces.current = workspaces.current.min(workspaces.list.len() - 1);
        Ok(workspaces)
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(WORKSPACES)
    }

    pub fn save_to(&self, file: &str) -> io::Result<()> {
        settings::write_json(file, self)
    }

    /// Where the next is when `ix` goes.
    pub fn neighbour(&self, ix: usize) -> usize {
        if ix + 1 < self.list.len() { ix + 1 } else { ix.saturating_sub(1) }
    }

    /// Why `name` cannot be workspace `ix`'s (none: a new one's).
    pub fn name_error(&self, name: &str, ix: Option<usize>) -> Option<String> {
        if name.trim().is_empty() {
            return Some(crate::t!("workspaces.error.no_name"));
        }
        let taken = self.list.iter().enumerate().any(|(at, other)| Some(at) != ix && other.name == name.trim());
        taken.then(|| crate::t!("workspaces.error.name_taken"))
    }

    /// `base`, or with the first number that makes it a free name.
    pub fn free_name(&self, base: &str) -> String {
        let names = std::iter::once(base.to_owned()).chain((2..).map(|n| format!("{base} {n}")));
        names.into_iter().find(|name| self.name_error(name, None).is_none()).unwrap_or_default()
    }

    /// A new workspace at `at`; the open one stays open.
    pub fn insert(&mut self, at: usize, name: &str, open: OpenTabs) {
        let at = at.min(self.list.len());
        self.list.insert(at, Workspace { name: name.trim().to_owned(), open });
        if self.current >= at {
            self.current += 1;
        }
    }

    /// Gone, unless it is the last or the open one: open another first.
    pub fn remove(&mut self, ix: usize) -> bool {
        if self.list.len() < 2 || ix >= self.list.len() || ix == self.current {
            return false;
        }
        self.list.remove(ix);
        if self.current > ix {
            self.current -= 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_always_a_workspace_and_names_are_once() {
        let mut workspaces = Workspaces::default();
        assert_eq!((workspaces.list.len(), workspaces.current, workspaces.list[0].name.as_str()), (1, 0, "Default"));
        assert!(!workspaces.remove(0));
        workspaces.insert(0, " Site ", OpenTabs::default());
        assert_eq!((workspaces.list[0].name.as_str(), workspaces.current), ("Site", 1));
        assert!(workspaces.name_error("site", None).is_none());
        assert!(workspaces.name_error("Site ", None).is_some());
        assert!(workspaces.name_error("Site", Some(0)).is_none());
        assert!(workspaces.name_error("  ", Some(0)).is_some());
        assert_eq!(workspaces.free_name("Site"), "Site 2");
        assert!(!workspaces.remove(1));
        assert!(workspaces.remove(0));
        assert_eq!((workspaces.list.len(), workspaces.current), (1, 0));
    }

    #[test]
    fn a_session_saved_before_workspaces_reads() {
        let old = r#"{"sides":[{"tabs":[{"path":"/x"}],"active":0}],"active":1,"split":0.4,"terminal":true}"#;
        let session: Session = serde_json::from_str(old).unwrap();
        assert_eq!(session.open.sides[0].tabs[0].path, PathBuf::from("/x"));
        assert_eq!((session.open.active, session.open.terminal, session.split), (1, true, 0.4));
        assert!(session.open.terminals.is_empty());
    }
}
