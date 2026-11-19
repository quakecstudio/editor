// SPDX-License-Identifier: GPL-3.0-or-later

//! The menu bar on macOS, drawn from [`super::MENUS`] through `muda`.

use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use qs_layout::{Layout, Panel};

use super::{Chosen, Item, MENUS, MenuChoice, Role, SystemItem, map_view_commands};
use crate::keymap::Keymap;
use crate::strings::{Text, menu_title};

/// An item of the bar that the window is told of when it is chosen.
struct Entry {
    id: MenuId,
    item: Item,
    /// The menu it is in.
    role: Role,
    /// The key shown beside it, which the menu takes.
    key: Option<KeyboardShortcut>,
    /// The item, for a command whose key follows the keymap.
    handle: Option<MenuItem>,
}

/// The menu bar, kept alive while the app runs.
pub struct MenuBar {
    _menu: Menu,
    entries: Vec<Entry>,
    panels: Vec<(Panel, CheckMenuItem)>,
    /// The Map menu, and whether it is on.
    map: Option<(Submenu, MapShown)>,
    chosen: Receiver<MenuEvent>,
}

/// Whether a map shows in the window, which the Map menu's items act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapShown {
    /// One does.
    Shown,
    /// None does.
    Hidden,
}

impl MenuBar {
    /// Builds the menu bar and installs it in the app: its View items checked as `layout`
    /// shows the panels, and each command's key as `keymap` binds it. `wake` is called when an
    /// item is chosen, from the main thread, so a frame is drawn that carries it out.
    ///
    /// # Errors
    ///
    /// `muda`'s error when a menu cannot be built.
    pub fn install(
        layout: &Layout,
        keymap: &Keymap,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, muda::Error> {
        let menu = Menu::new();
        let mut bar = Self {
            _menu: menu.clone(),
            entries: Vec::new(),
            panels: Vec::new(),
            map: None,
            chosen: channel().1,
        };
        let mut window = None;
        let mut help = None;
        for spec in MENUS {
            let submenu = Submenu::new(spec.title.get(), true);
            bar.fill(&submenu, (spec.role, spec.items), layout, keymap)?;
            menu.append(&submenu)?;
            match spec.role {
                Role::Window => window = Some(submenu),
                Role::Help => help = Some(submenu),
                Role::Map => bar.map = Some((submenu, MapShown::Shown)),
                Role::App | Role::Plain => {}
            }
        }
        let (sender, chosen) = channel();
        bar.chosen = chosen;
        MenuEvent::set_event_handler(Some(move |event| {
            // The receiver lives as long as the app; a send after it is gone has no one to
            // tell.
            let _gone = sender.send(event);
            wake();
        }));
        menu.init_for_nsapp();
        // muda finds a menu to give AppKit one of these roles in the app's installed menu
        // bar, so before the bar is installed either call does nothing, with no error: the
        // Window menu would not list the open windows, and AppKit would put the Help
        // search on a menu of its own choosing.
        if let Some(window) = window {
            window.set_as_windows_menu_for_nsapp();
        }
        if let Some(help) = help {
            help.set_as_help_menu_for_nsapp();
        }
        if let Some(main) = objc2::MainThreadMarker::new() {
            let app = objc2_app_kit::NSApplication::sharedApplication(main);
            if app.windowsMenu().is_none() || app.helpMenu().is_none() {
                tracing::warn!("the Window or Help menu did not take its role");
            }
        }
        Ok(bar)
    }

    /// Appends `items` to `menu`, keeping each one the window is told of.
    fn fill(
        &mut self,
        menu: &Submenu,
        (role, items): (Role, &'static [Item]),
        layout: &Layout,
        keymap: &Keymap,
    ) -> Result<(), muda::Error> {
        for item in items {
            match *item {
                Item::Separator => menu.append(&PredefinedMenuItem::separator())?,
                Item::MapViewKeys(title) => {
                    let submenu = Submenu::new(title.get(), true);
                    for id in map_view_commands() {
                        self.command(&submenu, (role, id), keymap)?;
                    }
                    menu.append(&submenu)?;
                }
                Item::System(system) => match system.told() {
                    Some((title, key)) => {
                        let made = MenuItem::new(title.get(), true, accelerator(key));
                        menu.append(&made)?;
                        self.keep(&made, (role, *item), Some(key), None);
                    }
                    None => menu.append(&system.predefined())?,
                },
                Item::Choice(MenuChoice::Toggle(panel)) => {
                    let title = MenuChoice::Toggle(panel).title().get();
                    let made = CheckMenuItem::new(title, true, layout.panel(panel).visible, None);
                    menu.append(&made)?;
                    self.entries.push(Entry {
                        id: made.id().clone(),
                        item: *item,
                        role,
                        key: None,
                        handle: None,
                    });
                    self.panels.push((panel, made));
                }
                Item::Choice(choice) => {
                    let key = choice
                        .window_key()
                        .or_else(|| keymap.menu_shortcut(choice.command()));
                    let made = MenuItem::new(choice.title().get(), true, key.and_then(accelerator));
                    menu.append(&made)?;
                    let follows = choice.window_key().is_none().then(|| made.clone());
                    self.keep(&made, (role, *item), key, follows);
                }
                Item::Command(id) => self.command(menu, (role, id), keymap)?,
            }
        }
        Ok(())
    }

    /// Appends the item of the command `id` to `menu`, with the key `keymap` binds it to.
    fn command(
        &mut self,
        menu: &Submenu,
        (role, id): (Role, &'static str),
        keymap: &Keymap,
    ) -> Result<(), muda::Error> {
        let key = keymap.menu_shortcut(id);
        let title = menu_title(id).unwrap_or(id);
        let made = MenuItem::new(title, true, key.and_then(accelerator));
        menu.append(&made)?;
        self.keep(&made, (role, Item::Command(id)), key, Some(made.clone()));
        Ok(())
    }

    /// Keeps `made`, the item `item` of the menu `role` with the key `key`, to tell the window when it is chosen;
    /// `follows` is the item when its key follows the keymap.
    fn keep(
        &mut self,
        made: &MenuItem,
        (role, item): (Role, Item),
        key: Option<KeyboardShortcut>,
        follows: Option<MenuItem>,
    ) {
        self.entries.push(Entry {
            id: made.id().clone(),
            item,
            role,
            // A key the menu cannot show is not taken by it: the window still has it.
            key: key.filter(|key| accelerator(*key).is_some()),
            handle: follows,
        });
    }

    /// What the items chosen since the last call ask of the window.
    pub fn chosen(&self) -> Vec<Chosen> {
        let mut chosen = Vec::new();
        while let Ok(event) = self.chosen.try_recv() {
            let Some(entry) = self.entries.iter().find(|entry| entry.id == *event.id()) else {
                continue;
            };
            chosen.push(match (entry.item, entry.key) {
                (Item::System(system), _) => Chosen::Edit(system),
                // The window's own key for it is the item; any other is carried out the
                // window's way, with its dialog and its notice, whatever key it shows.
                (Item::Choice(choice), Some(key)) if choice.window_key() == Some(key) => {
                    match choice {
                        MenuChoice::Zoom(_) => Chosen::Choice(choice),
                        MenuChoice::OpenFolder
                        | MenuChoice::NewProject(_)
                        | MenuChoice::FileFinder
                        | MenuChoice::GameFiles
                        | MenuChoice::ExportMod
                        | MenuChoice::Toggle(_)
                        | MenuChoice::Palette => Chosen::Key(key),
                    }
                }
                (Item::Choice(choice), _) => Chosen::Choice(choice),
                (Item::Command(_), Some(key)) if entry.role == Role::Map => Chosen::MapKey(key),
                (Item::Command(_), Some(key)) => Chosen::Key(key),
                (Item::Command(id), None) => Chosen::Run(id),
                (Item::Separator | Item::MapViewKeys(_), _) => continue,
            });
        }
        chosen
    }

    /// Checks each View item as `layout` shows its panel, shows each command's key as `keymap`
    /// binds it now, and turns the Map menu on only while a map shows.
    pub fn sync(&mut self, layout: &Layout, keymap: &Keymap, map: MapShown) {
        for (panel, item) in &self.panels {
            item.set_checked(layout.panel(*panel).visible);
        }
        for entry in &mut self.entries {
            let command = match entry.item {
                Item::Command(id) => id,
                Item::Choice(choice) => choice.command(),
                Item::System(_) | Item::Separator | Item::MapViewKeys(_) => continue,
            };
            let Some(handle) = &entry.handle else {
                continue;
            };
            let key = keymap
                .menu_shortcut(command)
                .filter(|key| accelerator(*key).is_some());
            if key != entry.key {
                if let Err(error) = handle.set_accelerator(key.and_then(accelerator)) {
                    tracing::warn!(%error, command, "menu key not shown");
                }
                entry.key = key;
            }
        }
        if let Some((menu, shown)) = &mut self.map
            && *shown != map
        {
            menu.set_enabled(map == MapShown::Shown);
            *shown = map;
        }
    }
}

impl SystemItem {
    /// The title and key of an item the window is told of, since egui has the text and the
    /// map's selection, not a responder macOS could ask; none for one macOS carries out.
    const fn told(self) -> Option<(Text, KeyboardShortcut)> {
        let command = eframe::egui::Modifiers::COMMAND;
        match self {
            Self::Cut => Some((Text::CutMenu, KeyboardShortcut::new(command, Key::X))),
            Self::Copy => Some((Text::CopyMenu, KeyboardShortcut::new(command, Key::C))),
            Self::Paste => Some((Text::PasteMenu, KeyboardShortcut::new(command, Key::V))),
            Self::SelectAll => Some((Text::SelectAllMenu, KeyboardShortcut::new(command, Key::A))),
            Self::Hide
            | Self::HideOthers
            | Self::ShowAll
            | Self::Quit
            | Self::Minimize
            | Self::Maximize
            | Self::Fullscreen => None,
        }
    }

    /// The system's item, titled in the system's language, but for the two that name the app:
    /// a build run outside its bundle would be named by its binary there.
    fn predefined(self) -> PredefinedMenuItem {
        match self {
            Self::Hide => PredefinedMenuItem::hide(Some(Text::HideApp.get())),
            Self::HideOthers => PredefinedMenuItem::hide_others(None),
            Self::ShowAll => PredefinedMenuItem::show_all(None),
            Self::Quit => PredefinedMenuItem::quit(Some(Text::QuitApp.get())),
            Self::Cut => PredefinedMenuItem::cut(None),
            Self::Copy => PredefinedMenuItem::copy(None),
            Self::Paste => PredefinedMenuItem::paste(None),
            Self::SelectAll => PredefinedMenuItem::select_all(None),
            Self::Minimize => PredefinedMenuItem::minimize(None),
            Self::Maximize => PredefinedMenuItem::maximize(None),
            Self::Fullscreen => PredefinedMenuItem::fullscreen(None),
        }
    }
}

/// `shortcut` as the menu shows a key; none for a key `muda` has no name for, which the
/// window then keeps.
fn accelerator(shortcut: KeyboardShortcut) -> Option<Accelerator> {
    let held = shortcut.modifiers;
    let mut modifiers = Modifiers::empty();
    if held.command || held.mac_cmd {
        modifiers |= Modifiers::META;
    }
    if held.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if held.alt {
        modifiers |= Modifiers::ALT;
    }
    if held.shift {
        modifiers |= Modifiers::SHIFT;
    }
    let code = match shortcut.logical_key {
        Key::A => Code::KeyA,
        Key::B => Code::KeyB,
        Key::C => Code::KeyC,
        Key::D => Code::KeyD,
        Key::E => Code::KeyE,
        Key::F => Code::KeyF,
        Key::G => Code::KeyG,
        Key::H => Code::KeyH,
        Key::I => Code::KeyI,
        Key::J => Code::KeyJ,
        Key::K => Code::KeyK,
        Key::L => Code::KeyL,
        Key::M => Code::KeyM,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::P => Code::KeyP,
        Key::Q => Code::KeyQ,
        Key::R => Code::KeyR,
        Key::S => Code::KeyS,
        Key::T => Code::KeyT,
        Key::U => Code::KeyU,
        Key::V => Code::KeyV,
        Key::W => Code::KeyW,
        Key::X => Code::KeyX,
        Key::Y => Code::KeyY,
        Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4,
        Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6,
        Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8,
        Key::Num9 => Code::Digit9,
        Key::F1 => Code::F1,
        Key::F2 => Code::F2,
        Key::F3 => Code::F3,
        Key::F4 => Code::F4,
        Key::F5 => Code::F5,
        Key::F6 => Code::F6,
        Key::F7 => Code::F7,
        Key::F8 => Code::F8,
        Key::F9 => Code::F9,
        Key::F10 => Code::F10,
        Key::F11 => Code::F11,
        Key::F12 => Code::F12,
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Tab => Code::Tab,
        Key::Backtick => Code::Backquote,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        Key::Backslash => Code::Backslash,
        Key::Semicolon => Code::Semicolon,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Slash => Code::Slash,
        Key::Minus => Code::Minus,
        Key::Plus | Key::Equals => Code::Equal,
        _ => return None,
    };
    Some(Accelerator::new(modifiers, code))
}
