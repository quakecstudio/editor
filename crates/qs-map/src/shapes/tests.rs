// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap or index is a failed test; the values compared are exact"
)]

use proptest::prelude::*;

use super::{
    CircleMode, CurveSides, Facing, SHAPE_MAX_STEPS, SHAPE_STEP_DEPTH, SHAPE_STEP_HEIGHT, Shape,
    ShapeError, Spandrels, Wall,
};
use crate::geometry::plane;
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
    let quarter = |arc, start| {
        make(Shape::Arch {
            sides: 8,
            arc,
            start,
            thickness: 8.0,
            spandrels: Spandrels::Open,
        })
    };
    assert!(quarter(90.0, 90.0).is_ok());
    assert!(quarter(180.0, 0.0).is_ok());
    for (arc, start) in [
        (0.0, 0.0),
        (-10.0, 0.0),
        (181.0, 0.0),
        (90.0, 91.0),
        (90.0, -1.0),
        (f64::NAN, 0.0),
        (90.0, f64::NAN),
    ] {
        assert!(
            matches!(quarter(arc, start), Err(ShapeError::Arc { .. })),
            "{arc} from {start}"
        );
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
                for wall in [Wall::Solid, Wall::Hollow { thickness: grid }] {
                    let made = Shape::Curve { sides, grid, wall }.brushes(
                        [grid * 3.0 + 1.0, -grid, 0.0],
                        [grid * 3.0 + size, -grid + size, grid * 4.0],
                        &surface(),
                    );
                    let brushes = made.unwrap();
                    assert!(!brushes.is_empty());
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
            wall: Wall::Solid
        }),
        Err(ShapeError::Grid(_))
    ));
    let hollow = make(Shape::Curve {
        sides: CurveSides::TwentyFour,
        grid: 8.0,
        wall: Wall::Hollow { thickness: 16.0 },
    })
    .unwrap();
    assert_eq!(hollow.len(), 24);
    for brush in &hollow {
        for v in brush.vertices() {
            let r = (v[0].powi(2) + v[1].powi(2)).sqrt();
            assert!(r > 30.0, "a hollow curve's wall is outside its hole: {v:?}");
        }
    }
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
    let steps = |length: f64| {
        Shape::Stairs {
            height: 1.0,
            depth: 1.0,
            facing: Facing::East,
        }
        .brushes([0.0, 0.0, 0.0], [length, 4.0, length], &surface())
    };
    let most = f64::from(SHAPE_MAX_STEPS);
    assert_eq!(steps(most).unwrap().len(), 256);
    assert!(matches!(steps(most + 1.0), Err(ShapeError::Step)));
    assert!(matches!(
        Shape::SpiralStairs {
            sides: 8,
            height: 1.0,
            core: 0.0
        }
        .brushes([0.0, 0.0, 0.0], [64.0, 64.0, most + 1.0], &surface()),
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
            length.clone(),
            -360.0..360.0_f64,
            length.clone(),
            any::<bool>()
        )
            .prop_map(|(sides, arc, start, thickness, filled)| Shape::Arch {
                sides,
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
        (0_usize..4, length.clone(), length.clone(), any::<bool>()).prop_map(
            |(sides, grid, thickness, hollow)| Shape::Curve {
                sides: [
                    CurveSides::Twelve,
                    CurveSides::TwentyFour,
                    CurveSides::FortyEight,
                    CurveSides::NinetySix
                ][sides],
                grid,
                wall: if hollow {
                    Wall::Hollow { thickness }
                } else {
                    Wall::Solid
                },
            }
        ),
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

/// A box off the origin and longer one way than another, so a centre or radius worked out
/// wrongly moves the shape.
const OFF_LOW: [f64; 3] = [16.0, 48.0, 512.0];
const OFF_HIGH: [f64; 3] = [208.0, 112.0, 704.0];
/// `OFF_LOW` to `OFF_HIGH`'s middle and half sizes.
const OFF_CENTRE: [f64; 3] = [112.0, 80.0, 608.0];
const OFF_RADII: [f64; 3] = [96.0, 32.0, 96.0];

fn off(shape: Shape) -> Vec<Brush> {
    shape.brushes(OFF_LOW, OFF_HIGH, &surface()).unwrap()
}

/// How far out `v` is on the ellipse about `centre` of `radii` across x and y: 1 on it.
fn across(v: [f64; 3], centre: [f64; 2], radii: [f64; 2]) -> f64 {
    ((v[0] - centre[0]) / radii[0]).powi(2) + ((v[1] - centre[1]) / radii[1]).powi(2)
}

/// How far out `v` is on the ellipsoid about `centre` of `radii`: 1 on it.
fn round(v: [f64; 3], centre: [f64; 3], radii: [f64; 3]) -> f64 {
    (0..3)
        .map(|i| ((v[i] - centre[i]) / radii[i]).powi(2))
        .sum()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

const FOOT: [f64; 2] = [OFF_CENTRE[0], OFF_CENTRE[1]];
const FOOT_RADII: [f64; 2] = [OFF_RADII[0], OFF_RADII[1]];

// spec MAP-10.2: corners on the ellipse lie on the footprint's ellipse, the first at angle 0;
// with edges on it each corner is out by 1 / cos(pi / n), the first at half a side, and a
// four-sided shape is the footprint
#[test]
fn the_corners_lie_on_the_footprint_s_ellipse() {
    let cylinder = |sides, mode| {
        off(Shape::Cylinder {
            sides,
            mode,
            wall: Wall::Solid,
        })
    };
    let on = vertices(&cylinder(16, CircleMode::Vertices));
    assert_eq!(on.len(), 32);
    for v in &on {
        assert!(close(across(*v, FOOT, FOOT_RADII), 1.0), "{v:?}");
        assert!(v[2] == OFF_LOW[2] || v[2] == OFF_HIGH[2], "{v:?}");
    }
    assert!(on.iter().any(|&v| near(v, [208.0, 80.0, 512.0])));
    assert!(on.iter().any(|&v| near(v, [112.0, 112.0, 704.0])));
    let out = vertices(&cylinder(16, CircleMode::Edges));
    let grow = 1.0 / (core::f64::consts::PI / 16.0).cos().powi(2);
    for v in &out {
        assert!(close(across(*v, FOOT, FOOT_RADII), grow), "{v:?}");
    }
    assert!(
        !out.iter().any(|&v| close(v[1], OFF_CENTRE[1])),
        "none at angle 0"
    );
    let mut square = vertices(&cylinder(4, CircleMode::Edges));
    square.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut corners = Vec::new();
    for x in [OFF_LOW[0], OFF_HIGH[0]] {
        for y in [OFF_LOW[1], OFF_HIGH[1]] {
            for z in [OFF_LOW[2], OFF_HIGH[2]] {
                corners.push([x, y, z]);
            }
        }
    }
    assert_eq!(square.len(), 8);
    for (v, c) in square.iter().zip(&corners) {
        assert!(near(*v, *c), "{v:?} {c:?}");
    }
}

// spec MAP-10.4: a hollow cylinder's wall runs from the ellipse to the one `t` smaller
#[test]
fn a_hollow_cylinder_s_wall_is_its_thickness() {
    let wall = off(Shape::Cylinder {
        sides: 16,
        mode: CircleMode::Vertices,
        wall: Wall::Hollow { thickness: 8.0 },
    });
    assert_eq!(wall.len(), 16);
    let inner = [FOOT_RADII[0] - 8.0, FOOT_RADII[1] - 8.0];
    for v in vertices(&wall) {
        let outer = close(across(v, FOOT, FOOT_RADII), 1.0);
        assert!(outer || close(across(v, FOOT, inner), 1.0), "{v:?}");
    }
}

// spec MAP-10.5: a cone's base is on the box's bottom and its point the middle of the top
#[test]
fn a_cone_off_the_origin_rises_to_its_top_s_middle() {
    let cone = vertices(&off(Shape::Cone {
        sides: 12,
        mode: CircleMode::Vertices,
    }));
    assert_eq!(cone.len(), 13);
    assert!(cone.iter().any(|&v| near(v, [112.0, 80.0, 704.0])));
    for v in cone.iter().filter(|v| v[2] < 704.0) {
        assert_eq!(v[2], 512.0);
        assert!(close(across(*v, FOOT, FOOT_RADII), 1.0), "{v:?}");
    }
}

// spec MAP-10.6: an arch's curve is the ellipse in x and z about the middle of the box's
// bottom, from its start over its arc, its inner edge the wall's thickness in, through the box
// along y; filled spandrels reach the top over the whole width
#[test]
fn an_arch_follows_its_ellipse_from_its_start() {
    let arch = |start, arc, spandrels| {
        off(Shape::Arch {
            sides: 8,
            arc,
            start,
            thickness: 16.0,
            spandrels,
        })
    };
    let centre = [OFF_CENTRE[0], OFF_LOW[2]];
    let outer = [OFF_RADII[0], OFF_HIGH[2] - OFF_LOW[2]];
    let inner = [outer[0] - 16.0, outer[1] - 16.0];
    let in_xz = |v: [f64; 3]| [v[0], v[2], 0.0];
    let half = vertices(&arch(0.0, 180.0, Spandrels::Open));
    for &v in &half {
        assert!(v[1] == OFF_LOW[1] || v[1] == OFF_HIGH[1], "{v:?}");
        let o = across(in_xz(v), centre, outer);
        assert!(
            close(o, 1.0) || close(across(in_xz(v), centre, inner), 1.0),
            "{v:?}"
        );
    }
    assert!(half.iter().any(|&v| near(v, [208.0, 48.0, 512.0])));
    assert!(half.iter().any(|&v| near(v, [112.0, 48.0, 704.0])));
    let thirty = core::f64::consts::FRAC_PI_6;
    let first = [
        112.0 + 96.0 * thirty.cos(),
        48.0,
        512.0 + 192.0 * thirty.sin(),
    ];
    let turned = vertices(&arch(30.0, 90.0, Spandrels::Open));
    assert!(turned.iter().any(|&v| near(v, first)), "{turned:?}");
    assert!(!turned.iter().any(|&v| near(v, [208.0, 48.0, 512.0])));
    let filled = arch(0.0, 180.0, Spandrels::Filled);
    assert_eq!(filled.len(), 16, "a spandrel under each segment");
    let top = vertices(&filled);
    assert!(top.iter().any(|&v| near(v, [16.0, 48.0, 704.0])));
    assert!(top.iter().any(|&v| near(v, [208.0, 48.0, 704.0])));
}

// spec MAP-10.7: a spheroid's corners lie on the box's ellipsoid, a UV spheroid's poles at the
// middles of its top and bottom
#[test]
fn spheroids_lie_on_the_box_s_ellipsoid() {
    let uv = vertices(&off(Shape::UvSpheroid { sides: 8, rings: 4 }));
    assert_eq!(uv.len(), 8 * 3 + 2);
    for &v in &uv {
        assert!(close(round(v, OFF_CENTRE, OFF_RADII), 1.0), "{v:?}");
    }
    for pole in [
        [112.0, 80.0, 512.0],
        [112.0, 80.0, 704.0],
        [208.0, 80.0, 608.0],
    ] {
        assert!(uv.iter().any(|&v| near(v, pole)), "{pole:?}");
    }
    for (subdivisions, count) in [(0, 12), (1, 42)] {
        let ico = vertices(&off(Shape::Icospheroid { subdivisions }));
        assert_eq!(ico.len(), count);
        for v in ico {
            assert!(close(round(v, OFF_CENTRE, OFF_RADII), 1.0), "{v:?}");
        }
    }
}

// spec MAP-10.8: a rock's points lie between 0.7 of the box's ellipsoid and the ellipsoid,
// spread above and below its middle
#[test]
fn a_rock_lies_within_the_box_s_ellipsoid() {
    for seed in [0, 1, 99, u64::MAX] {
        let rock = vertices(&off(Shape::Rock { points: 32, seed }));
        assert!(rock.len() >= 4);
        for &v in &rock {
            let r = round(v, OFF_CENTRE, OFF_RADII);
            assert!(
                (0.49 - 1e-9..=1.0 + 1e-9).contains(&r),
                "{seed}: {v:?} at {r}"
            );
        }
        assert!(rock.iter().any(|v| v[2] > OFF_CENTRE[2] + 48.0));
        assert!(rock.iter().any(|v| v[2] < OFF_CENTRE[2] - 48.0));
    }
}

// spec MAP-10.9: a scalable curve fills its box on the grid; a box off the grid snaps to it;
// a hollow curve's inner corners are on the ellipse the wall's thickness in
#[test]
fn a_scalable_curve_fills_its_box_on_the_grid() {
    let curve = |a, b, wall| {
        Shape::Curve {
            sides: CurveSides::TwentyFour,
            grid: 16.0,
            wall,
        }
        .brushes(a, b, &surface())
        .unwrap()
    };
    let solid = curve(OFF_LOW, OFF_HIGH, Wall::Solid);
    assert_eq!(solid.len(), 1);
    let corners = vertices(&solid);
    for axis in 0..3 {
        let low = corners.iter().map(|v| v[axis]).fold(f64::MAX, f64::min);
        let high = corners.iter().map(|v| v[axis]).fold(f64::MIN, f64::max);
        assert_eq!((low, high), (OFF_LOW[axis], OFF_HIGH[axis]), "axis {axis}");
    }
    assert_eq!(
        curve([18.0, 50.0, 514.0], [206.0, 110.0, 702.0], Wall::Solid),
        solid
    );
    let fine = Shape::Curve {
        sides: CurveSides::FortyEight,
        grid: 1.0,
        wall: Wall::Hollow { thickness: 16.0 },
    }
    .brushes(OFF_LOW, OFF_HIGH, &surface())
    .unwrap();
    let inner = [FOOT_RADII[0] - 16.0, FOOT_RADII[1] - 16.0];
    for v in vertices(&fine) {
        let outer = (across(v, FOOT, FOOT_RADII) - 1.0).abs() < 0.1;
        assert!(outer || (across(v, FOOT, inner) - 1.0).abs() < 0.2, "{v:?}");
    }
}

// spec MAP-10.10: each step is the box from the floor to its top, the steps laid end to end
// from the side they climb from; a step that is no number is refused
#[test]
fn each_step_is_its_box() {
    let bounds = |brush: &Brush| {
        let v = brush.vertices();
        let pick = |f: fn(f64, f64) -> f64, start| {
            [0, 1, 2].map(|i| v.iter().map(|p| p[i]).fold(start, f))
        };
        [pick(f64::min, f64::MAX), pick(f64::max, f64::MIN)]
    };
    let [x0, y0, z0] = OFF_LOW;
    let [x1, y1, _] = OFF_HIGH;
    for facing in [Facing::East, Facing::West, Facing::North, Facing::South] {
        let steps = off(Shape::Stairs {
            height: 16.0,
            depth: 32.0,
            facing,
        });
        let expected = match facing {
            Facing::East | Facing::West => 6,
            Facing::North | Facing::South => 2,
        };
        assert_eq!(steps.len(), expected, "{facing:?}");
        for (i, step) in steps.iter().enumerate() {
            let i = f64::from(u32::try_from(i).unwrap());
            let (near, far, top) = (32.0 * i, 32.0 * (i + 1.0), z0 + 16.0 * (i + 1.0));
            let want = match facing {
                Facing::East => [[x0 + near, y0, z0], [x0 + far, y1, top]],
                Facing::West => [[x1 - far, y0, z0], [x1 - near, y1, top]],
                Facing::North => [[x0, y0 + near, z0], [x1, y0 + far, top]],
                Facing::South => [[x0, y1 - far, z0], [x1, y1 - near, top]],
            };
            assert_eq!(bounds(step), want, "{facing:?} step {i}");
        }
    }
    for (height, depth) in [
        (f64::NAN, 32.0),
        (16.0, f64::NAN),
        (f64::INFINITY, 32.0),
        (16.0, f64::INFINITY),
        (0.0, 32.0),
        (16.0, 0.0),
        (-16.0, 32.0),
    ] {
        assert!(
            matches!(
                Shape::Stairs {
                    height,
                    depth,
                    facing: Facing::East
                }
                .brushes(OFF_LOW, OFF_HIGH, &surface()),
                Err(ShapeError::Step)
            ),
            "{height} {depth}"
        );
    }
}

// spec MAP-10.11: spiral step `i` is the slice from corner `i` to the next, a step's height
// above the last, between the core's circle and the ellipse or from the middle
#[test]
fn each_spiral_step_is_its_slice() {
    let ring = super::ring(8, CircleMode::Vertices, FOOT, FOOT_RADII);
    let corner = |i: usize, z: f64| {
        let [x, y] = ring[i % 8];
        [x, y, z]
    };
    for core in [0.0, 8.0] {
        let steps = off(Shape::SpiralStairs {
            sides: 8,
            height: 16.0,
            core,
        });
        assert_eq!(steps.len(), 12);
        for (i, step) in steps.iter().enumerate() {
            let low = 512.0 + 16.0 * f64::from(u32::try_from(i).unwrap());
            let v = vertices(std::slice::from_ref(step));
            assert!(v.iter().all(|p| p[2] == low || p[2] == low + 16.0), "{i}");
            assert!(v.iter().any(|&p| near(p, corner(i, low))), "{i}");
            assert!(v.iter().any(|&p| near(p, corner(i + 1, low + 16.0))), "{i}");
            let middle = v.iter().any(|&p| near(p, [112.0, 80.0, low]));
            assert_eq!(middle, core == 0.0, "{i} with core {core}");
            if core > 0.0 {
                assert_eq!(
                    v.iter()
                        .filter(|p| close(across(**p, FOOT, [core, core]), 1.0))
                        .count(),
                    4
                );
            }
        }
    }
    for height in [0.0, -16.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            Shape::SpiralStairs {
                sides: 8,
                height,
                core: 0.0
            }
            .brushes(OFF_LOW, OFF_HIGH, &surface()),
            Err(ShapeError::Step)
        ));
    }
}

// spec MAP-10.9: the hull keeps each corner once, counter-clockwise from the lowest, none
// inside or on an edge
#[test]
fn the_hull_is_the_outside_corners() {
    let points = [
        [1.0, 1.0],
        [0.5, 0.5],
        [0.0, 0.0],
        [1.0, 0.0],
        [0.5, 0.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ];
    assert_eq!(
        super::hull_2d(&points),
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    );
    assert_eq!(super::hull_2d(&[[0.0, 0.0], [1.0, 1.0]]).len(), 2);
}

// spec MAP-10.7: a zero vector stays zero when made unit length; another is made length 1
#[test]
fn unit_leaves_zero_alone() {
    assert_eq!(super::unit([0.0; 3]), [0.0; 3]);
    assert_eq!(super::unit([0.0, 3.0, 4.0]), [0.0, 0.6, 0.8]);
}

// spec MAP-3.4: a face's plane faces away from the point inside; one on the plane leaves it
#[test]
fn a_face_faces_away_from_the_inside() {
    let points = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
    let plane = plane(points).unwrap();
    let above = [0.0, 0.0, 1.0];
    let facing = |inside| super::outward(points, inside).unwrap().side(above);
    assert!(facing([0.0, 0.0, -1.0]) > 0.0);
    assert!(facing([0.0, 0.0, 1.0]) < 0.0);
    assert_eq!(facing([0.5, 0.5, 0.0]), plane.side(above));
}

// spec MAP-10.1: each refusal says what was wrong and with which number
#[test]
fn each_refusal_names_its_number() {
    for (error, number) in [
        (ShapeError::Sides(2), "2"),
        (ShapeError::Rings(99), "99"),
        (ShapeError::Faces(1344), "1344"),
        (ShapeError::Subdivisions(4), "4"),
        (ShapeError::Points(70), "70"),
        (ShapeError::Thickness(-3.0), "-3"),
        (ShapeError::Grid(0.5), "0.5"),
        (ShapeError::CurveSides(13), "13"),
        (
            ShapeError::Arc {
                arc: 270.0,
                start: 5.0,
            },
            "270",
        ),
        (ShapeError::Step, "256"),
    ] {
        let text = error.to_string();
        assert!(text.contains(number), "{text}");
    }
    assert!(!ShapeError::Size.to_string().is_empty());
}

/// The unit normal of `brush`'s face `index`, as its three points give it.
fn normal(brush: &Brush, index: usize) -> [f64; 3] {
    plane(*brush.faces[index].points()).unwrap().normal
}

// spec MAP-10.2: a ring's corners run counter-clockwise from angle 0, so a prism's sides follow
// one another that way; a three-sided one has its first corner on +x
#[test]
fn a_ring_runs_counter_clockwise_from_plus_x() {
    let prism = off(Shape::Cylinder {
        sides: 3,
        mode: CircleMode::Vertices,
        wall: Wall::Solid,
    });
    let corners = vertices(&prism);
    assert!(corners.iter().any(|&v| near(v, [208.0, 80.0, 512.0])));
    assert!(!corners.iter().any(|&v| close(v[0], 16.0)), "{corners:?}");
    let sides = make(Shape::Cylinder {
        sides: 8,
        mode: CircleMode::Vertices,
        wall: Wall::Solid,
    })
    .unwrap();
    let angles: Vec<f64> = (0..sides[0].faces.len())
        .map(|i| normal(&sides[0], i))
        .filter(|n| n[2].abs() < 1e-6)
        .map(|n| n[1].atan2(n[0]))
        .collect();
    assert_eq!(angles.len(), 8);
    let step = core::f64::consts::TAU / 8.0;
    for pair in angles.windows(2) {
        let turn = (pair[1] - pair[0]).rem_euclid(core::f64::consts::TAU);
        assert!(close(turn, step), "{angles:?}");
    }
}

// spec MAP-10.7: a spheroid's first face is its first triangle: a UV spheroid's at the bottom
// pole between longitudes 0 and one slice, an icospheroid's the icosahedron's first
#[test]
fn a_spheroid_s_faces_start_where_its_triangles_do() {
    let unit = |v: [f64; 3]| {
        let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
        v.map(|c| c / length)
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let uv = make(Shape::UvSpheroid { sides: 8, rings: 4 }).unwrap();
    let half = core::f64::consts::PI / 8.0;
    let low = -3.0 * core::f64::consts::PI / 8.0;
    let toward = [low.cos() * half.cos(), low.cos() * half.sin(), low.sin()];
    assert!(
        dot(normal(&uv[0], 0), toward) > 0.99,
        "{:?}",
        normal(&uv[0], 0)
    );
    let ico = make(Shape::Icospheroid { subdivisions: 0 }).unwrap();
    let phi = core::f64::consts::GOLDEN_RATIO;
    let first = [[-1.0, phi, 0.0], [-phi, 0.0, 1.0], [0.0, 1.0, phi]];
    let middle = unit([0, 1, 2].map(|i| first.iter().map(|p| p[i]).sum::<f64>()));
    assert!(
        dot(normal(&ico[0], 0), middle) > 0.99,
        "{:?}",
        normal(&ico[0], 0)
    );
}

// spec MAP-10.8: a rock's points spiral down from its top, each the golden angle round from
// the one before; four points are all corners of its hull
#[test]
fn a_rock_s_points_spiral_down_by_the_golden_angle() {
    let mut rock = vertices(&off(Shape::Rock { points: 4, seed: 3 }));
    assert_eq!(rock.len(), 4);
    rock.sort_by(|a, b| b[2].total_cmp(&a[2]));
    for (i, v) in rock.iter().enumerate() {
        let angle = ((v[1] - FOOT[1]) / FOOT_RADII[1]).atan2((v[0] - FOOT[0]) / FOOT_RADII[0]);
        let want = (super::GOLDEN_ANGLE * f64::from(u32::try_from(i).unwrap()))
            .rem_euclid(core::f64::consts::TAU);
        let got = angle.rem_euclid(core::f64::consts::TAU);
        assert!(close(got, want), "point {i}: {got} for {want}");
        assert_eq!(v[2] > OFF_CENTRE[2], i < 2, "point {i}");
    }
}

// spec MAP-10.9: a box narrower than the grid makes a curve one grid step each way
#[test]
fn a_curve_is_at_least_a_grid_step() {
    let tiny = Shape::Curve {
        sides: CurveSides::Twelve,
        grid: 16.0,
        wall: Wall::Solid,
    }
    .brushes([0.0, 0.0, 0.0], [4.0, 4.0, 4.0], &surface())
    .unwrap();
    let corners = vertices(&tiny);
    for axis in 0..3 {
        let high = corners.iter().map(|v| v[axis]).fold(f64::MIN, f64::max);
        let low = corners.iter().map(|v| v[axis]).fold(f64::MAX, f64::min);
        assert_eq!((low, high), (0.0, 16.0), "axis {axis}");
    }
}

// spec MAP-10.10: the box's height can limit the steps, from its floor up
#[test]
fn a_low_box_limits_the_steps() {
    let steps = off(Shape::Stairs {
        height: 64.0,
        depth: 32.0,
        facing: Facing::East,
    });
    assert_eq!(steps.len(), 3);
}
