// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test, and offsets are small"
)]

use super::{
    BspFile, BspWriteError, DiskClipNode, DiskLeaf, DiskModel, DiskNode, DiskPlane, offset_field,
};
use crate::{Bsp, BspLoader};

/// A world of one plane splitting space into the shared solid leaf and one empty leaf.
fn minimal() -> BspFile {
    let leaf = |contents| DiskLeaf {
        contents,
        visofs: -1,
        mins: [-8, -8, -8],
        maxs: [8, 8, 8],
        firstmarksurface: 0,
        nummarksurfaces: 0,
        ambient_level: [0; 4],
    };
    BspFile {
        entities: b"{\n\"classname\" \"worldspawn\"\n}\n\0".to_vec(),
        planes: vec![DiskPlane {
            normal: [1.0, 0.0, 0.0],
            dist: 0.0,
            plane_type: 0,
        }],
        nodes: vec![DiskNode {
            planenum: 0,
            children: [-2, -1],
            mins: [-8, -8, -8],
            maxs: [8, 8, 8],
            firstface: 0,
            numfaces: 0,
        }],
        clipnodes: vec![DiskClipNode {
            planenum: 0,
            children: [-1, -2],
        }],
        leafs: vec![leaf(-2), leaf(-1)],
        models: vec![DiskModel {
            mins: [-7.0; 3],
            maxs: [7.0; 3],
            visleafs: 1,
            ..DiskModel::default()
        }],
        ..BspFile::default()
    }
}

fn lump(bytes: &[u8], number: usize) -> (usize, usize) {
    let at = 4 + number * 8;
    let field = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    (
        usize::try_from(field(at)).unwrap(),
        usize::try_from(field(at + 4)).unwrap(),
    )
}

// spec BSP-23.1, BSP-23.2: version 29, the lumps in WriteBSPFile's order after the header, each
// padded to four bytes, its length unpadded
#[test]
fn lumps_follow_the_header_in_id_order() {
    let bytes = minimal().to_bytes().unwrap();
    assert_eq!(i32::from_le_bytes(bytes[..4].try_into().unwrap()), 29);
    // planes 1, leafs 10, vertexes 3, nodes 5, texinfo 6, faces 7, clipnodes 9,
    // marksurfaces 11, surfedges 13, edges 12, models 14, lighting 8, visibility 4,
    // entities 0, textures 2.
    let order = [1, 10, 3, 5, 6, 7, 9, 11, 13, 12, 14, 8, 4, 0, 2];
    let mut at = 124;
    for number in order {
        let (offset, length) = lump(&bytes, number);
        assert_eq!(offset, at, "lump {number}");
        at += length.next_multiple_of(4);
    }
    assert_eq!(bytes.len(), at);
    let (entities, length) = lump(&bytes, 0);
    assert_eq!(length, 30, "the entity text's own length");
    assert_eq!(
        &bytes[entities + 30..entities + 32],
        [0, 0],
        "padded with NUL"
    );
    assert_eq!(lump(&bytes, 1).1, 20);
    assert_eq!(lump(&bytes, 10).1, 2 * 28);
    assert_eq!(lump(&bytes, 5).1, 24);
    assert_eq!(lump(&bytes, 9).1, 8);
    assert_eq!(lump(&bytes, 14).1, 64);
}

// spec BSP-23.1: what is written loads as a BSP29
#[test]
fn a_written_bsp_loads() {
    let bytes = minimal().to_bytes().unwrap();
    let bsp = Bsp::read(&bytes, BspLoader::WinQuake).unwrap();
    assert_eq!(bsp.planes.len(), 1);
    assert_eq!(bsp.nodes.len(), 1);
    assert_eq!(bsp.nodes[0].children, [-2, -1]);
    assert_eq!(bsp.leafs.len(), 2);
    assert_eq!(bsp.leafs[0].contents, -2);
    assert_eq!(bsp.models.len(), 1);
    assert_eq!(bsp.models[0].visleafs, 1);
    assert_eq!(bsp.entities(), b"{\n\"classname\" \"worldspawn\"\n}\n");
}

// spec BSP-23.3: a size or offset past a signed 32-bit integer is refused
#[test]
fn offsets_past_32_bits_are_refused() {
    assert_eq!(offset_field(0x7fff_ffff), Ok(i32::MAX));
    assert_eq!(offset_field(0x8000_0000), Err(BspWriteError::TooLarge));
    assert!(BspWriteError::TooLarge.to_string().contains("2 GiB"));
}
