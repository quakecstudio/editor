// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask`: build automation for the workspace.
//!
//! - `conventions`: checks every config value against `docs/conventions.md`.
//! - `clippy-fixtures`: proves the workspace lints reject the code they target.
//! - `deny-fixtures`: proves `deny.toml` rejects incompatible licenses and
//!   wildcard versions (needs network access to the crates.io index).

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a command-line tool; this is its crate root"
)]

mod conventions;
mod fixtures;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <conventions | clippy-fixtures | deny-fixtures>";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(command), None) = (args.next(), args.next()) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let root = workspace_root();
    let result = match command.as_str() {
        "conventions" => conventions::check(&root),
        "clippy-fixtures" => fixtures::clippy(&root),
        "deny-fixtures" => fixtures::deny(&root),
        _ => {
            eprintln!("unknown command {command:?}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
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
                ExitCode::SUCCESS
            } else {
                println!("{command}: {} problems", report.problems.len());
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{command}: {error}");
            ExitCode::FAILURE
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
