// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::*;
use crate::mdl::write::Model;
use crate::mdl::{MDL_DT_FACES_FRONT, MDL_ST_RAND, Mdl};

/// A square pyramid on the ground: four corners of its foot and its tip, six triangles
/// whose corners go around each as seen from outside.
fn pyramid(tip: [f32; 3]) -> Vec<[f32; 3]> {
    vec![
        [-8.0, -8.0, 0.0],
        [8.0, -8.0, 0.0],
        [8.0, 8.0, 0.0],
        [-8.0, 8.0, 0.0],
        tip,
    ]
}

fn shape(poses: Vec<PoseEntry>) -> Shape {
    let front = |vertindex: [usize; 3]| Triangle {
        facesfront: MDL_DT_FACES_FRONT,
        vertindex,
    };
    Shape {
        skin_width: 4,
        skin_height: 2,
        skins: vec![ModelSkin::Single(vec![3; 8])],
        stverts: vec![
            StVert {
                onseam: 0,
                s: 1,
                t: 1
            };
            5
        ],
        // Corners go around a triangle clockwise as seen from outside, as id's models have
        // them (MDL-7.4).
        triangles: vec![
            front([4, 1, 0]),
            front([4, 2, 1]),
            front([4, 3, 2]),
            front([4, 0, 3]),
            front([1, 2, 0]),
            front([2, 3, 0]),
        ],
        poses,
        eye_position: [0.0, 0.0, 12.0],
        flags: 8,
        synctype: MDL_ST_RAND,
    }
}

fn pose(name: &str, tip: [f32; 3]) -> Pose {
    Pose {
        name: name.as_bytes().to_vec(),
        positions: pyramid(tip),
    }
}

// spec MDL-7.1, MDL-7.2: the box the frames fill starts as the origin alone and grows to
// every vertex of every frame; a step is the box's size over 255.9, divided in double and
// stored in single, the origin its low corner, and the radius is the far corner's distance, the farther of each axis's two sides
#[test]
fn the_header_is_the_box_the_frames_fill() {
    let model = shape(vec![
        PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0])),
        PoseEntry::Single(pose("stand2", [4.0, -20.0, 32.0])),
    ])
    .build()
    .unwrap();
    let header = model.header;
    assert_eq!(header.scale_origin, [-8.0, -20.0, 0.0]);
    // 16, 28 and 32 over 255.9, each divided in double and rounded to single.
    assert_eq!(
        header.scale.map(f32::to_bits),
        [0x3d80_0cce, 0x3de0_1669, 0x3e00_0cce]
    );
    // The far sides: 8 either way in x (the high side when they are even), -20 in y, 32 in z.
    assert_eq!(
        header.bounding_radius,
        (8.0_f32 * 8.0 + 20.0 * 20.0 + 32.0 * 32.0).sqrt()
    );
    assert_eq!(header.eye_position, [0.0, 0.0, 12.0]);
    assert_eq!((header.flags, header.synctype), (8, MDL_ST_RAND));
    assert_eq!((header.skinwidth, header.skinheight), (4, 2));

    // A model all on one side of the origin still has the origin in its box.
    let lifted: Vec<[f32; 3]> = pyramid([0.0, 0.0, 16.0])
        .into_iter()
        .map(|[x, y, z]| [x + 100.0, y, z + 50.0])
        .collect();
    let model = shape(vec![PoseEntry::Single(Pose {
        name: b"up".to_vec(),
        positions: lifted,
    })])
    .build()
    .unwrap();
    assert_eq!(model.header.scale_origin, [0.0, -8.0, 0.0]);
    assert_eq!(
        model.header.scale[0].to_bits(),
        0x3ed8_159c,
        "108 over 255.9"
    );

    // A size whose quotient in single is one step of the last place off the one in double:
    // modelgen's is 0x3e180c09, and a division by a single 255.9 gives 0x3e180c0a.
    let wide: Vec<[f32; 3]> = pyramid([0.0, 0.0, 16.0])
        .into_iter()
        .map(|[x, y, z]| [if x > 0.0 { 29.996_906 } else { x }, y, z])
        .collect();
    let model = shape(vec![PoseEntry::Single(Pose {
        name: b"wide".to_vec(),
        positions: wide,
    })])
    .build()
    .unwrap();
    assert_eq!(model.header.scale[0].to_bits(), 0x3e18_0c09);
}

// spec MDL-7.3: a position packs to the whole part of its distance from the origin in steps,
// so the box's low side is 0 and its high side 255; a frame's bounds are the least and the
// most of its packed vertices, and a written position is within a step of where it was
#[test]
fn positions_pack_into_bytes() {
    let made = shape(vec![PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0]))])
        .build()
        .unwrap();
    let ModelFrameEntry::Single(frame) = &made.frames[0] else {
        panic!("single");
    };
    let bytes: Vec<[u8; 3]> = frame.verts.iter().map(|vert| vert.v).collect();
    assert_eq!(
        bytes,
        [
            [0, 0, 0],
            [255, 0, 0],
            [255, 255, 0],
            [0, 255, 0],
            [127, 127, 255]
        ]
    );
    assert_eq!(
        (frame.bboxmin.v, frame.bboxmax.v),
        ([0, 0, 0], [255, 255, 255])
    );
    assert_eq!(frame.name(), b"stand1");
    let written = made.to_bytes().unwrap();
    let read = Mdl::read(&written).unwrap();
    for (vert, was) in frame.verts.iter().zip(pyramid([0.0, 0.0, 16.0])) {
        let at = read.position(*vert);
        for ((now, then), step) in at.iter().zip(was).zip(made.header.scale) {
            assert!((now - then).abs() <= step, "{at:?} for {was:?}");
        }
    }

    // A model with no extent on an axis has no step there: every vertex packs to 0 on it.
    let flat = Shape {
        triangles: vec![
            Triangle {
                facesfront: MDL_DT_FACES_FRONT,
                vertindex: [0, 1, 2],
            },
            Triangle {
                facesfront: MDL_DT_FACES_FRONT,
                vertindex: [0, 2, 3],
            },
        ],
        stverts: vec![
            StVert {
                onseam: 0,
                s: 0,
                t: 0
            };
            4
        ],
        poses: vec![PoseEntry::Single(Pose {
            name: b"flat".to_vec(),
            positions: pyramid([0.0; 3])[..4].to_vec(),
        })],
        ..shape(Vec::new())
    }
    .build()
    .unwrap();
    assert_eq!(flat.header.scale[2], 0.0);
    let ModelFrameEntry::Single(frame) = &flat.frames[0] else {
        panic!("single");
    };
    assert!(frame.verts.iter().all(|vert| vert.v[2] == 0));
}

// spec MDL-7.4: a vertex's normal is the average of the unit normals of its triangles, each
// the cross product of its first and third corners from its second, made a unit vector and
// matched to the nearest entry of the normal table; a vertex of no triangle is refused
#[test]
fn normals_are_the_nearest_of_the_table() {
    let made = shape(vec![PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0]))])
        .build()
        .unwrap();
    let ModelFrameEntry::Single(frame) = &made.frames[0] else {
        panic!("single");
    };
    // The tip's four sides average to straight up; each foot corner leans down and out.
    let normal =
        |vertex: usize| MDL_VERTEX_NORMALS[usize::from(frame.verts[vertex].lightnormalindex)];
    assert_eq!(normal(4), [0.0, 0.0, 1.0]);
    for (vertex, [x, y, _]) in pyramid([0.0, 0.0, 16.0]).into_iter().enumerate().take(4) {
        let [nx, ny, nz] = normal(vertex);
        assert!(
            nx * x > 0.0 && ny * y > 0.0 && nz < 0.0,
            "{vertex}: {:?}",
            normal(vertex)
        );
    }

    let mut loose = shape(vec![PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0]))]);
    loose.stverts.push(StVert {
        onseam: 0,
        s: 0,
        t: 0,
    });
    let PoseEntry::Single(pose) = &mut loose.poses[0] else {
        panic!("single");
    };
    pose.positions.push([1.0, 1.0, 1.0]);
    assert_eq!(
        loose.build().unwrap_err(),
        MdlWriteError::LooseVertex { vertex: 5 }
    );
}

// spec MDL-7.5: the header's size is the first frame's triangle areas over their count
#[test]
fn the_size_is_the_average_triangle() {
    let made = shape(vec![PoseEntry::Single(pose("stand1", [0.0, 0.0, 6.0]))])
        .build()
        .unwrap();
    // Each side is a triangle of base 16 and slant height 10; the foot is two halves of a
    // square of 16.
    let total = 4.0 * (16.0 * 10.0 / 2.0) + 16.0 * 16.0;
    assert_eq!(made.header.size, total / 6.0_f32);
}

// spec MDL-7.6: a group's intervals are the times its frames end at, each frame's time added
// to those before it, and its bounds those of all its frames
#[test]
fn a_group_sums_its_intervals() {
    let made = shape(vec![
        PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0])),
        PoseEntry::Group(vec![
            (pose("run1", [0.0, 0.0, 8.0]), 0.1),
            (pose("run2", [0.0, 0.0, 32.0]), 0.25),
        ]),
    ])
    .build()
    .unwrap();
    let ModelFrameEntry::Group {
        frame_type,
        bboxmin,
        bboxmax,
        intervals,
        frames,
    } = &made.frames[1]
    else {
        panic!("group");
    };
    assert_eq!(*frame_type, MDL_FRAME_GROUP);
    assert_eq!(*intervals, [0.1, 0.1 + 0.25]);
    assert_eq!(
        (frames[0].name(), frames[1].name()),
        (b"run1".as_slice(), b"run2".as_slice())
    );
    // The first frame's tip is a quarter of the way up the box, the second's at its top.
    assert_eq!(frames[0].bboxmax.v[2], 63);
    assert_eq!((bboxmin.v, bboxmax.v), ([0, 0, 0], [255, 255, 255]));
    // What is made is written and read back as itself.
    let read = Model::read(&made.to_bytes().unwrap()).unwrap();
    assert_eq!(read.frames, made.frames);
}

// spec MDL-7.7: a pose without a position for each vertex, a name too long and a model the
// loader would refuse are not made
#[test]
fn what_cannot_be_a_model_is_refused() {
    let mut short = shape(vec![PoseEntry::Single(pose("stand1", [0.0, 0.0, 16.0]))]);
    let PoseEntry::Single(one) = &mut short.poses[0] else {
        panic!("single");
    };
    one.positions.pop();
    assert_eq!(
        short.build().unwrap_err(),
        MdlWriteError::FrameVertices { frame: 0 }
    );
    let named = shape(vec![PoseEntry::Single(pose(
        "a_name_too_long_",
        [0.0, 0.0, 16.0],
    ))]);
    assert_eq!(named.build().unwrap_err(), MdlWriteError::FrameName);
    let none = shape(Vec::new());
    assert_eq!(
        none.build().unwrap_err(),
        MdlWriteError::Unreadable(crate::MdlError::InvalidFrameCount { numframes: 0 })
    );
}
