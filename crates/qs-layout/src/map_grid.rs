// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's grid (P7-08, MAP-30): the step brushes, entities and the selection snap to,
//! a power of two from 0.125 to 256 units, as TrenchBroom's grid is ("The grid size can be 1, 2,
//! 4, 8, 16, etc. up to 256. It is also possible to set the grid size to a value smaller than
//! 1, more precisely to 0.5, 0.25 or 0.125", its manual, "The Grid"). A command sets it, so the
//! keys, the menu, the CLI and the agent share it, and the layout file keeps it.

use core::fmt;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command that sets the map editor's grid.
pub const MAP_GRID: &str = "view.map_grid";

/// The id of the command that halves or doubles the map editor's grid.
pub const MAP_GRID_STEP: &str = "view.map_grid_step";

/// The map editor's grid: 2 to the power of its exponent, in units.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "f64", into = "f64")]
pub struct MapGrid(i8);

impl MapGrid {
    /// The finest grid, 0.125 units: TrenchBroom's "Set Grid Size 0.125", `setGridSize(-3)`.
    pub const FINEST: Self = Self(-3);
    /// The coarsest grid, 256 units: TrenchBroom's "Set Grid Size 256", `setGridSize(8)`.
    pub const COARSEST: Self = Self(8);
    /// 16 units, TrenchBroom's default (`Grid(4)` in its `Map.cpp`).
    pub const DEFAULT: Self = Self(4);

    /// The grid's step in units.
    #[must_use]
    pub fn units(self) -> f64 {
        f64::from(self.0).exp2()
    }

    /// Half the step, or this one at the finest.
    #[must_use]
    pub fn finer(self) -> Self {
        Self(self.0.saturating_sub(1).max(Self::FINEST.0))
    }

    /// Twice the step, or this one at the coarsest.
    #[must_use]
    pub fn coarser(self) -> Self {
        Self(self.0.saturating_add(1).min(Self::COARSEST.0))
    }

    /// `value` on the grid: the nearest multiple of its step.
    #[must_use]
    pub fn snap(self, value: f64) -> f64 {
        (value / self.units()).round() * self.units()
    }

    /// Every grid, finest first, as the View menu lists them.
    pub fn all() -> impl Iterator<Item = Self> {
        (Self::FINEST.0..=Self::COARSEST.0).map(Self)
    }
}

impl Default for MapGrid {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for MapGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.units())
    }
}

/// A grid that is not a power of two from 0.125 to 256 units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapGridError(f64);

impl fmt::Display for MapGridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a grid of {} units is not a power of two from {} to {}",
            self.0,
            MapGrid::FINEST,
            MapGrid::COARSEST
        )
    }
}

impl std::error::Error for MapGridError {}

impl TryFrom<f64> for MapGrid {
    type Error = MapGridError;

    fn try_from(units: f64) -> Result<Self, MapGridError> {
        Self::all()
            .find(|grid| grid.units().to_bits() == units.to_bits())
            .ok_or(MapGridError(units))
    }
}

impl From<MapGrid> for f64 {
    fn from(grid: MapGrid) -> Self {
        grid.units()
    }
}

/// Which way `view.map_grid_step` steps the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridStep {
    /// Half the step, as TrenchBroom's Decrease Grid Size.
    Finer,
    /// Twice the step, as TrenchBroom's Increase Grid Size.
    Coarser,
}

/// The parameters of `view.map_grid`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapGridParams {
    /// The grid in units: a power of two from 0.125 to 256.
    pub size: MapGrid,
}

/// The parameters of `view.map_grid_step`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapGridStepParams {
    /// Which way.
    pub step: GridStep,
}

/// `view.map_grid`.
struct SetMapGrid;

impl<C: HasLayout + 'static> Command<C> for SetMapGrid {
    const ID: &'static str = MAP_GRID;
    const SUMMARY: &'static str = "Sets the map editor's grid in units, a power of two from \
        0.125 to 256.";
    type Params = MapGridParams;
    type Output = MapGrid;
    type Error = !;

    fn run(&self, context: &mut C, params: MapGridParams) -> Result<Outcome<MapGrid, C>, !> {
        let layout = context.layout_mut();
        layout.map_grid = params.size;
        Ok(Outcome::outside_history(layout.map_grid))
    }
}

/// `view.map_grid_step`.
struct StepMapGrid;

impl<C: HasLayout + 'static> Command<C> for StepMapGrid {
    const ID: &'static str = MAP_GRID_STEP;
    const SUMMARY: &'static str = "Halves or doubles the map editor's grid, held to 0.125 to \
        256 units.";
    type Params = MapGridStepParams;
    type Output = MapGrid;
    type Error = !;

    fn run(&self, context: &mut C, params: MapGridStepParams) -> Result<Outcome<MapGrid, C>, !> {
        let layout = context.layout_mut();
        layout.map_grid = match params.step {
            GridStep::Finer => layout.map_grid.finer(),
            GridStep::Coarser => layout.map_grid.coarser(),
        };
        Ok(Outcome::outside_history(layout.map_grid))
    }
}

/// Registers `view.map_grid` and `view.map_grid_step`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(SetMapGrid)?;
    registry.register(StepMapGrid)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test; powers of two are exact"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{MAP_GRID, MAP_GRID_STEP, MapGrid};
    use crate::Layout;

    // No spec statement: P7-08: the grid is a power of two from 0.125 to 256 units, 16 by
    // default, as TrenchBroom's; a step holds at either end
    #[test]
    fn the_grid_steps_by_powers_of_two_within_its_range() {
        assert_eq!(MapGrid::DEFAULT.units(), 16.0);
        assert_eq!(MapGrid::FINEST.units(), 0.125);
        assert_eq!(MapGrid::COARSEST.units(), 256.0);
        assert_eq!(MapGrid::DEFAULT.finer().units(), 8.0);
        assert_eq!(MapGrid::DEFAULT.coarser().units(), 32.0);
        assert_eq!(MapGrid::FINEST.finer(), MapGrid::FINEST);
        assert_eq!(MapGrid::COARSEST.coarser(), MapGrid::COARSEST);
        assert_eq!(MapGrid::all().count(), 12);
        assert_eq!(MapGrid::all().next(), Some(MapGrid::FINEST));
        assert_eq!(MapGrid::all().last(), Some(MapGrid::COARSEST));
        assert_eq!(MapGrid::try_from(0.5).unwrap().units(), 0.5);
        for bad in [0.0, 0.0625, 3.0, 512.0, -16.0, f64::NAN] {
            assert!(MapGrid::try_from(bad).is_err(), "{bad}");
        }
        assert_eq!(MapGrid::FINEST.to_string(), "0.125");
        assert_eq!(MapGrid::DEFAULT.snap(23.0), 16.0);
        assert_eq!(MapGrid::DEFAULT.snap(25.0), 32.0);
        assert_eq!(MapGrid::DEFAULT.snap(-9.0), -16.0);
        assert_eq!(MapGrid::FINEST.snap(0.3), 0.25);
    }

    // No spec statement: P7-08: view.map_grid sets a size and view.map_grid_step steps it,
    // outside the undo history; the layout file keeps it, an older file reads as 16, and a bad
    // size or step is refused
    #[test]
    fn the_grid_is_set_stepped_and_kept() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new(Layout::default(), registry);
        session.execute(MAP_GRID, json!({ "size": 64 })).unwrap();
        assert_eq!(session.context().map_grid.units(), 64.0);
        session
            .execute(MAP_GRID_STEP, json!({ "step": "finer" }))
            .unwrap();
        assert_eq!(session.context().map_grid.units(), 32.0);
        session
            .execute(MAP_GRID_STEP, json!({ "step": "coarser" }))
            .unwrap();
        assert_eq!(session.context().map_grid.units(), 64.0);
        assert!(session.undo().is_err());
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.map_grid.units(), 64.0);
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.map_grid, MapGrid::DEFAULT);
        assert!(toml::from_str::<Layout>("map_grid = 3.0\n").is_err());
        for bad in [json!({ "size": 3 }), json!({ "size": "fine" }), json!({})] {
            assert!(session.execute(MAP_GRID, bad.clone()).is_err(), "{bad}");
        }
        assert!(
            session
                .execute(MAP_GRID_STEP, json!({ "step": "sideways" }))
                .is_err()
        );
    }
}
