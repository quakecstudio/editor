// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unreachable,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{Justify, TexAxis, TextureSize, UvEdit};
use crate::geometry::paraxial_axes;
use crate::{Brush, Face, Item, MapFile, Surface, TexDef};

const NEAR: f64 = 1e-6;

/// Where a justified texture's edge is, from an axis's least and greatest texel.
type Edge = fn([f64; 2]) -> f64;

const STANDARD: TexDef = TexDef::Standard {
    shift: [5.0, -3.0],
    rotate: 30.0,
    scale: [0.5, -2.0],
};

const VALVE: TexDef = TexDef::Valve {
    s: [1.0, 0.0, 0.0, 7.0],
    t: [0.0, -1.0, 0.0, 2.0],
    rotate: 0.0,
    scale: [0.25, 1.5],
};

fn cuboid(texdef: TexDef) -> Brush {
    let surface = Surface {
        texture: b"rock".to_vec(),
        texdef,
    };
    Brush::cuboid([-16.0, 8.0, 0.0], [48.0, 40.0, 24.0], &surface).unwrap()
}

/// The texels of `point` on `face` with its definition `texdef`.
fn texels(face: &Face, texdef: TexDef, point: [f64; 3]) -> [f64; 2] {
    let mut face = face.clone();
    let points = *face.points();
    face.set(points, b"rock", texdef).unwrap();
    face.texvecs()
        .unwrap()
        .map(|axis| point[0] * axis[0] + point[1] * axis[1] + point[2] * axis[2] + axis[3])
}

/// The least and greatest texel of `polygon` along each axis.
fn extent(face: &Face, texdef: TexDef, polygon: &[[f64; 3]]) -> [[f64; 2]; 2] {
    let mut out = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for &point in polygon {
        for (range, at) in out.iter_mut().zip(texels(face, texdef, point)) {
            *range = [range[0].min(at), range[1].max(at)];
        }
    }
    out
}

/// Each face of `brush` with its polygon.
fn faces(brush: &Brush) -> Vec<(&Face, Vec<[f64; 3]>)> {
    brush
        .faces
        .iter()
        .zip(brush.face_polygons())
        .map(|(face, polygon)| (face, polygon.unwrap()))
        .collect()
}

fn on_multiple(value: f64, of: f64) -> bool {
    let off = value.rem_euclid(of);
    off < NEAR || of - off < NEAR
}

// spec MAP-13.1: each face of a brush has its polygon, in face order; a face with no
// plane has none
#[test]
fn every_face_has_its_polygon() {
    let mut brush = cuboid(STANDARD);
    let polygons = brush.face_polygons();
    assert_eq!(polygons.len(), brush.faces.len());
    for (face, polygon) in brush.faces.iter().zip(&polygons) {
        let plane = face.plane().unwrap();
        let polygon = polygon.as_ref().unwrap();
        assert_eq!(polygon.len(), 4);
        for p in polygon {
            let side = p[0] * plane.normal[0] + p[1] * plane.normal[1] + p[2] * plane.normal[2];
            assert!((side - plane.dist).abs() < NEAR);
        }
    }
    let flat = brush.faces[0].clone();
    let point = flat.points()[0];
    let mut collinear = flat;
    collinear
        .set([point, point, point], b"rock", STANDARD)
        .unwrap();
    brush.faces.insert(0, collinear);
    let polygons = brush.face_polygons();
    assert!(polygons[0].is_none());
    assert!(polygons[1..].iter().all(Option::is_some));
    let face = &brush.faces[0];
    assert_eq!(
        face.uv_edited(&[], UvEdit::Fit, TextureSize::new(8, 8)),
        None
    );
    assert_eq!(face.aligned_from(&brush.faces[1]), None);
    assert_eq!(brush.faces[1].aligned_from(face), None);
}

// spec MAP-13.2: fit makes one copy of the texture span the face exactly, for textures
// larger and smaller than the face, keeping the scale's sign
#[test]
fn fit_spans_the_face() {
    for texdef in [STANDARD, VALVE] {
        let brush = cuboid(texdef);
        for (face, polygon) in faces(&brush) {
            let texdef = *face.texdef();
            for (w, h) in [(128, 256), (16, 8), (64, 32)] {
                let fitted = face
                    .uv_edited(&polygon, UvEdit::Fit, TextureSize::new(w, h))
                    .unwrap();
                let [s, t] = extent(face, fitted, &polygon);
                for ([lo, hi], size) in [(s, f64::from(w)), (t, f64::from(h))] {
                    if (hi - lo).abs() < NEAR {
                        continue;
                    }
                    assert!((hi - lo - size).abs() < NEAR, "{fitted:?} {lo} {hi} {size}");
                    assert!(on_multiple(lo, size), "{fitted:?} starts at {lo}");
                }
                let scale = |texdef: TexDef| match texdef {
                    TexDef::Standard { scale, .. } | TexDef::Valve { scale, .. } => scale,
                };
                for (old, new) in scale(texdef).into_iter().zip(scale(fitted)) {
                    assert_eq!(old.signum(), new.signum());
                }
            }
        }
    }
}

// spec MAP-13.2: an axis along which the face has no extent, or a texture of no size, is kept
#[test]
fn fit_keeps_what_it_cannot_fit() {
    let brush = cuboid(STANDARD);
    let (face, polygon) = &faces(&brush)[0];
    let point = polygon[0];
    assert_eq!(
        face.uv_edited(&[point], UvEdit::Fit, TextureSize::new(64, 64)),
        Some(STANDARD)
    );
    assert_eq!(
        face.uv_edited(polygon, UvEdit::Fit, TextureSize::new(0, 0)),
        Some(STANDARD)
    );
    assert_eq!(
        face.uv_edited(
            &[],
            UvEdit::Justify(Justify::Centre),
            TextureSize::new(8, 8)
        ),
        Some(STANDARD)
    );
}

// spec MAP-13.3, MAP-13.7: justify puts the texture's edge on the face's edge, or its centre
// on the centre, changing only the shift, which it keeps within one copy of the texture
#[test]
fn justify_meets_each_edge() {
    let size = TextureSize::new(64, 16);
    for texdef in [STANDARD, VALVE] {
        let brush = cuboid(texdef);
        for (face, polygon) in faces(&brush) {
            let texdef = *face.texdef();
            let [s0, t0] = extent(face, texdef, &polygon);
            let cases: [(Justify, usize, Edge); 4] = [
                (Justify::Left, 0, |[lo, _]| lo),
                (Justify::Right, 0, |[_, hi]| hi - 64.0),
                (Justify::Top, 1, |[lo, _]| lo),
                (Justify::Bottom, 1, |[_, hi]| hi - 16.0),
            ];
            for (to, axis, wanted) in cases {
                let justified = face.uv_edited(&polygon, UvEdit::Justify(to), size).unwrap();
                let ranges = extent(face, justified, &polygon);
                let length = [64.0, 16.0][axis];
                assert!(
                    on_multiple(wanted(ranges[axis]), length),
                    "{to:?} {ranges:?}"
                );
                let other = 1 - axis;
                let before = [s0, t0][other];
                assert!(
                    (ranges[other][0] - before[0]).abs() < NEAR,
                    "{to:?} moved the other axis"
                );
                let shift = match justified {
                    TexDef::Standard { shift, .. } => shift,
                    TexDef::Valve { s, t, .. } => [s[3], t[3]],
                };
                assert!((0.0..length).contains(&shift[axis]), "{shift:?}");
            }
            let centred = face
                .uv_edited(&polygon, UvEdit::Justify(Justify::Centre), size)
                .unwrap();
            let [s, t] = extent(face, centred, &polygon);
            assert!(on_multiple((s[0] + s[1]) / 2.0 - 32.0, 64.0), "{s:?}");
            assert!(on_multiple((t[0] + t[1]) / 2.0 - 8.0, 16.0), "{t:?}");
        }
    }
}

// spec MAP-13.4: flipping negates one axis's scale and keeps the texel at the face's centre
#[test]
fn flip_mirrors_about_the_centre() {
    for texdef in [STANDARD, VALVE] {
        let brush = cuboid(texdef);
        for (face, polygon) in faces(&brush) {
            let texdef = *face.texdef();
            let [s, t] = extent(face, texdef, &polygon);
            let centre = polygon.iter().fold([0.0; 3], |sum, p| {
                [
                    sum[0] + p[0] / 4.0,
                    sum[1] + p[1] / 4.0,
                    sum[2] + p[2] / 4.0,
                ]
            });
            for axis in [TexAxis::S, TexAxis::T] {
                let flipped = face
                    .uv_edited(&polygon, UvEdit::Flip(axis), TextureSize::new(1, 1))
                    .unwrap();
                let before = texels(face, texdef, centre);
                let after = texels(face, flipped, centre);
                assert!((before[0] - after[0]).abs() < NEAR && (before[1] - after[1]).abs() < NEAR);
                let [fs, ft] = extent(face, flipped, &polygon);
                let i = axis.index();
                assert!(((fs[1] - fs[0]) - (s[1] - s[0])).abs() < NEAR);
                assert!(((ft[1] - ft[0]) - (t[1] - t[0])).abs() < NEAR);
                let corner = polygon[0];
                let (b, a) = (texels(face, texdef, corner), texels(face, flipped, corner));
                assert!((b[i] + a[i] - 2.0 * before[i]).abs() < NEAR, "{b:?} {a:?}");
                assert!((b[1 - i] - a[1 - i]).abs() < NEAR);
                let twice = {
                    let mut face = face.clone();
                    let points = *face.points();
                    face.set(points, b"rock", flipped).unwrap();
                    face.uv_edited(&polygon, UvEdit::Flip(axis), TextureSize::new(1, 1))
                        .unwrap()
                };
                let back = texels(face, twice, corner);
                assert!((back[0] - b[0]).abs() < NEAR && (back[1] - b[1]).abs() < NEAR);
            }
        }
    }
}

// spec MAP-13.5: reset clears the shift, rotation and scaling; a Valve 220 face takes its
// plane's paraxial axes
#[test]
fn reset_clears_the_alignment() {
    let size = TextureSize::new(64, 64);
    let brush = cuboid(STANDARD);
    let (face, polygon) = &faces(&brush)[0];
    assert_eq!(
        face.uv_edited(polygon, UvEdit::Reset, size),
        Some(TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        })
    );
    let brush = cuboid(TexDef::Valve {
        s: [0.0, 0.6, 0.8, 3.0],
        t: [0.0, 0.8, -0.6, 4.0],
        rotate: 12.0,
        scale: [2.0, 2.0],
    });
    for (face, polygon) in faces(&brush) {
        let normal = face.plane().unwrap().normal;
        let [s, t] = paraxial_axes(normal);
        assert_eq!(
            face.uv_edited(&polygon, UvEdit::Reset, size),
            Some(TexDef::Valve {
                s: [s[0], s[1], s[2], 0.0],
                t: [t[0], t[1], t[2], 0.0],
                rotate: 0.0,
                scale: [1.0; 2],
            })
        );
    }
}

// spec MAP-13.6: quick snap moves the shift the least that puts a corner on a texture edge,
// along each axis
#[test]
fn snap_puts_a_corner_on_an_edge() {
    let size = TextureSize::new(64, 16);
    for texdef in [STANDARD, VALVE] {
        let brush = cuboid(texdef);
        for (face, polygon) in faces(&brush) {
            let texdef = *face.texdef();
            let snapped = face.uv_edited(&polygon, UvEdit::Snap, size).unwrap();
            let before: Vec<[f64; 2]> = polygon.iter().map(|&p| texels(face, texdef, p)).collect();
            let after: Vec<[f64; 2]> = polygon.iter().map(|&p| texels(face, snapped, p)).collect();
            for (axis, length) in [(0, 64.0), (1, 16.0)] {
                assert!(
                    after.iter().any(|at| on_multiple(at[axis], length)),
                    "{after:?}"
                );
                let least = before
                    .iter()
                    .map(|at| {
                        let to = (at[axis] / length).round() * length - at[axis];
                        to.abs()
                    })
                    .fold(f64::INFINITY, f64::min);
                let moved = after[0][axis] - before[0][axis];
                assert!(
                    on_multiple(moved.abs() - least, length)
                        || on_multiple(moved.abs() + least, length)
                );
            }
        }
    }
}

/// The two points the faces `a` and `b` of `brush` share, if they meet at an edge.
fn shared_edge(brush: &Brush, a: usize, b: usize) -> Option<[[f64; 3]; 2]> {
    let polygons = brush.face_polygons();
    let (pa, pb) = (polygons[a].as_ref()?, polygons[b].as_ref()?);
    let near = |p: [f64; 3], q: [f64; 3]| (0..3).all(|i| (p[i] - q[i]).abs() < NEAR);
    let shared: Vec<[f64; 3]> = pa
        .iter()
        .copied()
        .filter(|&p| pb.iter().any(|&q| near(p, q)))
        .collect();
    (shared.len() == 2).then(|| [shared[0], shared[1]])
}

/// Asserts that every point of the edge has the same texels on both faces.
fn no_seam(from: &Face, to: &Face, texdef: TexDef, [a, b]: [[f64; 3]; 2]) {
    for k in [0.0, 0.25, 0.5, 1.0] {
        let p = [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * k);
        let want = texels(from, *from.texdef(), p);
        let got = texels(to, texdef, p);
        assert!(
            (want[0] - got[0]).abs() < NEAR && (want[1] - got[1]).abs() < NEAR,
            "{want:?} {got:?} at {p:?} with {texdef:?}"
        );
    }
}

fn wedge(texdef: TexDef) -> Brush {
    let mut brush = cuboid(texdef);
    let cut = Face::new(
        [[48.0, 8.0, 8.0], [48.0, 40.0, 8.0], [0.0, 8.0, 24.0]],
        b"rock",
        texdef,
    )
    .unwrap();
    brush.faces.push(cut);
    brush
}

proptest! {
    // spec MAP-13.8: alignment carried across a corner leaves no seam, in
    // the standard and the Valve 220 form, from either form, on square and slanted corners
    #[test]
    fn alignment_carries_across_corners(
        shift in prop::array::uniform2(-64.0..64.0_f64),
        rotate in 0.0..360.0_f64,
        scale in prop::array::uniform2(prop_oneof![0.25..4.0_f64, -4.0..-0.25_f64]),
        valve_source in any::<bool>(),
        valve_target in any::<bool>(),
        slanted in any::<bool>(),
    ) {
        let standard = TexDef::Standard { shift, rotate, scale };
        let target_def = if valve_target { VALVE } else { STANDARD };
        let brush = if slanted { wedge(target_def) } else { cuboid(target_def) };
        let mut met = false;
        for a in 0..brush.faces.len() {
            let mut from = brush.faces[a].clone();
            let points = *from.points();
            let source_def = if valve_source {
                VALVE.reset(from.plane().unwrap().normal).aligned(crate::Alignment {
                    shift: Some(shift),
                    rotate: Some(rotate),
                    scale: Some(scale),
                })
            } else {
                standard
            };
            if from.set(points, b"rock", source_def).is_err() {
                continue;
            }
            if from.texvecs().is_none() {
                continue;
            }
            for b in 0..brush.faces.len() {
                let Some(edge) = shared_edge(&brush, a, b) else { continue };
                if a == b {
                    continue;
                }
                let to = &brush.faces[b];
                let carried = to.aligned_from(&from).unwrap();
                prop_assert_eq!(
                    core::mem::discriminant(&carried),
                    core::mem::discriminant(to.texdef()),
                    "the target keeps its form"
                );
                no_seam(&from, to, carried, edge);
                met = true;
            }
        }
        prop_assert!(met);
    }
}

// spec MAP-13.9: between parallel planes a standard source's definition is copied, and a
// Valve 220 target takes the source's axes, so the texture lines up across the two
#[test]
fn parallel_faces_take_the_source_definition() {
    let brush = cuboid(STANDARD);
    let floor = &brush.faces[0];
    let mut other = brush.faces[0].clone();
    let points = other.points().map(|p| [p[0] + 128.0, p[1], p[2]]);
    other
        .set(
            points,
            b"rock",
            TexDef::Standard {
                shift: [0.0; 2],
                rotate: 0.0,
                scale: [1.0; 2],
            },
        )
        .unwrap();
    assert_eq!(other.aligned_from(floor), Some(STANDARD));
    let mut valve = other.clone();
    valve.set(points, b"rock", VALVE).unwrap();
    let carried = valve.aligned_from(floor).unwrap();
    let p = [200.0, 17.0, points[0][2]];
    let (want, got) = (texels(floor, STANDARD, p), texels(&valve, carried, p));
    assert!((want[0] - got[0]).abs() < NEAR && (want[1] - got[1]).abs() < NEAR);
    let mut standard = other;
    standard.set(points, b"rock", STANDARD).unwrap();
    let source = {
        let mut source = floor.clone();
        let points = *source.points();
        source.set(points, b"rock", VALVE).unwrap();
        source
    };
    let carried = standard.aligned_from(&source).unwrap();
    let line = super::Line::within(&standard.plane().unwrap());
    let along = |k: f64| [0, 1, 2].map(|i| line.point[i] + line.along[i] * k);
    for k in [0.0, 10.0, -33.0] {
        let p = along(k);
        let (want, got) = (texels(&source, VALVE, p), texels(&standard, carried, p));
        assert!((want[0] - got[0]).abs() < NEAR && (want[1] - got[1]).abs() < NEAR);
    }
}

// spec MAP-13.10: every UV edit is written and read back unchanged, in either form
#[test]
fn edits_round_trip_through_the_file() {
    let size = TextureSize::new(64, 32);
    for texdef in [STANDARD, VALVE] {
        let mut file = MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
        file.entities[0].items.push(Item::Brush(cuboid(texdef)));
        let source = cuboid(texdef).faces[0].clone();
        let edits = [
            UvEdit::Fit,
            UvEdit::Justify(Justify::Right),
            UvEdit::Justify(Justify::Centre),
            UvEdit::Flip(TexAxis::T),
            UvEdit::Snap,
            UvEdit::Reset,
        ];
        for edit in edits {
            let mut edited = file.clone();
            let Some(Item::Brush(brush)) = edited.entities[0].items.last_mut() else {
                unreachable!("a brush was pushed")
            };
            let polygons = brush.face_polygons();
            let mut wanted = Vec::new();
            for (face, polygon) in brush.faces.iter_mut().zip(polygons) {
                let texdef = face.uv_edited(&polygon.unwrap(), edit, size).unwrap();
                let points = *face.points();
                face.set(points, b"rock", texdef).unwrap();
                wanted.push(texdef);
            }
            let carried = brush.faces[1].aligned_from(&source).unwrap();
            let points = *brush.faces[1].points();
            brush.faces[1].set(points, b"rock", carried).unwrap();
            wanted[1] = carried;
            let read = MapFile::parse(&edited.to_bytes()).unwrap();
            let read: Vec<TexDef> = read
                .entities
                .iter()
                .flat_map(crate::Entity::brushes)
                .flat_map(|brush| brush.faces.iter().map(|face| *face.texdef()))
                .collect();
            assert_eq!(read, wanted, "{edit:?}");
        }
    }
}
