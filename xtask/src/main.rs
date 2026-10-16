// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask`: build automation for the workspace.
//!
//! - `ci`: runs every check below plus fmt, clippy, the tests and cargo-deny;
//!   `make ci` calls it, and so does CI.
//! - `adr`: checks the decision records in `docs/adr/` and their index.
//! - `conventions`: checks every config value against `docs/conventions.md`.
//! - `clippy-fixtures`: proves the workspace lints reject the code they target.
//! - `deny-fixtures`: proves `deny.toml` rejects incompatible licenses and
//!   wildcard versions (needs network access to the crates.io index).
//! - `layers`: checks every crate's layer in `layers.toml` and that it depends
//!   only on its own layer or lower (`docs/layers.md`).
//! - `licenses`: checks the SPDX header of every file and the ported-file
//!   header rule in `docs/porting.md`.
//! - `attribution`: checks every asset file is listed in `ATTRIBUTION.md`.
//! - `contrast`: checks every colour pair in `docs/design/tokens.toml` meets
//!   WCAG AA in both themes.
//! - `bench`: runs the performance-budget benchmarks, checks them against
//!   `docs/perf-budgets.md` and the last run, and records them in the JSON
//!   history; `bench history` prints the history.
//! - `readiness`: measures one readiness check (coverage, mutation score, spec
//!   citations or flakes) for a list of crates and writes the numbers as JSON.
//! - `ratchet`: judges those numbers against the bar in ADR-0009 and the
//!   minimums in `ratchets.toml`, and checks how `ratchets.toml` has changed.
//! - `fuzz`: runs the cargo-fuzz targets in `fuzz/` on nightly for a fixed time
//!   each; `make test-fuzz` and the nightly `Fuzz` workflow call it.
//! - `corpus`: `corpus verify [--online]` checks `corpus/manifest.toml` and the repository
//!   against the corpus policy; `corpus fetch` downloads codebases and maps by checksum into a
//!   cache outside the source tree; `corpus build` compiles the codebases an oracle compiler
//!   builds and checks each progs.dat round-trips through `qc-progs`.
//! - `assets`: `assets fetch` downloads, checks and extracts the open test assets;
//!   `assets precache` checks they cover the rerelease's precaches.
//! - `lex-corpus`: lexes every `.qc` file of a codebase directory and
//!   preprocesses the files its `progs.src` lists, failing on any error.
//! - `oracles`: fetches id's and FTEQCC's sources by checksum, builds the
//!   oracles in `oracles/` (id's VM as a trace runner, id's qcc and FTEQCC) with
//!   the system C compiler and checks them; `oracles id101 <dir>` also compiles
//!   and runs id's QuakeC 1.01, and `oracles rerelease <dir>` compiles the
//!   rerelease with FTEQCC. CI and `make oracles` run it; it is not part of
//!   `ci`, which runs offline.
//! - `parse-corpus`: parses every `.qc` and `.qh` file under codebase
//!   directories and checks the tree gives back each file; `--vanilla` fails on
//!   any error and `--deletions` checks that recovery from each deleted token is
//!   local.
//!
//! `readiness` and `ratchet` run in the on-demand `Readiness` workflow, not in
//! `ci` (ADR-0009).

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a command-line tool; this is its crate root"
)]

mod adr;
mod attribution;
mod bench;
mod ci;
mod contrast;
mod conventions;
mod corpus;
mod download;
mod fixtures;
mod fuzz;
mod layers;
mod lex_corpus;
mod licenses;
mod oracles;
mod parse_corpus;
mod ported;
mod ratchet;
mod readiness;
mod repo;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci | adr | conventions | clippy-fixtures | \
                     deny-fixtures | layers | licenses | attribution | contrast | bench | \
                     readiness | ratchet | fuzz | corpus | assets | lex-corpus | oracles | \
                     parse-corpus>";

/// A command that takes arguments and says itself whether it passed.
type Command = fn(&Path, &[String]) -> Result<bool, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    if let Some((name, rest)) = args.split_first() {
        let command: Option<Command> = match name.as_str() {
            "assets" => Some(corpus::assets_main),
            "bench" => Some(bench::main),
            "corpus" => Some(corpus::main),
            "fuzz" => Some(fuzz::main),
            "lex-corpus" => Some(lex_corpus::main),
            "oracles" => Some(oracles::main),
            "parse-corpus" => Some(parse_corpus::main),
            "readiness" => Some(readiness::main),
            "ratchet" => Some(ratchet::main),
            _ => None,
        };
        if let Some(command) = command {
            return match command(&root, rest) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(error) => {
                    eprintln!("{name}: {error}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    let [command] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let ok = if command == "ci" {
        ci::run(&root)
    } else {
        let Some(check) = in_process_check(command) else {
            eprintln!("unknown command {command:?}\n{USAGE}");
            return ExitCode::FAILURE;
        };
        print_outcome(command, check(&root))
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A check that runs inside xtask and returns a [`Report`].
pub(crate) type Check = fn(&Path) -> Result<Report, String>;

/// The check named `command`, if xtask runs it in-process.
pub(crate) fn in_process_check(command: &str) -> Option<Check> {
    match command {
        "adr" => Some(adr::check),
        "conventions" => Some(conventions::check),
        "clippy-fixtures" => Some(fixtures::clippy),
        "deny-fixtures" => Some(fixtures::deny),
        "layers" => Some(layers::check),
        "licenses" => Some(licenses::run),
        "attribution" => Some(attribution::run),
        "contrast" => Some(contrast::check),
        _ => None,
    }
}

/// Prints a check's report and returns whether it passed.
pub(crate) fn print_outcome(command: &str, result: Result<Report, String>) -> bool {
    match result {
        Ok(report) => {
            for line in &report.passed {
                println!("ok    {line}");
            }
            for line in &report.problems {
                println!("FAIL  {line}");
            }
            if report.problems.is_empty() {
                println!("{command}: {} checks passed", report.passed.len());
                true
            } else {
                println!("{command}: {} problems", report.problems.len());
                false
            }
        }
        Err(error) => {
            eprintln!("{command}: {error}");
            false
        }
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// What a check found: one line per passed check and per problem.
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) passed: Vec<String>,
    pub(crate) problems: Vec<String>,
}

impl Report {
    pub(crate) fn record(&mut self, ok: bool, line: String) {
        if ok {
            self.passed.push(line);
        } else {
            self.problems.push(line);
        }
    }
}
