// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar (ADR-0003, ADR-0010): the app menu, a File menu with Open Folder and Quake
//! Files, and a
//! View menu whose items run the panel commands, through `muda`. Windows and Linux get theirs
//! with the client-side title bar.

use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use qs_layout::{Layout, Panel};
use qs_templates::Template;

use crate::strings::Text;

/// A menu item chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuChoice {
    /// File, Open Folder.
    OpenFolder,
    /// File, New id1 Project or New Rerelease Project (P4-17).
    NewProject(Template),
    /// File, Quake Files: the guide to the game's files (P3-21).
    GameFiles,
    /// File, Export Mod: the open project as a zip players install (P4-28).
    ExportMod,
    /// A View item: show or hide the panel.
    Toggle(Panel),
}

/// The menu bar, kept alive while the app runs.
pub struct MenuBar {
    _menu: Menu,
    open_folder: MenuItem,
    new_projects: [(Template, MenuItem); 2],
    game_files: MenuItem,
    export_mod: MenuItem,
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
        let new_projects = [
            (
                Template::Id1,
                MenuItem::new(Text::NewId1Menu.get(), true, None),
            ),
            (
                Template::Rerelease,
                MenuItem::new(Text::NewRereleaseMenu.get(), true, None),
            ),
        ];
        let game_files = MenuItem::new(Text::GameFilesMenu.get(), true, None);
        let export_mod = MenuItem::new(Text::ExportModMenu.get(), true, None);
        let file = Submenu::with_items(
            Text::File.get(),
            true,
            &[
                &new_projects[0].1,
                &new_projects[1].1,
                &open_folder,
                &export_mod,
                &game_files,
            ],
        )?;
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
            new_projects,
            game_files,
            export_mod,
            items,
        })
    }

    /// The items chosen since the last call.
    pub fn chosen(&self) -> Vec<MenuChoice> {
        let mut chosen = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == self.open_folder.id() {
                chosen.push(MenuChoice::OpenFolder);
            } else if let Some((template, _)) = self
                .new_projects
                .iter()
                .find(|(_, item)| item.id() == event.id())
            {
                chosen.push(MenuChoice::NewProject(*template));
            } else if event.id() == self.game_files.id() {
                chosen.push(MenuChoice::GameFiles);
            } else if event.id() == self.export_mod.id() {
                chosen.push(MenuChoice::ExportMod);
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
