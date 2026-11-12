// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index, or a wrong variant, is a failed test; the \
              normals and axes compared are exact"
)]

use proptest::prelude::*;

use super::{BrushError, SAME_NORMAL, SAME_POINT, Surface, TextureLock, face_points, same_point};
use crate::geometry::{ON_PLANE, Plane, plane};
use crate::{Brush, Item, MapFile, TexDef};

fn surface() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

fn cuboid(min: [f64; 3], max: [f64; 3]) -> Brush {
    Brush::cuboid(min, max, &surface()).unwrap()
}

/// The solid's volume, by the divergence theorem over its faces' polygons.
fn volume(brush: &Brush) -> f64 {
    let mut total = 0.0;
    for (face, polygon) in brush.faces.iter().zip(brush.polygons()) {
        let normal = face.plane().unwrap().normal;
        let origin = polygon[0];
        let mut area = [0.0; 3];
        for pair in polygon.windows(2).skip(1) {
            let a = [
                pair[0][0] - origin[0],
                pair[0][1] - origin[1],
                pair[0][2] - origin[2],
            ];
            let b = [
                pair[1][0] - origin[0],
                pair[1][1] - origin[1],
                pair[1][2] - origin[2],
            ];
            area[0] += a[1] * b[2] - a[2] * b[1];
            area[1] += a[2] * b[0] - a[0] * b[2];
            area[2] += a[0] * b[1] - a[1] * b[0];
        }
        let area = (area[0] * area[0] + area[1] * area[1] + area[2] * area[2]).sqrt() / 2.0;
        let height = origin[0] * normal[0] + origin[1] * normal[1] + origin[2] * normal[2];
        total += area * height / 3.0;
    }
    total
}

fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

// spec MAP-3.4: a box is the solid behind its six faces, each facing out, with its eight corners
#[test]
fn a_box_is_a_valid_brush_facing_out() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 32.0, 16.0]);
    assert!(brush.is_valid());
    assert_eq!(brush.faces.len(), 6);
    assert_eq!(brush.vertices().len(), 8);
    assert_eq!(round(volume(&brush)), 64.0 * 32.0 * 16.0);
    let inside = [32.0, 16.0, 8.0];
    for face in &brush.faces {
        let p = face.plane().unwrap();
        let side = inside[0] * p.normal[0] + inside[1] * p.normal[1] + inside[2] * p.normal[2];
        assert!(side < p.dist, "the middle is behind every face");
    }
    assert!(matches!(
        Brush::cuboid([0.0; 3], [0.0, 8.0, 8.0], &surface()),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-3.4: a brush with a face no polygon is left on, a duplicate plane or an open side is
// not valid
#[test]
fn a_brush_with_a_stray_or_doubled_face_is_not_valid() {
    let mut doubled = cuboid([0.0; 3], [8.0; 3]);
    let first = doubled.faces[0].clone();
    doubled.faces.push(first);
    assert!(!doubled.is_valid());
    let mut open = cuboid([0.0; 3], [8.0; 3]);
    open.faces.pop();
    assert!(!open.is_valid());
    let mut stray = cuboid([0.0; 3], [8.0; 3]);
    let outside = Brush::cuboid([100.0; 3], [108.0; 3], &surface()).unwrap();
    stray.faces.push(outside.faces[0].clone());
    assert!(!stray.is_valid());
}

/// The plane x = `x`, facing +x.
fn x_plane(x: f64) -> Plane<f64> {
    Plane {
        normal: [1.0, 0.0, 0.0],
        dist: x,
    }
}

// No spec statement: P6-08: a clip by a plane gives the parts behind and in front of it, which
// add up to the brush; a plane that misses it leaves it whole on one side
#[test]
fn a_clip_splits_a_brush_in_two() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    let (back, front) = brush.split(x_plane(16.0), &surface()).unwrap();
    let (back, front) = (back.unwrap(), front.unwrap());
    assert!(back.is_valid() && front.is_valid());
    assert_eq!(round(volume(&back)), 16.0 * 64.0 * 64.0);
    assert_eq!(round(volume(&front)), 48.0 * 64.0 * 64.0);
    let (whole, none) = brush.split(x_plane(100.0), &surface()).unwrap();
    assert_eq!(whole, Some(brush.clone()));
    assert_eq!(none, None);
    let (none, whole) = brush.split(x_plane(-1.0), &surface()).unwrap();
    assert_eq!((none, whole), (None, Some(brush)));
}

// No spec statement: P6-08: subtracting a brush leaves the parts outside it; a cutter that
// misses leaves the brush as it was, and one that holds it leaves nothing
#[test]
fn subtracting_leaves_what_is_outside_the_cutter() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    let cutter = cuboid([32.0, 32.0, -8.0], [96.0, 96.0, 72.0]);
    let pieces = brush.subtract(&cutter).unwrap();
    assert!(pieces.iter().all(Brush::is_valid));
    let left: f64 = pieces.iter().map(volume).sum();
    assert_eq!(round(left), 64.0 * 64.0 * 64.0 - 32.0 * 32.0 * 64.0);
    let away = cuboid([200.0; 3], [208.0; 3]);
    assert_eq!(brush.subtract(&away).unwrap(), vec![brush.clone()]);
    let around = cuboid([-8.0; 3], [72.0; 3]);
    assert!(brush.subtract(&around).unwrap().is_empty());
    let mut broken = cutter.clone();
    broken.faces.pop();
    assert!(matches!(
        brush.subtract(&broken),
        Err(BrushError::Degenerate)
    ));
}

// No spec statement: P6-08: merging brushes gives their convex hull, painted as their faces
#[test]
fn merging_gives_the_convex_hull() {
    let a = cuboid([0.0; 3], [64.0; 3]);
    let b = cuboid([64.0, 0.0, 0.0], [128.0, 64.0, 64.0]);
    let merged = a.merge(&[&b]).unwrap();
    assert!(merged.is_valid());
    assert_eq!(merged.faces.len(), 6);
    assert_eq!(round(volume(&merged)), 128.0 * 64.0 * 64.0);
    assert!(merged.faces.iter().all(|f| f.texture() == b"rock"));
    // Two boxes apart merge into one hull spanning both, with the gap.
    let far = cuboid([0.0, 0.0, 128.0], [64.0, 64.0, 192.0]);
    assert_eq!(
        round(volume(&a.merge(&[&far]).unwrap())),
        64.0 * 64.0 * 192.0
    );
}

// No spec statement: P6-08: a vertex moved outward makes a new convex brush with it as a corner;
// one moved inside, or a vertex the brush has not, is refused
#[test]
fn a_vertex_moves_and_the_brush_stays_convex() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    let moved = brush
        .move_vertex([64.0, 64.0, 64.0], [64.0, 64.0, 96.0])
        .unwrap();
    assert!(moved.is_valid());
    assert!(moved.vertices().contains(&[64.0, 64.0, 96.0]));
    assert!(volume(&moved) > volume(&brush));
    assert!(matches!(
        brush.move_vertex([1.0, 2.0, 3.0], [0.0; 3]),
        Err(BrushError::NoVertex(_))
    ));
    assert!(matches!(
        brush.move_vertex([64.0, 64.0, 64.0], [32.0, 32.0, 32.0]),
        Err(BrushError::VertexInside(_))
    ));
}

// No spec statement: P6-08: the faces of every new brush are written with three of its corners,
// so each lies on its plane
#[test]
fn new_faces_lie_on_their_planes() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    let tilted = plane([[0.0, 0.0, 48.0], [64.0, 0.0, 16.0], [0.0, 64.0, 48.0]]).unwrap();
    let (back, _) = brush.split(tilted, &surface()).unwrap();
    let back = back.unwrap();
    for face in &back.faces {
        let p = face.plane().unwrap();
        for corner in back.vertices() {
            let side = corner[0] * p.normal[0] + corner[1] * p.normal[1] + corner[2] * p.normal[2];
            assert!(side <= p.dist + 0.01, "every corner is behind every face");
        }
    }
}

// No spec statement: P6-08: a tetrahedron, four triangular faces, is the least valid brush
#[test]
fn a_tetrahedron_is_valid() {
    let corners = [
        [0.0; 3],
        [64.0, 0.0, 0.0],
        [0.0, 64.0, 0.0],
        [0.0, 0.0, 64.0],
    ];
    let brush = Brush::hull(&corners, &[], &surface()).unwrap();
    assert!(brush.is_valid());
    assert_eq!(brush.faces.len(), 4);
    assert_eq!(brush.vertices().len(), 4);
    assert_eq!(round(volume(&brush)), round(64.0 * 64.0 * 64.0 / 6.0));
}

// No spec statement: P6-08: a box whose corners are given the wrong way round on any axis is
// refused, not turned the right way out
#[test]
fn an_inside_out_box_is_refused() {
    for axis in 0..3 {
        let mut min = [0.0; 3];
        let mut max = [64.0; 3];
        min[axis] = 64.0;
        max[axis] = 0.0;
        assert!(
            matches!(
                Brush::cuboid(min, max, &surface()),
                Err(BrushError::Degenerate)
            ),
            "axis {axis}"
        );
    }
}

// No spec statement: P6-08: a plane touching a face of the brush leaves it whole on one side
#[test]
fn a_touching_plane_leaves_the_brush_whole() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    let (none, whole) = brush.split(x_plane(0.0), &surface()).unwrap();
    assert_eq!((none, whole), (None, Some(brush.clone())));
    let (whole, none) = brush.split(x_plane(64.0), &surface()).unwrap();
    assert_eq!((whole, none), (Some(brush), None));
}

// No spec statement: P6-08: a vertex move that would leave another vertex inside the brush or
// on a face is refused
#[test]
fn a_vertex_move_that_loses_a_corner_is_refused() {
    let brush = cuboid([0.0; 3], [64.0; 3]);
    assert!(matches!(
        brush.move_vertex([64.0, 64.0, 64.0], [0.0, 0.0, 128.0]),
        Err(BrushError::CornerLost(_))
    ));
}

/// A Valve 220 surface with axes `s` and `t`, shifted by 8 and 4.
fn valve(s: [f64; 3], t: [f64; 3]) -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Valve {
            s: [s[0], s[1], s[2], 8.0],
            t: [t[0], t[1], t[2], 4.0],
            rotate: 45.0,
            scale: [1.0, 1.0],
        },
    }
}

/// A Valve 220 surface whose axes lie in the floor, turned 45 degrees from its paraxial ones.
fn floor_valve() -> Surface {
    let half = core::f64::consts::FRAC_1_SQRT_2;
    valve([half, half, 0.0], [half, -half, 0.0])
}

// spec MAP-4.2: a new face painted with Valve 220 axes that do not lie in its plane takes its
// plane's paraxial axes, keeping the shifts; axes that lie in it are kept
#[test]
fn valve_axes_fit_each_new_face() {
    let brush = Brush::cuboid([0.0; 3], [64.0; 3], &floor_valve()).unwrap();
    for face in &brush.faces {
        let normal = face.plane().unwrap().normal;
        let TexDef::Valve { s, t, .. } = *face.texdef() else {
            panic!("a Valve face");
        };
        let along =
            |axis: [f64; 4]| axis[0] * normal[0] + axis[1] * normal[1] + axis[2] * normal[2];
        assert_eq!((along(s), along(t)), (0.0, 0.0), "{normal:?}");
        assert_eq!((s[3], t[3]), (8.0, 4.0));
        if normal[2] != 0.0 {
            assert_eq!(
                *face.texdef(),
                floor_valve().texdef,
                "the floor and ceiling keep theirs"
            );
        }
        if normal == [1.0, 0.0, 0.0] {
            assert_eq!(
                *face.texdef(),
                valve([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]).texdef,
                "the west wall row of the base axes, rotation and scale kept"
            );
        }
    }
    // An axis leaning out of the plane by exactly IN_PLANE does not lie in it.
    let leaning = valve([1.0, 0.0, super::IN_PLANE], [0.0, -1.0, 0.0]);
    let brush = Brush::cuboid([0.0; 3], [64.0; 3], &leaning).unwrap();
    let top = brush
        .faces
        .iter()
        .find(|face| face.plane().unwrap().normal == [0.0, 0.0, 1.0])
        .unwrap();
    assert_eq!(
        *top.texdef(),
        valve([1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).texdef
    );
}

// No spec statement: P6-08: a brush reaching half the reach of a base polygon is not taken as
// bounded, one just inside it is
#[test]
fn a_brush_at_the_reach_is_not_bounded() {
    let half = super::POLYGON_REACH / 2.0;
    assert!(Brush::cuboid([0.0; 3], [half - 8.0, 8.0, 8.0], &surface()).is_ok());
    assert!(matches!(
        Brush::cuboid([0.0; 3], [half, 8.0, 8.0], &surface()),
        Err(BrushError::Degenerate)
    ));
}

// No spec statement: P6-08: a face a brush edit cannot write is a map error, its cause kept
#[test]
fn an_unwritable_face_is_a_map_error() {
    let mut zero = floor_valve();
    zero.texdef = TexDef::Valve {
        s: [1.0, 0.0, 0.0, 0.0],
        t: [0.0, -1.0, 0.0, 0.0],
        rotate: 0.0,
        scale: [0.0, 1.0],
    };
    let error = Brush::cuboid([0.0; 3], [8.0; 3], &zero).unwrap_err();
    assert!(matches!(error, BrushError::Map(_)));
    assert!(std::error::Error::source(&error).is_some());
    assert!(std::error::Error::source(&BrushError::Missed).is_none());
}

// No spec statement: P6-08: each brush error says what went wrong
#[test]
fn brush_errors_are_described() {
    let cases = [
        (BrushError::Degenerate, "no volume"),
        (
            BrushError::NoVertex([1.0, 2.0, 3.0]),
            "no vertex at [1.0, 2.0, 3.0]",
        ),
        (BrushError::VertexInside([4.0; 3]), "inside the brush"),
        (
            BrushError::CornerLost([5.0; 3]),
            "take away the vertex at [5.0, 5.0, 5.0]",
        ),
        (BrushError::Missed, "misses the brush"),
    ];
    for (error, text) in cases {
        assert!(error.to_string().contains(text), "{error}");
    }
}

// No spec statement: P6-08: two points nearer than SAME_POINT are one vertex, and points that
// far apart are two; a polygon left with two corners has no face points
#[test]
fn near_points_are_one_vertex() {
    let origin = [0.0; 3];
    assert!(same_point(origin, origin));
    assert!(same_point(origin, [SAME_POINT / 2.0, 0.0, 0.0]));
    assert!(!same_point(origin, [SAME_POINT, 0.0, 0.0]));
    let sliver = [origin, [SAME_POINT / 10.0, 0.0, 0.0], [0.0, 64.0, 0.0]];
    assert_eq!(face_points(&sliver, [0.0, 0.0, 1.0]), None);
}

// No spec statement: P6-08: a rounded coordinate is never written as -0
#[test]
fn rounding_drops_negative_zero() {
    assert!(super::round(-1e-9).is_sign_positive());
    assert_eq!(super::round(1.234_567_89), 1.234_568);
}

// No spec statement: P6-08: two planes are one only when they face the same way and lie less
// than ON_PLANE apart
#[test]
fn planes_are_the_same_within_the_tolerances() {
    let x = x_plane(0.0);
    assert!(x.same(&x_plane(ON_PLANE / 2.0)));
    assert!(!x.same(&x_plane(ON_PLANE)));
    let tilted = Plane {
        normal: [SAME_NORMAL, 0.0, 0.0],
        dist: 0.0,
    };
    assert!(!x.same(&tilted));
}

// No spec statement: P6-08: an entity's brushes are numbered by their places among its items
#[test]
fn brushes_are_found_among_an_entitys_items() {
    let mut file = MapFile::parse(b"{\n\"classname\" \"func_wall\"\n}\n").unwrap();
    let entity = &mut file.entities[0];
    assert!(entity.brush_places().is_empty());
    entity.items.push(Item::Brush(cuboid([0.0; 3], [8.0; 3])));
    entity.items.push(Item::Brush(cuboid([16.0; 3], [24.0; 3])));
    assert_eq!(entity.brush_places(), vec![1, 2]);
}

proptest! {
    // No spec statement: P6-08: for any two boxes, the pieces of a subtraction are valid and
    // their volume is the first box's less the overlap
    #[test]
    fn subtraction_keeps_the_volume_outside(
        a in prop::array::uniform3(-64_i32..64),
        size in prop::array::uniform3(8_i32..96),
        b in prop::array::uniform3(-64_i32..64),
        size_b in prop::array::uniform3(8_i32..96),
    ) {
        let min = a.map(f64::from);
        let max = [0, 1, 2].map(|i| f64::from(a[i]) + f64::from(size[i]));
        let bmin = b.map(f64::from);
        let bmax = [0, 1, 2].map(|i| f64::from(b[i]) + f64::from(size_b[i]));
        let brush = cuboid(min, max);
        let cutter = cuboid(bmin, bmax);
        let pieces = brush.subtract(&cutter).unwrap();
        prop_assert!(pieces.iter().all(Brush::is_valid));
        let overlap: f64 = [0, 1, 2]
            .map(|i| (max[i].min(bmax[i]) - min[i].max(bmin[i])).max(0.0))
            .iter()
            .product();
        let left: f64 = pieces.iter().map(volume).sum();
        prop_assert!((left - (volume(&brush) - overlap)).abs() < 0.5, "{left} {overlap}");
    }

    // No spec statement: P6-08: for any box and plane through it, the two parts are valid and
    // add up to the box
    #[test]
    fn a_split_keeps_the_volume(
        size in prop::array::uniform3(8_i32..128),
        normal in prop::array::uniform3(-8_i32..8),
        at in 1_i32..7,
    ) {
        prop_assume!(normal != [0, 0, 0]);
        let max = size.map(f64::from);
        let brush = cuboid([0.0; 3], max);
        let n = normal.map(f64::from);
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let n = n.map(|c| c / length);
        let middle = [max[0] * f64::from(at) / 8.0, max[1] / 2.0, max[2] / 2.0];
        let cut = Plane { normal: n, dist: middle[0] * n[0] + middle[1] * n[1] + middle[2] * n[2] };
        let (back, front) = brush.split(cut, &surface()).unwrap();
        let parts: f64 = back.iter().chain(front.iter()).map(volume).sum();
        prop_assert!(back.iter().chain(front.iter()).all(Brush::is_valid));
        prop_assert!((parts - volume(&brush)).abs() < 1.0, "{parts}");
    }
}

/// Each face's texture coordinates of its three points.
fn coordinates(brush: &Brush) -> Vec<[f64; 6]> {
    brush
        .faces
        .iter()
        .map(|face| {
            let [s, t] = face.texvecs().unwrap();
            let at =
                |row: [f64; 4], p: [f64; 3]| row[0] * p[0] + row[1] * p[1] + row[2] * p[2] + row[3];
            let [a, b, c] = *face.points();
            [at(s, a), at(t, a), at(s, b), at(t, b), at(s, c), at(t, c)]
        })
        .collect()
}

/// A standard surface turned and scaled.
fn turned() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [3.0, -5.0],
            rotate: 30.0,
            scale: [0.5, 2.0],
        },
    }
}

// spec MAP-11.1: a moved brush's points move by the offset, its textures kept
// spec MAP-11.2: with texture lock off the definitions are unchanged
#[test]
fn a_brush_moves_by_its_offset() {
    let brush = Brush::cuboid([0.0; 3], [64.0, 32.0, 16.0], &turned()).unwrap();
    let moved = brush
        .translated([16.0, -32.0, 8.0], TextureLock::Free)
        .unwrap();
    assert_eq!(moved.faces.len(), brush.faces.len());
    for (before, after) in brush.faces.iter().zip(&moved.faces) {
        for (p, q) in before.points().iter().zip(after.points()) {
            assert_eq!([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [16.0, -32.0, 8.0]);
        }
        assert_eq!(before.texdef(), after.texdef());
        assert_eq!(before.texture(), after.texture());
    }
}

proptest! {
    // spec MAP-11.2: with texture lock on every point of a moved face keeps its texture
    // coordinates, for standard and Valve 220 faces
    #[test]
    fn texture_lock_keeps_textures_fixed_in_the_world(
        by in prop::array::uniform3(-512.0..512.0_f64),
        rotate in -360.0..360.0_f64,
        valve_face in any::<bool>(),
    ) {
        let surface = if valve_face {
            valve([1.0, 0.0, 0.0], [0.0, -1.0, 0.0])
        } else {
            Surface {
                texture: b"rock".to_vec(),
                texdef: TexDef::Standard { shift: [7.0, 1.0], rotate, scale: [0.25, 3.0] },
            }
        };
        let brush = Brush::cuboid([0.0; 3], [64.0, 32.0, 16.0], &surface).unwrap();
        let moved = brush.translated(by, TextureLock::Locked).unwrap();
        for (before, after) in coordinates(&brush).iter().zip(coordinates(&moved)) {
            for (b, a) in before.iter().zip(after) {
                prop_assert!((b - a).abs() < 1e-6, "{before:?} {after:?}");
            }
        }
    }
}
