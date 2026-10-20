// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the spec-citation threshold: no test cites FX-1.2. The `// spec` line in
//! `first` is shipped code, which cites nothing.

pub fn first() -> u8 {
    // spec FX-1.2: not a test
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
