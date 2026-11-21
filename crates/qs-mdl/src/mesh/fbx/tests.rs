// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or sum is a failed test"
)]

use super::super::Mesh;
use super::super::recorded::agrees;
use super::*;

/// The mesh turned as FTEQW's converter turns what it reads, a quarter about x.
fn as_the_converter_turns(mut mesh: Mesh) -> Mesh {
    for position in mesh
        .frames
        .iter_mut()
        .flat_map(|frame| &mut frame.positions)
    {
        let [x, y, z] = *position;
        *position = [x, -z, y];
    }
    mesh
}

// spec MDL-10.1, MDL-10.4: a text fbx, turned as the converter turns one, is the mesh
// FTEQW's converter makes of it, for a plain model and for one whose arm has a pre-rotation
// and whose mesh model stands moved, turned and scaled
#[test]
fn an_fbx_model_is_the_converters() {
    for (text, recorded) in [
        (
            include_str!("../fixtures/arm.fbx.txt"),
            include_str!("../fixtures/fbx.fteqw.txt"),
        ),
        (
            include_str!("../fixtures/hard-arm.fbx.txt"),
            include_str!("../fixtures/hard-fbx.fteqw.txt"),
        ),
    ] {
        let made = read(text, Up::Z).unwrap();
        let names: Vec<&str> = made
            .frames
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        assert_eq!(names, ["wave1", "wave2", "wave3"]);
        assert!(made.has_texcoords());
        agrees(&as_the_converter_turns(made), recorded);
    }
}

// spec MDL-10.5: a file whose y is up is turned as an .obj is, so that a model facing -z
// faces +x: a position (x, y, z) is (-z, -x, y), where the converter's is (x, -z, y); a file
// whose z is up is left as it is
#[test]
fn a_y_up_fbx_faces_as_an_obj_does() {
    let text = include_str!("../fixtures/arm.fbx.txt");
    let turned = read(text, Up::Y).unwrap();
    let as_is = read(text, Up::Z).unwrap();
    for (ours, raw) in turned.frames.iter().zip(&as_is.frames) {
        for (at, was) in ours.positions.iter().zip(&raw.positions) {
            assert_eq!(at.map(f32::to_bits), Up::Y.turned(*was).map(f32::to_bits));
        }
    }
    // The vertex at (0, 0, 4): 4 behind a model facing -z, so 4 behind one facing +x.
    let corner = as_is.frames[0]
        .positions
        .iter()
        .position(|at| at.map(f32::to_bits) == [0.0_f32, 0.0, 4.0].map(f32::to_bits))
        .unwrap();
    assert_eq!(turned.frames[0].positions[corner], [-4.0, -0.0, 0.0]);
}

/// A file of one geometry with the polygons `polygons` over a square in the plane z = 0.
fn square(polygons: &str) -> String {
    format!(
        "; FBX 7.4.0 project file\nObjects:  {{\n\tGeometry: 1, \"Geometry::s\", \"Mesh\" {{\n\
         \t\tVertices: *12 {{\n\t\t\ta: 0,0,0,1,0,0,1,1,0,0,1,0\n\t\t}}\n\
         \t\tPolygonVertexIndex: *4 {{\n\t\t\ta: {polygons}\n\t\t}}\n\t}}\n}}\n"
    )
}

// spec MDL-10.5: a polygon of more than three corners is a fan from its first, each
// triangle turned around as a triangle is; a file with no polygons has its vertices three
// to a triangle; with no animation the mesh is one pose, base
#[test]
fn a_polygon_is_a_fan() {
    let faces = |text: &str| -> Vec<[[f32; 3]; 3]> {
        let mesh = read(text, Up::Z).unwrap();
        assert_eq!(mesh.frames.len(), 1);
        assert_eq!(mesh.frames[0].name, FBX_FRAME);
        mesh.triangles
            .iter()
            .map(|corners| corners.map(|corner| mesh.frames[0].positions[corner.position]))
            .collect()
    };
    let (a, b, c, d) = (
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    );
    // A triangle a b c is kept as b a c.
    assert_eq!(faces(&square("0,1,-3")), [[b, a, c]]);
    // The square a b c d is the fan a b c and a c d, each turned the same way.
    assert_eq!(faces(&square("0,1,2,-4")), [[b, a, c], [c, a, d]]);
    // No polygons: the vertices in threes, the fourth left.
    let bare = square("0,1,2,-4").replace("PolygonVertexIndex", "Unread");
    assert_eq!(faces(&bare), [[b, a, c]]);
}

// spec MDL-10.4: commas are passed over, a semicolon starts a comment, a quoted name is one
// token, an array's count is one, and a property a reader does not know is passed over with
// the block it opens
#[test]
fn tokens_are_read_as_the_converter_reads_them() {
    let mut tokens = Tokens {
        rest: "Name: \"a b\", 1.5e1,-2 ; a comment {\n*3 {\n\ta: 1,2\n}\nNext: 7\n",
    };
    let mut seen = Vec::new();
    while let Some(token) = tokens.token() {
        seen.push(token);
    }
    assert_eq!(
        seen,
        [
            Token::Prop("Name"),
            Token::Text("a b"),
            Token::Number(15.0),
            Token::Number(-2.0),
            Token::Line,
            Token::Array,
            Token::Begin,
            Token::Line,
            Token::Prop("a"),
            Token::Number(1.0),
            Token::Number(2.0),
            Token::Line,
            Token::End,
            Token::Line,
            Token::Prop("Next"),
            Token::Number(7.0),
            Token::Line,
        ]
    );
    // A property's block is passed over whole, with the blocks inside it.
    let mut tokens = Tokens {
        rest: " 1 {\n\tInner: 2 {\n\t\tDeep: 3\n\t}\n\tMore: 4\n}\nAfter: 5\n",
    };
    assert!(tokens.skip_prop());
    assert_eq!(tokens.token(), Some(Token::Line));
    assert_eq!(tokens.token(), Some(Token::Prop("After")));
    // A sign with no number after it is 0, and one byte.
    let mut tokens = Tokens { rest: "- 3" };
    assert_eq!(tokens.token(), Some(Token::Number(0.0)));
    assert_eq!(tokens.token(), Some(Token::Number(3.0)));
}

// spec MDL-10.4, MDL-10.5: a file whose first line does not say FBX 7, as a binary file's
// does not, is refused; a file with no geometry is a mesh with no triangle
#[test]
fn what_is_no_text_fbx_is_refused() {
    for text in [
        "Kaydara FBX Binary  \0\x1a\0",
        "; FBX 6.1.0 project file\n",
        "",
    ] {
        assert_eq!(
            read(text, Up::Y).unwrap_err(),
            FbxError::Version,
            "{text:?}"
        );
    }
    assert_eq!(
        FbxError::Version.to_string(),
        "the file is not an FBX of version 7"
    );
    let empty = read("; FBX 7.3.0 project file\nObjects:  {\n}\n", Up::Y).unwrap();
    assert!(empty.triangles.is_empty());
}

// spec MDL-10.4: a number is a sign, digits with a point among them and an exponent when
// digits follow its e; a long run of signs is as many zeros, each one byte
#[test]
fn a_number_is_read_as_strtod_reads_one() {
    let read = |text: &'static str| {
        let mut tokens = Tokens { rest: text };
        let first = tokens.token();
        (first, tokens.rest)
    };
    assert_eq!(read("1.5e3x"), (Some(Token::Number(1500.0)), "x"));
    assert_eq!(read("-.5,"), (Some(Token::Number(-0.5)), ","));
    assert_eq!(read("2e"), (Some(Token::Number(2.0)), "e"));
    assert_eq!(read("3e+"), (Some(Token::Number(3.0)), "e+"));
    assert_eq!(read("4.e-2 "), (Some(Token::Number(0.04)), " "));
    assert_eq!(read("+-1"), (Some(Token::Number(0.0)), "-1"));
    assert_eq!(read(".x"), (Some(Token::Number(0.0)), "x"));
    // Two hundred thousand signs are read in one pass each.
    let signs = "-".repeat(200_000);
    let mut tokens = Tokens { rest: &signs };
    let mut count = 0_u32;
    while tokens.token().is_some() {
        count += 1;
    }
    assert_eq!(count, 200_000);
}

// spec MDL-10.1, MDL-10.4: a limb written before its parent follows it all the same
#[test]
fn a_limb_before_its_parent_follows_it() {
    let text = include_str!("../fixtures/arm.fbx.txt");
    // The arm's limb, 301, written before the root's, 300.
    let arm = text.find("\tModel: 301").unwrap();
    let root = text.find("\tModel: 300").unwrap();
    let skin = text.find("\tDeformer: 400").unwrap();
    let swapped = format!(
        "{}{}{}{}",
        &text[..root],
        &text[arm..skin],
        &text[root..arm],
        &text[skin..]
    );
    let as_written = read(text, Up::Z).unwrap();
    let made = read(&swapped, Up::Z).unwrap();
    assert_eq!(made.frames.len(), 3);
    // The limbs' numbers change places and nothing else: every pose is the same.
    for (ours, theirs) in made.frames.iter().zip(&as_written.frames) {
        for (at, want) in ours.positions.iter().zip(&theirs.positions) {
            assert!(
                at.iter()
                    .zip(want)
                    .all(|(at, want)| (at - want).abs() < 1e-4),
                "{}",
                ours.name
            );
        }
    }
}

proptest::proptest! {
    // spec MDL-10.4, MDL-10.5: whatever follows the first line, a mesh is made or the file
    // is refused
    #[test]
    fn any_fbx_is_read_or_refused(
        tail in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..300),
        swap in 0_usize..2600,
    ) {
        let mut bytes = include_bytes!("../fixtures/arm.fbx.txt").to_vec();
        let at = swap.clamp(26, bytes.len());
        bytes.splice(at..(at + tail.len()).min(bytes.len()), tail);
        let _made = read(&String::from_utf8_lossy(&bytes), Up::Y);
    }
}

/// The bytes a fixture of hexadecimal lines holds.
fn unhex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// spec MDL-11.1, MDL-11.2, MDL-11.3, MDL-11.4: a binary fbx is the mesh the text file of the
// same model is: one of version 7.4, whose counts are 32 bits, one of 7.5, whose counts are
// 64 bits and whose objects' numbers are past what a double holds, and the harder model;
// its arrays are deflated, and what follows its last record is passed over
#[test]
fn a_binary_fbx_is_the_text_files_mesh() {
    for (binary, text) in [
        (
            include_str!("../fixtures/arm-7400.fbx.hex"),
            include_str!("../fixtures/arm.fbx.txt"),
        ),
        (
            include_str!("../fixtures/arm-7500.fbx.hex"),
            include_str!("../fixtures/arm.fbx.txt"),
        ),
        (
            include_str!("../fixtures/hard-arm-7400.fbx.hex"),
            include_str!("../fixtures/hard-arm.fbx.txt"),
        ),
    ] {
        let bytes = unhex(binary);
        assert!(bytes.starts_with(b"Kaydara FBX Binary  \0"));
        let made = read_bytes(&bytes, Up::Y).unwrap();
        assert_eq!(made, read(text, Up::Y).unwrap());
        assert_eq!(made.frames.len(), 3);
    }
    // A text file's bytes are read as text.
    let text = include_str!("../fixtures/arm.fbx.txt");
    assert_eq!(
        read_bytes(text.as_bytes(), Up::Z).unwrap(),
        read(text, Up::Z).unwrap()
    );
}

// spec MDL-11.2, MDL-11.4: an object's name and class, written name first with a NUL and a 1
// between, are the text form's `Class::Name`; a number too great for a double is written as
// one of its own; a record's name keeps its letters, digits and underscores
#[test]
fn a_binary_file_is_written_as_text() {
    let text = binary::text(&unhex(include_str!("../fixtures/arm-7500.fbx.hex"))).unwrap();
    assert!(text.starts_with("; FBX 7 binary file\n"), "{text}");
    assert!(text.contains("\"AnimStack::wave\""), "{text}");
    assert!(
        text.contains("Model: -1.5,\"Model::arm\",\"Mesh\" {"),
        "{text}"
    );
    assert!(
        text.contains("PolygonVertexIndex: *15 {\na: 1,0,-3,"),
        "{text}"
    );
    assert!(
        text.contains("C: \"OP\",-9.5,-3.5,\"Lcl Rotation\"\n"),
        "{text}"
    );
    let narrow = binary::text(&unhex(include_str!("../fixtures/arm-7400.fbx.hex"))).unwrap();
    assert!(
        narrow.contains("Model: 200,\"Model::arm\",\"Mesh\" {"),
        "{narrow}"
    );
}

// spec MDL-11.1, MDL-11.5: a binary file of another version than 7 is refused as that; one
// cut short anywhere, one whose array does not inflate to its length, and one with a
// property of no kind are refused as damaged
#[test]
fn a_damaged_binary_file_is_refused() {
    let whole = unhex(include_str!("../fixtures/arm-7400.fbx.hex"));
    let mut old = whole.clone();
    old[23..27].copy_from_slice(&6100_u32.to_le_bytes());
    assert_eq!(read_bytes(&old, Up::Y).unwrap_err(), FbxError::Version);
    assert_eq!(
        read_bytes(&whole[..26], Up::Y).unwrap_err(),
        FbxError::Version
    );
    // The file ends in a record of zeros and a footer: cut before them, it is cut short.
    let records = whole.len() - 13 - 47;
    for cut in [28, 40, 100, records.midpoint(27), records - 1] {
        assert_eq!(
            read_bytes(&whole[..cut], Up::Y).unwrap_err(),
            FbxError::Binary,
            "{cut}"
        );
    }
    // The first array is the vertices': a deflated run of doubles. Spoil its stream.
    let array = whole.windows(1).position(|at| at == b"d").unwrap();
    let vertices = whole
        .windows(9)
        .position(|at| at.starts_with(b"Vertices") && at[8] == b'd')
        .unwrap()
        + 8;
    assert!(array <= vertices);
    let mut spoiled = whole.clone();
    for byte in &mut spoiled[vertices + 13..vertices + 21] {
        *byte ^= 0xff;
    }
    assert_eq!(read_bytes(&spoiled, Up::Y).unwrap_err(), FbxError::Binary);
    // An array that says it is longer than its contents inflate to.
    let mut long = whole.clone();
    long[vertices + 1..vertices + 5].copy_from_slice(&16_u32.to_le_bytes());
    assert_eq!(read_bytes(&long, Up::Y).unwrap_err(), FbxError::Binary);
    // And one that says more than a file's arrays may hold.
    long[vertices + 1..vertices + 5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read_bytes(&long, Up::Y).unwrap_err(), FbxError::Binary);
    // A property of a kind there is none of.
    let mut odd = whole;
    odd[vertices] = b'?';
    assert_eq!(read_bytes(&odd, Up::Y).unwrap_err(), FbxError::Binary);
    assert_eq!(
        FbxError::Binary.to_string(),
        "the binary file is cut short or damaged"
    );
}

proptest::proptest! {
    // spec MDL-11.5: whatever bytes follow a binary file's start, a mesh is made or the
    // file is refused
    #[test]
    fn any_binary_fbx_is_read_or_refused(
        tail in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..200),
        swap in 0_usize..6000,
        wide in proptest::prelude::any::<bool>(),
    ) {
        let fixture = if wide {
            include_str!("../fixtures/arm-7500.fbx.hex")
        } else {
            include_str!("../fixtures/arm-7400.fbx.hex")
        };
        let mut bytes = unhex(fixture);
        let at = swap.clamp(27, bytes.len());
        bytes.splice(at..(at + tail.len()).min(bytes.len()), tail);
        let _made = read_bytes(&bytes, Up::Y);
    }
}

// spec MDL-10.5: the corners that are one vertex of a geometry with one texture coordinate
// are one vertex of the mesh: fifteen corners of five vertices are five
#[test]
fn corners_of_one_vertex_are_one() {
    let mesh = read(include_str!("../fixtures/arm.fbx.txt"), Up::Z).unwrap();
    assert_eq!(mesh.triangles.len(), 5);
    assert_eq!(mesh.frames[0].positions.len(), 5);
    assert_eq!(mesh.texcoords.len(), 5);
    // A vertex with two texture coordinates is two.
    let split = include_str!("../fixtures/arm.fbx.txt").replacen(
        "a: 0.1,0.8,0.9,0.8,0.1,0.19999999999999996,0.9,0.19999999999999996,0.5,0.5",
        "a: 0.1,0.8,0.9,0.8,0.1,0.2,0.9,0.2,0.5,0.5,0.3,0.3",
        1,
    );
    assert_ne!(split, include_str!("../fixtures/arm.fbx.txt"));
    // The first corner of the second polygon, vertex 2, is given the sixth coordinate.
    let split = split.replacen("a: 1,0,2,2,0,3,", "a: 1,0,2,5,0,3,", 1);
    let mesh = read(&split, Up::Z).unwrap();
    assert_eq!(mesh.frames[0].positions.len(), 6);
}

// spec MDL-10.5: an animation whose curves are not of one length, which the converter makes
// none of, is sampled ten times a second by its keys' times, each curve read in a straight
// line between its keys: a turn of 0 to 90 degrees in two keys a second apart, with a lift
// in three, is eleven frames, the sixth half way
#[test]
fn uneven_curves_are_sampled_by_time() {
    let text = include_str!("../fixtures/arm.fbx.txt");
    let uneven = text
        .replacen(
            "KeyTime: *3 {\n\t\t\ta: 0,1,2\n\t\t}\n\t\tKeyValueFloat: *3 {\n\t\t\ta: 0,45,90",
            "KeyTime: *2 {\n\t\t\ta: 0,46186158000\n\t\t}\n\t\tKeyValueFloat: *2 {\n\t\t\ta: 0,90",
            1,
        )
        .replacen(
            "KeyTime: *3 {\n\t\t\ta: 0,1,2\n\t\t}\n\t\tKeyValueFloat: *3 {\n\t\t\ta: 0,1,2",
            "KeyTime: *3 {\n\t\t\ta: 0,23093079000,46186158000\n\t\t}\n\t\tKeyValueFloat: *3 {\n\t\t\ta: 0,1,2",
            1,
        );
    assert_ne!(uneven, text);
    let mesh = read(&uneven, Up::Z).unwrap();
    assert_eq!(mesh.frames.len(), 11);
    assert_eq!(mesh.frames[10].name, "wave11");
    // The arm's own vertex, at (16, 0, 0): 8 along the arm from its joint at (8, 0, 0).
    let tip = mesh.frames[0]
        .positions
        .iter()
        .position(|at| (at[0] - 16.0).abs() < 1e-4)
        .unwrap();
    let near = |at: [f32; 3], want: [f32; 3]| {
        at.iter()
            .zip(want)
            .all(|(at, want)| (at - want).abs() < 1e-3)
    };
    let half = 8.0 * core::f32::consts::FRAC_1_SQRT_2;
    assert!(
        near(mesh.frames[5].positions[tip], [8.0 + half, half, 1.0]),
        "{:?}",
        mesh.frames[5].positions[tip]
    );
    assert!(
        near(mesh.frames[10].positions[tip], [8.0, 8.0, 2.0]),
        "{:?}",
        mesh.frames[10].positions[tip]
    );
    // Curves of one length are a frame for each key, whatever their times.
    assert_eq!(read(text, Up::Z).unwrap().frames.len(), 3);
    // A curve with no time for each of its values is no animation: the mesh is one pose.
    let untimed = uneven.replacen(
        "KeyTime: *2 {\n\t\t\ta: 0,46186158000",
        "KeyTime: *1 {\n\t\t\ta: 0",
        1,
    );
    assert_eq!(read(&untimed, Up::Z).unwrap().frames.len(), 1);
}
