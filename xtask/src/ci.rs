// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ci`: every check a change must pass before it merges (QA-1).
//!
//! `make ci` and CI both run this command, so a local run is the same as CI.
//! Every step runs even after one fails, so one run reports every failure; the
//! command fails if any step failed.
//!
//! The plan checks (`plan/check.py` and its tests, P0-05) need `python3` with
//! PyYAML. The cached-spec check (`scripts/refresh-specs.sh --verify`, P0-26)
//! needs `bash` and `sha256sum` or `shasum`, and no network.

use std::path::Path;
use std::process::Command;

use crate::{in_process_check, print_outcome};

/// One step of the CI run.
pub(crate) enum Step {
    /// A cargo command, run from the workspace root.
    Cargo(&'static [&'static str]),
    /// An xtask check, run in this process.
    Xtask(&'static str),
    /// Another program and its arguments, run from the workspace root.
    Program(&'static str, &'static [&'static str]),
}

/// The steps, cheapest first.
pub(crate) const STEPS: [Step; 15] = [
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
    Step::Program("bash", &["scripts/refresh-specs.sh", "--verify"]),
    Step::Xtask("conventions"),
    Step::Xtask("adr"),
    Step::Xtask("layers"),
    Step::Xtask("licenses"),
    Step::Xtask("attribution"),
    Step::Xtask("contrast"),
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
pub(crate) fn run(root: &Path, steps: &[Step]) -> bool {
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
            "cargo xtask contrast",
            "cargo deny --locked check advisories bans licenses sources",
            "python3 plan/check.py",
            "python3 -m unittest discover -s plan -p test_*.py",
            "bash scripts/refresh-specs.sh --verify",
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
        assert!(run(root(), &steps));
    }

    #[test]
    fn one_failing_step_fails_the_run_and_the_rest_still_run() {
        assert!(!run(root(), &[Step::Cargo(&["no-such-command-xtask-ci"])]));
        assert!(!run(root(), &[Step::Xtask("no-such-check")]));
        assert!(!run(
            root(),
            &[Step::Program("no-such-program-xtask-ci", &[])]
        ));
        let steps = [Step::Xtask("no-such-check"), Step::Cargo(&["--version"])];
        assert!(!run(root(), &steps));
        let steps = [Step::Cargo(&["--version"]), Step::Xtask("no-such-check")];
        assert!(!run(root(), &steps));
    }

    #[test]
    fn the_verdict_counts_failures() {
        assert_eq!(verdict(0, 11), "ci: all 11 steps passed");
        assert_eq!(verdict(2, 11), "ci: 2 of 11 steps failed");
    }

    /// `scripts/refresh-specs.sh --verify` run on a copy of the script and a
    /// one-file cache, so the failures CI must report are exercised without
    /// touching `specs/`.
    #[cfg(unix)]
    mod verify_specs {
        #![expect(
            clippy::unwrap_used,
            reason = "test code: a failed unwrap is a failed test"
        )]

        use std::fs;
        use std::path::PathBuf;

        use super::*;

        const SPEC: &str = "specs/demo/1.0/page.md";
        const TEXT: &str = "The server MUST reply.\n";
        /// SHA-256 of `TEXT`.
        const SHA: &str = "c787ce0f8a9a52b4e471a68e684d7c836d364336b082f7b2f9818c235faa2c36";

        /// A scratch repository holding the script, a README, a manifest with
        /// one row for `SPEC`, and `SPEC` itself containing `text`.
        fn fixture(name: &str, text: &str) -> PathBuf {
            let dir = root()
                .join("target")
                .join("xtask-tests")
                .join("verify-specs")
                .join(name);
            if dir.exists() {
                fs::remove_dir_all(&dir).unwrap();
            }
            fs::create_dir_all(dir.join("scripts")).unwrap();
            fs::create_dir_all(dir.join("specs/demo/1.0")).unwrap();
            fs::copy(
                root().join("scripts/refresh-specs.sh"),
                dir.join("scripts/refresh-specs.sh"),
            )
            .unwrap();
            fs::write(dir.join("specs/README.md"), "# Cached specifications\n").unwrap();
            fs::write(
                dir.join("specs/manifest.tsv"),
                format!(
                    "path\tspec\turl\tsha256\tbytes\tfetched\n\
                     {SPEC}\tDemo 1.0\thttps://example.com/page.md\t{SHA}\t{}\t2026-10-15\n",
                    TEXT.len()
                ),
            )
            .unwrap();
            fs::write(dir.join(SPEC), text).unwrap();
            dir
        }

        /// Runs the verify step in `dir`; returns whether it passed and its stderr.
        fn verify(dir: &Path) -> (bool, String) {
            let output = Command::new("bash")
                .args(["scripts/refresh-specs.sh", "--verify"])
                .current_dir(dir)
                .output()
                .unwrap();
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        }

        #[test]
        fn a_cache_that_matches_its_manifest_passes() {
            let (ok, stderr) = verify(&fixture("clean", TEXT));
            assert!(ok, "{stderr}");
            assert!(stderr.contains("every cached file matches"), "{stderr}");
        }

        #[test]
        fn one_changed_byte_fails_and_names_the_file() {
            let (ok, stderr) = verify(&fixture("changed", "The server MAY reply.\n"));
            assert!(!ok);
            assert!(stderr.contains(&format!("{SPEC}: sha256 is")), "{stderr}");
        }

        #[test]
        fn a_file_without_a_manifest_row_fails_and_names_the_file() {
            let dir = fixture("unlisted", TEXT);
            fs::write(dir.join("specs/demo/1.0/extra.md"), "extra\n").unwrap();
            let (ok, stderr) = verify(&dir);
            assert!(!ok);
            assert!(
                stderr.contains("specs/demo/1.0/extra.md: cached but not in the manifest"),
                "{stderr}"
            );
        }

        #[test]
        fn a_listed_file_that_is_missing_fails_and_names_the_file() {
            let dir = fixture("missing", TEXT);
            fs::remove_file(dir.join(SPEC)).unwrap();
            let (ok, stderr) = verify(&dir);
            assert!(!ok);
            assert!(
                stderr.contains(&format!("{SPEC}: listed in the manifest but missing")),
                "{stderr}"
            );
        }
    }
}
