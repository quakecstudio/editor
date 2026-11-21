// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::super::recorded::agrees;
use super::*;

// spec MDL-10.1, MDL-10.3: an smd model and its animation are the mesh FTEQW's converter
// makes of them, for a plain model and for one whose base pose is turned, whose animation
// skips a time and whose last frame gives the arm no pose
#[test]
fn an_smd_model_is_the_converters() {
    for (model, animation, recorded) in [
        (
            include_str!("../fixtures/arm.smd"),
            include_str!("../fixtures/wave.smd"),
            include_str!("../fixtures/smd.fteqw.txt"),
        ),
        (
            include_str!("../fixtures/hard-arm.smd"),
            include_str!("../fixtures/hard-wave.smd"),
            include_str!("../fixtures/hard-smd.fteqw.txt"),
        ),
    ] {
        let model = Smd::read(model).unwrap();
        let animations = vec![("wave".to_owned(), Smd::read(animation).unwrap())];
        let made = model.mesh(&animations, Up::Z).unwrap();
        let names: Vec<&str> = made
            .frames
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        assert_eq!(names, ["wave1", "wave2", "wave3"]);
        agrees(&made, recorded);
    }
}

// spec MDL-10.3: a triangle's three vertices are kept last first, v is measured up the
// picture, and a vertex's links to its own joint are summed with what the weights lack of
// 1; comments and a vertexanimation section are passed over
#[test]
fn a_triangle_is_read_as_the_converter_reads_one() {
    let text = "// a model\nversion 1\nnodes\n0 \"root\" -1\n1 \"arm bone\" 0\n1 \"again\" 0\n\
                300 \"far\" 0\nend\n# the base pose\nskeleton\ntime 0\n0 0 0 0 0 0 0\n\
                1 8 0 0 0 0 0\nend\nvertexanimation\ntime 0\n0 9 9 9 0 0 1\nend\ntriangles\n\
                skin.bmp\n\
                0 1 0 0 0 0 1 0.25 0.75\n\
                0 2 0 0 0 0 1 0.5 0.5 2 0 0.25 1 0.25\n\
                1 3 0 0 0 0 1 1 0 3 1 0.5 0 0.3 1 0.2\n\
                end\n";
    let model = Smd::read(text).unwrap();
    assert_eq!(
        model.parents,
        [None, Some(0)],
        "two joints, the rest left out"
    );
    let made = model.mesh(&[], Up::Z).unwrap();
    assert_eq!(made.frames.len(), 1);
    assert_eq!(made.frames[0].name, SMD_FRAME);
    // The third vertex's line is the first corner.
    assert_eq!(
        made.frames[0].positions,
        [[3.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 0.0]]
    );
    assert_eq!(made.texcoords, [[1.0, 1.0], [0.5, 0.5], [0.25, 0.25]]);
    // The second line's links: 0.25 of its own joint and 0.25 of the arm, and the half the
    // weights lack is its own joint's: three quarters and a quarter.
    assert_eq!(model.vertices[1].blends, [(0, 0.75), (1, 0.25)]);
    // The third's: its own joint, the arm, twice, 0.7 in all, and the root 0.3.
    let third = &model.vertices[0].blends;
    assert_eq!(third[0].0, 1);
    assert!(
        (third[0].1 - 0.7).abs() < 1e-9 && (third[1].1 - 0.3).abs() < 1e-9,
        "{third:?}"
    );
    // A vertex with no links follows its own joint alone.
    assert_eq!(model.vertices[2].blends, [(0, 1.0)]);
}

// spec MDL-10.3: a file of another version is refused; a file with no triangles is an
// animation and no model; a section that ends at a line of too few numbers keeps what was
// whole before it
#[test]
fn what_is_no_smd_model_is_refused() {
    assert_eq!(Smd::read("version 2\n").unwrap_err(), SmdError::Version);
    let animation = Smd::read(include_str!("../fixtures/wave.smd")).unwrap();
    assert_eq!(
        animation.mesh(&[], Up::Z).unwrap_err(),
        SmdError::NoTriangles
    );
    assert!(
        SmdError::NoTriangles
            .to_string()
            .contains("it is an animation")
    );
    let cut = "version 1\nnodes\n0 \"root\" -1\nend\ntriangles\nskin\n0 0 0 0 0 0 1 0 0\n\
               0 1 0 0 0 0 1 0 0\n0 0 1 0 0 0 1 0 0\nskin\n0 0 0 0 0 0 1 0 0\n0 1 0 0\n";
    assert_eq!(Smd::read(cut).unwrap().vertices.len(), 3);
}
