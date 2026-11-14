// SPDX-License-Identifier: GPL-3.0-or-later

//! Base64, which the bridge carries a screenshot's PNG in (BRIDGE-4.9): the host's encoder and
//! the client's decoder.

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

/// `text` decoded from base64 (RFC 4648, section 4, with `=` padding), as the host writes a
/// screenshot's PNG; `None` for text that is not base64.
#[must_use]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    // The alphabet's runs and the value each starts at (RFC 4648, section 4, Table 1).
    const UPPER: u8 = 0;
    const LOWER: u8 = 26;
    const DIGITS: u8 = 52;
    const PLUS: u8 = 62;
    const SLASH: u8 = 63;
    // Each four symbols of six bits are three bytes; padding stands for the bytes not there.
    const GROUP_BYTES: usize = 3;
    const GROUP_SYMBOLS: usize = 4;
    const SYMBOL_BITS: u32 = 6;
    const MAX_PADDING: usize = 2;
    let value = |c: u8| -> Option<u32> {
        let index = match c {
            b'A'..=b'Z' => c.checked_sub(b'A')?.checked_add(UPPER)?,
            b'a'..=b'z' => c.checked_sub(b'a')?.checked_add(LOWER)?,
            b'0'..=b'9' => c.checked_sub(b'0')?.checked_add(DIGITS)?,
            b'+' => PLUS,
            b'/' => SLASH,
            _ => return None,
        };
        Some(u32::from(index))
    };
    let bytes = text.trim().as_bytes();
    if !bytes.len().is_multiple_of(GROUP_SYMBOLS) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    let (quads, _) = bytes.as_chunks::<GROUP_SYMBOLS>();
    let last = quads.len().saturating_sub(1);
    for (n, quad) in quads.iter().enumerate() {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > MAX_PADDING || (pad > 0 && n != last) {
            return None;
        }
        let mut word = 0_u32;
        for &c in quad.iter().take(GROUP_SYMBOLS.saturating_sub(pad)) {
            word = (word << SYMBOL_BITS) | value(c)?;
        }
        word <<= SYMBOL_BITS.checked_mul(u32::try_from(pad).ok()?)?;
        let [_, a, b, c] = word.to_be_bytes();
        out.extend([a, b, c].into_iter().take(GROUP_BYTES.saturating_sub(pad)));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{base64, base64_decode};

    // No spec statement: RFC 4648, section 10's test vectors encode and decode, and text that
    // is not base64 is refused
    #[test]
    fn base64_is_rfc_4648s() {
        for (bytes, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(bytes.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).as_deref(), Some(bytes.as_bytes()));
        }
        assert_eq!(base64(&[0xFF, 0xEF, 0xBE]), "/+++");
        for bad in ["Zg=", "Z===", "Zg==Zm9v", "Zm9!", "Zm9", "===="] {
            assert_eq!(base64_decode(bad), None, "{bad}");
        }
    }

    proptest::proptest! {
        // No spec statement: whatever bytes are encoded decode back
        #[test]
        fn base64_round_trips(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..300)) {
            proptest::prop_assert_eq!(base64_decode(&base64(&bytes)), Some(bytes));
        }
    }
}
