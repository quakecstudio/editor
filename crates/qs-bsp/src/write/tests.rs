// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test, and offsets are small"
)]

use super::{
    BspFile, BspFormat, BspWriteError, DiskClipNode, DiskFace, DiskLeaf, DiskModel, DiskNode,
    DiskPlane, offset_field,
};
use crate::{Bsp, BspLoader, BspVersion, LumpId};

/// A world of one plane splitting space into the shared solid leaf and one empty leaf.
fn minimal() -> BspFile {
    let leaf = |contents| DiskLeaf {
        contents,
        visofs: -1,
        mins: [-8.0; 3],
        maxs: [8.0; 3],
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
            mins: [-8.0; 3],
            maxs: [8.0; 3],
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
    let bytes = minimal().to_bytes(BspFormat::Bsp29).unwrap();
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
    let bytes = minimal().to_bytes(BspFormat::Bsp29).unwrap();
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

// spec BSP-23.4, BSP-17.1: a BSP2 file carries the version `BSP2` and BSP-17's record sizes
#[test]
fn bsp2_widens_the_six_lumps() {
    let mut file = minimal();
    file.marksurfaces = vec![0];
    file.edges = vec![[0, 1]];
    let bytes = file.to_bytes(BspFormat::Bsp2).unwrap();
    assert_eq!(&bytes[..4], b"BSP2");
    assert_eq!(lump(&bytes, 5).1, 44, "dl2node_t");
    assert_eq!(lump(&bytes, 10).1, 2 * 44, "dl2leaf_t");
    assert_eq!(lump(&bytes, 9).1, 12, "dlclipnode_t");
    assert_eq!(lump(&bytes, 11).1, 4, "unsigned int marksurfaces");
    assert_eq!(lump(&bytes, 12).1, 8, "dledge_t");
    assert_eq!(lump(&bytes, 1).1, 20, "planes as BSP29");
    assert_eq!(lump(&bytes, 14).1, 64, "models as BSP29");
    let bytes29 = file.to_bytes(BspFormat::Bsp29).unwrap();
    assert_eq!(lump(&bytes29, 11).1, 2);
    assert_eq!(lump(&bytes29, 12).1, 4);
}

// spec BSP-23.4: what is written as BSP2 loads as BSP2, with values past BSP29's fields
#[test]
fn a_written_bsp2_loads_with_wide_values() {
    let mut file = minimal();
    file.nodes[0].mins = [-8.5, -8.0, -8.0];
    file.leafs[1].mins = [-70_000.25, -8.0, -8.0];
    file.clipnodes[0].children = [-1, -2];
    let bytes = file.to_bytes(BspFormat::Bsp2).unwrap();
    let bsp = Bsp::read(&bytes, BspLoader::FitzQuake).unwrap();
    assert_eq!(bsp.version, BspVersion::Bsp2);
    assert_eq!(bsp.nodes[0].children, [-2, -1]);
    assert_eq!(bsp.leafs.len(), 2);
    assert_eq!(bsp.models[0].visleafs, 1);
}

// spec BSP-23.5: a BSP29 bound is the float truncated toward zero
#[test]
fn bsp29_bounds_truncate_toward_zero() {
    let mut file = minimal();
    file.nodes[0].mins = [-8.9, 7.9, -0.5];
    let bytes = file.to_bytes(BspFormat::Bsp29).unwrap();
    let (nodes, _) = lump(&bytes, 5);
    let short = |at: usize| i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    // planenum 4 bytes, children 4 bytes, then mins.
    assert_eq!(
        [short(nodes + 8), short(nodes + 10), short(nodes + 12)],
        [-8, 7, 0]
    );
    file.nodes[0].mins = [-32_768.9, 32_767.9, f32::NAN];
    assert_eq!(
        file.to_bytes(BspFormat::Bsp29),
        Err(BspWriteError::TooWide(LumpId::Nodes)),
        "NaN fits no short"
    );
    file.nodes[0].mins[2] = 0.0;
    let bytes = file.to_bytes(BspFormat::Bsp29).unwrap();
    let (nodes, _) = lump(&bytes, 5);
    let short = |at: usize| i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    assert_eq!([short(nodes + 8), short(nodes + 10)], [-32_768, 32_767]);
}

// spec BSP-23.5: a value BSP29's 16-bit field cannot hold is refused, naming the lump; BSP2
// takes it
#[test]
fn bsp29_refuses_values_past_16_bits() {
    type Change = fn(&mut BspFile);
    let cases: [(LumpId, Change); 8] = [
        (LumpId::Nodes, |f| f.nodes[0].children[0] = 32_768),
        (LumpId::Nodes, |f| f.nodes[0].numfaces = 65_536),
        (LumpId::Clipnodes, |f| f.clipnodes[0].children[1] = -32_769),
        (LumpId::Leafs, |f| f.leafs[0].nummarksurfaces = 65_536),
        (LumpId::Marksurfaces, |f| f.marksurfaces = vec![65_536]),
        (LumpId::Edges, |f| f.edges = vec![[0, 65_536]]),
        (LumpId::Nodes, |f| f.nodes[0].maxs[0] = 32_768.0),
        (LumpId::Leafs, |f| f.leafs[0].mins[2] = -32_769.0),
    ];
    for (lump, change) in cases {
        let mut file = minimal();
        change(&mut file);
        assert_eq!(
            file.to_bytes(BspFormat::Bsp29),
            Err(BspWriteError::TooWide(lump)),
            "{lump:?}"
        );
        assert!(file.to_bytes(BspFormat::Bsp2).is_ok(), "{lump:?}");
    }
    let mut file = minimal();
    file.faces = vec![DiskFace {
        planenum: 40_000,
        side: 0,
        firstedge: 0,
        numedges: 0,
        texinfo: 0,
        styles: [255; 4],
        lightofs: -1,
    }];
    assert_eq!(
        file.to_bytes(BspFormat::Bsp29),
        Err(BspWriteError::TooWide(LumpId::Faces))
    );
    let mut wide = file.clone();
    wide.faces[0].planenum = 0;
    wide.faces[0].texinfo = 32_768;
    assert_eq!(
        wide.to_bytes(BspFormat::Bsp29),
        Err(BspWriteError::TooWide(LumpId::Faces))
    );
    assert!(
        BspWriteError::TooWide(LumpId::Faces)
            .to_string()
            .contains("Faces")
    );
}

// spec BSP-23.1: the vertexes, texinfo and surfedges are written as BSP-5, BSP-8 and BSP-14 lay
// them out, in either format
#[test]
fn vertexes_texinfo_and_surfedges_are_written() {
    let mut file = minimal();
    file.vertexes = vec![[1.0, 2.0, 3.0]];
    file.texinfo = vec![super::DiskTexinfo {
        vecs: [[1.0, 0.0, 0.0, 4.0], [0.0, 1.0, 0.0, 8.0]],
        miptex: 7,
        flags: 1,
    }];
    file.surfedges = vec![-5];
    for format in [BspFormat::Bsp29, BspFormat::Bsp2] {
        let bytes = file.to_bytes(format).unwrap();
        let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let int = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let (v, len) = lump(&bytes, 3);
        assert_eq!(len, 12);
        assert_eq!([float(v), float(v + 4), float(v + 8)], [1.0, 2.0, 3.0]);
        let (t, len) = lump(&bytes, 6);
        assert_eq!(len, 40);
        assert_eq!(float(t + 12), 4.0);
        assert_eq!(float(t + 28), 8.0);
        assert_eq!([int(t + 32), int(t + 36)], [7, 1]);
        let (e, len) = lump(&bytes, 13);
        assert_eq!(len, 4);
        assert_eq!(int(e), -5);
    }
}
