// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the project graph (P1-23) as a module's source and the one file
//! it may bring in, split at the first NUL. Fails on a panic or a graph that breaks PRJ-6.1
//! or PRJ-6.4.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qc_project::{MemoryLoader, PRJ_MAX_FILES, build_graph};

fuzz_target!(|data: &[u8]| {
    let (root, other) = match data.iter().position(|b| *b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    let mut loader = MemoryLoader::new();
    loader.insert("main.src", root);
    loader.insert("a.qc", other);
    let graph = build_graph(&mut loader, Path::new("main.src"), &[], &[]);
    assert!(graph.files.len() <= PRJ_MAX_FILES);
    for (i, file) in graph.files.iter().enumerate() {
        assert!(file.parent.is_none_or(|p| p < i));
    }
});
