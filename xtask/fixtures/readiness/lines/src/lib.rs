// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the line-coverage threshold: no test calls `describe`.

pub fn double(value: u32) -> u32 {
    value.saturating_mul(2)
}

pub fn describe(value: u32) -> String {
    let doubled = double(value);
    let halved = value.checked_div(2).unwrap_or_default();
    let squared = value.saturating_mul(value);
    let mut text = String::new();
    text.push_str(&doubled.to_string());
    text.push(' ');
    text.push_str(&halved.to_string());
    text.push(' ');
    text.push_str(&squared.to_string());
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubles() {
        assert_eq!(super::double(2), 4);
    }
}
