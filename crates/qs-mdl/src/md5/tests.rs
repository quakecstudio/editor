// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::fixtures::{AnimSpec, MeshSpec, mesh_with_tri};
use crate::md5::{
    MD5_ANIM_FLAGS, MD5_FRAME_INTERVAL, MD5_MAGIC, MD5_MAX_SKIN_FRAMES, MD5_NO_PARENT, MD5_VERSION,
    Md5Anim, Md5Error, Md5Mesh, Md5Pose, Md5Vert, Md5Weight, Tokens, md5_anim_path, md5_quat_w,
    md5_skin_path,
};

fn tokens(text: &[u8]) -> Vec<Vec<u8>> {
    Tokens::new(text).map(<[u8]>::to_vec).collect()
}

fn words(list: &[&str]) -> Vec<Vec<u8>> {
    list.iter().map(|word| word.as_bytes().to_vec()).collect()
}

fn mesh(spec: &MeshSpec) -> Result<Md5Mesh, Md5Error> {
    Md5Mesh::read(&spec.build())
}

fn anim(spec: &AnimSpec) -> Result<Md5Anim, Md5Error> {
    Md5Anim::read(&spec.build())
}

fn with_mesh(body: String) -> MeshSpec {
    MeshSpec {
        meshes: vec![body],
        ..MeshSpec::default()
    }
}

// spec MD5-1.1: reading stops at the first NUL.
#[test]
fn nul_ends_the_file() {
    assert_eq!(tokens(b"a b\0c d"), words(&["a", "b"]));
    assert!(tokens(b"\0a").is_empty());
    assert!(tokens(b"").is_empty());
    let mut bytes = MeshSpec::default().build();
    let at = bytes.windows(7).position(|w| w == b"numtris").unwrap();
    bytes[at] = 0;
    assert_eq!(
        Md5Mesh::read(&bytes),
        Err(Md5Error::Expected { keyword: "numtris" })
    );
}

// spec MD5-1.2: bytes up to 0x20 and from 0x80 separate tokens.
#[test]
fn whitespace_includes_high_bytes() {
    assert_eq!(
        tokens(b"a\x01b\tc\x20d\x7Fe"),
        words(&["a", "b", "c", "d\x7Fe"])
    );
    assert_eq!(tokens(b"a\x80b\xFFc"), words(&["a", "b", "c"]));
    // Inside quotes high bytes are kept.
    assert_eq!(tokens(b"\"a\xC9b\""), vec![b"a\xC9b".to_vec()]);
}

// spec MD5-1.3: line and block comments are skipped.
#[test]
fn comments_are_skipped() {
    assert_eq!(tokens(b"a // b c\nd"), words(&["a", "d"]));
    assert_eq!(tokens(b"a /* b\nc */d"), words(&["a", "d"]));
    assert_eq!(tokens(b"a /* b"), words(&["a"]));
    assert_eq!(tokens(b"a//b"), words(&["a//b"]));
    assert_eq!(tokens(b"/**/a/ b"), words(&["a/", "b"]));
    assert_eq!(tokens(b"// only"), Vec::<Vec<u8>>::new());
}

// spec MD5-1.4: quoted tokens run to the next quote or the end.
#[test]
fn quoted_tokens() {
    assert_eq!(tokens(b"\"a b\" c"), words(&["a b", "c"]));
    assert_eq!(tokens(b"\"\" c"), words(&["", "c"]));
    assert_eq!(tokens(b"\"a{b}"), words(&["a{b}"]));
    assert_eq!(tokens(b"x\"y\""), words(&["x\"y\""]));
}

// spec MD5-1.5: single-character tokens and where words end.
#[test]
fn single_characters_split_words() {
    assert_eq!(tokens(b"abc(d)e"), words(&["abc", "(", "d", ")", "e"]));
    assert_eq!(tokens(b"{}':"), words(&["{", "}", "'", ":"]));
    assert_eq!(tokens(b"a:b"), words(&["a:b"]));
    assert_eq!(tokens(b"a'b{c}"), words(&["a", "'", "b", "{", "c", "}"]));
}

// spec MD5-1.6: keywords match quoted tokens, and case matters.
#[test]
fn keywords_match_exactly() {
    let text = MeshSpec::default().build();
    let quoted = String::from_utf8(text)
        .unwrap()
        .replace("numJoints", "\"numJoints\"");
    assert!(Md5Mesh::read(quoted.as_bytes()).is_ok());
    let text = String::from_utf8(MeshSpec::default().build())
        .unwrap()
        .replace("numJoints", "numjoints");
    assert_eq!(
        Md5Mesh::read(text.as_bytes()),
        Err(Md5Error::Expected {
            keyword: "numJoints"
        })
    );
}

// spec MD5-1.7: integers are optionally signed decimal digits that fit.
#[test]
fn integers_are_strict() {
    for count in ["+1", "01"] {
        let spec = MeshSpec {
            num_joints: count,
            ..MeshSpec::default()
        };
        assert!(mesh(&spec).is_ok(), "{count}");
    }
    for count in ["0x1", "1a", "-1", "", "18446744073709551616", "one", "1.0"] {
        let spec = MeshSpec {
            num_joints: count,
            ..MeshSpec::default()
        };
        let got = mesh(&spec);
        assert!(
            matches!(
                got,
                Err(Md5Error::BadNumber { .. } | Md5Error::Expected { .. })
            ),
            "{count}: {got:?}"
        );
    }
    let spec = MeshSpec {
        num_joints: "-1",
        ..MeshSpec::default()
    };
    assert_eq!(
        mesh(&spec),
        Err(Md5Error::BadNumber {
            token: b"-1".to_vec()
        })
    );
    assert_eq!(
        Md5Error::BadNumber {
            token: b"x".to_vec()
        }
        .to_string(),
        "\"x\" is not a number"
    );
    // A parent is signed 64-bit.
    let spec = MeshSpec {
        joints: vec!["\"root\" -9223372036854775809 ( 1 2 3 ) ( 0 0 0 )".to_owned()],
        ..MeshSpec::default()
    };
    assert!(matches!(mesh(&spec), Err(Md5Error::BadNumber { .. })));
}

// spec MD5-1.8: floats as Rust's f64 parser reads them, rounded to f32.
#[test]
fn floats_round_to_f32() {
    let spec = MeshSpec {
        joints: vec!["\"root\" -1 ( 1e2 -0.5 0.1 ) ( 0 0 0 )".to_owned()],
        ..MeshSpec::default()
    };
    let got = mesh(&spec).unwrap();
    assert_eq!(
        got.joints[0].position.map(f32::to_bits),
        [100.0_f32, -0.5, 0.1].map(f32::to_bits)
    );
    let spec = MeshSpec {
        joints: vec!["\"root\" -1 ( inf NaN 1e40 ) ( 0 0 0 )".to_owned()],
        ..MeshSpec::default()
    };
    let position = mesh(&spec).unwrap().joints[0].position;
    assert!(position[0].is_infinite() && position[1].is_nan() && position[2].is_infinite());
    for bad in ["0x1p3", "1,5", "abc"] {
        let spec = MeshSpec {
            joints: vec![format!("\"root\" -1 ( {bad} 0 0 ) ( 0 0 0 )")],
            ..MeshSpec::default()
        };
        assert!(
            matches!(mesh(&spec), Err(Md5Error::BadNumber { .. })),
            "{bad}"
        );
    }
}

// spec MD5-1.9: the end of the file where a token is required fails.
#[test]
fn early_end_fails() {
    let bytes = MeshSpec::default().build();
    let mut ends = 0;
    // The last two bytes are the final `}` and its newline.
    for len in 4..bytes.len() - 2 {
        let got = Md5Mesh::read(&bytes[..len]);
        if got == Err(Md5Error::UnexpectedEnd) {
            ends += 1;
        }
        assert!(got.is_err(), "length {len}");
    }
    assert!(ends > 0);
    assert_eq!(
        Md5Mesh::read(b"MD5Version 10 numJoints"),
        Err(Md5Error::UnexpectedEnd)
    );
    assert_eq!(
        Md5Error::UnexpectedEnd.to_string(),
        "ends where a token is required"
    );
}

// spec MD5-2.1: a mesh file starts with `MD5V`.
#[test]
fn mesh_must_start_with_magic() {
    assert_eq!(&MD5_MAGIC, b"MD5V");
    let bytes = MeshSpec::default().build();
    assert!(Md5Mesh::read(&bytes).is_ok());
    let mut spaced = b" ".to_vec();
    spaced.extend_from_slice(&bytes);
    assert_eq!(Md5Mesh::read(&spaced), Err(Md5Error::NotMd5));
    assert_eq!(Md5Mesh::read(b"MD5"), Err(Md5Error::NotMd5));
    assert_eq!(Md5Mesh::read(b"IDPO"), Err(Md5Error::NotMd5));
    // The magic alone is not the keyword.
    assert_eq!(
        Md5Mesh::read(b"MD5Vx 10"),
        Err(Md5Error::Expected {
            keyword: "MD5Version"
        })
    );
}

// spec MD5-2.2: the header, with an optional commandline.
#[test]
fn mesh_header() {
    assert_eq!(MD5_VERSION, "10");
    let got = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(got.commandline, Some(b"exported".to_vec()));
    let text = String::from_utf8(MeshSpec::default().build())
        .unwrap()
        .replace("commandline \"exported\"\n", "");
    assert_eq!(Md5Mesh::read(text.as_bytes()).unwrap().commandline, None);
    let text = String::from_utf8(MeshSpec::default().build())
        .unwrap()
        .replace("MD5Version 10", "MD5Version 11");
    assert_eq!(
        Md5Mesh::read(text.as_bytes()),
        Err(Md5Error::Expected { keyword: "10" })
    );
    assert_eq!(
        Md5Error::Expected { keyword: "joints" }.to_string(),
        "Expected \"joints\""
    );
}

// spec MD5-2.3: at least one joint and one mesh.
#[test]
fn needs_bones_and_meshes() {
    let spec = MeshSpec {
        num_joints: "0",
        ..MeshSpec::default()
    };
    assert_eq!(mesh(&spec), Err(Md5Error::NoBones));
    let spec = MeshSpec {
        num_meshes: "0",
        ..MeshSpec::default()
    };
    assert_eq!(mesh(&spec), Err(Md5Error::NoMeshes));
    assert_eq!(Md5Error::NoBones.to_string(), "has no bones");
    assert_eq!(Md5Error::NoMeshes.to_string(), "has no meshes");
}

// spec MD5-2.4: exactly numJoints joints, then `}`.
#[test]
fn joint_count_is_exact() {
    let got = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(got.joints.len(), 1);
    assert_eq!(got.joints[0].name, b"root");
    assert_eq!(got.joints[0].parent, MD5_NO_PARENT);
    assert_eq!(got.joints[0].position, [1.0, 2.0, 3.0]);
    let spec = MeshSpec {
        joints: vec![
            "\"root\" -1 ( 1 2 3 ) ( 0 0 0 )".to_owned(),
            "\"arm\" 0 ( 1 2 3 ) ( 0 0 0 )".to_owned(),
        ],
        ..MeshSpec::default()
    };
    assert_eq!(mesh(&spec), Err(Md5Error::Expected { keyword: "}" }));
    let spec = MeshSpec {
        num_joints: "2",
        ..spec
    };
    let got = mesh(&spec).unwrap();
    assert_eq!(got.joints[1].name, b"arm");
    assert_eq!(got.joints[1].parent, 0);
}

// spec MD5-2.5: a parent is -1 or a joint's index.
#[test]
fn parent_must_be_a_joint() {
    for parent in ["1", "-2", "5"] {
        let spec = MeshSpec {
            joints: vec![format!("\"root\" {parent} ( 1 2 3 ) ( 0 0 0 )")],
            ..MeshSpec::default()
        };
        assert_eq!(mesh(&spec), Err(Md5Error::BoneOutOfBounds), "{parent}");
    }
    // A joint may name itself or a later joint.
    let spec = MeshSpec {
        num_joints: "2",
        joints: vec![
            "\"a\" 1 ( 0 0 0 ) ( 0 0 0 )".to_owned(),
            "\"b\" 1 ( 0 0 0 ) ( 0 0 0 )".to_owned(),
        ],
        ..MeshSpec::default()
    };
    assert!(mesh(&spec).is_ok());
    assert_eq!(
        Md5Error::BoneOutOfBounds.to_string(),
        "bone index out of bounds"
    );
}

// spec MD5-2.6: w is -sqrt(1 - |xyz|^2), clamped at 0.
#[test]
fn quaternion_w() {
    assert_eq!(md5_quat_w([0.0, 0.0, 0.0]).to_bits(), (-1.0_f32).to_bits());
    assert_eq!(md5_quat_w([0.6, 0.0, 0.0]).to_bits(), (-0.8_f32).to_bits());
    assert_eq!(md5_quat_w([1.0, 1.0, 0.0]).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(md5_quat_w([0.0, 0.0, 0.6]).to_bits(), (-0.8_f32).to_bits());
    assert_eq!(md5_quat_w([0.0, 0.6, 0.0]).to_bits(), (-0.8_f32).to_bits());
    let spec = MeshSpec {
        joints: vec!["\"root\" -1 ( 1 2 3 ) ( 0 0.6 0 )".to_owned()],
        ..MeshSpec::default()
    };
    let orientation = mesh(&spec).unwrap().joints[0].orientation;
    assert_eq!(
        orientation.map(f32::to_bits),
        [0.0_f32, 0.6, 0.0, -0.8].map(f32::to_bits)
    );
}

// spec MD5-2.7: exactly numMeshes meshes; tokens after them are ignored.
#[test]
fn mesh_blocks() {
    let got = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(got.meshes.len(), 1);
    let surface = &got.meshes[0];
    assert_eq!(surface.shader, b"ogre");
    assert_eq!(
        (surface.num_verts, surface.num_tris, surface.num_weights),
        (3, 1, 1)
    );
    let spec = MeshSpec {
        num_meshes: "2",
        meshes: vec![mesh_with_tri("tri 0 0 1 2"), mesh_with_tri("tri 0 2 1 0")],
        trailer: "garbage { ( } 1.0x",
        ..MeshSpec::default()
    };
    let got = mesh(&spec).unwrap();
    assert_eq!(got.meshes[1].tri(0), Some([2, 1, 0]));
    let spec = MeshSpec {
        num_meshes: "2",
        ..MeshSpec::default()
    };
    assert_eq!(mesh(&spec), Err(Md5Error::Expected { keyword: "mesh" }));
    let spec = with_mesh(mesh_with_tri("tri 0 0 1 2").replace("shader", "material"));
    assert_eq!(mesh(&spec), Err(Md5Error::Expected { keyword: "shader" }));
}

// spec MD5-2.8: entries need not match the counts; missing ones are zero, repeats keep the
// last.
#[test]
fn sparse_entries() {
    let body = "shader \"s\"\nnumverts 4\nvert 2 ( 1 1 ) 0 1\nvert 2 ( 0.5 0.25 ) 0 0\n\
                numtris 2\ntri 1 3 2 1\nnumweights 3\nweight 1 0 0.5 ( 1 2 3 )\n";
    let got = mesh(&with_mesh(body.to_owned())).unwrap();
    let surface = &got.meshes[0];
    assert_eq!(surface.vert(0), Some(Md5Vert::default()));
    assert_eq!(
        surface.vert(2),
        Some(Md5Vert {
            st: [0.5, 0.25],
            first_weight: 0,
            weight_count: 0
        })
    );
    assert_eq!(surface.vert(4), None);
    assert_eq!(surface.tri(0), Some([0, 0, 0]));
    assert_eq!(surface.tri(1), Some([3, 2, 1]));
    assert_eq!(surface.tri(2), None);
    assert_eq!(surface.weight(0), Some(Md5Weight::default()));
    assert_eq!(
        surface.weight(1),
        Some(Md5Weight {
            joint: 0,
            bias: 0.5,
            position: [1.0, 2.0, 3.0]
        })
    );
    assert_eq!(surface.weight(3), None);
    // Empty lists are fine.
    let body = "shader \"s\" numverts 0 numtris 0 numweights 0 ";
    assert!(mesh(&with_mesh(body.to_owned())).is_ok());
}

// spec MD5-2.9: a vertex index below numverts.
#[test]
fn vertex_index_in_range() {
    let body = mesh_with_tri("tri 0 0 1 2").replace("vert 2", "vert 3");
    assert_eq!(mesh(&with_mesh(body)), Err(Md5Error::VertexOutOfBounds));
    assert_eq!(
        Md5Error::VertexOutOfBounds.to_string(),
        "vertex index out of bounds"
    );
    let got = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(
        got.meshes[0].vert(2),
        Some(Md5Vert {
            st: [0.5, 1.0],
            first_weight: 0,
            weight_count: 1
        })
    );
}

// spec MD5-2.10: triangle and vertex indices in range, numverts itself rejected.
#[test]
fn triangle_indices_in_range() {
    assert_eq!(
        mesh(&with_mesh(mesh_with_tri("tri 0 0 1 3"))),
        Err(Md5Error::VertexOutOfBounds)
    );
    assert_eq!(
        mesh(&with_mesh(mesh_with_tri("tri 1 0 1 2"))),
        Err(Md5Error::TriangleOutOfBounds)
    );
    assert_eq!(
        Md5Error::TriangleOutOfBounds.to_string(),
        "triangle index out of bounds"
    );
    let got = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(got.meshes[0].tri(0), Some([0, 1, 2]));
}

// spec MD5-2.11: weight and joint indices in range.
#[test]
fn weight_indices_in_range() {
    let body = mesh_with_tri("tri 0 0 1 2").replace("weight 0 0 1", "weight 1 0 1");
    assert_eq!(mesh(&with_mesh(body)), Err(Md5Error::WeightOutOfBounds));
    let body = mesh_with_tri("tri 0 0 1 2").replace("weight 0 0 1", "weight 0 1 1");
    assert_eq!(mesh(&with_mesh(body)), Err(Md5Error::BoneOutOfBounds));
    assert_eq!(
        Md5Error::WeightOutOfBounds.to_string(),
        "weight index out of bounds"
    );
}

// spec MD5-2.12: each vertex's weights lie within numweights.
#[test]
fn vertex_weights_in_range() {
    let body = mesh_with_tri("tri 0 0 1 2").replace("vert 1 ( 1 0 ) 0 1", "vert 1 ( 1 0 ) 0 2");
    assert_eq!(mesh(&with_mesh(body)), Err(Md5Error::WeightOutOfBounds));
    let body = mesh_with_tri("tri 0 0 1 2").replace("vert 1 ( 1 0 ) 0 1", "vert 1 ( 1 0 ) 1 0");
    assert!(mesh(&with_mesh(body)).is_ok());
    let body = mesh_with_tri("tri 0 0 1 2").replace(
        "vert 1 ( 1 0 ) 0 1",
        "vert 1 ( 1 0 ) 18446744073709551615 2",
    );
    assert_eq!(mesh(&with_mesh(body)), Err(Md5Error::WeightOutOfBounds));
}

// spec MD5-2.13: skin image paths.
#[test]
fn skin_paths() {
    assert_eq!(MD5_MAX_SKIN_FRAMES, 4);
    assert_eq!(md5_skin_path(b"ogre", 0, 0), b"progs/ogre_00_00");
    assert_eq!(md5_skin_path(b"v_shot", 1, 3), b"progs/v_shot_01_03");
    assert_eq!(md5_skin_path(b"x", 123, 7), b"progs/x_123_07");
}

// spec MD5-3.1: the animation path replaces the extension.
#[test]
fn anim_paths() {
    assert_eq!(md5_anim_path("progs/ogre.md5mesh"), "progs/ogre.md5anim");
    assert_eq!(md5_anim_path("progs/ogre"), "progs/ogre.md5anim");
    assert_eq!(md5_anim_path("a.b/ogre"), "a.b/ogre.md5anim");
    assert_eq!(md5_anim_path("ogre.x.md5mesh"), "ogre.x.md5anim");
}

// spec MD5-3.2: the animation header.
#[test]
fn anim_header() {
    let got = anim(&AnimSpec::default()).unwrap();
    assert_eq!(got.commandline, Some(Vec::new()));
    assert_eq!(got.num_frames, 2);
    assert_eq!(got.frame_rate, b"10");
    assert_eq!(got.num_animated_components(), 6);
    let spec = AnimSpec {
        num_frames: "0",
        ..AnimSpec::default()
    };
    assert_eq!(anim(&spec), Err(Md5Error::NoPoses));
    assert_eq!(Md5Error::NoPoses.to_string(), "has no poses");
    // The frame rate need not be a number.
    let text = String::from_utf8(AnimSpec::default().build())
        .unwrap()
        .replace("frameRate 10", "frameRate fast");
    assert_eq!(Md5Anim::read(text.as_bytes()).unwrap().frame_rate, b"fast");
}

// spec MD5-3.3: exactly numJoints hierarchy entries, flags in the low six bits.
#[test]
fn hierarchy_entries() {
    assert_eq!(MD5_ANIM_FLAGS, 63);
    let got = anim(&AnimSpec::default()).unwrap();
    assert_eq!(got.hierarchy.len(), 1);
    assert_eq!(got.hierarchy[0].name, b"root");
    assert_eq!(
        (
            got.hierarchy[0].parent,
            got.hierarchy[0].flags,
            got.hierarchy[0].first_component
        ),
        (-1, 63, 0)
    );
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 64 0".to_owned()],
        ..AnimSpec::default()
    };
    assert_eq!(anim(&spec), Err(Md5Error::UnsupportedFlags));
    assert_eq!(
        Md5Error::UnsupportedFlags.to_string(),
        "bone has unsupported flags"
    );
    let spec = AnimSpec {
        num_joints: "2",
        ..AnimSpec::default()
    };
    assert!(anim(&spec).is_err());
}

// spec MD5-3.4: a joint's components lie within numAnimatedComponents.
#[test]
fn component_offsets_in_range() {
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 63 1".to_owned()],
        ..AnimSpec::default()
    };
    assert_eq!(anim(&spec), Err(Md5Error::BadOffset));
    assert_eq!(Md5Error::BadOffset.to_string(), "bone has bad offset");
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 5 3".to_owned()],
        ..AnimSpec::default()
    };
    let got = anim(&spec).unwrap();
    // Flags 1 and 4 select x and z from components 3 and 4.
    assert_eq!(got.pose(1, 0).unwrap().position, [0.6, 0.0, 0.0]);
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 0 6".to_owned()],
        ..AnimSpec::default()
    };
    assert!(anim(&spec).is_ok());
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 1 18446744073709551615".to_owned()],
        ..AnimSpec::default()
    };
    assert_eq!(anim(&spec), Err(Md5Error::BadOffset));
}

// spec MD5-3.5: bounds and baseframe pairs are read and their counts not checked.
#[test]
fn bounds_and_baseframe() {
    let got = anim(&AnimSpec::default()).unwrap();
    assert_eq!(
        got.bounds,
        [
            [[-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]],
            [[-2.0, -2.0, -2.0], [2.0, 2.0, 2.0]]
        ]
    );
    assert_eq!(got.baseframe, [[[0.0; 3], [0.0; 3]]]);
    let text = String::from_utf8(AnimSpec::default().build())
        .unwrap()
        .replace("baseframe {\n\t( 0 0 0 ) ( 0 0 0 )\n}", "baseframe { }");
    assert!(Md5Anim::read(text.as_bytes()).unwrap().baseframe.is_empty());
    let text = String::from_utf8(AnimSpec::default().build())
        .unwrap()
        .replace("bounds", "bound");
    assert_eq!(
        Md5Anim::read(text.as_bytes()),
        Err(Md5Error::Expected { keyword: "bounds" })
    );
}

// spec MD5-3.6: frames by index; missing ones are zero, repeats keep the last.
#[test]
fn frames_by_index() {
    let got = anim(&AnimSpec::default()).unwrap();
    assert_eq!(got.frame(0), Some(vec![0.0; 6]));
    assert_eq!(got.frame(1), Some(vec![1.0, 2.0, 3.0, 0.6, 0.0, 0.0]));
    assert_eq!(got.frame(2), None);
    let spec = AnimSpec {
        frames: vec![
            ("0".to_owned(), "1 1 1 0 0 0".to_owned()),
            ("0".to_owned(), "2 2 2 0 0 0".to_owned()),
        ],
        trailer: "trailing ( junk",
        ..AnimSpec::default()
    };
    assert_eq!(
        anim(&spec).unwrap().frame(0),
        Some(vec![2.0, 2.0, 2.0, 0.0, 0.0, 0.0])
    );
    let spec = AnimSpec {
        frames: vec![("2".to_owned(), "1 1 1 0 0 0".to_owned())],
        ..AnimSpec::default()
    };
    assert_eq!(anim(&spec), Err(Md5Error::InvalidPoseIndex));
    assert_eq!(Md5Error::InvalidPoseIndex.to_string(), "invalid pose index");
    for components in ["1 2 3 4 5", "1 2 3 4 5 6 7"] {
        let spec = AnimSpec {
            frames: vec![("0".to_owned(), components.to_owned())],
            ..AnimSpec::default()
        };
        assert!(anim(&spec).is_err(), "{components}");
    }
}

// spec MD5-3.7: poses take unselected components as 0, not from baseframe.
#[test]
fn poses() {
    let got = anim(&AnimSpec::default()).unwrap();
    let pose = got.pose(1, 0).unwrap();
    assert_eq!(pose.position, [1.0, 2.0, 3.0]);
    assert_eq!(
        pose.orientation.map(f32::to_bits),
        [0.6_f32, 0.0, 0.0, -0.8].map(f32::to_bits)
    );
    assert_eq!(
        got.pose(0, 0),
        Some(Md5Pose {
            position: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, -1.0]
        })
    );
    assert_eq!(got.pose(2, 0), None);
    assert_eq!(got.pose(0, 1), None);
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" -1 34 0".to_owned()],
        frames: vec![("1".to_owned(), "5 0.6 0 0 0 0".to_owned())],
        ..AnimSpec::default()
    };
    let text = String::from_utf8(spec.build())
        .unwrap()
        .replace("( 0 0 0 ) ( 0 0 0 )", "( 9 9 9 ) ( 0.5 0.5 0.5 )");
    let got = Md5Anim::read(text.as_bytes()).unwrap();
    let pose = got.pose(1, 0).unwrap();
    // Flags 2 and 32: y from component 0, quaternion z from component 1.
    assert_eq!(pose.position, [0.0, 5.0, 0.0]);
    assert_eq!(pose.orientation[..3], [0.0, 0.0, 0.6]);
}

// spec MD5-3.8: the end of the file after the frame rate fails.
#[test]
fn anim_ends_after_frame_rate() {
    let text = b"MD5Version 10 numFrames 1 numJoints 1 frameRate 24";
    assert_eq!(
        Md5Anim::read(text),
        Err(Md5Error::Expected {
            keyword: "numAnimatedComponents"
        })
    );
    let bytes = AnimSpec::default().build();
    let baseframe = bytes.windows(9).position(|w| w == b"baseframe").unwrap();
    for len in 0..baseframe + 9 {
        assert!(Md5Anim::read(&bytes[..len]).is_err(), "length {len}");
    }
}

// spec MD5-3.9: the animation's hierarchy must match the mesh's joints.
#[test]
fn anim_must_fit_mesh() {
    let mesh = mesh(&MeshSpec::default()).unwrap();
    assert_eq!(
        anim(&AnimSpec::default()).unwrap().check_mesh(&mesh),
        Ok(())
    );
    let spec = AnimSpec {
        hierarchy: vec!["\"Root\" -1 63 0".to_owned()],
        ..AnimSpec::default()
    };
    assert_eq!(
        anim(&spec).unwrap().check_mesh(&mesh),
        Err(Md5Error::BoneRenamed)
    );
    let spec = AnimSpec {
        hierarchy: vec!["\"root\" 0 63 0".to_owned()],
        ..AnimSpec::default()
    };
    assert_eq!(
        anim(&spec).unwrap().check_mesh(&mesh),
        Err(Md5Error::WrongParent)
    );
    let spec = AnimSpec {
        num_joints: "2",
        num_components: "12",
        hierarchy: vec!["\"root\" -1 63 0".to_owned(), "\"arm\" 0 63 6".to_owned()],
        frames: vec![],
        ..AnimSpec::default()
    };
    assert_eq!(
        anim(&spec).unwrap().check_mesh(&mesh),
        Err(Md5Error::IncorrectBoneCount)
    );
    assert_eq!(
        Md5Error::IncorrectBoneCount.to_string(),
        "has incorrect bone count"
    );
    assert_eq!(Md5Error::BoneRenamed.to_string(), "bone was renamed");
    assert_eq!(Md5Error::WrongParent.to_string(), "bone has wrong parent");
}

// spec MD5-3.11: a few bytes with numJoints 0, no frames and numAnimatedComponents near
// u64::MAX are refused, so frame 0, never given, cannot ask for that many zeros.
#[test]
fn a_huge_component_count_is_refused() {
    let text = format!(
        "MD5Version 10 numFrames 1 numJoints 0 frameRate 24 numAnimatedComponents {} \
         hierarchy {{ }} bounds {{ }} baseframe {{ }}",
        u64::MAX - 1
    );
    assert_eq!(
        Md5Anim::read(text.as_bytes()),
        Err(Md5Error::TooManyComponents)
    );
    assert_eq!(
        Md5Error::TooManyComponents.to_string(),
        "has too many animated components"
    );
}

// spec MD5-3.11: the count may be the text's length in bytes, and frame 0 then holds that many
// zeros, but not one more; a NUL ends the text that counts (MD5-1.1).
#[test]
fn the_component_limit_is_the_texts_length() {
    let head = "MD5Version 10 numFrames 1 numJoints 0 frameRate 24 numAnimatedComponents ";
    let tail = " hierarchy { } bounds { } baseframe { }";
    // The count's own digits are part of the text: 3 digits make it head + 3 + tail bytes.
    let len = head.len() + 3 + tail.len();
    let at_limit = format!("{head}{len}{tail}");
    assert_eq!(at_limit.len(), len);
    let read = Md5Anim::read(at_limit.as_bytes()).unwrap();
    assert_eq!(read.frame(0).unwrap(), vec![0.0; len]);
    assert_eq!(read.frame(1), None);
    let over = format!("{head}{}{tail}", len + 1);
    assert_eq!(over.len(), len);
    assert_eq!(
        Md5Anim::read(over.as_bytes()),
        Err(Md5Error::TooManyComponents)
    );
    let mut padded = over.into_bytes();
    padded.extend_from_slice(&[0; 8]);
    assert_eq!(Md5Anim::read(&padded), Err(Md5Error::TooManyComponents));
}

// spec MD5-3.10: each frame is shown for 0.1 seconds.
#[test]
fn frame_interval() {
    assert_eq!(MD5_FRAME_INTERVAL.to_bits(), 0.1_f32.to_bits());
}

proptest! {
    // spec MD5-1.1, MD5-1.9: arbitrary bytes never panic either reader or the tokenizer.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _count = Tokens::new(&bytes).count();
        let _mesh = Md5Mesh::read(&bytes);
        let _anim = Md5Anim::read(&bytes);
    }

    // spec MD5-3.6, MD5-3.11: whatever count an animation states, every frame it accepts holds
    // that many components, at most the file's length, and frames end at numFrames.
    #[test]
    fn frames_hold_a_bounded_count(
        count in prop_oneof![0_u64..64, any::<u64>()],
        frames in 1_u64..4,
        given in proptest::collection::vec(-1.0e6_f32..1.0e6, 0..8),
    ) {
        let mut text = format!(
            "MD5Version 10 numFrames {frames} numJoints 0 frameRate 24 \
             numAnimatedComponents {count} hierarchy {{ }} bounds {{ }} baseframe {{ }}"
        );
        if !given.is_empty() {
            text.push_str(" frame 0 {");
            for value in &given {
                text.push_str(&format!(" {value}"));
            }
            text.push_str(" }");
        }
        let len = u64::try_from(text.len()).unwrap();
        if let Ok(read) = Md5Anim::read(text.as_bytes()) {
            prop_assert!(read.num_animated_components() <= len);
            for index in 0..read.num_frames {
                let components = read.frame(index).unwrap();
                prop_assert_eq!(
                    u64::try_from(components.len()).unwrap(),
                    read.num_animated_components()
                );
            }
            prop_assert!(read.frame(read.num_frames).is_none());
        } else {
            let listed = u64::try_from(given.len()).unwrap();
            prop_assert!(count > len || (listed > 0 && count != listed));
        }
    }

    // spec MD5-1.2, MD5-1.3, MD5-1.5: without quotes, every token is non-empty and holds no
    // whitespace.
    #[test]
    fn tokens_hold_no_whitespace(text in "[a-z0-9 (){}':/*\n\t.-]{0,64}") {
        for token in Tokens::new(text.as_bytes()) {
            prop_assert!(!token.is_empty());
            prop_assert!(!token.iter().any(u8::is_ascii_whitespace));
        }
    }

    // spec MD5-2.7, MD5-2.8, MD5-2.12: a valid header followed by arbitrary tokens never panics,
    // and every accessor stays within its count.
    #[test]
    fn arbitrary_meshes_never_panic(tail in "[a-z0-9 (){}\"\n.-]{0,200}") {
        let mut bytes = b"MD5Version 10 numJoints 1 numMeshes 1 joints { \"r\" -1 ( 0 0 0 ) ( 0 0 0 ) } mesh { ".to_vec();
        bytes.extend_from_slice(tail.as_bytes());
        if let Ok(mesh) = Md5Mesh::read(&bytes) {
            for surface in &mesh.meshes {
                prop_assert!(surface.vert(surface.num_verts).is_none());
                prop_assert!(surface.tri(surface.num_tris).is_none());
                prop_assert!(surface.weight(surface.num_weights).is_none());
            }
        }
    }
}
