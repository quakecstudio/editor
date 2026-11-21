// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::super::recorded::agrees;
use super::*;

// spec MDL-10.1, MDL-10.4: a text fbx is the mesh FTEQW's converter makes of it, for a
// plain model and for one whose arm has a pre-rotation and whose mesh model stands moved,
// turned and scaled
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
        let made = read(text, Up::Y).unwrap();
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

// spec MDL-10.4: with y up a position (x, y, z) is (x, -z, y), the converter's quarter turn
// about x; a file whose z is up is left as it is
#[test]
fn an_fbx_is_turned_a_quarter_about_x() {
    let text = include_str!("../fixtures/arm.fbx.txt");
    let turned = read(text, Up::Y).unwrap();
    let as_is = read(text, Up::Z).unwrap();
    // The fifth corner the file's polygons name is the vertex (0, 0, 4).
    let corner = as_is.frames[0]
        .positions
        .iter()
        .position(|at| at.map(f32::to_bits) == [0.0_f32, 0.0, 4.0].map(f32::to_bits))
        .unwrap();
    assert_eq!(turned.frames[0].positions[corner], [0.0, -4.0, 0.0]);
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
    assert!(FbxError::Version.to_string().contains("export it as ASCII"));
    let empty = read("; FBX 7.3.0 project file\nObjects:  {\n}\n", Up::Y).unwrap();
    assert!(empty.triangles.is_empty());
}
