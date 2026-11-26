// SPDX-License-Identifier: GPL-3.0-or-later

//! The menu bar (ADR-0003, ADR-0010, P7-17): the menus a Mac app has, each item a command of
//! the registry with its key shown. The menus are a table here, read on every platform by the
//! test that holds each item to a command and each keyed command to an item; macOS draws the
//! table through `muda` ([`native`]). Windows and Linux get theirs with the client-side title
//! bar.
//!
//! A key equivalent in the menu bar is taken by the menu before the window sees the key, so an
//! item with a key gives the key back to the window ([`Chosen::Key`]), which does with it what
//! it does where the keyboard is: Cmd+Z takes back typing in the editor and a brush in the map
//! view. An item of the Map menu gives the map view the keyboard first, where the map's keys
//! are taken. An item with no key runs its command, and so does one whose key is the map
//! view's own, which the menu does not take: a field being typed in has uses for those keys.
//! Choosing an item with the mouse does the same as pressing its key, so Edit, Undo acts on
//! what has the keyboard.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use qs_layout::{DEFAULT_KEYS, KeyContext, Panel};
use qs_map_edit::backup::MAP_BACKUP;
use qs_workbench::checkpoints::CREATE_CHECKPOINT;
use qs_workbench::export::PROJECT_EXPORT;
use qs_workbench::map_compile::MAP_LEAKS;
use qs_workbench::map_profile::{MAP_CHOOSE_PROFILE, MAP_RUN_PROFILE};
use qs_workbench::tools::{COMPILE, PROJECT_FILES};

use crate::keymap;
use crate::shell::ZoomStep;
use crate::strings::Text;

#[cfg(target_os = "macos")]
pub mod native;

/// An item the window carries out its own way: with a dialog first, or with what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuChoice {
    /// File, Open Folder.
    OpenFolder,
    /// File, New Project: the New Project dialog (P4-17, L-28).
    NewProject,
    /// File, New Random Map: a generated level, opened in the map tab (L-48).
    RandomMap,
    /// File, Go to File: the file finder.
    FileFinder,
    /// File, Quake Files: the guide to the game's files (P3-21).
    GameFiles,
    /// File, Export Mod: the open project as a zip players install (P4-28).
    ExportMod,
    /// A View item: show or hide the panel.
    Toggle(Panel),
    /// View, Command Palette.
    Palette,
    /// View, Zoom In, Zoom Out or Actual Size (P7-12).
    Zoom(ZoomStep),
}

impl MenuChoice {
    /// The registry command the item is the window's way of running.
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::OpenFolder => qs_workbench::OPEN_PROJECT,
            Self::NewProject => qs_templates::NEW_PROJECT,
            Self::RandomMap => qs_map_edit::MAP_GENERATE,
            Self::FileFinder => PROJECT_FILES,
            Self::GameFiles => qs_workbench::CHOOSE_GAME,
            Self::ExportMod => PROJECT_EXPORT,
            Self::Toggle(_) => qs_layout::TOGGLE_PANEL,
            Self::Palette => qs_commands::LIST_COMMANDS,
            Self::Zoom(_) => qs_layout::ZOOM,
        }
    }

    /// The item's title.
    #[must_use]
    pub const fn title(self) -> Text {
        match self {
            Self::OpenFolder => Text::OpenFolderMenu,
            Self::NewProject => Text::NewProjectMenu,
            Self::RandomMap => Text::NewRandomMapMenu,
            Self::FileFinder => Text::FileFinderMenu,
            Self::GameFiles => Text::GameFilesMenu,
            Self::ExportMod => Text::ExportModMenu,
            Self::Toggle(Panel::Left) => Text::ToggleLeftPanel,
            Self::Toggle(Panel::Right) => Text::ToggleRightPanel,
            Self::Toggle(Panel::Bottom) => Text::ToggleBottomPanel,
            Self::Palette => Text::PaletteMenu,
            Self::Zoom(ZoomStep::In) => Text::ZoomInMenu,
            Self::Zoom(ZoomStep::Out) => Text::ZoomOutMenu,
            Self::Zoom(ZoomStep::Actual) => Text::ActualSizeMenu,
        }
    }

    /// The key the window takes for the item itself, outside the keymap; none for an item
    /// whose key, if it has one, is its command's in the keymap.
    #[must_use]
    pub const fn window_key(self) -> Option<KeyboardShortcut> {
        match self {
            Self::OpenFolder => Some(keymap::OPEN_FOLDER),
            Self::FileFinder => Some(keymap::OPEN_FILE_FINDER),
            Self::Palette => Some(keymap::OPEN_PALETTE),
            // egui's own zoom keys, which the menu takes first and so runs itself.
            Self::Zoom(ZoomStep::In) => Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus)),
            Self::Zoom(ZoomStep::Out) => {
                Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus))
            }
            Self::Zoom(ZoomStep::Actual) => {
                Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0))
            }
            Self::NewProject
            | Self::RandomMap
            | Self::GameFiles
            | Self::ExportMod
            | Self::Toggle(_) => None,
        }
    }
}

/// One of the system's own items, which macOS carries out or the window is told of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemItem {
    /// Hide the app.
    Hide,
    /// Hide every other app.
    HideOthers,
    /// Show every app.
    ShowAll,
    /// Quit the app.
    Quit,
    /// Cut the selection to the clipboard.
    Cut,
    /// Copy the selection to the clipboard.
    Copy,
    /// Paste the clipboard.
    Paste,
    /// Select everything where the keyboard is.
    SelectAll,
    /// Minimize the window.
    Minimize,
    /// Zoom the window, as its green button does.
    Maximize,
    /// Make the window fill the screen, or stop.
    Fullscreen,
}

/// An item of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A registry command that takes no parameters, titled by
    /// [`crate::strings::menu_title`]; its key is the keymap's.
    Command(&'static str),
    /// An item the window carries out its own way.
    Choice(MenuChoice),
    /// One of the system's own.
    System(SystemItem),
    /// A line between groups of items.
    Separator,
    /// A menu inside the menu holding every command the map view has a default key for, in the
    /// keymap's order: its standard views, its camera's moves and its saved views.
    MapViewKeys(Text),
}

/// A menu of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuSpec {
    /// Its title.
    pub title: Text,
    /// Which of the bar's menus it is to the system.
    pub role: Role,
    /// Its items, top to bottom.
    pub items: &'static [Item],
}

/// What a menu is to the system, beyond its items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The app's own menu, first in the bar under the app's name.
    App,
    /// An ordinary menu.
    Plain,
    /// The Map menu, whose items act on the map shown: off while no map is.
    Map,
    /// The Window menu, which macOS adds the open windows to.
    Window,
    /// The Help menu, which macOS adds its search to.
    Help,
}

/// What choosing an item asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// Press this key, which the menu took: the window does what the key does where the
    /// keyboard is.
    Key(KeyboardShortcut),
    /// Press this key of the Map menu, once the map view has the keyboard: the map's keys are
    /// taken there, and not while a field is typed in.
    MapKey(KeyboardShortcut),
    /// Run this command, which has no key the menu shows.
    Run(&'static str),
    /// Carry out this item.
    Choice(MenuChoice),
    /// Cut, copy, paste or select all where the keyboard is.
    Edit(SystemItem),
}

/// The menu bar, left to right.
pub const MENUS: &[MenuSpec] = &[
    MenuSpec {
        title: Text::AppName,
        role: Role::App,
        items: &[
            Item::Command(qs_layout::ABOUT),
            Item::Separator,
            Item::Command(qs_layout::OPEN_SETTINGS),
            Item::Separator,
            Item::System(SystemItem::Hide),
            Item::System(SystemItem::HideOthers),
            Item::System(SystemItem::ShowAll),
            Item::Separator,
            Item::System(SystemItem::Quit),
        ],
    },
    MenuSpec {
        title: Text::File,
        role: Role::Plain,
        items: &[
            Item::Choice(MenuChoice::NewProject),
            Item::Choice(MenuChoice::OpenFolder),
            Item::Choice(MenuChoice::RandomMap),
            Item::Choice(MenuChoice::FileFinder),
            Item::Separator,
            Item::Command(CREATE_CHECKPOINT),
            Item::Choice(MenuChoice::ExportMod),
            Item::Separator,
            Item::Choice(MenuChoice::GameFiles),
        ],
    },
    MenuSpec {
        title: Text::AgentEdit,
        role: Role::Plain,
        items: &[
            Item::Command(qs_commands::UNDO),
            Item::Command(qs_commands::REDO),
            Item::Command(qs_commands::REPEAT),
            Item::Separator,
            Item::System(SystemItem::Cut),
            Item::System(SystemItem::Copy),
            Item::System(SystemItem::Paste),
            Item::System(SystemItem::SelectAll),
            Item::Separator,
            Item::Command(qs_layout::PROJECT_SEARCH_VIEW),
            Item::Separator,
            Item::Command(qs_layout::FOLD),
            Item::Command(qs_layout::UNFOLD),
            Item::Command(qs_layout::FOLD_ALL),
            Item::Command(qs_layout::UNFOLD_ALL),
            Item::Command(qs_layout::TOGGLE_SOFT_WRAP),
        ],
    },
    MenuSpec {
        title: Text::ViewMenu,
        role: Role::Plain,
        items: &[
            Item::Choice(MenuChoice::Toggle(Panel::Left)),
            Item::Choice(MenuChoice::Toggle(Panel::Right)),
            Item::Choice(MenuChoice::Toggle(Panel::Bottom)),
            Item::Command(qs_layout::TOGGLE_OUTLINE),
            Item::Separator,
            Item::Command(qs_layout::SPLIT_RIGHT),
            Item::Command(qs_layout::SPLIT_DOWN),
            Item::Command(qs_layout::CLOSE_SPLIT),
            Item::Command(qs_layout::TOGGLE_EMBEDDED_GAME),
            Item::Command(qs_layout::POP_OUT_GAME),
            Item::Command(qs_layout::POP_IN_GAME),
            Item::Command(qs_layout::TOGGLE_GAME_FULL_SCREEN),
            Item::Separator,
            Item::Choice(MenuChoice::Palette),
            Item::Separator,
            Item::Choice(MenuChoice::Zoom(ZoomStep::In)),
            Item::Choice(MenuChoice::Zoom(ZoomStep::Out)),
            Item::Choice(MenuChoice::Zoom(ZoomStep::Actual)),
        ],
    },
    MenuSpec {
        title: Text::GoMenu,
        role: Role::Plain,
        items: &[
            Item::Command(qs_layout::FOCUS_EDITOR),
            Item::Command(qs_layout::FOCUS_LEFT_PANEL),
            Item::Command(qs_layout::FOCUS_RIGHT_PANEL),
            Item::Command(qs_layout::FOCUS_BOTTOM_PANEL),
            Item::Command(qs_layout::FOCUS_COMMAND_LINE),
            Item::Separator,
            Item::Command(qs_layout::GO_TO_LINE),
            Item::Command(qs_layout::GO_TO_SYMBOL),
            Item::Command(qs_layout::TAB_SWITCHER),
        ],
    },
    MenuSpec {
        title: Text::PaletteRunButton,
        role: Role::Plain,
        items: &[
            Item::Command(qs_workbench::PLAY),
            Item::Command(qs_workbench::PAUSE),
            Item::Command(qs_workbench::RESUME),
            Item::Command(qs_workbench::STEP),
            Item::Command(qs_workbench::STOP),
            Item::Command(qs_workbench::RESTART),
            Item::Command(qs_workbench::RELOAD),
            Item::Separator,
            Item::Command(qs_workbench::BUILD_PROJECT),
            Item::Command(qs_workbench::CHECK_PROJECT),
            Item::Command(COMPILE),
            Item::Command(qs_workbench::RUN_TESTS),
            Item::Separator,
            Item::Command(qs_workbench::TERMINAL_NEW),
        ],
    },
    MenuSpec {
        title: Text::Map,
        role: Role::Map,
        items: &[
            Item::Command(qs_map_edit::MAP_SAVE),
            Item::Command(MAP_BACKUP),
            Item::Separator,
            Item::Command(MAP_RUN_PROFILE),
            Item::Command(MAP_CHOOSE_PROFILE),
            Item::Command(MAP_LEAKS),
            Item::Separator,
            Item::Command(qs_map_edit::MAP_COPY),
            Item::Command(qs_map_edit::MAP_SELECT),
            Item::Command(qs_map_edit::MAP_SHOW_ALL),
            Item::Command(qs_map_edit::MAP_UNLOCK_ALL),
            Item::Separator,
            Item::Command(qs_map_edit::MAP_STAGE),
            Item::Command(qs_map_edit::MAP_UNSTAGE),
            Item::Command(qs_map_edit::MAP_ACCEPT),
            Item::Command(qs_map_edit::MAP_REJECT),
            Item::Separator,
            Item::MapViewKeys(Text::MapCamera),
        ],
    },
    MenuSpec {
        title: Text::WindowMenu,
        role: Role::Window,
        items: &[
            Item::System(SystemItem::Minimize),
            Item::System(SystemItem::Maximize),
            Item::System(SystemItem::Fullscreen),
        ],
    },
    MenuSpec {
        title: Text::HelpMenu,
        role: Role::Help,
        items: &[
            Item::Command(qs_layout::USER_GUIDE),
            Item::Command(qs_layout::REPORT_ISSUE),
            Item::Separator,
            Item::Command(qs_layout::CHECK_FOR_UPDATES),
        ],
    },
];

impl MenuSpec {
    /// Every item of the menu and of the menu inside it, in order.
    #[must_use]
    pub fn flat(&self) -> Vec<Item> {
        let mut out = Vec::new();
        for item in self.items {
            match item {
                Item::MapViewKeys(_) => out.extend(map_view_commands().map(Item::Command)),
                Item::Command(_) | Item::Choice(_) | Item::System(_) | Item::Separator => {
                    out.push(*item);
                }
            }
        }
        out
    }
}

/// The commands the map view has a default key for, in the keymap's order.
pub fn map_view_commands() -> impl Iterator<Item = &'static str> {
    DEFAULT_KEYS
        .iter()
        .filter(|key| key.context == KeyContext::MapView)
        .map(|key| key.command)
}
