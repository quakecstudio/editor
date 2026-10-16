// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out WAV files as the spec's vectors do.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap or overflow is a failed test"
)]

/// One chunk of a [`WavSpec`]: its id, its data, and the length stored for it, which is the
/// data's length unless set.
#[derive(Clone, Debug)]
pub(crate) struct ChunkSpec {
    pub(crate) id: [u8; 4],
    pub(crate) data: Vec<u8>,
    pub(crate) len: Option<i32>,
}

/// A chunk named `id` holding `data`.
pub(crate) fn chunk(id: &[u8; 4], data: &[u8]) -> ChunkSpec {
    ChunkSpec {
        id: *id,
        data: data.to_vec(),
        len: None,
    }
}

/// A `fmt ` chunk of 16 bytes.
pub(crate) fn fmt(format: i16, channels: i16, rate: i32, bits: i16) -> ChunkSpec {
    let mut data = Vec::new();
    data.extend_from_slice(&format.to_le_bytes());
    data.extend_from_slice(&channels.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    let block = i16::try_from(i32::from(channels) * i32::from(bits) / 8).unwrap_or(0);
    data.extend_from_slice(&(rate * i32::from(block)).to_le_bytes());
    data.extend_from_slice(&block.to_le_bytes());
    data.extend_from_slice(&bits.to_le_bytes());
    chunk(b"fmt ", &data)
}

/// A `cue ` chunk of one cue point whose sample offset is `offset`.
pub(crate) fn cue(offset: i32) -> ChunkSpec {
    let mut data = Vec::new();
    data.extend_from_slice(&1_i32.to_le_bytes());
    data.extend_from_slice(&1_i32.to_le_bytes());
    data.extend_from_slice(&offset.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&0_i32.to_le_bytes());
    data.extend_from_slice(&0_i32.to_le_bytes());
    data.extend_from_slice(&offset.to_le_bytes());
    chunk(b"cue ", &data)
}

/// A `LIST` chunk of type `adtl` holding one `ltxt` sub-chunk of `count` samples whose
/// purpose is `purpose`, as Cool Edit writes a loop length.
pub(crate) fn list(count: i32, purpose: &[u8; 4]) -> ChunkSpec {
    let mut data = Vec::new();
    data.extend_from_slice(b"adtl");
    data.extend_from_slice(b"ltxt");
    data.extend_from_slice(&20_i32.to_le_bytes());
    data.extend_from_slice(&1_i32.to_le_bytes());
    data.extend_from_slice(&count.to_le_bytes());
    data.extend_from_slice(purpose);
    data.extend_from_slice(&[0; 8]);
    chunk(b"LIST", &data)
}

/// A WAV file to build. The default is the first row of the WAV-3 vectors: PCM, one
/// channel, 11025 samples per second, 8 bits, and 5 samples.
#[derive(Clone, Debug)]
pub(crate) struct WavSpec {
    pub(crate) form: [u8; 4],
    pub(crate) chunks: Vec<ChunkSpec>,
}

impl Default for WavSpec {
    fn default() -> Self {
        Self {
            form: *b"WAVE",
            chunks: vec![fmt(1, 1, 11025, 8), chunk(b"data", &[128, 160, 255, 0, 96])],
        }
    }
}

impl WavSpec {
    /// The default file with `extra` inserted before its `data` chunk.
    pub(crate) fn before_data(extra: Vec<ChunkSpec>) -> Self {
        let mut spec = Self::default();
        let data = spec.chunks.pop().unwrap();
        spec.chunks.extend(extra);
        spec.chunks.push(data);
        spec
    }

    /// The file's bytes: a `RIFF` chunk holding the form type and every chunk, each padded
    /// to an even length.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut body = self.form.to_vec();
        for spec in &self.chunks {
            body.extend_from_slice(&spec.id);
            let len = spec
                .len
                .unwrap_or_else(|| i32::try_from(spec.data.len()).unwrap());
            body.extend_from_slice(&len.to_le_bytes());
            body.extend_from_slice(&spec.data);
            if spec.data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&i32::try_from(body.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&body);
        bytes
    }
}
