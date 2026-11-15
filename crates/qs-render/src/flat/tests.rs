// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a missing index is a failed test"
)]

use super::{FlatFace, Shade};
use crate::look::Look;
use crate::mesh::{Image, MapMesh, Surface};

/// A 2 by 2 image of one colour.
fn image(value: u8) -> Image {
    Image {
        width: 2,
        height: 2,
        rgba: [value, value, value, 255].repeat(4),
    }
}

/// A square face at height `z` with texture `texture` and shade `shade`.
fn square(z: f32, texture: usize, shade: Shade) -> FlatFace {
    FlatFace {
        corners: vec![
            ([0.0, 0.0, z], [0.0, 0.0]),
            ([64.0, 0.0, z], [1.0, 0.0]),
            ([64.0, 64.0, z], [1.0, 1.0]),
            ([0.0, 64.0, z], [0.0, 1.0]),
        ],
        texture,
        shade,
    }
}

// No spec statement: P7-13: faces become one batch per texture of fanned triangles, with the
// mesh's bounds; a face with too few corners or no texture is not drawn
#[test]
fn faces_become_batches_of_triangles_by_texture() {
    let faces = [
        square(0.0, 1, Shade::FULL),
        square(32.0, 0, Shade::new(128)),
        square(64.0, 1, Shade::new(128)),
        FlatFace {
            corners: vec![([0.0; 3], [0.0; 2]); 2],
            texture: 0,
            shade: Shade::FULL,
        },
        square(96.0, 7, Shade::FULL),
    ];
    let mesh = MapMesh::flat(&faces, vec![image(10), image(20)], Look::QuakeSpasm);
    assert_eq!(mesh.batches.len(), 2);
    assert_eq!(mesh.batches[0].texture, 0);
    assert_eq!(mesh.batches[0].count, 6);
    assert_eq!(mesh.batches[1].texture, 1);
    assert_eq!(mesh.batches[1].count, 12);
    assert!(
        mesh.batches
            .iter()
            .all(|b| b.surface == Surface::Lit && b.model == 0)
    );
    assert_eq!(mesh.indices.len(), 18);
    assert_eq!(mesh.vertices.len(), 12);
    assert_eq!(mesh.mins, [0.0, 0.0, 0.0]);
    assert_eq!(mesh.maxs, [64.0, 64.0, 64.0]);
    assert_eq!(mesh.textures.len(), 2);
    assert_eq!(mesh.glows.len(), 2);
}

// No spec statement: P7-13: the lightmap holds one texel per shade, each the shade over the
// look's light scale, and each face's corners read its own shade's texel
#[test]
fn each_shade_is_one_lightmap_texel() {
    let faces = [square(0.0, 0, Shade::FULL), square(8.0, 0, Shade::new(128))];
    let spasm = MapMesh::flat(&faces, vec![image(1)], Look::QuakeSpasm);
    assert_eq!((spasm.lightmap.width, spasm.lightmap.height), (2, 1));
    // QuakeSpasm doubles a texel (REND-13.5): 128 / 2 and 255 / 2, rounded.
    assert_eq!(spasm.lightmap.rgba, [64, 64, 64, 255, 128, 128, 128, 255]);
    let full = spasm.vertices[0].light;
    let half = spasm.vertices[4].light;
    assert!(
        (full[0] - 0.75).abs() < 1e-6 && (half[0] - 0.25).abs() < 1e-6,
        "{full:?} {half:?}"
    );
    let glquake = MapMesh::flat(&faces, vec![image(1)], Look::GlQuake);
    assert_eq!(
        glquake.lightmap.rgba,
        [128, 128, 128, 255, 255, 255, 255, 255]
    );
    let empty = MapMesh::flat(&[], Vec::new(), Look::QuakeSpasm);
    assert_eq!((empty.lightmap.width, empty.lightmap.rgba.len()), (1, 4));
    assert!(empty.batches.is_empty());
    assert_eq!(Shade::new(7).get(), 7);
}
