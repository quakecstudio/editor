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
//! - `bench`: runs the performance-budget benchmarks, checks them against
//!   `docs/perf-budgets.md` and the last run, and records them in the JSON
//!   history; `bench history` prints the history.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a command-line tool; this is its crate root"
)]

mod adr;
mod attribution;
mod bench;
mod ci;
mod conventions;
mod fixtures;
mod layers;
mod licenses;
mod ported;
mod repo;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci | adr | conventions | clippy-fixtures | \
                     deny-fixtures | layers | licenses | attribution | bench>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    if let Some(("bench", rest)) = args
        .split_first()
        .map(|(command, rest)| (command.as_str(), rest))
    {
        return match bench::main(&root, rest) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("bench: {error}");
                ExitCode::FAILURE
            }
        };
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
