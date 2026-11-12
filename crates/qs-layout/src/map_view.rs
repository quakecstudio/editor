// SPDX-License-Identifier: GPL-3.0-or-later

//! The map view's layout (P6-09, MAP-3): the 3D view alone, with a top view inset, or with top,
//! front and side views, after TrenchBroom's one-, two- and four-pane layouts, chosen by a
//! command so a menu, the CLI and the agent set it the same way.

use std::convert::Infallible;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command that sets the map view's panes.
pub const MAP_PANES: &str = "view.map_panes";

/// How the map view is split into panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MapPanes {
    /// The 3D view alone.
    #[default]
    Single,
    /// The 3D view with a top view in its lower right corner.
    Inset,
    /// The 3D view and the top, front and side views, two by two.
    Quad,
}

impl MapPanes {
    /// Every layout, in the toolbar's order.
    pub const ALL: [Self; 3] = [Self::Single, Self::Inset, Self::Quad];
}

/// The parameters of `view.map_panes`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapPanesParams {
    /// The layout.
    pub panes: MapPanes,
}

/// `view.map_panes`.
struct SetMapPanes;

impl<C: HasLayout + 'static> Command<C> for SetMapPanes {
    const ID: &'static str = MAP_PANES;
    const SUMMARY: &'static str = "Splits the map view: the 3D view alone, with a top view in \
        its corner, or beside top, front and side views.";
    type Params = MapPanesParams;
    type Output = MapPanes;
    type Error = Infallible;

    fn run(
        &self,
        context: &mut C,
        params: MapPanesParams,
    ) -> Result<Outcome<MapPanes, C>, Infallible> {
        let layout = context.layout_mut();
        layout.map_panes = params.panes;
        Ok(Outcome::outside_history(layout.map_panes))
    }
}

/// Registers `view.map_panes`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(SetMapPanes)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{MAP_PANES, MapPanes};
    use crate::Layout;

    // No spec statement: P6-09: view.map_panes sets the map view's layout outside the undo
    // history, the layout file keeps it, and an older file without it reads as the 3D view alone
    #[test]
    fn the_map_view_s_panes_are_set_and_kept() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new(Layout::default(), registry);
        session
            .execute(MAP_PANES, json!({ "panes": "quad" }))
            .unwrap();
        assert_eq!(session.context().map_panes, MapPanes::Quad);
        assert!(
            session.undo().is_err(),
            "a layout change is outside the history"
        );
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.map_panes, MapPanes::Quad);
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.map_panes, MapPanes::Single);
        assert!(
            session
                .execute(MAP_PANES, json!({ "panes": "eight" }))
                .is_err()
        );
    }
}
