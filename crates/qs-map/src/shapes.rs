// SPDX-License-Identifier: GPL-3.0-or-later

//! Shapes made of brushes (P6-18, MAP-10): cylinders, cones, arches, spheroids, rocks,
//! grid-aligned scalable curves and stairs, each filling a box the modder drags, as Hammer's
//! and J.A.C.K.'s block tool make them (sides, wall thickness, arc and start angle). Every brush
//! is made from its faces' planes, so each is convex and valid (MAP-3.4) or the shape is
//! refused.

use core::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::brush_edit::BrushError;
use crate::geometry::{Plane, dot, plane as plane_of};
use crate::{Brush, Surface};

/// The fewest sides a round shape may have (MAP-10.3).
pub const SHAPE_MIN_SIDES: u32 = 3;
/// The most sides a round shape may have (MAP-10.3).
pub const SHAPE_MAX_SIDES: u32 = 128;
/// The fewest bands of latitude of a UV spheroid (MAP-10.7).
pub const SHAPE_MIN_RINGS: u32 = 2;
/// The most bands of latitude of a UV spheroid (MAP-10.7).
pub const SHAPE_MAX_RINGS: u32 = 64;
/// The most faces a UV spheroid may have, its slices times its bands: an icospheroid's at
/// its most (MAP-10.7).
pub const SHAPE_MAX_SPHEROID_FACES: u32 = 1280;
/// The most times an icosphere's triangles are each cut in four (MAP-10.7).
pub const SHAPE_MAX_SUBDIVISIONS: u32 = 3;
/// The fewest points a rock is the hull of (MAP-10.8).
pub const SHAPE_MIN_ROCK_POINTS: u32 = 4;
/// The most points a rock is the hull of (MAP-10.8).
pub const SHAPE_MAX_ROCK_POINTS: u32 = 64;
/// The most steps stairs may have (MAP-10.10).
pub const SHAPE_MAX_STEPS: u32 = 256;
/// The degrees in half a turn, which an arch stays within (MAP-10.6).
const HALF_TURN: f64 = 180.0;
/// The default height of a step: below the 18 units `SV_WalkMove` climbs (`STEPSIZE`,
/// PHYS-8.3), so a player walks up it (MAP-10.10).
pub const SHAPE_STEP_HEIGHT: f64 = 16.0;
/// The default depth of a step: the player's box is 32 units across (MAP-10.10).
pub const SHAPE_STEP_DEPTH: f64 = 32.0;

/// Where a rock's points lie, as a part of the box's radius: from this to the whole of it.
const ROCK_INNER: f64 = 0.7;
/// The golden angle, which spreads a rock's points over a sphere evenly.
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;
/// The multiplier of the rock's random sequence, Knuth's MMIX constant.
const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
/// The increment of the rock's random sequence.
const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;
/// The bits of the random sequence a fraction is made from.
const FRACTION_SHIFT: u32 = 11;
/// One more than the largest fraction's numerator, `2^53`.
const FRACTION_SCALE: f64 = 9_007_199_254_740_992.0;
/// A point with every coordinate 0.
const ORIGIN: [f64; 3] = [0.0; 3];
/// Where in its band of the spiral each rock point lies: its middle.
const BAND_MIDDLE: f64 = 0.5;
/// How far up the box a cone's inside point is taken, as a part of its height: below the
/// middle, so it is inside the cone.
const CONE_INSIDE: f64 = 0.25;
/// The fewest corners of a polygon.
const MIN_CORNERS: usize = 3;
/// The triangles each triangle is cut into.
const SUBDIVISION_PARTS: usize = 4;
/// The golden ratio, which places an icosahedron's corners.
const PHI: f64 = core::f64::consts::GOLDEN_RATIO;
/// A unit icosahedron's corners before they are made unit length.
const ICOSAHEDRON_CORNERS: [[f64; 3]; 12] = [
    [-1.0, PHI, 0.0],
    [1.0, PHI, 0.0],
    [-1.0, -PHI, 0.0],
    [1.0, -PHI, 0.0],
    [0.0, -1.0, PHI],
    [0.0, 1.0, PHI],
    [0.0, -1.0, -PHI],
    [0.0, 1.0, -PHI],
    [PHI, 0.0, -1.0],
    [PHI, 0.0, 1.0],
    [-PHI, 0.0, -1.0],
    [-PHI, 0.0, 1.0],
];
/// An icosahedron's triangles, by its corners.
const ICOSAHEDRON_FACES: [[usize; 3]; 20] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];
/// The sides of a 12-sided scalable curve.
const TWELVE_SIDES: u32 = 12;
/// The sides of a 24-sided scalable curve.
const TWENTY_FOUR_SIDES: u32 = 24;
/// The sides of a 48-sided scalable curve.
const FORTY_EIGHT_SIDES: u32 = 48;
/// The sides of a 96-sided scalable curve.
const NINETY_SIX_SIDES: u32 = 96;
/// How far a spandrel's corner may be below the box's top and still be on it.
const ON_TOP: f64 = 1e-6;

/// Where a round shape's corners lie (MAP-10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CircleMode {
    /// On the ellipse the box holds: the shape fits inside it.
    Vertices,
    /// Outside it, each edge touching it at its middle: the shape holds the ellipse.
    Edges,
}

/// Whether a shape is solid or a wall around a hole (MAP-10.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wall {
    /// Solid.
    Solid,
    /// A wall this thick, in map units, measured in from the box.
    Hollow {
        /// The thickness.
        thickness: f64,
    },
}

/// Whether an arch fills the space between its curve and the box's top (MAP-10.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spandrels {
    /// The arch alone.
    Open,
    /// The arch in a wall up to the box's top.
    Filled,
}

/// The sides of a scalable curve (MAP-10.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveSides {
    /// 12 sides.
    Twelve,
    /// 24 sides.
    TwentyFour,
    /// 48 sides.
    FortyEight,
    /// 96 sides.
    NinetySix,
}

impl CurveSides {
    /// The sides.
    #[must_use]
    pub const fn count(self) -> u32 {
        match self {
            Self::Twelve => TWELVE_SIDES,
            Self::TwentyFour => TWENTY_FOUR_SIDES,
            Self::FortyEight => FORTY_EIGHT_SIDES,
            Self::NinetySix => NINETY_SIX_SIDES,
        }
    }
}

/// The way stairs climb (MAP-10.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// Toward +x.
    East,
    /// Toward -x.
    West,
    /// Toward +y.
    North,
    /// Toward -y.
    South,
}

/// A shape and its settings (MAP-10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// An upright prism of `sides` sides around the box's ellipse (MAP-10.3, MAP-10.4).
    Cylinder {
        /// The sides.
        sides: u32,
        /// Where the corners lie.
        mode: CircleMode,
        /// Solid or hollow.
        wall: Wall,
    },
    /// A cone: the cylinder's base and a point at the middle of the box's top (MAP-10.5).
    Cone {
        /// The sides.
        sides: u32,
        /// Where the corners lie.
        mode: CircleMode,
    },
    /// An arch in the box's x-z face, through the box along y, its corners on its ellipse
    /// (MAP-10.6).
    Arch {
        /// The segments of its curve.
        sides: u32,
        /// The degrees of the curve, above 0, the curve within half a turn.
        arc: f64,
        /// The degrees the curve starts at, from +x toward +z, 0 or more.
        start: f64,
        /// The thickness of the curve.
        thickness: f64,
        /// Whether it is filled up to the box's top.
        spandrels: Spandrels,
    },
    /// A spheroid of bands of latitude and `sides` slices of longitude (MAP-10.7).
    UvSpheroid {
        /// The slices.
        sides: u32,
        /// The bands.
        rings: u32,
    },
    /// A spheroid of an icosahedron's triangles, each cut in four `subdivisions` times
    /// (MAP-10.7).
    Icospheroid {
        /// The times each triangle is cut.
        subdivisions: u32,
    },
    /// The hull of `points` points scattered over the box's spheroid by `seed` (MAP-10.8).
    Rock {
        /// The points.
        points: u32,
        /// The seed: one seed, one rock.
        seed: u64,
    },
    /// A curve whose every corner is on the grid at every size (MAP-10.9).
    Curve {
        /// The sides.
        sides: CurveSides,
        /// The grid, in map units.
        grid: f64,
        /// Solid or hollow.
        wall: Wall,
    },
    /// Straight stairs (MAP-10.10).
    Stairs {
        /// A step's height.
        height: f64,
        /// A step's depth.
        depth: f64,
        /// The way they climb.
        facing: Facing,
    },
    /// Spiral stairs around the box's middle, `sides` steps a turn (MAP-10.11).
    SpiralStairs {
        /// The steps a turn.
        sides: u32,
        /// A step's height.
        height: f64,
        /// The radius of the empty middle; 0 for none.
        core: f64,
    },
}

/// Why a shape could not be made.
#[derive(Debug)]
pub enum ShapeError {
    /// The box has no volume, or a corner that is not a number.
    Size,
    /// The sides are outside the range the shape takes.
    Sides(u32),
    /// The wall is not thinner than the box's smaller radius, or not above 0.
    Thickness(f64),
    /// The arc is not above 0, or the start below 0, or the arch past half a turn.
    Arc {
        /// The arc's degrees.
        arc: f64,
        /// The start's degrees.
        start: f64,
    },
    /// A scalable curve's sides are not 12, 24, 48 or 96.
    CurveSides(u32),
    /// The bands of latitude are outside the range.
    Rings(u32),
    /// The subdivisions are past the most.
    Subdivisions(u32),
    /// A UV spheroid's slices times its bands are past the most faces.
    Faces(u32),
    /// The rock's points are outside the range.
    Points(u32),
    /// A step's height or depth is not above 0, or no step fits in the box, or more than
    /// [`SHAPE_MAX_STEPS`] would.
    Step,
    /// The grid is not above 0, or a curve's wall leaves no hole.
    Grid(f64),
    /// A brush of the shape is not valid.
    Brush(BrushError),
}

impl core::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Size => f.write_str("the box has no volume"),
            Self::Sides(sides) => write!(
                f,
                "{sides} sides; a shape has {SHAPE_MIN_SIDES} to {SHAPE_MAX_SIDES}"
            ),
            Self::Thickness(t) => write!(
                f,
                "a wall {t} thick does not fit: it must be above 0 and thinner than the box's \
                 smaller radius"
            ),
            Self::Arc { arc, start } => write!(
                f,
                "an arc of {arc} degrees from {start}; the arc must be above 0 and the arch \
                 within half a turn, from 0 to 180 degrees"
            ),
            Self::CurveSides(sides) => {
                write!(f, "{sides} sides; a scalable curve has 12, 24, 48 or 96")
            }
            Self::Rings(rings) => write!(
                f,
                "{rings} bands; a spheroid has {SHAPE_MIN_RINGS} to {SHAPE_MAX_RINGS}"
            ),
            Self::Faces(faces) => write!(
                f,
                "{faces} faces; a spheroid has at most {SHAPE_MAX_SPHEROID_FACES}"
            ),
            Self::Subdivisions(n) => write!(
                f,
                "{n} subdivisions; an icospheroid has at most {SHAPE_MAX_SUBDIVISIONS}"
            ),
            Self::Points(points) => write!(
                f,
                "{points} points; a rock has {SHAPE_MIN_ROCK_POINTS} to {SHAPE_MAX_ROCK_POINTS}"
            ),
            Self::Step => write!(
                f,
                "a step must be above 0 high and deep, and 1 to {SHAPE_MAX_STEPS} must fit"
            ),
            Self::Grid(grid) => write!(
                f,
                "a grid of {grid} with that wall leaves no curve; the grid must be above 0"
            ),
            Self::Brush(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ShapeError {}

impl From<BrushError> for ShapeError {
    fn from(error: BrushError) -> Self {
        Self::Brush(error)
    }
}

/// The box a shape fills.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}

impl Bounds {
    fn new(a: [f64; 3], b: [f64; 3]) -> Result<Self, ShapeError> {
        let mut min = ORIGIN;
        let mut max = ORIGIN;
        for (i, (lo, hi)) in min.iter_mut().zip(max.iter_mut()).enumerate() {
            let (p, q) = (
                a.get(i).copied().unwrap_or(0.0),
                b.get(i).copied().unwrap_or(0.0),
            );
            // A box no wider than a rounding error has no volume.
            if !p.is_finite() || !q.is_finite() || (p - q).abs() < f64::EPSILON {
                return Err(ShapeError::Size);
            }
            *lo = p.min(q);
            *hi = p.max(q);
        }
        Ok(Self { min, max })
    }

    /// The middle of the box's footprint and its radii along x and y.
    fn footprint(&self) -> ([f64; 2], [f64; 2]) {
        let [x0, y0, _] = self.min;
        let [x1, y1, _] = self.max;
        (
            [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
            [(x1 - x0) / 2.0, (y1 - y0) / 2.0],
        )
    }
}

/// Checks a round shape's sides.
fn checked_sides(sides: u32) -> Result<u32, ShapeError> {
    if (SHAPE_MIN_SIDES..=SHAPE_MAX_SIDES).contains(&sides) {
        Ok(sides)
    } else {
        Err(ShapeError::Sides(sides))
    }
}

/// The corners of a polygon of `sides` sides around the ellipse at `centre` of `radii`, as
/// `mode` places them, counter-clockwise, the first at angle 0 for corners on the ellipse and
/// at half a side for edges touching it (MAP-10.2).
fn ring(sides: u32, mode: CircleMode, centre: [f64; 2], radii: [f64; 2]) -> Vec<[f64; 2]> {
    let n = f64::from(sides);
    let (offset, grow) = match mode {
        CircleMode::Vertices => (0.0, 1.0),
        CircleMode::Edges => (PI / n, 1.0 / (PI / n).cos()),
    };
    let [cx, cy] = centre;
    let [rx, ry] = radii;
    (0..sides)
        .map(|i| {
            let angle = offset + TAU * f64::from(i) / n;
            [cx + rx * grow * angle.cos(), cy + ry * grow * angle.sin()]
        })
        .collect()
}

/// The axis a flat profile is pushed along to make a prism.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Along {
    /// The profile is in x and y, pushed along z.
    Z,
    /// The profile is in x and z, pushed along y.
    Y,
}

impl Along {
    /// The point of the profile point `p` at `depth` along the axis.
    const fn point(self, p: [f64; 2], depth: f64) -> [f64; 3] {
        let [u, v] = p;
        match self {
            Self::Z => [u, v, depth],
            Self::Y => [u, depth, v],
        }
    }

    /// The unit vector along the axis.
    const fn axis(self) -> [f64; 3] {
        match self {
            Self::Z => [0.0, 0.0, 1.0],
            Self::Y => [0.0, 1.0, 0.0],
        }
    }
}

/// The plane through `points` facing away from `inside`.
fn outward(points: [[f64; 3]; 3], inside: [f64; 3]) -> Option<Plane<f64>> {
    let plane = plane_of(points)?;
    Some(if plane.side(inside) > 0.0 {
        plane.flipped()
    } else {
        plane
    })
}

/// The middle of `points`.
fn centroid(points: &[[f64; 3]]) -> [f64; 3] {
    let mut sum = ORIGIN;
    for p in points {
        for (s, c) in sum.iter_mut().zip(p) {
            *s += c;
        }
    }
    let count = f64::from(u32::try_from(points.len()).unwrap_or(u32::MAX).max(1));
    sum.map(|s| s / count)
}

/// The brush of the prism of the convex `profile` pushed along `along` from `low` to `high`.
fn prism(
    profile: &[[f64; 2]],
    along: Along,
    [low, high]: [f64; 2],
    surface: &Surface,
) -> Result<Brush, ShapeError> {
    let corners: Vec<[f64; 3]> = profile
        .iter()
        .flat_map(|&p| [along.point(p, low), along.point(p, high)])
        .collect();
    let inside = centroid(&corners);
    let axis = along.axis();
    let mut planes = vec![
        Plane {
            normal: axis.map(|c| -c),
            dist: -low,
        },
        Plane {
            normal: axis,
            dist: high,
        },
    ];
    for (i, &a) in profile.iter().enumerate() {
        let b = profile
            .get(i.saturating_add(1))
            .or_else(|| profile.first())
            .copied()
            .unwrap_or(a);
        let plane = outward(
            [
                along.point(a, low),
                along.point(b, low),
                along.point(a, high),
            ],
            inside,
        )
        .ok_or(BrushError::Degenerate)?;
        planes.push(plane);
    }
    Ok(Brush::from_planes(&planes, surface)?)
}

/// The brush behind every face of the closed surface `triangles`, each a plane facing away
/// from `inside`.
fn polyhedron(
    triangles: &[[[f64; 3]; 3]],
    inside: [f64; 3],
    surface: &Surface,
) -> Result<Brush, ShapeError> {
    let mut planes = Vec::new();
    for &triangle in triangles {
        if let Some(plane) = outward(triangle, inside) {
            planes.push(plane);
        }
    }
    Ok(Brush::from_planes(&planes, surface)?)
}

/// Whether a ring goes all the way round, its last corner joined to its first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ends {
    /// All the way round.
    Closed,
    /// An arc, its ends apart.
    Open,
}

/// The segments of a ring between `outer` and `inner`, each the quadrilateral from one corner
/// to the next of both, so neighbours share their sides (MAP-10.4).
fn segments(outer: &[[f64; 2]], inner: &[[f64; 2]], ends: Ends) -> Vec<Vec<[f64; 2]>> {
    let count = outer.len().min(inner.len());
    let last = match ends {
        Ends::Closed => count,
        Ends::Open => count.saturating_sub(1),
    };
    (0..last)
        .filter_map(|i| {
            let j = if i.saturating_add(1) == count {
                0
            } else {
                i.saturating_add(1)
            };
            Some(vec![
                *outer.get(i)?,
                *outer.get(j)?,
                *inner.get(j)?,
                *inner.get(i)?,
            ])
        })
        .collect()
}

/// Checks a wall's thickness against the smaller of `radii`.
fn checked_thickness(thickness: f64, radii: [f64; 2]) -> Result<f64, ShapeError> {
    let [rx, ry] = radii;
    if thickness > 0.0 && thickness < rx.min(ry) {
        Ok(thickness)
    } else {
        Err(ShapeError::Thickness(thickness))
    }
}

/// The next fraction from 0 up to 1 of the rock's random sequence, and its next state.
fn next_fraction(state: u64) -> (f64, u64) {
    let next = state
        .wrapping_mul(LCG_MULTIPLIER)
        .wrapping_add(LCG_INCREMENT);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the top 53 bits are a double exactly; the rest are left out"
    )]
    let fraction = (next >> FRACTION_SHIFT) as f64 / FRACTION_SCALE;
    (fraction, next)
}

/// The convex hull of `points`, counter-clockwise, each corner once and none on an edge:
/// Andrew's monotone chain.
fn hull_2d(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();
    if sorted.len() < MIN_CORNERS {
        return sorted;
    }
    let turn = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::new();
    for pass in [sorted.clone(), sorted.into_iter().rev().collect()] {
        let start = hull.len();
        for p in pass {
            while hull.len() >= start.saturating_add(2) {
                let (Some(&b), Some(&a)) = (hull.last(), hull.get(hull.len().saturating_sub(2)))
                else {
                    break;
                };
                if turn(a, b, p) > 0.0 {
                    break;
                }
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

impl Shape {
    /// The shape's brushes filling the box from `a` to `b`, each face painted with `surface`.
    ///
    /// Implements MAP-10.
    ///
    /// # Errors
    ///
    /// The [`ShapeError`] of a setting out of range, or of a brush that would not be valid.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per shape, each its construction in full"
    )]
    pub fn brushes(
        &self,
        a: [f64; 3],
        b: [f64; 3],
        surface: &Surface,
    ) -> Result<Vec<Brush>, ShapeError> {
        let bounds = Bounds::new(a, b)?;
        let (centre, radii) = bounds.footprint();
        let [x0, y0, z0] = bounds.min;
        let [x1, y1, z1] = bounds.max;
        match *self {
            Self::Cylinder { sides, mode, wall } => {
                let outer = ring(checked_sides(sides)?, mode, centre, radii);
                match wall {
                    Wall::Solid => Ok(vec![prism(&outer, Along::Z, [z0, z1], surface)?]),
                    Wall::Hollow { thickness } => {
                        let t = checked_thickness(thickness, radii)?;
                        let inner = ring(sides, mode, centre, radii.map(|r| r - t));
                        segments(&outer, &inner, Ends::Closed)
                            .iter()
                            .map(|quad| prism(quad, Along::Z, [z0, z1], surface))
                            .collect()
                    }
                }
            }
            Self::Cone { sides, mode } => {
                let base = ring(checked_sides(sides)?, mode, centre, radii);
                let [cx, cy] = centre;
                let apex = [cx, cy, z1];
                let inside = [cx, cy, z0 + (z1 - z0) * CONE_INSIDE];
                let mut triangles = vec![];
                let corners: Vec<[f64; 3]> = base.iter().map(|&p| Along::Z.point(p, z0)).collect();
                for (i, &p) in corners.iter().enumerate() {
                    let q = corners
                        .get(i.saturating_add(1))
                        .or_else(|| corners.first())
                        .copied()
                        .unwrap_or(p);
                    triangles.push([p, q, apex]);
                }
                if let [first, second, third, ..] = corners.as_slice() {
                    triangles.push([*first, *second, *third]);
                }
                Ok(vec![polyhedron(&triangles, inside, surface)?])
            }
            Self::Arch {
                sides,
                arc,
                start,
                thickness,
                spandrels,
            } => {
                let sides = checked_sides(sides)?;
                if !(arc > 0.0 && start >= 0.0 && start + arc <= HALF_TURN) {
                    return Err(ShapeError::Arc { arc, start });
                }
                let centre = [(x0 + x1) / 2.0, z0];
                let radii = [(x1 - x0) / 2.0, z1 - z0];
                let t = checked_thickness(thickness, radii)?;
                let (from, sweep) = (start.to_radians(), arc.to_radians());
                let curve = |radii: [f64; 2]| -> Vec<[f64; 2]> {
                    let n = f64::from(sides);
                    let [cx, cz] = centre;
                    let [rx, rz] = radii;
                    (0..=sides)
                        .map(|i| {
                            let angle = from + sweep * f64::from(i) / n;
                            [cx + rx * angle.cos(), cz + rz * angle.sin()]
                        })
                        .collect()
                };
                let outer = curve(radii);
                let inner = curve(radii.map(|r| r - t));
                let mut brushes = Vec::new();
                for quad in segments(&outer, &inner, Ends::Open) {
                    brushes.push(prism(&quad, Along::Y, [y0, y1], surface)?);
                }
                if spandrels == Spandrels::Filled {
                    for pair in outer.windows(2) {
                        let [p, q] =
                            [pair.first(), pair.get(1)].map(|c| c.copied().unwrap_or_default());
                        let [px, pz] = p;
                        let [qx, qz] = q;
                        let mut profile = vec![p, q];
                        if z1 - qz >= ON_TOP {
                            profile.push([qx, z1]);
                        }
                        if z1 - pz >= ON_TOP {
                            profile.push([px, z1]);
                        }
                        if profile.len() >= MIN_CORNERS {
                            brushes.push(prism(&profile, Along::Y, [y0, y1], surface)?);
                        }
                    }
                }
                Ok(brushes)
            }
            Self::UvSpheroid { sides, rings } => {
                let sides = checked_sides(sides)?;
                if !(SHAPE_MIN_RINGS..=SHAPE_MAX_RINGS).contains(&rings) {
                    return Err(ShapeError::Rings(rings));
                }
                let faces = sides.saturating_mul(rings);
                if faces > SHAPE_MAX_SPHEROID_FACES {
                    return Err(ShapeError::Faces(faces));
                }
                let middle = [centre[0], centre[1], (z0 + z1) / 2.0];
                let radius = [radii[0], radii[1], (z1 - z0) / 2.0];
                let point = |ring: u32, slice: u32| {
                    let lat = -FRAC_PI_2 + PI * f64::from(ring) / f64::from(rings);
                    let lon = TAU * f64::from(slice) / f64::from(sides);
                    [
                        middle[0] + radius[0] * lat.cos() * lon.cos(),
                        middle[1] + radius[1] * lat.cos() * lon.sin(),
                        middle[2] + radius[2] * lat.sin(),
                    ]
                };
                let mut triangles = Vec::new();
                for ring in 0..rings {
                    for slice in 0..sides {
                        let next = if slice.saturating_add(1) == sides {
                            0
                        } else {
                            slice.saturating_add(1)
                        };
                        let (a, b) = (point(ring, slice), point(ring, next));
                        let (c, d) = (
                            point(ring.saturating_add(1), next),
                            point(ring.saturating_add(1), slice),
                        );
                        // A band's quadrilateral is flat; at a pole two of its corners are one.
                        if ring == 0 {
                            triangles.push([a, c, d]);
                        } else {
                            triangles.push([a, b, c]);
                        }
                    }
                }
                Ok(vec![polyhedron(&triangles, middle, surface)?])
            }
            Self::Icospheroid { subdivisions } => {
                if subdivisions > SHAPE_MAX_SUBDIVISIONS {
                    return Err(ShapeError::Subdivisions(subdivisions));
                }
                let middle = [centre[0], centre[1], (z0 + z1) / 2.0];
                let radius = [radii[0], radii[1], (z1 - z0) / 2.0];
                let mut triangles = icosahedron();
                for _ in 0..subdivisions {
                    triangles = subdivide(&triangles);
                }
                let placed: Vec<[[f64; 3]; 3]> = triangles
                    .iter()
                    .map(|t| {
                        t.map(|p| {
                            [
                                middle[0] + radius[0] * p[0],
                                middle[1] + radius[1] * p[1],
                                middle[2] + radius[2] * p[2],
                            ]
                        })
                    })
                    .collect();
                Ok(vec![polyhedron(&placed, middle, surface)?])
            }
            Self::Rock { points, seed } => {
                if !(SHAPE_MIN_ROCK_POINTS..=SHAPE_MAX_ROCK_POINTS).contains(&points) {
                    return Err(ShapeError::Points(points));
                }
                let middle = [centre[0], centre[1], (z0 + z1) / 2.0];
                let radius = [radii[0], radii[1], (z1 - z0) / 2.0];
                let mut state = seed;
                let mut corners = Vec::new();
                let n = f64::from(points);
                for i in 0..points {
                    // A spiral over the sphere spreads the points; the sequence varies how far
                    // out each is.
                    let z = 1.0 - 2.0 * (f64::from(i) + BAND_MIDDLE) / n;
                    let across = (1.0 - z * z).max(0.0).sqrt();
                    let angle = GOLDEN_ANGLE * f64::from(i);
                    let (fraction, next) = next_fraction(state);
                    state = next;
                    let out = ROCK_INNER + (1.0 - ROCK_INNER) * fraction;
                    corners.push([
                        middle[0] + radius[0] * out * across * angle.cos(),
                        middle[1] + radius[1] * out * across * angle.sin(),
                        middle[2] + radius[2] * out * z,
                    ]);
                }
                Ok(vec![Brush::hull(&corners, &[], surface)?])
            }
            Self::Curve { sides, grid, wall } => {
                if !(grid > 0.0 && grid.is_finite()) {
                    return Err(ShapeError::Grid(grid));
                }
                let snap = |v: f64| (v / grid).round() * grid;
                let low = [snap(x0), snap(y0)];
                let high = [snap(x1).max(low[0] + grid), snap(y1).max(low[1] + grid)];
                let centre = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0];
                let radii = [(high[0] - low[0]) / 2.0, (high[1] - low[1]) / 2.0];
                let on_grid = |radii: [f64; 2]| -> Vec<[f64; 2]> {
                    ring(sides.count(), CircleMode::Vertices, centre, radii)
                        .into_iter()
                        .map(|corner| corner.map(snap))
                        .collect()
                };
                let outer = on_grid(radii);
                let heights = [snap(z0), snap(z1).max(snap(z0) + grid)];
                match wall {
                    Wall::Solid => Ok(vec![prism(&hull_2d(&outer), Along::Z, heights, surface)?]),
                    Wall::Hollow { thickness } => {
                        let t = checked_thickness(thickness, radii)?;
                        let inner = on_grid(radii.map(|r| r - t));
                        let mut brushes = Vec::new();
                        for quad in segments(&outer, &inner, Ends::Closed) {
                            let hull = hull_2d(&quad);
                            if hull.len() >= MIN_CORNERS {
                                brushes.push(prism(&hull, Along::Z, heights, surface)?);
                            }
                        }
                        Ok(brushes)
                    }
                }
            }
            Self::Stairs {
                height,
                depth,
                facing,
            } => {
                if !(height > 0.0 && depth > 0.0 && height.is_finite() && depth.is_finite()) {
                    return Err(ShapeError::Step);
                }
                let length = match facing {
                    Facing::East | Facing::West => x1 - x0,
                    Facing::North | Facing::South => y1 - y0,
                };
                let by_length = (length / depth).floor();
                let by_height = ((z1 - z0) / height).floor();
                let count = by_length.min(by_height);
                if !(1.0..=f64::from(SHAPE_MAX_STEPS)).contains(&count) {
                    return Err(ShapeError::Step);
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a whole number of steps, at least 1, that fit in a finite box"
                )]
                let count = count as u32;
                let mut brushes = Vec::new();
                for i in 0..count {
                    let (near, far) =
                        (f64::from(i) * depth, f64::from(i.saturating_add(1)) * depth);
                    let top = z0 + f64::from(i.saturating_add(1)) * height;
                    let (min, max) = match facing {
                        Facing::East => ([x0 + near, y0, z0], [x0 + far, y1, top]),
                        Facing::West => ([x1 - far, y0, z0], [x1 - near, y1, top]),
                        Facing::North => ([x0, y0 + near, z0], [x1, y0 + far, top]),
                        Facing::South => ([x0, y1 - far, z0], [x1, y1 - near, top]),
                    };
                    brushes.push(Brush::cuboid(min, max, surface)?);
                }
                Ok(brushes)
            }
            Self::SpiralStairs {
                sides,
                height,
                core,
            } => {
                let sides = checked_sides(sides)?;
                let [rx, ry] = radii;
                if !(core >= 0.0 && core < rx.min(ry)) {
                    return Err(ShapeError::Thickness(core));
                }
                let count = ((z1 - z0) / height).floor();
                if !(1.0..=f64::from(SHAPE_MAX_STEPS)).contains(&count) {
                    return Err(ShapeError::Step);
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a whole number of steps, at least 1, that fit in a finite box"
                )]
                let count = count as u32;
                let outer = ring(sides, CircleMode::Vertices, centre, radii);
                let inner = ring(sides, CircleMode::Vertices, centre, [core, core]);
                let mut brushes = Vec::new();
                for i in 0..count {
                    let slice =
                        usize::try_from(i.checked_rem(sides).unwrap_or(0)).unwrap_or_default();
                    let next = usize::try_from(i.saturating_add(1).checked_rem(sides).unwrap_or(0))
                        .unwrap_or_default();
                    let (Some(&o0), Some(&o1)) = (outer.get(slice), outer.get(next)) else {
                        continue;
                    };
                    let profile = if core > 0.0 {
                        let (Some(&n0), Some(&n1)) = (inner.get(slice), inner.get(next)) else {
                            continue;
                        };
                        vec![o0, o1, n1, n0]
                    } else {
                        vec![centre, o0, o1]
                    };
                    let low = z0 + f64::from(i) * height;
                    brushes.push(prism(&profile, Along::Z, [low, low + height], surface)?);
                }
                Ok(brushes)
            }
        }
    }
}

/// A unit icosahedron's twenty triangles.
fn icosahedron() -> Vec<[[f64; 3]; 3]> {
    let corners = ICOSAHEDRON_CORNERS.map(unit);
    ICOSAHEDRON_FACES
        .iter()
        .map(|face| face.map(|i| corners.get(i).copied().unwrap_or_default()))
        .collect()
}

/// Each triangle cut in four at its edges' middles, pushed out onto the unit sphere.
fn subdivide(triangles: &[[[f64; 3]; 3]]) -> Vec<[[f64; 3]; 3]> {
    let mid = |a: [f64; 3], b: [f64; 3]| unit([a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
    let mut out = Vec::with_capacity(triangles.len().saturating_mul(SUBDIVISION_PARTS));
    for &[a, b, c] in triangles {
        let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
        out.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
    }
    out
}

/// `v` made unit length.
fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = dot(v, v).sqrt();
    if length > 0.0 {
        v.map(|c| c / length)
    } else {
        v
    }
}

#[cfg(test)]
mod tests;
