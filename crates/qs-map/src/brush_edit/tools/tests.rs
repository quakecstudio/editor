// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index, is a failed test; the \
              normals compared are exact and the counts small"
)]

use proptest::prelude::*;

use super::{Affine, BrushFace, MAP_MAX_CHAMFER_FACES, WorldAxis, WorldTexture};
use crate::brush_edit::{BrushError, Surface, TextureLock};
use crate::geometry::{MAP_ON_EPSILON, plane};
use crate::{Brush, TexDef};

fn standard() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [3.0, 5.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

fn valve() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Valve {
            s: [1.0, 0.0, 0.0, 3.0],
            t: [0.0, 0.0, -1.0, 5.0],
            rotate: 0.0,
            scale: [0.5, 2.0],
        },
    }
}

fn cuboid(min: [f64; 3], max: [f64; 3], surface: &Surface) -> Brush {
    Brush::cuboid(min, max, surface).unwrap()
}

/// The solid's volume, by the divergence theorem over its faces' polygons.
fn volume(brush: &Brush) -> f64 {
    let mut total = 0.0;
    for (face, polygon) in brush.faces.iter().zip(brush.face_polygons()) {
        let Some(polygon) = polygon else {
            continue;
        };
        let normal = face.plane().unwrap().normal;
        let origin = polygon[0];
        let mut area = [0.0; 3];
        for pair in polygon.windows(2).skip(1) {
            let a = [0, 1, 2].map(|i| pair[0][i] - origin[i]);
            let b = [0, 1, 2].map(|i| pair[1][i] - origin[i]);
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// Whether every corner of each face of `before`, mapped by `map`, has on the matching face of
/// `after` the texel it had.
fn texture_fixed(before: &Brush, after: &Brush, map: &Affine) -> bool {
    before
        .faces
        .iter()
        .zip(&after.faces)
        .zip(before.face_polygons())
        .all(|((old, new), polygon)| {
            let polygon = polygon.unwrap_or_default();
            let (Some(old), Some(new)) = (WorldTexture::of(old), WorldTexture::of(new)) else {
                return false;
            };
            polygon.iter().all(|&p| {
                let q = map.apply(p);
                (0..2).all(|i| (old.texel(i, p) - new.texel(i, q)).abs() < 1e-3)
            })
        })
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

// spec MAP-15.1, MAP-15.2: a scale about a point keeps the brush valid, scales its volume and,
// with texture lock, keeps every face's texture fixed in the world in both forms; without it
// the definitions are kept
#[test]
fn a_scaled_brush_keeps_its_textures_in_the_world() {
    for surface in [standard(), valve()] {
        let brush = cuboid([0.0, 0.0, 0.0], [64.0, 32.0, 16.0], &surface);
        let map = Affine::scale([32.0, 16.0, 8.0], [2.0, 0.5, 3.0]);
        let scaled = brush.transformed(&map, TextureLock::Locked).unwrap();
        assert!(scaled.is_valid());
        assert!(near(volume(&scaled), volume(&brush) * 3.0));
        assert!(texture_fixed(&brush, &scaled, &map));
        let free = brush.transformed(&map, TextureLock::Free).unwrap();
        for (a, b) in brush.faces.iter().zip(&free.faces) {
            assert_eq!(a.texdef(), b.texdef());
        }
    }
}

// spec MAP-15.1, MAP-15.2: a shear keeps a Valve 220 face's texture fixed exactly; a mirror
// keeps every face facing out; a map that flattens is refused
#[test]
fn shears_mirrors_and_flattening() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 32.0, 16.0], &valve());
    let map = Affine::shear([0.0; 3], WorldAxis::X, WorldAxis::Z, 0.75).unwrap();
    let sheared = brush.transformed(&map, TextureLock::Locked).unwrap();
    assert!(sheared.is_valid());
    assert!(near(volume(&sheared), volume(&brush)));
    assert!(texture_fixed(&brush, &sheared, &map));
    assert_eq!(
        Affine::shear([0.0; 3], WorldAxis::Y, WorldAxis::Y, 1.0),
        None
    );
    let standard = cuboid([0.0, 0.0, 0.0], [64.0, 32.0, 16.0], &standard());
    assert!(
        standard
            .transformed(&map, TextureLock::Locked)
            .unwrap()
            .is_valid()
    );
    let mirror = Affine::scale([0.0; 3], [-1.0, 1.0, 1.0]);
    let mirrored = brush.transformed(&mirror, TextureLock::Locked).unwrap();
    assert!(mirrored.is_valid());
    assert!(near(volume(&mirrored), volume(&brush)));
    assert!(texture_fixed(&brush, &mirrored, &mirror));
    assert!(matches!(
        brush.transformed(
            &Affine::scale([0.0; 3], [1.0, 0.0, 1.0]),
            TextureLock::Locked
        ),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.3: an extruded face moves along its normal, out or in, the other faces kept as
// they are, its texture fixed under lock; a move through the brush is refused
#[test]
fn faces_extrude_out_and_in() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 32.0, 16.0], &standard());
    let top = brush
        .faces
        .iter()
        .position(|f| f.plane().unwrap().normal == [0.0, 0.0, 1.0])
        .unwrap();
    let out = brush
        .extruded(BrushFace::new(top), 8.0, TextureLock::Locked)
        .unwrap();
    assert!(near(volume(&out), 64.0 * 32.0 * 24.0));
    for (i, (a, b)) in brush.faces.iter().zip(&out.faces).enumerate() {
        if i != top {
            assert_eq!(a, b, "face {i} is kept");
        }
    }
    let lift = Affine {
        linear: IDENTITY,
        offset: [0.0, 0.0, 8.0],
    };
    let old = WorldTexture::of(&brush.faces[top]).unwrap();
    let new = WorldTexture::of(&out.faces[top]).unwrap();
    for p in brush.face_polygons()[top].as_ref().unwrap() {
        assert!((0..2).all(|i| near(old.texel(i, *p), new.texel(i, lift.apply(*p)))));
    }
    let inward = brush
        .extruded(BrushFace::new(top), -8.0, TextureLock::Locked)
        .unwrap();
    assert!(near(volume(&inward), 64.0 * 32.0 * 8.0));
    assert!(matches!(
        brush.extruded(BrushFace::new(top), -16.0, TextureLock::Locked),
        Err(BrushError::Degenerate)
    ));
    assert!(matches!(
        brush.extruded(BrushFace::new(9), 8.0, TextureLock::Locked),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.4: a brush swept along a path is one brush for each step, each the hull of the
// brush at the step's two ends; a path of one point, or a step of no length, is refused
#[test]
fn a_brush_sweeps_along_a_path() {
    let brush = cuboid([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], &standard());
    let path = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [64.0, 64.0, 32.0]];
    let swept = brush.swept(&path, TextureLock::Locked).unwrap();
    assert_eq!(swept.len(), 2);
    assert!(swept.iter().all(Brush::is_valid));
    assert!(near(volume(&swept[0]), 80.0 * 16.0 * 16.0));
    assert!(matches!(
        brush.swept(&path[..1], TextureLock::Locked),
        Err(BrushError::Degenerate)
    ));
    assert!(matches!(
        brush.swept(&[[0.0; 3], [0.0; 3]], TextureLock::Locked),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.5: a hollowed brush is walls of the thickness given, their volume the brush's less
// the room inside; a wall that leaves no room, or is not positive, is refused
#[test]
fn a_brush_hollows_into_walls() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 64.0, 32.0], &standard());
    let walls = brush.hollowed(8.0).unwrap();
    assert!(walls.len() >= 6);
    assert!(walls.iter().all(Brush::is_valid));
    let total: f64 = walls.iter().map(volume).sum();
    assert!(near(total, 64.0 * 64.0 * 32.0 - 48.0 * 48.0 * 16.0));
    assert!(matches!(brush.hollowed(16.0), Err(BrushError::Degenerate)));
    assert!(matches!(brush.hollowed(0.0), Err(BrushError::Degenerate)));
    assert!(matches!(
        brush.hollowed(f64::NAN),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.6: the intersection of brushes is the solid all hold, with their faces as they
// were; brushes that share no volume are refused
#[test]
fn brushes_intersect() {
    let a = cuboid([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], &standard());
    let b = cuboid([32.0, 16.0, -8.0], [96.0, 48.0, 40.0], &valve());
    let both = a.intersected(&[&b]).unwrap();
    assert!(both.is_valid());
    assert!(near(volume(&both), 32.0 * 32.0 * 40.0));
    assert!(
        both.faces
            .iter()
            .all(|f| a.faces.contains(f) || b.faces.contains(f))
    );
    let apart = cuboid([100.0, 0.0, 0.0], [120.0, 20.0, 20.0], &standard());
    assert!(matches!(
        a.intersected(&[&apart]),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.7: several vertices move at once; each from must be a vertex, each to must stay
// one, and no other vertex may be lost
#[test]
fn several_vertices_move_together() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], &standard());
    let moved = brush
        .moved_vertices(&[
            ([0.0, 0.0, 64.0], [0.0, 0.0, 96.0]),
            ([64.0, 0.0, 64.0], [64.0, 0.0, 96.0]),
        ])
        .unwrap();
    assert!(moved.is_valid());
    assert!(near(
        volume(&moved),
        64.0 * 64.0 * 64.0 + 64.0 * 64.0 * 32.0 / 2.0
    ));
    assert!(matches!(
        brush.moved_vertices(&[([1.0, 2.0, 3.0], [0.0; 3])]),
        Err(BrushError::NoVertex(_))
    ));
    assert!(matches!(
        brush.moved_vertices(&[([0.0, 0.0, 64.0], [32.0, 32.0, 32.0])]),
        Err(BrushError::VertexInside(_))
    ));
}

// spec MAP-15.8: an edge or a face splits by a new vertex outside it; a pair that is no edge,
// or a point that would not be a vertex, is refused
#[test]
fn edges_and_faces_split() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], &standard());
    let roof = brush
        .split_edge([0.0, 0.0, 64.0], [64.0, 0.0, 64.0], [32.0, -16.0, 80.0])
        .unwrap();
    assert!(roof.is_valid());
    assert_eq!(roof.vertices().len(), 9);
    assert!(matches!(
        brush.split_edge([0.0, 0.0, 64.0], [64.0, 64.0, 64.0], [32.0, 32.0, 80.0]),
        Err(BrushError::NoVertex(_))
    ));
    assert!(matches!(
        brush.split_edge([0.0, 0.0, 64.0], [64.0, 0.0, 64.0], [32.0, 8.0, 32.0]),
        Err(BrushError::VertexInside(_))
    ));
    let top = brush
        .faces
        .iter()
        .position(|f| f.plane().unwrap().normal == [0.0, 0.0, 1.0])
        .unwrap();
    assert_eq!(
        brush.face_out(BrushFace::new(top), 32.0),
        Some([32.0, 32.0, 96.0])
    );
    assert_eq!(brush.face_out(BrushFace::new(42), 32.0), None);
    let out = brush
        .edge_out([0.0, 0.0, 64.0], [64.0, 0.0, 64.0], 2.0_f64.sqrt())
        .unwrap();
    assert!(
        out.iter()
            .zip([32.0, -1.0, 65.0])
            .all(|(a, b)| (a - b).abs() < 1e-9)
    );
    assert_eq!(
        brush.edge_out([0.0, 0.0, 64.0], [64.0, 64.0, 64.0], 1.0),
        None
    );
    let pyramid = brush
        .split_face(BrushFace::new(top), [32.0, 32.0, 96.0])
        .unwrap();
    assert!(pyramid.is_valid());
    assert_eq!(pyramid.vertices().len(), 9);
    assert!(near(
        volume(&pyramid),
        64.0 * 64.0 * 64.0 + 64.0 * 64.0 * 32.0 / 3.0
    ));
    assert!(matches!(
        brush.split_face(BrushFace::new(top), [32.0, 32.0, 32.0]),
        Err(BrushError::VertexInside(_))
    ));
    assert!(matches!(
        brush.split_face(BrushFace::new(42), [32.0, 32.0, 96.0]),
        Err(BrushError::Degenerate)
    ));
}

// spec MAP-15.9: an edge chamfers into one bevel face, or several on an arc tangent to both of
// its faces, the edge's ends gone; a bad distance, count or pair is refused
#[test]
fn edges_chamfer_into_bevels() {
    let brush = cuboid([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], &standard());
    let (a, b) = ([0.0, 0.0, 64.0], [64.0, 0.0, 64.0]);
    let flat = brush.chamfered(a, b, 16.0, 1).unwrap();
    assert!(flat.is_valid());
    assert_eq!(flat.faces.len(), 7);
    assert!(near(
        volume(&flat),
        64.0f64.powi(3) - 16.0 * 16.0 / 2.0 * 64.0
    ));
    let round = brush.chamfered(a, b, 16.0, 4).unwrap();
    assert!(round.is_valid());
    assert_eq!(round.faces.len(), 10);
    let quarter = 16.0 * 16.0 - core::f64::consts::PI * 16.0 * 16.0 / 4.0;
    assert!(volume(&round) > volume(&flat));
    assert!(volume(&round) < 64.0f64.powi(3) - quarter * 64.0 + 1.0);
    for bad in [0, MAP_MAX_CHAMFER_FACES + 1] {
        assert!(matches!(
            brush.chamfered(a, b, 16.0, bad),
            Err(BrushError::Degenerate)
        ));
    }
    assert!(matches!(
        brush.chamfered(a, b, 0.0, 1),
        Err(BrushError::Degenerate)
    ));
    assert!(matches!(
        brush.chamfered(a, b, 80.0, 1),
        Err(BrushError::Degenerate)
    ));
    assert!(matches!(
        brush.chamfered(a, [64.0, 64.0, 64.0], 16.0, 1),
        Err(BrushError::NoVertex(_))
    ));
}

/// Whether every vertex of `inner` is behind or on every face of `outer`, within qbsp's
/// `ON_EPSILON`: a cut at a sharp corner can leave a face too small to keep, and the solid
/// without it reaches out by less than that.
fn within(inner: &Brush, outer: &Brush) -> bool {
    let planes: Vec<_> = outer.faces.iter().map(|f| f.plane().unwrap()).collect();
    inner
        .vertices()
        .iter()
        .all(|&v| planes.iter().all(|p| p.side(v) < MAP_ON_EPSILON))
}

/// Whether the brush is one a map would hold: each face has a polygon and no two vertices
/// are within 8 units.
fn sound(brush: &Brush) -> bool {
    let corners = brush.vertices();
    brush.face_polygons().iter().all(Option::is_some)
        && corners.iter().enumerate().all(|(i, a)| {
            corners.iter().skip(i + 1).all(|b| {
                let d = [0, 1, 2].map(|k| a[k] - b[k]);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() >= 8.0
            })
        })
}

/// A brush as maps hold them: a box on an 8-unit grid, 32 to 256 units on a side, cut by up
/// to two planes through grid points, each keeping the part behind it.
fn arbitrary_brush() -> impl Strategy<Value = Brush> {
    (
        prop::array::uniform3(-8_i32..8),
        prop::array::uniform3(4_i32..=32),
        prop::collection::vec(
            prop::array::uniform3(prop::array::uniform3(-32_i32..=32)),
            0..=2,
        ),
    )
        .prop_filter_map("a sound brush", |(corner, size, cuts)| {
            let min = corner.map(|c| f64::from(c) * 8.0);
            let max = [0, 1, 2].map(|i| min[i] + f64::from(size[i]) * 8.0);
            let mut brush = Brush::cuboid(min, max, &valve()).ok()?;
            for points in cuts {
                let points = points.map(|p| p.map(|c| f64::from(c) * 8.0));
                let Some(plane) = plane(points) else {
                    continue;
                };
                if let Ok((Some(back), _)) = brush.split(plane, &valve()) {
                    brush = back;
                }
            }
            (sound(&brush) && volume(&brush) > 1000.0).then_some(brush)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // spec MAP-15.1, MAP-15.2: any scale or shear of any brush is a valid brush whose Valve 220
    // textures stay fixed in the world
    #[test]
    fn transforms_keep_brushes_valid_and_textures_fixed(
        brush in arbitrary_brush(),
        factors in prop::array::uniform3(prop_oneof![0.25..4.0_f64, -4.0..-0.25_f64]),
        shear in -2.0..2.0_f64,
    ) {
        let scale = Affine::scale([5.0, -3.0, 7.0], factors);
        let scaled = brush.transformed(&scale, TextureLock::Locked).unwrap();
        prop_assert!(scaled.is_valid());
        prop_assert!(texture_fixed(&brush, &scaled, &scale));
        let sheared_map = Affine::shear([0.0; 3], WorldAxis::Y, WorldAxis::X, shear).unwrap();
        let sheared = brush.transformed(&sheared_map, TextureLock::Locked).unwrap();
        prop_assert!(sheared.is_valid());
        prop_assert!(texture_fixed(&brush, &sheared, &sheared_map));
    }

    // spec MAP-15.3, MAP-15.5, MAP-15.6, MAP-15.9, MAP-15.10: every tool on any brush gives valid brushes
    // or refuses
    #[test]
    fn tools_give_valid_brushes_or_refuse(
        brush in arbitrary_brush(),
        other in arbitrary_brush(),
        face in 0_usize..12,
        by in -64.0..64.0_f64,
        wall in 1.0..32.0_f64,
        edge in 0_usize..64,
        distance in 1.0..48.0_f64,
        faces in 1_u32..=4,
    ) {
        if let Ok(out) = brush.extruded(BrushFace::new(face), by, TextureLock::Locked) {
            prop_assert!(out.is_valid());
        }
        if let Ok(walls) = brush.hollowed(wall) {
            prop_assert!(walls.iter().all(Brush::is_valid));
            let total: f64 = walls.iter().map(volume).sum();
            prop_assert!(total < volume(&brush) + 1e-3);
            prop_assert!(walls.iter().all(|w| within(w, &brush)));
        }
        if let Ok(both) = brush.intersected(&[&other]) {
            prop_assert!(both.is_valid());
            prop_assert!(within(&both, &brush) && within(&both, &other));
        }
        let corners = brush.vertices();
        let a = corners[edge % corners.len()];
        for &b in &corners {
            if let Ok(out) = brush.chamfered(a, b, distance, faces) {
                prop_assert!(out.is_valid());
                prop_assert!(within(&out, &brush));
            }
        }
    }
}
