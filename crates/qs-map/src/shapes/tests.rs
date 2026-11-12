// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{
    CircleMode, CurveSides, CurveWall, Facing, SHAPE_STEP_DEPTH, SHAPE_STEP_HEIGHT, Shape,
    ShapeError, Spandrels, Wall,
};
use crate::{Brush, Surface, TexDef};

/// The default step is under `STEPSIZE`, 18 (PHYS-8.3).
const _: () = assert!(SHAPE_STEP_HEIGHT < 18.0);

fn surface() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

const LOW: [f64; 3] = [-64.0, -64.0, 0.0];
const HIGH: [f64; 3] = [64.0, 64.0, 128.0];

fn make(shape: Shape) -> Result<Vec<Brush>, ShapeError> {
    shape.brushes(LOW, HIGH, &surface())
}

/// Whether `a` and `b` are one point, within arithmetic noise.
fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

/// Every vertex of `brushes`, each once.
fn vertices(brushes: &[Brush]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    for v in brushes.iter().flat_map(Brush::vertices) {
        if !out.iter().any(|&known| near(known, v)) {
            out.push(v);
        }
    }
    out
}

// spec MAP-10.1: a box with no volume or a corner that is no number is refused
#[test]
fn a_flat_or_unreal_box_is_refused() {
    let cylinder = Shape::Cylinder {
        sides: 8,
        mode: CircleMode::Vertices,
        wall: Wall::Solid,
    };
    for (a, b) in [
        ([0.0, 0.0, 0.0], [64.0, 64.0, 0.0]),
        ([0.0, 0.0, 0.0], [f64::NAN, 64.0, 64.0]),
        ([0.0, 0.0, 0.0], [f64::INFINITY, 64.0, 64.0]),
    ] {
        assert!(matches!(
            cylinder.brushes(a, b, &surface()),
            Err(ShapeError::Size)
        ));
    }
    // The corners in either order make the same shape.
    assert_eq!(
        cylinder.brushes(HIGH, LOW, &surface()).unwrap(),
        make(cylinder).unwrap()
    );
}

// spec MAP-10.2: corners on the ellipse lie on it; with edges on it a four-sided shape is the
// box's footprint
#[test]
fn the_circle_modes_place_the_corners() {
    let on = make(Shape::Cylinder {
        sides: 12,
        mode: CircleMode::Vertices,
        wall: Wall::Solid,
    })
    .unwrap();
    for v in vertices(&on) {
        let r = (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!((r - 64.0).abs() < 1e-3, "{v:?}");
    }
    let square = make(Shape::Cylinder {
        sides: 4,
        mode: CircleMode::Edges,
        wall: Wall::Solid,
    })
    .unwrap();
    let corners = vertices(&square);
    for v in &corners {
        assert!((v[0].abs() - 64.0).abs() < 1e-3 && (v[1].abs() - 64.0).abs() < 1e-3);
    }
    assert_eq!(corners.len(), 8);
}

// spec MAP-10.3: three to 128 sides
#[test]
fn the_sides_are_bounded() {
    for sides in [0, 1, 2, 129, u32::MAX] {
        assert!(matches!(
            make(Shape::Cylinder {
                sides,
                mode: CircleMode::Vertices,
                wall: Wall::Solid
            }),
            Err(ShapeError::Sides(n)) if n == sides
        ));
    }
    for sides in [3, 128] {
        assert!(
            make(Shape::Cylinder {
                sides,
                mode: CircleMode::Edges,
                wall: Wall::Solid
            })
            .is_ok()
        );
    }
}

// spec MAP-10.4: a hollow cylinder's segments share their sides, and a solid cylinder on the
// box shrunk by the wall fits its hole with no gap, in either mode
#[test]
fn shapes_of_the_same_sides_and_mode_fit_together() {
    for mode in [CircleMode::Vertices, CircleMode::Edges] {
        let ring = make(Shape::Cylinder {
            sides: 16,
            mode,
            wall: Wall::Hollow { thickness: 16.0 },
        })
        .unwrap();
        assert_eq!(ring.len(), 16);
        let core = Shape::Cylinder {
            sides: 16,
            mode,
            wall: Wall::Solid,
        }
        .brushes([-48.0, -48.0, 0.0], [48.0, 48.0, 128.0], &surface())
        .unwrap();
        let ring_points = vertices(&ring);
        for v in vertices(&core) {
            assert!(
                ring_points.iter().any(|&r| near(r, v)),
                "{mode:?}: the core's corner {v:?} is no corner of the ring"
            );
        }
        // Neighbours share their upright edges: 16 outer and 16 inner corners, top and bottom.
        assert_eq!(ring_points.len(), 16 * 4, "{mode:?}");
    }
    for thickness in [0.0, -1.0, 64.0, f64::NAN] {
        assert!(matches!(
            make(Shape::Cylinder {
                sides: 8,
                mode: CircleMode::Vertices,
                wall: Wall::Hollow { thickness }
            }),
            Err(ShapeError::Thickness(_))
        ));
    }
}

// spec MAP-10.5: a cone's point is the middle of the box's top
#[test]
fn a_cone_rises_to_the_middle_of_the_top() {
    let cone = make(Shape::Cone {
        sides: 8,
        mode: CircleMode::Vertices,
    })
    .unwrap();
    let points = vertices(&cone);
    assert_eq!(points.len(), 9);
    assert!(points.iter().any(|&v| near(v, [0.0, 0.0, 128.0])));
}

// spec MAP-10.6: an arch's segments follow the half ellipse; filled spandrels reach the box's top
#[test]
fn an_arch_with_spandrels_fills_the_wall() {
    let arch = |spandrels| {
        make(Shape::Arch {
            sides: 8,
            mode: CircleMode::Vertices,
            arc: 180.0,
            start: 0.0,
            thickness: 16.0,
            spandrels,
        })
        .unwrap()
    };
    let open = arch(Spandrels::Open);
    assert_eq!(open.len(), 8);
    let filled = arch(Spandrels::Filled);
    assert!(filled.len() > open.len());
    let top = vertices(&filled)
        .iter()
        .filter(|v| (v[2] - 128.0).abs() < 1e-3)
        .count();
    assert!(top > 2);
    for arc in [0.0, -10.0, 361.0, f64::NAN] {
        assert!(matches!(
            make(Shape::Arch {
                sides: 8,
                mode: CircleMode::Vertices,
                arc,
                start: 0.0,
                thickness: 8.0,
                spandrels: Spandrels::Open
            }),
            Err(ShapeError::Arc(_))
        ));
    }
}

// spec MAP-10.7: a UV spheroid and an icospheroid are one brush each, within their limits
#[test]
fn spheroids_are_one_brush() {
    let uv = make(Shape::UvSpheroid {
        sides: 12,
        rings: 6,
    })
    .unwrap();
    assert_eq!(uv.len(), 1);
    for v in vertices(&uv) {
        let r =
            ((v[0] / 64.0).powi(2) + (v[1] / 64.0).powi(2) + ((v[2] - 64.0) / 64.0).powi(2)).sqrt();
        assert!(r <= 1.0 + 1e-6, "{v:?}");
    }
    for subdivisions in 0..=3 {
        assert_eq!(make(Shape::Icospheroid { subdivisions }).unwrap().len(), 1);
    }
    assert!(matches!(
        make(Shape::Icospheroid { subdivisions: 4 }),
        Err(ShapeError::Subdivisions(4))
    ));
    assert!(
        make(Shape::UvSpheroid {
            sides: 20,
            rings: 64
        })
        .is_ok()
    );
    assert!(matches!(
        make(Shape::UvSpheroid {
            sides: 21,
            rings: 64
        }),
        Err(ShapeError::Faces(1344))
    ));
    for rings in [0, 1, 65] {
        assert!(matches!(
            make(Shape::UvSpheroid { sides: 8, rings }),
            Err(ShapeError::Rings(_))
        ));
    }
}

// spec MAP-10.8: one seed, one rock; another seed, another rock
#[test]
fn a_rock_is_its_seed_s() {
    let rock = |seed| make(Shape::Rock { points: 24, seed }).unwrap();
    assert_eq!(rock(7), rock(7));
    assert_ne!(rock(7), rock(8));
    for points in [0, 3, 65] {
        assert!(matches!(
            make(Shape::Rock { points, seed: 1 }),
            Err(ShapeError::Points(_))
        ));
    }
}

// spec MAP-10.9: every corner of a scalable curve lies on the grid at every size, hollow or
// solid
#[test]
fn every_corner_of_a_scalable_curve_is_on_the_grid() {
    for sides in [
        CurveSides::Twelve,
        CurveSides::TwentyFour,
        CurveSides::FortyEight,
        CurveSides::NinetySix,
    ] {
        for grid in [1.0, 8.0, 16.0] {
            for size in [64.0, 200.0, 640.0, 1500.0] {
                for wall in [CurveWall::Solid, CurveWall::Hollow { steps: 1 }] {
                    let made = Shape::Curve { sides, grid, wall }.brushes(
                        [grid * 3.0 + 1.0, -grid, 0.0],
                        [grid * 3.0 + size, -grid + size, grid * 4.0],
                        &surface(),
                    );
                    let Ok(brushes) = made else {
                        // A curve one template wide has no room for a hole.
                        assert!(wall != CurveWall::Solid, "{sides:?} {grid} {size}");
                        continue;
                    };
                    for v in vertices(&brushes) {
                        for c in v {
                            assert!(
                                ((c / grid) - (c / grid).round()).abs() < 1e-9,
                                "{sides:?} grid {grid} size {size}: {v:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(matches!(
        make(Shape::Curve {
            sides: CurveSides::Twelve,
            grid: 0.0,
            wall: CurveWall::Solid
        }),
        Err(ShapeError::Grid(_))
    ));
}

// spec MAP-10.10: stairs of the default step, under STEPSIZE, as many as fit
#[test]
fn stairs_fill_the_box_with_climbable_steps() {
    let stairs = Shape::Stairs {
        height: SHAPE_STEP_HEIGHT,
        depth: SHAPE_STEP_DEPTH,
        facing: Facing::East,
    }
    .brushes([0.0, 0.0, 0.0], [256.0, 64.0, 96.0], &surface())
    .unwrap();
    // 256 / 32 = 8 by length, 96 / 16 = 6 by height.
    assert_eq!(stairs.len(), 6);
    for (i, step) in stairs.iter().enumerate() {
        let top = step
            .vertices()
            .iter()
            .map(|v| v[2])
            .fold(f64::MIN, f64::max);
        assert_eq!(top, 16.0 * f64::from(u32::try_from(i).unwrap() + 1));
    }
    for facing in [Facing::West, Facing::North, Facing::South] {
        assert!(
            Shape::Stairs {
                height: 16.0,
                depth: 32.0,
                facing
            }
            .brushes([0.0, 0.0, 0.0], [256.0, 256.0, 96.0], &surface())
            .is_ok()
        );
    }
    assert!(matches!(
        Shape::Stairs {
            height: 200.0,
            depth: 32.0,
            facing: Facing::East
        }
        .brushes([0.0, 0.0, 0.0], [256.0, 64.0, 96.0], &surface()),
        Err(ShapeError::Step)
    ));
}

// spec MAP-10.11: spiral stairs climb a step a slice, with a core or to the middle
#[test]
fn spiral_stairs_climb_a_slice_at_a_time() {
    for core in [0.0, 16.0] {
        let stairs = make(Shape::SpiralStairs {
            sides: 12,
            height: 16.0,
            core,
        })
        .unwrap();
        assert_eq!(stairs.len(), 8);
    }
    assert!(matches!(
        make(Shape::SpiralStairs {
            sides: 12,
            height: 16.0,
            core: 64.0
        }),
        Err(ShapeError::Thickness(_))
    ));
}

/// Any shape, its settings drawn from wide ranges with zero and the limits among them.
fn any_shape() -> impl Strategy<Value = Shape> {
    let sides = prop_oneof![
        Just(0_u32),
        Just(2),
        Just(3),
        Just(128),
        Just(129),
        0_u32..200
    ];
    let length = prop_oneof![Just(0.0), Just(-1.0), Just(f64::NAN), -100.0..200.0_f64];
    let mode = prop_oneof![Just(CircleMode::Vertices), Just(CircleMode::Edges)];
    let wall = prop_oneof![
        Just(Wall::Solid),
        length
            .clone()
            .prop_map(|thickness| Wall::Hollow { thickness })
    ];
    prop_oneof![
        (sides.clone(), mode.clone(), wall).prop_map(|(sides, mode, wall)| Shape::Cylinder {
            sides,
            mode,
            wall
        }),
        (sides.clone(), mode.clone()).prop_map(|(sides, mode)| Shape::Cone { sides, mode }),
        (
            sides.clone(),
            mode,
            length.clone(),
            -720.0..720.0_f64,
            length.clone(),
            any::<bool>()
        )
            .prop_map(|(sides, mode, arc, start, thickness, filled)| Shape::Arch {
                sides,
                mode,
                arc,
                start,
                thickness,
                spandrels: if filled {
                    Spandrels::Filled
                } else {
                    Spandrels::Open
                },
            }),
        (sides.clone(), 0_u32..70).prop_map(|(sides, rings)| Shape::UvSpheroid { sides, rings }),
        (0_u32..5).prop_map(|subdivisions| Shape::Icospheroid { subdivisions }),
        (0_u32..70, any::<u64>()).prop_map(|(points, seed)| Shape::Rock { points, seed }),
        (0_usize..4, length.clone(), 0_u32..4).prop_map(|(sides, grid, steps)| Shape::Curve {
            sides: [
                CurveSides::Twelve,
                CurveSides::TwentyFour,
                CurveSides::FortyEight,
                CurveSides::NinetySix
            ][sides],
            grid,
            wall: if steps == 0 {
                CurveWall::Solid
            } else {
                CurveWall::Hollow { steps }
            },
        }),
        (length.clone(), length.clone(), 0_usize..4).prop_map(|(height, depth, facing)| {
            Shape::Stairs {
                height,
                depth,
                facing: [Facing::East, Facing::West, Facing::North, Facing::South][facing],
            }
        }),
        (sides, length.clone(), length).prop_map(|(sides, height, core)| Shape::SpiralStairs {
            sides,
            height,
            core
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    // spec MAP-10.1: no setting crashes a shape or makes an invalid brush: each is made valid
    // or refused
    #[test]
    fn no_setting_crashes_or_makes_an_invalid_brush(
        shape in any_shape(),
        a in prop::array::uniform3(-512.0..512.0_f64),
        b in prop::array::uniform3(-512.0..512.0_f64),
    ) {
        if let Ok(brushes) = shape.brushes(a, b, &surface()) {
            for brush in &brushes {
                prop_assert!(brush.is_valid(), "{shape:?}");
            }
        }
    }
}
