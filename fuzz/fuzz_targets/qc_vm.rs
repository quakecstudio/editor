// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzes `qc-vm` against the id VM oracle with well-formed random progs (FND-10, P1-15).
//!
//! Each input builds a well-formed version 6 progs and a scenario
//! (`qs_differential::generate`), runs it in QuakeC Studio's VM through the differential host
//! loop (`qs_differential::run_scenario`, DIFF-1.1), and, when the id VM oracle is available,
//! runs the same scenario in the oracle and compares the two traces statement by statement
//! (`qs_differential::compare`, DIFF-9). The check:
//!
//! - QuakeC Studio's VM never panics or runs away on a well-formed progs: it either writes a
//!   trace or returns a refusal, never aborts (VM-19).
//! - When `QS_VM_ORACLE` names the oracle binary (`cargo xtask oracles` builds it), the two
//!   traces agree (DIFF-9): any `Comparison::Failure` is a divergence that DIFF-10 does not
//!   expect, so it is a defect in `qc-vm`, the host, or the oracle, and fails the run
//!   (DIFF-10.3).
//!
//! Without the oracle (a plain `cargo xtask fuzz` or a pull request build, which does not
//! build the C oracle) the target still runs QuakeC Studio's VM over every input and fails on
//! a panic or runaway; the nightly `Fuzz` workflow builds the oracle, sets `QS_VM_ORACLE` and
//! so does the full differential comparison. The acceptance of P1-15, a 24-hour run with no
//! divergence or crash, is that nightly job's over the persisted corpus; a crash minimises
//! into a regression seed in `fuzz/corpus/qc-vm/`.
//!
//! See `fuzz/README.md` for what this target compares and how the oracle is wired in.

#![no_main]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use qc_trace::decode;
use qs_differential::suite::initial_globals;
use qs_differential::{Comparison, Generated, compare, generate, run_scenario};

/// The entity lump every generated scenario runs, the one `worldspawn` default (DIFF-2.2).
const ENTITIES: &[u8] = b"{\n\"classname\" \"worldspawn\"\n}\n";

/// The scenario text that drives a generated progs, matching `generate`'s scenario: the
/// default map, four server frames, the default seed and frametime (DIFF-2). Both runners
/// read the same file (DIFF-2.5).
const SCENARIO_TEXT: &str = "\
progs progs.dat
entities map.ent
map test
maxclients 1
clients 0
submodels 0
frames 4
frametime 0.1
seed 1
";

fuzz_target!(|data: &[u8]| {
    let Generated { progs, scenario } = generate(data);

    // QuakeC Studio's VM must not panic or run away on a well-formed progs (VM-19).
    let ours = match run_scenario(&scenario, &progs, ENTITIES) {
        Ok(output) => output.trace,
        // A refusal is a reported error, not a crash; nothing to compare (DIFF-3.1).
        Err(_) => return,
    };

    // Without the oracle, running QuakeC Studio's VM without a panic is the whole check.
    let Some(oracle) = oracle_binary() else {
        return;
    };

    // Run the same scenario in the id VM oracle and compare the two traces (DIFF-1.1, DIFF-9).
    let Some(theirs_bytes) = run_oracle(oracle, &progs) else {
        // The oracle could not be run (a setup problem), not a divergence; skip this input.
        return;
    };
    let theirs = decode(&theirs_bytes).expect("the oracle wrote a decodable trace");
    let initial = initial_globals(&progs).expect("a generated progs has readable globals");

    // DIFF-9.6, DIFF-10.3: the comparator already treats every divergence it returns as a
    // failure, the expected divergences of DIFF-10 handled inside it, so a Failure is a defect.
    if let Comparison::Failure(divergence) = compare(&ours, &theirs, &initial) {
        panic!(
            "qc-vm diverged from the id VM oracle at statement {}: {divergence:?}",
            divergence.statement()
        );
    }
});

/// The id VM oracle binary, from `QS_VM_ORACLE`, or `None` when it is unset or not a file.
fn oracle_binary() -> Option<&'static Path> {
    static ORACLE: OnceLock<Option<PathBuf>> = OnceLock::new();
    ORACLE
        .get_or_init(|| {
            std::env::var_os("QS_VM_ORACLE")
                .map(PathBuf::from)
                .filter(|path| path.is_file())
        })
        .as_deref()
}

/// Writes the progs and scenario to a temporary directory, runs the oracle on it and returns
/// its trace bytes, or `None` when the oracle could not run.
///
/// The oracle reads the scenario file, which names `progs.dat` and `map.ent` beside it, and
/// writes the trace to the path given as its third argument (`oracle scenario <scn> <trace>`),
/// the same call the differential suite makes (`src/bin/differential.rs`).
fn run_oracle(oracle: &Path, progs: &[u8]) -> Option<Vec<u8>> {
    let dir = tempdir()?;
    let scenario = dir.join("case.scn");
    let trace = dir.join("case.trace");
    std::fs::write(dir.join("progs.dat"), progs).ok()?;
    std::fs::write(dir.join("map.ent"), ENTITIES).ok()?;
    std::fs::write(&scenario, SCENARIO_TEXT).ok()?;

    let status = Command::new(oracle)
        .arg("scenario")
        .arg(&scenario)
        .arg(&trace)
        .status()
        .ok()?;
    let bytes = if status.success() {
        std::fs::read(&trace).ok()
    } else {
        // A non-zero exit on a well-formed progs can be the oracle crashing on VM behaviour
        // id's C leaves undefined (DIFF-10.1); the comparator would stop at the agreeing error
        // record anyway, so let the next input continue rather than flag a false divergence.
        None
    };
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

/// A fresh temporary directory for one oracle run, under the system temp dir, named from a
/// process-unique counter so parallel workers do not collide.
fn tempdir() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("qs-fuzz-qc-vm-{pid}-{n}"));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
