//! The menus, the button bar and the commands; their keys are the key map's.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Command {
    // Files
    /// The newest copy, move, rename or move to the trash undone (`undo`).
    Undo,
    /// The newest undone one done again.
    Redo,
    ChangeAttributes,
    Pack,
    UnpackSpecificFiles,
    TestArchives,
    CompareByContent,
    AssociateWith,
    InternalAssociations,
    Properties,
    CalculateOccupiedSpace,
    /// Every folder shown gets its size in the Size column.
    AllFolderSizes,
    /// Links to the chosen in the other panel's folder (`links`).
    CreateLink,
    MultiRenameTool,
    SplitFile,
    CombineFiles,
    CreateChecksumFiles,
    VerifyChecksums,
    Quit,
    // Mark
    SelectGroup,
    UnselectGroup,
    SelectAll,
    UnselectAll,
    InvertSelection,
    SelectAllWithSameExtension,
    SaveSelection,
    RestoreSelection,
    SaveSelectionToFile,
    LoadSelectionFromFile,
    CopyNamesToClipboard,
    CopyNamesWithPathToClipboard,
    CopyWithAllDetails,
    CopyWithPathAndDetails,
    CopyToClipboard,
    CutToClipboard,
    PasteFromClipboard,
    CompareDirectories,
    MarkNewerHideSame,
    // Commands
    /// Gone (Places' bookmarks and the Folders tree do it); kept so old settings files load.
    CdTree,
    Search,
    SearchInSeparateProcess,
    VolumeLabel,
    SystemInformation,
    SynchronizeDirs,
    /// Total Commander's Directory Hotlist: the bookmarks in a list over the panel.
    #[serde(alias = "DirectoryHotlist")]
    Bookmarks,
    /// The folder shown, into Bookmarks.
    BookmarkFolder,
    /// Bookmarks to add, remove, rename and put in order.
    ManageBookmarks,
    /// A workspace of the home folder's tabs, opened.
    NewWorkspace,
    /// A copy of the open workspace, opened.
    DuplicateWorkspace,
    /// Workspaces to make, copy, rename and delete.
    ManageWorkspaces,
    /// The Tools menu's command lines to add, change, delete and give keys.
    ManageTools,
    /// One box to find commands, bookmarks, drives, connections and tools in.
    QuickOpen,
    /// Total Commander's Alt+F1 and Alt+F2: the drives in a list over that panel.
    LeftDrives,
    RightDrives,
    /// Ctrl+Alt+1 to 9: that workspace opened.
    Workspace1,
    Workspace2,
    Workspace3,
    Workspace4,
    Workspace5,
    Workspace6,
    Workspace7,
    Workspace8,
    Workspace9,
    GoBack,
    GoForward,
    /// Alt+Down: the tab's last places, to pick one.
    FolderHistory,
    /// This computer's home folder; the server's on a server.
    GoHome,
    OpenTerminal,
    BranchView,
    /// Branch View of the marked folders only.
    BranchViewSelected,
    /// Gone (Desktop is a first bookmark); kept so old settings files load.
    OpenDesktopFolder,
    BackgroundTransferManager,
    /// What was done to files, and how each ended (`activity`).
    ActivityLog,
    SwapPanels,
    TargetEqualsSource,
    /// The other panel makes the active one's moves too, as Krusader's.
    SyncBrowsing,
    // Show
    Brief,
    Full,
    /// Shows or hides the sidebar's tab.
    #[serde(alias = "Tree")]
    SidebarTree,
    #[serde(alias = "SidebarFavourites")]
    SidebarConnections,
    SidebarPlaces,
    SidebarOperations,
    ToggleSidebar,
    ThumbnailView,
    /// Dot files, and Windows' hidden and system files, shown or not.
    HiddenFiles,
    QuickViewPanel,
    /// The built-in terminal under the panels, shown or hidden; its shell lives on.
    TerminalPanel,
    /// Another tab of the built-in terminal, in the active panel's folder; shown if hidden.
    NewTerminal,
    VerticalArrangement,
    DuplicateTab,
    AllFiles,
    Programs,
    AllWithExtension,
    CustomFilter,
    /// A link's file (HTTP(S), FTP(S)) into the active panel's folder.
    DownloadLink,
    /// Ctrl+I: what is typed hides the names without it, until Escape or another folder.
    QuickFilter,
    OnlySelectedFiles,
    SortByName,
    SortByExtension,
    SortByTime,
    SortBySize,
    Unsorted,
    ReversedOrder,
    RereadSource,
    // Configuration
    Settings,
    Shortcuts,
    // Help
    Website,
    About,
    // Button bar
    View,
    Edit,
    EditExternally,
    ToggleMark,
    /// Total Commander's Space: the cursor stays; a folder gets its size.
    MarkInPlace,
    OperationQueue,
    /// Gone with the Net menu; kept so old settings files load.
    ConnectToServer,
    Disconnect,
    Copy,
    Move,
    NewFolder,
    Delete,
    DeletePermanently,
    // Tab menu
    RenameTab,
    LockTab,
    CopyTabToOtherPanel,
    CloseTab,
    CloseAllTabs,
    CloseDuplicateTabs,
    ListTabs,
    NextTab,
    PreviousTab,
    // Keys only, as Total Commander's
    SwapPanelsAndTabs,
    /// Ctrl+Left and Ctrl+Right: the other panel's folder, or the one at its cursor, in that panel.
    OpenInLeftPanel,
    OpenInRightPanel,
    GoToRoot,
    OpenInNewTab,
    OpenInOtherNewTab,
    PathToCommandLine,
    NameToCommandLine,
    PathAndNameToCommandLine,
    RenameInPlace,
    NewTextFile,
}

pub enum Item {
    Command {
        label: &'static str,
        command: Command,
    },
    Submenu {
        label: &'static str,
        items: &'static [Item],
    },
    /// System, Light and Dark.
    Appearance {
        label: &'static str,
    },
    /// A submenu of the side's recently closed tabs.
    ClosedTabs {
        label: &'static str,
    },
    /// The user's tools, each run when clicked; a separator after them.
    Tools,
    /// Back or Forward: runs `command` when clicked, its submenu lists the tab's places that way.
    History {
        label: &'static str,
        command: Command,
    },
    /// A submenu of the workspaces, to open one, then `items`.
    Workspaces {
        label: &'static str,
        items: &'static [Item],
    },
    Separator,
}

pub struct Menu {
    pub label: &'static str,
    pub items: &'static [Item],
}

const fn cmd(label: &'static str, command: Command) -> Item {
    Item::Command { label, command }
}

const SEP: Item = Item::Separator;

use Command::*;

/// Total Commander's menus in its order, Go added; long lists in
/// submenus, every command that works on one.
pub const MENUS: &[Menu] = &[
    Menu {
        label: "Files",
        items: &[
            cmd("New File...", NewTextFile),
            cmd("New Folder...", NewFolder),
            SEP,
            cmd("View", View),
            cmd("Edit", Edit),
            cmd("Edit With External Editor", EditExternally),
            SEP,
            cmd("Copy...", Copy),
            cmd("Move...", Move),
            cmd("Download from Link...", DownloadLink),
            cmd("Rename", RenameInPlace),
            cmd("Create Link...", CreateLink),
            cmd("Delete", Delete),
            cmd("Delete Permanently", DeletePermanently),
            SEP,
            cmd("Properties...", Properties),
            cmd("Change Attributes...", ChangeAttributes),
            cmd("Calculate Occupied Space...", CalculateOccupiedSpace),
            cmd("Calculate All Folder Sizes", AllFolderSizes),
            cmd("Multi-Rename Tool...", MultiRenameTool),
            cmd("Compare By Content...", CompareByContent),
            SEP,
            Item::Submenu {
                label: "Archive",
                items: &[
                    cmd("Pack...", Pack),
                    cmd("Unpack Specific Files...", UnpackSpecificFiles),
                    cmd("Test Archive(s)", TestArchives),
                ],
            },
            Item::Submenu {
                label: "Checksums",
                items: &[
                    cmd("Create Checksum File(s) (CRC32, MD5, SHA1, SHA256, SHA512)...", CreateChecksumFiles),
                    cmd("Verify Checksums (from checksum files)", VerifyChecksums),
                ],
            },
            Item::Submenu {
                label: "Split and Combine",
                items: &[cmd("Split File...", SplitFile), cmd("Combine Files...", CombineFiles)],
            },
            Item::Submenu {
                label: "Associations",
                items: &[
                    cmd("Associate With...", AssociateWith),
                    cmd("Internal Associations (Spark Commander Only)...", InternalAssociations),
                ],
            },
            SEP,
            cmd("Quit", Quit),
        ],
    },
    Menu {
        label: "Mark",
        items: &[
            cmd("Mark / Unmark", ToggleMark),
            cmd("Select Group...", SelectGroup),
            cmd("Unselect Group...", UnselectGroup),
            cmd("Select All", SelectAll),
            cmd("Unselect All", UnselectAll),
            cmd("Invert Selection", InvertSelection),
            cmd("Select All With Same Extension", SelectAllWithSameExtension),
            Item::Submenu {
                label: "Selection",
                items: &[
                    cmd("Save Selection", SaveSelection),
                    cmd("Restore Selection", RestoreSelection),
                    cmd("Save Selection To File", SaveSelectionToFile),
                    cmd("Load Selection From File", LoadSelectionFromFile),
                ],
            },
            SEP,
            cmd("Copy Files", CopyToClipboard),
            cmd("Cut Files", CutToClipboard),
            cmd("Paste Files", PasteFromClipboard),
            Item::Submenu {
                label: "Copy Names",
                items: &[
                    cmd("Copy Selected Names To Clipboard", CopyNamesToClipboard),
                    cmd("Copy Names With Path To Clipboard", CopyNamesWithPathToClipboard),
                    cmd("Copy To Clipboard With All Details", CopyWithAllDetails),
                    cmd("Copy To Clipboard With Path+Details", CopyWithPathAndDetails),
                ],
            },
            SEP,
            Item::Submenu {
                label: "Compare Directories",
                items: &[
                    cmd("Mark Newer", CompareDirectories),
                    cmd("Mark Newer, Hide Same Files", MarkNewerHideSame),
                ],
            },
        ],
    },
    Menu {
        label: "Go",
        items: &[
            Item::History { label: "Back", command: GoBack },
            Item::History { label: "Forward", command: GoForward },
            cmd("Folder History...", FolderHistory),
            cmd("Home", GoHome),
            cmd("Root Folder", GoToRoot),
            cmd("Drives (Left Panel)", LeftDrives),
            cmd("Drives (Right Panel)", RightDrives),
            SEP,
            cmd("Bookmarks", Bookmarks),
            cmd("Bookmark This Folder", BookmarkFolder),
            cmd("Manage Bookmarks...", ManageBookmarks),
            SEP,
            cmd("Quick Open...", QuickOpen),
        ],
    },
    Menu {
        label: "Commands",
        items: &[
            cmd("Search...", Search),
            cmd("Search in Separate Window...", SearchInSeparateProcess),
            cmd("Synchronize Dirs...", SynchronizeDirs),
            SEP,
            cmd("Undo", Undo),
            cmd("Redo", Redo),
            cmd("Operation Queue...", OperationQueue),
            cmd("Activity Log...", ActivityLog),
            SEP,
            cmd("Open Terminal Here", OpenTerminal),
            cmd("Branch View (With Subdirs)", BranchView),
            cmd("Branch View of Selected Folders", BranchViewSelected),
            SEP,
            cmd("Swap Panels", SwapPanels),
            cmd("Swap Panels With Their Tabs", SwapPanelsAndTabs),
            cmd("Target = Source", TargetEqualsSource),
            cmd("Synchronized Browsing", SyncBrowsing),
        ],
    },
    Menu {
        label: "Show",
        items: &[
            cmd("Brief", Brief),
            cmd("Full", Full),
            cmd("Thumbnail View", ThumbnailView),
            SEP,
            cmd("Hidden Files", HiddenFiles),
            SEP,
            Item::Submenu {
                label: "Sidebar",
                items: &[
                    cmd("Show Sidebar", ToggleSidebar),
                    SEP,
                    cmd("Places", SidebarPlaces),
                    cmd("Folders", SidebarTree),
                    cmd("Connections", SidebarConnections),
                    cmd("Operations", SidebarOperations),
                ],
            },
            cmd("Quick View Panel", QuickViewPanel),
            cmd("Terminal", TerminalPanel),
            cmd("New Terminal Tab", NewTerminal),
            cmd("Vertical Arrangement", VerticalArrangement),
            SEP,
            Item::Submenu {
                label: "Filter",
                items: &[
                    cmd("Quick Filter", QuickFilter),
                    SEP,
                    cmd("All Files", AllFiles),
                    cmd("Programs", Programs),
                    cmd("*.*", AllWithExtension),
                    cmd("Custom...", CustomFilter),
                    cmd("Only Selected Files", OnlySelectedFiles),
                ],
            },
            Item::Submenu {
                label: "Sort",
                items: &[
                    cmd("Name", SortByName),
                    cmd("Extension", SortByExtension),
                    cmd("Time", SortByTime),
                    cmd("Size", SortBySize),
                    cmd("Unsorted", Unsorted),
                    SEP,
                    cmd("Reversed Order", ReversedOrder),
                ],
            },
            Item::Submenu {
                label: "Tabs",
                items: &[
                    cmd("New Tab", DuplicateTab),
                    cmd("Open Folder in New Tab", OpenInNewTab),
                    cmd("Close Tab", CloseTab),
                    cmd("Close All Tabs", CloseAllTabs),
                    cmd("Close Duplicate Tabs", CloseDuplicateTabs),
                    SEP,
                    cmd("Next Tab", NextTab),
                    cmd("Previous Tab", PreviousTab),
                    cmd("List of Tabs", ListTabs),
                ],
            },
            // Krusader's Profiles are in its Window menu: what is shown, not where.
            Item::Workspaces {
                label: "Workspaces",
                items: &[
                    SEP,
                    cmd("New Workspace...", NewWorkspace),
                    cmd("Duplicate Workspace...", DuplicateWorkspace),
                    cmd("Manage Workspaces...", ManageWorkspaces),
                ],
            },
            SEP,
            cmd("Reread Source", RereadSource),
        ],
    },
    Menu {
        label: "Tools",
        items: &[Item::Tools, cmd("Manage Tools...", ManageTools)],
    },
    Menu {
        label: "Configuration",
        items: &[
            cmd("Settings...", Settings),
            cmd("Shortcuts...", Shortcuts),
        ],
    },
    Menu {
        label: "Help",
        items: &[cmd("Spark Commander Website", Website), SEP, cmd("About Spark Commander", About)],
    },
];

/// The right-click menu of folder tabs.
pub const TAB_MENU: &[Item] = &[
    cmd("Duplicate this tab", DuplicateTab),
    cmd("Rename tab...", RenameTab),
    cmd("Lock this tab", LockTab),
    SEP,
    cmd("Copy tab to other panel", CopyTabToOtherPanel),
    SEP,
    cmd("Close this tab", CloseTab),
    cmd("Close all tabs", CloseAllTabs),
    cmd("Close duplicate tabs", CloseDuplicateTabs),
    Item::ClosedTabs {
        label: "Recently closed tabs",
    },
    cmd("Show list of all tabs", ListTabs),
];

/// The function key buttons under the panels.
pub const BUTTON_BAR: &[Item] = &[
    cmd("View", View),
    cmd("Edit", Edit),
    cmd("Copy", Copy),
    cmd("Move", Move),
    cmd("New Folder", NewFolder),
    cmd("Delete", Delete),
    cmd("Exit", Quit),
];

/// The toolbar's buttons until the user changes them: what the function
/// keys' bar leaves out and is used often.
pub const TOOLBAR: &[ToolbarItem] = {
    use ToolbarItem::{Button as B, Separator as S};
    &[B(GoBack), B(GoForward), S, B(RereadSource), B(Search), S, B(SynchronizeDirs), B(MultiRenameTool), B(Pack), S, B(TerminalPanel)]
};

/// A toolbar's button, or a separator between its groups: a line, on macOS
/// the end of a group drawn as one capsule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItem {
    Button(Command),
    Separator,
}

/// As saved: the command's name, or "Separator".
impl Serialize for ToolbarItem {
    fn serialize<S: serde::Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
        match self {
            ToolbarItem::Button(command) => command.serialize(to),
            ToolbarItem::Separator => to.serialize_str("Separator"),
        }
    }
}

/// A toolbar's items; the commands this version does not know left out.
pub fn known_toolbar<'de, D: serde::Deserializer<'de>>(list: D) -> Result<Vec<ToolbarItem>, D::Error> {
    let names = Vec::<String>::deserialize(list)?;
    let known = names.iter().filter_map(|name| match name.as_str() {
        "Separator" => Some(ToolbarItem::Separator),
        _ => Command::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name)).ok().map(ToolbarItem::Button),
    });
    Ok(known.collect())
}

/// A list of commands; the ones this version does not know left out.
pub fn known<'de, D: serde::Deserializer<'de>>(list: D) -> Result<Vec<Command>, D::Error> {
    let names = Vec::<String>::deserialize(list)?;
    let known = names.iter().filter_map(|name| {
        Command::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name)).ok()
    });
    Ok(known.collect())
}

/// Commands on no menu or button.
pub const OTHER: &[Item] = &[
    cmd("Disconnect from the server", Disconnect),
    cmd("Show in the left panel", OpenInLeftPanel),
    cmd("Show in the right panel", OpenInRightPanel),
    cmd("Open the folder in a new tab of the other panel", OpenInOtherNewTab),
    cmd("Path to the command line", PathToCommandLine),
    cmd("Name to the command line", NameToCommandLine),
    cmd("Path and name to the command line", PathAndNameToCommandLine),
    cmd("Mark / Unmark in place, a folder measured", MarkInPlace),
    cmd("Open Workspace 1", Workspace1),
    cmd("Open Workspace 2", Workspace2),
    cmd("Open Workspace 3", Workspace3),
    cmd("Open Workspace 4", Workspace4),
    cmd("Open Workspace 5", Workspace5),
    cmd("Open Workspace 6", Workspace6),
    cmd("Open Workspace 7", Workspace7),
    cmd("Open Workspace 8", Workspace8),
    cmd("Open Workspace 9", Workspace9),
];

impl Command {
    /// Workspace1 to 9: which workspace, from 0.
    pub fn workspace(self) -> Option<usize> {
        const ALL: [Command; 9] = [Workspace1, Workspace2, Workspace3, Workspace4, Workspace5, Workspace6, Workspace7, Workspace8, Workspace9];
        ALL.iter().position(|command| *command == self)
    }
}

/// Names for where the menu gives no context.
const FULL_NAMES: &[(Command, &str)] = &[
    (Brief, "Brief View"),
    (Full, "Full View"),
    (SidebarTree, "Sidebar: Folders"),
    (SidebarConnections, "Sidebar: Connections"),
    (SidebarPlaces, "Sidebar: Places"),
    (SidebarOperations, "Sidebar: Operations"),
    (ToggleSidebar, "Sidebar: show or hide"),
    (AllFiles, "Show All Files"),
    (Programs, "Show Programs"),
    (AllWithExtension, "Show *.*"),
    (CustomFilter, "Custom Filter"),
    (QuickFilter, "Quick Filter"),
    (HiddenFiles, "Show Hidden Files"),
    (OnlySelectedFiles, "Show Only Selected Files"),
    (SortByName, "Sort by Name"),
    (SortByExtension, "Sort by Extension"),
    (SortByTime, "Sort by Time"),
    (SortBySize, "Sort by Size"),
    (Unsorted, "Sort Unsorted"),
    (RenameInPlace, "Rename in place"),
    (CompareDirectories, "Compare Directories: Mark Newer"),
    (GoBack, "Go Back"),
    (GoForward, "Go Forward"),
    (GoHome, "Go Home"),
    (DuplicateTab, "New Tab (a copy of this one)"),
];

/// `CopyNamesToClipboard`, `Split and Combine`: `copy_names_to_clipboard`, `split_and_combine`.
fn key_words(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            c if c.is_ascii_uppercase() && !out.is_empty() && !out.ends_with('_') => {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            }
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ if !out.is_empty() && !out.ends_with('_') => out.push('_'),
            _ => {}
        }
    }
    out.trim_end_matches('_').to_owned()
}

/// The list a command's label is in, by its translation key's area: the
/// menus' (and the other commands'), the tab menu's, the button bar's.
const LISTS: [(&str, &[Item]); 3] = [("tab_menu", TAB_MENU), ("button_bar", BUTTON_BAR), ("command", OTHER)];

fn command_key(area: &str, command: Command) -> String {
    format!("{area}.{}", key_words(&format!("{command:?}")))
}

/// A command's label in the menus (or, not on them, its own), in the app's language.
pub fn label(command: Command) -> String {
    crate::l10n::tr(&command_key("command", command))
}

/// A command's label on the tab's right-click menu.
pub fn tab_label(command: Command) -> String {
    crate::l10n::tr(&command_key("tab_menu", command))
}

/// A command's label on the button bar.
pub fn button_label(command: Command) -> String {
    crate::l10n::tr(&command_key("button_bar", command))
}

/// A menu's, a submenu's or a group's name, given in English: `Files`.
pub fn menu_label(english: &str) -> String {
    crate::l10n::tr(&format!("menu.{}", key_words(english)))
}

/// A command's name where no menu tells what it is part of: Shortcuts,
/// Quick Open, the toolbar's tooltips.
pub fn name(command: Command) -> String {
    if FULL_NAMES.iter().any(|(named, _)| *named == command) {
        return crate::l10n::tr(&command_key("command_name", command));
    }
    let in_menus = |items: &[Item]| contains(items, command);
    let area = match () {
        _ if MENUS.iter().any(|menu| in_menus(menu.items)) => "command",
        _ => LISTS.iter().find(|(_, items)| in_menus(items)).map_or("command", |(area, _)| area),
    };
    let label = crate::l10n::tr(&command_key(area, command));
    label.trim_end_matches("...").trim_end_matches('…').to_owned()
}

fn contains(items: &[Item], wanted: Command) -> bool {
    items.iter().any(|item| match *item {
        Item::Command { command, .. } | Item::History { command, .. } => command == wanted,
        Item::Submenu { items, .. } | Item::Workspaces { items, .. } => contains(items, wanted),
        _ => false,
    })
}

/// Every command once, under its menu's name, for the Shortcuts dialog; in
/// the app's language.
pub fn groups() -> Vec<(String, Vec<(String, Command)>)> {
    fn walk(items: &[Item], seen: &mut Vec<Command>, out: &mut Vec<(String, Command)>) {
        for item in items {
            match *item {
                Item::Command { command, .. } | Item::History { command, .. } if !seen.contains(&command) => {
                    seen.push(command);
                    out.push((name(command), command));
                }
                Item::Submenu { items, .. } | Item::Workspaces { items, .. } => walk(items, seen, out),
                _ => {}
            }
        }
    }
    let mut seen = Vec::new();
    let named = MENUS.iter().map(|menu| (menu.label, menu.items));
    let others = [("Tabs", TAB_MENU), ("Button bar", BUTTON_BAR), ("Other", OTHER)];
    let mut groups = Vec::new();
    for (name, items) in named.chain(others) {
        let mut commands = Vec::new();
        walk(items, &mut seen, &mut commands);
        if !commands.is_empty() {
            groups.push((menu_label(name), commands));
        }
    }
    groups
}

/// The menus' words as `l10n/en.json` holds them: each key, its English and
/// a note for the translators.
pub fn english_entries() -> Vec<(String, String, String)> {
    let mut entries: Vec<(String, String, String)> = Vec::new();
    let mut add = |key: String, message: &str, note: String| {
        if !entries.iter().any(|(known, ..)| *known == key) {
            entries.push((key, message.to_owned(), note));
        }
    };
    fn walk(items: &[Item], path: &str, area: &str, add: &mut dyn FnMut(String, &str, String)) {
        for item in items {
            match *item {
                Item::Command { label, command } | Item::History { label, command } => {
                    let note = match area {
                        "tab_menu" => "Right-click menu of a folder tab.".to_owned(),
                        "button_bar" => "Function key button under the panels (as Total Commander's F3 View, F5 Copy): keep it short.".to_owned(),
                        _ if path.is_empty() => "A command on no menu: its name in the Shortcuts dialog and Quick Open.".to_owned(),
                        _ => format!("Menu item: {path}. \"...\" means a dialog opens."),
                    };
                    add(command_key(area, command), label, note);
                }
                Item::Submenu { label, items } | Item::Workspaces { label, items } => {
                    add(format!("menu.{}", key_words(label)), label, format!("Submenu of {path}."));
                    walk(items, &format!("{path} › {label}"), area, add);
                }
                Item::Appearance { label } | Item::ClosedTabs { label } => add(format!("menu.{}", key_words(label)), label, format!("Submenu of {path}.")),
                Item::Tools | Item::Separator => {}
            }
        }
    }
    for menu in MENUS {
        add(format!("menu.{}", key_words(menu.label)), menu.label, "A menu of the menu bar.".to_owned());
        walk(menu.items, menu.label, "command", &mut add);
    }
    for (area, items) in LISTS {
        walk(items, "", area, &mut add);
    }
    for group in ["Tabs", "Button bar", "Other"] {
        add(format!("menu.{}", key_words(group)), group, "A group of commands in the Shortcuts dialog.".to_owned());
    }
    for (command, name) in FULL_NAMES {
        add(command_key("command_name", *command), name, "A command's name where no menu tells what it belongs to (Shortcuts dialog, Quick Open, toolbar tooltips).".to_owned());
    }
    entries
}

/// Each label's letter to press, none twice nor one of `taken`: a word's first
/// letter first, else another of its letters; the fewest taken earlier move to
/// others of their own when that leaves a label none. A label in a script
/// without letters to type (Chinese, Japanese, Korean) gets one added, as
/// Windows does: `ファイル(A)`. Gives the label as shown and its letter's char index.
pub fn mnemonics(labels: &[&str], taken: &[char]) -> Vec<(String, Option<usize>)> {
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let ideographic = |c: char| matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF);
    let taken: Vec<char> = taken.iter().map(|&c| lower(c)).collect();
    // Each label's letters in the order it would rather have them, as char indices.
    let wishes: Vec<Vec<(char, usize)>> = labels
        .iter()
        .map(|label| {
            let chars: Vec<char> = label.chars().collect();
            let starts = (0..chars.len()).filter(|&i| i == 0 || !chars[i - 1].is_alphanumeric());
            let mut wishes: Vec<(char, usize)> = Vec::new();
            for i in starts.chain(0..chars.len()) {
                let c = lower(chars[i]);
                if chars[i].is_alphanumeric() && !ideographic(chars[i]) && !taken.contains(&c) && !wishes.iter().any(|(had, _)| *had == c) {
                    wishes.push((c, i));
                }
            }
            wishes
        })
        .collect();
    // First come, first served; then a label left without one takes a letter whose
    // holder moves to another, by the fewest moves (a shortest augmenting path).
    let mut has: Vec<Option<char>> = vec![None; labels.len()];
    for ix in 0..labels.len() {
        has[ix] = wishes[ix].iter().map(|(c, _)| *c).find(|c| !has.contains(&Some(*c)));
    }
    for start in 0..labels.len() {
        if has[start].is_some() {
            continue;
        }
        // For each label reached: who would take its letter, and which letter that is.
        let mut came: Vec<Option<(usize, char)>> = vec![None; labels.len()];
        let mut seen = vec![false; labels.len()];
        let mut queue = std::collections::VecDeque::from([start]);
        seen[start] = true;
        'search: while let Some(ix) = queue.pop_front() {
            for &(c, _) in &wishes[ix] {
                match has.iter().position(|held| *held == Some(c)) {
                    None => {
                        let (mut at, mut letter) = (ix, c);
                        loop {
                            let next = came[at];
                            has[at] = Some(letter);
                            let Some((by, c)) = next else { break };
                            (at, letter) = (by, c);
                        }
                        break 'search;
                    }
                    Some(holder) if !seen[holder] => {
                        seen[holder] = true;
                        came[holder] = Some((ix, c));
                        queue.push_back(holder);
                    }
                    Some(_) => {}
                }
            }
        }
    }
    let holder: Vec<(char, usize)> = has.iter().enumerate().filter_map(|(ix, c)| Some(((*c)?, ix))).collect();
    let mut used: Vec<char> = taken.iter().copied().chain(holder.iter().map(|(c, _)| *c)).collect();
    labels
        .iter()
        .enumerate()
        .map(|(ix, label)| {
            if let Some((c, _)) = holder.iter().find(|(_, by)| *by == ix) {
                let at = wishes[ix].iter().find(|(had, _)| had == c).map(|(_, at)| *at);
                return (label.to_string(), at);
            }
            let chars: Vec<char> = label.chars().collect();
            let added = ('a'..='z').chain('0'..='9').find(|c| !used.contains(c));
            match added {
                Some(c) if chars.iter().any(|&c| ideographic(c)) && !chars.iter().any(|c| c.is_ascii_alphanumeric()) => {
                    used.push(c);
                    // Before a dialog's dots, as Windows puts it.
                    let bare = label.trim_end_matches(['.', '…']);
                    let shown = format!("{bare}({}){}", c.to_ascii_uppercase(), &label[bare.len()..]);
                    (shown, Some(bare.chars().count() + 1))
                }
                _ => (label.to_string(), None),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeySettings, Preset};

    #[test]
    fn mnemonics_are_unique_and_typable() {
        let letters = |labels: &[&str], taken: &[char]| -> Vec<String> {
            mnemonics(labels, taken).into_iter().map(|(shown, at)| at.map_or("-".into(), |at| shown.chars().nth(at).unwrap().to_string())).collect()
        };
        let menus = ["Files", "Mark", "Go", "Commands", "Show", "Tools", "Configuration", "Help"];
        assert_eq!(letters(&menus, &['h']), ["F", "M", "G", "C", "S", "T", "o", "e"]);
        assert_eq!(letters(&["Copy Files", "Cut Files", "*.*"], &[]), ["C", "F", "-"]);
        let (shown, at) = mnemonics(&["検索...", "ファイル"], &['a']).remove(0);
        assert_eq!((shown.as_str(), shown.chars().nth(at.unwrap())), ("検索(B)...", Some('B')));
        assert_eq!(letters(&["Файлы", "Фильтр"], &[]), ["Ф", "и"]);
        // "Ab" gives up its A for "A", which has no other.
        assert_eq!(letters(&["Ab", "A"], &[]), ["b", "A"]);
    }

    #[test]
    fn unknown_commands_are_left_out() {
        #[derive(Deserialize)]
        struct Bar(#[serde(deserialize_with = "known")] Vec<Command>);
        let bar: Bar = serde_json::from_str(r#"["Search", "Gone", "Pack"]"#).unwrap();
        assert_eq!(bar.0, [Search, Pack]);
    }

    /// The menus' English in `l10n/en.json` is the menus' own.
    #[test]
    fn menu_words_are_in_english() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../l10n/en.json");
        let english: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        for (key, message, _) in english_entries() {
            assert_eq!(english[&key]["message"].as_str(), Some(message.as_str()), "{key}: run `cargo test -p sc-core writes_menu_words -- --ignored`");
        }
        assert_eq!(key_words("CopyNamesToClipboard"), "copy_names_to_clipboard");
        assert_eq!(key_words("Split and Combine"), "split_and_combine");
    }

    /// Writes the menus' entries into `l10n/en.json`, the others kept.
    #[test]
    #[ignore]
    fn writes_menu_words() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../l10n/en.json");
        let mut english: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        for (key, message, note) in english_entries() {
            english.insert(key, serde_json::json!({ "message": message, "description": note }));
        }
        let sorted: std::collections::BTreeMap<_, _> = english.into_iter().collect();
        std::fs::write(&file, serde_json::to_string_pretty(&sorted).unwrap() + "\n").unwrap();
    }

    #[test]
    fn every_command_with_keys_is_listed() {
        let listed: Vec<Command> = groups().into_iter().flat_map(|(_, c)| c).map(|(_, c)| c).collect();
        for preset in Preset::ALL {
            let keys = KeySettings { preset, ..Default::default() };
            for (key, command) in keys.bindings() {
                assert!(listed.contains(&command), "{key}: {command:?} is on no list");
            }
        }
    }
}
