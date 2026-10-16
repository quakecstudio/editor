// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the branch-coverage threshold: every line runs, but the test takes
//! only one side of each condition.

pub fn clamp(value: i32) -> i32 {
    if value > 10 { 10 } else { value }
}

pub fn sign(value: i32) -> i32 {
    if value < 0 { -1 } else { 1 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_small_positive_values() {
        assert_eq!(super::clamp(5), 5);
        assert_eq!(super::sign(5), 1);
    }
}
