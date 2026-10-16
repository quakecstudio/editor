// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out alias models as the spec's vectors do. The default
//! is the `base` vector of `docs/spec/vectors/mdl/`, byte for byte.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

/// A frame: bounding box, name and vertices, each x, y, z and normal index.
#[derive(Clone, Debug)]
pub(crate) struct FrameSpec {
    pub(crate) bboxmin: [u8; 4],
    pub(crate) bboxmax: [u8; 4],
    pub(crate) name: Vec<u8>,
    pub(crate) verts: Vec<[u8; 4]>,
}

/// A skin entry: a type and, for a group, the intervals; then each skin's fill byte.
#[derive(Clone, Debug)]
pub(crate) enum SkinSpec {
    Single(u8),
    Group(i32, Vec<f32>, Vec<u8>),
}

/// A frame entry.
#[derive(Clone, Debug)]
pub(crate) enum EntrySpec {
    Single(FrameSpec),
    /// A frame type, the group's count, bounding box and intervals, and its frames.
    Group(i32, i32, [u8; 4], [u8; 4], Vec<f32>, Vec<FrameSpec>),
}

/// An alias model to build. The default is the `base` vector.
#[derive(Clone, Debug)]
pub(crate) struct MdlSpec {
    pub(crate) magic: [u8; 4],
    pub(crate) version: i32,
    pub(crate) numskins: Option<i32>,
    pub(crate) skinwidth: i32,
    pub(crate) skinheight: i32,
    pub(crate) numverts: Option<i32>,
    pub(crate) numtris: Option<i32>,
    pub(crate) numframes: Option<i32>,
    pub(crate) flags: i32,
    pub(crate) skins: Vec<SkinSpec>,
    /// The skin pixels of a single skin of the base: 1 to 8.
    pub(crate) counting_skin: bool,
    pub(crate) stverts: Vec<[i32; 3]>,
    pub(crate) triangles: Vec<[i32; 4]>,
    pub(crate) frames: Vec<EntrySpec>,
}

pub(crate) fn frame(
    name: &[u8],
    bboxmin: [u8; 4],
    bboxmax: [u8; 4],
    verts: Vec<[u8; 4]>,
) -> FrameSpec {
    FrameSpec {
        bboxmin,
        bboxmax,
        name: name.to_vec(),
        verts,
    }
}

impl Default for MdlSpec {
    fn default() -> Self {
        Self {
            magic: *b"IDPO",
            version: 6,
            numskins: None,
            skinwidth: 4,
            skinheight: 2,
            numverts: None,
            numtris: None,
            numframes: None,
            flags: 8,
            skins: vec![SkinSpec::Single(0)],
            counting_skin: true,
            stverts: vec![[0, 0, 0], [0x20, 1, 2], [0, 3, 1]],
            triangles: vec![[1, 0, 1, 2]],
            frames: vec![
                EntrySpec::Single(frame(
                    b"stand1",
                    [0, 0, 0, 0],
                    [16, 16, 16, 0],
                    vec![[0, 0, 0, 0], [1, 2, 3, 1], [2, 4, 6, 2]],
                )),
                EntrySpec::Group(
                    1,
                    2,
                    [1, 1, 1, 0],
                    [9, 9, 9, 0],
                    vec![0.1, 0.2],
                    vec![
                        frame(b"run1", [0, 0, 0, 0], [9, 9, 9, 0], vec![[5, 0, 0, 7]; 3]),
                        frame(b"run2", [1, 1, 1, 0], [9, 9, 9, 0], vec![[6, 0, 0, 7]; 3]),
                    ],
                ),
            ],
        }
    }
}

fn put(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn count(len: usize) -> i32 {
    i32::try_from(len).unwrap()
}

fn put_frame(out: &mut Vec<u8>, frame: &FrameSpec) {
    out.extend_from_slice(&frame.bboxmin);
    out.extend_from_slice(&frame.bboxmax);
    let mut name = [0_u8; 16];
    name[..frame.name.len()].copy_from_slice(&frame.name);
    out.extend_from_slice(&name);
    for vert in &frame.verts {
        out.extend_from_slice(vert);
    }
}

impl MdlSpec {
    /// The file's bytes.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut out = self.magic.to_vec();
        put(&mut out, self.version);
        for value in [1.0, 1.0, 1.0, -8.0, -8.0, -8.0, 10.0, 0.0, 0.0, 22.0] {
            put_f32(&mut out, value);
        }
        put(
            &mut out,
            self.numskins.unwrap_or_else(|| count(self.skins.len())),
        );
        put(&mut out, self.skinwidth);
        put(&mut out, self.skinheight);
        put(
            &mut out,
            self.numverts.unwrap_or_else(|| count(self.stverts.len())),
        );
        put(
            &mut out,
            self.numtris.unwrap_or_else(|| count(self.triangles.len())),
        );
        put(
            &mut out,
            self.numframes.unwrap_or_else(|| count(self.frames.len())),
        );
        put(&mut out, 1);
        put(&mut out, self.flags);
        put_f32(&mut out, 11.0);
        let size =
            usize::try_from(self.skinwidth.max(0).saturating_mul(self.skinheight.max(0))).unwrap();
        for skin in &self.skins {
            match skin {
                SkinSpec::Single(fill) => {
                    put(&mut out, 0);
                    if self.counting_skin {
                        out.extend((1..=size).map(|n| u8::try_from(n % 256).unwrap()));
                    } else {
                        out.extend(core::iter::repeat_n(*fill, size));
                    }
                }
                SkinSpec::Group(skin_type, intervals, fills) => {
                    put(&mut out, *skin_type);
                    put(&mut out, count(fills.len()));
                    for interval in intervals {
                        put_f32(&mut out, *interval);
                    }
                    for fill in fills {
                        out.extend(core::iter::repeat_n(*fill, size));
                    }
                }
            }
        }
        for stvert in &self.stverts {
            for value in stvert {
                put(&mut out, *value);
            }
        }
        for triangle in &self.triangles {
            for value in triangle {
                put(&mut out, *value);
            }
        }
        for entry in &self.frames {
            match entry {
                EntrySpec::Single(frame) => {
                    put(&mut out, 0);
                    put_frame(&mut out, frame);
                }
                EntrySpec::Group(frame_type, stored, bboxmin, bboxmax, intervals, frames) => {
                    put(&mut out, *frame_type);
                    put(&mut out, *stored);
                    out.extend_from_slice(bboxmin);
                    out.extend_from_slice(bboxmax);
                    for interval in intervals {
                        put_f32(&mut out, *interval);
                    }
                    for frame in frames {
                        put_frame(&mut out, frame);
                    }
                }
            }
        }
        out
    }
}

/// The bytes of an `xxd` dump.
pub(crate) fn from_xxd(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        for group in rest.get(..39).unwrap_or(rest).split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                out.push(u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap());
            }
        }
    }
    out
}
