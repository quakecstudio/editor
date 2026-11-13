// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test, and the numbers are small \
              and exact"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map_bsp::Compiler;
use qs_wad::write::WadBuilder;

use crate::flow::{Flow, Leaves};
use crate::lump::{compress, written};
use crate::portals::PortalFile;
use crate::winding::{Plane, Winding};
use crate::{Level, Mode, VIS_MAX_MAP_VISIBILITY, Vis, VisError};

/// The side of one cell of the snake, in map units.
const CELL: i32 = 128;
/// The snake's width and length in cells.
const WIDE: i32 = 3;
const LONG: i32 = 7;
/// The snake's cells: three rows joined by columns two cells long, at alternate ends, so no
/// straight line goes from the first row to the last; the others are solid.
const OPEN: [[i32; 2]; 13] = [
    [0, 0],
    [1, 0],
    [2, 0],
    [2, 1],
    [2, 2],
    [0, 3],
    [1, 3],
    [2, 3],
    [0, 4],
    [0, 5],
    [0, 6],
    [1, 6],
    [2, 6],
];
/// A point in the first row and one in the last, where the water is.
const START: [f32; 3] = [64.0, 64.0, 48.0];
const END: [f32; 3] = [64.0, 832.0, 48.0];

/// A brush, a box from `min` to `max`, faced with `texture`.
fn cuboid(min: [i32; 3], max: [i32; 3], texture: &str) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) {texture} 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) {texture} 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) {texture} 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) {texture} 0 0 0 1 1\n}}\n"
    )
}

/// The solid boxes of the snake: its closed cells, a floor, a ceiling and walls round it.
fn solids() -> Vec<([i32; 3], [i32; 3])> {
    let (lo, hx, hy, top) = (0, WIDE * CELL, LONG * CELL, CELL);
    let mut boxes = vec![
        ([lo - 16, lo - 16, -16], [hx + 16, hy + 16, 0]),
        ([lo - 16, lo - 16, top], [hx + 16, hy + 16, top + 16]),
        ([lo - 16, lo - 16, 0], [lo, hy + 16, top]),
        ([hx, lo - 16, 0], [hx + 16, hy + 16, top]),
        ([lo, lo - 16, 0], [hx, lo, top]),
        ([lo, hy, 0], [hx, hy + 16, top]),
    ];
    for x in 0..WIDE {
        for y in 0..LONG {
            if !OPEN.contains(&[x, y]) {
                boxes.push((
                    [x * CELL, y * CELL, 0],
                    [(x + 1) * CELL, (y + 1) * CELL, top],
                ));
            }
        }
    }
    boxes
}

/// A texture lump `name`, 16 by 16.
fn miptex(name: &str) -> Vec<u8> {
    let mut lump = name.as_bytes().to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.resize(40 + 256 + 64 + 16 + 4, 1);
    lump
}

/// The snake compiled: its BSP and its portal file; the last cell's floor is `floor`.
fn snake(floor: &str) -> (Vec<u8>, String) {
    let mut world = String::from("{\n\"classname\" \"worldspawn\"\n\"wad\" \"test.wad\"\n");
    for (min, max) in solids() {
        world.push_str(&cuboid(min, max, "wall"));
    }
    world.push_str(&cuboid(
        [0, (LONG - 1) * CELL, 0],
        [CELL, LONG * CELL, 8],
        floor,
    ));
    world.push_str("}\n{\n\"classname\" \"info_player_start\"\n\"origin\" \"64 64 32\"\n}\n");
    let mut wad = WadBuilder::new();
    for name in ["wall", floor] {
        wad.add(name.as_bytes(), qs_wad::WAD_TYP_MIPTEX, miptex(name))
            .unwrap();
    }
    let compiled = Compiler::default()
        .compile(world.as_bytes(), Some(&wad.to_bytes().unwrap()))
        .unwrap();
    let prt = String::from_utf8(compiled.portal_file.unwrap()).unwrap();
    (compiled.bsp, prt)
}

/// The vis leaf `point` is in, walking the BSP's nodes; none for the solid leaf.
fn leaf_of(bsp: &Bsp<'_>, point: [f32; 3]) -> Option<usize> {
    let mut node = 0_i32;
    while node >= 0 {
        let n = &bsp.nodes[usize::try_from(node).unwrap()];
        let plane = &bsp.planes[n.planenum];
        let d =
            point[0] * plane.normal[0] + point[1] * plane.normal[1] + point[2] * plane.normal[2]
                - plane.dist;
        node = n.children[usize::from(d < 0.0)];
    }
    let leaf = usize::try_from(-1 - node).unwrap();
    leaf.checked_sub(1)
}

/// Whether the segment from `a` to `b` passes through no solid box.
fn clear(a: [f32; 3], b: [f32; 3]) -> bool {
    const STEPS: u16 = 256;
    let boxes = solids();
    (0..=STEPS).all(|i| {
        let t = f32::from(i) / f32::from(STEPS);
        let p = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
        !boxes
            .iter()
            .any(|(min, max)| (0..3).all(|k| (min[k] as f32) < p[k] && p[k] < (max[k] as f32)))
    })
}

/// Points in the snake's open cells, on a grid.
fn samples() -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for &[x, y] in &OPEN {
        for i in [24, 64, 104] {
            for j in [24, 64, 104] {
                out.push([(x * CELL + i) as f32, (y * CELL + j) as f32, 48.0]);
            }
        }
    }
    out
}

// spec VIS-4.2, VIS-4.6: vis is conservative: a leaf sees every leaf a clear line reaches from
// it, in either mode and at every level
#[test]
fn every_clear_line_is_seen() {
    let (bsp, prt) = snake("*water");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let points = samples();
    for mode in [
        Mode::Fast,
        Mode::Full(Level::Zero),
        Mode::Full(Level::One),
        Mode::Full(Level::Two),
        Mode::Full(Level::Three),
        Mode::Full(Level::Four),
    ] {
        let visible = Vis::new(mode).visible(&prt).unwrap();
        for &a in &points {
            for &b in &points {
                if !clear(a, b) {
                    continue;
                }
                let (Some(from), Some(to)) = (leaf_of(&read, a), leaf_of(&read, b)) else {
                    continue;
                };
                assert!(visible.sees(from, to), "{mode:?}: {a:?} to {b:?}");
            }
        }
    }
}

// spec VIS-1.3, VIS-4.4: the full mode sees no more than the fast one, and round the snake's
// corners it sees less: its two ends do not see each other
#[test]
fn the_flow_narrows_what_might_be_seen() {
    let (bsp, prt) = snake("*water");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let fast = Vis::new(Mode::Fast).visible(&prt).unwrap();
    let full = Vis::new(Mode::default()).visible(&prt).unwrap();
    assert_eq!(fast.leafs(), full.leafs());
    let mut narrower = false;
    for from in 0..full.leafs() {
        for to in 0..full.leafs() {
            assert!(!full.sees(from, to) || fast.sees(from, to));
            narrower |= fast.sees(from, to) && !full.sees(from, to);
        }
    }
    assert!(narrower, "the flow hides some leaf the bound shows");
    let start = leaf_of(&read, START).unwrap();
    let end = leaf_of(&read, END).unwrap();
    assert!(!full.sees(start, end), "round the corners");
    assert!(full.sees(start, start));
}

// spec VIS-5.1, VIS-5.2, VIS-5.4, VIS-6.1, VIS-6.3: run writes each leaf's compressed row at its
// visofs and full water for a leaf that sees the water, none for one that does not, and keeps
// the rest of the file
#[test]
fn run_writes_the_rows_and_ambients() {
    let (bsp, prt) = snake("*water");
    let written = Vis::default().run(&bsp, &prt).unwrap();
    let read = Bsp::read(&written, BspLoader::FitzQuake).unwrap();
    let visible = Vis::default().visible(&prt).unwrap();
    let leafs = visible.leafs();
    for leaf in 0..leafs {
        let row: Vec<u8> = (0..leafs.div_ceil(8))
            .map(|byte| {
                (0..8)
                    .filter(|bit| visible.sees(leaf, byte * 8 + bit))
                    .fold(0_u8, |acc, bit| acc | (1 << bit))
            })
            .collect();
        let at = usize::try_from(read.leafs[leaf + 1].visofs).unwrap();
        let packed = compress(&row);
        assert_eq!(&read.visibility[at..at + packed.len()], packed.as_slice());
    }
    let start = leaf_of(&read, START).unwrap();
    let end = leaf_of(&read, END).unwrap();
    assert_eq!(read.leafs[end + 1].ambient_level, [255, 0, 0, 0]);
    assert_eq!(read.leafs[start + 1].ambient_level, [0, 0, 0, 0]);
    let before = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    assert_eq!(read.entity_lump, before.entity_lump);
    assert_eq!(read.faces, before.faces);
}

// spec VIS-6.1, VIS-6.3: sky is full sky, lava only by its own name, slime and *04water count
// as water; a texture of another name is no emitter
#[test]
fn ambient_kinds_by_name() {
    let level = |name: &str| {
        let (bsp, prt) = snake(name);
        let written = Vis::default().run(&bsp, &prt).unwrap();
        let read = Bsp::read(&written, BspLoader::FitzQuake).unwrap();
        let end = leaf_of(&read, END).unwrap();
        read.leafs[end + 1].ambient_level
    };
    assert_eq!(level("sky1"), [0, 255, 0, 0]);
    assert_eq!(level("*lava"), [0, 0, 0, 255]);
    assert_eq!(level("*lava1"), [0, 0, 0, 0]);
    assert_eq!(level("*SLIME0"), [255, 0, 0, 0]);
    assert_eq!(level("*04water1"), [255, 0, 0, 0]);
    assert_eq!(level("rock"), [0, 0, 0, 0]);
}

// spec VIS-5.1: the test vectors
#[test]
fn rows_compress_as_vis_does() {
    assert_eq!(compress(&[0xff, 0, 0, 1]), [0xff, 0, 2, 1]);
    assert_eq!(compress(&[0]), [0, 1]);
    assert_eq!(compress(&[0; 300]), [0, 0xff, 0, 0x2d]);
    assert_eq!(compress(&[]), Vec::<u8>::new());
    assert_eq!(compress(&[0; 255]), [0, 0xff]);
    assert_eq!(compress(&[0; 256]), [0, 0xff, 0, 1]);
}

/// A portal file of `leafs` leaves and these portals, each its points and two leaves.
fn prt(leafs: usize, portals: &[(&[[f32; 3]], i64, i64)]) -> String {
    let mut text = format!("PRT1\n{leafs}\n{}\n", portals.len());
    for (points, a, b) in portals {
        text.push_str(&format!("{} {a} {b} ", points.len()));
        for p in *points {
            text.push_str(&format!("({} {} {} ) ", p[0], p[1], p[2]));
        }
        text.push('\n');
    }
    text
}

const SQUARE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [0.0, 64.0, 0.0],
    [0.0, 64.0, 64.0],
    [0.0, 0.0, 64.0],
];

// spec VIS-2.1, VIS-2.2, VIS-2.4, VIS-2.6: a bad header, another first word, a short or bad
// portal, a leaf out of range, too many points and too many portals on a leaf stop the run
#[test]
fn bad_portal_files_stop_the_run() {
    let read = |text: &str| PortalFile::read(text).map(|_| ());
    assert_eq!(read("PRT1\n2\n"), Err(VisError::Header));
    assert_eq!(read(""), Err(VisError::Header));
    assert_eq!(read("PRT2\n2\n0\n"), Err(VisError::NotPortalFile));
    assert_eq!(read(&prt(2, &[(&SQUARE, 0, 1)])), Ok(()));
    assert_eq!(read("PRT1\n2\n1\n4 0"), Err(VisError::Portal { number: 0 }));
    assert_eq!(
        read(&prt(2, &[(&SQUARE, 0, 2)])),
        Err(VisError::Portal { number: 0 })
    );
    assert_eq!(
        read(&prt(2, &[(&SQUARE, -1, 1)])),
        Err(VisError::Portal { number: 0 })
    );
    let most = [[0.0_f32; 3]; 64];
    assert_eq!(read(&prt(2, &[(&most, 0, 1)])), Ok(()));
    let many = [[0.0_f32; 3]; 65];
    assert_eq!(
        read(&prt(2, &[(&many, 0, 1)])),
        Err(VisError::TooManyPoints { number: 0 })
    );
    let bad_point = "PRT1\n2\n1\n4 0 1 (0 0 0 ) (0 64 0 ) (0 64 64 ) 0 0 64 )\n";
    assert_eq!(read(bad_point), Err(VisError::Portal { number: 0 }));
    let crowded: Vec<(&[[f32; 3]], i64, i64)> = (0..129).map(|_| (&SQUARE[..], 0, 1)).collect();
    assert_eq!(read(&prt(2, &crowded)), Err(VisError::TooManyLeafPortals));
}

// spec VIS-2.3: each file portal is two one-way portals: the forward one in the first leaf on
// the winding's plane reversed, looking into the second; the backward one the other way
#[test]
fn a_portal_is_two_one_way_portals() {
    let file = PortalFile::read(&prt(2, &[(&SQUARE, 0, 1)])).unwrap();
    assert_eq!(file.portals.len(), 2);
    let (forward, backward) = (&file.portals[0], &file.portals[1]);
    assert_eq!((forward.leaf, backward.leaf), (1, 0));
    assert_eq!(file.leaf_portals, [vec![0], vec![1]]);
    // cross(p0 - p1, p2 - p1) for the square in the x = 0 plane points along -x.
    assert_eq!(backward.plane.normal, [-1.0, 0.0, 0.0]);
    assert_eq!(forward.plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(forward.plane.dist, 0.0);
}

// spec VIS-4.6: a leaf whose portals see it stops the run
#[test]
fn a_leaf_seeing_itself_through_its_portals_stops_the_run() {
    let text = prt(1, &[(&SQUARE, 0, 0)]);
    assert_eq!(
        Vis::new(Mode::Fast).visible(&text),
        Err(VisError::SawIntoLeaf { leaf: 0 })
    );
}

// spec VIS-4.3: a clip keeps the front and the points on the plane, puts a split point exactly
// on an axial plane, and keeps nothing when no point is in front
#[test]
fn windings_clip_to_the_front() {
    let square = Winding::new(SQUARE.to_vec());
    let across = Plane {
        normal: [0.0, 1.0, 0.0],
        dist: 32.0,
    };
    let half = square.clipped(across).unwrap();
    assert_eq!(
        half.points(),
        [
            [0.0, 32.0, 0.0],
            [0.0, 64.0, 0.0],
            [0.0, 64.0, 64.0],
            [0.0, 32.0, 64.0]
        ]
    );
    let behind = Plane {
        normal: [0.0, 1.0, 0.0],
        dist: 100.0,
    };
    assert_eq!(square.clipped(behind), None);
    let on = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 0.0,
    };
    assert_eq!(
        square.clipped(on),
        None,
        "a winding on the plane is clipped away"
    );
    let all = Plane {
        normal: [0.0, 1.0, 0.0],
        dist: -10.0,
    };
    assert_eq!(square.clipped(all), Some(square.clone()));
}

// spec VIS-3.1, VIS-3.2, VIS-4.1: a portal might see the leaf it looks into and what a flood
// through facing portals reaches; the flow goes fewest first, the earlier of equal ones
#[test]
fn might_see_floods_through_facing_portals() {
    let at = |x: f32| -> Vec<[f32; 3]> { SQUARE.iter().map(|p| [x, p[1], p[2]]).collect() };
    let (a, b) = (at(0.0), at(64.0));
    let text = prt(3, &[(&a, 0, 1), (&b, 1, 2)]);
    let file = PortalFile::read(&text).unwrap();
    let flow = Flow::new(&file, Level::Two);
    let fast: Vec<Leaves> = Flow::new(&file, Level::Two).fast();
    let counts: Vec<usize> = fast.iter().map(Leaves::count).collect();
    // Leaf 0's portal looks into leaf 1 and through the next into leaf 2; the way back from
    // leaf 2 reaches leaf 1 and leaf 0; the others each one leaf.
    assert_eq!(counts, [2, 1, 1, 2]);
    assert!(fast[0].has(1) && fast[0].has(2) && !fast[0].has(0));
    let order = flow.order();
    let mut sorted = order.clone();
    sorted.sort_by_key(|&p| (counts[p], p));
    assert_eq!(order, sorted);
}

// spec VIS-4.7, VIS-4.8: the full mode gives the same sets and counts on one thread as on several
#[test]
fn threads_do_not_change_the_result() {
    let (_, prt) = snake("*water");
    let file = PortalFile::read(&prt).unwrap();
    let one = Flow::new(&file, Level::Two).full(1).unwrap();
    let many = Flow::new(&file, Level::Two).full(8).unwrap();
    assert_eq!(one, many);
}

/// The bytes of an `xxd` hex dump.
fn unxxd(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        let hex: String = rest
            .get(..40)
            .unwrap_or(rest)
            .chars()
            .filter(char::is_ascii_hexdigit)
            .collect();
        for pair in hex.as_bytes().chunks(2) {
            out.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
    }
    out
}

/// A vector file of `docs/spec/vectors/vis/`.
fn vector(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/vectors/vis")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

// spec VIS-4.2, VIS-4.4, VIS-4.7, VIS-4.8, VIS-5.2, VIS-6.2: every map of the vectors gives id's
// vis's visibility lump, visofs, ambient levels and flow counts, in the fast mode and at every
// level, on every thread
#[test]
fn every_vector_matches_id_vis() {
    let results = vector("results.tsv");
    let mut checked = 0;
    for line in results.lines().skip(1) {
        let [map, mode, visibility, visofs, ambients, counts] =
            line.split('\t').collect::<Vec<_>>()[..]
        else {
            panic!("a bad row: {line}");
        };
        let bsp = unxxd(&vector(&format!("{map}.bsp.txt")));
        let prt = vector(&format!("{map}.prt.txt"));
        let mode = match mode {
            "fast" => Mode::Fast,
            "0" => Mode::Full(Level::Zero),
            "1" => Mode::Full(Level::One),
            "2" => Mode::Full(Level::Two),
            "3" => Mode::Full(Level::Three),
            _ => Mode::Full(Level::Four),
        };
        let written = Vis::new(mode).run(&bsp, &prt).unwrap();
        let read = Bsp::read(&written, BspLoader::FitzQuake).unwrap();
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(read.visibility), visibility, "{map} {mode:?}");
        let leafs = read.models[0].visleafs;
        let leaves = &read.leafs[1..=usize::try_from(leafs).unwrap()];
        let ours: Vec<String> = leaves.iter().map(|l| l.visofs.to_string()).collect();
        assert_eq!(ours.join(","), visofs, "{map} {mode:?}");
        let levels: Vec<u8> = leaves.iter().flat_map(|l| l.ambient_level).collect();
        assert_eq!(hex(&levels), ambients, "{map} {mode:?}");
        let c = Vis::new(mode).visible(&prt).unwrap().counts();
        let ours = [
            c.chains,
            c.vis_tests,
            c.might_tests,
            c.checks,
            c.tests,
            c.passes,
        ];
        let ours: Vec<String> = ours.iter().map(ToString::to_string).collect();
        assert_eq!(ours.join(","), counts, "{map} {mode:?}");
        checked += 1;
    }
    assert_eq!(checked, 18, "three maps, six modes each");
}

// spec VIS-2.1, VIS-2.2, VIS-2.4, VIS-4.5, VIS-4.6, VIS-5.2: each error reads as vis's message
#[test]
fn errors_read_as_vis_writes_them() {
    let cases = [
        (VisError::Header, "LoadPortals: failed to read header"),
        (VisError::NotPortalFile, "LoadPortals: not a portal file"),
        (
            VisError::Portal { number: 3 },
            "LoadPortals: reading portal 3",
        ),
        (
            VisError::TooManyPoints { number: 4 },
            "LoadPortals: portal 4 has too many points",
        ),
        (VisError::TooManyLeafPortals, "Leaf with too many portals"),
        (VisError::LeafRecursion, "CheckStack: leaf recursion"),
        (
            VisError::SawIntoLeaf { leaf: 5 },
            "Leaf portals saw into leaf 5",
        ),
        (VisError::VismapOverflow, "Vismap expansion overflow"),
        (VisError::Format, "the BSP cannot be given visibility"),
        (VisError::Thread, "a vis thread could not be run"),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}

/// A winding of these points in the z = 0 plane.
fn flat(points: &[[f32; 2]]) -> Winding {
    Winding::new(points.iter().map(|&[x, y]| [x, y, 0.0]).collect())
}

/// The plane x = 0, facing +x.
const X0: Plane = Plane {
    normal: [1.0, 0.0, 0.0],
    dist: 0.0,
};

// spec VIS-4.3: a point exactly ON_EPSILON in front or behind is on the plane, as is one
// nearer; a split point on a plane facing -x is at minus its distance
#[test]
fn clip_epsilon_is_on_the_plane() {
    let only_on = flat(&[[0.1, 0.0], [-10.0, 0.0], [-10.0, 64.0]]);
    assert_eq!(
        only_on.clipped(X0),
        None,
        "0.1 in front is on, not in front"
    );
    let back_on = flat(&[[-0.1, 0.0], [10.0, 0.0], [10.0, 64.0]]);
    assert_eq!(
        back_on.clipped(X0),
        Some(back_on.clone()),
        "0.1 behind is on"
    );
    let near = flat(&[[0.05, 0.0], [10.0, 0.0], [10.0, 64.0]]);
    assert_eq!(near.clipped(X0), Some(near.clone()));
    let wide = flat(&[[0.0, 0.0], [64.0, 0.0], [64.0, 64.0], [0.0, 64.0]]);
    let below_32 = Plane {
        normal: [-1.0, 0.0, 0.0],
        dist: -32.0,
    };
    assert_eq!(
        wide.clipped(below_32),
        Some(flat(&[[0.0, 0.0], [32.0, 0.0], [32.0, 64.0], [0.0, 64.0]]))
    );
}

/// A straight tube of `cells` leaves along x, each joined to the next by a 64-unit square.
fn tube(cells: usize) -> String {
    let squares: Vec<Vec<[f32; 3]>> = (1..cells)
        .map(|i| {
            SQUARE
                .iter()
                .map(|p| [64.0 * i as f32, p[1], p[2]])
                .collect()
        })
        .collect();
    let portals: Vec<(&[[f32; 3]], i64, i64)> = squares
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                s.as_slice(),
                i64::try_from(i).unwrap(),
                i64::try_from(i + 1).unwrap(),
            )
        })
        .collect();
    prt(cells, &portals)
}

// spec VIS-4.2, VIS-4.4: down a straight tube every leaf sees every other, however long the
// path of leaves a flow walks
#[test]
fn a_long_tube_sees_end_to_end() {
    const CELLS: usize = 300;
    let visible = Vis::new(Mode::Full(Level::Four))
        .visible(&tube(CELLS))
        .unwrap();
    assert_eq!(visible.leafs(), CELLS);
    for from in [0, 1, CELLS.div_euclid(2), CELLS - 1] {
        assert!((0..CELLS).all(|to| visible.sees(from, to)), "{from}");
    }
    assert_eq!(
        visible.counts().chains,
        CELLS * (CELLS - 1),
        "each portal walks to the end"
    );
}

// spec VIS-5.2: a lump of exactly MAX_MAP_VISIBILITY bytes is written; one byte more stops the
// run
#[test]
fn the_lump_may_fill_max_map_visibility() {
    const LEAFS: usize = 2900;
    let width = LEAFS.div_ceil(8);
    // A row whose first `bytes` bytes are full, the rest empty.
    let row = |bytes: usize| {
        let mut row = Leaves::none(LEAFS);
        for leaf in 0..(bytes * 8).min(LEAFS) {
            row.add(leaf);
        }
        row
    };
    let size = |bytes: usize| compress(&row(bytes).row(LEAFS)).len();
    let rows_of = |target: usize| {
        let empty = size(0);
        let full = size(width);
        let spare = target - empty * LEAFS;
        let fulls = spare.div_euclid(full - empty);
        let rest = spare.rem_euclid(full - empty);
        let mut rows: Vec<Leaves> = (0..fulls).map(|_| row(width)).collect();
        let partial = (0..width).find(|&k| size(k) == empty + rest).unwrap();
        rows.push(row(partial));
        rows.extend((rows.len()..LEAFS).map(|_| row(0)));
        let total: usize = rows.iter().map(|r| compress(&r.row(LEAFS)).len()).sum();
        assert_eq!(total, target);
        rows
    };
    let (bsp, _) = snake("*water");
    let read = Bsp::read(&bsp, BspLoader::FitzQuake).unwrap();
    let at_most = written(&bsp, &read, &rows_of(VIS_MAX_MAP_VISIBILITY));
    assert_ne!(at_most, Err(VisError::VismapOverflow));
    let over = written(&bsp, &read, &rows_of(VIS_MAX_MAP_VISIBILITY + 1));
    assert_eq!(over, Err(VisError::VismapOverflow));
}

// spec VIS-5.4: the lump is appended at the first four-byte boundary past the file
#[test]
fn the_lump_starts_on_four_bytes() {
    let (bsp, prt) = snake("*water");
    assert!(bsp.len().is_multiple_of(4));
    let lump_at = |bytes: &[u8]| {
        let written = Vis::default().run(bytes, &prt).unwrap();
        let read = Bsp::read(&written, BspLoader::FitzQuake).unwrap();
        usize::try_from(read.lumps[qs_bsp::LumpId::Visibility as usize].fileofs).unwrap()
    };
    assert_eq!(lump_at(&bsp), bsp.len());
    let mut longer = bsp.clone();
    longer.push(0);
    assert_eq!(lump_at(&longer), bsp.len() + 4);
}

// spec VIS-4.3: two normals are one when each component is within EQUAL_EPSILON, and not when
// one differs by more
#[test]
fn normals_compare_within_equal_epsilon() {
    let plane = |normal| Plane { normal, dist: 0.0 };
    let tilted = plane([0.6, 0.8, 0.0]);
    assert!(tilted.same_normal(tilted));
    assert!(tilted.same_normal(plane([0.6005, 0.8, 0.0])));
    assert!(!tilted.same_normal(plane([0.602, 0.8, 0.0])));
    assert!(!plane([1.0, 0.0, 0.0]).same_normal(plane([0.0, 1.0, 0.0])));
}

// spec VIS-4.3: a split on a plane facing -x is put at minus its distance exactly, where the
// interpolation between the points would round elsewhere
#[test]
fn a_split_on_a_negative_axial_plane_is_exact() {
    let edge = flat(&[[0.1, 0.0], [10.0, 0.0], [10.0, 64.0], [0.1, 64.0]]);
    let below = Plane {
        normal: [-1.0, 0.0, 0.0],
        dist: -3.3,
    };
    let kept = edge.clipped(below).unwrap();
    let xs: Vec<f32> = kept.points().iter().map(|p| p[0]).collect();
    assert_eq!(xs, [0.1, 3.3, 3.3, 0.1]);
}
