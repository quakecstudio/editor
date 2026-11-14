// SPDX-License-Identifier: GPL-3.0-or-later

//! The brush tools (P6-18, MAP-15) against the map compiler: every brush a tool makes, set in a
//! sealed room, compiles with `qs-map-bsp` in both dialects with no fault.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::geometry::plane;
use qs_map::{Affine, Brush, BrushFace, Item, MapFile, Surface, TexDef, TextureLock, WorldAxis};
use qs_map_bsp::{Compiler, Dialect, Warning};

fn standard() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

fn valve() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Valve {
            s: [1.0, 0.0, 0.0, 0.0],
            t: [0.0, 0.0, -1.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

/// `brushes` in a room 2048 units across, sealed, with a player start.
fn compiles(brushes: &[Brush], what: &str) {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-900 -900 -900\"\n}\n",
    )
    .unwrap();
    let (lo, hi) = (-1024.0, 1024.0);
    for (min, max) in [
        (
            [lo - 16.0, lo - 16.0, lo - 16.0],
            [hi + 16.0, hi + 16.0, lo],
        ),
        (
            [lo - 16.0, lo - 16.0, hi],
            [hi + 16.0, hi + 16.0, hi + 16.0],
        ),
        ([lo - 16.0, lo - 16.0, lo], [lo, hi + 16.0, hi]),
        ([hi, lo - 16.0, lo], [hi + 16.0, hi + 16.0, hi]),
        ([lo, lo - 16.0, lo], [hi, lo, hi]),
        ([lo, hi, lo], [hi, hi + 16.0, hi]),
    ] {
        let wall = Brush::cuboid(min, max, &standard()).unwrap();
        file.entities[0].items.push(Item::Brush(wall));
    }
    for brush in brushes {
        assert!(brush.is_valid(), "{what}: a valid brush");
        file.entities[0].items.push(Item::Brush(brush.clone()));
    }
    let text = file.to_bytes();
    // id's qbsp reads a Valve 220 face, or a number not whole, otherwise (MAP-3.3, MAP-5.5):
    // the id dialect compiles only brushes with none.
    let id_reads = brushes
        .iter()
        .flat_map(|b| &b.faces)
        .all(|f| !f.qbsp_differs());
    let dialects: &[Dialect] = if id_reads {
        &[Dialect::Id, Dialect::Extended]
    } else {
        &[Dialect::Extended]
    };
    for &dialect in dialects {
        let compiled = Compiler::default()
            .with_dialect(dialect)
            .compile(&text, None);
        assert!(
            compiled.is_ok(),
            "{what} in {dialect:?}: {:?}",
            compiled.err()
        );
        let compiled = compiled.unwrap();
        let faults: Vec<&Warning> = compiled
            .warnings
            .iter()
            .filter(|w| matches!(w, Warning::NoBrushFaces | Warning::Leak))
            .collect();
        assert!(faults.is_empty(), "{what} in {dialect:?}: {faults:?}");
    }
}

// spec MAP-15.2, MAP-15.3, MAP-15.4, MAP-15.5, MAP-15.6, MAP-15.7, MAP-15.8, MAP-15.9: every
// tool's brushes compile in both dialects with no fault
#[test]
fn every_tool_s_brushes_compile() {
    let block = Brush::cuboid([0.0, 0.0, 0.0], [128.0, 96.0, 64.0], &standard()).unwrap();
    let other = Brush::cuboid([64.0, 32.0, -32.0], [192.0, 128.0, 32.0], &valve()).unwrap();
    let top = block
        .faces
        .iter()
        .position(|f| f.plane().unwrap().normal[2] > 0.5)
        .unwrap();
    let scale = Affine::scale([64.0, 48.0, 32.0], [1.5, -0.75, 2.0]);
    let shear = Affine::shear([0.0; 3], WorldAxis::X, WorldAxis::Z, 0.5).unwrap();
    let (a, b) = ([0.0, 0.0, 64.0], [128.0, 0.0, 64.0]);
    let made: Vec<(&str, Vec<Brush>)> = vec![
        (
            "scale",
            vec![block.transformed(&scale, TextureLock::Locked).unwrap()],
        ),
        (
            "shear",
            vec![other.transformed(&shear, TextureLock::Locked).unwrap()],
        ),
        (
            "extrude",
            vec![
                block
                    .extruded(BrushFace::new(top), 40.0, TextureLock::Locked)
                    .unwrap(),
            ],
        ),
        (
            "sweep",
            block
                .swept(
                    &[[0.0; 3], [200.0, 0.0, 0.0], [200.0, 200.0, 100.0]],
                    TextureLock::Locked,
                )
                .unwrap(),
        ),
        ("hollow", block.hollowed(16.0).unwrap()),
        ("intersect", vec![block.intersected(&[&other]).unwrap()]),
        (
            "vertices",
            vec![block.moved_vertices(&[(a, [0.0, 0.0, 96.0])]).unwrap()],
        ),
        (
            "split edge",
            vec![block.split_edge(a, b, [64.0, -32.0, 96.0]).unwrap()],
        ),
        (
            "split face",
            vec![
                block
                    .split_face(BrushFace::new(top), [64.0, 48.0, 128.0])
                    .unwrap(),
            ],
        ),
        ("chamfer", vec![block.chamfered(a, b, 24.0, 1).unwrap()]),
        ("round", vec![block.chamfered(a, b, 24.0, 6).unwrap()]),
    ];
    for (what, brushes) in made {
        compiles(&brushes, what);
    }
}

/// A number from 0 to 1 from the state `seed`, which it moves on.
fn unit(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    f64::from(u32::try_from(*seed >> 40).unwrap()) / f64::from(1_u32 << 24)
}

/// A step on the 8-unit grid from -`steps` to `steps`.
fn grid(seed: &mut u64, steps: f64) -> f64 {
    ((unit(seed) * 2.0 - 1.0) * steps).round() * 8.0
}

// spec MAP-15.1, MAP-15.3, MAP-15.5, MAP-15.6, MAP-15.9: the tools on random brushes on the grid,
// each cut by a random plane, give brushes that compile with no fault, or refuse
#[test]
fn random_tool_results_compile() {
    let mut seed = 819;
    let mut compiled = 0;
    for _ in 0..32 {
        let min = [0, 1, 2].map(|_| grid(&mut seed, 16.0));
        let max = [0, 1, 2].map(|i| min[i] + 32.0 + (unit(&mut seed) * 24.0).round() * 8.0);
        let mut brush = Brush::cuboid(min, max, &valve()).unwrap();
        let points = [0, 1, 2].map(|_| [0, 1, 2].map(|_| grid(&mut seed, 32.0)));
        if let Some(plane) = plane(points)
            && let Ok((Some(back), _)) = brush.split(plane, &valve())
        {
            brush = back;
        }
        let mut results: Vec<Brush> = Vec::new();
        let factors = [0, 1, 2].map(|_| 0.5 + unit(&mut seed) * 1.5);
        results.extend(
            brush
                .transformed(&Affine::scale(min, factors), TextureLock::Locked)
                .ok(),
        );
        let face = BrushFace::new(usize::from(u8::try_from(seed >> 61).unwrap()));
        results.extend(
            brush
                .extruded(face, grid(&mut seed, 4.0), TextureLock::Locked)
                .ok(),
        );
        results.extend(brush.hollowed(8.0).unwrap_or_default());
        let corners = brush.vertices();
        for &b in &corners {
            if let Ok(chamfered) = brush.chamfered(corners[0], b, 8.0, 2) {
                results.push(chamfered);
                break;
            }
        }
        // Each result in a room of its own, so that two never overlap into one solid.
        for (i, result) in results.iter().enumerate() {
            compiles(std::slice::from_ref(result), &format!("random {seed} {i}"));
            compiled += 1;
        }
    }
    assert!(compiled > 60, "{compiled} brushes compiled");
}
