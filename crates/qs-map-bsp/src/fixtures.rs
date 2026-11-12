// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps for tests, written as `.map` text from a spec whose `Default` is the ordinary case: a
//! sealed room with the player standing in it.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: writing to a String cannot fail and the fixtures' numbers are small"
)]

use std::fmt::Write as _;

/// How a face's texture axes are written (MAP-4, MAP-5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Form {
    /// `tex xoff yoff rot xscale yscale`.
    #[default]
    Standard,
    /// `tex [ ux uy uz uoff ] [ vx vy vz voff ] rot xscale yscale`.
    Valve,
}

/// A brush: a box, or a vertical prism over a polygon given counter-clockwise.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Shape {
    Box {
        mins: [i32; 3],
        maxs: [i32; 3],
    },
    Prism {
        corners: Vec<[i32; 2]>,
        bottom: i32,
        top: i32,
    },
}

/// A brush and the texture on all its faces.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushSpec {
    pub(crate) shape: Shape,
    pub(crate) texture: String,
}

impl BrushSpec {
    pub(crate) fn cube(mins: [i32; 3], maxs: [i32; 3], texture: &str) -> Self {
        Self {
            shape: Shape::Box { mins, maxs },
            texture: texture.to_owned(),
        }
    }
}

/// An entity after the world.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntitySpec {
    pub(crate) classname: String,
    pub(crate) origin: Option<[i32; 3]>,
    pub(crate) pairs: Vec<(String, String)>,
    pub(crate) brushes: Vec<BrushSpec>,
}

impl EntitySpec {
    pub(crate) fn point(classname: &str, origin: [i32; 3]) -> Self {
        Self {
            classname: classname.to_owned(),
            origin: Some(origin),
            pairs: Vec::new(),
            brushes: Vec::new(),
        }
    }

    pub(crate) fn brushes(classname: &str, brushes: Vec<BrushSpec>) -> Self {
        Self {
            classname: classname.to_owned(),
            origin: None,
            pairs: Vec::new(),
            brushes,
        }
    }
}

/// A map.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapSpec {
    pub(crate) world: Vec<BrushSpec>,
    pub(crate) world_pairs: Vec<(String, String)>,
    pub(crate) entities: Vec<EntitySpec>,
    pub(crate) form: Form,
}

/// The inside of the default room.
pub(crate) const ROOM_MINS: [i32; 3] = [-128, -128, -64];
pub(crate) const ROOM_MAXS: [i32; 3] = [128, 128, 128];
/// The default walls' thickness.
pub(crate) const WALL: i32 = 16;
/// Where the default player stands.
pub(crate) const PLAYER: [i32; 3] = [0, 0, 24];

impl Default for MapSpec {
    fn default() -> Self {
        Self {
            world: room(ROOM_MINS, ROOM_MAXS, "wall"),
            world_pairs: Vec::new(),
            entities: vec![EntitySpec::point("info_player_start", PLAYER)],
            form: Form::Standard,
        }
    }
}

/// The six walls, `WALL` thick, around the box `mins` to `maxs`: floor, ceiling, then -x, +x,
/// -y and +y.
pub(crate) fn room(mins: [i32; 3], maxs: [i32; 3], texture: &str) -> Vec<BrushSpec> {
    let [x0, y0, z0] = mins;
    let [x1, y1, z1] = maxs;
    let (ox0, oy0, oz0) = (x0 - WALL, y0 - WALL, z0 - WALL);
    let (ox1, oy1, oz1) = (x1 + WALL, y1 + WALL, z1 + WALL);
    [
        ([ox0, oy0, oz0], [ox1, oy1, z0]),
        ([ox0, oy0, z1], [ox1, oy1, oz1]),
        ([ox0, oy0, z0], [x0, oy1, z1]),
        ([x1, oy0, z0], [ox1, oy1, z1]),
        ([x0, oy0, z0], [x1, y0, z1]),
        ([x0, y1, z0], [x1, oy1, z1]),
    ]
    .into_iter()
    .map(|(lo, hi)| BrushSpec::cube(lo, hi, texture))
    .collect()
}

type Point = [f64; 3];

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Three points of a face, ordered so qbsp's normal, `(p0 - p1) x (p2 - p1)`, is `outward`.
fn oriented(mut points: [Point; 3], outward: Point) -> [Point; 3] {
    let [p0, p1, p2] = points;
    if dot(cross(sub(p0, p1), sub(p2, p1)), outward) < 0.0 {
        points.swap(0, 2);
    }
    points
}

/// Each face's three points.
fn faces(shape: &Shape) -> Vec<[Point; 3]> {
    match shape {
        Shape::Box { mins, maxs } => {
            let lo = mins.map(f64::from);
            let hi = maxs.map(f64::from);
            let mut out = Vec::new();
            for axis in 0..3 {
                let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                for (at, sign) in [(lo[axis], -1.0), (hi[axis], 1.0)] {
                    let mut base = [0.0; 3];
                    base[axis] = at;
                    base[b] = lo[b];
                    base[c] = lo[c];
                    let mut along_b = base;
                    along_b[b] = hi[b];
                    let mut along_c = base;
                    along_c[c] = hi[c];
                    let mut outward = [0.0; 3];
                    outward[axis] = sign;
                    out.push(oriented([along_b, base, along_c], outward));
                }
            }
            out
        }
        Shape::Prism {
            corners,
            bottom,
            top,
        } => {
            let (z0, z1) = (f64::from(*bottom), f64::from(*top));
            let mut out = vec![
                oriented(
                    [[0.0, 0.0, z0], [1.0, 0.0, z0], [0.0, 1.0, z0]],
                    [0.0, 0.0, -1.0],
                ),
                oriented(
                    [[0.0, 0.0, z1], [1.0, 0.0, z1], [0.0, 1.0, z1]],
                    [0.0, 0.0, 1.0],
                ),
            ];
            for (i, a) in corners.iter().enumerate() {
                let b = corners[(i + 1) % corners.len()];
                let (a, b) = (a.map(f64::from), b.map(f64::from));
                let outward = [b[1] - a[1], a[0] - b[0], 0.0];
                out.push(oriented(
                    [[a[0], a[1], z0], [b[0], b[1], z0], [a[0], a[1], z1]],
                    outward,
                ));
            }
            out
        }
    }
}

fn brush_text(out: &mut String, brush: &BrushSpec, form: Form) {
    out.push_str("{\n");
    for face in faces(&brush.shape) {
        for p in face {
            write!(out, "( {} {} {} ) ", p[0], p[1], p[2]).unwrap();
        }
        match form {
            Form::Standard => writeln!(out, "{} 0 0 0 1 1", brush.texture).unwrap(),
            Form::Valve => {
                writeln!(out, "{} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1", brush.texture).unwrap();
            }
        }
    }
    out.push_str("}\n");
}

fn pair(out: &mut String, key: &str, value: &str) {
    writeln!(out, "\"{key}\" \"{value}\"").unwrap();
}

/// The map's text.
pub(crate) fn map_text(spec: &MapSpec) -> String {
    let mut out = String::from("{\n");
    pair(&mut out, "classname", "worldspawn");
    if spec.form == Form::Valve {
        pair(&mut out, "mapversion", "220");
    }
    for (key, value) in &spec.world_pairs {
        pair(&mut out, key, value);
    }
    for brush in &spec.world {
        brush_text(&mut out, brush, spec.form);
    }
    out.push_str("}\n");
    for entity in &spec.entities {
        out.push_str("{\n");
        pair(&mut out, "classname", &entity.classname);
        if let Some([x, y, z]) = entity.origin {
            pair(&mut out, "origin", &format!("{x} {y} {z}"));
        }
        for (key, value) in &entity.pairs {
            pair(&mut out, key, value);
        }
        for brush in &entity.brushes {
            brush_text(&mut out, brush, spec.form);
        }
        out.push_str("}\n");
    }
    out
}
