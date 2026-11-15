//! Quick Open: one box to find a bookmark, a recent folder, a drive, a
//! connection, a workspace, a tool or any command in, and go there or run it.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::command::{Command as Palette, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{Icon, WindowExt};
use gpui_kit::*;
use sc_core::devices::Volume;
use sc_core::menu::{self, Command};
use sc_core::remote::Address;

use crate::Commander;
use crate::menus::{GoToHistory, OpenWorkspace, Run, RunTool, place_label};
use crate::sidebar::tilde;

/// What a row stands for.
enum Pick {
    Folder(PathBuf),
    Volume(Volume),
    Server(Address),
    /// Sent where the panels' keys go.
    Action(Box<dyn Action>),
}

impl Clone for Pick {
    fn clone(&self) -> Self {
        match self {
            Pick::Folder(path) => Pick::Folder(path.clone()),
            Pick::Volume(volume) => Pick::Volume(volume.clone()),
            Pick::Server(address) => Pick::Server(address.clone()),
            Pick::Action(action) => Pick::Action(action.boxed_clone()),
        }
    }
}

struct QuickOpen {
    state: Entity<CommandState>,
    groups: Vec<CommandGroup>,
    /// By group and row, as the palette numbers them.
    picks: Rc<Vec<Vec<Pick>>>,
    commander: WeakEntity<Commander>,
    focused: bool,
}

impl Render for QuickOpen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Its field takes the focus once the palette has drawn.
        if !std::mem::replace(&mut self.focused, true) {
            let state = self.state.clone();
            window.defer(cx, move |window, cx| state.update(cx, |state, cx| state.focus(window, cx)));
        }
        let (picks, commander) = (self.picks.clone(), self.commander.clone());
        let panels = commander.upgrade().map(|commander| commander.read(cx).focus.clone());
        let palette = Palette::new(&self.state)
            .bordered(false)
            .max_h(px(440.))
            .placeholder("A command, bookmark, folder, drive, connection, workspace or tool")
            .on_confirm(move |at, window, cx| {
                let Some(pick) = picks.get(at.section).and_then(|rows| rows.get(at.row)).cloned() else {
                    return;
                };
                window.close_dialog(cx);
                let _ = commander.update(cx, |this, cx| this.quick_pick(pick, window, cx));
            });
        let palette = match panels {
            Some(panels) => palette.action_context(panels),
            None => palette,
        };
        self.groups.iter().cloned().fold(palette, Palette::group)
    }
}

impl Commander {
    /// Go > Quick Open.
    pub(crate) fn quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (mut groups, mut picks) = (Vec::new(), Vec::new());
        let mut add = |label: &'static str, rows: Vec<(CommandItem, Pick)>| {
            if !rows.is_empty() {
                let (items, rows): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
                groups.push(CommandGroup::new().label(label).items(items));
                picks.push(rows);
            }
        };
        let row = |label: String, icon: Lucide| CommandItem::new().label(label).icon(Icon::new(icon));
        let home = std::env::home_dir();
        let home = home.as_deref();

        let bookmarks = self.settings.bookmarks.iter().map(|path| {
            let label = format!("{}   {}", self.settings.bookmark_name(path), tilde(path, home));
            (row(label, Lucide::Bookmark), Pick::Folder(path.clone()))
        });
        add("Bookmarks", bookmarks.collect());

        // The active tab's history, the latest first, each place once.
        let history = &self.panels[self.active].tabs.current().history;
        let mut seen = vec![history.places.get(history.at)];
        let mut recent = Vec::new();
        for (ix, place) in history.places.iter().enumerate().rev() {
            if !seen.contains(&Some(place)) {
                seen.push(Some(place));
                recent.push((row(place_label(place), Lucide::FolderClock), Pick::Action(Box::new(GoToHistory(ix)))));
            }
        }
        add("Recent Folders", recent);

        let drives = self.sidebar.volumes.iter().map(|volume| {
            let label = match &volume.path {
                Some(path) => format!("{}   {}", volume.name, tilde(path, home)),
                None => volume.name.clone(),
            };
            (row(label, Lucide::HardDrive), Pick::Volume(volume.clone()))
        });
        add("Drives", drives.collect());

        let servers = sc_core::sidebar::servers(&self.sidebar.servers, &self.sidebar.hosts);
        let servers = servers.into_iter().flat_map(|(_, servers)| servers).map(|server| {
            let label = format!("{}   {}", server.name, server.detail);
            (row(label, Lucide::Server), Pick::Server(server.address))
        });
        add("Connections", servers.collect());

        let workspaces = self.workspaces.list.iter().enumerate().map(|(ix, workspace)| {
            let item = row(workspace.name.clone(), Lucide::Layers).checked(ix == self.workspaces.current);
            (item.keys_of(Box::new(OpenWorkspace(ix))), Pick::Action(Box::new(OpenWorkspace(ix))))
        });
        add("Workspaces", workspaces.collect());

        let tools = self.settings.tools.iter().enumerate().map(|(ix, tool)| {
            let item = row(tool.name.clone(), Lucide::Wrench).keys_of(Box::new(RunTool(ix)));
            (item, Pick::Action(Box::new(RunTool(ix))))
        });
        add("Tools", tools.collect());

        // Every command, under its menu's name; workspaces are listed by theirs above.
        for (menu_name, commands) in menu::groups() {
            let listed = commands.iter().filter(|(_, command)| *command != Command::QuickOpen && command.workspace().is_none());
            let rows = listed.map(|(label, command)| {
                let item = CommandItem::new().label(*label).keys_of(Box::new(Run(*command)));
                (item, Pick::Action(Box::new(Run(*command))))
            });
            add(menu_name, rows.collect());
        }

        let state = cx.new(|cx| CommandState::new(window, cx));
        let commander = cx.entity().downgrade();
        let view = cx.new(|_| QuickOpen { state, groups, picks: Rc::new(picks), commander, focused: false });
        window.open_dialog(cx, move |dialog, _, _| dialog.title("Quick Open").width(px(640.)).child(view.clone()));
    }

    fn quick_pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        match pick {
            Pick::Folder(path) => self.open_folder(path, window, cx),
            Pick::Volume(volume) => self.open_volume(volume, window, cx),
            Pick::Server(address) => self.connect(self.active, address, window, cx),
            Pick::Action(action) => window.dispatch_action(action, cx),
        }
    }
}
