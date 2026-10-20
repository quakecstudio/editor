// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ratchet`: judges readiness measurements against the bar in
//! ADR-0009 and the per-crate minimums in `ratchets.toml` (TEST-1, TEST-2,
//! TEST-3, TEST-7, TEST-10).
//!
//! - Every measured crate must reach the bar for each check, or its recorded
//!   minimum where that is higher. A check listed under a crate's
//!   `exceptions` (with the reason, also stated in the crate README) waives the
//!   bar but not the minimum. Every surviving mutant must be listed under
//!   `equivalent_mutants` with a reason. The flake check needs [`FLAKE_RUNS`]
//!   passing runs on each of [`FLAKE_OSES`].
//! - `ratchets.toml` itself: every crate with recorded minimums names a
//!   readiness issue of the plan in `readiness`, and a minimum below the bar
//!   needs an exception. A crate without a review may still hold a table that
//!   waives an unmeasurable check (an `exception`) or justifies a surviving
//!   mutant (an `equivalent_mutant`), each with a reason.
//! - Its git history: a commit that changes a crate's minimums must also
//!   change `readiness` to the readiness issue that set them, and one that
//!   lowers a minimum must give a new `reason`. A crate's table may be removed
//!   only once the crate is no longer in `layers.toml`.
//!
//! With `--history <file>` the measured numbers are appended to a JSON history.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::bench::{commit, unix_time};
use crate::layers::FILE as LAYERS;
use crate::readiness::{Check, FLAKE_RUNS, Measurement, Score, describe, floor_percent, reaches};
use crate::{Report, print_outcome};

pub(crate) const FILE: &str = "ratchets.toml";
const READINESS_PLAN: &str = "plan/backlog/readiness.yaml";
const HISTORY_VERSION: u32 = 1;

/// The operating systems the flake check runs on.
pub(crate) const FLAKE_OSES: [&str; 3] = ["linux", "macos", "windows"];

/// The checks a ratchet records a minimum for.
const RATCHETED: [Check; 4] = [Check::Lines, Check::Branches, Check::Mutation, Check::Spec];

const USAGE: &str = "usage: cargo xtask ratchet [--history <file.json>] [<measurement.json>...]";

/// `ratchets.toml`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ratchets {
    #[serde(default)]
    crates: BTreeMap<String, Minimums>,
}

/// One crate's table in `ratchets.toml`: the minimums a passing readiness
/// review recorded, or, before any review has passed, only exceptions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Minimums {
    /// The plan key of the readiness issue that set these numbers; absent
    /// with them.
    #[serde(default)]
    readiness: Option<String>,
    #[serde(default)]
    lines: Option<u64>,
    #[serde(default)]
    branches: Option<u64>,
    #[serde(default)]
    mutation: Option<u64>,
    #[serde(default)]
    spec: Option<u64>,
    /// Why a minimum went down; required whenever one does.
    #[serde(default)]
    reason: Option<String>,
    /// Checks whose bar is waived for this crate, each with its reason.
    #[serde(default)]
    exceptions: BTreeMap<Check, String>,
    /// Surviving mutants (by [`crate::readiness::mutant_key`]) that are
    /// equivalent to the original, each with its reason.
    #[serde(default)]
    equivalent_mutants: BTreeMap<String, String>,
}

impl Minimums {
    fn minimum(&self, check: Check) -> Option<u64> {
        match check {
            Check::Lines => self.lines,
            Check::Branches => self.branches,
            Check::Mutation => self.mutation,
            Check::Spec => self.spec,
            Check::Flakes => None,
        }
    }

    /// Whether a review has recorded numbers: any ratcheted minimum is there.
    fn recorded(&self) -> bool {
        RATCHETED
            .into_iter()
            .any(|check| self.minimum(check).is_some())
    }
}

pub(crate) fn parse(text: &str) -> Result<Ratchets, String> {
    toml::from_str(text).map_err(|error| format!("{FILE}: {error}"))
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let (history, files) = parse_args(args)?;
    let text = fs::read_to_string(root.join(FILE)).map_err(|error| format!("{FILE}: {error}"))?;
    let ratchets = parse(&text)?;
    let keys = readiness_keys(
        &fs::read_to_string(root.join(READINESS_PLAN))
            .map_err(|error| format!("{READINESS_PLAN}: {error}"))?,
    );
    let members = layer_crates(root)?;

    let mut report = Report::default();
    report.problems.extend(validate(&ratchets, &keys));
    report
        .problems
        .extend(history_problems(&versions(root)?, &members));

    let mut measurements = Vec::new();
    for file in &files {
        let text =
            fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
        let measurement: Measurement =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", file.display()))?;
        measurements.push(measurement);
    }
    let measured = group(&measurements, &mut report);
    for (name, checks) in &measured {
        evaluate(name, checks, ratchets.crates.get(name), &mut report);
    }
    let passed = report.problems.is_empty();
    if passed && !measured.is_empty() {
        println!("Minimums these numbers support, for {FILE}:\n");
        print!("{}", proposed_minimums(&measured));
    }
    if let Some(path) = history {
        record(root, &path, passed, &measured)?;
    }
    Ok(print_outcome("ratchet", Ok(report)))
}

fn parse_args(args: &[String]) -> Result<(Option<PathBuf>, Vec<PathBuf>), String> {
    let mut history = None;
    let mut files = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--history" => history = Some(PathBuf::from(iter.next().ok_or(USAGE)?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            file => files.push(PathBuf::from(file)),
        }
    }
    Ok((history, files))
}

/// The readiness issue keys (`P1-R0`) in the generated readiness plan.
pub(crate) fn readiness_keys(plan: &str) -> BTreeSet<String> {
    plan.lines()
        .filter_map(|line| line.trim_start().strip_prefix("- key: "))
        .map(str::trim)
        .filter(|key| {
            key.split_once("-R")
                .is_some_and(|(phase, _)| phase.starts_with('P'))
        })
        .map(str::to_owned)
        .collect()
}

/// The crates in `layers.toml`, which `cargo xtask layers` keeps equal to the
/// workspace members.
fn layer_crates(root: &Path) -> Result<BTreeSet<String>, String> {
    let text =
        fs::read_to_string(root.join(LAYERS)).map_err(|error| format!("{}: {error}", LAYERS))?;
    let layers: toml::Table =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", LAYERS))?;
    Ok(layers
        .get("crates")
        .and_then(toml::Value::as_table)
        .map(|crates| crates.keys().cloned().collect())
        .unwrap_or_default())
}

/// What is wrong with the file as it stands.
pub(crate) fn validate(ratchets: &Ratchets, keys: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, minimums) in &ratchets.crates {
        if minimums.recorded() {
            match &minimums.readiness {
                Some(key) if keys.contains(key) => {}
                Some(key) => problems.push(format!(
                    "{FILE}: {name}: readiness {key:?} is not a readiness issue in {READINESS_PLAN}"
                )),
                None => problems.push(format!(
                    "{FILE}: {name}: minimums need the readiness issue that set them"
                )),
            }
        } else {
            if minimums.readiness.is_some() {
                problems.push(format!(
                    "{FILE}: {name}: a readiness issue but no minimums; a review records both"
                ));
            }
            if minimums.exceptions.is_empty() && minimums.equivalent_mutants.is_empty() {
                problems.push(format!(
                    "{FILE}: {name}: a table needs the minimums of a review, an exception or an \
                     equivalent mutant"
                ));
            }
        }
        for check in RATCHETED {
            let Some(minimum) = minimums.minimum(check) else {
                if minimums.recorded() {
                    problems.push(format!(
                        "{FILE}: {name}: {} has no minimum; a review records all four",
                        check.name()
                    ));
                }
                continue;
            };
            if minimum > 100 {
                problems.push(format!(
                    "{FILE}: {name}: {} is {minimum}, over 100",
                    check.name()
                ));
            }
            if minimum < check.threshold() && !minimums.exceptions.contains_key(&check) {
                problems.push(format!(
                    "{FILE}: {name}: {} minimum {minimum} is below the bar of {}% without an exception",
                    check.name(),
                    check.threshold()
                ));
            }
        }
        if minimums.exceptions.contains_key(&Check::Flakes) {
            problems.push(format!(
                "{FILE}: {name}: the flake check takes no exception"
            ));
        }
        let reasons = minimums
            .exceptions
            .values()
            .chain(minimums.equivalent_mutants.values())
            .chain(minimums.reason.iter());
        if reasons.into_iter().any(|reason| reason.trim().is_empty()) {
            problems.push(format!("{FILE}: {name}: every reason must say something"));
        }
    }
    problems
}

/// What is wrong with going from `old` to `new` in one commit.
pub(crate) fn check_change(
    old: &Ratchets,
    new: &Ratchets,
    members: &BTreeSet<String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, before) in &old.crates {
        let Some(after) = new.crates.get(name) else {
            if members.contains(name) {
                problems.push(format!(
                    "{name}: its ratchet was removed while the crate still exists"
                ));
            }
            continue;
        };
        if after == before {
            continue;
        }
        // A table of exceptions alone, before any review, changes with a new
        // reason (below); recorded minimums change only with a new review.
        let reviewed = before.recorded() || after.recorded();
        if reviewed && after.readiness == before.readiness {
            problems.push(format!(
                "{name}: minimums changed without naming the readiness issue that set them \
                 (readiness is still {})",
                after.readiness.as_deref().unwrap_or("absent")
            ));
        }
        let lowered: Vec<&str> = RATCHETED
            .into_iter()
            .filter(|check| after.minimum(*check) < before.minimum(*check))
            .map(Check::name)
            .collect();
        let waived = after
            .exceptions
            .keys()
            .any(|check| !before.exceptions.contains_key(check))
            || after
                .equivalent_mutants
                .keys()
                .any(|mutant| !before.equivalent_mutants.contains_key(mutant));
        let new_reason = after.reason.is_some() && after.reason != before.reason;
        if (!lowered.is_empty() || waived) && !new_reason {
            let what = if lowered.is_empty() {
                "added an exception or equivalent mutant".to_owned()
            } else {
                format!("lowered {}", lowered.join(", "))
            };
            problems.push(format!("{name}: {what} without a new reason"));
        }
    }
    problems
}

/// One version of `ratchets.toml`: where it is from, and its text (`None` if
/// the file did not exist there).
pub(crate) type Version = (String, Option<String>);

/// Every committed version of `ratchets.toml`, oldest first, then the working
/// tree's if it differs from the last commit. A commit that deleted the file is
/// a version without it; a version git cannot read is an error, since skipping
/// it would skip the history check of that commit.
pub(crate) fn versions(root: &Path) -> Result<Vec<Version>, String> {
    let shas = git(root, &["log", "--format=%H", "--reverse", "--", FILE])?;
    let mut versions = Vec::new();
    for sha in shas.lines().filter(|line| !line.is_empty()) {
        let listed = git(root, &["ls-tree", "--name-only", sha, "--", FILE])?;
        let text = if listed.trim().is_empty() {
            None
        } else {
            Some(git(root, &["show", &format!("{sha}:{FILE}")])?)
        };
        versions.push((sha.to_owned(), text));
    }
    let working = fs::read_to_string(root.join(FILE)).ok();
    if versions.last().map(|(_, text)| text) != Some(&working) {
        versions.push(("working tree".to_owned(), working));
    }
    Ok(versions)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("git {}: {error}", args.join(" ")))
}

/// The problems of every change between consecutive versions.
pub(crate) fn history_problems(versions: &[Version], members: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut previous = Ratchets::default();
    for (label, text) in versions {
        let current = match text.as_deref().map(parse) {
            None => Ratchets::default(),
            Some(Ok(current)) => current,
            Some(Err(error)) => {
                problems.push(format!("{label}: {error}"));
                continue;
            }
        };
        let short = label.get(..12).unwrap_or(label);
        for problem in check_change(&previous, &current, members) {
            problems.push(format!("{FILE} at {short}: {problem}"));
        }
        previous = current;
    }
    problems
}

/// Every measurement by crate, check and OS.
type Measured = BTreeMap<String, BTreeMap<Check, BTreeMap<String, Score>>>;

fn group(measurements: &[Measurement], report: &mut Report) -> Measured {
    let mut measured = Measured::new();
    for measurement in measurements {
        for (name, score) in &measurement.crates {
            let by_os = measured
                .entry(name.clone())
                .or_default()
                .entry(measurement.check)
                .or_default();
            if by_os
                .insert(measurement.os.clone(), score.clone())
                .is_some()
            {
                report.problems.push(format!(
                    "{name} {} on {} is measured twice",
                    measurement.check.name(),
                    measurement.os
                ));
            }
        }
    }
    measured
}

/// Judges one crate's measurements; every check must be there.
fn evaluate(
    name: &str,
    checks: &BTreeMap<Check, BTreeMap<String, Score>>,
    minimums: Option<&Minimums>,
    report: &mut Report,
) {
    for check in Check::ALL {
        let scores = checks.get(&check);
        let required: Vec<String> = if check == Check::Flakes {
            FLAKE_OSES.iter().map(|os| (*os).to_owned()).collect()
        } else {
            scores
                .into_iter()
                .flat_map(BTreeMap::keys)
                .cloned()
                .collect()
        };
        if required.is_empty() {
            // A check no tool can measure for the crate (the oracles' C has no
            // mutation tool) passes unmeasured only under an exception that
            // leaves no minimum to hold.
            let waiver = minimums
                .filter(|minimums| minimums.minimum(check).unwrap_or_default() == 0)
                .and_then(|minimums| minimums.exceptions.get(&check));
            match waiver {
                Some(reason) => report.passed.push(format!(
                    "{name} {}: not measured, waived by an exception: {reason}",
                    check.name()
                )),
                None => report
                    .problems
                    .push(format!("{name} {}: not measured", check.name())),
            }
        }
        for os in required {
            match scores.and_then(|scores| scores.get(&os)) {
                Some(score) => {
                    let (ok, line) = judge(check, score, minimums);
                    report.record(ok, format!("{name} {} on {os}: {line}", check.name()));
                }
                None => report
                    .problems
                    .push(format!("{name} {} on {os}: not measured", check.name())),
            }
        }
    }
}

/// Whether `score` passes `check` for a crate with these minimums, and why.
pub(crate) fn judge(check: Check, score: &Score, minimums: Option<&Minimums>) -> (bool, String) {
    let waived = minimums.is_some_and(|minimums| minimums.exceptions.contains_key(&check));
    let bar = if waived { 0 } else { check.threshold() };
    let floor = minimums
        .and_then(|minimums| minimums.minimum(check))
        .unwrap_or_default();
    let required = bar.max(floor);
    let mut line = describe(score);
    let mut ok = reaches(score, required);
    line.push_str(&format!(", needs {required}%"));
    if waived {
        line.push_str(" (bar waived by an exception)");
    }
    match check {
        Check::Flakes if score.total < FLAKE_RUNS => {
            ok = false;
            line.push_str(&format!("; only {} runs, needs {FLAKE_RUNS}", score.total));
        }
        Check::Mutation => {
            for survivor in &score.failures {
                let listed = minimums
                    .is_some_and(|minimums| minimums.equivalent_mutants.contains_key(survivor));
                if !listed {
                    ok = false;
                    line.push_str(&format!(
                        "; survivor not killed or listed as equivalent: {survivor}"
                    ));
                }
            }
        }
        Check::Lines | Check::Branches | Check::Spec | Check::Flakes => {
            if !ok {
                for failure in &score.failures {
                    line.push_str(&format!("; {failure}"));
                }
            }
        }
    }
    (ok, line)
}

/// A `ratchets.toml` table per crate holding the lowest measured percentage
/// of each ratcheted check, for the readiness reviewer to commit.
fn proposed_minimums(measured: &Measured) -> String {
    let mut text = String::new();
    for (name, checks) in measured {
        text.push_str(&format!(
            "[crates.{name}]\nreadiness = \"<readiness issue key>\"\n"
        ));
        for check in RATCHETED {
            let lowest = checks
                .get(&check)
                .into_iter()
                .flat_map(BTreeMap::values)
                .map(floor_percent)
                .min()
                .unwrap_or_default();
            text.push_str(&format!("{} = {lowest}\n", check.name()));
        }
        text.push('\n');
    }
    text
}

/// The JSON history of readiness runs.
#[derive(Debug, Serialize, Deserialize)]
struct History {
    version: u32,
    runs: Vec<Run>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Run {
    /// Seconds since the Unix epoch, UTC.
    unix_time: u64,
    commit: Option<String>,
    passed: bool,
    crates: Measured,
}

fn record(root: &Path, path: &Path, passed: bool, measured: &Measured) -> Result<(), String> {
    let mut history = match fs::read_to_string(path) {
        Ok(text) => {
            let history: History = serde_json::from_str(&text)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if history.version != HISTORY_VERSION {
                return Err(format!(
                    "{}: history version {} is not {HISTORY_VERSION}",
                    path.display(),
                    history.version
                ));
            }
            history
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => History {
            version: HISTORY_VERSION,
            runs: Vec::new(),
        },
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    history.runs.push(Run {
        unix_time: unix_time(),
        commit: commit(root),
        passed,
        crates: measured.clone(),
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&history)
        .map_err(|error| format!("cannot serialise the history: {error}"))?;
    text.push('\n');
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    println!("ratchet: recorded in {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests;
