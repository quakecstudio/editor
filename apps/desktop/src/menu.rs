// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar (ADR-0003, ADR-0010): the app menu, a File menu with Open Folder, and a
//! View menu whose items run the panel commands, through `muda`. Windows and Linux get theirs
//! with the client-side title bar.

use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use qs_layout::{Layout, Panel};

use crate::strings::Text;

/// A menu item chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuChoice {
    /// File, Open Folder.
    OpenFolder,
    /// A View item: show or hide the panel.
    Toggle(Panel),
}

/// The menu bar, kept alive while the app runs.
pub struct MenuBar {
    _menu: Menu,
    open_folder: MenuItem,
    items: [(Panel, CheckMenuItem); 3],
}

impl MenuBar {
    /// Builds the menu bar and installs it in the app, its View items checked as `layout`
    /// shows the panels.
    ///
    /// # Errors
    ///
    /// `muda`'s error when a menu cannot be built.
    pub fn install(layout: &Layout) -> Result<Self, muda::Error> {
        let item = |panel: Panel, text: Text| {
            (
                panel,
                CheckMenuItem::new(text.get(), true, layout.panel(panel).visible, None),
            )
        };
        let items = [
            item(Panel::Left, Text::ToggleLeftPanel),
            item(Panel::Right, Text::ToggleRightPanel),
            item(Panel::Bottom, Text::ToggleBottomPanel),
        ];
        let app = Submenu::with_items(
            Text::AppName.get(),
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::quit(None),
            ],
        )?;
        // Cmd+O is the window's own binding (`keymap::OPEN_FOLDER`), on every platform; an
        // accelerator here would take the key first on macOS and open the dialog twice.
        let open_folder = MenuItem::new(Text::OpenFolderMenu.get(), true, None);
        let file = Submenu::with_items(Text::File.get(), true, &[&open_folder])?;
        let view = Submenu::with_items(
            Text::ViewMenu.get(),
            true,
            &[&items[0].1, &items[1].1, &items[2].1],
        )?;
        let menu = Menu::with_items(&[&app, &file, &view])?;
        menu.init_for_nsapp();
        Ok(Self {
            _menu: menu,
            open_folder,
            items,
        })
    }

    /// The items chosen since the last call.
    pub fn chosen(&self) -> Vec<MenuChoice> {
        let mut chosen = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == self.open_folder.id() {
                chosen.push(MenuChoice::OpenFolder);
            } else if let Some((panel, _)) =
                self.items.iter().find(|(_, item)| item.id() == event.id())
            {
                chosen.push(MenuChoice::Toggle(*panel));
            }
        }
        chosen
    }

    /// Checks each View item as `layout` shows its panel.
    pub fn sync(&self, layout: &Layout) {
        for (panel, item) in &self.items {
            item.set_checked(layout.panel(*panel).visible);
        }
    }
}
