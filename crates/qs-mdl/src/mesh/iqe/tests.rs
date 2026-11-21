// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::*;

/// An arm: a root joint at the origin and a joint one unit along x, a triangle whose first
/// corner is bound to the root, its second to the arm and its third to both in halves, and
/// an animation of two frames in the second of which the arm is turned a quarter about z.
const ARM: &str = "\
# Inter-Quake Export

joint \"root\" -1
\tpq 0 0 0 0 0 0 1
joint \"fore arm\" 0
\tpq 1 0 0 0 0 0 1

mesh \"arm\"
material \"skin\"
vp 0 0 0
\tvt 0 0
\tvn 0 0 1
\tvb 0 1
vp 2 0 0
\tvt 1 0
\tvb 1 1
vp 2 0 0
\tvt 1 1
\tvb 0 0.5 1 0.5 2 0
fm 0 1 2

animation \"wave\"
framerate 10
frame
pq 0 0 0 0 0 0 1
pq 1 0 0 0 0 0 1
frame
pq 0 0 0 0 0 0 1
pq 1 0 0 0 0 0.70710678 0.70710678
";

fn near(at: [f32; 3], want: [f32; 3]) -> bool {
    at.into_iter()
        .zip(want)
        .all(|(at, want)| (at - want).abs() < 1e-5)
}

// spec MDL-9.2: each frame of an animation is a pose of the mesh, named by the animation
// and its number from 1; a vertex goes where the joints it is blended to take it from where
// the base poses had it, by their weights; a name in quotes is one word
#[test]
fn a_frame_poses_the_mesh_by_its_joints() {
    let mesh = read(ARM, Up::Z).unwrap();
    let names: Vec<&str> = mesh
        .frames
        .iter()
        .map(|frame| frame.name.as_str())
        .collect();
    assert_eq!(names, ["wave1", "wave2"]);
    assert_eq!(
        mesh.triangles,
        [[0, 1, 2].map(|vertex| Corner {
            position: vertex,
            texcoord: Some(vertex)
        })]
    );
    assert_eq!(mesh.texcoords, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
    // The first frame is the base pose: every vertex where the file has it.
    for (at, want) in
        mesh.frames[0]
            .positions
            .iter()
            .zip([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 0.0, 0.0]])
    {
        assert!(near(*at, want), "{at:?}");
    }
    // The second: the arm's vertex, one unit along the arm, is turned about the arm's joint
    // to (1, 1, 0); the vertex of both is half way between where each takes it.
    let second = &mesh.frames[1].positions;
    assert!(near(second[0], [0.0, 0.0, 0.0]), "{second:?}");
    assert!(near(second[1], [1.0, 1.0, 0.0]), "{second:?}");
    assert!(near(second[2], [1.5, 0.5, 0.0]), "{second:?}");
}

// spec MDL-9.2: a file with no animation is one pose named base; with no face, each three
// vertices of a mesh are a triangle; fa counts from the file's first vertex and fm from the
// mesh's, and below 0 both count back from the last vertex; more corners are a fan
#[test]
fn faces_name_vertices_three_ways() {
    let plain = "# Inter-Quake Export\nmesh a\nvp 0 0 0\nvp 1 0 0\nvp 0 1 0\nvp 9 9 9\n\
                 mesh b\nvp 0 0 1\nvp 1 0 1\nvp 0 1 1\n";
    let mesh = read(plain, Up::Z).unwrap();
    assert_eq!(mesh.frames.len(), 1);
    assert_eq!(mesh.frames[0].name, IQE_FRAME);
    let faces = |mesh: &Mesh| -> Vec<[usize; 3]> {
        mesh.triangles
            .iter()
            .map(|corners| corners.map(|corner| corner.position))
            .collect()
    };
    // The fourth vertex of the first mesh is no whole triangle and is left.
    assert_eq!(faces(&mesh), [[0, 1, 2], [4, 5, 6]]);
    assert!(!mesh.has_texcoords());

    let faced = format!("{plain}fm 0 1 2\nfa 0 1 2 3\nfa -1 -2 -3\n");
    let mesh = read(&faced, Up::Z).unwrap();
    assert_eq!(faces(&mesh), [[4, 5, 6], [0, 1, 2], [0, 2, 3], [6, 5, 4]]);
    // Everything after a comment command is the comment.
    let commented = format!("{plain}comment\nfa 0 0\nnot a line of the format\n");
    assert_eq!(faces(&read(&commented, Up::Z).unwrap()).len(), 2);
}

// spec MDL-9.2: a pq pose with no w has the w below 0 that makes a unit quaternion; a pm
// pose is its matrix's rows; a pose scales a point, then turns it, then moves it
#[test]
fn poses_are_read_as_the_format_says() {
    let posed = |pose: &str| {
        let text = format!(
            "# Inter-Quake Export\njoint j -1\npq 0 0 0 0 0 0 1\nvp 1 0 0\nvb 0 1\nvp 0 1 0\n\
             vb 0 1\nvp 0 0 1\nvb 0 1\nanimation a\nframe\n{pose}\n"
        );
        read(&text, Up::Z).unwrap().frames[0].positions.clone()
    };
    // (0, 0, sin 45) with w = -cos 45: a quarter turn the other way about z.
    let back = posed("pq 0 0 0 0 0 0.70710678");
    assert!(near(back[0], [0.0, -1.0, 0.0]), "{back:?}");
    // The matrix that takes x to y, with a move and a scale of 2 on x.
    let turned = posed("pm 5 0 0  0 -1 0  1 0 0  0 0 1  2 1 1");
    assert!(near(turned[0], [5.0, 2.0, 0.0]), "{turned:?}");
    assert!(near(turned[1], [4.0, 0.0, 0.0]), "{turned:?}");
    assert!(near(turned[2], [5.0, 0.0, 1.0]), "{turned:?}");
    // A scale, then the turn, then the move.
    let all = posed("pq 0 0 3 0 0 0.70710678 0.70710678 2 2 2");
    assert!(near(all[0], [0.0, 2.0, 3.0]), "{all:?}");
}

// spec MDL-9.2: a frame's name is its animation's letters, digits and underscores, cut so
// that the name with its number is at most 15 bytes; an animation with no name is anim
#[test]
fn frames_are_named_by_animation_and_number() {
    assert_eq!(frame_name("run", 3), "run3");
    assert_eq!(frame_name("a long animation name", 12), "alonganimatio12");
    assert_eq!(frame_name("", 1), "anim1");
    assert_eq!(frame_name("death (b)", 7), "deathb7");
}

// spec MDL-9.2: a file without the header, an argument that is no number, a face that is
// not three vertices of the file, a joint whose parent is not above it, a pa pose and a
// base pose of no size are refused, with the line
#[test]
fn what_is_no_iqe_is_refused_with_its_line() {
    let refused = |body: &str| read(&format!("# Inter-Quake Export\n{body}"), Up::Z).unwrap_err();
    assert_eq!(
        read("vp 0 0 0\n", Up::Z).unwrap_err(),
        IqeError {
            line: 1,
            fault: IqeFault::Header
        }
    );
    for (body, line, fault) in [
        ("vp 0 zero 0\n", 2, IqeFault::Number),
        ("vp 0 0 0\nvb 0 heavy\n", 3, IqeFault::Number),
        ("vp 0 0 0\nvp 1 0 0\nfa 0 1\n", 4, IqeFault::Face),
        ("vp 0 0 0\nvp 1 0 0\nfa 0 1 2\n", 4, IqeFault::Face),
        ("vp 0 0 0\nvp 1 0 0\nfa 0 1 -3\n", 4, IqeFault::Face),
        ("joint a 0\n", 2, IqeFault::Joint),
        ("joint a -1\njoint b 5\n", 3, IqeFault::Joint),
        ("joint a -1\npa 0 0 0 0 0 0\n", 3, IqeFault::Angles),
        ("joint a -1\npq 0 0 0 0 0 0 1 0 1 1\n", 0, IqeFault::Flat),
    ] {
        assert_eq!(refused(body), IqeError { line, fault }, "{body}");
    }
    assert!(
        refused("joint a -1\npa 0 0 0 0 0 0\n")
            .to_string()
            .contains("write poses as pq")
    );
}
