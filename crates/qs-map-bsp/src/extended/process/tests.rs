// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test, and the numbers are small"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::MapFile;
use qs_map::qbsp::QbspWarning;
use qs_wad::write::WadBuilder;

use super::{compile, generates_clip_nodes, textures};
use crate::fixtures::{BrushSpec, EntitySpec, MapSpec, map_text, room};
use crate::{Compiled, Hull, Warning};

fn compiled(spec: &MapSpec) -> Compiled {
    compile(&MapFile::parse(map_text(spec).as_bytes()).unwrap(), None).unwrap()
}

fn read(c: &Compiled) -> Bsp<'_> {
    Bsp::read(&c.bsp, BspLoader::FitzQuake).unwrap()
}

fn side(normal: [f32; 3], dist: f32, p: [f32; 3]) -> usize {
    let d = normal[0] * p[0] + normal[1] * p[1] + normal[2] * p[2] - dist;
    usize::from(d < 0.0)
}

/// The contents of the drawing hull's leaf at `p`.
fn contents_at(bsp: &Bsp<'_>, p: [f32; 3]) -> i32 {
    let mut node = bsp.models[0].headnode[0];
    while node >= 0 {
        let n = &bsp.nodes[usize::try_from(node).unwrap()];
        let plane = &bsp.planes[n.planenum];
        node = n.children[side(plane.normal, plane.dist, p)];
    }
    bsp.leafs[usize::try_from(-1 - node).unwrap()].contents
}

/// The contents of clipping hull `hull`'s leaf at `p`.
fn clip_contents_at(bsp: &Bsp<'_>, hull: usize, p: [f32; 3]) -> i32 {
    let mut node = bsp.models[0].headnode[hull];
    while node >= 0 {
        let n = &bsp.clipnodes[usize::try_from(node).unwrap()];
        let plane = &bsp.planes[usize::try_from(n.planenum).unwrap()];
        node = n.children[side(plane.normal, plane.dist, p)];
    }
    node
}

const SOLID: i32 = -2;
const EMPTY: i32 = -1;

// spec QBSP-15.48: each hull is made for the world and each brush entity, the world filled from
// the void
// spec QBSP-15.49: the drawing hull's world is filled from the inside and built again
// spec QBSP-15.46: the drawing hull's leaves and nodes are written with leaf 0 the shared solid
// leaf, each clipping hull's clip nodes, and the model's bounds without the side space, shrunk
// by a unit
// spec QBSP-15.43: edge 0 is unused
#[test]
fn a_sealed_room() {
    let c = compiled(&MapSpec::default());
    let bsp = read(&c);
    assert_eq!(bsp.leafs[0].contents, SOLID);
    assert_eq!(bsp.edges[0], [0, 0]);
    assert_eq!(bsp.models.len(), 1);
    assert_eq!(contents_at(&bsp, [0.0, 0.0, 24.0]), EMPTY);
    assert_eq!(contents_at(&bsp, [0.0, 0.0, -72.0]), SOLID, "in the floor");
    for hull in [1, 2] {
        assert_eq!(clip_contents_at(&bsp, hull, [0.0, 0.0, 24.0]), EMPTY);
        assert_eq!(clip_contents_at(&bsp, hull, [0.0, 0.0, -60.0]), SOLID);
    }
    assert!(c.leak_file.is_none() && c.leak_trail.is_empty());
    assert!(!c.warnings.contains(&Warning::Leak));
    let model = &bsp.models[0];
    assert!(model.mins.iter().all(|c| *c <= -64.0) && model.maxs.iter().all(|c| *c >= 128.0));
}

// spec QBSP-15.44: vertices are welded, no two within half POINT_EQUAL_EPSILON
// spec QBSP-15.45: each face's edges are written, one the reverse of an edge of a face of the
// same contents used again once, and every face written has at least three
// spec QBSP-15.39: a face is made for each visible side of the room's inside
// spec QBSP-15.42: and listed in the leaf it touches
#[test]
fn faces_and_edges() {
    let c = compiled(&MapSpec::default());
    let bsp = read(&c);
    assert!(bsp.faces.len() >= 6);
    assert!(bsp.faces.iter().all(|f| f.numedges >= 3));
    let mut seen = std::collections::HashSet::new();
    for &e in &bsp.surfedges {
        assert_ne!(e, 0);
        if e < 0 {
            assert!(seen.insert(-e), "an edge used backwards once");
        }
    }
    for (i, a) in bsp.vertexes.iter().enumerate() {
        for b in bsp.vertexes.iter().skip(i + 1) {
            assert!(a.iter().zip(b).any(|(x, y)| (x - y).abs() > 0.025));
        }
    }
    assert!(
        bsp.leafs.iter().any(|l| l.nummarksurfaces >= 6),
        "the room's leaf lists its faces"
    );
}

// spec QBSP-15.36: a hole leaks, with the leak file's points and no portal file
#[test]
fn a_leak() {
    let mut spec = MapSpec::default();
    spec.world.remove(1);
    let c = compiled(&spec);
    assert!(c.warnings.contains(&Warning::Leak));
    let text = String::from_utf8(c.leak_file.unwrap()).unwrap();
    assert!(text.starts_with("0 0 24\n"), "{text}");
    assert!(!c.leak_trail.is_empty());
    assert_eq!(c.portal_file, None);
}

// spec QBSP-15.50: the entity lump leaves out the world brush entities and TrenchBroom's keys,
// and names each brush model; the textures lump has each texture, from the WAD when it holds it
#[test]
fn entities_and_textures() {
    let mut spec = MapSpec::default();
    let mut wall = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([-16, -16, 0], [16, 16, 16], "wall")],
    );
    wall.pairs = vec![("_tb_layer".into(), "1".into())];
    spec.entities.push(wall);
    spec.entities.push(EntitySpec::brushes(
        "func_group",
        vec![BrushSpec::cube([32, 32, -64], [48, 48, -48], "wall")],
    ));
    let c = compiled(&spec);
    let bsp = read(&c);
    let lump = String::from_utf8_lossy(bsp.entity_lump);
    assert!(lump.contains("\"model\" \"*1\""), "{lump}");
    assert!(!lump.contains("func_group") && !lump.contains("_tb_layer"));
    assert_eq!(bsp.models.len(), 2);

    let mut wad = WadBuilder::new();
    wad.add(b"wall", b'D', vec![7; 8]).unwrap();
    let wad = wad.to_bytes().unwrap();
    let mut warnings = Vec::new();
    let names = [b"skip".to_vec(), b"wall".to_vec()];
    let quiet = textures(&names, Some(&wad), super::SkipUse::Unnamed, &mut warnings).unwrap();
    assert!(
        warnings.is_empty(),
        "the compile's own skip texture is not reported"
    );
    let lump = textures(&names, Some(&wad), super::SkipUse::Named, &mut warnings).unwrap();
    assert_eq!(quiet, lump);
    assert_eq!(&lump[..4], 2_i32.to_le_bytes());
    assert_eq!(&lump[4..8], (-1_i32).to_le_bytes());
    assert_eq!(&lump[8..12], 12_i32.to_le_bytes());
    assert_eq!(&lump[12..], [7; 8]);
    assert_eq!(
        warnings,
        [Warning::Map(QbspWarning::TextureNotFound {
            name: b"skip".to_vec()
        })]
    );
}

// spec QBSP-15.48: an entity's `_hulls` bits choose the clipping hulls it collides in
#[test]
fn hulls_by_key() {
    let mut spec = MapSpec::default();
    let mut wall = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([-16, -16, 0], [16, 16, 16], "wall")],
    );
    wall.pairs = vec![("_hulls".into(), "2".into())];
    spec.entities.push(wall);
    let file = MapFile::parse(map_text(&spec).as_bytes()).unwrap();
    let entity = &file.entities[2];
    assert!(generates_clip_nodes(entity, Hull::Player));
    assert!(!generates_clip_nodes(entity, Hull::Large));
    assert!(generates_clip_nodes(&file.entities[1], Hull::Large));
    let c = compile(&file, None).unwrap();
    let bsp = read(&c);
    let head = bsp.models[1].headnode[2];
    assert!(head >= 0, "an empty tree's node for hull 2");
    let node = &bsp.clipnodes[usize::try_from(head).unwrap()];
    assert_eq!(node.children, [EMPTY, EMPTY]);
}

// spec QBSP-15.51: the portal file is PRT1 numbered by leaf without detail, PRT2 by cluster with
// detail, each cluster's leaves listed
#[test]
fn portal_files() {
    let c = compiled(&MapSpec::default());
    let prt = String::from_utf8(c.portal_file.unwrap()).unwrap();
    let mut lines = prt.lines();
    assert_eq!(lines.next(), Some("PRT1"));
    let leaves: usize = lines.next().unwrap().parse().unwrap();
    let portals: usize = lines.next().unwrap().parse().unwrap();
    assert!(leaves >= 1);
    assert_eq!(lines.count(), portals);
    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_detail",
        room([40, -32, 0], [96, 32, 48], "wall"),
    ));
    spec.world
        .push(BrushSpec::cube([-100, -8, -64], [-60, 8, 128], "wall"));
    let c = compiled(&spec);
    let prt = String::from_utf8(c.portal_file.unwrap()).unwrap();
    let mut lines = prt.lines();
    assert_eq!(lines.next(), Some("PRT2"));
    let leaves: usize = lines.next().unwrap().parse().unwrap();
    let clusters: usize = lines.next().unwrap().parse().unwrap();
    let portals: usize = lines.next().unwrap().parse().unwrap();
    let rest: Vec<&str> = lines.collect();
    assert_eq!(rest.len(), portals + clusters);
    let listed: usize = rest[portals..]
        .iter()
        .map(|l| l.split_whitespace().count() - 1)
        .sum();
    assert_eq!(listed, leaves);
}
