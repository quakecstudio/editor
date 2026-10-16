// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary text to the parser of the VM vectors (`docs/spec/vectors/vm/README.md`) that
//! `cargo xtask oracles` runs through the VM oracle, `xtask/src/oracles/vectors.rs`, included
//! here as a module because xtask is a program, not a library. Each input is parsed as given
//! and as text built from the format's own lines, so the fuzzer reaches every block; every
//! vector that parses is then turned into its progs and init file. Fails on a panic.

#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

#[path = "../../xtask/src/oracles/vectors.rs"]
#[expect(dead_code, reason = "the fuzz target uses the parser, not the runner")]
mod vectors;

/// What `vectors.rs` names as `super::Oracles`: where the oracle runner finds the VM oracle.
#[expect(dead_code, reason = "only the runner, which the fuzz target leaves out, reads it")]
pub(crate) struct Oracles {
    pub(crate) vm: PathBuf,
    pub(crate) work: PathBuf,
}

/// Lines of the vector format, which each input byte picks from.
const VOCABULARY: [&str; 32] = [
    "== v\n",
    "spec: VM-1.1, BUILTIN-2.5\n",
    "globals: 100=1.5 101=e1 102=p1.105 103=i-1 104=nan\n",
    "fields: e1.30=2 e2.105=es\n",
    "strings: \"a\" \"\\x80\"\n",
    "numglobals: 300\n",
    "edicts: 3\n",
    "maxclients: 2\n",
    "seed: 7\n",
    "server: loading\n",
    "builtins: engine\n",
    "entry: 2\n",
    "functions:\n",
    "  2: first=1 parm_start=100 locals=2 parms=1,3\n",
    "  3: builtin=12\n",
    "host:\n",
    "  clock 1.25\n",
    "  run 2\n",
    "  think e1 0.1\n",
    "  frame 0.1 3\n",
    "  edict 2 100\n",
    "steps: 4\n",
    "after: depth=0 localstack=2\n",
    "datagram: 1 255\n",
    "oracle: no\n",
    "code:\n",
    "  STORE_F 100 101\n",
    "  CALL1 4\n",
    "expect:\n",
    "  1 STORE_F 101=1.5\n",
    "  error \"runaway loop error\"\n",
    "  done\n",
];

/// Text of the lines `data` picks, one byte each.
fn lines(data: &[u8]) -> String {
    let mut text = String::new();
    for &byte in data {
        text.push_str(VOCABULARY[usize::from(byte) % VOCABULARY.len()]);
    }
    text
}

fn check(text: &str) {
    if let Ok(vectors) = vectors::parse("fuzz.txt", text) {
        for vector in &vectors {
            let _init = vectors::init_text(vector);
            let _progs = vectors::build_progs(vector);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        check(text);
    }
    check(&lines(data));
});
