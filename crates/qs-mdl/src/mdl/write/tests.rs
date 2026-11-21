// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index, sum or panic is a failed test"
)]

use super::*;

/// Every `.mdl` of the PAKs named in `QS_MDL_PAKS`, separated by `:`, with its name.
fn corpus() -> Vec<(String, Vec<u8>)> {
    let Ok(paks) = std::env::var("QS_MDL_PAKS") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pak in paks.split(':').filter(|pak| !pak.is_empty()) {
        let data = std::fs::read(pak).unwrap();
        let word = |at: usize| {
            usize::try_from(i32::from_le_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
        };
        let (dir, len) = (word(4), word(8));
        for entry in (dir..dir + len).step_by(64) {
            let name: Vec<u8> = data[entry..entry + 56]
                .iter()
                .copied()
                .take_while(|&byte| byte != 0)
                .collect();
            let name = String::from_utf8_lossy(&name).into_owned();
            if name.ends_with(".mdl") {
                let (at, size) = (word(entry + 56), word(entry + 60));
                out.push((format!("{pak}:{name}"), data[at..at + size].to_vec()));
            }
        }
    }
    out
}

// spec MDL-6.1: a model read and written with nothing changed is its file, byte for byte;
// run over a game's models with QS_MDL_PAKS, and over none without it
#[test]
fn a_games_models_are_written_back_as_read() {
    let mut count = 0;
    for (name, bytes) in corpus() {
        let model = Model::read(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let written = model
            .to_bytes()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            written == bytes,
            "{name}: {} bytes for {}",
            written.len(),
            bytes.len()
        );
        count += 1;
    }
    eprintln!("{count} models written back as read");
}

use proptest::prelude::*;

use crate::mdl::fixtures::MdlSpec;
use crate::mdl::{MDL_FRAME_GROUP, MDL_SKIN_GROUP};

/// The `base` vector, read whole.
fn base() -> (Vec<u8>, Model) {
    let bytes = MdlSpec::default().build();
    let model = Model::read(&bytes).unwrap();
    (bytes, model)
}

// spec MDL-6.1: the base vector read and written with nothing changed is itself, and bytes
// after its last frame entry are written back with it
#[test]
fn a_model_is_written_back_as_read() {
    let (bytes, model) = base();
    assert_eq!(model.to_bytes().unwrap(), bytes);
    assert!(model.trailing.is_empty());

    let mut longer = bytes.clone();
    longer.extend_from_slice(b"what a tool left");
    let model = Model::read(&longer).unwrap();
    assert_eq!(model.trailing, b"what a tool left");
    assert_eq!(model.to_bytes().unwrap(), longer);
}

// spec MDL-6.2: the header's counts are those of the parts written, not those read
#[test]
fn the_header_counts_what_is_written() {
    let (_, mut model) = base();
    let first = model.frames[0].clone();
    model.frames.push(first);
    model.skins.push(model.skins[0].clone());
    model.triangles.push(model.triangles[0]);
    // The counts held are still those read.
    assert_eq!(model.header.numframes, 2);
    let read = Model::read(&model.to_bytes().unwrap()).unwrap();
    assert_eq!(read.header.numframes, 3);
    assert_eq!(read.header.numskins, 2);
    assert_eq!(read.header.numtris, 2);
    assert_eq!(read.header.numverts, 3);
    assert_eq!(read.frames, model.frames);
    // The rest of the header is written as held.
    model.header.flags = 1;
    model.header.scale = [2.0, 3.0, 4.0];
    let read = Model::read(&model.to_bytes().unwrap()).unwrap();
    assert_eq!((read.header.flags, read.header.scale), (1, [2.0, 3.0, 4.0]));
}

// spec MDL-6.3: a frame keeps the 16 bytes of its name as read, the bytes after its NUL with
// them; a name that is set is followed by NULs, and one too long or holding a NUL is refused
#[test]
fn a_frames_name_keeps_its_bytes() {
    let (mut bytes, _) = base();
    let at = bytes.windows(6).position(|w| w == b"stand1").unwrap();
    bytes[at + 7] = 0xE5;
    bytes[at + 8] = 0x3F;
    let mut model = Model::read(&bytes).unwrap();
    let ModelFrameEntry::Single(frame) = &mut model.frames[0] else {
        panic!("the base vector's first frame is single");
    };
    assert_eq!(frame.name(), b"stand1");
    assert_eq!(&frame.name[6..9], &[0, 0xE5, 0x3F]);
    assert_eq!(model.to_bytes().unwrap(), bytes);

    let ModelFrameEntry::Single(frame) = &mut model.frames[0] else {
        panic!("the base vector's first frame is single");
    };
    frame.set_name(b"walk1").unwrap();
    assert_eq!(frame.name(), b"walk1");
    assert_eq!(frame.name[5..], [0; 11]);
    frame.set_name(b"fifteen_bytes15").unwrap();
    assert_eq!(
        frame.set_name(b"sixteen_bytes_16"),
        Err(MdlWriteError::FrameName)
    );
    assert_eq!(frame.set_name(b"a\0b"), Err(MdlWriteError::FrameName));
    assert_eq!(frame.name(), b"fifteen_bytes15");
}

// spec MDL-6.4: parts that do not fit each other are not written, nor is a model the loader
// would refuse, which is refused with the loader's error
#[test]
fn a_model_that_would_not_load_is_not_written() {
    let (_, model) = base();
    let broken = |change: fn(&mut Model)| {
        let mut model = model.clone();
        change(&mut model);
        model.to_bytes().unwrap_err()
    };
    assert_eq!(
        broken(|m| m.skins[0] = ModelSkin::Single(vec![0; 7])),
        MdlWriteError::SkinSize { skin: 0 }
    );
    assert_eq!(
        broken(|m| {
            m.skins.push(ModelSkin::Group {
                skin_type: MDL_SKIN_GROUP,
                intervals: vec![0.1],
                skins: vec![vec![0; 8], vec![0; 9]],
            });
        }),
        MdlWriteError::Intervals
    );
    assert_eq!(
        broken(|m| {
            m.skins.push(ModelSkin::Group {
                skin_type: MDL_SKIN_GROUP,
                intervals: vec![0.1, 0.2],
                skins: vec![vec![0; 8], vec![0; 9]],
            });
        }),
        MdlWriteError::SkinSize { skin: 1 }
    );
    assert_eq!(
        broken(|m| {
            let ModelFrameEntry::Single(frame) = &mut m.frames[0] else {
                panic!("single");
            };
            frame.verts.pop();
        }),
        MdlWriteError::FrameVertices { frame: 0 }
    );
    assert_eq!(
        broken(|m| {
            let ModelFrameEntry::Group { intervals, .. } = &mut m.frames[1] else {
                panic!("group");
            };
            intervals.pop();
        }),
        MdlWriteError::Intervals
    );
    // The loader's own checks, with its errors.
    assert_eq!(
        broken(|m| m.frames.clear()),
        MdlWriteError::Unreadable(MdlError::InvalidFrameCount { numframes: 0 })
    );
    assert_eq!(
        broken(|m| m.triangles[0].vertindex[2] = 3),
        MdlWriteError::Unreadable(MdlError::BadVertexIndex { triangle: 0 })
    );
    assert_eq!(
        broken(|m| {
            let ModelFrameEntry::Group { intervals, .. } = &mut m.frames[1] else {
                panic!("group");
            };
            intervals[1] = 0.0;
        }),
        MdlWriteError::Unreadable(MdlError::FrameInterval)
    );
    assert_eq!(
        broken(|m| m.header.skinwidth = 6),
        MdlWriteError::SkinSize { skin: 0 }
    );
    // Each error says what is wrong.
    for error in [
        MdlWriteError::FrameName,
        MdlWriteError::SkinSize { skin: 1 },
        MdlWriteError::FrameVertices { frame: 2 },
        MdlWriteError::Intervals,
        MdlWriteError::TooMany,
        MdlWriteError::Unreadable(MdlError::NoTriangles),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

/// A vertex of any bytes.
fn vert() -> impl Strategy<Value = TriVertX> {
    any::<[u8; 4]>().prop_map(|[x, y, z, normal]| TriVertX {
        v: [x, y, z],
        lightnormalindex: normal,
    })
}

/// A frame of `numverts` vertices with any name and bounds.
fn frame(numverts: usize) -> impl Strategy<Value = ModelFrame> {
    (
        vert(),
        vert(),
        any::<[u8; 16]>(),
        proptest::collection::vec(vert(), numverts),
    )
        .prop_map(|(bboxmin, bboxmax, name, verts)| ModelFrame {
            bboxmin,
            bboxmax,
            name,
            verts,
        })
}

/// An interval the loader takes: greater than 0.
fn interval() -> impl Strategy<Value = f32> {
    0.01_f32..10.0
}

/// A model the loader takes, of any shape within small sizes.
fn model() -> impl Strategy<Value = Model> {
    (1_usize..5, 1_i32..3, 0_i32..4).prop_flat_map(|(numverts, quarter_width, height)| {
        let width = quarter_width * 4;
        let pixels = usize::try_from(width * height).unwrap();
        let skin = proptest::collection::vec(any::<u8>(), pixels);
        let skin_entry = prop_oneof![
            skin.clone().prop_map(ModelSkin::Single),
            (1_usize..3).prop_flat_map(move |count| {
                (
                    proptest::collection::vec(interval(), count),
                    proptest::collection::vec(
                        proptest::collection::vec(any::<u8>(), pixels),
                        count,
                    ),
                )
                    .prop_map(|(intervals, skins)| ModelSkin::Group {
                        skin_type: MDL_SKIN_GROUP,
                        intervals,
                        skins,
                    })
            }),
        ];
        let frame_entry = prop_oneof![
            frame(numverts).prop_map(ModelFrameEntry::Single),
            (1_usize..3).prop_flat_map(move |count| {
                (
                    vert(),
                    vert(),
                    proptest::collection::vec(interval(), count),
                    proptest::collection::vec(frame(numverts), count),
                )
                    .prop_map(|(bboxmin, bboxmax, intervals, frames)| {
                        ModelFrameEntry::Group {
                            frame_type: MDL_FRAME_GROUP,
                            bboxmin,
                            bboxmax,
                            intervals,
                            frames,
                        }
                    })
            }),
        ];
        (
            any::<[f32; 10]>(),
            any::<[i32; 2]>(),
            proptest::collection::vec(skin_entry, 1..3),
            proptest::collection::vec(any::<[i32; 3]>(), numverts),
            proptest::collection::vec(
                (any::<i32>(), [0..numverts, 0..numverts, 0..numverts]),
                1..4,
            ),
            proptest::collection::vec(frame_entry, 1..4),
            proptest::collection::vec(any::<u8>(), 0..5),
        )
            .prop_map(
                move |(floats, [synctype, flags], skins, stverts, triangles, frames, trailing)| {
                    let [sx, sy, sz, ox, oy, oz, radius, ex, ey, size] = floats;
                    Model {
                        header: MdlHeader {
                            scale: [sx, sy, sz],
                            scale_origin: [ox, oy, oz],
                            bounding_radius: radius,
                            eye_position: [ex, ey, size],
                            numskins: 0,
                            skinwidth: width,
                            skinheight: height,
                            numverts: 0,
                            numtris: 0,
                            numframes: 0,
                            synctype,
                            flags,
                            size,
                        },
                        skins,
                        stverts: stverts
                            .into_iter()
                            .map(|[onseam, s, t]| StVert { onseam, s, t })
                            .collect(),
                        triangles: triangles
                            .into_iter()
                            .map(|(facesfront, vertindex)| Triangle {
                                facesfront,
                                vertindex,
                            })
                            .collect(),
                        frames,
                        trailing,
                    }
                },
            )
    })
}

proptest! {
    // spec MDL-6.1: any model the loader takes, written and read, is the model, and written
    // again is the same bytes
    #[test]
    fn a_written_model_reads_as_the_model(model in model()) {
        let bytes = model.to_bytes().unwrap();
        let read = Model::read(&bytes).unwrap();
        // Floats may be NaN, which equal nothing: the bytes are what is compared.
        prop_assert_eq!(read.to_bytes().unwrap(), bytes);
        prop_assert_eq!(&read.skins, &model.skins);
        prop_assert_eq!(&read.stverts, &model.stverts);
        prop_assert_eq!(&read.triangles, &model.triangles);
        prop_assert_eq!(&read.trailing, &model.trailing);
        prop_assert_eq!(read.frames.len(), model.frames.len());
    }
}
