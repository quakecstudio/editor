// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::*;
use crate::mdl::write::ModelFrameEntry;
use crate::mdl::{MDL_ALIAS_ONSEAM, Mdl};

fn corner(position: usize, texcoord: usize) -> Corner {
    Corner {
        position,
        texcoord: Some(texcoord),
    }
}

/// A tetrahedron standing on the ground, its corners going clockwise as seen from outside,
/// with a second frame whose top is higher; with texture coordinates when `textured`.
fn tetra(textured: bool) -> Mesh {
    let foot = [[8.0, 0.0, 0.0], [-8.0, 8.0, 0.0], [-8.0, -8.0, 0.0]];
    let pose = |name: &str, top: f32| MeshFrame {
        name: name.to_owned(),
        positions: foot.into_iter().chain([[0.0, 0.0, top]]).collect(),
    };
    let faces = [[0, 3, 1], [1, 3, 2], [2, 3, 0], [0, 1, 2]];
    Mesh {
        triangles: faces
            .iter()
            .enumerate()
            .map(|(face, corners)| {
                corners.map(|position| Corner {
                    position,
                    // Each face has its own corner of the skin for the top.
                    texcoord: textured.then_some(if position == 3 {
                        face.saturating_add(4)
                    } else {
                        position
                    }),
                })
            })
            .collect(),
        texcoords: if textured {
            vec![
                [0.0, 0.0],
                [0.5, 0.0],
                [1.0, 1.0],
                [9.0, 9.0],
                [0.25, 0.5],
                [0.5, 0.5],
                [0.75, 0.5],
                [-1.0, 0.999],
            ]
        } else {
            Vec::new()
        },
        frames: vec![pose("stand1", 16.0), pose("stand2", 24.0)],
    }
}

fn names(model: &Model) -> Vec<String> {
    model
        .frames
        .iter()
        .map(|entry| {
            let ModelFrameEntry::Single(frame) = entry else {
                panic!("a single frame");
            };
            String::from_utf8_lossy(frame.name()).into_owned()
        })
        .collect()
}

// spec MDL-9.3: a mesh with texture coordinates has a vertex for each pair of a position
// and a texture coordinate, at the pixel its coordinate is at on a skin of the size given,
// kept on the skin; its triangles are all on the front, and its skin is the picture given
// or one colour
#[test]
fn a_textured_mesh_keeps_its_own_coordinates() {
    let mesh = tetra(true);
    assert!(mesh.has_texcoords());
    let model = mesh
        .model(&MeshSkin::Flat {
            colour: 9,
            size: [64, 32],
        })
        .unwrap();
    // Three vertices of the foot, and the top once for each of its three faces.
    assert_eq!(model.stverts.len(), 6);
    let st: Vec<(i32, i32, i32)> = model
        .stverts
        .iter()
        .map(|vert| (vert.onseam, vert.s, vert.t))
        .collect();
    assert_eq!(
        st,
        [
            (0, 0, 0),
            (0, 16, 16),
            (0, 32, 0),
            (0, 32, 16),
            (0, 63, 31),
            (0, 48, 16)
        ]
    );
    assert_eq!(model.triangles.len(), 4);
    assert!(
        model
            .triangles
            .iter()
            .all(|triangle| triangle.facesfront == MDL_DT_FACES_FRONT)
    );
    assert_eq!(model.triangles[1].vertindex, [2, 3, 4]);
    assert_eq!(model.triangles[3].vertindex, [0, 2, 4]);
    assert_eq!(model.skins, [ModelSkin::Single(vec![9; 64 * 32])]);
    assert_eq!(names(&model), ["stand1", "stand2"]);
    // The file loads, and its top is where the second pose has it.
    let bytes = model.to_bytes().unwrap();
    let mdl = Mdl::read(&bytes).unwrap();
    let ModelFrameEntry::Single(second) = &model.frames[1] else {
        panic!("a single frame");
    };
    let [x, y, z] = mdl.position(second.verts[1]);
    assert!(x.abs() < 0.1 && y.abs() < 0.1 && (z - 24.0).abs() < 0.1);

    let pixels: Vec<u8> = (0..64 * 32_u32).map(|at| at.to_le_bytes()[0]).collect();
    let pictured = mesh
        .model(&MeshSkin::Picture {
            size: [64, 32],
            pixels: pixels.clone(),
        })
        .unwrap();
    assert_eq!(pictured.skins, [ModelSkin::Single(pixels)]);
    // A picture that is not the size it says is refused by the writer.
    let short = mesh.model(&MeshSkin::Picture {
        size: [64, 32],
        pixels: vec![0; 5],
    });
    assert!(matches!(short, Err(MeshError::Write(_))), "{short:?}");
}

// spec MDL-9.4: a mesh without texture coordinates is laid out as modelgen lays a base, from
// its first pose: what faces +x on the skin's left half and what faces -x on its right, a
// vertex of both on the seam; its skin is one colour of the size the layout needs, and a
// picture is refused
#[test]
fn an_untextured_mesh_is_laid_out_front_and_back() {
    let mesh = tetra(false);
    assert!(!mesh.has_texcoords());
    let model = mesh
        .model(&MeshSkin::Flat {
            colour: 3,
            size: [1, 1],
        })
        .unwrap();
    // 16 wide in y and 16 high: 8 pixels a unit, 132 a half.
    assert_eq!(
        (model.header.skinwidth, model.header.skinheight),
        (264, 132)
    );
    assert_eq!(model.skins, [ModelSkin::Single(vec![3; 264 * 132])]);
    assert_eq!(model.stverts.len(), 4);
    assert_eq!(names(&model), ["stand1", "stand2"]);
    // The face away from +x, the one opposite the foot's forward corner, is on the back.
    let facing: Vec<i32> = model
        .triangles
        .iter()
        .map(|triangle| triangle.facesfront)
        .collect();
    assert_eq!(facing, [1, 0, 1, 1]);
    // The forward corner is only on front faces; the rest are on the seam.
    let seams: Vec<i32> = model.stverts.iter().map(|vert| vert.onseam).collect();
    assert_eq!(
        seams,
        [0, MDL_ALIAS_ONSEAM, MDL_ALIAS_ONSEAM, MDL_ALIAS_ONSEAM]
    );
    // The mesh's left, +y, is to the right on the front's picture.
    assert!(model.stverts[2].s > model.stverts[3].s);
    // The model's vertices are the mesh's positions, not turned.
    let bytes = model.to_bytes().unwrap();
    let mdl = Mdl::read(&bytes).unwrap();
    let ModelFrameEntry::Single(first) = &model.frames[0] else {
        panic!("a single frame");
    };
    let [x, y, z] = mdl.position(first.verts[0]);
    assert!((x - 8.0).abs() < 0.1 && y.abs() < 0.1 && z.abs() < 0.1);

    assert_eq!(
        mesh.model(&MeshSkin::Picture {
            size: [264, 132],
            pixels: vec![0; 264 * 132]
        })
        .unwrap_err(),
        MeshError::NoTexcoords
    );
}

// spec MDL-9.3, MDL-9.4: a triangle two of whose corners are one position is left out; a
// mesh with no triangle left, or no frame, is refused, and so is a corner past the mesh's
// positions or texture coordinates
#[test]
fn what_is_no_model_is_refused() {
    let flat = MeshSkin::Flat {
        colour: 0,
        size: [8, 8],
    };
    let mut mesh = tetra(true);
    mesh.triangles
        .push([corner(0, 0), corner(0, 1), corner(2, 2)]);
    assert_eq!(mesh.model(&flat).unwrap().triangles.len(), 4);

    let mut none = tetra(true);
    none.triangles.clear();
    assert_eq!(none.model(&flat).unwrap_err(), MeshError::Empty);
    let mut still = tetra(true);
    still.frames.clear();
    assert_eq!(still.model(&flat).unwrap_err(), MeshError::Empty);

    let mut far = tetra(true);
    far.triangles[1][0].position = 4;
    assert_eq!(
        far.model(&flat).unwrap_err(),
        MeshError::Corner { triangle: 1 }
    );
    let mut unmapped = tetra(true);
    unmapped.triangles[2][1].texcoord = Some(8);
    assert_eq!(
        unmapped.model(&flat).unwrap_err(),
        MeshError::Corner { triangle: 2 }
    );
    let mut short = tetra(false);
    short.frames[1].positions.pop();
    assert!(matches!(
        short.model(&flat).unwrap_err(),
        MeshError::Corner { .. }
    ));
    assert!(
        MeshError::NoTexcoords
            .to_string()
            .contains("export the skin, paint it and set it")
    );
}
