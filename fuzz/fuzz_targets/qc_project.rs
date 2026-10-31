// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the project graph (P1-23) as a module's source and the one file
//! it may bring in, split at the first NUL, under the vanilla and the FTEQCC profiles, whose
//! `#if` readers differ (PRJ-5.5, FTEF-14.11) and whose files the FTEQCC profile reads with the
//! preprocessor's directive reader (PRJ-5.13). Fails on a panic or a graph that breaks PRJ-6.1
//! or PRJ-6.4.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qc_lexer::Dialect;
use qc_project::{MemoryLoader, PRJ_MAX_FILES, build_graph};

fuzz_target!(|data: &[u8]| {
    let (root, other) = match data.iter().position(|b| *b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let mut loader = MemoryLoader::new();
        loader.insert("main.src", root);
        loader.insert("a.qc", other);
        let graph = build_graph(&mut loader, Path::new("main.src"), &[], &[], dialect);
        assert!(graph.files.len() <= PRJ_MAX_FILES);
        for (i, file) in graph.files.iter().enumerate() {
            assert!(file.parent.is_none_or(|p| p.index() < i));
        }
    }
});
