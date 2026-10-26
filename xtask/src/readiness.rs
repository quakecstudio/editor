// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask readiness`: measures one readiness check for a list of crates
//! (`docs/testing.md`, ADR-0009) and writes the numbers as a JSON file that
//! `cargo xtask ratchet` judges.
//!
//! | Check | Tool | Passed of total |
//! | --- | --- | --- |
//! | `lines` | `cargo llvm-cov` on the pinned stable toolchain | lines covered (TEST-1) |
//! | `branches` | `cargo llvm-cov --branch` on [`NIGHTLY`] | branches covered (TEST-1) |
//! | `mutation` | `cargo mutants` | mutants caught or timed out, of those that built (TEST-2) |
//! | `spec` | the crate README, `docs/spec/requirements.tsv` and `docs/spec/coverage-baseline.tsv` | active, non-baseline statements cited by a `// spec` test (TEST-3) |
//! | `flakes` | `cargo test`, [`FLAKE_RUNS`] times | passing runs (TEST-7) |
//!
//! A crate is named by its package name (a workspace member) or by its
//! directory, which is how the fixtures under `xtask/fixtures/readiness/` are
//! measured. Each crate's numbers come from its own tests. Measuring never
//! fails because a number is low; only a tool that cannot run fails it. The
//! coverage checks run property tests on a fixed seed ([`PROPTEST_FIXED`]).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exec::{Exec, System};
use crate::host::cargo_host;
use crate::oracles::{COVERAGE_ENV, vm_oracle};
use crate::test_code::{FIXTURES, TestCode, shipped_citation, test_code};

/// The date-pinned nightly that measures branch coverage, the only check that
/// needs nightly. The `Readiness` workflow installs the same toolchain, and a
/// test keeps the two equal (`docs/conventions.md`).
pub(crate) const NIGHTLY: &str = "nightly-2026-10-01";

/// Consecutive passing runs the flake check needs on each OS (TEST-7).
pub(crate) const FLAKE_RUNS: u64 = 20;

const USAGE: &str = "usage: cargo xtask readiness <lines | branches | mutation | spec | flakes> \
                     --out <file.json> [--os <name>] <crate>...";

/// One readiness check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Check {
    Lines,
    Branches,
    Mutation,
    Spec,
    Flakes,
}

impl Check {
    pub(crate) const ALL: [Check; 5] = [
        Check::Lines,
        Check::Branches,
        Check::Mutation,
        Check::Spec,
        Check::Flakes,
    ];

    pub(crate) fn parse(name: &str) -> Option<Check> {
        Check::ALL.into_iter().find(|check| check.name() == name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Check::Lines => "lines",
            Check::Branches => "branches",
            Check::Mutation => "mutation",
            Check::Spec => "spec",
            Check::Flakes => "flakes",
        }
    }

    /// The bar in ADR-0009, as a whole percentage.
    pub(crate) fn threshold(self) -> u64 {
        match self {
            Check::Lines | Check::Mutation => 90,
            Check::Branches => 80,
            Check::Spec | Check::Flakes => 100,
        }
    }
}

/// What one check measured for one crate: `passed` of `total`, and what did
/// not pass (uncovered nothing is listed; surviving mutants, uncited
/// statements and failing runs are).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Score {
    pub(crate) passed: u64,
    pub(crate) total: u64,
    #[serde(default)]
    pub(crate) failures: Vec<String>,
}

/// The JSON file one measurement writes: one check, on one OS, for each crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Measurement {
    pub(crate) check: Check,
    pub(crate) os: String,
    pub(crate) crates: BTreeMap<String, Score>,
}

/// A crate to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    name: String,
    dir: PathBuf,
    kind: Kind,
}

/// What kind of code a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A member of the workspace.
    Member,
    /// A crate that is its own workspace (a fixture), so it gets its own
    /// target directory under `target/xtask/readiness/`.
    Standalone,
    /// The C of the oracles, [`ORACLES`]: the C tools layer's own code, which
    /// `cargo xtask oracles` builds and checks.
    Oracles,
}

/// The name, and the directory, of the C tools layer's code (`oracles/README.md`).
pub(crate) const ORACLES: &str = "oracles";

/// Where the corpus codebase id 1.01 is, under `QS_TEST_ASSETS`, for the oracles' coverage.
const ID101: &str = "codebase/id-1.01";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    check: Check,
    out: PathBuf,
    os: String,
    crates: Vec<String>,
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let tools = Tools {
        exec: &System,
        assets: std::env::var_os("QS_TEST_ASSETS"),
        scratch: scratch(root),
    };
    run_check(root, args, &tools)
}

/// What a measurement runs: the programs `exec` runs, the corpus directory
/// `QS_TEST_ASSETS` names, for the oracles' coverage, and the directory the
/// tools write their reports and builds in, [`scratch`] (a test gives each
/// measurement its own).
struct Tools<'a> {
    exec: &'a dyn Exec,
    assets: Option<OsString>,
    scratch: PathBuf,
}

/// [`main`] with these tools.
fn run_check(root: &Path, args: &[String], tools: &Tools<'_>) -> Result<bool, String> {
    let options = parse_args(args)?;
    // The tools write their reports here and do not create it themselves.
    let scratch = &tools.scratch;
    fs::create_dir_all(scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let mut measurement = Measurement {
        check: options.check,
        os: options.os,
        crates: BTreeMap::new(),
    };
    for name in &options.crates {
        let target = resolve(root, name)?;
        let Some(score) = measure(root, options.check, &target, tools)? else {
            println!(
                "{} {}: not measured: {NO_C_MUTATION}",
                target.name,
                options.check.name()
            );
            continue;
        };
        println!(
            "{} {}: {}",
            target.name,
            options.check.name(),
            describe(&score)
        );
        measurement.crates.insert(target.name, score);
    }
    let mut text = serde_json::to_string_pretty(&measurement)
        .map_err(|error| format!("cannot serialise the measurement: {error}"))?;
    text.push('\n');
    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&options.out, text).map_err(|error| format!("{}: {error}", options.out.display()))?;
    println!("readiness: wrote {}", options.out.display());
    Ok(true)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut iter = args.iter();
    let Some(check) = iter.next().and_then(|name| Check::parse(name)) else {
        return Err(USAGE.to_owned());
    };
    let mut out = None;
    let mut os = std::env::consts::OS.to_owned();
    let mut crates = Vec::new();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(iter.next().ok_or(USAGE)?)),
            "--os" => iter.next().ok_or(USAGE)?.clone_into(&mut os),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            name => crates.push(name.to_owned()),
        }
    }
    let Some(out) = out else {
        return Err(format!("--out is required\n{USAGE}"));
    };
    if crates.is_empty() {
        return Err(format!("name at least one crate\n{USAGE}"));
    }
    Ok(Options {
        check,
        out,
        os,
        crates,
    })
}

/// A package name of a workspace member, a crate directory inside the
/// repository, or [`ORACLES`].
fn resolve(root: &Path, name: &str) -> Result<Target, String> {
    if name.trim_end_matches(['/', '\\']) == ORACLES {
        return Ok(Target {
            name: ORACLES.to_owned(),
            dir: root.join(ORACLES),
            kind: Kind::Oracles,
        });
    }
    let dir = if name.contains('/') || name.contains('\\') {
        let dir = root
            .join(name)
            .canonicalize()
            .map_err(|error| format!("{name}: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("{}: {error}", root.display()))?;
        if !dir.starts_with(&root) {
            return Err(format!("{name} is outside the repository"));
        }
        dir
    } else {
        member_dir(root, name)?
    };
    let manifest = fs::read_to_string(dir.join("Cargo.toml"))
        .map_err(|error| format!("{name}: no Cargo.toml: {error}"))?;
    let manifest: toml::Table =
        toml::from_str(&manifest).map_err(|error| format!("{name}/Cargo.toml: {error}"))?;
    let Some(package) = manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Err(format!("{name}/Cargo.toml has no package name"));
    };
    let kind = if manifest.contains_key("workspace") {
        Kind::Standalone
    } else {
        Kind::Member
    };
    Ok(Target {
        name: package.to_owned(),
        dir,
        kind,
    })
}

fn member_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let output = Command::new(cargo())
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata: {error}"))?;
    metadata
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|package| package.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|package| package.get("manifest_path").and_then(Value::as_str))
        .and_then(|manifest| Path::new(manifest).parent().map(Path::to_path_buf))
        .ok_or_else(|| format!("{name} is not a workspace crate"))
}

/// Why the oracles have no mutation score: the exception `ratchets.toml` and
/// `oracles/README.md` record.
const NO_C_MUTATION: &str = "cargo mutants mutates Rust, and the oracles are C; \
                             their exception is in ratchets.toml";

/// The score of `check` for `target`, or `None` when no tool measures it
/// (the oracles' mutation score).
fn measure(
    root: &Path,
    check: Check,
    target: &Target,
    tools: &Tools<'_>,
) -> Result<Option<Score>, String> {
    let score = match (check, target.kind) {
        (Check::Lines | Check::Branches, Kind::Oracles) => c_coverage(root, check, tools)?,
        (Check::Lines | Check::Branches, Kind::Member | Kind::Standalone) => {
            coverage(root, target, check, tools)?
        }
        (Check::Mutation, Kind::Oracles) => return Ok(None),
        (Check::Mutation, Kind::Member | Kind::Standalone) => mutation(root, target, tools)?,
        (Check::Spec, _) => spec(&target.dir)?,
        (Check::Flakes, Kind::Oracles) => oracle_flakes(root, tools.exec)?,
        (Check::Flakes, Kind::Member | Kind::Standalone) => flakes(root, target, tools)?,
    };
    Ok(Some(score))
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// A cargo command for `target`, with its own target directory in `scratch` if
/// it is a standalone fixture.
fn cargo_for(root: &Path, scratch: &Path, target: &Target, toolchain: Option<&str>) -> Command {
    let mut command = match toolchain {
        // `+toolchain` needs rustup's cargo proxy, not the cargo running xtask.
        Some(toolchain) => {
            let mut command = Command::new("cargo");
            command.arg(format!("+{toolchain}"));
            command
        }
        None => Command::new(cargo()),
    };
    command.current_dir(root);
    if target.kind == Kind::Standalone {
        command.env("CARGO_TARGET_DIR", scratch.join(&target.name));
    }
    command
}

fn scratch(root: &Path) -> PathBuf {
    root.join("target").join("xtask").join("readiness")
}

fn manifest(target: &Target) -> PathBuf {
    target.dir.join("Cargo.toml")
}

fn run(exec: &dyn Exec, command: &mut Command, what: &str) -> Result<std::process::Output, String> {
    exec.output(command)
        .map_err(|error| format!("cannot run {what}: {error}"))
}

/// The proptest settings the coverage checks run with, so that a line or
/// branch reached only by a generated case counts the same on every run.
/// proptest 1.11 reads each from the environment (`test_runner/config.rs`,
/// `Config`): "Seed used for the RNG. Set by using the PROPTEST_RNG_SEED
/// environment variable"; the RNG algorithm "can be overridden by setting the
/// `PROPTEST_RNG_ALGORITHM` environment variable", whose value `cc` selects
/// `RngAlgorithm::ChaCha`; and for the case count, "The default is 256, which
/// can be overridden by setting the `PROPTEST_CASES` environment variable". A
/// test that sets its own case count keeps it. `cargo test`, mutation and flake
/// runs set none of these and keep proptest's random seed.
pub(crate) const PROPTEST_FIXED: [(&str, &str); 3] = [
    ("PROPTEST_RNG_SEED", "461"),
    ("PROPTEST_RNG_ALGORITHM", "cc"),
    ("PROPTEST_CASES", "256"),
];

/// Runs `command`'s property tests on [`PROPTEST_FIXED`] alone. proptest's
/// `contextualize_config` reads every `PROPTEST_*` variable, and some of the
/// others (`PROPTEST_MAX_DEFAULT_SIZE_RANGE`, the reject limits) change what
/// is generated, so each one in `inherited`, the names in xtask's environment,
/// is removed.
fn fix_proptest(command: &mut Command, inherited: impl IntoIterator<Item = OsString>) {
    for name in inherited {
        if name.to_string_lossy().starts_with("PROPTEST_") {
            command.env_remove(name);
        }
    }
    command.envs(PROPTEST_FIXED);
}

fn coverage(
    root: &Path,
    target: &Target,
    check: Check,
    tools: &Tools<'_>,
) -> Result<Score, String> {
    let report = tools
        .scratch
        .join(format!("{}-{}.json", check.name(), target.name));
    let branches = check == Check::Branches;
    let mut command = cargo_for(root, &tools.scratch, target, branches.then_some(NIGHTLY));
    fix_proptest(&mut command, std::env::vars_os().map(|(name, _)| name));
    command
        .args(["llvm-cov", "--locked", "--all-features", "--manifest-path"])
        .arg(manifest(target))
        .args(["--json", "--summary-only", "--output-path"])
        .arg(&report);
    if branches {
        command.arg("--branch");
    }
    let output = run(tools.exec, &mut command, "cargo llvm-cov")?;
    if !output.status.success() {
        return Err(format!(
            "{}: cargo llvm-cov failed (a failing test fails it too):\n{}",
            target.name,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text =
        fs::read_to_string(&report).map_err(|error| format!("{}: {error}", report.display()))?;
    parse_llvm_cov(&text, check)
}

/// Lines or branches from `cargo llvm-cov --json --summary-only`. The totals
/// cover only the measured package's files.
pub(crate) fn parse_llvm_cov(text: &str, check: Check) -> Result<Score, String> {
    let key = match check {
        Check::Lines => "lines",
        Check::Branches => "branches",
        Check::Mutation | Check::Spec | Check::Flakes => {
            return Err(format!("llvm-cov does not measure {}", check.name()));
        }
    };
    let json: Value =
        serde_json::from_str(text).map_err(|error| format!("llvm-cov report: {error}"))?;
    let totals = json
        .get("data")
        .and_then(|data| data.get(0))
        .and_then(|data| data.get("totals"))
        .and_then(|totals| totals.get(key))
        .ok_or_else(|| format!("llvm-cov report has no data[0].totals.{key}"))?;
    let number = |field: &str| {
        totals
            .get(field)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("llvm-cov report: totals.{key}.{field} is not a count"))
    };
    let (covered, total) = (number("covered")?, number("count")?);
    if covered > total {
        return Err(format!(
            "llvm-cov report: {covered} of {total} {key} covered"
        ));
    }
    Ok(Score {
        passed: covered,
        total,
        failures: Vec::new(),
    })
}

/// The least time, in seconds, cargo mutants gives a mutant's tests. Measured
/// on a 10-core Mac at `--jobs 4` with the `mutants` profile, the slowest test
/// run of any caught or missed mutant in the four L2 crates took 3.0 s.
const MUTANT_TIMEOUT_FLOOR: &str = "10";

/// The data memory, in bytes, each test binary of a mutant may use where the
/// host enforces it ([`mutant_runner`]): 3 GiB. A mutant that stops a reader
/// advancing can make a test take a 16 GB runner's memory before its timeout
/// ends it, and the runner then loses its agent (P2-109); past this limit the
/// allocation fails and aborts the binary instead, which counts as caught, and
/// four such jobs together stay inside the runner.
const MUTANT_DATA_LIMIT: u64 = 3 * 1024 * 1024 * 1024;

/// cargo's runner for the test binaries of a mutation run on `host`, as the
/// environment variable that sets it, or `None` where the host does not
/// enforce the limit. On Linux, `prlimit --data` (util-linux) sets
/// `RLIMIT_DATA` to [`MUTANT_DATA_LIMIT`] and `--core=0` writes no core file
/// for a binary it aborts; a single value "Specify[s] both limits to the same
/// value" (prlimit(1), <https://man7.org/linux/man-pages/man1/prlimit.1.html>),
/// so a process the test starts cannot raise it. macOS does not enforce
/// `RLIMIT_DATA` and has no prlimit, and Windows has neither.
///
/// cargo reads the runner for a target from `CARGO_TARGET_<triple>_RUNNER`:
/// "Keys are converted to uppercase, dots and dashes are converted to
/// underscores" (The Cargo Book, Configuration, "Environment variables"), and
/// "If a runner is provided, executables for the target <triple> will be
/// executed by invoking the specified runner with the actual executable passed
/// as an argument. This applies to cargo run, cargo test and cargo bench
/// commands" (`target.<triple>.runner`; both at
/// <https://doc.rust-lang.org/cargo/reference/config.html>). cargo mutants runs
/// each mutant's tests with `cargo test`, so every test binary gets the runner.
pub(crate) fn mutant_runner(host: &str) -> Option<(String, String)> {
    if !host.contains("-linux-") {
        return None;
    }
    let name = format!(
        "CARGO_TARGET_{}_RUNNER",
        host.to_ascii_uppercase().replace('-', "_")
    );
    let value = format!("prlimit --data={MUTANT_DATA_LIMIT} --core=0 --");
    Some((name, value))
}

fn mutation(root: &Path, target: &Target, tools: &Tools<'_>) -> Result<Score, String> {
    let out = tools.scratch.join(format!("mutants-{}", target.name));
    let mut command = cargo_for(root, &tools.scratch, target, None);
    // Each test binary under the data limit, where the host enforces one. The
    // limit is set here, on the triple cargo reports for the host, rather than
    // in the workflow, so a local run and an arm64 runner get it too.
    if let Some((name, value)) = mutant_runner(&cargo_host(root, tools.exec)?) {
        command.env(name, value);
    }
    // cargo mutants tests a copy of the tree, which without `.git` fails the
    // tests that read the repository through git (xtask's licenses,
    // attribution and corpus checks), so no mutant would be tested.
    // One build directory per core: a crate of a few thousand mutants (xtask)
    // tested one at a time outlasts the workflow's time limit.
    let jobs = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    // Incremental builds: each mutant rebuilds one crate, and CI's rust-cache
    // sets CARGO_INCREMENTAL=0, which would make every rebuild a full one.
    command.env("CARGO_INCREMENTAL", "1").args([
        "mutants",
        "--all-features",
        "--no-shuffle",
        "--copy-vcs=true",
    ]);
    // A member builds with the workspace's `mutants` profile (Cargo.toml),
    // which a standalone crate, its own workspace, does not have. The test
    // timeout stays five times the baseline's, with a floor of
    // MUTANT_TIMEOUT_FLOOR seconds rather than cargo-mutants' 20, measured
    // with that profile: a mutant that hangs a fast suite is cut off sooner,
    // and it still counts as caught.
    if target.kind == Kind::Member {
        command.args([
            "--profile",
            "mutants",
            "--minimum-test-timeout",
            MUTANT_TIMEOUT_FLOOR,
        ]);
    }
    command
        .arg("--manifest-path")
        .arg(manifest(target))
        .arg("--jobs")
        .arg(jobs.to_string())
        .arg("--output")
        .arg(&out);
    let output = run(tools.exec, &mut command, "cargo mutants")?;
    // 0: every mutant caught; 2: some missed; 3: some timed out. Anything else
    // (a usage error, a failing baseline) means there is no score.
    if !matches!(output.status.code(), Some(0 | 2 | 3)) {
        return Err(format!(
            "{}: cargo mutants failed ({}):\n{}{}",
            target.name,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let outcomes = out.join("mutants.out").join("outcomes.json");
    let text = fs::read_to_string(&outcomes)
        .map_err(|error| format!("{}: {error}", outcomes.display()))?;
    parse_mutants(&text)
}

/// The score from cargo-mutants' `outcomes.json`. A caught or timed-out
/// mutant passes, a missed one fails and is listed by [`mutant_key`]; an
/// unviable mutant (it does not build) and the unmutated baseline do not count.
pub(crate) fn parse_mutants(text: &str) -> Result<Score, String> {
    let json: Value =
        serde_json::from_str(text).map_err(|error| format!("mutants outcomes: {error}"))?;
    let outcomes = json
        .get("outcomes")
        .and_then(Value::as_array)
        .ok_or("mutants outcomes: no outcomes array")?;
    let mut score = Score {
        passed: 0,
        total: 0,
        failures: Vec::new(),
    };
    for outcome in outcomes {
        let summary = outcome
            .get("summary")
            .and_then(Value::as_str)
            .ok_or("mutants outcomes: an outcome has no summary")?;
        let name = outcome
            .get("scenario")
            .and_then(|scenario| scenario.get("Mutant"))
            .and_then(|mutant| mutant.get("name"))
            .and_then(Value::as_str);
        let Some(name) = name else {
            continue;
        };
        match summary {
            "CaughtMutant" | "Timeout" => {
                score.passed = score.passed.saturating_add(1);
                score.total = score.total.saturating_add(1);
            }
            "MissedMutant" => {
                score.total = score.total.saturating_add(1);
                score.failures.push(mutant_key(name));
            }
            "Unviable" => {}
            other => {
                return Err(format!(
                    "mutants outcomes: unknown summary {other:?} for {name}"
                ));
            }
        }
    }
    Ok(score)
}

/// A mutant's name without its line and column, so a recorded equivalent
/// mutant still matches after unrelated edits move it:
/// `src/lib.rs:7:5: replace add -> u32 with 0` becomes
/// `src/lib.rs: replace add -> u32 with 0`.
pub(crate) fn mutant_key(name: &str) -> String {
    let Some((location, description)) = name.split_once(": ") else {
        return name.to_owned();
    };
    let mut file = location;
    for _ in 0..2 {
        if let Some((rest, number)) = file.rsplit_once(':')
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit())
        {
            file = rest;
        }
    }
    format!("{file}: {description}")
}

fn flakes(root: &Path, target: &Target, tools: &Tools<'_>) -> Result<Score, String> {
    let test = |no_run: bool| {
        let mut command = cargo_for(root, &tools.scratch, target, None);
        command
            .args(["test", "--locked", "--all-features", "--manifest-path"])
            .arg(manifest(target));
        if no_run {
            command.arg("--no-run");
        }
        run(tools.exec, &mut command, "cargo test")
    };
    let build = test(true)?;
    if !build.status.success() {
        return Err(format!(
            "{}: the tests do not build:\n{}",
            target.name,
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    repeat(FLAKE_RUNS, || {
        test(false).map(|output| output.status.success())
    })
}

/// `cargo xtask <args>`, run as this program so that nothing is rebuilt.
fn xtask(root: &Path, args: &[&str]) -> Result<Command, String> {
    let exe = std::env::current_exe().map_err(|error| format!("cannot find xtask: {error}"))?;
    let mut command = Command::new(exe);
    command.args(args).current_dir(root);
    Ok(command)
}

/// Runs `command`, failing with its output unless it succeeds.
fn run_ok(exec: &dyn Exec, command: &mut Command, what: &str) -> Result<(), String> {
    let output = run(exec, command, what)?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{what} failed ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// The oracles' flake check: `cargo xtask oracles`, every vector, number
/// vector and probe through the VM oracle, [`FLAKE_RUNS`] times after one build.
fn oracle_flakes(root: &Path, exec: &dyn Exec) -> Result<Score, String> {
    run_ok(
        exec,
        &mut xtask(root, &[ORACLES, "build"])?,
        "cargo xtask oracles build",
    )?;
    repeat(FLAKE_RUNS, || {
        run(exec, &mut xtask(root, &[ORACLES])?, "cargo xtask oracles")
            .map(|output| output.status.success())
    })
}

/// The line or branch coverage of our C (`oracles/vm`): the VM oracle built
/// with clang's source-based coverage, run on every vector, number vector and
/// probe (`cargo xtask oracles`) and on the id 1.01 scenarios
/// (`cargo xtask oracles id101`, from the corpus under `QS_TEST_ASSETS`).
/// id's files in the build are not ours and are not counted.
fn c_coverage(root: &Path, check: Check, tools: &Tools<'_>) -> Result<Score, String> {
    let exec = tools.exec;
    let assets = tools.assets.as_ref().ok_or(
        "the oracles' coverage runs id 1.01: set QS_TEST_ASSETS and run \
         cargo xtask corpus fetch id-1.01",
    )?;
    let id101 = Path::new(&assets).join(ID101);
    if !id101.join("progs.src").is_file() {
        return Err(format!(
            "no id 1.01 in {}: run cargo xtask corpus fetch id-1.01",
            id101.display()
        ));
    }
    let dir = tools.scratch.join(format!("{}-{ORACLES}", check.name()));
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let profiles = dir.join("profiles");
    fs::create_dir_all(&profiles).map_err(|error| format!("{}: {error}", profiles.display()))?;
    // `%m` merges every run of one binary into one profile.
    let profile_file = profiles.join("vm-oracle-%m.profraw");
    let id101 = id101.display().to_string();
    for args in [vec![ORACLES], vec![ORACLES, "id101", &id101]] {
        let mut command = xtask(root, &args)?;
        command
            .env(COVERAGE_ENV, "1")
            .env("LLVM_PROFILE_FILE", &profile_file);
        run_ok(
            exec,
            &mut command,
            &format!("cargo xtask {}", args.join(" ")),
        )?;
    }
    let mut raw = Vec::new();
    for entry in
        fs::read_dir(&profiles).map_err(|error| format!("{}: {error}", profiles.display()))?
    {
        raw.push(
            entry
                .map_err(|error| format!("{}: {error}", profiles.display()))?
                .path(),
        );
    }
    if raw.is_empty() {
        return Err(format!(
            "the VM oracle wrote no profile in {}",
            profiles.display()
        ));
    }
    let merged = dir.join("vm-oracle.profdata");
    let mut merge = llvm_tool("QS_ORACLE_LLVM_PROFDATA", "llvm-profdata");
    merge
        .args(["merge", "-sparse", "-o"])
        .arg(&merged)
        .args(&raw);
    run_ok(exec, &mut merge, "llvm-profdata merge")?;
    let sources = c_sources(&root.join(ORACLES))?;
    let mut export = llvm_tool("QS_ORACLE_LLVM_COV", "llvm-cov");
    export
        .args(["export", "-summary-only", "-instr-profile"])
        .arg(&merged)
        .arg(vm_oracle(root))
        .args(&sources);
    let output = run(exec, &mut export, "llvm-cov export")?;
    if !output.status.success() {
        return Err(format!(
            "llvm-cov export failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let report = dir.join("llvm-cov.json");
    fs::write(&report, text.as_bytes())
        .map_err(|error| format!("{}: {error}", report.display()))?;
    parse_llvm_cov(&text, check)
}

/// An LLVM tool: the program the environment variable `var` names, or `name`,
/// through `xcrun` on macOS, where Xcode's clang keeps its tools.
fn llvm_tool(var: &str, name: &str) -> Command {
    if let Some(program) = std::env::var_os(var) {
        return Command::new(program);
    }
    if cfg!(target_os = "macos") {
        let mut command = Command::new("xcrun");
        command.arg(name);
        return command;
    }
    Command::new(name)
}

/// The C sources and headers under `dir`: ours, which the coverage counts.
fn c_sources(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("{}: {error}", dir.display()))?
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "c" || extension == "h")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Runs `run` `runs` times, every time even after a failure, and scores the
/// passing runs.
fn repeat(runs: u64, mut run: impl FnMut() -> Result<bool, String>) -> Result<Score, String> {
    let mut score = Score {
        passed: 0,
        total: runs,
        failures: Vec::new(),
    };
    for number in 1..=runs {
        if run()? {
            score.passed = score.passed.saturating_add(1);
        } else {
            score
                .failures
                .push(format!("run {number} of {runs} failed"));
        }
    }
    Ok(score)
}

/// Statement citations for the crate in `dir`: the sections its README lists
/// after `**Spec sections:**`, their active statements in the nearest
/// `docs/spec/requirements.tsv` above it, the single statements it lists after
/// `**Spec statements:**`, and the `// spec` citations in the test code of the
/// Rust files under that tree's [`CITING_TREES`] (`docs/spec/README.md`).
///
/// A crate that implements only some statements of a section claims them one
/// by one on the `**Spec statements:**` line, so the section's other
/// statements, which belong to another crate or to the baseline, are not
/// counted against it.
///
/// A statement in [`BASELINE`] (`docs/spec/coverage-baseline.tsv`) is not
/// counted: the baseline holds statements written ahead of the code that
/// implements them (`docs/spec/README.md`: "A statement written ahead of the
/// code that implements it goes there, and the PR that adds the code and its
/// tests removes it."), so it is not yet owned by this crate and not yet
/// cited, exactly as the spec-citation ratchet in `qs-tests` treats it.
fn spec(dir: &Path) -> Result<Score, String> {
    let readme_path = dir.join("README.md");
    let readme = fs::read_to_string(&readme_path)
        .map_err(|error| format!("{}: {error}", readme_path.display()))?;
    let sections = spec_sections(&readme)
        .ok_or_else(|| format!("{}: no `**Spec sections:**` line", readme_path.display()))?;
    let Some(spec_root) = dir
        .ancestors()
        .find(|ancestor| ancestor.join(REQUIREMENTS).is_file())
    else {
        return Err(format!("no {REQUIREMENTS} above {}", dir.display()));
    };
    let index_path = spec_root.join(REQUIREMENTS);
    let index = fs::read_to_string(&index_path)
        .map_err(|error| format!("{}: {error}", index_path.display()))?;
    let statements = active_statements(&index)?;
    let baseline_path = spec_root.join(BASELINE);
    let baseline_text = fs::read_to_string(&baseline_path)
        .map_err(|error| format!("{}: {error}", baseline_path.display()))?;
    let baseline = baseline_statements(&baseline_text)?;
    let cited = cited_in_tests(spec_root)?;
    let claims = Claims {
        sections,
        statements: spec_statements(&readme),
        disclaimed: disclaimed_statements(&readme),
    };
    spec_score(&claims, &statements, &baseline, &cited)
}

const REQUIREMENTS: &str = "docs/spec/requirements.tsv";

/// The baseline of statements written ahead of their code
/// (`docs/spec/README.md`); its rows are not counted against a crate.
const BASELINE: &str = "docs/spec/coverage-baseline.tsv";

/// The statement IDs in `coverage-baseline.tsv`: a header line `id`, then one
/// ID per line (`docs/spec/README.md`). Blank lines are skipped.
fn baseline_statements(text: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = text.lines();
    if lines.next() != Some("id") {
        return Err(format!("{BASELINE}: unexpected header"));
    }
    let mut ids = BTreeSet::new();
    for line in lines {
        let id = line.trim();
        if !id.is_empty() {
            ids.insert(id.to_owned());
        }
    }
    Ok(ids)
}

/// The trees whose Rust files hold `// spec` citations, as the spec-citation
/// ratchet in `qs-tests` reads them: the crates, the apps, and xtask, whose
/// oracle module tests the oracles' C through the built oracle.
const CITING_TREES: [&str; 3] = ["crates", "apps", "xtask"];

/// The section IDs (`PREFIX-n`) on a README's `**Spec sections:**` line, up to
/// the next list item, `A to B` being every section from `A` to `B`, less the
/// sections on its `**Not this crate's:**` line. A statement ID on the line is
/// no section. `None` if there is no such line; empty if it says none.
pub(crate) fn spec_sections(readme: &str) -> Option<Vec<String>> {
    let (sections, _) = field_ids(&readme_field(readme, "**Spec sections:**")?);
    let (disclaimed, _) = readme_field(readme, NOT_THIS_CRATES)
        .map_or_else(|| (Vec::new(), Vec::new()), |text| field_ids(&text));
    Some(
        sections
            .into_iter()
            .filter(|section| !disclaimed.contains(section))
            .collect(),
    )
}

/// The statement IDs (`PREFIX-n.m`) on a README's `**Spec statements:**` line,
/// up to the next list item; empty if there is no such line.
pub(crate) fn spec_statements(readme: &str) -> Vec<String> {
    readme_field(readme, "**Spec statements:**")
        .map(|text| field_ids(&text).1)
        .unwrap_or_default()
}

/// The statement IDs on a README's `**Not this crate's:**` line: statements of a
/// section the crate claims that another crate implements, so they are not
/// counted against it.
pub(crate) fn disclaimed_statements(readme: &str) -> Vec<String> {
    readme_field(readme, NOT_THIS_CRATES)
        .map(|text| field_ids(&text).1)
        .unwrap_or_default()
}

/// The README line that names the sections and statements, written near the
/// crate's own, that another crate or a later issue implements.
const NOT_THIS_CRATES: &str = "**Not this crate's:**";

/// The section and statement IDs written in `text`, in order and each once. `A to B`
/// with `A` and `B` of one prefix is every section from `A` to `B`, or with two
/// statements of one section every statement from `A` to `B`.
fn field_ids(text: &str) -> (Vec<String>, Vec<String>) {
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| !word.is_empty())
        .collect();
    let mut sections: Vec<String> = Vec::new();
    let mut statements: Vec<String> = Vec::new();
    let add = |list: &mut Vec<String>, id: String| {
        if !list.contains(&id) {
            list.push(id);
        }
    };
    let mut rest = words.as_slice();
    while let Some((&word, after)) = rest.split_first() {
        rest = after;
        let range_end = match after {
            [to, end, ..] if *to == "to" => Some(*end),
            _ => None,
        };
        if let Some((prefix, from)) = section_parts(word) {
            match range_end.and_then(section_parts) {
                Some((end_prefix, to)) if end_prefix == prefix && from <= to => {
                    for number in from..=to {
                        add(&mut sections, format!("{prefix}-{number}"));
                    }
                    rest = rest.get(2..).unwrap_or_default();
                }
                _ => add(&mut sections, word.to_owned()),
            }
        } else if let Some((section, from)) = statement_parts(word) {
            match range_end.and_then(statement_parts) {
                Some((end_section, to)) if end_section == section && from <= to => {
                    for number in from..=to {
                        add(&mut statements, format!("{section}.{number}"));
                    }
                    rest = rest.get(2..).unwrap_or_default();
                }
                _ => add(&mut statements, word.to_owned()),
            }
        }
    }
    (sections, statements)
}

/// The prefix and number of a section ID, `PREFIX-n`.
fn section_parts(id: &str) -> Option<(&str, u32)> {
    if !is_section_id(id) {
        return None;
    }
    let (prefix, number) = id.split_once('-')?;
    Some((prefix, number.parse().ok()?))
}

/// The section and number of a statement ID, `PREFIX-n.m`.
fn statement_parts(id: &str) -> Option<(&str, u32)> {
    // The words of `field_ids` hold only letters, digits, `-` and `.`, so a number that
    // parses is all digits.
    let (section, number) = id.rsplit_once('.')?;
    if !is_section_id(section) {
        return None;
    }
    Some((section, number.parse().ok()?))
}

/// The text of the README list item that starts with `label`, its
/// continuation lines joined, up to the next list item, heading or blank line.
fn readme_field(readme: &str, label: &str) -> Option<String> {
    let mut lines = readme.lines();
    let first = lines.find_map(|line| line.split_once(label).map(|(_, rest)| rest))?;
    let mut text = first.to_owned();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("- ") || trimmed.starts_with('#') {
            break;
        }
        text.push(' ');
        text.push_str(trimmed);
    }
    Some(text)
}

/// `PREFIX-n`: an upper-case prefix starting with a letter, a hyphen and a number.
fn is_section_id(id: &str) -> bool {
    let Some((prefix, number)) = id.split_once('-') else {
        return false;
    };
    let mut chars = prefix.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// `(statement, section)` for every active row of `requirements.tsv`.
fn active_statements(index: &str) -> Result<Vec<(String, String)>, String> {
    let mut rows = index.lines();
    if rows.next() != Some("id\tspec\tsection\tstatus\tstatement") {
        return Err(format!("{REQUIREMENTS}: unexpected header"));
    }
    let mut statements = Vec::new();
    for row in rows {
        let mut columns = row.split('\t');
        let (Some(id), Some(_), Some(section), Some(status)) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            return Err(format!("{REQUIREMENTS}: cannot read row {row:?}"));
        };
        if status == "active" {
            statements.push((id.to_owned(), section.to_owned()));
        }
    }
    Ok(statements)
}

/// The statement IDs cited in test code ([`test_code`]) by the Rust files of
/// `root`'s [`CITING_TREES`], as the spec-citation ratchet reads them.
fn cited_in_tests(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for tree in CITING_TREES {
        for file in rust_files(&root.join(tree))? {
            let relative = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            // The fixture repositories cite their own statements, as the ratchet skips them.
            if relative.starts_with(FIXTURES) {
                continue;
            }
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            files.push((relative, text));
        }
    }
    let mut cited = BTreeSet::new();
    for ((path, text), code) in files.iter().zip(test_code(&files)) {
        cited.extend(citations(path, text, &code)?);
    }
    Ok(cited)
}

/// Statement IDs cited by lines of `code` whose first text is
/// `// spec <ID>, <ID>: ...`. A `// spec` line outside test code cites
/// nothing, so it is an error naming `path` and the line, as the
/// spec-citation ratchet in `qs-tests` reports it (`docs/spec/README.md`).
pub(crate) fn citations(path: &Path, text: &str, code: &TestCode) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    for (line, number) in text.lines().zip(1_u32..) {
        let Some(rest) = line.trim_start().strip_prefix("// spec ") else {
            continue;
        };
        if !code.contains(number) {
            let file = path.to_string_lossy().replace('\\', "/");
            return Err(shipped_citation(&file, number, rest));
        }
        let list = rest.split_once(':').map_or(rest, |(list, _)| list);
        ids.extend(list.split(',').map(|id| id.trim().to_owned()));
    }
    Ok(ids)
}

/// What a crate README claims: whole sections, single statements, and the
/// statements of its sections that it disclaims.
#[derive(Debug, Default)]
pub(crate) struct Claims {
    pub(crate) sections: Vec<String>,
    pub(crate) statements: Vec<String>,
    pub(crate) disclaimed: Vec<String>,
}

fn spec_score(
    claims: &Claims,
    statements: &[(String, String)],
    baseline: &BTreeSet<String>,
    cited: &BTreeSet<String>,
) -> Result<Score, String> {
    let mut score = Score {
        passed: 0,
        total: 0,
        failures: Vec::new(),
    };
    let mut counted = BTreeSet::new();
    let mut count = |id: &String, score: &mut Score| {
        if claims.disclaimed.contains(id) || !counted.insert(id.clone()) {
            return;
        }
        score.total = score.total.saturating_add(1);
        if cited.contains(id) {
            score.passed = score.passed.saturating_add(1);
        } else {
            score.failures.push(format!("{id} is not cited"));
        }
    };
    for section in &claims.sections {
        let mut found = false;
        for (id, owner) in statements {
            if owner != section {
                continue;
            }
            found = true;
            // A statement written ahead of its code sits in the baseline and
            // is not yet owned by this crate (`docs/spec/README.md`); it is
            // not counted, just as the ratchet in `qs-tests` does not require
            // a baseline statement to be cited.
            if baseline.contains(id) {
                continue;
            }
            count(id, &mut score);
        }
        if !found {
            return Err(format!(
                "section {section} has no active statements in {REQUIREMENTS}"
            ));
        }
    }
    for id in &claims.statements {
        if !statements.iter().any(|(statement, _)| statement == id) {
            return Err(format!(
                "statement {id} is not an active statement in {REQUIREMENTS}"
            ));
        }
        // A crate that claims a statement by name implements it, so the
        // statement cannot also be one written ahead of its code.
        if baseline.contains(id) {
            return Err(format!("statement {id} is claimed but is in {BASELINE}"));
        }
        count(id, &mut score);
    }
    Ok(score)
}

fn rust_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("{}: {error}", dir.display()))?
                .path();
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "target") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// `passed` of `total` as a percentage to one decimal place; 100% when there
/// is nothing to measure.
pub(crate) fn percent(score: &Score) -> String {
    let tenths = score
        .passed
        .saturating_mul(1000)
        .checked_div(score.total)
        .unwrap_or(1000);
    format!(
        "{}.{}%",
        tenths.checked_div(10).unwrap_or_default(),
        tenths.checked_rem(10).unwrap_or_default()
    )
}

/// The whole percentage `passed` of `total` reaches, rounded down; 100 when
/// there is nothing to measure. This is what a ratchet records.
pub(crate) fn floor_percent(score: &Score) -> u64 {
    score
        .passed
        .saturating_mul(100)
        .checked_div(score.total)
        .unwrap_or(100)
}

/// Whether `passed` of `total` is at least `required` percent.
pub(crate) fn reaches(score: &Score, required: u64) -> bool {
    u128::from(score.passed).saturating_mul(100)
        >= u128::from(required).saturating_mul(u128::from(score.total))
}

pub(crate) fn describe(score: &Score) -> String {
    format!("{} ({} of {})", percent(score), score.passed, score.total)
}

#[cfg(test)]
mod tests;
