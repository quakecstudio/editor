// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::Writer;
use crate::extended::game::Game;
use crate::extended::map::{Texinfos, load};
use crate::extended::vec::Plane;
use crate::fixtures::{MapSpec, map_text};

// spec QBSP-15.43: the file begins with edge 0 unused and leaf 0 solid; a plane and a texinfo
// are written once, the first time they are used, and a texinfo not drawn is -1
// spec QBSP-15.44: a vertex within half POINT_EQUAL_EPSILON of one written is that one
#[test]
fn planes_texinfo_and_vertices_once() {
    let mut map = load(
        &qs_map::MapFile::parse(map_text(&MapSpec::default()).as_bytes()).unwrap(),
        Game::Quake,
    );
    let mut w = Writer::new();
    assert_eq!(w.file.edges, [[0, 0]]);
    assert_eq!(w.file.leafs[0].contents, -2);
    let up = map.planes.add_or_find(Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 8.0,
    });
    assert_eq!(w.plane(&map.planes, up).unwrap(), 0);
    assert_eq!(w.plane(&map.planes, up.opposite()).unwrap(), 1);
    assert_eq!(w.plane(&map.planes, up).unwrap(), 0);
    assert_eq!(w.file.planes[0].plane_type, 2);
    let wall = map.entities[0].brushes[0].faces[0].texinfo;
    assert_eq!(w.texinfo(&map.texinfos, wall).unwrap(), 0);
    assert_eq!(w.texinfo(&map.texinfos, wall).unwrap(), 0);
    assert_eq!(w.texinfo(&map.texinfos, Texinfos::SKIP).unwrap(), -1);
    assert_eq!(w.file.texinfo.len(), 1);
    let a = w.vertex([1.0, 2.0, 3.0]).unwrap();
    assert_eq!(w.vertex([1.02, 1.98, 3.0]).unwrap(), a);
    assert_ne!(w.vertex([1.03, 2.0, 3.0]).unwrap(), a);
    assert_ne!(w.vertex([5.0, 2.0, 3.0]).unwrap(), a);
    w.finish();
    assert_eq!(w.file.vertexes.len(), 3);
    let mut empty = Writer::new();
    empty.finish();
    assert_eq!(empty.file.vertexes, [[0.0; 3]]);
}
