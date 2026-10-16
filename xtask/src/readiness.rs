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
//! | `spec` | the crate README and `docs/spec/requirements.tsv` | active statements cited by a `// spec` test (TEST-3) |
//! | `flakes` | `cargo test`, [`FLAKE_RUNS`] times | passing runs (TEST-7) |
//!
//! A crate is named by its package name (a workspace member) or by its
//! directory, which is how the fixtures under `xtask/fixtures/readiness/` are
//! measured. Each crate's numbers come from its own tests. Measuring never
//! fails because a number is low; only a tool that cannot run fails it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// The crate is its own workspace (a fixture), so it gets its own target
    /// directory under `target/xtask/readiness/`.
    standalone: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    check: Check,
    out: PathBuf,
    os: String,
    crates: Vec<String>,
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let options = parse_args(args)?;
    let mut measurement = Measurement {
        check: options.check,
        os: options.os,
        crates: BTreeMap::new(),
    };
    for name in &options.crates {
        let target = resolve(root, name)?;
        let score = measure(root, options.check, &target)?;
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

/// A package name of a workspace member, or a crate directory inside the
/// repository.
fn resolve(root: &Path, name: &str) -> Result<Target, String> {
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
    Ok(Target {
        name: package.to_owned(),
        dir,
        standalone: manifest.contains_key("workspace"),
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

fn measure(root: &Path, check: Check, target: &Target) -> Result<Score, String> {
    match check {
        Check::Lines | Check::Branches => coverage(root, target, check),
        Check::Mutation => mutation(root, target),
        Check::Spec => spec(&target.dir),
        Check::Flakes => flakes(root, target),
    }
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// A cargo command for `target`, with its own target directory if it is a
/// standalone fixture.
fn cargo_for(root: &Path, target: &Target, toolchain: Option<&str>) -> Command {
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
    if target.standalone {
        command.env("CARGO_TARGET_DIR", scratch(root).join(&target.name));
    }
    command
}

fn scratch(root: &Path) -> PathBuf {
    root.join("target").join("xtask").join("readiness")
}

fn manifest(target: &Target) -> PathBuf {
    target.dir.join("Cargo.toml")
}

fn run(command: &mut Command, what: &str) -> Result<std::process::Output, String> {
    command
        .output()
        .map_err(|error| format!("cannot run {what}: {error}"))
}

fn coverage(root: &Path, target: &Target, check: Check) -> Result<Score, String> {
    let report = scratch(root).join(format!("{}-{}.json", check.name(), target.name));
    let branches = check == Check::Branches;
    let mut command = cargo_for(root, target, branches.then_some(NIGHTLY));
    command
        .args(["llvm-cov", "--locked", "--all-features", "--manifest-path"])
        .arg(manifest(target))
        .args(["--json", "--summary-only", "--output-path"])
        .arg(&report);
    if branches {
        command.arg("--branch");
    }
    let output = run(&mut command, "cargo llvm-cov")?;
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

fn mutation(root: &Path, target: &Target) -> Result<Score, String> {
    let out = scratch(root).join(format!("mutants-{}", target.name));
    let mut command = cargo_for(root, target, None);
    command
        .args([
            "mutants",
            "--all-features",
            "--no-shuffle",
            "--manifest-path",
        ])
        .arg(manifest(target))
        .arg("--output")
        .arg(&out);
    let output = run(&mut command, "cargo mutants")?;
    // 0: every mutant caught; 2: some missed; 3: some timed out. Anything else
    // (a usage error, a failing baseline) means there is no score.
    if !matches!(output.status.code(), Some(0 | 2 | 3)) {
        return Err(format!(
            "{}: cargo mutants failed ({}):\n{}",
            target.name,
            output.status,
            String::from_utf8_lossy(&output.stdout)
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

fn flakes(root: &Path, target: &Target) -> Result<Score, String> {
    let test = |no_run: bool| {
        let mut command = cargo_for(root, target, None);
        command
            .args(["test", "--locked", "--all-features", "--manifest-path"])
            .arg(manifest(target));
        if no_run {
            command.arg("--no-run");
        }
        run(&mut command, "cargo test")
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
/// `docs/spec/requirements.tsv` above it, and the `// spec` citations in the
/// Rust files under that tree's `crates/` and `apps/` (`docs/spec/README.md`).
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
    let mut cited = BTreeSet::new();
    for tree in ["crates", "apps"] {
        for file in rust_files(&spec_root.join(tree))? {
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            cited.extend(citations(&text));
        }
    }
    spec_score(&sections, &statements, &cited)
}

const REQUIREMENTS: &str = "docs/spec/requirements.tsv";

/// The section IDs (`PREFIX-n`) on a README's `**Spec sections:**` line, up to
/// the next list item. `None` if there is no such line; empty if it says none.
pub(crate) fn spec_sections(readme: &str) -> Option<Vec<String>> {
    let mut lines = readme.lines();
    let first =
        lines.find_map(|line| line.split_once("**Spec sections:**").map(|(_, rest)| rest))?;
    let mut text = first.to_owned();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("- ") || trimmed.starts_with('#') {
            break;
        }
        text.push(' ');
        text.push_str(trimmed);
    }
    let mut sections = Vec::new();
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        if is_section_id(word) && !sections.iter().any(|section| section == word) {
            sections.push(word.to_owned());
        }
    }
    Some(sections)
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

/// Statement IDs cited by lines whose first text is `// spec <ID>, <ID>: ...`.
pub(crate) fn citations(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("// spec ") else {
            continue;
        };
        let list = rest.split_once(':').map_or(rest, |(list, _)| list);
        ids.extend(list.split(',').map(|id| id.trim().to_owned()));
    }
    ids
}

fn spec_score(
    sections: &[String],
    statements: &[(String, String)],
    cited: &BTreeSet<String>,
) -> Result<Score, String> {
    let mut score = Score {
        passed: 0,
        total: 0,
        failures: Vec::new(),
    };
    for section in sections {
        let mut found = false;
        for (id, owner) in statements {
            if owner != section {
                continue;
            }
            found = true;
            score.total = score.total.saturating_add(1);
            if cited.contains(id) {
                score.passed = score.passed.saturating_add(1);
            } else {
                score.failures.push(format!("{id} is not cited"));
            }
        }
        if !found {
            return Err(format!(
                "section {section} has no active statements in {REQUIREMENTS}"
            ));
        }
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
