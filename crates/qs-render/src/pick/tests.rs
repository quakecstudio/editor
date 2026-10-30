// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::intersect;
use crate::camera::Camera;
use crate::mesh::tests::{MapSpec, bsp, palette, samples};
use crate::mesh::{MapMesh, MeshBuilder, Placement};

/// The test map's one face, 64 by 32 in the z = 0 plane, as the world, and the same face
/// again as a second model placed 10 units up.
fn two_floors() -> MapMesh {
    let lighting = samples(&[50]);
    let world = bsp(&MapSpec::default(), &lighting);
    let palette = palette();
    let mut builder = MeshBuilder::new(&palette);
    let raised = Placement {
        origin: [0.0, 0.0, 10.0],
        ..Placement::WORLD
    };
    builder.add(&world, &[Placement::WORLD, raised]).unwrap();
    builder.finish()
}

const DOWN: [f32; 3] = [0.0, 0.0, -1.0];

#[test]
fn a_ray_hits_the_nearest_surface_where_it_is_drawn() {
    let mesh = two_floors();
    let hit = mesh.pick([16.0, 8.0, 100.0], DOWN, &[]).unwrap();
    assert_eq!(hit.model, 1, "the raised floor is nearer");
    assert_eq!(hit.texture, 0);
    assert_eq!(hit.distance, 90.0);
    assert_eq!(hit.point, [16.0, 8.0, 10.0]);
    assert_eq!(hit.triangle % 3, 0);
    // From below, the world's floor is nearer, hit from its back.
    let from_below = mesh.pick([16.0, 8.0, -5.0], [0.0, 0.0, 1.0], &[]).unwrap();
    assert_eq!((from_below.model, from_below.distance), (0, 5.0));
    // Matrices given for this frame place the models: the second moved out of the way.
    let moved = Placement {
        origin: [500.0, 0.0, 10.0],
        ..Placement::WORLD
    };
    let now = [Placement::WORLD.matrix(), moved.matrix()];
    let hit = mesh.pick([16.0, 8.0, 100.0], DOWN, &now).unwrap();
    assert_eq!((hit.model, hit.distance), (0, 100.0));
    let hit = mesh.pick([516.0, 8.0, 100.0], DOWN, &now).unwrap();
    assert_eq!(hit.model, 1);
}

#[test]
fn a_ray_that_misses_or_points_away_hits_nothing() {
    let mesh = two_floors();
    assert_eq!(
        mesh.pick([100.0, 8.0, 100.0], DOWN, &[]),
        None,
        "beside the floor"
    );
    assert_eq!(
        mesh.pick([16.0, 8.0, 100.0], [0.0, 0.0, 1.0], &[]),
        None,
        "away"
    );
    assert_eq!(
        mesh.pick([16.0, 8.0, 5.0], [1.0, 0.0, 0.0], &[]),
        None,
        "along it"
    );
    let empty = MeshBuilder::new(&palette()).finish();
    assert_eq!(empty.pick([0.0; 3], DOWN, &[]), None);
}

#[test]
fn a_triangle_is_hit_inside_and_on_its_edges_only() {
    let triangle = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
    let at = |x: f32, y: f32| intersect([x, y, 1.0], DOWN, triangle);
    assert_eq!(at(1.0, 1.0), Some(1.0));
    assert_eq!(at(0.0, 0.0), Some(1.0), "a corner");
    assert_eq!(at(5.0, 5.0), Some(1.0), "the long edge");
    assert_eq!(at(-1.0, 1.0), None, "left of it");
    assert_eq!(at(1.0, -1.0), None, "below it");
    assert_eq!(at(6.0, 6.0), None, "past the long edge");
    assert_eq!(
        intersect([1.0, 1.0, 0.0], DOWN, triangle),
        None,
        "on it, not in front"
    );
}

#[test]
fn the_camera_ray_goes_through_the_point_of_the_image() {
    let camera = Camera::new([1.0, 2.0, 3.0], 0.0);
    let (origin, centre) = camera.ray(1.0, [0.0, 0.0]);
    assert_eq!(origin, [1.0, 2.0, 3.0]);
    assert_eq!(centre, [1.0, 0.0, 0.0], "straight ahead at yaw 0");
    // fov 90: the right edge is 45 degrees right, which is -y in Quake's axes.
    let (_, right) = camera.ray(1.0, [1.0, 0.0]);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (right[0] - half).abs() < 1e-6 && (right[1] + half).abs() < 1e-6,
        "{right:?}"
    );
    // A wide image is as wide but less tall: the top edge of a 2:1 image is less steep.
    let (_, top) = camera.ray(2.0, [0.0, 1.0]);
    assert!(top[2] > 0.0 && top[2] < half, "{top:?}");
    assert!(
        (top.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-6,
        "a unit vector"
    );
}
