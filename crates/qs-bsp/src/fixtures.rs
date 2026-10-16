// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out BSPs as the spec's vectors do. The default is the
//! `base` vector of `docs/spec/vectors/bsp/`, byte for byte.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::integer_division,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use crate::{BSP_HEADER_LUMPS, BSP_HEADER_SIZE, BspVersion};

/// A texture: name, width, height and the value of every pixel.
#[derive(Clone, Debug)]
pub(crate) struct TextureSpec {
    pub(crate) name: &'static [u8],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel: u8,
}

pub(crate) fn texture(name: &'static [u8]) -> Option<TextureSpec> {
    Some(TextureSpec {
        name,
        width: 16,
        height: 16,
        pixel: 7,
    })
}

#[derive(Clone, Debug)]
pub(crate) struct NodeSpec {
    pub(crate) planenum: i32,
    pub(crate) children: [i32; 2],
    pub(crate) mins: [i16; 3],
    pub(crate) maxs: [i16; 3],
    pub(crate) firstface: u32,
    pub(crate) numfaces: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct TexinfoSpec {
    pub(crate) vecs: [[f32; 4]; 2],
    pub(crate) miptex: i32,
    pub(crate) flags: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct FaceSpec {
    pub(crate) planenum: i32,
    pub(crate) side: i32,
    pub(crate) firstedge: i32,
    pub(crate) numedges: i32,
    pub(crate) texinfo: i32,
    pub(crate) styles: [u8; 4],
    pub(crate) lightofs: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct LeafSpec {
    pub(crate) contents: i32,
    pub(crate) visofs: i32,
    pub(crate) mins: [i16; 3],
    pub(crate) maxs: [i16; 3],
    pub(crate) firstmarksurface: u32,
    pub(crate) nummarksurfaces: u32,
    pub(crate) ambient: [u8; 4],
}

#[derive(Clone, Debug)]
pub(crate) struct ModelSpec {
    pub(crate) mins: [f32; 3],
    pub(crate) maxs: [f32; 3],
    pub(crate) headnode: [i32; 4],
    pub(crate) visleafs: i32,
    pub(crate) firstface: i32,
    pub(crate) numfaces: i32,
}

/// A BSP to build. The default is the `base` vector.
#[derive(Clone, Debug)]
pub(crate) struct BspSpec {
    pub(crate) version: BspVersion,
    /// Overrides the version field.
    pub(crate) raw_version: Option<i32>,
    pub(crate) entities: Vec<u8>,
    pub(crate) planes: Vec<([f32; 3], f32, i32)>,
    /// `None` is an empty lump.
    pub(crate) textures: Option<Vec<Option<TextureSpec>>>,
    pub(crate) vertexes: Vec<[f32; 3]>,
    /// Bytes added to the vertexes lump.
    pub(crate) vertex_padding: usize,
    pub(crate) visibility: Vec<u8>,
    pub(crate) nodes: Vec<NodeSpec>,
    pub(crate) texinfo: Vec<TexinfoSpec>,
    pub(crate) faces: Vec<FaceSpec>,
    pub(crate) lighting: Vec<u8>,
    pub(crate) clipnodes: Vec<(i32, [i32; 2])>,
    pub(crate) leafs: Vec<LeafSpec>,
    pub(crate) marksurfaces: Vec<u32>,
    pub(crate) edges: Vec<[u32; 2]>,
    pub(crate) surfedges: Vec<i32>,
    pub(crate) models: Vec<ModelSpec>,
}

pub(crate) fn leaf(contents: i32, visofs: i32, firstmarksurface: u32, count: u32) -> LeafSpec {
    LeafSpec {
        contents,
        visofs,
        mins: [0, 0, 0],
        maxs: [64, 64, 0],
        firstmarksurface,
        nummarksurfaces: count,
        ambient: [0, 1, 2, 3],
    }
}

impl Default for BspSpec {
    fn default() -> Self {
        Self {
            version: BspVersion::Bsp29,
            raw_version: None,
            entities: b"{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/base.wad\"\n}\n\
{\n\"classname\" \"func_door\"\n\"model\" \"*1\"\n}\n\0"
                .to_vec(),
            planes: vec![([0.0, 0.0, 1.0], 0.0, 2)],
            textures: Some(vec![texture(b"wall")]),
            vertexes: square(64.0),
            vertex_padding: 0,
            visibility: vec![1],
            nodes: vec![NodeSpec {
                planenum: 0,
                children: [-2, -1],
                mins: [0, 0, 0],
                maxs: [64, 64, 0],
                firstface: 0,
                numfaces: 1,
            }],
            texinfo: vec![TexinfoSpec {
                vecs: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
                miptex: 0,
                flags: 0,
            }],
            faces: vec![FaceSpec {
                planenum: 0,
                side: 0,
                firstedge: 0,
                numedges: 4,
                texinfo: 0,
                styles: [0, 255, 255, 255],
                lightofs: 0,
            }],
            lighting: (0..25).collect(),
            clipnodes: vec![(0, [-1, -2])],
            leafs: vec![leaf(-2, -1, 0, 0), leaf(-1, 0, 0, 1)],
            marksurfaces: vec![0],
            edges: vec![[0, 0], [0, 1], [1, 2], [2, 3], [3, 0]],
            surfedges: vec![1, 2, 3, -4],
            models: vec![
                ModelSpec {
                    mins: [0.0; 3],
                    maxs: [64.0, 64.0, 0.0],
                    headnode: [0; 4],
                    visleafs: 1,
                    firstface: 0,
                    numfaces: 1,
                },
                ModelSpec {
                    mins: [0.0; 3],
                    maxs: [64.0, 64.0, 8.0],
                    headnode: [0; 4],
                    visleafs: 0,
                    firstface: 0,
                    numfaces: 1,
                },
            ],
        }
    }
}

/// The base vector's square, of side `side`.
pub(crate) fn square(side: f32) -> Vec<[f32; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [side, 0.0, 0.0],
        [side, side, 0.0],
        [0.0, side, 0.0],
    ]
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i16(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&i16::try_from(value).unwrap().to_le_bytes());
}

fn put_u16(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
}

impl BspSpec {
    fn textures_lump(&self) -> Vec<u8> {
        let Some(textures) = &self.textures else {
            return Vec::new();
        };
        let mut out = Vec::new();
        put_i32(&mut out, i32::try_from(textures.len()).unwrap());
        let mut data = Vec::new();
        let table_size = 4 + 4 * textures.len();
        for texture in textures {
            let Some(texture) = texture else {
                put_i32(&mut out, -1);
                continue;
            };
            put_i32(&mut out, i32::try_from(table_size + data.len()).unwrap());
            let mut name = [0_u8; 16];
            name[..texture.name.len()].copy_from_slice(texture.name);
            data.extend_from_slice(&name);
            put_i32(&mut data, i32::try_from(texture.width).unwrap());
            put_i32(&mut data, i32::try_from(texture.height).unwrap());
            let area = texture.width * texture.height;
            let mut offset = 40;
            for level in 0..4 {
                put_i32(&mut data, i32::try_from(offset).unwrap());
                offset += area >> (2 * level);
            }
            let pixels = usize::try_from(area / 64 * 85).unwrap();
            data.extend(core::iter::repeat_n(texture.pixel, pixels));
        }
        out.extend_from_slice(&data);
        out
    }

    fn lumps(&self) -> [Vec<u8>; BSP_HEADER_LUMPS] {
        let wide = !matches!(self.version, BspVersion::Bsp29);
        let mut lumps: [Vec<u8>; BSP_HEADER_LUMPS] = Default::default();
        lumps[0].clone_from(&self.entities);
        for (normal, dist, kind) in &self.planes {
            for value in normal {
                put_f32(&mut lumps[1], *value);
            }
            put_f32(&mut lumps[1], *dist);
            put_i32(&mut lumps[1], *kind);
        }
        lumps[2] = self.textures_lump();
        for vertex in &self.vertexes {
            for value in vertex {
                put_f32(&mut lumps[3], *value);
            }
        }
        lumps[3].extend(core::iter::repeat_n(0, self.vertex_padding));
        lumps[4].clone_from(&self.visibility);
        for node in &self.nodes {
            let out = &mut lumps[5];
            put_i32(out, node.planenum);
            for child in node.children {
                if wide {
                    put_i32(out, child);
                } else {
                    put_i16(out, child);
                }
            }
            for value in node.mins.iter().chain(&node.maxs) {
                if self.version == BspVersion::Bsp2 {
                    put_f32(out, f32::from(*value));
                } else {
                    put_i16(out, i32::from(*value));
                }
            }
            if wide {
                out.extend_from_slice(&node.firstface.to_le_bytes());
                out.extend_from_slice(&node.numfaces.to_le_bytes());
            } else {
                put_u16(out, node.firstface);
                put_u16(out, node.numfaces);
            }
        }
        for info in &self.texinfo {
            for value in info.vecs.iter().flatten() {
                put_f32(&mut lumps[6], *value);
            }
            put_i32(&mut lumps[6], info.miptex);
            put_i32(&mut lumps[6], info.flags);
        }
        for face in &self.faces {
            let out = &mut lumps[7];
            if wide {
                for value in [
                    face.planenum,
                    face.side,
                    face.firstedge,
                    face.numedges,
                    face.texinfo,
                ] {
                    put_i32(out, value);
                }
            } else {
                put_i16(out, face.planenum);
                put_i16(out, face.side);
                put_i32(out, face.firstedge);
                put_i16(out, face.numedges);
                put_i16(out, face.texinfo);
            }
            out.extend_from_slice(&face.styles);
            put_i32(out, face.lightofs);
        }
        lumps[8].clone_from(&self.lighting);
        for (planenum, children) in &self.clipnodes {
            put_i32(&mut lumps[9], *planenum);
            for child in children {
                if wide {
                    put_i32(&mut lumps[9], *child);
                } else {
                    put_i16(&mut lumps[9], *child);
                }
            }
        }
        for leaf in &self.leafs {
            let out = &mut lumps[10];
            put_i32(out, leaf.contents);
            put_i32(out, leaf.visofs);
            for value in leaf.mins.iter().chain(&leaf.maxs) {
                if self.version == BspVersion::Bsp2 {
                    put_f32(out, f32::from(*value));
                } else {
                    put_i16(out, i32::from(*value));
                }
            }
            if wide {
                out.extend_from_slice(&leaf.firstmarksurface.to_le_bytes());
                out.extend_from_slice(&leaf.nummarksurfaces.to_le_bytes());
            } else {
                put_u16(out, leaf.firstmarksurface);
                put_u16(out, leaf.nummarksurfaces);
            }
            out.extend_from_slice(&leaf.ambient);
        }
        for mark in &self.marksurfaces {
            if wide {
                lumps[11].extend_from_slice(&mark.to_le_bytes());
            } else {
                put_u16(&mut lumps[11], *mark);
            }
        }
        for edge in &self.edges {
            for vertex in edge {
                if wide {
                    lumps[12].extend_from_slice(&vertex.to_le_bytes());
                } else {
                    put_u16(&mut lumps[12], *vertex);
                }
            }
        }
        for surfedge in &self.surfedges {
            put_i32(&mut lumps[13], *surfedge);
        }
        for model in &self.models {
            let out = &mut lumps[14];
            for value in model.mins.iter().chain(&model.maxs).chain(&[0.0; 3]) {
                put_f32(out, *value);
            }
            for value in
                model
                    .headnode
                    .iter()
                    .chain(&[model.visleafs, model.firstface, model.numfaces])
            {
                put_i32(out, *value);
            }
        }
        lumps
    }

    /// The file's bytes: the header, then each lump in order at a multiple of 4.
    pub(crate) fn build(&self) -> Vec<u8> {
        let version = self.raw_version.unwrap_or(match self.version {
            BspVersion::Bsp29 => 29,
            BspVersion::Bsp2Psb => i32::from_le_bytes(*b"2PSB"),
            BspVersion::Bsp2 => i32::from_le_bytes(*b"BSP2"),
        });
        let mut out = vec![0; BSP_HEADER_SIZE];
        out[..4].copy_from_slice(&version.to_le_bytes());
        for (index, lump) in self.lumps().iter().enumerate() {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
            let at = 4 + 8 * index;
            let offset = i32::try_from(out.len()).unwrap();
            out[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&i32::try_from(lump.len()).unwrap().to_le_bytes());
            out.extend_from_slice(lump);
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
        let hex = rest.get(..39).unwrap_or(rest);
        for group in hex.split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                let pair = core::str::from_utf8(pair).unwrap();
                out.push(u8::from_str_radix(pair, 16).unwrap());
            }
        }
    }
    out
}
