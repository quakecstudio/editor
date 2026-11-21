// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::super::recorded::agrees;
use super::*;

fn model(mesh: &str, animation: &str) -> (Md5Mesh, Vec<(String, Md5Anim)>) {
    (
        Md5Mesh::read(mesh.as_bytes()).unwrap(),
        vec![(
            "wave".to_owned(),
            Md5Anim::read(animation.as_bytes()).unwrap(),
        )],
    )
}

// spec MDL-10.1, MDL-10.2: an md5 model and its animation are the mesh FTEQW's converter
// makes of them: every triangle of every frame, with its texture coordinates, for a model
// whose joints stand straight and for one whose arm's joint is turned in the base pose
#[test]
fn an_md5_model_is_the_converters() {
    for (mesh_text, animation, recorded) in [
        (
            include_str!("../fixtures/arm.md5mesh.txt"),
            include_str!("../fixtures/wave.md5anim.txt"),
            include_str!("../fixtures/md5.fteqw.txt"),
        ),
        (
            include_str!("../fixtures/hard-arm.md5mesh.txt"),
            include_str!("../fixtures/hard-wave.md5anim.txt"),
            include_str!("../fixtures/hard-md5.fteqw.txt"),
        ),
    ] {
        let (md5, animations) = model(mesh_text, animation);
        let made = md5.mesh(&animations, Up::Z).unwrap();
        let names: Vec<&str> = made
            .frames
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        assert_eq!(names, ["wave1", "wave2", "wave3"]);
        assert!(made.has_texcoords());
        agrees(&made, recorded);
    }
}

// spec MDL-10.2: a vertex is where its weights put it by the mesh's joints; in a frame the
// arm's vertex is turned about the arm's joint and lifted with the root; with no animation
// the mesh is one pose, base
#[test]
fn an_md5_vertex_follows_its_joints() {
    let (md5, animations) = model(
        include_str!("../fixtures/arm.md5mesh.txt"),
        include_str!("../fixtures/wave.md5anim.txt"),
    );
    let still = md5.mesh(&[], Up::Z).unwrap();
    assert_eq!(still.frames.len(), 1);
    assert_eq!(still.frames[0].name, MD5_FRAME);
    assert_eq!(still.frames[0].positions[3], [16.0, 0.0, 0.0]);
    assert_eq!(still.frames[0].positions[4], [8.0, 2.0, 0.0]);

    let made = md5.mesh(&animations, Up::Z).unwrap();
    let near = |at: [f32; 3], want: [f32; 3]| {
        at.iter()
            .zip(want)
            .all(|(at, want)| (at - want).abs() < 1e-4)
    };
    // The last frame: the arm a quarter turn about z at (8, 0, 0), the root up by 2.
    let last = &made.frames[2].positions;
    assert!(near(last[3], [8.0, 8.0, 2.0]), "{last:?}");
    // The vertex of both joints is half way between where each takes it.
    assert!(near(last[4], [7.0, 1.0, 2.0]), "{last:?}");
    assert!(near(last[1], [0.0, 4.0, 2.0]), "{last:?}");
}

// spec MDL-10.2: an animation of another number of joints than the mesh is refused, by its
// name
#[test]
fn an_animation_of_other_joints_is_refused() {
    let (md5, _) = model(
        include_str!("../fixtures/arm.md5mesh.txt"),
        include_str!("../fixtures/wave.md5anim.txt"),
    );
    let one = "MD5Version 10\ncommandline \"\"\nnumFrames 1\nnumJoints 1\nframeRate 10\n\
               numAnimatedComponents 0\nhierarchy {\n\"root\" -1 0 0\n}\nbounds {\n\
               ( 0 0 0 ) ( 1 1 1 )\n}\nbaseframe {\n( 0 0 0 ) ( 0 0 0 )\n}\nframe 0 {\n}\n";
    let other = vec![("nod".to_owned(), Md5Anim::read(one.as_bytes()).unwrap())];
    let error = md5.mesh(&other, Up::Z).unwrap_err();
    assert_eq!(
        error,
        Md5MeshError::Joints {
            animation: "nod".to_owned()
        }
    );
    assert_eq!(
        error.to_string(),
        "the animation nod is not of as many joints as the mesh"
    );
}
