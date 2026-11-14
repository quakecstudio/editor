// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::MapFile;

use super::{Fill, FillType, Filler, Occupant, Prefer};
use crate::Hull;
use crate::extended::brush::{HullBrushes, Loader, VisibleFaces};
use crate::extended::game::{Contents, Game};
use crate::extended::map::{EntityNum, LoadedMap, load};
use crate::extended::portals::Portalizer;
use crate::extended::tree::{BspInput, Tree, TreeSplit};
use crate::fixtures::{EntitySpec, MapSpec, PLAYER, map_text, room};

struct Built {
    map: LoadedMap,
    tree: Tree,
    loaded: HullBrushes,
    visible: VisibleFaces,
    occupants: Vec<Occupant>,
}

fn built(spec: &MapSpec, hull: Hull) -> Built {
    let file = MapFile::parse(map_text(spec).as_bytes()).unwrap();
    let mut map = load(&file, Game::Quake);
    let mut visible = VisibleFaces::default();
    let loaded = Loader {
        file: &file,
        map: &mut map,
        game: Game::Quake,
        visible: &mut visible,
    }
    .load_entity(EntityNum::WORLD, hull)
    .sorted();
    let extent = map.world_extent;
    let mut tree = BspInput {
        planes: &mut map.planes,
        texinfos: &map.texinfos,
        visible: &visible,
        extent,
    }
    .brush_bsp(loaded.bounds, &loaded.brushes, TreeSplit::Fast);
    Portalizer {
        planes: &map.planes,
        extent,
    }
    .make_tree_portals(&mut tree);
    let occupants = Occupant::of(map.entities.iter().map(|e| e.origin).zip(&file.entities));
    Built {
        map,
        tree,
        loaded,
        visible,
        occupants,
    }
}

fn fill(b: &mut Built, kind: FillType) -> Fill {
    Filler {
        planes: &b.map.planes,
        texinfos: &b.map.texinfos,
        originals: &b.loaded.brushes,
    }
    .fill_outside(
        &mut b.tree,
        &b.occupants,
        kind,
        &b.loaded.brushes,
        &mut b.visible,
    )
}

fn contents_at(b: &Built, point: [f64; 3]) -> Contents {
    let leaf = b.tree.point_in_leaf(&b.map.planes, point, Prefer::Sealing);
    b.tree.leaf_data(leaf).unwrap().contents
}

/// Beyond the walls, grown for the player too.
const BEYOND: [f64; 3] = [172.0, 172.0, 180.0];
const INSIDE: [f64; 3] = [0.0, 0.0, 24.0];

// spec QBSP-15.35: the occupants are the entities after the world away from the origin and not
// nofill
// spec QBSP-15.37: a sealed room's outside is made solid and its inside left, from the inside in
// the drawing hull and from the void in a clipping hull
#[test]
fn a_sealed_room_is_filled() {
    for (hull, kind) in [
        (Hull::Draw, FillType::Inside),
        (Hull::Player, FillType::Outside),
    ] {
        let mut b = built(&MapSpec::default(), hull);
        assert_eq!(b.occupants.len(), 1);
        assert_eq!(b.occupants[0].entity, EntityNum::new(1));
        assert!(contents_at(&b, BEYOND).is_empty());
        assert_eq!(fill(&mut b, kind), Fill::Filled, "{hull:?}");
        assert!(contents_at(&b, BEYOND).is_solid());
        assert!(contents_at(&b, INSIDE).is_empty());
    }
    let mut spec = MapSpec::default();
    let mut nofill = EntitySpec::point("info_null", [1000, 0, 0]);
    nofill.pairs = vec![("_nofill".into(), "1".into())];
    spec.entities.push(nofill);
    spec.entities
        .push(EntitySpec::point("info_null", [0, 0, 0]));
    assert_eq!(built(&spec, Hull::Draw).occupants.len(), 1);
}

// spec QBSP-15.35: a point exactly on a node's plane is in the leaf that seals when asked to,
// else the open one
#[test]
fn points_on_planes() {
    let mut b = built(&MapSpec::default(), Hull::Draw);
    assert_eq!(fill(&mut b, FillType::Inside), Fill::Filled);
    let on_floor = [0.0, 0.0, -64.0];
    let sealed = b
        .tree
        .point_in_leaf(&b.map.planes, on_floor, Prefer::Sealing);
    let open = b.tree.point_in_leaf(&b.map.planes, on_floor, Prefer::Open);
    assert!(b.tree.leaf_data(sealed).unwrap().contents.is_solid());
    assert!(b.tree.leaf_data(open).unwrap().contents.is_empty());
}

// spec QBSP-15.36: a room with a hole leaks, the line from the occupant to the void; the leak
// file's points are each segment's ends
#[test]
fn a_hole_leaks() {
    for (hull, kind) in [
        (Hull::Draw, FillType::Inside),
        (Hull::Player, FillType::Outside),
    ] {
        let mut spec = MapSpec::default();
        spec.world.remove(1);
        let mut b = built(&spec, hull);
        let Fill::Leak {
            entity,
            origin,
            line,
        } = fill(&mut b, kind)
        else {
            panic!("a leak in {hull:?}");
        };
        assert_eq!(entity, EntityNum::new(1));
        assert_eq!(origin, PLAYER.map(f64::from));
        assert!(!line.is_empty());
        let points = b.tree.leak_points(origin, &line);
        assert_eq!(points.len(), line.len() * 2);
        assert_eq!(points[0], origin);
        assert_eq!(points[1], points[2]);
        assert!(contents_at(&b, BEYOND).is_empty(), "not filled");
    }
}

// spec QBSP-15.35: with no occupant nothing is filled
#[test]
fn no_occupant_no_fill() {
    let spec = MapSpec {
        entities: Vec::new(),
        ..MapSpec::default()
    };
    let mut b = built(&spec, Hull::Draw);
    assert_eq!(fill(&mut b, FillType::Inside), Fill::NoOccupant);
    assert!(contents_at(&b, BEYOND).is_empty());
}

// spec QBSP-15.38: a pocket enclosed by detail that no occupant reaches is made detail solid
#[test]
fn enclosed_pockets_are_detail_filled() {
    let mut spec = MapSpec::default();
    spec.entities.push(EntitySpec::brushes(
        "func_detail",
        room([56, -32, 0], [96, 32, 48], "wall"),
    ));
    let pocket = [76.0, 0.0, 24.0];
    let mut b = built(&spec, Hull::Draw);
    assert_eq!(fill(&mut b, FillType::Inside), Fill::Filled);
    assert!(
        contents_at(&b, pocket).is_empty(),
        "the outside fill passes detail"
    );
    let filler = Filler {
        planes: &b.map.planes,
        texinfos: &b.map.texinfos,
        originals: &b.loaded.brushes,
    };
    filler.fill_detail(
        &mut b.tree,
        &[],
        Prefer::Sealing,
        &b.loaded.brushes,
        &mut b.visible,
    );
    assert!(contents_at(&b, pocket).is_empty(), "no occupant, no fill");
    filler.fill_detail(
        &mut b.tree,
        &b.occupants,
        Prefer::Sealing,
        &b.loaded.brushes,
        &mut b.visible,
    );
    assert!(contents_at(&b, pocket).is_detail_solid());
    assert!(contents_at(&b, INSIDE).is_empty(), "occupied");
}

// spec QBSP-15.37: a brush model's sides are seen from its open leaves without a fill
#[test]
fn brush_models_mark_their_sides() {
    let mut b = built(&MapSpec::default(), Hull::Draw);
    let face = b.loaded.brushes[0].sides[0].source.unwrap();
    b.visible.hide(face);
    Filler {
        planes: &b.map.planes,
        texinfos: &b.map.texinfos,
        originals: &b.loaded.brushes,
    }
    .fill_brush_entity(&mut b.tree, &b.loaded.brushes, &mut b.visible);
    let seen = b
        .loaded
        .brushes
        .iter()
        .flat_map(|br| &br.sides)
        .filter_map(|s| s.source);
    assert!(seen.clone().any(|f| b.visible.contains(f)));
    assert!(contents_at(&b, BEYOND).is_empty(), "nothing filled");
}
