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
use crate::{Compiled, Hull, LeafKind, SurfaceFlags, Warning};

fn compiled(spec: &MapSpec) -> Compiled {
    compile(
        &MapFile::parse(map_text(spec).as_bytes()).unwrap(),
        None,
        240.0,
    )
    .unwrap()
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
// spec QBSP-15.43: edge 0 is unused, and leaf 0 has no visibility offset of its own, a value
// initialised `mleaf_t`'s 0
#[test]
fn a_sealed_room() {
    let c = compiled(&MapSpec::default());
    let bsp = read(&c);
    assert_eq!(bsp.leafs[0].contents, SOLID);
    assert_eq!(bsp.leafs[0].visofs, 0);
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
    let c = compile(&file, None, 240.0).unwrap();
    let bsp = read(&c);
    let head = bsp.models[1].headnode[2];
    assert!(head >= 0, "an empty tree's node for hull 2");
    let node = &bsp.clipnodes[usize::try_from(head).unwrap()];
    assert_eq!(node.children, [EMPTY, EMPTY]);
}

// spec QBSP-15.52: with no detail the portal file is PRT1 numbered by leaf; with detail it is
// PRT2 numbered by cluster, fewer than the leaves, each open leaf listed once in the cluster map,
// and its portals stop at the detail
#[test]
fn portal_files() {
    let open_leaves = |c: &Compiled| {
        read(c)
            .leafs
            .iter()
            .skip(1)
            .filter(|l| l.contents != SOLID)
            .count()
    };
    let c = compiled(&MapSpec::default());
    let prt = String::from_utf8(c.portal_file.clone().unwrap()).unwrap();
    let mut lines = prt.lines();
    assert_eq!(lines.next(), Some("PRT1"));
    let leaves: usize = lines.next().unwrap().parse().unwrap();
    let portals: usize = lines.next().unwrap().parse().unwrap();
    assert_eq!(leaves, open_leaves(&c), "every open leaf is numbered");
    assert_eq!(lines.count(), portals);

    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_detail",
        room([40, -32, 0], [96, 32, 48], "wall"),
    ));
    spec.world
        .push(BrushSpec::cube([-100, -8, -64], [-60, 8, 128], "*water"));
    let c = compiled(&spec);
    let prt = String::from_utf8(c.portal_file.clone().unwrap()).unwrap();
    let mut lines = prt.lines();
    assert_eq!(lines.next(), Some("PRT2"));
    let leaves: usize = lines.next().unwrap().parse().unwrap();
    let clusters: usize = lines.next().unwrap().parse().unwrap();
    let portals: usize = lines.next().unwrap().parse().unwrap();
    assert_eq!(leaves, open_leaves(&c), "every open leaf is numbered");
    assert!(clusters < leaves, "{clusters} clusters of {leaves} leaves");
    let rest: Vec<&str> = lines.collect();
    let (portal_lines, map) = rest.split_at(portals);
    assert!(portal_lines.iter().all(|l| {
        let numbers: Vec<usize> = l
            .split(' ')
            .skip(1)
            .take(2)
            .map(|n| n.parse().unwrap())
            .collect();
        numbers.iter().all(|&n| n < clusters)
    }));
    assert_eq!(map.len(), clusters);
    let mut listed: Vec<usize> = Vec::new();
    for line in map {
        let mut words: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(words.pop(), Some("-1"));
        listed.extend(words.iter().map(|w| w.parse::<usize>().unwrap()));
    }
    let sorted = {
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        sorted
    };
    assert_eq!(
        sorted,
        (0..leaves).collect::<Vec<_>>(),
        "each leaf in one cluster"
    );
}

// spec QBSP-15.42: the faces only a detail fence's leaves list, written solid, are also listed in
// the open leaves around it, so the game draws every face
#[test]
fn detail_fences_are_seen_from_outside() {
    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_detail_fence",
        vec![BrushSpec::cube([-32, -32, -64], [32, 32, 32], "fence")],
    ));
    spec.world
        .push(BrushSpec::cube([-8, -8, -64], [8, 8, 16], "pillar"));
    let c = compiled(&spec);
    let bsp = read(&c);
    let listed: std::collections::HashSet<usize> = bsp
        .leafs
        .iter()
        .flat_map(|leaf| {
            bsp.marksurfaces[leaf.firstmarksurface..leaf.firstmarksurface + leaf.nummarksurfaces]
                .iter()
                .copied()
        })
        .collect();
    assert!(
        bsp.leafs
            .iter()
            .any(|leaf| leaf.contents == SOLID && leaf.nummarksurfaces == 0)
    );
    assert_eq!(
        listed.len(),
        bsp.faces.len(),
        "every face is listed in a leaf"
    );
}

// spec QBSP-15.54: the compile gives each texinfo written its lighting flags, in the file's
// order
// spec QBSP-15.55: and what each leaf written is to a light sample: the shared solid leaf and
// a sky leaf closed, the inside of a fence open though the file calls it solid
#[test]
fn the_compile_tells_light_each_texinfos_flags_and_each_leafs_kind() {
    let mut spec = MapSpec::default();
    spec.world
        .push(BrushSpec::cube([-128, -128, 96], [128, 128, 128], "sky1"));
    let mut smooth = EntitySpec::brushes(
        "func_group",
        vec![BrushSpec::cube([-96, -96, -64], [-64, -64, -32], "smooth")],
    );
    smooth.pairs = vec![("_phong".into(), "1".into())];
    let fence = EntitySpec::brushes(
        "func_detail_fence",
        vec![BrushSpec::cube([32, 32, -64], [64, 64, 0], "{fence")],
    );
    spec.entities.extend([smooth, fence]);
    let c = compiled(&spec);
    let bsp = read(&c);
    assert_eq!(c.surfaces.len(), bsp.texinfo.len());
    assert_eq!(c.leaves.len(), bsp.leafs.len());
    assert_eq!(c.leaves[0], LeafKind::Closed);
    // The cube's six faces are the only smoothed ones, on a texinfo, or a few, of their own.
    let smooth = SurfaceFlags {
        phong_angle: 89.0,
        ..SurfaceFlags::default()
    };
    assert!(c.surfaces.contains(&smooth));
    assert!(c.surfaces.contains(&SurfaceFlags::default()));
    assert!(
        c.surfaces
            .iter()
            .all(|flags| *flags == smooth || *flags == SurfaceFlags::default())
    );
    let smoothed = bsp
        .faces
        .iter()
        .filter(|face| c.surfaces[face.texinfo] == smooth)
        .count();
    assert_eq!(smoothed, 5);
    let leaf_at = |p: [f32; 3]| {
        let mut node = bsp.models[0].headnode[0];
        while node >= 0 {
            let n = &bsp.nodes[usize::try_from(node).unwrap()];
            let plane = &bsp.planes[n.planenum];
            node = n.children[side(plane.normal, plane.dist, p)];
        }
        usize::try_from(-1 - node).unwrap()
    };
    let (air, in_fence, in_sky) = (
        leaf_at([0.0, 0.0, 24.0]),
        leaf_at([48.0, 48.0, -32.0]),
        leaf_at([0.0, 0.0, 112.0]),
    );
    assert_eq!(bsp.leafs[air].contents, EMPTY);
    assert_eq!(c.leaves[air], LeafKind::Open);
    assert_eq!(bsp.leafs[in_fence].contents, SOLID);
    assert_ne!(in_fence, 0);
    assert_eq!(c.leaves[in_fence], LeafKind::Open);
    assert_eq!(c.leaves[in_sky], LeafKind::Closed);
    assert!(!c.leaves.contains(&LeafKind::Native));
}
