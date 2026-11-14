// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::MapFile;

use super::{Face, FaceMaker};
use crate::extended::brush::FaceRef;
use crate::extended::game::{Contents, Game};
use crate::extended::map::{EntityNum, LoadedMap, TexinfoNum, Texinfos, load};
use crate::extended::tree::Tree;
use crate::extended::vec::{Bounds, Plane, PlaneNum};
use crate::extended::winding::Winding;
use crate::fixtures::{BrushSpec, MapSpec, map_text};

/// A map whose world has a `wall` box and a `sky1` box, and the floor plane.
fn map() -> (LoadedMap, PlaneNum) {
    let spec = MapSpec {
        world: vec![
            BrushSpec::cube([0, 0, -16], [64, 64, 0], "wall"),
            BrushSpec::cube([100, 0, -16], [164, 64, 0], "sky1"),
        ],
        entities: Vec::new(),
        ..MapSpec::default()
    };
    let mut map = load(
        &MapFile::parse(map_text(&spec).as_bytes()).unwrap(),
        Game::Quake,
    );
    let floor = map.planes.add_or_find(Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 0.0,
    });
    (map, floor)
}

fn texinfo_of(map: &LoadedMap, brush: usize) -> TexinfoNum {
    map.entities[0].brushes[brush].faces[5].texinfo
}

fn face(map: &LoadedMap, planenum: PlaneNum, brush: usize, points: Vec<[f64; 3]>) -> Face {
    let mut f = Face {
        planenum,
        texinfo: texinfo_of(map, brush),
        contents: [Contents::EMPTY, Contents::new(Contents::SOLID)],
        winding: Winding::new(points),
        original_side: FaceRef {
            entity: EntityNum::WORLD,
            brush,
            face: 5,
        },
        origin: [0.0; 3],
        radius: 0.0,
        markleafs: Vec::new(),
    };
    f.update_sphere();
    f
}

/// A square on the floor from `x` to `x + w`, clockwise seen from above.
fn square(x: f64, w: f64) -> Vec<[f64; 3]> {
    vec![
        [x, 0.0, 0.0],
        [x, 32.0, 0.0],
        [x + w, 32.0, 0.0],
        [x + w, 0.0, 0.0],
    ]
}

// spec QBSP-15.40: two faces sharing an edge on one plane with one texinfo merge into one, the
// points in a line dropped; another texinfo, or liquid on one side only, keeps them apart
#[test]
fn faces_merge_across_shared_edges() {
    let (map, floor) = map();
    let maker = FaceMaker {
        map: &map,
        originals: &[],
        subdivide: 240.0,
    };
    let a = face(&map, floor, 0, square(0.0, 32.0));
    let b = face(&map, floor, 0, square(32.0, 32.0));
    let merged = maker.try_merge(&a, &b).unwrap();
    assert_eq!(merged.winding.len(), 4);
    assert!((merged.winding.area() - 64.0 * 32.0).abs() < 1e-9);
    let apart = face(&map, floor, 0, square(40.0, 32.0));
    assert!(maker.try_merge(&a, &apart).is_none(), "no shared edge");
    let other_texture = face(&map, floor, 1, square(32.0, 32.0));
    assert!(maker.try_merge(&a, &other_texture).is_none());
    let mut wet = b.clone();
    wet.contents[0] = Contents::new(Contents::WATER);
    assert!(maker.try_merge(&a, &wet).is_none());
    let list = maker.merge_list(vec![a.clone(), apart, b]);
    assert_eq!(list.len(), 2, "the two neighbours merged, the third kept");
}

// spec QBSP-15.41: a face wider than the subdivision size is cut along s, then t, into pieces
// no wider; a special face is left whole; a face whose sphere is on one side of a plane is not
// cut by it
#[test]
fn faces_are_subdivided() {
    let (map, floor) = map();
    let maker = FaceMaker {
        map: &map,
        originals: &[],
        subdivide: 240.0,
    };
    let long = face(&map, floor, 0, square(0.0, 1000.0));
    let pieces = maker.subdivide(long);
    assert!(pieces.len() >= 5, "{}", pieces.len());
    let texinfo = *map.texinfos.get(texinfo_of(&map, 0)).unwrap();
    for piece in &pieces {
        let s: Vec<f64> = piece
            .winding
            .points()
            .iter()
            .map(|p| {
                let [x, y, z, _] = texinfo.vecs[0].map(f64::from);
                p[0] * x + p[1] * y + p[2] * z
            })
            .collect();
        let (lo, hi) = s
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(hi.ceil() - lo.floor() <= 240.0);
    }
    let area: f64 = pieces.iter().map(|p| p.winding.area()).sum();
    assert!((area - 32_000.0).abs() < 1e-6);
    let sky = face(&map, floor, 1, square(0.0, 1000.0));
    assert_eq!(maker.subdivide(sky).len(), 1);
    let small = face(&map, floor, 0, square(0.0, 32.0));
    let far = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 100.0,
    };
    let [front, back] = small.clone().split(far);
    assert_eq!((front, back), (None, Some(small)));
}

// spec QBSP-15.42: a face not drawn, a hint or in front of sky is left out of the BSP
#[test]
fn some_faces_are_left_out() {
    let (map, floor) = map();
    let maker = FaceMaker {
        map: &map,
        originals: &[],
        subdivide: 240.0,
    };
    let tree = Tree {
        nodes: Vec::new(),
        bounds: Bounds::default(),
        portals: Vec::new(),
        outside: None,
        faces: Vec::new(),
    };
    let drawn = face(&map, floor, 0, square(0.0, 32.0));
    assert!(!maker.omits(&tree, &drawn));
    let mut skip = drawn.clone();
    skip.texinfo = Texinfos::SKIP;
    assert!(maker.omits(&tree, &skip));
    let mut in_sky = drawn;
    in_sky.contents[0] = Contents::new(Contents::SKY);
    assert!(maker.omits(&tree, &in_sky));
}
