// SPDX-License-Identifier: GPL-3.0-or-later
// expect: pass
// A test module opts out of the no-panic lints with a module-level expect that
// names each lint and gives a "test code: ..." reason (docs/conventions.md).

pub fn double(value: u8) -> Option<u8> {
    value.checked_mul(2)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::*;

    #[test]
    fn doubles() {
        let values: &[u8] = &[double(2).unwrap()];
        assert_eq!(values[0], 4);
    }
}
