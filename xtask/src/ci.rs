// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ci`: every check a change must pass before it merges (QA-1).
//!
//! `make ci` runs every step. CI runs it as `cargo xtask ci --skip <names>`,
//! leaving out the steps another required job runs with the same command, so
//! each runs once per CI run (`docs/ci.md`). Every step that runs does so even
//! after one fails, so one run reports every failure; the command fails if any
//! step failed.
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
    /// Another program and its arguments, run from the workspace root, after
    /// the name `--skip` knows the step by.
    Program(&'static str, &'static str, &'static [&'static str]),
}

/// The steps, cheapest first.
pub(crate) const STEPS: [Step; 15] = [
    Step::Cargo(&["fmt", "--all", "--check"]),
    Step::Program("plan", "python3", &["plan/check.py"]),
    Step::Program(
        "plan-tests",
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
    Step::Program("specs", "bash", &["scripts/refresh-specs.sh", "--verify"]),
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
        "--all-features",
        "check",
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]),
    Step::Xtask("deny-fixtures"),
];

impl Step {
    /// The name `--skip` takes: the cargo subcommand, the xtask check or the
    /// program step's name.
    fn name(&self) -> &'static str {
        match self {
            Step::Cargo(args) => args.first().copied().unwrap_or_default(),
            Step::Xtask(name) | Step::Program(name, _, _) => name,
        }
    }

    fn label(&self) -> String {
        match self {
            Step::Cargo(args) => format!("cargo {}", args.join(" ")),
            Step::Xtask(name) => format!("cargo xtask {name}"),
            Step::Program(_, program, args) => format!("{program} {}", args.join(" ")),
        }
    }

    fn run(&self, root: &Path) -> bool {
        match self {
            Step::Cargo(args) => {
                let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                self.spawn(Command::new(cargo).args(*args), root)
            }
            Step::Program(_, program, args) => self.spawn(Command::new(program).args(*args), root),
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

const USAGE: &str = "usage: cargo xtask ci [--skip <step>[,<step>...]]";

/// Runs `cargo xtask ci` with the arguments `args`: every step of `steps`
/// except those `--skip` names. Returns whether every step run passed, or an
/// error for a malformed argument or a name no step has.
pub(crate) fn main(root: &Path, args: &[String], steps: &[Step]) -> Result<bool, String> {
    let skip = parse_skip(args, steps)?;
    Ok(run(root, steps, &skip))
}

/// The names of the steps `args` skips; each must name a step of `steps`.
fn parse_skip<'a>(args: &'a [String], steps: &[Step]) -> Result<Vec<&'a str>, String> {
    let list = match args {
        [] => return Ok(Vec::new()),
        [flag, list] if flag == "--skip" => list,
        _ => return Err(USAGE.to_owned()),
    };
    let mut skip = Vec::new();
    for name in list.split(',') {
        if !steps.iter().any(|step| step.name() == name) {
            let names: Vec<&str> = steps.iter().map(Step::name).collect();
            return Err(format!(
                "no step is named {name:?}; the steps are {}\n{USAGE}",
                names.join(", ")
            ));
        }
        skip.push(name);
    }
    Ok(skip)
}

/// What became of one step.
enum Outcome {
    Passed,
    Failed,
    Skipped,
}

/// Runs every step not named in `skip` and prints a summary; returns whether
/// every step run passed.
pub(crate) fn run(root: &Path, steps: &[Step], skip: &[&str]) -> bool {
    let skipped = |step: &Step| skip.contains(&step.name());
    let total = steps.iter().filter(|step| !skipped(step)).count();
    let mut results = Vec::new();
    let mut index = 0_usize;
    for step in steps {
        if skipped(step) {
            results.push((step.label(), Outcome::Skipped));
            continue;
        }
        index = index.saturating_add(1);
        println!("\n==> [{index}/{total}] {}", step.label());
        let outcome = if step.run(root) {
            Outcome::Passed
        } else {
            Outcome::Failed
        };
        results.push((step.label(), outcome));
    }
    println!("\n==> cargo xtask ci summary");
    let mut failed = 0_usize;
    for (label, outcome) in &results {
        let tag = match outcome {
            Outcome::Passed => "ok  ",
            Outcome::Failed => {
                failed = failed.saturating_add(1);
                "FAIL"
            }
            Outcome::Skipped => "skip",
        };
        println!("{tag}  {label}");
    }
    println!("{}", verdict(failed, total));
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
            "cargo deny --locked --all-features check advisories bans licenses sources",
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
            Step::Program("python", "python3", &["--version"]),
        ];
        assert!(run(root(), &steps, &[]));
    }

    #[test]
    fn one_failing_step_fails_the_run_and_the_rest_still_run() {
        assert!(!run(
            root(),
            &[Step::Cargo(&["no-such-command-xtask-ci"])],
            &[]
        ));
        assert!(!run(root(), &[Step::Xtask("no-such-check")], &[]));
        assert!(!run(
            root(),
            &[Step::Program("missing", "no-such-program-xtask-ci", &[])],
            &[]
        ));
        let steps = [Step::Xtask("no-such-check"), Step::Cargo(&["--version"])];
        assert!(!run(root(), &steps, &[]));
        let steps = [Step::Cargo(&["--version"]), Step::Xtask("no-such-check")];
        assert!(!run(root(), &steps, &[]));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn every_step_has_its_own_name() {
        let names: Vec<&str> = STEPS.iter().map(Step::name).collect();
        for (index, name) in names.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(
                !names.get(..index).unwrap_or_default().contains(name),
                "{name}"
            );
        }
        for name in [
            "fmt",
            "clippy",
            "test",
            "deny",
            "plan",
            "plan-tests",
            "specs",
        ] {
            assert!(names.contains(&name), "{name}");
        }
    }

    #[test]
    fn skip_names_exactly_the_steps_it_lists() {
        assert_eq!(parse_skip(&[], &STEPS), Ok(Vec::new()));
        let skip = args(&["--skip", "fmt,clippy,test,deny"]);
        let names = parse_skip(&skip, &STEPS).unwrap_or_default();
        assert_eq!(names, ["fmt", "clippy", "test", "deny"]);
        let skip = args(&["--skip", "clippy-fixtures"]);
        assert_eq!(parse_skip(&skip, &STEPS), Ok(vec!["clippy-fixtures"]));
    }

    #[test]
    fn skip_rejects_an_unknown_step_and_a_malformed_argument() {
        for bad in [
            &["--skip", "no-such-step"][..],
            &["--skip", "fmt,no-such-step"],
            &["--skip", ""],
            &["--skip", "fmt,"],
            &["--skip", "cargo fmt --all --check"],
        ] {
            let error = parse_skip(&args(bad), &STEPS).err().unwrap_or_default();
            assert!(error.contains("no step is named"), "{bad:?}: {error}");
            assert!(error.contains("fmt, plan, plan-tests"), "{bad:?}: {error}");
        }
        for bad in [
            &["--skip"][..],
            &["--only", "fmt"],
            &["fmt"],
            &["--skip", "fmt", "--skip", "test"],
        ] {
            assert_eq!(
                parse_skip(&args(bad), &STEPS),
                Err(USAGE.to_owned()),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_skipped_step_does_not_run_and_the_others_do() {
        let steps = [Step::Cargo(&["--version"]), Step::Xtask("no-such-check")];
        assert!(main(root(), &args(&["--skip", "no-such-check"]), &steps).unwrap_or(false));
        assert!(!main(root(), &args(&["--skip", "--version"]), &steps).unwrap_or(true));
        assert!(!main(root(), &[], &steps).unwrap_or(true));
        let steps = [
            Step::Xtask("no-such-check"),
            Step::Program("missing", "no-such-program-xtask-ci", &[]),
        ];
        assert!(!main(root(), &args(&["--skip", "no-such-check"]), &steps).unwrap_or(true));
        assert!(!main(root(), &args(&["--skip", "missing"]), &steps).unwrap_or(true));
        let skip = args(&["--skip", "missing,no-such-check"]);
        assert!(main(root(), &skip, &steps).unwrap_or(false));
        assert!(main(root(), &args(&["--skip", "nothing"]), &steps).is_err());
    }

    #[test]
    fn make_ci_runs_every_step() {
        let makefile = std::fs::read_to_string(root().join("Makefile")).unwrap_or_default();
        let recipe: Vec<&str> = makefile
            .lines()
            .skip_while(|line| !line.starts_with("ci:"))
            .skip(1)
            .take_while(|line| line.starts_with('\t'))
            .collect();
        assert_eq!(recipe, ["\t$(CARGO) xtask ci"]);
    }

    /// Every step CI's `cargo xtask ci` job skips is a `run:` line of
    /// `ci.yml` with the step's exact command, so another job runs it.
    #[test]
    fn every_step_ci_skips_runs_in_another_job_with_the_same_command() {
        let workflow =
            std::fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap_or_default();
        let skip_line = workflow
            .lines()
            .find(|line| line.trim_start().starts_with("SKIP: "))
            .unwrap_or_default();
        let quoted: Vec<&str> = skip_line.split('\'').skip(1).step_by(2).collect();
        assert_eq!(
            quoted,
            ["refs/heads/main", "clippy,test", "fmt,clippy,test,deny"]
        );
        let lists = quoted.get(1..).unwrap_or_default();
        for list in lists {
            for name in parse_skip(&args(&["--skip", list]), &STEPS).unwrap_or_default() {
                let step = STEPS.iter().find(|step| step.name() == name);
                let label = step.map(Step::label).unwrap_or_default();
                assert!(workflow.contains(&format!("run: {label}\n")), "{label}");
            }
        }
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
