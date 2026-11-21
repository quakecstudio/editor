// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::*;
use crate::mdl::write::{ModelFrame, ModelFrameEntry};
use crate::mdl::{MDL_ST_RAND, MDL_ST_SYNC};

const A: [f32; 3] = [-8.0, -8.0, 0.0];
const B: [f32; 3] = [8.0, -8.0, 0.0];
const C: [f32; 3] = [8.0, 8.0, 0.0];
const E: [f32; 3] = [-8.0, 8.0, 0.0];

/// The spec's pyramid with its tip at `tip`: `abt`, `bct`, `cet`, `eat`, `acb`, `aec`.
fn pyramid(tip: [f32; 3]) -> Vec<Corners> {
    vec![
        [A, B, tip],
        [B, C, tip],
        [C, E, tip],
        [E, A, tip],
        [A, C, B],
        [A, E, C],
    ]
}

/// A picture of 320 by 200 whose pixel at `(x, y)` is `x + y`, wrapped to a byte.
fn picture() -> Picture {
    Picture {
        width: 320,
        pixels: (0..200_u32)
            .flat_map(|y| (0..320_u32).map(move |x| x.wrapping_add(y).to_le_bytes()[0]))
            .collect(),
    }
}

fn frame(name: &str, triangles: Vec<Corners>) -> GrabFrame {
    GrabFrame {
        name: name.as_bytes().to_vec(),
        triangles,
    }
}

/// A script of `base` with one picture and the frames `frames`, nothing else given.
fn grab(base: Vec<Corners>, frames: Vec<GrabEntry>) -> Grab {
    Grab {
        base,
        skins: vec![picture()],
        frames,
        origin: [0.0; 3],
        scale: 1.0,
        eye_position: [0.0; 3],
        flags: 0,
        synctype: MDL_ST_RAND,
    }
}

fn single(model: &Model, entry: usize) -> &ModelFrame {
    let ModelFrameEntry::Single(frame) = &model.frames[entry] else {
        panic!("a single frame");
    };
    frame
}

fn packed(model: &Model, entry: usize) -> Vec<([u8; 3], u8)> {
    single(model, entry)
        .verts
        .iter()
        .map(|vert| (vert.v, vert.lightnormalindex))
        .collect()
}

// spec MDL-8.1, MDL-8.2, MDL-8.3, MDL-8.4, MDL-8.6, MDL-8.7, MDL-8.8: the spec's pyramid is
// the model id's modelgen wrote of it: its vertices in the order their corners come, its
// skin's size, where each vertex lies on it, which triangle is on the back, each frame's
// packed vertices and normals, and its size
#[test]
fn the_pyramid_is_modelgens() {
    let tip = [0.0, 0.0, 16.0];
    let model = grab(
        pyramid(tip),
        vec![
            GrabEntry::Single(frame("stand1", pyramid(tip))),
            GrabEntry::Single(frame("stand2", pyramid([2.0, 1.0, 20.5]))),
        ],
    )
    .build()
    .unwrap();
    assert_eq!(
        (model.header.skinwidth, model.header.skinheight),
        (264, 132)
    );
    let st: Vec<(i32, i32, i32)> = model
        .stverts
        .iter()
        .map(|vert| (vert.onseam, vert.s, vert.t))
        .collect();
    assert_eq!(
        st,
        [
            (32, 2, 130),
            (32, 130, 130),
            (32, 66, 2),
            (0, 130, 130),
            (0, 2, 130)
        ]
    );
    let triangles: Vec<(i32, [usize; 3])> = model
        .triangles
        .iter()
        .map(|triangle| (triangle.facesfront, triangle.vertindex))
        .collect();
    assert_eq!(
        triangles,
        [
            (0, [0, 1, 2]),
            (1, [1, 3, 2]),
            (1, [3, 4, 2]),
            (1, [4, 0, 2]),
            (1, [0, 3, 1]),
            (1, [0, 4, 3]),
        ]
    );
    assert_eq!(
        packed(&model, 0),
        [
            ([255, 0, 0], 17),
            ([255, 255, 0], 118),
            ([127, 127, 199], 84),
            ([0, 255, 0], 139),
            ([0, 0, 0], 42),
        ]
    );
    assert_eq!(packed(&model, 1)[2], ([111, 159, 255], 84));
    assert_eq!(single(&model, 1).name(), b"stand2");
    assert_eq!(model.header.size.to_bits(), 138.072_24_f32.to_bits());
    assert_eq!(model.header.synctype, MDL_ST_RAND);
    // The skin is the top left of its picture.
    let ModelSkin::Single(pixels) = &model.skins[0] else {
        panic!("a single skin");
    };
    assert_eq!(pixels.len(), 264 * 132);
    assert_eq!(pixels[264 + 5], 6, "the second row's sixth pixel");
}

// spec MDL-8.1, MDL-8.2, MDL-8.8: a corner within 0.001 of a vertex on every axis is that
// vertex and one farther on any axis a new one; a triangle with two corners at one vertex is
// degenerate: it is not written, its corners are vertices all the same, and it is counted in
// what the size is divided by
#[test]
fn corners_within_the_epsilon_are_one_vertex() {
    let tip = [0.0, 0.0, 16.0];
    let near = [8.000_5, -8.000_5, 0.000_5];
    let apart = [8.002, -8.0, 0.0];
    let base = Base::of(&[[A, B, tip], [A, near, tip], [A, apart, tip]]).unwrap();
    assert_eq!(
        base.vertices(),
        4,
        "a, b, the tip and the corner 0.002 away"
    );
    assert_eq!(base.triangles().len(), 3);
    assert_eq!(base.triangles()[1].vertindex, [0, 1, 2]);
    assert_eq!(base.triangles()[2].vertindex, [0, 3, 2]);

    let whole = pyramid(tip);
    let mut with_flat = whole.clone();
    with_flat.push([A, A, tip]);
    let plain = grab(
        whole.clone(),
        vec![GrabEntry::Single(frame("stand1", whole))],
    )
    .build()
    .unwrap();
    let flat = grab(
        with_flat.clone(),
        vec![GrabEntry::Single(frame("stand1", with_flat))],
    )
    .build()
    .unwrap();
    assert_eq!(flat.triangles, plain.triangles);
    assert_eq!(flat.stverts.len(), plain.stverts.len());
    // The same area over seven triangles, not six.
    assert_eq!(flat.header.size, plain.header.size * 6.0 / 7.0);
}

// spec MDL-8.3: a unit of the model is 8 pixels until the model's width would pass 150
// pixels or its height 190, and then the scale is what fits it; each half of the skin is
// 4 wider and higher, and the skin's width a multiple of 4
#[test]
fn the_skin_fits_the_base() {
    let tall = |width: f32, height: f32| {
        let (low, high, top) = (
            [0.0, 0.0, 0.0],
            [width, 0.0, 0.0],
            [width / 2.0, 0.0, height],
        );
        Base::of(&[[low, high, top]]).unwrap().skin_size()
    };
    // 10 wide and 10 high: 80 pixels each way, 84 with the margin, two halves.
    assert_eq!(tall(10.0, 10.0), [168, 84]);
    // 18.5 wide is 19 whole units: 152 pixels would pass 150, so 150 / 19 a unit.
    assert_eq!(tall(18.5, 4.0), [308, 4 * 8 + 4]);
    // 100 by 300: the width's scale of 1.5 gives a height of 450, so 190 / 300 a unit, and
    // a width of ceil(63.33) + 4 = 68.
    assert_eq!(tall(100.0, 300.0), [136, 194]);
    // An odd half-width is rounded up to a multiple of 4: 3 wide is 28 a half, 56; 3.5
    // wide is 4 units, 36 a half, 72; and 0.3 wide 1 unit, 12 a half, 24.
    assert_eq!(tall(3.0, 1.0), [56, 12]);
    assert_eq!(tall(0.3, 1.0), [24, 12]);
}

// spec MDL-8.4: a triangle whose normal points to +y is on the back, laid on the skin's
// right half; a vertex of both a front and a back triangle is on the seam and keeps the
// front's place, whichever triangle comes first
#[test]
fn front_and_back_share_the_seam() {
    let (low, high, top) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 0.0, 10.0]);
    // (low - high) x (top - high) points to +y: the back. Its mirror is the front.
    let back: Corners = [low, high, top];
    let front: Corners = [high, low, top];
    for order in [[back, front], [front, back]] {
        let base = Base::of(&order).unwrap();
        let facing: Vec<i32> = base.triangles().iter().map(|t| t.facesfront).collect();
        assert_eq!(facing.iter().sum::<i32>(), 1);
        for vert in base.stverts() {
            assert_eq!(vert.onseam, MDL_ALIAS_ONSEAM);
            assert!(vert.s <= 82, "on the front's half: {vert:?}");
        }
    }
    // The back alone is laid from the right half's edge, 84 + 2.
    let base = Base::of(&[back]).unwrap();
    assert_eq!(base.triangles()[0].facesfront, 0);
    let st: Vec<(i32, i32, i32)> = base
        .stverts()
        .iter()
        .map(|vert| (vert.onseam, vert.s, vert.t))
        .collect();
    assert_eq!(st, [(0, 86, 82), (0, 166, 82), (0, 126, 2)]);
}

// spec MDL-8.5: a skin is the top left of its picture, row by row; a picture narrower or
// shorter than the skin is refused
#[test]
fn a_skin_is_cut_from_its_picture() {
    let tip = [0.0, 0.0, 16.0];
    let mut script = grab(
        pyramid(tip),
        vec![GrabEntry::Single(frame("stand1", pyramid(tip)))],
    );
    script.skins.push(Picture {
        width: 264,
        pixels: vec![7; 264 * 132],
    });
    let model = script.build().unwrap();
    assert_eq!(model.skins.len(), 2);
    assert_eq!(model.skins[1], ModelSkin::Single(vec![7; 264 * 132]));

    for small in [
        Picture {
            width: 263,
            pixels: vec![0; 263 * 200],
        },
        Picture {
            width: 264,
            pixels: vec![0; 264 * 131],
        },
    ] {
        script.skins[1] = small;
        assert_eq!(
            script.build().unwrap_err(),
            GrabError::Skin {
                skin: 1,
                needs: [264, 132]
            }
        );
    }
}

// spec MDL-8.6: a corner (x, y, z) is the vertex (-y, x, z) times the scale, moved by the
// origin turned the same way and taken away; the eye position is turned and moved with it
#[test]
fn frames_are_turned_scaled_and_moved() {
    let tip = [0.0, 0.0, 16.0];
    let mut script = grab(
        pyramid(tip),
        vec![GrabEntry::Single(frame("stand1", pyramid(tip)))],
    );
    script.scale = 2.0;
    script.origin = [1.0, 2.0, 3.0];
    script.eye_position = [4.0, 5.0, 22.0];
    script.synctype = MDL_ST_SYNC;
    script.flags = 8;
    let model = script.build().unwrap();
    let header = model.header;
    // x runs from -(8 * 2) + 2 to 16 + 2, y from -16 - 1 to 16 - 1, z from 0 - 3 to 32 - 3;
    // the box holds the origin too (MDL-7.1).
    assert_eq!(header.scale_origin, [-14.0, -17.0, -3.0]);
    let reach = |axis: usize| header.scale[axis] * 255.9;
    for (axis, size) in [32.0_f32, 32.0, 32.0].into_iter().enumerate() {
        assert!((reach(axis) - size).abs() < 0.001, "{axis}");
    }
    assert_eq!(header.eye_position, [-5.0 + 2.0, 4.0 - 1.0, 22.0 - 3.0]);
    assert_eq!((header.synctype, header.flags), (MDL_ST_SYNC, 8));
    // The corner a, (-8, -8, 0), is the vertex (8, -8, 0) doubled and moved: (18, -17, -3),
    // the box's high x and low y and z.
    assert_eq!(single(&model, 0).verts[0].v, [255, 0, 0]);
    // Its size is the area of the triangles doubled twice over.
    let plain = grab(
        pyramid(tip),
        vec![GrabEntry::Single(frame("stand1", pyramid(tip)))],
    )
    .build()
    .unwrap();
    assert_eq!(header.size, plain.header.size * 4.0);
    // Its normals are the unscaled triangles': the same entries.
    assert_eq!(
        packed(&model, 0)
            .iter()
            .map(|(_, normal)| *normal)
            .collect::<Vec<_>>(),
        [17, 118, 84, 139, 42]
    );
}

// spec MDL-8.7: a vertex that is a corner of 20 triangles is made and one of 21 is refused;
// a vertex all of whose triangles are degenerate has no normal and is refused
#[test]
fn a_vertex_keeps_twenty_normals() {
    let hub = [0.0, 0.0, 0.0];
    let fan = |blades: u8| -> Vec<Corners> {
        (0..blades)
            .map(|blade| {
                let at = f32::from(blade);
                [hub, [at + 1.0, 1.0, 0.0], [at + 1.0, 0.0, 1.0]]
            })
            .collect()
    };
    let twenty = fan(20);
    assert!(
        grab(
            twenty.clone(),
            vec![GrabEntry::Single(frame("fan", twenty))]
        )
        .build()
        .is_ok()
    );
    let more = fan(21);
    assert_eq!(
        grab(more.clone(), vec![GrabEntry::Single(frame("fan", more))])
            .build()
            .unwrap_err(),
        GrabError::Corners { vertex: 0 }
    );

    let tip = [0.0, 0.0, 16.0];
    let lone = [40.0, 40.0, 40.0];
    let mut base = pyramid(tip);
    base.push([lone, lone, A]);
    assert_eq!(
        grab(base.clone(), vec![GrabEntry::Single(frame("stand1", base))])
            .build()
            .unwrap_err(),
        GrabError::Write(MdlWriteError::LooseVertex { vertex: 5 })
    );
}

// spec MDL-8.8: the size is the first frame's, whatever the later ones are; a group's frames
// end at the sums of their intervals; a frame with another number of triangles than the
// base's is refused
#[test]
fn the_first_frame_gives_the_size() {
    let tip = [0.0, 0.0, 16.0];
    let first = frame("stand1", pyramid(tip));
    let tall = frame("stand2", pyramid([0.0, 0.0, 64.0]));
    let one = grab(pyramid(tip), vec![GrabEntry::Single(first.clone())])
        .build()
        .unwrap();
    let grouped = grab(
        pyramid(tip),
        vec![GrabEntry::Group(vec![
            (first.clone(), MDL_GRAB_INTERVAL),
            (tall, 0.25),
        ])],
    )
    .build()
    .unwrap();
    assert_eq!(grouped.header.size, one.header.size);
    let ModelFrameEntry::Group {
        intervals, frames, ..
    } = &grouped.frames[0]
    else {
        panic!("a group");
    };
    assert_eq!(intervals, &[0.1, 0.1 + 0.25]);
    assert_eq!(frames.len(), 2);

    let mut short = pyramid(tip);
    short.pop();
    assert_eq!(
        grab(
            pyramid(tip),
            vec![
                GrabEntry::Single(first),
                GrabEntry::Single(frame("stand3", short))
            ]
        )
        .build()
        .unwrap_err(),
        GrabError::Triangles {
            frame: "stand3".to_owned()
        }
    );
}

// spec MDL-8.9: a base with no triangle is refused
#[test]
fn a_base_needs_a_triangle() {
    assert_eq!(Base::of(&[]).unwrap_err(), GrabError::NoTriangles);
    assert_eq!(
        grab(Vec::new(), Vec::new()).build().unwrap_err(),
        GrabError::NoTriangles
    );
    assert_eq!(
        GrabError::NoTriangles.to_string(),
        "the base has no triangle"
    );
}
