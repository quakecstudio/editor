// SPDX-License-Identifier: GPL-3.0-or-later

//! The shapes of rooms, each a space taken out of the rock and what is built in it. They are
//! the stock of id's levels, built from faces that are level, upright or at half a right
//! angle: a hall with pillars under a raised ceiling, an eight-sided chamber under a stepped
//! dome, a hall around a pit of liquid crossed by bridges, a room on two levels joined by
//! stairs, a courtyard under the sky, a cross of two aisles under a lantern, and a long room
//! under a pitched vault.

use qs_map::{Brush, BrushError};

use crate::RoomShape;
use crate::layout::{RAISE, RoomPlan, Side};
use crate::mass::{Bounds, Coat, Mass, Point};
use crate::theme::{Hazard, Paints};

/// How far from a wall something stands: room for a large monster's box, 64 wide, and to
/// spare (`docs/agent-reference/maps.md`, Units and the player).
const SPOT_INSET: f64 = 48.0;
/// How far an entity's origin is above the floor it stands on: the player's and a monster's
/// box reach 24 below their origin.
pub(crate) const STAND: f64 = 24.0;

/// A hall's pillars stand this far in from its walls.
const PILLAR_INSET: f64 = 128.0;
/// Half a pillar's width, and how much of that its corners are cut by.
const PILLAR_HALF: f64 = 24.0;
const PILLAR_CUT: f64 = 8.0;
/// Half the width of a pillar's foot and head, and their height.
const PLINTH_HALF: f64 = 32.0;
const PLINTH_HEIGHT: f64 = 8.0;
/// How far in from a hall's walls its raised ceiling starts, and how much higher it is.
const COFFER_INSET: f64 = 64.0;
const COFFER_RISE: f64 = 48.0;

/// How much of a chamber's corner is cut.
const CHAMBER_CUT: f64 = 96.0;
/// A dome's steps: how far in each is from the one below, how much less its corner is cut,
/// how high it is, and how many there are.
const DOME_INSET: f64 = 48.0;
const DOME_CUT_LESS: f64 = 24.0;
const DOME_RISE: f64 = 40.0;
const DOME_STEPS: usize = 2;
/// How far from a chamber's middle, in x and in y, something stands between its dais and its
/// cut corners.
const CHAMBER_DIAGONAL: f64 = 120.0;
/// A dais's two steps: half the width and the cut of each, and a step's height, which a
/// walking player climbs (PHYS-8.3).
const DAIS_HALVES: [f64; 2] = [72.0, 48.0];
const DAIS_CUTS: [f64; 2] = [24.0, 16.0];
const DAIS_STEP: f64 = 16.0;

/// How wide the walk around a pit is, and how deep the pit and its liquid are: the liquid's
/// surface is [`PIT_DEPTH`] less [`LIQUID_DEPTH`] under the rim, which a swimmer climbs out
/// over ([`crate::CLIMB_OUT`]).
const PIT_RIM: f64 = 96.0;
const PIT_DEPTH: f64 = 96.0;
const LIQUID_DEPTH: f64 = 72.0;
const _: () = assert!(
    PIT_DEPTH > LIQUID_DEPTH && PIT_DEPTH - LIQUID_DEPTH <= crate::CLIMB_OUT,
    "a pit's liquid is under its rim, near enough to climb out"
);
/// A rail along the edge of a pit of lava and along its bridges: higher than a walking
/// player climbs (`STEPSIZE`, PHYS-8.3), so no one falls in who does not jump; and how thick.
const RAIL_HEIGHT: f64 = 32.0;
const RAIL_THICK: f64 = 8.0;
/// Half a bridge's width, and its thickness.
const BRIDGE_HALF: f64 = 48.0;
const BRIDGE_THICK: f64 = 16.0;

/// Where the raised part of a room on two levels starts, from the room's middle toward its
/// raised side; the stairs run up to it from as far the other way.
const SPLIT_EDGE: f64 = 64.0;
/// Half the stairs' width, a step's depth and its height.
const STAIR_HALF: f64 = 64.0;
const STAIR_DEPTH: f64 = 32.0;
const STAIR_RISE: f64 = 16.0;
/// How far toward its low side a room on two levels is stood in.
const SPLIT_STAND: f64 = 128.0;

/// A courtyard's obelisk: half its foot, half the widths of its shaft's ends, and heights.
const OBELISK_FOOT: f64 = 48.0;
const OBELISK_BASE: f64 = 32.0;
const OBELISK_TIP: f64 = 16.0;
const OBELISK_FOOT_HEIGHT: f64 = 16.0;
const OBELISK_HEIGHT: f64 = 176.0;

/// Half the width of a cross's aisles, how much lower the shorter aisle's ceiling is, and
/// how much higher the lantern over their crossing.
const AISLE_HALF: f64 = 128.0;
const AISLE_DROP: f64 = 48.0;
const LANTERN_RISE: f64 = 64.0;
/// How far from an aisle's middle line something stands in it.
const AISLE_SPOT: f64 = 72.0;

/// How much of a vault's upper corners its pitch cuts.
const VAULT_CUT: f64 = 96.0;

/// A pilaster: half its width, how far it stands out of its wall, and how far under the
/// ceiling it stops.
const PILASTER_HALF: f64 = 16.0;
const PILASTER_DEPTH: f64 = 16.0;
/// A beam under a ceiling: half its width and its depth.
const BEAM_HALF: f64 = 16.0;
const BEAM_DEPTH: f64 = 24.0;
/// How far above the floor a torch burns, and how far out of its pilaster.
const TORCH_LIFT: f64 = 96.0;
const TORCH_OUT: f64 = 12.0;

/// What a room is to the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// A room on the way.
    Way,
    /// The room the level ends in: the exit takes the place of a chamber's dais.
    Exit,
}

/// A room being built.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Room<'a> {
    plan: &'a RoomPlan,
    part: Part,
}

/// What a room adds to the level besides the space it takes out.
#[derive(Clone, Debug, Default)]
pub(crate) struct Built {
    /// Solid brushes of the world.
    pub(crate) solids: Vec<Brush>,
    /// Where things may stand, each the origin of a box standing on the floor there.
    pub(crate) spots: Vec<Point>,
    /// Where lights hang, each with how bright it is.
    pub(crate) lights: Vec<(Point, f64)>,
    /// Where torches burn on its walls.
    pub(crate) torches: Vec<Point>,
}

/// How bright a room's main light and its lesser lights are (`light`'s default is 300,
/// `DEFAULTLIGHTLEVEL`).
const MAIN_LIGHT: f64 = 450.0;
const LESSER_LIGHT: f64 = 300.0;
const GLOW_LIGHT: f64 = 200.0;
/// How far under the ceiling the main light hangs, how far above the floor the lesser ones,
/// and how far from the walls those are.
const LIGHT_DROP: f64 = 40.0;
const LIGHT_LIFT: f64 = 112.0;
const LIGHT_INSET: f64 = 72.0;
/// How far above its liquid a pit's glow is.
const GLOW_LIFT: f64 = 24.0;

/// The eight corners of a box of half-widths `half` around `at` with its corners cut by
/// `cut`.
fn eight_sided([x, y]: [f64; 2], [hx, hy]: [f64; 2], cut: f64) -> [[f64; 2]; 8] {
    [
        [x + hx, y - hy + cut],
        [x + hx, y + hy - cut],
        [x + hx - cut, y + hy],
        [x - hx + cut, y + hy],
        [x - hx, y + hy - cut],
        [x - hx, y - hy + cut],
        [x - hx + cut, y - hy],
        [x + hx - cut, y - hy],
    ]
}

impl<'a> Room<'a> {
    /// The room of `plan`.
    pub(crate) const fn new(plan: &'a RoomPlan, part: Part) -> Self {
        Self { plan, part }
    }

    /// How far below its floor and above its ceiling the room's space reaches: a pit, a
    /// raised ceiling, a dome, a lantern.
    pub(crate) fn beyond(&self) -> [f64; 2] {
        match self.plan.shape {
            RoomShape::Hall => [0.0, COFFER_RISE],
            RoomShape::Chamber => [0.0, DOME_RISE + DOME_RISE],
            RoomShape::Pit => [PIT_DEPTH, 0.0],
            RoomShape::Cross => [0.0, LANTERN_RISE],
            RoomShape::Split | RoomShape::Court | RoomShape::Vault => [0.0, 0.0],
        }
    }

    /// The room's middle on the map.
    fn middle(&self) -> [f64; 2] {
        self.plan.middle
    }

    /// The box `x` by `y` about the room's middle, between the heights `z`.
    fn bounds(&self, x: [f64; 2], y: [f64; 2], z: [f64; 2]) -> Bounds {
        let [mx, my] = self.middle();
        let ([x0, x1], [y0, y1], [z0, z1]) = (x, y, z);
        Bounds {
            min: [mx + x0, my + y0, z0],
            max: [mx + x1, my + y1, z1],
        }
    }

    /// The box of half-widths `half` about the room's middle, between the heights `z`.
    fn centred(&self, [hx, hy]: [f64; 2], z: [f64; 2]) -> Bounds {
        self.bounds([-hx, hx], [-hy, hy], z)
    }

    /// The point `[x, y]` from the room's middle, at height `z`.
    fn at(&self, [x, y]: [f64; 2], z: f64) -> Point {
        let [mx, my] = self.middle();
        [mx + x, my + y, z]
    }

    /// Where a box stands at `offset` from the room's middle on a floor at `floor`.
    fn standing(&self, offset: [f64; 2], floor: f64) -> Point {
        self.at(offset, floor + STAND)
    }

    /// The room's corners and the middles of its sides, `inset` in from its walls.
    fn around(&self, inset: f64) -> ([[f64; 2]; 4], [[f64; 2]; 4]) {
        let [hx, hy] = self.plan.half;
        let (x, y) = (hx - inset, hy - inset);
        (
            [[x, y], [-x, y], [-x, -y], [x, -y]],
            [[x, 0.0], [0.0, y], [-x, 0.0], [0.0, -y]],
        )
    }

    /// How far the room reaches toward the side it is turned to, and across that.
    fn turned(&self) -> (f64, f64) {
        let [hx, hy] = self.plan.half;
        match self.plan.turned {
            Side::East | Side::West => (hx, hy),
            Side::North | Side::South => (hy, hx),
        }
    }

    /// The offset from the room's middle that is `along` toward the side it is turned to and
    /// `over` across that.
    fn turned_offset(&self, along: f64, over: f64) -> [f64; 2] {
        match self.plan.turned {
            Side::East => [along, over],
            Side::West => [-along, over],
            Side::North => [over, along],
            Side::South => [over, -along],
        }
    }

    /// The box from `along[0]` to `along[1]` toward the side the room is turned to, `over`
    /// across that, between the heights `z`.
    fn turned_bounds(&self, along: [f64; 2], over: [f64; 2], z: [f64; 2]) -> Bounds {
        let [a0, a1] = along;
        match self.plan.turned {
            Side::East => self.bounds([a0, a1], over, z),
            Side::West => self.bounds([-a1, -a0], over, z),
            Side::North => self.bounds(over, [a0, a1], z),
            Side::South => self.bounds(over, [-a1, -a0], z),
        }
    }

    /// Where the player, the key or the exit stands in the room: on its floor, clear of what
    /// is built in it.
    pub(crate) fn stand(&self) -> Point {
        let floor = self.plan.floor;
        let [hx, hy] = self.plan.half;
        match (self.plan.shape, self.part) {
            (RoomShape::Hall | RoomShape::Pit | RoomShape::Cross | RoomShape::Vault, _)
            | (RoomShape::Chamber, Part::Exit) => self.standing([0.0, 0.0], floor),
            (RoomShape::Chamber, Part::Way) => {
                self.standing([0.0, 0.0], floor + DAIS_STEP + DAIS_STEP)
            }
            (RoomShape::Split, _) => self.standing(self.turned_offset(-SPLIT_STAND, 0.0), floor),
            (RoomShape::Court, _) => self.standing([hx / 2.0, hy / 2.0], floor),
        }
    }

    /// Takes the room out of `mass` and builds what stands in it.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a brush that could not be made.
    pub(crate) fn build(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        match self.plan.shape {
            RoomShape::Hall => self.hall(mass, paints),
            RoomShape::Chamber => self.chamber(mass, paints),
            RoomShape::Pit => self.pit(mass, paints),
            RoomShape::Split => self.split(mass, paints),
            RoomShape::Court => self.court(mass, paints),
            RoomShape::Cross => self.cross(mass, paints),
            RoomShape::Vault => self.vault(mass, paints),
        }
    }

    /// The plain space of the room, floor to ceiling.
    fn space(&self, paints: &Paints) -> Result<Brush, BrushError> {
        let floor = self.plan.floor;
        Coat::space(&paints.floor, &paints.ceiling, &paints.wall)
            .boxed(self.centred(self.plan.half, [floor, floor + self.plan.height]))
    }

    /// Pilasters on the room's four walls, two to a wall and clear of its doorway, from
    /// `foot` to `head`, a torch on each.
    fn pilasters(
        &self,
        paints: &Paints,
        [foot, head]: [f64; 2],
        built: &mut Built,
    ) -> Result<(), BrushError> {
        let trim = Coat::all(&paints.trim);
        let [hx, hy] = self.plan.half;
        for side in Side::ALL {
            let [fx, fy] = side.facing();
            // Along the wall is a quarter turn from the way it faces.
            let (reach, width) = if fx.abs() > fy.abs() {
                (hx, hy)
            } else {
                (hy, hx)
            };
            for over in [-width / 2.0, width / 2.0] {
                let place =
                    |out: f64, across: f64| [fx * out - fy * across, fy * out + fx * across];
                let [ax, ay] = place(reach - PILASTER_DEPTH, over - PILASTER_HALF);
                let [bx, by] = place(reach, over + PILASTER_HALF);
                built.solids.push(trim.boxed(self.bounds(
                    [ax.min(bx), ax.max(bx)],
                    [ay.min(by), ay.max(by)],
                    [foot, head],
                ))?);
                built.torches.push(self.at(
                    place(reach - PILASTER_DEPTH - TORCH_OUT, over),
                    foot + TORCH_LIFT,
                ));
            }
        }
        Ok(())
    }

    /// Two beams under a ceiling at `top`, across the side the room is turned to.
    fn beams(&self, paints: &Paints, top: f64, built: &mut Built) -> Result<(), BrushError> {
        let trim = Coat::all(&paints.trim);
        let (reach, across) = self.turned();
        for along in [-reach / 2.0, reach / 2.0] {
            built.solids.push(trim.boxed(self.turned_bounds(
                [along - BEAM_HALF, along + BEAM_HALF],
                [-across, across],
                [top - BEAM_DEPTH, top],
            ))?);
        }
        Ok(())
    }

    /// A hall: pillars in from its corners, under a ceiling raised in its middle.
    fn hall(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let [hx, hy] = self.plan.half;
        mass.carve(&self.space(paints)?)?;
        let coffer =
            Coat::space(&paints.floor, &paints.ceiling, &paints.trim).boxed(self.centred(
                [hx - COFFER_INSET, hy - COFFER_INSET],
                [top, top + COFFER_RISE],
            ))?;
        mass.carve(&coffer)?;
        let mut built = Built::default();
        let trim = Coat::all(&paints.trim);
        let (posts, _) = self.around(PILLAR_INSET);
        for [x, y] in posts {
            let [mx, my] = self.middle();
            built.solids.push(trim.prism(
                &eight_sided([mx + x, my + y], [PILLAR_HALF, PILLAR_HALF], PILLAR_CUT),
                [floor + PLINTH_HEIGHT, top - PLINTH_HEIGHT],
            )?);
            let plinth = |z: [f64; 2]| {
                trim.boxed(self.bounds(
                    [x - PLINTH_HALF, x + PLINTH_HALF],
                    [y - PLINTH_HALF, y + PLINTH_HALF],
                    z,
                ))
            };
            built.solids.push(plinth([floor, floor + PLINTH_HEIGHT])?);
            built
                .solids
                .push(plinth([top - PLINTH_HEIGHT, top + COFFER_RISE])?);
        }
        self.pilasters(paints, [floor, top], &mut built)?;
        let (corners, sides) = self.around(SPOT_INSET);
        for offset in corners.into_iter().chain(sides) {
            built.spots.push(self.standing(offset, floor));
        }
        built.lights.push((
            self.at([0.0, 0.0], top + COFFER_RISE - LIGHT_DROP),
            MAIN_LIGHT,
        ));
        Ok(built)
    }

    /// A chamber: eight walls under a dome of two steps, a dais in its middle unless the
    /// exit is there.
    fn chamber(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let [hx, hy] = self.plan.half;
        let wall = Coat::space(&paints.floor, &paints.ceiling, &paints.wall);
        mass.carve(&wall.prism(
            &eight_sided(self.middle(), [hx, hy], CHAMBER_CUT),
            [floor, top],
        )?)?;
        let ring = Coat::space(&paints.floor, &paints.ceiling, &paints.trim);
        let mut low = top;
        let (mut half, mut cut) = ([hx, hy], CHAMBER_CUT);
        for _ in 0..DOME_STEPS {
            half = half.map(|h| h - DOME_INSET);
            cut -= DOME_CUT_LESS;
            mass.carve(&ring.prism(
                &eight_sided(self.middle(), half, cut),
                [low, low + DOME_RISE],
            )?)?;
            low += DOME_RISE;
        }
        let mut built = Built::default();
        match self.part {
            Part::Exit => {}
            Part::Way => {
                let step = Coat::deck(&paints.floor, &paints.trim);
                let mut foot = floor;
                for (half, cut) in DAIS_HALVES.into_iter().zip(DAIS_CUTS) {
                    built.solids.push(step.prism(
                        &eight_sided(self.middle(), [half, half], cut),
                        [foot, foot + DAIS_STEP],
                    )?);
                    foot += DAIS_STEP;
                }
            }
        }
        let (_, sides) = self.around(SPOT_INSET);
        let d = CHAMBER_DIAGONAL;
        for offset in sides
            .into_iter()
            .chain([[d, d], [-d, d], [-d, -d], [d, -d]])
        {
            built.spots.push(self.standing(offset, floor));
        }
        built
            .lights
            .push((self.at([0.0, 0.0], low - LIGHT_DROP), MAIN_LIGHT));
        let (_, lesser) = self.around(LIGHT_INSET);
        for offset in lesser {
            built
                .lights
                .push((self.at(offset, floor + LIGHT_LIFT), LESSER_LIGHT));
        }
        Ok(built)
    }

    /// A hall around a pit of liquid, crossed by two bridges that meet in its middle.
    fn pit(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let [hx, hy] = self.plan.half;
        let (px, py) = (hx - PIT_RIM, hy - PIT_RIM);
        mass.carve(&self.space(paints)?)?;
        let pit = Coat::space(&paints.wall, &paints.ceiling, &paints.wall);
        mass.carve(&pit.boxed(self.centred([px, py], [floor - PIT_DEPTH, floor]))?)?;
        let mut built = Built::default();
        if let Some(liquid) = &paints.liquid {
            built.solids.push(Coat::all(liquid).boxed(self.centred(
                [px, py],
                [floor - PIT_DEPTH, floor - PIT_DEPTH + LIQUID_DEPTH],
            ))?);
        }
        let deck = Coat::deck(&paints.floor, &paints.trim);
        let z = [floor - BRIDGE_THICK, floor];
        let across = [-BRIDGE_HALF, BRIDGE_HALF];
        built
            .solids
            .push(deck.boxed(self.bounds([-px, px], across, z))?);
        // The other bridge in two parts, each up to the first: no two brushes share space.
        for y in [[-py, -BRIDGE_HALF], [BRIDGE_HALF, py]] {
            built.solids.push(deck.boxed(self.bounds(across, y, z))?);
        }
        if paints.liquid.as_ref().map(Hazard::of) == Some(Hazard::Deadly) {
            self.rails([px, py], floor, paints, &mut built)?;
        }
        self.pilasters(paints, [floor, top], &mut built)?;
        self.beams(paints, top, &mut built)?;
        let (corners, _) = self.around(SPOT_INSET);
        for offset in corners {
            built.spots.push(self.standing(offset, floor));
        }
        built.lights.push((
            self.at([0.0, 0.0], top - BEAM_DEPTH - LIGHT_DROP),
            MAIN_LIGHT,
        ));
        for [x, y] in [[px, py], [-px, -py]] {
            built.lights.push((
                self.at(
                    [x / 2.0, y / 2.0],
                    floor - PIT_DEPTH + LIQUID_DEPTH + GLOW_LIFT,
                ),
                GLOW_LIGHT,
            ));
        }
        Ok(built)
    }

    /// Rails on a floor at `floor` around a pit of half-widths `pit` and along both sides of
    /// its bridges, so the only way into it is a jump: on the rim just outside each of its
    /// edges, and on each bridge just inside each of its sides, broken where the bridges meet
    /// the rim and each other. Rails that meet touch at a corner, which no box passes.
    fn rails(
        &self,
        [px, py]: [f64; 2],
        floor: f64,
        paints: &Paints,
        built: &mut Built,
    ) -> Result<(), BrushError> {
        let trim = Coat::all(&paints.trim);
        let z = [floor, floor + RAIL_HEIGHT];
        let b = BRIDGE_HALF;
        let t = RAIL_THICK;
        let mut rail = |x: [f64; 2], y: [f64; 2]| -> Result<(), BrushError> {
            built.solids.push(trim.boxed(self.bounds(x, y, z))?);
            Ok(())
        };
        for (low, high) in [(-py, -b), (b, py)] {
            // The rim beyond the pit's east and west edges, either side of the bridge along x.
            rail([px, px + t], [low, high])?;
            rail([-px - t, -px], [low, high])?;
            // The bridge along y, each side of each of its halves.
            rail([b - t, b], [low, high])?;
            rail([-b, -b + t], [low, high])?;
        }
        for (low, high) in [(-px, -b), (b, px)] {
            // The rim beyond the pit's north and south edges, either side of the bridge along y.
            rail([low, high], [py, py + t])?;
            rail([low, high], [-py - t, -py])?;
            // The bridge along x, each side of each of its halves.
            rail([low, high], [b - t, b])?;
            rail([low, high], [-b, -b + t])?;
        }
        Ok(())
    }

    /// A room on two levels: the part toward one side raised, stairs up to it in the middle.
    fn split(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let (reach, across) = self.turned();
        mass.carve(&self.space(paints)?)?;
        let step = Coat::deck(&paints.floor, &paints.trim);
        let mut built = Built::default();
        built.solids.push(step.boxed(self.turned_bounds(
            [SPLIT_EDGE, reach],
            [-across, across],
            [floor, floor + RAISE],
        ))?);
        let mut from = -SPLIT_EDGE;
        let mut foot = floor;
        while foot < floor + RAISE {
            built.solids.push(step.boxed(self.turned_bounds(
                [from, SPLIT_EDGE],
                [-STAIR_HALF, STAIR_HALF],
                [foot, foot + STAIR_RISE],
            ))?);
            from += STAIR_DEPTH;
            foot += STAIR_RISE;
        }
        self.pilasters(paints, [floor, top], &mut built)?;
        // Above the raised floor as they are above the low one elsewhere.
        for [_, _, z] in &mut built.torches {
            *z += RAISE;
        }
        self.beams(paints, top, &mut built)?;
        let (along, over) = (reach - SPOT_INSET, across - SPOT_INSET);
        for (a, o, z) in [
            (-along, over, floor),
            (-along, -over, floor),
            (along, over, floor + RAISE),
            (along, -over, floor + RAISE),
        ] {
            built.spots.push(self.standing(self.turned_offset(a, o), z));
        }
        built.lights.push((
            self.at([0.0, 0.0], top - BEAM_DEPTH - LIGHT_DROP),
            MAIN_LIGHT,
        ));
        for a in [-reach / 2.0, reach / 2.0] {
            built.lights.push((
                self.at(self.turned_offset(a, 0.0), floor + RAISE + LIGHT_LIFT),
                LESSER_LIGHT,
            ));
        }
        Ok(built)
    }

    /// A courtyard: high walls open to the sky, an obelisk in its middle.
    fn court(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let sky = paints.sky.as_ref().unwrap_or(&paints.ceiling);
        let space = Coat::space(&paints.floor, sky, &paints.wall);
        mass.carve(&space.boxed(self.centred(self.plan.half, [floor, top]))?)?;
        let trim = Coat::all(&paints.trim);
        let mut built = Built::default();
        let foot_top = floor + OBELISK_FOOT_HEIGHT;
        built
            .solids
            .push(trim.boxed(self.centred([OBELISK_FOOT, OBELISK_FOOT], [floor, foot_top]))?);
        let [mx, my] = self.middle();
        let ring = |half: f64, z: f64| {
            [
                [mx + half, my + half, z],
                [mx - half, my + half, z],
                [mx - half, my - half, z],
                [mx + half, my - half, z],
            ]
        };
        let mut shaft = ring(OBELISK_BASE, foot_top).to_vec();
        shaft.extend(ring(OBELISK_TIP, floor + OBELISK_HEIGHT));
        built.solids.push(trim.hull(&shaft)?);
        self.pilasters(paints, [floor, top - VAULT_CUT], &mut built)?;
        let (corners, sides) = self.around(SPOT_INSET);
        for offset in corners.into_iter().chain(sides) {
            built.spots.push(self.standing(offset, floor));
        }
        let (lesser, _) = self.around(LIGHT_INSET + LIGHT_INSET);
        for offset in lesser {
            built
                .lights
                .push((self.at(offset, floor + LIGHT_LIFT + LIGHT_LIFT), MAIN_LIGHT));
        }
        Ok(built)
    }

    /// A cross: two aisles through the room's middle, the one toward the side the room is
    /// turned to the higher, under a lantern where they cross.
    fn cross(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let (reach, across) = self.turned();
        let space = Coat::space(&paints.floor, &paints.ceiling, &paints.wall);
        let aisle = [-AISLE_HALF, AISLE_HALF];
        mass.carve(&space.boxed(self.turned_bounds([-reach, reach], aisle, [floor, top]))?)?;
        // The lower aisle in two arms, each up to the higher: their spaces share no volume.
        for over in [[-across, -AISLE_HALF], [AISLE_HALF, across]] {
            mass.carve(&space.boxed(self.turned_bounds(
                aisle,
                over,
                [floor, top - AISLE_DROP],
            ))?)?;
        }
        let lantern = Coat::space(&paints.floor, &paints.ceiling, &paints.trim);
        mass.carve(&lantern.boxed(self.turned_bounds(aisle, aisle, [top, top + LANTERN_RISE]))?)?;
        let mut built = Built::default();
        for (along, over) in [
            (reach - SPOT_INSET, AISLE_SPOT),
            (AISLE_SPOT, across - SPOT_INSET),
        ] {
            for (a, o) in [
                (along, over),
                (-along, over),
                (-along, -over),
                (along, -over),
            ] {
                built
                    .spots
                    .push(self.standing(self.turned_offset(a, o), floor));
            }
        }
        built.lights.push((
            self.at([0.0, 0.0], top + LANTERN_RISE - LIGHT_DROP),
            MAIN_LIGHT,
        ));
        for (a, o) in [
            (reach - LIGHT_INSET, 0.0),
            (-(reach - LIGHT_INSET), 0.0),
            (0.0, across - LIGHT_INSET),
            (0.0, -(across - LIGHT_INSET)),
        ] {
            built.lights.push((
                self.at(self.turned_offset(a, o), floor + LIGHT_LIFT),
                LESSER_LIGHT,
            ));
        }
        Ok(built)
    }

    /// A long room under a pitched vault, its ridge toward the side the room is turned to.
    fn vault(&self, mass: &mut Mass, paints: &Paints) -> Result<Built, BrushError> {
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let (reach, across) = self.turned();
        let mut corners = Vec::new();
        for along in [-reach, reach] {
            for (over, z) in [
                (-across, floor),
                (across, floor),
                (across, top - VAULT_CUT),
                (across - VAULT_CUT, top),
                (-across + VAULT_CUT, top),
                (-across, top - VAULT_CUT),
            ] {
                corners.push(self.at(self.turned_offset(along, over), z));
            }
        }
        let space = Coat {
            slopes: &paints.ceiling,
            ..Coat::space(&paints.floor, &paints.ceiling, &paints.wall)
        };
        mass.carve(&space.hull(&corners)?)?;
        let mut built = Built::default();
        self.pilasters(paints, [floor, top - VAULT_CUT], &mut built)?;
        let (corners, sides) = self.around(SPOT_INSET);
        for offset in corners.into_iter().chain(sides) {
            built.spots.push(self.standing(offset, floor));
        }
        for along in [-reach / 2.0, 0.0, reach / 2.0] {
            built.lights.push((
                self.at(self.turned_offset(along, 0.0), top - LIGHT_DROP),
                MAIN_LIGHT,
            ));
        }
        Ok(built)
    }
}
