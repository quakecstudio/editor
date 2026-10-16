// SPDX-License-Identifier: GPL-3.0-or-later
// expect: pass
// The control: code that spells out its arithmetic and lookups passes, and an
// exception made with #[expect] and a reason is accepted.

pub fn add(a: u32, b: u32) -> Option<u32> {
    a.checked_add(b)
}

pub fn first(values: &[u8]) -> Option<u8> {
    values.first().copied()
}

#[expect(dead_code, reason = "fixture: an expectation with a reason is allowed")]
fn unused() {}
