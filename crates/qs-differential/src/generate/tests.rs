// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! Tests for the well-formed progs generator (FND-10).

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a failed unwrap or expect is a failed test"
)]

use qc_progs::Progs;

use super::{Generated, NUM_GLOBALS, generate};
use crate::{RunOutput, run_scenario};

/// Runs a generated program through QuakeC Studio's runner, as the fuzz target does.
fn run(data: &[u8]) -> RunOutput {
    let Generated { progs, scenario } = generate(data);
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    run_scenario(&scenario, &progs, entities).expect("a generated progs must load and run")
}

// FND-10: the generator feeds well-formed progs, so the loader of a runner accepts every one
// it builds, empty input included.
#[test]
fn every_generated_progs_loads() {
    for data in [
        &[][..],
        &[0][..],
        &[0xff; 4][..],
        &[1, 2, 3, 4, 5, 6, 7, 8][..],
        &(0..=255).collect::<Vec<u8>>()[..],
    ] {
        let Generated { progs, .. } = generate(data);
        let loaded = Progs::read(&progs);
        assert!(
            loaded.is_ok(),
            "generated progs rejected: {:?}",
            loaded.err()
        );
        let header = loaded.unwrap();
        assert_eq!(header.header().numglobals.cast_unsigned(), NUM_GLOBALS);
    }
}

// FND-10: a well-formed progs runs in QuakeC Studio's VM without a panic, writing a trace
// that both runners can compare (DIFF-1.1).
#[test]
fn a_generated_progs_runs_and_writes_a_trace() {
    let output = run(&[]);
    assert!(
        !output.trace.records.is_empty(),
        "even the empty program's run writes trace records"
    );
}

// FND-10: the generator is deterministic in its bytes (DIFF-1.4), so a crashing input
// replays; the same bytes build the same progs.
#[test]
fn generation_is_deterministic() {
    let data = &[9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 42, 17, 200, 13][..];
    let a = generate(data);
    let b = generate(data);
    assert_eq!(a.progs, b.progs);
    assert_eq!(a.scenario, b.scenario);
}

// FND-10: a body of opcodes and jumps built from bytes still loads and runs; the forward
// jumps never run away (the run returns).
#[test]
fn a_body_with_jumps_runs() {
    // Bytes chosen to exercise branches, gotos and ordinary opcodes.
    let data: Vec<u8> = (0..512_u32)
        .map(|i| u8::try_from((i * 7 + 3) % 256).unwrap_or(0))
        .collect();
    let output = run(&data);
    assert!(!output.trace.records.is_empty());
}

// FND-10: the StartFrame function the server frame calls runs the generated body, so a
// non-empty body executes more statements than the empty one (DIFF-5.3).
#[test]
fn the_body_is_executed() {
    let empty = run(&[]);
    // A body of a few plain store opcodes: op byte picks a store, then operands.
    let data: Vec<u8> = vec![
        // statement 1: choice % 8 != 0,1 -> Op; opcode byte, three operand u16s.
        2, 50, 0, 0, 0, 1, 0, // Op
        2, 51, 0, 1, 0, 2, 0, // Op
        2, 52, 0, 2, 0, 3, 0, // Op
    ];
    let body = run(&data);
    let count = |o: &RunOutput| {
        o.trace
            .records
            .iter()
            .filter(|r| matches!(r, qc_trace::Record::Step(_)))
            .count()
    };
    assert!(
        count(&body) > count(&empty),
        "a body should execute more statements than the empty program"
    );
}

// FND-10: a regression seed. The input below mixes arithmetic, a forward branch, a goto
// and arbitrary scratch values (a NaN and a large float among them); it must build a progs
// `qc-vm` runs cleanly, so a future change that breaks this shape is caught here. A crash the
// fuzz target finds is minimised and added as a case like this (the project's "minimised into
// a regression test" rule), so the nightly corpus is not the only record of it.
#[test]
fn regression_body_arith_branch_goto_runs_clean() {
    let data: &[u8] = &[
        2, 6, 0, 0, 1, 0, 2, 0, // ADD_F g0, g1 -> g2
        2, 8, 3, 0, 4, 0, 5, 0, // SUB-class op over other scratch slots
        0, 0, 7, 0, 3, 0, // IF g7, forward skip
        1, 2, 0, // GOTO forward skip
        2, 4, 6, 0, 7, 0, 8, 0, // another op
        0x00, 0x00, 0x80, 0x3f, // scratch value 1.0
        0x00, 0x00, 0xc0, 0x7f, // scratch value: a quiet NaN (VM-4.3)
        0xff, 0xff, 0x7f, 0x7f, // scratch value: a large finite float
        0x05, 0x00, 0x00, 0x00, // scratch value: a small integer bit pattern
    ];
    let output = run(data);
    assert!(
        output
            .trace
            .records
            .iter()
            .any(|r| matches!(r, qc_trace::Record::Step(_))),
        "the regression program executes statements"
    );
}
