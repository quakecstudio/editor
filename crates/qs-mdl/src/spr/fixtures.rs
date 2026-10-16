// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out sprites as the spec's vectors do.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use crate::spr::{SPR_MAGIC, SPR_SINGLE, SPR_VERSION, SPR_VP_PARALLEL};

/// One frame of a [`SprSpec`]: its origin and size, and pixels counting up from 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSpec {
    pub(crate) origin: [i32; 2],
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// A frame of `width` by `height` pixels at origin (0, 0).
pub(crate) fn frame(width: i32, height: i32) -> FrameSpec {
    FrameSpec {
        origin: [0, 0],
        width,
        height,
    }
}

impl FrameSpec {
    fn build(self, out: &mut Vec<u8>) {
        for value in [self.origin[0], self.origin[1], self.width, self.height] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        // A size that overflows, as SPR-2.4 tests, gets no pixels.
        let len = self
            .width
            .max(0)
            .checked_mul(self.height.max(0))
            .map_or(0, |len| usize::try_from(len).unwrap());
        out.extend((1..=len).map(|n| u8::try_from(n % 256).unwrap()));
    }
}

/// One frame entry of a [`SprSpec`].
#[derive(Clone, Debug)]
pub(crate) enum EntrySpec {
    Single(FrameSpec),
    /// A frame type, then the group's intervals and frames, one interval per frame.
    Group(i32, Vec<f32>, Vec<FrameSpec>),
}

/// A sprite to build. The default is the first row of the SPR-2 vectors.
#[derive(Clone, Debug)]
pub(crate) struct SprSpec {
    pub(crate) magic: [u8; 4],
    pub(crate) version: i32,
    pub(crate) sprite_type: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// The stored `numframes`; the number of entries unless set.
    pub(crate) numframes: Option<i32>,
    pub(crate) entries: Vec<EntrySpec>,
    pub(crate) trailer: Vec<u8>,
}

impl Default for SprSpec {
    fn default() -> Self {
        Self {
            magic: SPR_MAGIC,
            version: SPR_VERSION,
            sprite_type: SPR_VP_PARALLEL,
            width: 33,
            height: 16,
            numframes: None,
            entries: vec![EntrySpec::Single(FrameSpec {
                origin: [-16, 8],
                width: 2,
                height: 3,
            })],
            trailer: Vec::new(),
        }
    }
}

impl SprSpec {
    /// The file's bytes: the header, each entry, then the trailer.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut out = self.magic.to_vec();
        let numframes = self
            .numframes
            .unwrap_or_else(|| i32::try_from(self.entries.len()).unwrap());
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.sprite_type.to_le_bytes());
        out.extend_from_slice(&8.5_f32.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&numframes.to_le_bytes());
        out.extend_from_slice(&0.25_f32.to_le_bytes());
        out.extend_from_slice(&1_i32.to_le_bytes());
        for entry in &self.entries {
            match entry {
                EntrySpec::Single(spec) => {
                    out.extend_from_slice(&SPR_SINGLE.to_le_bytes());
                    spec.build(&mut out);
                }
                EntrySpec::Group(frame_type, intervals, frames) => {
                    out.extend_from_slice(&frame_type.to_le_bytes());
                    let count = i32::try_from(frames.len()).unwrap();
                    out.extend_from_slice(&count.to_le_bytes());
                    for interval in intervals {
                        out.extend_from_slice(&interval.to_le_bytes());
                    }
                    for spec in frames {
                        spec.build(&mut out);
                    }
                }
            }
        }
        out.extend_from_slice(&self.trailer);
        out
    }
}
