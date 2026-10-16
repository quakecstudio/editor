// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the parser (P1-21). Fails on a panic, a stack overflow, or a tree
//! that does not give back the input byte for byte.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qc_syntax::parse;

fuzz_target!(|data: &[u8]| {
    let parsed = parse(data);
    assert_eq!(parsed.syntax().text(), data);
});
