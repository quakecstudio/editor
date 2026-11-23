// SPDX-License-Identifier: GPL-3.0-or-later

//! The passage between two rooms: a space from one's wall to the other's, square or under an
//! arch whose corners are cut at half a right angle, with stairs where the floors differ and
//! a door at its higher end when it has one.

use qs_map::{Brush, BrushError};

use crate::layout::{Doorway, Link, Plan, Side};
use crate::mass::{Bounds, Coat, Mass, Point};
use crate::theme::Paints;

/// Half a passage's width: a large monster's box is 64 wide.
const HALF_WIDTH: f64 = 56.0;
/// A passage's height above its higher floor.
const HEIGHT: f64 = 128.0;
/// How much of an arch's upper corners is cut.
const ARCH_CUT: f64 = 32.0;
/// A step's height, which a walking player climbs (PHYS-8.3), and the depths a step may
/// have, the easiest first.
const STEP_RISE: f64 = 16.0;
const STEP_DEPTHS: [f64; 3] = [32.0, 24.0, 16.0];
/// The level floor a passage keeps at each end, before its stairs.
const LANDING: f64 = 32.0;
/// Where a door stands from the end of its passage, and how thick it is.
const DOOR_FROM_END: f64 = 8.0;
const DOOR_THICK: f64 = 16.0;
/// How bright a passage's light is, and how far under its ceiling it hangs.
const LIGHT: f64 = 200.0;
const LIGHT_DROP: f64 = 24.0;

/// A flight of steps in a passage.
#[derive(Clone, Copy, Debug)]
struct Flight {
    /// A step's depth.
    depth: f64,
    /// How far the first step's edge is from the passage's lower end.
    edge: f64,
}

/// A passage being built.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Passage {
    /// Where it leaves its first room: the middle of that room's wall, at height 0.
    start: [f64; 2],
    /// The side of the first room it leaves by.
    side: Side,
    /// How long it is.
    length: f64,
    /// The floor at its start and at its end.
    floors: [f64; 2],
    /// What stands in it.
    doorway: Doorway,
}

/// What a passage adds to the level besides its space.
#[derive(Clone, Debug, Default)]
pub(crate) struct Passed {
    /// Solid brushes of the world: its steps.
    pub(crate) solids: Vec<Brush>,
    /// Its door, with whether it needs the key.
    pub(crate) door: Option<(Brush, Doorway)>,
    /// Its light.
    pub(crate) light: Option<(Point, f64)>,
}

impl Passage {
    /// The passage of `link` in `plan`; `None` for a link to a room that is not there.
    pub(crate) fn of(plan: &Plan, link: &Link) -> Option<Self> {
        let from = plan.room_of(link.from)?;
        let to = plan.room_of(link.to)?;
        let [fx, fy] = link.side.facing();
        let [ax, ay] = from.middle;
        let [bx, by] = to.middle;
        let between = (bx - ax) * fx + (by - ay) * fy;
        let reach = from.reach(link.side);
        Some(Self {
            start: [ax + fx * reach, ay + fy * reach],
            side: link.side,
            length: between - reach - to.reach(link.side.opposite()),
            floors: [
                from.door_floor(link.side),
                to.door_floor(link.side.opposite()),
            ],
            doorway: link.doorway,
        })
    }

    /// The passage from `start`, leaving by `side`, `length` long, between the floors
    /// `floors` at its two ends, with `doorway` in it.
    pub(crate) const fn new(
        start: [f64; 2],
        side: Side,
        length: f64,
        floors: [f64; 2],
        doorway: Doorway,
    ) -> Self {
        Self {
            start,
            side,
            length,
            floors,
            doorway,
        }
    }

    /// The steps it needs, and the least length that holds them with a landing at each end.
    pub(crate) fn least_length(climb: f64) -> f64 {
        (climb.abs() / STEP_RISE) * STEP_RISE + LANDING + LANDING
    }

    /// The rock a passage alone is taken out of: its space with `wall` around it, above it
    /// and below it, and none at its ends, which open onto what it joins.
    pub(crate) fn shell(&self, wall: f64) -> Bounds {
        let [low, high] = self.low_high();
        self.bounds(
            [0.0, self.length],
            [-HALF_WIDTH - wall, HALF_WIDTH + wall],
            [low - wall, high + HEIGHT + wall],
        )
    }

    /// The box from `along[0]` to `along[1]` down the passage, `over` across it, between the
    /// heights `z`.
    fn bounds(&self, along: [f64; 2], over: [f64; 2], z: [f64; 2]) -> Bounds {
        let ([a0, a1], [o0, o1], [z0, z1]) = (along, over, z);
        let [a, b] = [self.point(a0, o0, z0), self.point(a1, o1, z1)];
        let mut min = a;
        let mut max = a;
        for ((low, high), at) in min.iter_mut().zip(&mut max).zip(b) {
            *low = low.min(at);
            *high = high.max(at);
        }
        Bounds { min, max }
    }

    /// The point `along` down the passage and `over` across it, at height `z`.
    fn point(&self, along: f64, over: f64, z: f64) -> Point {
        let [fx, fy] = self.side.facing();
        let [sx, sy] = self.start;
        // Across is a quarter turn from along.
        [sx + fx * along - fy * over, sy + fy * along + fx * over, z]
    }

    /// The lower and the higher of its two floors.
    fn low_high(&self) -> [f64; 2] {
        let [a, b] = self.floors;
        [a.min(b), a.max(b)]
    }

    /// The flight of steps between its floors: how many, how deep each is, and how far its
    /// first edge is from the passage's lower end; `None` for a level passage.
    fn flight(&self) -> Option<Flight> {
        let [low, high] = self.low_high();
        let climb = high - low;
        if climb <= 0.0 {
            return None;
        }
        let count = climb / STEP_RISE;
        let room = self.length - LANDING - LANDING;
        let depth = STEP_DEPTHS
            .into_iter()
            .find(|depth| depth * count <= room)
            .unwrap_or(STEP_RISE);
        Some(Flight {
            depth,
            edge: (self.length - depth * count) / 2.0,
        })
    }

    /// Whether its floor rises from its first end to its second.
    fn rises_along(&self) -> bool {
        let [first, second] = self.floors;
        first < second
    }

    /// The part of the passage's space from `along[0]` to `along[1]` between the heights `z`:
    /// a box, or under an arch its upper corners cut.
    fn section(
        &self,
        paints: &Paints,
        along: [f64; 2],
        [floor, top]: [f64; 2],
    ) -> Result<Brush, BrushError> {
        let space = Coat::space(&paints.floor, &paints.ceiling, &paints.trim);
        match self.doorway {
            Doorway::Arch => {
                let mut corners = Vec::new();
                for at in along {
                    for (over, z) in [
                        (-HALF_WIDTH, floor),
                        (HALF_WIDTH, floor),
                        (HALF_WIDTH, top - ARCH_CUT),
                        (HALF_WIDTH - ARCH_CUT, top),
                        (-HALF_WIDTH + ARCH_CUT, top),
                        (-HALF_WIDTH, top - ARCH_CUT),
                    ] {
                        corners.push(self.point(at, over, z));
                    }
                }
                space.hull(&corners)
            }
            Doorway::Open
            | Doorway::Door
            | Doorway::Locked(_)
            | Doorway::Remote(_)
            | Doorway::Secret => {
                space.boxed(self.bounds(along, [-HALF_WIDTH, HALF_WIDTH], [floor, top]))
            }
        }
    }

    /// Takes the passage out of `mass` and builds its steps and its door. Its space is a
    /// doorway's height above the floor at each end: up to its first step it is that high
    /// above its lower floor, and from there on that high above its higher one, so each end
    /// is the doorway of the room it meets and no higher.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a brush that could not be made.
    pub(crate) fn build(&self, mass: &mut Mass, paints: &Paints) -> Result<Passed, BrushError> {
        let [low, high] = self.low_high();
        let top = high + HEIGHT;
        let across = [-HALF_WIDTH, HALF_WIDTH];
        match self.flight() {
            None => mass.carve(&self.section(paints, [0.0, self.length], [low, top])?)?,
            Some(flight) => {
                // The landing before the first step, at the lower end, and the rest.
                let (landing, rest) = if self.rises_along() {
                    ([0.0, flight.edge], [flight.edge, self.length])
                } else {
                    let from = self.length - flight.edge;
                    ([from, self.length], [0.0, from])
                };
                mass.carve(&self.section(paints, landing, [low, low + HEIGHT])?)?;
                mass.carve(&self.section(paints, rest, [low, top])?)?;
            }
        }
        let mut passed = Passed {
            light: Some((self.point(self.length / 2.0, 0.0, top - LIGHT_DROP), LIGHT)),
            ..Passed::default()
        };
        self.steps(paints, &mut passed.solids)?;
        // The door stands at the higher end, where the passage is no taller than it; a secret
        // door at the first end, flush with the wall of the room it is hidden in.
        let door_at = if self.doorway == Doorway::Secret {
            [0.0, DOOR_THICK]
        } else if self.rises_along() {
            [
                self.length - DOOR_FROM_END - DOOR_THICK,
                self.length - DOOR_FROM_END,
            ]
        } else {
            [DOOR_FROM_END, DOOR_FROM_END + DOOR_THICK]
        };
        match self.doorway {
            Doorway::Open | Doorway::Arch => {}
            Doorway::Door | Doorway::Locked(_) | Doorway::Remote(_) => {
                let door =
                    Coat::all(&paints.door).boxed(self.bounds(door_at, across, [high, top]))?;
                passed.door = Some((door, self.doorway));
            }
            Doorway::Secret => {
                // Painted as the wall it stands in, from the first end's floor.
                let [first, _] = self.floors;
                let door = Coat::all(&paints.wall).boxed(self.bounds(
                    door_at,
                    across,
                    [first, first + HEIGHT],
                ))?;
                passed.door = Some((door, self.doorway));
            }
        }
        Ok(passed)
    }

    /// The steps from its lower floor to its higher, each a slab from its own edge to the
    /// higher end.
    fn steps(&self, paints: &Paints, solids: &mut Vec<Brush>) -> Result<(), BrushError> {
        let Some(flight) = self.flight() else {
            return Ok(());
        };
        let [low, high] = self.low_high();
        let step = Coat::deck(&paints.floor, &paints.trim);
        let mut edge = flight.edge;
        let mut foot = low;
        while foot < high {
            let along = if self.rises_along() {
                [edge, self.length]
            } else {
                [0.0, self.length - edge]
            };
            solids.push(step.boxed(self.bounds(
                along,
                [-HALF_WIDTH, HALF_WIDTH],
                [foot, foot + STEP_RISE],
            ))?);
            edge += flight.depth;
            foot += STEP_RISE;
        }
        Ok(())
    }
}
