// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;

const SQUARE: &str = "\
# a square and a triangle
o thing
v 0 0 0
v 1 0 0   # the second
v 1 1 0
v 0 1 0
vt 0 0
vt 1 0
vt 1 1
vt 0 0.25
vn 0 0 1
usemtl stone
s off
f 1/1/1 2/2/1 3/3/1 4/4/1
f -4 -3 -1
";

fn corners(mesh: &Mesh, triangle: usize) -> [(usize, Option<usize>); 3] {
    mesh.triangles[triangle].map(|corner| (corner.position, corner.texcoord))
}

// spec MDL-9.1: vertices and texture vertices are numbered from 1 down the file; a face of
// more than three corners is a fan from its first, each triangle's corners turned to go
// clockwise; a statement that is none of the three, and a comment, are passed over
#[test]
fn a_face_is_a_fan_of_clockwise_triangles() {
    let mesh = read(SQUARE, Up::Z).unwrap();
    assert_eq!(mesh.frames.len(), 1);
    assert_eq!(mesh.frames[0].name, OBJ_FRAME);
    assert_eq!(
        mesh.frames[0].positions,
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0]
        ]
    );
    assert_eq!(mesh.triangles.len(), 3);
    assert_eq!(
        corners(&mesh, 0),
        [(0, Some(0)), (2, Some(2)), (1, Some(1))]
    );
    assert_eq!(
        corners(&mesh, 1),
        [(0, Some(0)), (3, Some(3)), (2, Some(2))]
    );
    // References below 0 count back up the file from the face: -1 is the fourth vertex.
    assert_eq!(corners(&mesh, 2), [(0, None), (3, None), (1, None)]);
    assert!(!mesh.has_texcoords(), "one face has no texture vertices");
}

// spec MDL-9.1: a texture vertex's v, measured up the picture, is 1 - v down it, and one
// with no v has 0; with y up a position (x, y, z) is Quake's (-z, -x, y)
#[test]
fn texture_vertices_and_axes_are_turned() {
    let mesh = read(SQUARE, Up::Y).unwrap();
    assert_eq!(
        mesh.texcoords,
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.75]]
    );
    assert_eq!(mesh.frames[0].positions[2], [-0.0, -1.0, 1.0]);
    let lone = read("v 1 2 3\nvt 0.5\n", Up::Y).unwrap();
    assert_eq!(lone.texcoords, [[0.5, 1.0]]);
    assert_eq!(lone.frames[0].positions, [[-3.0, -1.0, 2.0]]);
    assert_eq!(Up::default(), Up::Y);
}

// spec MDL-9.1: a vertex without three numbers, a face of fewer than three corners and a
// reference to a vertex the file does not have above the face are refused, with the line
#[test]
fn what_is_no_mesh_is_refused_with_its_line() {
    let refused = |text: &str| read(text, Up::Z).unwrap_err();
    for (text, line, fault) in [
        ("v 1 2\n", 1, ObjFault::Number),
        ("v 1 2 x\n", 1, ObjFault::Number),
        ("v 0 0 0\nvt nan\n", 2, ObjFault::Number),
        ("v 0 0 0\nv 1 0 0\nf 1 2\n", 3, ObjFault::Face),
        ("v 0 0 0\nv 1 0 0\nf 1 2 3\n", 3, ObjFault::Reference),
        ("v 0 0 0\nv 1 0 0\nf 1 2 0\n", 3, ObjFault::Reference),
        ("v 0 0 0\nv 1 0 0\nf 1 2 -3\n", 3, ObjFault::Reference),
        (
            "v 0 0 0\nv 1 0 0\nv 1 1 0\nf 1/1 2/1 3/1\n",
            4,
            ObjFault::Reference,
        ),
        ("v 0 0 0\nf 1 1 one\n", 2, ObjFault::Reference),
    ] {
        assert_eq!(refused(text), ObjError { line, fault }, "{text}");
    }
    assert_eq!(
        refused("f 1 2\n").to_string(),
        "line 1: a face refers to a vertex the file does not have above it"
    );
    // A file with nothing in it is a mesh with no triangle.
    assert!(read("", Up::Z).unwrap().triangles.is_empty());
}
