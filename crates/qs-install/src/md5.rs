// SPDX-License-Identifier: GPL-3.0-or-later

//! MD5 (RFC 1321), the checksum the published tables of Quake's game files use (FS-6.2).
//!
//! It identifies files; it is not used for security. Written from RFC 1321, section 3
//! (<https://www.rfc-editor.org/rfc/rfc1321.txt>, fetched 2026-10-16).

/// The digest of the empty input's initial state, words A to D (RFC 1321, section 3.3).
const INIT: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

/// `T[i]`, the integer part of 4294967296 times abs(sin(i)) for i = 1 to 64 (section 3.4).
const T: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// The rotation of each step, four per round (section 3.4, `S11` to `S44`).
const S: [[u32; 4]; 4] = [
    [7, 12, 17, 22],
    [5, 9, 14, 20],
    [4, 11, 16, 23],
    [6, 10, 15, 21],
];

const BLOCK: usize = 64;

/// An MD5 computation fed in pieces.
#[derive(Clone, Debug)]
pub(crate) struct Md5 {
    state: [u32; 4],
    pending: Vec<u8>,
    /// The message length in bytes, modulo 2^64.
    length: u64,
}

impl Md5 {
    pub(crate) fn new() -> Self {
        Self {
            state: INIT,
            pending: Vec::with_capacity(BLOCK),
            length: 0,
        }
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        let added = u64::try_from(data.len()).unwrap_or(u64::MAX);
        self.length = self.length.wrapping_add(added);
        let mut data = data;
        if !self.pending.is_empty() {
            let need = BLOCK.saturating_sub(self.pending.len());
            let (head, tail) = data.split_at(need.min(data.len()));
            self.pending.extend_from_slice(head);
            data = tail;
            if self.pending.len() < BLOCK {
                return;
            }
            let block = std::mem::take(&mut self.pending);
            self.compress_all(&block);
            self.pending = block;
            self.pending.clear();
        }
        let whole = data.len().saturating_sub(data.len() % BLOCK);
        let (blocks, rest) = data.split_at(whole);
        self.compress_all(blocks);
        self.pending.extend_from_slice(rest);
    }

    /// Pads the message (section 3.1 and 3.2) and returns the digest, A to D low byte first
    /// (section 3.5).
    pub(crate) fn finish(mut self) -> [u8; 16] {
        let bits = self.length.wrapping_mul(8);
        let mut tail = std::mem::take(&mut self.pending);
        tail.push(0x80);
        while tail.len() % BLOCK != BLOCK.saturating_sub(8) {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_le_bytes());
        self.compress_all(&tail);
        let mut digest = [0; 16];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_le_bytes();
        }
        digest
    }

    fn compress_all(&mut self, blocks: &[u8]) {
        for block in blocks.as_chunks::<BLOCK>().0 {
            let mut x = [0_u32; 16];
            for (word, bytes) in x.iter_mut().zip(block.as_chunks::<4>().0) {
                *word = u32::from_le_bytes(*bytes);
            }
            self.state = compress(self.state, &x);
        }
    }
}

/// One application of the four rounds of section 3.4 to a 16-word block.
fn compress(state: [u32; 4], x: &[u32; 16]) -> [u32; 4] {
    let [mut a, mut b, mut c, mut d] = state;
    for (step, &t) in T.iter().enumerate() {
        let (round, f, k) = match step {
            0..16 => (0, (b & c) | (!b & d), step),
            16..32 => (1, (b & d) | (c & !d), step.wrapping_mul(5).wrapping_add(1)),
            32..48 => (2, b ^ c ^ d, step.wrapping_mul(3).wrapping_add(5)),
            _ => (3, c ^ (b | !d), step.wrapping_mul(7)),
        };
        let word = x.get(k & 15).copied().unwrap_or(0);
        let shift = S
            .get(round)
            .and_then(|shifts| shifts.get(step & 3))
            .copied()
            .unwrap_or(0);
        let sum = a.wrapping_add(f).wrapping_add(word).wrapping_add(t);
        let next = b.wrapping_add(sum.rotate_left(shift));
        (a, b, c, d) = (d, next, b, c);
    }
    let [a0, b0, c0, d0] = state;
    [
        a0.wrapping_add(a),
        b0.wrapping_add(b),
        c0.wrapping_add(c),
        d0.wrapping_add(d),
    ]
}

/// The lowercase hexadecimal form of a digest, as the published tables write it.
pub(crate) fn hex(digest: &[u8; 16]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(32);
    for byte in digest {
        let _ignored = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn md5(bytes: &[u8]) -> String {
        let mut hasher = Md5::new();
        hasher.update(bytes);
        hex(&hasher.finish())
    }

    #[test]
    fn rfc_1321_test_suite() {
        // RFC 1321, appendix A.5.
        let cases: [(&str, &str); 7] = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(md5(input.as_bytes()), expected, "{input:?}");
        }
    }

    #[test]
    fn padding_boundaries() {
        // Checked against Python's `hashlib.md5` on 2026-10-16: 55 bytes fit one padded block, 56
        // and 64 need a second.
        assert_eq!(md5(&[b'x'; 55]), "04364420e25c512fd958a70738aa8f72");
        assert_eq!(md5(&[b'x'; 56]), "668a72d5ba17f08e62dabcafad6db14b");
        assert_eq!(md5(&[b'x'; 64]), "c1bb4f81d892b2d57947682aeb252456");
    }

    proptest! {
        #[test]
        fn splitting_the_input_never_changes_the_digest(
            bytes in proptest::collection::vec(any::<u8>(), 0..300),
            cuts in proptest::collection::vec(0_usize..300, 0..6),
        ) {
            let mut cuts: Vec<usize> = cuts.into_iter().map(|cut| cut.min(bytes.len())).collect();
            cuts.sort_unstable();
            let mut hasher = Md5::new();
            let mut from = 0;
            for cut in cuts.into_iter().chain([bytes.len()]) {
                hasher.update(bytes.get(from..cut).unwrap_or_default());
                from = cut;
            }
            prop_assert_eq!(hex(&hasher.finish()), md5(&bytes));
        }
    }
}
