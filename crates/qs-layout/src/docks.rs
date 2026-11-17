// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's docks as Zed's (P7-15, UI-2): each of the left, right and bottom docks shows one
//! panel at a time, chosen from the status bar, and each panel can be moved to another dock, as
//! Zed's `Dock` holds its panels and its `PanelButtons` switch and move them
//! (zed-industries/zed@4a6121bb, `crates/workspace/src/dock.rs`). Commands switch and move them,
//! so the status bar, the palette, the CLI and the agent share them, and the layout file keeps
//! them.

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasLayout, Panel};

/// The id of the command that shows a panel, or hides its dock when it already shows.
pub const SHOW_PANEL: &str = "view.show_panel";

/// The id of the command that moves a panel to another dock.
pub const MOVE_PANEL: &str = "view.move_panel";

/// One panel a dock shows.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DockPanel {
    /// The project's files.
    Project,
    /// The open file's symbols, or the project's.
    Symbols,
    /// The running game's entities.
    Entities,
    /// The open map's outline.
    Map,
    /// The selection's properties.
    Properties,
    /// The agent.
    Agent,
    /// The problems.
    Problems,
    /// The game's console.
    Console,
    /// The debugger.
    Debugger,
    /// The terminals.
    Terminal,
    /// Git.
    Git,
}

impl DockPanel {
    /// Every panel, in the status bar's order.
    pub const ALL: [Self; 11] = [
        Self::Project,
        Self::Symbols,
        Self::Entities,
        Self::Map,
        Self::Properties,
        Self::Agent,
        Self::Problems,
        Self::Console,
        Self::Debugger,
        Self::Terminal,
        Self::Git,
    ];

    /// The dock it starts in, as the approved play-loop mockup places it (UI-2).
    #[must_use]
    pub const fn home(self) -> Panel {
        match self {
            Self::Project | Self::Symbols | Self::Entities | Self::Map => Panel::Left,
            Self::Properties | Self::Agent => Panel::Right,
            Self::Problems | Self::Console | Self::Debugger | Self::Terminal | Self::Git => {
                Panel::Bottom
            }
        }
    }

    /// Its place in [`DockPanel::ALL`].
    const fn index(self) -> usize {
        self as usize
    }
}

/// Which dock each panel is in and which panel each dock shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Docks {
    /// Each panel's dock, in [`DockPanel::ALL`]'s order.
    places: [Panel; 11],
    /// The panel the left dock shows.
    left: DockPanel,
    /// The panel the right dock shows.
    right: DockPanel,
    /// The panel the bottom dock shows.
    bottom: DockPanel,
}

impl Default for Docks {
    /// Every panel in its home dock; the left dock showing the project, the right the
    /// properties and the bottom the problems.
    fn default() -> Self {
        Self {
            places: DockPanel::ALL.map(DockPanel::home),
            left: DockPanel::Project,
            right: DockPanel::Properties,
            bottom: DockPanel::Problems,
        }
    }
}

impl Docks {
    /// The dock `panel` is in.
    #[must_use]
    pub fn place(&self, panel: DockPanel) -> Panel {
        self.places
            .get(panel.index())
            .copied()
            .unwrap_or(panel.home())
    }

    /// The panels in `dock`, in the status bar's order.
    pub fn panels(&self, dock: Panel) -> impl Iterator<Item = DockPanel> + '_ {
        DockPanel::ALL
            .into_iter()
            .filter(move |&panel| self.place(panel) == dock)
    }

    /// The panel `dock` shows: the one chosen, or its first when that one has moved away; none
    /// when no panel is in it.
    #[must_use]
    pub fn shown(&self, dock: Panel) -> Option<DockPanel> {
        let chosen = match dock {
            Panel::Left => self.left,
            Panel::Right => self.right,
            Panel::Bottom => self.bottom,
        };
        if self.place(chosen) == dock {
            Some(chosen)
        } else {
            self.panels(dock).next()
        }
    }

    /// Makes `panel` the one its dock shows.
    fn choose(&mut self, panel: DockPanel) {
        match self.place(panel) {
            Panel::Left => self.left = panel,
            Panel::Right => self.right = panel,
            Panel::Bottom => self.bottom = panel,
        }
    }
}

/// What `view.show_panel` does when the dock already shows the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WhenShown {
    /// Hide the dock, as a second click on Zed's status-bar button does.
    #[default]
    Hide,
    /// Keep it shown, as showing a panel for something it holds does.
    Keep,
}

/// The parameters of `view.show_panel`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowPanelParams {
    /// The panel.
    pub panel: DockPanel,
    /// What to do when its dock already shows it: hide the dock unless `keep`.
    #[serde(default)]
    pub when_shown: WhenShown,
}

/// The parameters of `view.move_panel`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MovePanelParams {
    /// The panel.
    pub panel: DockPanel,
    /// The dock it moves to.
    pub dock: Panel,
}

/// `view.show_panel`.
struct ShowPanel;

impl<C: HasLayout + 'static> Command<C> for ShowPanel {
    const ID: &'static str = SHOW_PANEL;
    const SUMMARY: &'static str = "Shows a panel in its dock, opening the dock; when the dock \
        already shows it, hides the dock, as a click on Zed's status-bar button does, unless \
        when_shown is keep.";
    type Params = ShowPanelParams;
    type Output = Docks;
    type Error = !;

    fn run(&self, context: &mut C, params: ShowPanelParams) -> Result<Outcome<Docks, C>, !> {
        let layout = context.layout_mut();
        let dock = layout.docks.place(params.panel);
        let showing = layout.docks.shown(dock) == Some(params.panel);
        let state = layout.panel_mut(dock);
        if showing && state.visible && params.when_shown == WhenShown::Hide {
            state.visible = false;
        } else {
            state.visible = true;
            layout.docks.choose(params.panel);
        }
        Ok(Outcome::outside_history(layout.docks))
    }
}

/// `view.move_panel`.
struct MovePanel;

impl<C: HasLayout + 'static> Command<C> for MovePanel {
    const ID: &'static str = MOVE_PANEL;
    const SUMMARY: &'static str = "Moves a panel to another dock, where it shows, as Zed's \
        status-bar menu moves one.";
    type Params = MovePanelParams;
    type Output = Docks;
    type Error = !;

    fn run(&self, context: &mut C, params: MovePanelParams) -> Result<Outcome<Docks, C>, !> {
        let layout = context.layout_mut();
        if let Some(place) = layout.docks.places.get_mut(params.panel.index()) {
            *place = params.dock;
        }
        layout.docks.choose(params.panel);
        layout.panel_mut(params.dock).visible = true;
        Ok(Outcome::outside_history(layout.docks))
    }
}

/// Registers `view.show_panel` and `view.move_panel`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ShowPanel)?;
    registry.register(MovePanel)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{DockPanel, Docks, MOVE_PANEL, SHOW_PANEL};
    use crate::{Layout, Panel};

    fn session() -> Session<Layout> {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        Session::new(Layout::default(), registry)
    }

    // No spec statement: P7-15: each panel starts in its home dock, each dock showing its
    // first, as the play-loop mockup places them (UI-2)
    #[test]
    fn panels_start_in_their_home_docks() {
        let docks = Docks::default();
        assert_eq!(
            docks.panels(Panel::Left).collect::<Vec<_>>(),
            [
                DockPanel::Project,
                DockPanel::Symbols,
                DockPanel::Entities,
                DockPanel::Map
            ]
        );
        assert_eq!(
            docks.panels(Panel::Right).collect::<Vec<_>>(),
            [DockPanel::Properties, DockPanel::Agent]
        );
        assert_eq!(docks.panels(Panel::Bottom).count(), 5);
        assert_eq!(docks.shown(Panel::Left), Some(DockPanel::Project));
        assert_eq!(docks.shown(Panel::Right), Some(DockPanel::Properties));
        assert_eq!(docks.shown(Panel::Bottom), Some(DockPanel::Problems));
    }

    // No spec statement: P7-15: view.show_panel shows a panel and opens its dock; run again
    // while the dock shows it, it hides the dock, as Zed's status-bar button toggles; outside
    // the undo history; the layout file keeps the choice, an older file reads as the default
    #[test]
    fn a_panel_is_shown_and_its_dock_toggled() {
        let mut session = session();
        session
            .execute(SHOW_PANEL, json!({ "panel": "map" }))
            .unwrap();
        let layout = *session.context();
        assert_eq!(layout.docks.shown(Panel::Left), Some(DockPanel::Map));
        assert!(layout.left.visible);
        session
            .execute(SHOW_PANEL, json!({ "panel": "map" }))
            .unwrap();
        assert!(
            !session.context().left.visible,
            "shown already: the dock hides"
        );
        session
            .execute(SHOW_PANEL, json!({ "panel": "map" }))
            .unwrap();
        assert!(session.context().left.visible, "and opens again");
        session
            .execute(SHOW_PANEL, json!({ "panel": "map", "when_shown": "keep" }))
            .unwrap();
        assert!(session.context().left.visible, "kept open when asked");
        assert!(session.undo().is_err());
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.docks, session.context().docks);
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.docks, Docks::default());
        assert!(
            session
                .execute(SHOW_PANEL, json!({ "panel": "nothing" }))
                .is_err()
        );
    }

    // No spec statement: P7-15: view.move_panel puts a panel in another dock, shows it there and
    // opens that dock; the dock it left shows its first remaining panel, and a dock with none
    // shows nothing
    #[test]
    fn a_panel_moves_to_another_dock() {
        let mut session = session();
        session
            .execute(SHOW_PANEL, json!({ "panel": "agent" }))
            .unwrap();
        session
            .execute(SHOW_PANEL, json!({ "panel": "agent" }))
            .unwrap();
        session
            .execute(MOVE_PANEL, json!({ "panel": "agent", "dock": "left" }))
            .unwrap();
        let docks = session.context().docks;
        assert_eq!(docks.place(DockPanel::Agent), Panel::Left);
        assert_eq!(docks.shown(Panel::Left), Some(DockPanel::Agent));
        assert_eq!(docks.shown(Panel::Right), Some(DockPanel::Properties));
        assert!(session.context().left.visible);
        session
            .execute(
                MOVE_PANEL,
                json!({ "panel": "properties", "dock": "bottom" }),
            )
            .unwrap();
        let docks = session.context().docks;
        assert_eq!(docks.shown(Panel::Right), None, "nothing left in it");
        assert_eq!(docks.shown(Panel::Bottom), Some(DockPanel::Properties));
        assert!(
            session
                .execute(MOVE_PANEL, json!({ "panel": "agent", "dock": "top" }))
                .is_err()
        );
    }
}
