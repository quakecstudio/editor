// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; a frame's size is small"
)]

use qs_mdl::spr::{
    SPR_FACING_UPRIGHT, SPR_MAGIC, SPR_ORIENTED, SPR_SINGLE, SPR_VERSION, SPR_VP_PARALLEL,
    SPR_VP_PARALLEL_ORIENTED, SPR_VP_PARALLEL_UPRIGHT,
};

use super::{Eye, SPR_ANGLED, SpriteEntity, SpriteError, SpriteModel, SpriteVertex};
use crate::mesh::Palette;

/// One frame: its origin, size and every pixel's index.
struct FrameSpec {
    origin: [i32; 2],
    size: [i32; 2],
    pixel: u8,
}

enum EntrySpec {
    Single(FrameSpec),
    /// A frame type, the intervals and the frames.
    Group(i32, Vec<f32>, Vec<FrameSpec>),
}

struct SprSpec {
    sprite_type: i32,
    entries: Vec<EntrySpec>,
}

impl Default for SprSpec {
    /// One 2 by 3 frame whose origin puts it 16 left and 8 up, as SPR-2's first vector.
    fn default() -> Self {
        Self {
            sprite_type: SPR_VP_PARALLEL,
            entries: vec![EntrySpec::Single(FrameSpec {
                origin: [-16, 8],
                size: [2, 3],
                pixel: 7,
            })],
        }
    }
}

fn frame_bytes(frame: &FrameSpec, out: &mut Vec<u8>) {
    for v in [
        frame.origin[0],
        frame.origin[1],
        frame.size[0],
        frame.size[1],
    ] {
        out.extend(v.to_le_bytes());
    }
    let len = usize::try_from(frame.size[0] * frame.size[1]).unwrap();
    out.extend(core::iter::repeat_n(frame.pixel, len));
}

fn spr(spec: &SprSpec) -> Vec<u8> {
    let mut out = SPR_MAGIC.to_vec();
    out.extend(SPR_VERSION.to_le_bytes());
    out.extend(spec.sprite_type.to_le_bytes());
    out.extend(8.5_f32.to_le_bytes());
    for v in [33, 16, i32::try_from(spec.entries.len()).unwrap()] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0.25_f32.to_le_bytes());
    out.extend(0_i32.to_le_bytes());
    for entry in &spec.entries {
        match entry {
            EntrySpec::Single(frame) => {
                out.extend(SPR_SINGLE.to_le_bytes());
                frame_bytes(frame, &mut out);
            }
            EntrySpec::Group(kind, intervals, frames) => {
                out.extend(kind.to_le_bytes());
                out.extend(i32::try_from(frames.len()).unwrap().to_le_bytes());
                for interval in intervals {
                    out.extend(interval.to_le_bytes());
                }
                for frame in frames {
                    frame_bytes(frame, &mut out);
                }
            }
        }
    }
    out
}

/// A palette whose colour `i` is `(i, 1, 0)`.
fn palette() -> Palette {
    let bytes: Vec<u8> = (0..=255_u8).flat_map(|i| [i, 1, 0]).collect();
    Palette::read(&bytes).unwrap()
}

fn read(spec: &SprSpec) -> SpriteModel {
    SpriteModel::read("progs/s_test.spr", &spr(spec), &palette()).unwrap()
}

/// An eye at the origin looking down +x, right -y, up +z.
fn eye() -> Eye {
    Eye {
        origin: [0.0; 3],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, -1.0, 0.0],
        up: [0.0, 0.0, 1.0],
    }
}

fn entity(origin: [f32; 3]) -> SpriteEntity {
    SpriteEntity {
        origin,
        angles: [0.0; 3],
        frame: 0,
        syncbase: 0.0,
    }
}

fn draw(
    model: &SpriteModel,
    entity: &SpriteEntity,
    time: f32,
) -> (Vec<SpriteVertex>, Option<usize>) {
    let mut out = Vec::new();
    let image = model.draw(entity, time, &eye(), &mut out);
    (out, image)
}

fn group(kind: i32, intervals: &[f32]) -> SprSpec {
    SprSpec {
        sprite_type: SPR_VP_PARALLEL,
        entries: vec![EntrySpec::Group(
            kind,
            intervals.to_vec(),
            (0..intervals.len())
                .map(|i| FrameSpec {
                    origin: [0, 0],
                    size: [1, 1],
                    pixel: u8::try_from(i).unwrap(),
                })
                .collect(),
        )],
    }
}

#[test]
fn a_file_that_is_not_a_sprite_is_reported() {
    let error = SpriteModel::read("x.spr", b"IDPO", &palette()).unwrap_err();
    assert!(
        matches!(error, SpriteError::Spr(ref m) if m.starts_with("x.spr: ")),
        "{error}"
    );
}

// spec REND-11.2: a frame's texels through the palette, index 255 transparent
#[test]
fn frames_are_images_with_index_255_transparent() {
    let model = read(&SprSpec {
        entries: vec![
            EntrySpec::Single(FrameSpec {
                origin: [0, 0],
                size: [2, 1],
                pixel: 9,
            }),
            EntrySpec::Single(FrameSpec {
                origin: [0, 0],
                size: [1, 1],
                pixel: 255,
            }),
        ],
        ..SprSpec::default()
    });
    let images = model.images();
    assert_eq!((images[0].width, images[0].height), (2, 1));
    assert_eq!(images[0].rgba, [9, 1, 0, 255, 9, 1, 0, 255]);
    assert_eq!(images[1].rgba, [0, 0, 0, 0]);
}

// spec REND-11.1: a frame out of the sprite's range is drawn as frame 0; a single frame is
// itself
#[test]
fn a_missing_frame_draws_frame_0() {
    let model = read(&SprSpec {
        entries: vec![
            EntrySpec::Single(FrameSpec {
                origin: [0, 0],
                size: [1, 1],
                pixel: 0,
            }),
            EntrySpec::Single(FrameSpec {
                origin: [0, 0],
                size: [1, 1],
                pixel: 1,
            }),
        ],
        ..SprSpec::default()
    });
    let at = |frame| {
        let entity = SpriteEntity {
            frame,
            ..entity([10.0, 0.0, 0.0])
        };
        draw(&model, &entity, 0.0).1
    };
    assert_eq!(at(1), Some(1));
    assert_eq!(at(2), Some(0));
    assert_eq!(at(-1), Some(0));
}

// spec REND-11.1: a group picks the first subframe whose interval exceeds `time + syncbase`
// modulo the last interval, the last if none does
#[test]
fn a_group_picks_its_frame_by_time_and_syncbase() {
    let model = read(&group(1, &[0.1, 0.3, 0.6]));
    let at = |time: f32, syncbase: f32| {
        let entity = SpriteEntity {
            syncbase,
            ..entity([10.0, 0.0, 0.0])
        };
        draw(&model, &entity, time).1.unwrap()
    };
    assert_eq!(at(0.0, 0.0), 0);
    assert_eq!(at(0.2, 0.0), 1);
    assert_eq!(at(0.4, 0.0), 2);
    assert_eq!(at(0.59, 0.0), 2);
    // Wrapped by the full interval, and shifted by the sync base.
    assert_eq!(at(0.8, 0.0), 1);
    assert_eq!(at(0.0, 0.2), 1);
    // `targettime` at an interval exactly is not below it: the next frame.
    assert_eq!(at(0.1, 0.0), 1);
}

// spec REND-11.1: an angled group picks subframe `dir & 7` from the sprite's forward against
// the view's forward and right
#[test]
fn an_angled_group_picks_its_frame_by_direction() {
    let model = read(&group(SPR_ANGLED, &[1.0; 8]));
    let facing = |yaw: f32| {
        let entity = SpriteEntity {
            angles: [0.0, yaw, 0.0],
            ..entity([10.0, 0.0, 0.0])
        };
        draw(&model, &entity, 0.0).1.unwrap()
    };
    // Facing the eye's forward: `atan2 (0, 1) = 0`, `(0 + 1.125 pi) * 4 / pi = 4.5`: 4.
    assert_eq!(facing(0.0), 4);
    // Facing back at the eye: `atan2 (0, -1) = pi`, `2.125 * 4 = 8.5`: 8 & 7 = 0.
    assert_eq!(facing(180.0), 0);
    // Facing the eye's left (yaw 90 is -right): `atan2 (-1, 0) = -pi/2`, `0.625 * 4 = 2.5`.
    assert_eq!(facing(90.0), 2);
    assert_eq!(facing(270.0), 6);
}

// spec REND-11.2: the corners are the origin plus the frame's up, down, left and right along
// the type's axes, with the texture's corners, as two triangles of a fan
#[test]
fn the_quad_is_the_frames_extent_along_the_axes() {
    let model = read(&SprSpec::default());
    let (out, image) = draw(&model, &entity([10.0, 0.0, 0.0]), 0.0);
    assert_eq!(image, Some(0));
    // `SPR_VP_PARALLEL`: the view's up and right. down = 8 - 3 = 5, left = -16, right = -14.
    let corner = |up: f32, right: f32| [10.0, -right, up];
    let positions: Vec<[f32; 3]> = out.iter().map(|v| v.position).collect();
    assert_eq!(
        positions,
        [
            corner(5.0, -16.0),
            corner(8.0, -16.0),
            corner(8.0, -14.0),
            corner(5.0, -16.0),
            corner(8.0, -14.0),
            corner(5.0, -14.0),
        ]
    );
    let textures: Vec<[f32; 2]> = out.iter().map(|v| v.texture).collect();
    assert_eq!(
        textures,
        [
            [0.0, 1.0],
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 0.0],
            [1.0, 1.0]
        ]
    );
}

// spec REND-11.2: each type's up and right axes; an unknown type is not drawn
#[test]
fn each_type_faces_the_eye_its_own_way() {
    let axes = |sprite_type: i32, origin: [f32; 3], angles: [f32; 3]| {
        let model = read(&SprSpec {
            sprite_type,
            ..SprSpec::default()
        });
        let entity = SpriteEntity {
            angles,
            ..entity(origin)
        };
        model.axes(&entity, &eye())
    };
    let eye_up = [0.0, 0.0, 1.0];
    // Upright, parallel to the view plane: world up, and `forward x up` normalised.
    assert_eq!(
        axes(SPR_VP_PARALLEL_UPRIGHT, [10.0, 0.0, 0.0], [0.0; 3]),
        Some((eye_up, [0.0, -1.0, 0.0]))
    );
    // Facing the eye: forward from the eye to the sprite, flattened; right `(f.y, -f.x, 0)`.
    assert_eq!(
        axes(SPR_FACING_UPRIGHT, [0.0, 10.0, 5.0], [0.0; 3]),
        Some((eye_up, [1.0, 0.0, 0.0]))
    );
    assert_eq!(
        axes(SPR_VP_PARALLEL, [10.0, 0.0, 0.0], [0.0; 3]),
        Some(([0.0, 0.0, 1.0], [0.0, -1.0, 0.0]))
    );
    // Oriented: the sprite's own angles, here yaw 90: forward is +y and right is +x.
    let (up, right) = axes(SPR_ORIENTED, [10.0, 0.0, 0.0], [0.0, 90.0, 0.0]).unwrap();
    assert!(
        (right[0] - 1.0).abs() < 1e-6 && right[1].abs() < 1e-6,
        "{right:?}"
    );
    assert_eq!(up, [0.0, 0.0, 1.0]);
    // Parallel, turned by the roll: at 90 degrees right becomes up and up becomes -right.
    let (up, right) = axes(SPR_VP_PARALLEL_ORIENTED, [10.0, 0.0, 0.0], [0.0, 0.0, 90.0]).unwrap();
    assert!((right[2] - 1.0).abs() < 1e-6, "{right:?}");
    assert!((up[1] - 1.0).abs() < 1e-6, "{up:?}");
    assert_eq!(axes(9, [10.0, 0.0, 0.0], [0.0; 3]), None);
    let unknown = read(&SprSpec {
        sprite_type: 9,
        ..SprSpec::default()
    });
    assert_eq!(
        draw(&unknown, &entity([10.0, 0.0, 0.0]), 0.0),
        (Vec::new(), None)
    );
}

// spec REND-11.2: only an oriented sprite is a decal with the polygon offset
#[test]
fn only_oriented_sprites_are_decals() {
    assert!(
        read(&SprSpec {
            sprite_type: SPR_ORIENTED,
            ..SprSpec::default()
        })
        .is_oriented()
    );
    assert!(!read(&SprSpec::default()).is_oriented());
}
