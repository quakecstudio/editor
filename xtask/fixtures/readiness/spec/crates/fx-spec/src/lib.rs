// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the spec-citation threshold: no test cites FX-1.2.

pub fn first() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    // spec FX-1.1: the first statement
    #[test]
    fn first_is_one() {
        assert_eq!(super::first(), 1);
    }
}
