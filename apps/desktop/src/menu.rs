// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar (ADR-0003, ADR-0010): the app menu and a View menu whose items run the
//! panel commands, through `muda`. Windows and Linux get theirs with the client-side title bar.

use muda::{CheckMenuItem, Menu, MenuEvent, PredefinedMenuItem, Submenu};
use qs_layout::{Layout, Panel};

use crate::strings::Text;

/// The menu bar, kept alive while the app runs.
pub struct MenuBar {
    _menu: Menu,
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
        let view = Submenu::with_items(
            Text::ViewMenu.get(),
            true,
            &[&items[0].1, &items[1].1, &items[2].1],
        )?;
        let menu = Menu::with_items(&[&app, &view])?;
        menu.init_for_nsapp();
        Ok(Self { _menu: menu, items })
    }

    /// The panels whose View item was chosen since the last call.
    pub fn chosen(&self) -> Vec<Panel> {
        let mut panels = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some((panel, _)) = self.items.iter().find(|(_, item)| item.id() == event.id()) {
                panels.push(*panel);
            }
        }
        panels
    }

    /// Checks each View item as `layout` shows its panel.
    pub fn sync(&self, layout: &Layout) {
        for (panel, item) in &self.items {
            item.set_checked(layout.panel(*panel).visible);
        }
    }
}
