// SPDX-License-Identifier: GPL-3.0-or-later

//! Base64, which the bridge carries a screenshot's PNG in.

/// `bytes` in base64 with padding (RFC 4648, section 4): a screenshot's PNG as the host sends it
/// (BRIDGE-4.9), and an image as the agent passes it to a model.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // RFC 4648, section 4: each group of three bytes, a 24-bit number, is four symbols of six
    // bits, the first the highest.
    const GROUP_BYTES: usize = 3;
    const GROUP_SYMBOLS: usize = 4;
    const BYTE_BITS: u32 = 8;
    const SYMBOL_BITS: u32 = 6;
    const SYMBOL_MASK: u32 = (1 << SYMBOL_BITS) - 1;
    const SYMBOL_SHIFTS: [u32; GROUP_SYMBOLS] = [3 * SYMBOL_BITS, 2 * SYMBOL_BITS, SYMBOL_BITS, 0];
    let symbol = |index: u32| {
        let index = usize::try_from(index & SYMBOL_MASK).unwrap_or(0);
        char::from(ALPHABET.get(index).copied().unwrap_or(b'A'))
    };
    let mut out = String::with_capacity(
        bytes
            .len()
            .div_ceil(GROUP_BYTES)
            .saturating_mul(GROUP_SYMBOLS),
    );
    for chunk in bytes.chunks(GROUP_BYTES) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let group = (byte(0) << (2 * BYTE_BITS)) | (byte(1) << BYTE_BITS) | byte(2);
        // A last chunk of n bytes fills n + 1 symbols; the rest are padding.
        let written = chunk.len().saturating_add(1);
        for (place, shift) in SYMBOL_SHIFTS.into_iter().enumerate() {
            out.push(if place < written {
                symbol(group >> shift)
            } else {
                '='
            });
        }
    }
    out
}
