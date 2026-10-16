// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ci`: every check a change must pass before it merges (QA-1).
//!
//! `make ci` and CI both run this command, so a local run is the same as CI.
//! Every step runs even after one fails, so one run reports every failure; the
//! command fails if any step failed.
//!
//! The plan checks (`plan/check.py` and its tests, P0-05) need `python3` with
//! PyYAML.

use std::path::Path;
use std::process::Command;

use crate::{in_process_check, print_outcome};

/// One step of the CI run.
enum Step {
    /// A cargo command, run from the workspace root.
    Cargo(&'static [&'static str]),
    /// An xtask check, run in this process.
    Xtask(&'static str),
    /// Another program and its arguments, run from the workspace root.
    Program(&'static str, &'static [&'static str]),
}

/// The steps, cheapest first.
const STEPS: [Step; 13] = [
    Step::Cargo(&["fmt", "--all", "--check"]),
    Step::Program("python3", &["plan/check.py"]),
    Step::Program(
        "python3",
        &[
            "-m",
            "unittest",
            "discover",
            "-s",
            "plan",
            "-p",
            "test_*.py",
        ],
    ),
    Step::Xtask("conventions"),
    Step::Xtask("adr"),
    Step::Xtask("layers"),
    Step::Xtask("licenses"),
    Step::Xtask("attribution"),
    Step::Cargo(&[
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ]),
    Step::Cargo(&["test", "--locked", "--workspace", "--all-features"]),
    Step::Xtask("clippy-fixtures"),
    Step::Cargo(&[
        "deny",
        "--locked",
        "check",
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]),
    Step::Xtask("deny-fixtures"),
];

impl Step {
    fn label(&self) -> String {
        match self {
            Step::Cargo(args) => format!("cargo {}", args.join(" ")),
            Step::Xtask(name) => format!("cargo xtask {name}"),
            Step::Program(program, args) => format!("{program} {}", args.join(" ")),
        }
    }

    fn run(&self, root: &Path) -> bool {
        match self {
            Step::Cargo(args) => {
                let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                self.spawn(Command::new(cargo).args(*args), root)
            }
            Step::Program(program, args) => self.spawn(Command::new(program).args(*args), root),
            Step::Xtask(name) => match in_process_check(name) {
                Some(check) => print_outcome(name, check(root)),
                None => {
                    eprintln!("{name} is not an xtask check");
                    false
                }
            },
        }
    }

    fn spawn(&self, command: &mut Command, root: &Path) -> bool {
        match command.current_dir(root).status() {
            Ok(status) => status.success(),
            Err(error) => {
                eprintln!("cannot run {}: {error}", self.label());
                false
            }
        }
    }
}

/// Runs every step and prints a summary; returns whether all passed.
pub(crate) fn run(root: &Path) -> bool {
    run_steps(root, &STEPS)
}

fn run_steps(root: &Path, steps: &[Step]) -> bool {
    let mut results = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        println!(
            "\n==> [{}/{}] {}",
            index.saturating_add(1),
            steps.len(),
            step.label()
        );
        results.push((step.label(), step.run(root)));
    }
    println!("\n==> cargo xtask ci summary");
    for (label, ok) in &results {
        println!("{}  {label}", if *ok { "ok  " } else { "FAIL" });
    }
    let failed = results.iter().filter(|(_, ok)| !ok).count();
    println!("{}", verdict(failed, results.len()));
    failed == 0
}

fn verdict(failed: usize, total: usize) -> String {
    if failed == 0 {
        format!("ci: all {total} steps passed")
    } else {
        format!("ci: {failed} of {total} steps failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_runs_every_check_the_issue_names() {
        let labels: Vec<String> = STEPS.iter().map(Step::label).collect();
        for expected in [
            "cargo fmt --all --check",
            "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
            "cargo test --locked --workspace --all-features",
            "cargo xtask conventions",
            "cargo xtask adr",
            "cargo xtask clippy-fixtures",
            "cargo xtask deny-fixtures",
            "cargo xtask layers",
            "cargo xtask licenses",
            "cargo xtask attribution",
            "cargo deny --locked check advisories bans licenses sources",
            "python3 plan/check.py",
            "python3 -m unittest discover -s plan -p test_*.py",
        ] {
            assert!(labels.iter().any(|label| label == expected), "{expected}");
        }
    }

    #[test]
    fn every_xtask_step_is_a_known_check() {
        for step in &STEPS {
            if let Step::Xtask(name) = step {
                assert!(in_process_check(name).is_some(), "{name}");
            }
        }
    }

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new("."))
    }

    #[test]
    fn passing_steps_pass() {
        let steps = [
            Step::Cargo(&["--version"]),
            Step::Xtask("layers"),
            Step::Program("python3", &["--version"]),
        ];
        assert!(run_steps(root(), &steps));
    }

    #[test]
    fn one_failing_step_fails_the_run_and_the_rest_still_run() {
        assert!(!run_steps(
            root(),
            &[Step::Cargo(&["no-such-command-xtask-ci"])]
        ));
        assert!(!run_steps(root(), &[Step::Xtask("no-such-check")]));
        assert!(!run_steps(
            root(),
            &[Step::Program("no-such-program-xtask-ci", &[])]
        ));
        let steps = [Step::Xtask("no-such-check"), Step::Cargo(&["--version"])];
        assert!(!run_steps(root(), &steps));
        let steps = [Step::Cargo(&["--version"]), Step::Xtask("no-such-check")];
        assert!(!run_steps(root(), &steps));
    }

    #[test]
    fn the_verdict_counts_failures() {
        assert_eq!(verdict(0, 11), "ci: all 11 steps passed");
        assert_eq!(verdict(2, 11), "ci: 2 of 11 steps failed");
    }
}
