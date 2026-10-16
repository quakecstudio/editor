// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use proptest::prelude::*;

use super::fixtures::{EntrySpec, FrameSpec, SprSpec, frame};
use crate::spr::{
    SPR_FACING_UPRIGHT, SPR_FRAME_HEADER_SIZE, SPR_GROUP, SPR_HEADER_SIZE, SPR_MAGIC, SPR_ORIENTED,
    SPR_SINGLE, SPR_ST_RAND, SPR_ST_SYNC, SPR_VERSION, SPR_VP_PARALLEL, SPR_VP_PARALLEL_ORIENTED,
    SPR_VP_PARALLEL_UPRIGHT, SprError, Sprite, SpriteFrame, SpriteFrameEntry, SpriteGroup,
};

fn read(spec: &SprSpec) -> Result<Sprite<'static>, SprError> {
    let bytes: &'static [u8] = Vec::leak(spec.build());
    Sprite::read(bytes)
}

fn single<'a>(sprite: &'a Sprite<'_>, index: usize) -> &'a SpriteFrame<'a> {
    match &sprite.frames[index] {
        SpriteFrameEntry::Single(frame) => frame,
        SpriteFrameEntry::Group(group) => panic!("entry {index} is a group: {group:?}"),
    }
}

fn group<'a>(sprite: &'a Sprite<'_>, index: usize) -> &'a SpriteGroup<'a> {
    match &sprite.frames[index] {
        SpriteFrameEntry::Group(group) => group,
        SpriteFrameEntry::Single(frame) => panic!("entry {index} is a frame: {frame:?}"),
    }
}

fn two_frame_group(frame_type: i32, intervals: Vec<f32>) -> SprSpec {
    SprSpec {
        sprite_type: 9,
        entries: vec![EntrySpec::Group(
            frame_type,
            intervals,
            vec![frame(1, 1), frame(1, 1)],
        )],
        ..SprSpec::default()
    }
}

// spec SPR-1.1, SPR-1.7, SPR-2.2, SPR-2.3: the first SPR-2 vector.
#[test]
fn reads_the_default_vector() {
    assert_eq!(SPR_HEADER_SIZE, 36);
    assert_eq!(SPR_FRAME_HEADER_SIZE, 16);
    let sprite = read(&SprSpec::default()).unwrap();
    assert_eq!(sprite.header.sprite_type, SPR_VP_PARALLEL);
    assert_eq!(sprite.header.bounding_radius.to_bits(), 8.5_f32.to_bits());
    assert_eq!((sprite.header.width, sprite.header.height), (33, 16));
    assert_eq!(sprite.header.numframes, 1);
    assert_eq!(sprite.header.beam_length.to_bits(), 0.25_f32.to_bits());
    assert_eq!(sprite.header.sync_type, SPR_ST_RAND);
    assert_eq!(sprite.bounds(), ([-16, -16, -8], [16, 16, 8]));
    let frame = single(&sprite, 0);
    assert_eq!(frame.origin, [-16, 8]);
    assert_eq!((frame.width, frame.height), (2, 3));
    assert_eq!(frame.pixels, &[1, 2, 3, 4, 5, 6]);
    assert_eq!(
        (frame.up(), frame.down(), frame.left(), frame.right()),
        (8, 5, -16, -14)
    );
}

// spec SPR-1.2: only `IDSP` is a sprite.
#[test]
fn wrong_id_fails() {
    assert_eq!(SPR_MAGIC, [0x49, 0x44, 0x53, 0x50]);
    for magic in [*b"IDPO", *b"idsp", *b"IDS2", [0; 4]] {
        let spec = SprSpec {
            magic,
            ..SprSpec::default()
        };
        assert_eq!(read(&spec), Err(SprError::WrongId));
    }
    for len in 0..4 {
        assert_eq!(Sprite::read(&b"IDSP"[..len]), Err(SprError::WrongId));
    }
}

// spec SPR-1.3: the version must be 1.
#[test]
fn wrong_version_fails() {
    assert_eq!(SPR_VERSION, 1);
    for version in [0, 2, 32, -1] {
        let spec = SprSpec {
            version,
            ..SprSpec::default()
        };
        assert_eq!(read(&spec), Err(SprError::WrongVersion { version }));
    }
    assert_eq!(
        format!("s.spr {}", SprError::WrongVersion { version: 2 }),
        "s.spr has wrong version number (2 should be 1)"
    );
}

// spec SPR-1.4: at least one frame entry.
#[test]
fn frame_count_must_be_positive() {
    for numframes in [0, -1, i32::MIN] {
        let spec = SprSpec {
            numframes: Some(numframes),
            ..SprSpec::default()
        };
        assert_eq!(read(&spec), Err(SprError::InvalidFrameCount { numframes }));
    }
    assert_eq!(
        SprError::InvalidFrameCount { numframes: 0 }.to_string(),
        "Mod_LoadSpriteModel: Invalid # of frames: 0"
    );
}

// spec SPR-1.5: type, sizes and sync type are not checked.
#[test]
fn header_fields_are_not_checked() {
    assert_eq!(
        [
            SPR_VP_PARALLEL_UPRIGHT,
            SPR_FACING_UPRIGHT,
            SPR_VP_PARALLEL,
            SPR_ORIENTED,
            SPR_VP_PARALLEL_ORIENTED
        ],
        [0, 1, 2, 3, 4]
    );
    assert_eq!((SPR_ST_SYNC, SPR_ST_RAND), (0, 1));
    let spec = SprSpec {
        sprite_type: 9,
        width: -5,
        height: 1,
        ..SprSpec::default()
    };
    let sprite = read(&spec).unwrap();
    assert_eq!(sprite.header.sprite_type, 9);
    // The 2 by 3 frame is larger than the header's -5 by 1.
    assert_eq!(single(&sprite, 0).width, 2);
}

// spec SPR-1.6: a file shorter than the header fails.
#[test]
fn short_header_fails() {
    let bytes = SprSpec::default().build();
    for len in 4..SPR_HEADER_SIZE {
        assert_eq!(
            Sprite::read(&bytes[..len]),
            Err(SprError::Truncated { offset: 0 })
        );
    }
}

// spec SPR-1.7: bounds use C integer division and x86 wrapping.
#[test]
fn bounds_truncate_toward_zero() {
    for (width, height, mins, maxs) in [
        (33, 16, [-16, -16, -8], [16, 16, 8]),
        (1, 3, [0, 0, -1], [0, 0, 1]),
        (-3, -4, [1, 1, 2], [-1, -1, -2]),
        (
            i32::MIN,
            i32::MAX,
            [-1_073_741_824, -1_073_741_824, -1_073_741_823],
            [-1_073_741_824, -1_073_741_824, 1_073_741_823],
        ),
    ] {
        let spec = SprSpec {
            width,
            height,
            ..SprSpec::default()
        };
        assert_eq!(
            read(&spec).unwrap().bounds(),
            (mins, maxs),
            "{width} {height}"
        );
    }
}

// spec SPR-2.1, SPR-2.5: any non-zero frame type is a group.
#[test]
fn any_non_zero_type_is_a_group() {
    assert_eq!((SPR_SINGLE, SPR_GROUP), (0, 1));
    for frame_type in [SPR_GROUP, 7, -1] {
        let sprite = read(&two_frame_group(frame_type, vec![0.1, 0.2])).unwrap();
        let group = group(&sprite, 0);
        assert_eq!(group.frame_type, frame_type);
        assert_eq!(group.intervals, [0.1, 0.2]);
        assert_eq!(group.frames.len(), 2);
        assert_eq!(group.frames[1].pixels, &[1]);
    }
}

// spec SPR-2.1, SPR-2.2: entries follow one another, singles and groups mixed.
#[test]
fn entries_follow_one_another() {
    let spec = SprSpec {
        entries: vec![
            EntrySpec::Single(frame(2, 2)),
            EntrySpec::Group(SPR_GROUP, vec![1.0], vec![frame(3, 1)]),
            EntrySpec::Single(FrameSpec {
                origin: [5, 6],
                width: 1,
                height: 2,
            }),
        ],
        ..SprSpec::default()
    };
    let sprite = read(&spec).unwrap();
    assert_eq!(sprite.frames.len(), 3);
    assert_eq!(single(&sprite, 0).pixels, &[1, 2, 3, 4]);
    assert_eq!(group(&sprite, 1).frames[0].pixels, &[1, 2, 3]);
    assert_eq!(single(&sprite, 2).origin, [5, 6]);
    assert_eq!(single(&sprite, 2).pixels, &[1, 2]);
}

// spec SPR-2.2: an empty frame has no pixels.
#[test]
fn empty_frame_loads() {
    let spec = SprSpec {
        entries: vec![
            EntrySpec::Single(frame(0, 5)),
            EntrySpec::Single(frame(4, 0)),
        ],
        ..SprSpec::default()
    };
    let sprite = read(&spec).unwrap();
    assert!(single(&sprite, 0).pixels.is_empty());
    assert!(single(&sprite, 1).pixels.is_empty());
}

// spec SPR-2.3: extents wrap as on x86.
#[test]
fn extents_wrap() {
    let frame = SpriteFrame {
        origin: [i32::MAX, i32::MIN],
        width: 1,
        height: 1,
        pixels: &[0],
    };
    assert_eq!(frame.down(), i32::MAX);
    assert_eq!(frame.right(), i32::MIN);
    assert_eq!((frame.up(), frame.left()), (i32::MIN, i32::MAX));
}

// spec SPR-2.4: negative or overflowing sizes, and pixels past the end, fail.
#[test]
fn bad_frame_sizes_fail() {
    for (width, height) in [
        (-1, 2),
        (2, -1),
        (-2, -2),
        (-1, 0),
        (0, -3),
        (65_536, 65_536),
    ] {
        let spec = SprSpec {
            entries: vec![EntrySpec::Single(frame(width, height))],
            ..SprSpec::default()
        };
        assert_eq!(
            read(&spec),
            Err(SprError::BadFrameSize { width, height }),
            "{width} {height}"
        );
    }
    let bytes = SprSpec::default().build();
    for len in SPR_HEADER_SIZE..bytes.len() {
        let want = if len < SPR_HEADER_SIZE + 4 {
            SprError::Truncated { offset: 36 }
        } else {
            SprError::Truncated { offset: 40 }
        };
        assert_eq!(Sprite::read(&bytes[..len]), Err(want), "length {len}");
    }
    assert_eq!(
        SprError::BadFrameSize {
            width: -1,
            height: 2
        }
        .to_string(),
        "sprite frame of -1 by 2 pixels is invalid"
    );
}

// spec SPR-2.6: intervals must be above 0; NaN passes.
#[test]
fn intervals_must_be_positive() {
    for bad in [0.0, -0.0, -1.0, f32::NEG_INFINITY] {
        assert_eq!(
            read(&two_frame_group(SPR_GROUP, vec![0.1, bad])),
            Err(SprError::BadInterval { interval: bad })
        );
    }
    let sprite = read(&two_frame_group(SPR_GROUP, vec![f32::NAN, f32::INFINITY])).unwrap();
    assert!(group(&sprite, 0).intervals[0].is_nan());
    assert_eq!(
        SprError::BadInterval { interval: 0.0 }.to_string(),
        "Mod_LoadSpriteGroup: interval<=0"
    );
}

// spec SPR-2.7: a group needs at least one frame.
#[test]
fn empty_group_fails() {
    for numframes in [0_i32, -1] {
        let mut bytes = SprSpec {
            entries: vec![],
            numframes: Some(1),
            ..SprSpec::default()
        }
        .build();
        bytes.extend_from_slice(&SPR_GROUP.to_le_bytes());
        bytes.extend_from_slice(&numframes.to_le_bytes());
        assert_eq!(
            Sprite::read(&bytes),
            Err(SprError::InvalidGroupCount { numframes })
        );
    }
    assert_eq!(
        SprError::InvalidGroupCount { numframes: 0 }.to_string(),
        "Mod_LoadSpriteGroup: Invalid # of frames: 0"
    );
}

// spec SPR-2.8: a group's intervals or frames past the end fail.
#[test]
fn truncated_group_fails() {
    let bytes = two_frame_group(SPR_GROUP, vec![0.1, 0.2]).build();
    // Type at 36, count at 40, intervals at 44 and 48, frames at 52 and 69.
    for (len, offset) in [(43, 40), (47, 44), (51, 48), (60, 52), (85, 69)] {
        assert_eq!(
            Sprite::read(&bytes[..len]),
            Err(SprError::Truncated { offset }),
            "length {len}"
        );
    }
    assert_eq!(bytes.len(), 86);
    assert!(Sprite::read(&bytes).is_ok());
    assert_eq!(
        SprError::Truncated { offset: 52 }.to_string(),
        "sprite data at offset 52 runs past the end of the file"
    );
}

// spec SPR-2.9: bytes after the last entry are ignored.
#[test]
fn trailing_bytes_are_ignored() {
    let spec = SprSpec {
        entries: vec![
            EntrySpec::Single(frame(1, 1)),
            EntrySpec::Single(frame(1, 1)),
        ],
        trailer: vec![0xAA; 5],
        ..SprSpec::default()
    };
    assert_eq!(read(&spec).unwrap().frames.len(), 2);
    // A count smaller than the entries reads only that many.
    let spec = SprSpec {
        numframes: Some(1),
        ..spec
    };
    assert_eq!(read(&spec).unwrap().frames.len(), 1);
}

proptest! {
    // spec SPR-1.6, SPR-2.4, SPR-2.8: arbitrary bytes never panic the reader.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _sprite = Sprite::read(&bytes);
    }

    // spec SPR-2.2, SPR-2.4: a valid header followed by arbitrary bytes never panics, and
    // every frame's pixels match its size.
    #[test]
    fn arbitrary_frames_never_panic(tail in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut bytes = SprSpec::default().build();
        bytes.truncate(SPR_HEADER_SIZE);
        bytes.extend_from_slice(&tail);
        if let Ok(sprite) = Sprite::read(&bytes) {
            let _bounds = sprite.bounds();
            for entry in &sprite.frames {
                let frames = match entry {
                    SpriteFrameEntry::Single(frame) => vec![*frame],
                    SpriteFrameEntry::Group(group) => group.frames.clone(),
                };
                for frame in frames {
                    let size = usize::try_from(frame.width).unwrap()
                        * usize::try_from(frame.height).unwrap();
                    prop_assert_eq!(frame.pixels.len(), size);
                }
            }
        }
    }
}
