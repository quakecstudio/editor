// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC grammar differential (DIFF-13): random FTEQCC programs ([`generate`]), each
//! compiled by the pinned FTEQCC and read by the FTEQCC profile, which must agree whether the
//! program is accepted, and where both reject it, on FTEQCC's first error (DIFF-13.2).
//! A difference is a failure unless an expected difference names the spec statement that
//! explains it. The programs and the oracle's outcomes are recorded in [`FIXTURE`], which
//! `cargo test -p xtask` replays without the oracle.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use qc_preproc::CommandLine;
use qc_sema::{Db, FileId, ModuleInput};
use qc_syntax::Dialect;

use super::Oracles;
use super::builtins::{across_cores, threads};

pub(crate) mod generate;

/// The recorded programs and outcomes of the oracle, relative to the repository.
pub(crate) const FIXTURE: &str = "xtask/fixtures/oracles/grammar-outcomes.json";

/// How long FTEQCC may take over one program before it is taken to hang.
const ORACLE_LIMIT: Duration = Duration::from_secs(20);

/// What FTEQCC did with a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Compiled it.
    Accepted,
    /// Refused it, with its first error's line and message when it printed one.
    Rejected(Option<(u32, String)>),
    /// Still running after [`ORACLE_LIMIT`].
    Hangs,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => f.write_str("accepted"),
            Self::Rejected(None) => f.write_str("rejected"),
            Self::Rejected(Some((line, message))) => write!(f, "rejected at {line}: {message}"),
            Self::Hangs => f.write_str("hangs"),
        }
    }
}

impl Outcome {
    /// The outcome [`fmt::Display`] wrote as `text`.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "accepted" => Some(Self::Accepted),
            "rejected" => Some(Self::Rejected(None)),
            "hangs" => Some(Self::Hangs),
            _ => {
                let (line, message) = text.strip_prefix("rejected at ")?.split_once(": ")?;
                Some(Self::Rejected(Some((
                    line.parse().ok()?,
                    message.to_owned(),
                ))))
            }
        }
    }

    /// The outcome of FTEQCC's run whose output is `text` and which succeeded or not.
    fn of_run(success: bool, text: &str) -> Self {
        if success {
            return Self::Accepted;
        }
        Self::Rejected(first_error(text))
    }

    /// Whether the profile, which reported the errors `errors` (their lines and messages),
    /// agrees (DIFF-13.2): both accept, or both reject, the profile with FTEQCC's first error's
    /// message, which names its code, among its own.
    fn agrees(&self, errors: &[(u32, String)]) -> bool {
        match self {
            Self::Accepted => errors.is_empty(),
            Self::Rejected(None) => !errors.is_empty(),
            Self::Rejected(Some((_, first))) => errors.iter().any(|(_, message)| message == first),
            Self::Hangs => false,
        }
    }
}

/// FTEQCC's first error in `text`: the line and message of the first `t.qc:LINE: error...`.
fn first_error(text: &str) -> Option<(u32, String)> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix("t.qc:")?;
        let (number, rest) = rest.split_once(':')?;
        let rest = rest.trim_start().strip_prefix("error")?;
        let (_, message) = rest.split_once(": ")?;
        Some((number.parse().ok()?, message.to_owned()))
    })
}

/// One generated program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Case {
    pub(crate) key: String,
    pub(crate) program: String,
}

/// Every program a check runs, in order.
pub(crate) fn cases() -> Vec<Case> {
    (0..generate::PROGRAMS)
        .map(|n| Case {
            key: format!("grammar-{n:04}"),
            program: generate::program(n),
        })
        .collect()
}

/// The errors the FTEQCC profile reports for `program`, with their lines, counted from 1, in
/// the order of their places.
pub(crate) fn profile(program: &str) -> Vec<(u32, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FileId::new(0), "t.qc".into(), program.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId::new(0)],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    let mut errors: Vec<(usize, u32, String)> = analysis
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| {
            let start = d.diagnostic.span.start;
            let before = program.get(..start).unwrap_or(program);
            let lines = before.bytes().filter(|&b| b == b'\n').count();
            let line = u32::try_from(lines).unwrap_or(u32::MAX).saturating_add(1);
            (start, line, d.diagnostic.message.clone())
        })
        .collect();
    errors.sort_by_key(|(start, _, _)| *start);
    errors
        .into_iter()
        .map(|(_, line, message)| (line, message))
        .collect()
}

/// The statement that explains why the profile, which reported `errors`, differs from the
/// oracle on `case`, if one does (DIFF-13.3): a body that lost its `}`, which FTEQCC reads to
/// the end of the file and the profile ends at the next declaration line (FTEF-18.34).
fn expected_difference(case: &Case, errors: &[(u32, String)]) -> Option<&'static str> {
    let count = |c: char| case.program.chars().filter(|&x| x == c).count();
    let lost_brace = count('{') > count('}');
    let ended = errors
        .iter()
        .any(|(_, message)| message.starts_with("expected }, found "));
    (lost_brace && ended).then_some("FTEF-18.34")
}

/// Runs FTEQCC on `case` in `dir`.
fn oracle(oracles: &Oracles, dir: &Path, case: &Case) -> Result<Outcome, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, text: &[u8]| {
        let path = dir.join(name);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    write("progs.src", b"progs.dat\nt.qc\n")?;
    write("t.qc", case.program.as_bytes())?;
    let mut command = Command::new(&oracles.fteqcc);
    command.arg("-Tid").current_dir(dir);
    let output = oracles
        .exec
        .output_within(&mut command, ORACLE_LIMIT)
        .map_err(|e| format!("cannot run fteqcc: {e}"))?;
    let Some(output) = output else {
        return Ok(Outcome::Hangs);
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Outcome::of_run(output.status.success(), &text))
}

/// A recorded case: its program and the oracle's outcome.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Recorded {
    program: String,
    oracle: String,
}

/// Reads [`FIXTURE`]: each case's program and outcome, by key.
pub(crate) fn read_fixture(root: &Path) -> Result<BTreeMap<String, (String, Outcome)>, String> {
    let path = root.join(FIXTURE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: BTreeMap<String, Recorded> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    raw.into_iter()
        .map(|(key, recorded)| {
            let outcome = Outcome::parse(&recorded.oracle)
                .ok_or_else(|| format!("{FIXTURE}: {key}: bad outcome `{}`", recorded.oracle))?;
            Ok((key, (recorded.program, outcome)))
        })
        .collect()
}

/// Writes `outcomes` of `cases` to [`FIXTURE`].
fn write_fixture(root: &Path, cases: &[Case], outcomes: &[Outcome]) -> Result<(), String> {
    let raw: BTreeMap<&str, Recorded> = cases
        .iter()
        .zip(outcomes)
        .map(|(case, outcome)| {
            let recorded = Recorded {
                program: case.program.clone(),
                oracle: outcome.to_string(),
            };
            (case.key.as_str(), recorded)
        })
        .collect();
    let text = serde_json::to_string_pretty(&raw).map_err(|e| e.to_string())?;
    let path = root.join(FIXTURE);
    fs::write(&path, format!("{text}\n")).map_err(|e| format!("{}: {e}", path.display()))
}

/// How the cases of [`compare`] came out.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) agree: usize,
    pub(crate) expected: BTreeMap<&'static str, usize>,
    pub(crate) differ: usize,
}

impl fmt::Display for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} agree, {} differ", self.agree, self.differ)?;
        for (statement, count) in &self.expected {
            write!(f, ", {count} as {statement} says")?;
        }
        Ok(())
    }
}

/// Compares the profile with `outcomes`, the oracle's for `cases`, in order; prints each
/// difference no statement explains.
pub(crate) fn compare(cases: &[Case], outcomes: &[Outcome]) -> Result<Tally, String> {
    let profiled = across_cores(cases, threads(), |_, case| Ok(profile(&case.program)))?;
    let mut tally = Tally::default();
    for ((case, outcome), errors) in cases.iter().zip(outcomes).zip(&profiled) {
        if outcome.agrees(errors) {
            tally.agree = tally.agree.saturating_add(1);
        } else if let Some(statement) = expected_difference(case, errors) {
            let count = tally.expected.entry(statement).or_default();
            *count = count.saturating_add(1);
        } else {
            tally.differ = tally.differ.saturating_add(1);
            let ours = errors
                .first()
                .map_or_else(|| "accepted".to_owned(), |(line, m)| format!("{line}: {m}"));
            println!("FAIL  {}: oracle {outcome}, profile {ours}", case.key);
        }
    }
    Ok(tally)
}

/// `cargo xtask oracles grammar [--record]`: runs every program through FTEQCC, checks the
/// outcomes against [`FIXTURE`] (or with `record` writes them there), then compares the profile.
pub(crate) fn check(root: &Path, oracles: &Oracles, record: bool) -> Result<bool, String> {
    let cases = cases();
    let work = oracles.work.join("grammar");
    let ran = across_cores(&cases, threads(), |thread, case| {
        oracle(oracles, &work.join(format!("thread{thread}")), case)
    });
    let outcomes = match ran {
        Ok(outcomes) => outcomes,
        Err(problem) => {
            println!("FAIL  grammar differential: {problem}");
            return Ok(false);
        }
    };
    let mut ok = true;
    if record {
        write_fixture(root, &cases, &outcomes)?;
        println!(
            "oracles: recorded {} grammar programs in {FIXTURE}",
            cases.len()
        );
    } else {
        let recorded = match read_fixture(root) {
            Ok(recorded) => recorded,
            Err(problem) => {
                println!("FAIL  {problem} (--record)");
                BTreeMap::new()
            }
        };
        for (case, outcome) in cases.iter().zip(&outcomes) {
            let same = recorded
                .get(&case.key)
                .is_some_and(|(program, held)| *program == case.program && held == outcome);
            if !same {
                ok = false;
                println!(
                    "FAIL  {}: the oracle gives {outcome}, not as {FIXTURE} records (--record)",
                    case.key
                );
            }
        }
        if recorded.len() != cases.len() {
            ok = false;
            println!("FAIL  {FIXTURE} records programs the generator does not make (--record)");
        }
    }
    let tally = compare(&cases, &outcomes)?;
    println!("oracles: {} grammar programs: {tally}", cases.len());
    Ok(ok && tally.differ == 0)
}

/// Checks the profile against the outcomes [`FIXTURE`] records, without the oracle; a
/// program the generator no longer makes as recorded fails (DIFF-13.4).
pub(crate) fn check_recorded(root: &Path) -> Result<bool, String> {
    let recorded = read_fixture(root)?;
    let cases = cases();
    let mut ok = recorded.len() == cases.len();
    if !ok {
        println!("FAIL  {FIXTURE} records programs the generator does not make (--record)");
    }
    let mut outcomes = Vec::with_capacity(cases.len());
    for case in &cases {
        match recorded.get(&case.key) {
            Some((program, outcome)) if *program == case.program => outcomes.push(outcome.clone()),
            _ => {
                ok = false;
                println!(
                    "FAIL  {}: not recorded as generated in {FIXTURE} (--record)",
                    case.key
                );
                outcomes.push(Outcome::Hangs);
            }
        }
    }
    let tally = compare(&cases, &outcomes)?;
    println!(
        "oracles: {} grammar programs against {FIXTURE}: {tally}",
        cases.len()
    );
    Ok(ok && tally.differ == 0)
}

#[cfg(test)]
mod tests;
