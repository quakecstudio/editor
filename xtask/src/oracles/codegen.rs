// SPDX-License-Identifier: GPL-3.0-or-later

//! The compiler differential (P2-06): `qc-codegen` against id's `qcc`, progs.dat byte for byte.
//!
//! [`compile_codebase`] builds a corpus codebase with our compiler as the editor does (its
//! `progs.src` opened as a project, each file named as `progs.src` lists it, QC-25.4);
//! [`differences`] says where two progs differ, table by table, so a failure names the first
//! statement, definition or global that differs. `cargo xtask oracles id101` compares id 1.01
//! this way.
//!
//! `cargo xtask oracles codegen` compiles each program of [`CASES`] and the generated programs of
//! `generate.rs` with both compilers: the two
//! must write the same progs.dat, or both refuse the program with `qcc`'s message among ours,
//! but where a QC-27 statement says `qc-codegen` does otherwise. `qcc`'s outcomes are recorded
//! in [`FIXTURE`], so that `cargo test -p xtask` checks `qc-codegen` against them on every host
//! without the oracles; `cargo xtask oracles codegen --record` writes it again.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qc_codegen::{CompileError, Compiled, Options, compile_module};
use qc_progs::{Progs, ReadOptions};
use qc_sema::{Db, FileId, ModuleInput};

use super::{Oracles, sha256};

mod cases;
mod generate;
#[cfg(test)]
mod tests;

use cases::CASES;

/// The dialect a corpus codebase is compiled in, as its oracle compiler reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    /// id's QuakeC, as id's `qcc` compiles id 1.01.
    Vanilla,
    /// FTEQCC's, as the pinned FTEQCC compiles the rerelease (`rerelease.rs`).
    Fteqcc {
        /// Prototypes are collected first, FTEQCC's `-Fautoproto` (FTE-1.2).
        autoproto: bool,
    },
}

/// Compiles the codebase whose `progs.src` is in `dir` with `qc-codegen` in `profile`.
pub(crate) fn compile_codebase(
    dir: &Path,
    profile: Profile,
    options: Options,
) -> Result<Compiled, String> {
    let project = qc_project::open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let [module] = project.modules.as_slice() else {
        return Err(format!("{}: one module expected", dir.display()));
    };
    let mut db = Db::new();
    if let Profile::Fteqcc { autoproto } = profile {
        db.set_dialect(qc_project::Dialect::Fteqcc.into());
        db.set_autoproto(autoproto);
    }
    for (index, file) in module.graph.files.iter().enumerate() {
        let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        db.set_file(FileId(index), file.path.clone(), text);
    }
    let base = module.src.parent().unwrap_or(dir).to_path_buf();
    let names: Vec<String> = module
        .graph
        .files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(&base)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let input = ModuleInput::from_module(module);
    compile_module(
        &mut db,
        &input,
        |id| names.get(id.0).cloned().unwrap_or_default(),
        options,
    )
    .map_err(|e| describe(&db, &e))
}

/// A compile error as text, each diagnostic with its file.
fn describe(db: &Db, error: &CompileError) -> String {
    match error {
        CompileError::Diagnostics(diagnostics) => diagnostics
            .iter()
            .map(|d| {
                let path = db
                    .file_path(d.file)
                    .map_or_else(|| "?".to_owned(), |p| p.display().to_string());
                format!(
                    "{path}@{}: {} {}",
                    d.diagnostic.span.start,
                    d.diagnostic.code.as_str(),
                    d.diagnostic.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        CompileError::Dialect(_)
        | CompileError::NotAnalysed(_)
        | CompileError::Write(_)
        | CompileError::Internal { .. } => {
            format!("{error:?}")
        }
    }
}

/// Our progs as bytes.
pub(crate) fn bytes(compiled: &Compiled) -> Result<Vec<u8>, String> {
    qc_progs::write(&compiled.progs).map_err(|e| format!("cannot write our progs: {e:?}"))
}

/// Where progs `ours` differs from `theirs`: the header's values and, for each table, its
/// length and first differing entry. Empty when the two read the same.
pub(crate) fn differences(ours: &[u8], theirs: &[u8]) -> Result<Vec<String>, String> {
    // PROGS-15.8: the two are compared whatever their system definitions.
    let options = ReadOptions {
        check_header_crc: false,
    };
    let ours = Progs::read_with(ours, &options)
        .map_err(|e| format!("our progs does not load: {e:?}"))?
        .to_file();
    let theirs = Progs::read_with(theirs, &options)
        .map_err(|e| format!("qcc's progs does not load: {e:?}"))?
        .to_file();
    let mut out = Vec::new();
    if ours.crc != theirs.crc {
        out.push(format!("crc {} against {}", ours.crc, theirs.crc));
    }
    if ours.entityfields != theirs.entityfields {
        out.push(format!(
            "entityfields {} against {}",
            ours.entityfields, theirs.entityfields
        ));
    }
    table(&mut out, "statement", &ours.statements, &theirs.statements);
    table(&mut out, "function", &ours.functions, &theirs.functions);
    table(
        &mut out,
        "global def",
        &ours.global_defs,
        &theirs.global_defs,
    );
    table(&mut out, "field def", &ours.field_defs, &theirs.field_defs);
    table(&mut out, "global", &ours.globals, &theirs.globals);
    table(&mut out, "string byte", &ours.strings, &theirs.strings);
    Ok(out)
}

/// Appends to `out` how table `what` of ours differs from qcc's: its length, its first
/// differing entry.
fn table<T: PartialEq + std::fmt::Debug>(
    out: &mut Vec<String>,
    what: &str,
    ours: &[T],
    theirs: &[T],
) {
    if ours.len() != theirs.len() {
        out.push(format!("{} {what}s against {}", ours.len(), theirs.len()));
    }
    if let Some((i, (a, b))) = ours
        .iter()
        .zip(theirs)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        out.push(format!("{what} {i}: {a:?} against {b:?}"));
    }
}

/// The recorded outcomes of id's `qcc` on [`CASES`], relative to the repository.
pub(crate) const FIXTURE: &str = "xtask/fixtures/oracles/codegen-outcomes.json";

/// How long `qcc` may take over one case.
const ORACLE_LIMIT: Duration = Duration::from_secs(20);

/// One program of the differential: `t.qc`, then `files`, in that order in `progs.src`.
#[derive(Clone, Debug)]
pub(crate) struct Case {
    /// The case's name, its key in [`FIXTURE`].
    pub(crate) name: Cow<'static, str>,
    /// The files after `t.qc`, each with its text.
    pub(crate) files: &'static [(&'static str, &'static str)],
    /// The text of `t.qc`.
    pub(crate) source: Cow<'static, str>,
    /// What `qc-codegen` does otherwise than id's `qcc`, as a QC-27 statement says.
    pub(crate) divergence: Option<Divergence>,
}

/// A case `qc-codegen` compiles otherwise than id's `qcc` on purpose.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Divergence {
    /// The QC-27 statement that says so.
    pub(crate) statement: &'static str,
    /// What `qc-codegen` does instead: compile it, or refuse it with this message.
    pub(crate) ours: Option<&'static str>,
}

impl Divergence {
    /// Whether `ours` is what the statement says.
    fn holds(&self, ours: &Ours) -> bool {
        match self.ours {
            None => matches!(ours.outcome, Outcome::Progs(_)),
            Some(message) => ours.messages.iter().any(|m| m == message),
        }
    }
}

impl Case {
    /// Every file, in compilation order.
    fn all_files(&self) -> Vec<(&str, &str)> {
        let mut files = vec![("t.qc", self.source.as_ref())];
        files.extend_from_slice(self.files);
        files
    }
}

/// Every case: [`CASES`], then the generated programs, `generated-000` on.
pub(crate) fn all_cases() -> Vec<Case> {
    let mut cases = CASES.to_vec();
    for n in 0..generate::PROGRAMS {
        cases.push(Case {
            name: Cow::Owned(format!("generated-{n:03}")),
            files: &[],
            source: Cow::Owned(generate::program(n)),
            divergence: None,
        });
    }
    cases
}

/// What a compiler did with a case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Wrote a progs.dat with this SHA-256.
    Progs(String),
    /// Refused it; the first error's message.
    Error(String),
    /// Ended without a progs or an error message: id's `qcc` crashing (QC-25.7).
    Crash,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Progs(digest) => write!(f, "progs {digest}"),
            Self::Error(message) => write!(f, "error {message}"),
            Self::Crash => f.write_str("crash"),
        }
    }
}

impl Outcome {
    /// Reads the form [`std::fmt::Display`] writes.
    fn parse(text: &str) -> Option<Self> {
        if text == "crash" {
            return Some(Self::Crash);
        }
        let (kind, rest) = text.split_once(' ')?;
        match kind {
            "progs" => Some(Self::Progs(rest.to_owned())),
            "error" => Some(Self::Error(rest.to_owned())),
            _ => None,
        }
    }
}

/// What `qc-codegen` did with a case.
pub(crate) struct Ours {
    /// Its outcome: an internal error is an error whose message says so.
    pub(crate) outcome: Outcome,
    /// Every error message it reported.
    pub(crate) messages: Vec<String>,
    /// The progs it wrote.
    pub(crate) bytes: Option<Vec<u8>>,
}

/// Compiles `case` with `qc-codegen`.
pub(crate) fn ours(case: &Case) -> Ours {
    let files = case.all_files();
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (index, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(index), PathBuf::from(name), text.as_bytes().to_vec());
        input.compiled.push(FileId(index));
    }
    let result = compile_module(
        &mut db,
        &input,
        |id| {
            files
                .get(id.0)
                .map(|(n, _)| (*n).to_owned())
                .unwrap_or_default()
        },
        Options::default(),
    );
    let refused = |message: String| Ours {
        outcome: Outcome::Error(message.clone()),
        messages: vec![message],
        bytes: None,
    };
    match result {
        Ok(compiled) => match bytes(&compiled) {
            Ok(bytes) => Ours {
                outcome: Outcome::Progs(sha256::hex_digest(&bytes)),
                messages: Vec::new(),
                bytes: Some(bytes),
            },
            Err(e) => refused(e),
        },
        Err(CompileError::Diagnostics(diagnostics)) => {
            let messages: Vec<String> = diagnostics
                .into_iter()
                .map(|d| d.diagnostic.message)
                .collect();
            let first = messages.first().cloned().unwrap_or_default();
            Ours {
                outcome: Outcome::Error(first),
                messages,
                bytes: None,
            }
        }
        Err(other) => refused(format!("internal: {other:?}")),
    }
}

/// Runs id's `qcc` on `case` in a directory of its own.
fn oracle(oracles: &Oracles, case: &Case) -> Result<Outcome, String> {
    let dir = oracles.work.join("codegen").join(case.name.as_ref());
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let files = case.all_files();
    let mut list = String::from("progs.dat\n");
    for (name, text) in &files {
        fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"))?;
        list.push_str(name);
        list.push('\n');
    }
    fs::write(dir.join("progs.src"), list).map_err(|e| e.to_string())?;
    let output = oracles
        .exec
        .output_within(Command::new(&oracles.qcc).current_dir(&dir), ORACLE_LIMIT)
        .map_err(|e| format!("cannot run qcc: {e}"))?;
    let Some(output) = output else {
        return Err(format!("{}: qcc did not finish", case.name));
    };
    let log = String::from_utf8_lossy(&output.stdout).into_owned();
    if let Some(message) = first_error(&log, &files) {
        return Ok(Outcome::Error(message));
    }
    let progs = dir.join("progs.dat");
    if !output.status.success() || !progs.is_file() {
        return Ok(Outcome::Crash);
    }
    let bytes = fs::read(&progs).map_err(|e| format!("{}: {e}", progs.display()))?;
    Ok(Outcome::Progs(sha256::hex_digest(&bytes)))
}

/// The first error `qcc` printed: a `FILE:LINE:message` line of one of `files`, "function NAME
/// was not defined", or the message after `cmdlib.c`'s `Error` banner.
fn first_error(log: &str, files: &[(&str, &str)]) -> Option<String> {
    let mut lines = log.lines();
    while let Some(line) = lines.next() {
        for (name, _) in files {
            if let Some(rest) = line.strip_prefix(name).and_then(|r| r.strip_prefix(':'))
                && let Some((number, message)) = rest.split_once(':')
                && !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
            {
                return Some(message.to_owned());
            }
        }
        if line.starts_with("function ") && line.ends_with(" was not defined") {
            return Some(line.to_owned());
        }
        if line.contains("************ ERROR ************") {
            return lines.next().map(str::to_owned);
        }
    }
    None
}

/// Whether `ours` (with every message ours reported) agrees with `theirs`: the same progs, or
/// both refusing, ours with `qcc`'s message among its own.
fn agree(ours: &Outcome, messages: &[String], theirs: &Outcome) -> bool {
    match (ours, theirs) {
        (Outcome::Progs(a), Outcome::Progs(b)) => a == b,
        (Outcome::Error(_), Outcome::Error(message)) => messages.iter().any(|m| m == message),
        (Outcome::Progs(_) | Outcome::Error(_) | Outcome::Crash, _) => false,
    }
}

/// Reads [`FIXTURE`].
pub(crate) fn read_fixture(root: &Path) -> Result<BTreeMap<String, Outcome>, String> {
    let path = root.join(FIXTURE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: BTreeMap<String, String> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    raw.into_iter()
        .map(|(key, outcome)| {
            Outcome::parse(&outcome)
                .map(|outcome| (key.clone(), outcome))
                .ok_or_else(|| format!("{FIXTURE}: {key}: bad outcome `{outcome}`"))
        })
        .collect()
}

/// Compares `qc-codegen` with `recorded`, `qcc`'s outcomes: each case must agree, but a case
/// that diverges under a QC-27 statement, which must not. Prints each failure; returns whether
/// every case passed. With `work`, the directory `qcc` wrote each case's progs in, a case whose
/// two progs differ is shown table by table ([`differences`]).
pub(crate) fn compare(
    recorded: &BTreeMap<String, Outcome>,
    work: Option<&Path>,
) -> Result<bool, String> {
    let mut ok = true;
    let mut agreed = 0_usize;
    let cases = all_cases();
    for case in &cases {
        let Some(theirs) = recorded.get(case.name.as_ref()) else {
            println!("FAIL  codegen {}: not recorded in {FIXTURE}", case.name);
            ok = false;
            continue;
        };
        let ours = ours(case);
        let outcome = &ours.outcome;
        let same = agree(outcome, &ours.messages, theirs);
        match (same, case.divergence) {
            (true, None) => agreed = agreed.saturating_add(1),
            (false, Some(divergence)) if divergence.holds(&ours) => {}
            (false, Some(divergence)) => {
                ok = false;
                println!(
                    "FAIL  codegen {}: ours {outcome}, not as {} says",
                    case.name, divergence.statement
                );
            }
            (false, None) => {
                ok = false;
                println!("FAIL  codegen {}: ours {outcome}, qcc {theirs}", case.name);
                if let (Some(work), Some(bytes), Outcome::Progs(_)) = (work, &ours.bytes, theirs) {
                    let path = work.join(case.name.as_ref()).join("progs.dat");
                    let theirs = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    for difference in differences(bytes, &theirs)? {
                        println!("      {difference}");
                    }
                }
            }
            (true, Some(divergence)) => {
                ok = false;
                println!(
                    "FAIL  codegen {}: listed as diverging ({}) but agrees",
                    case.name, divergence.statement
                );
            }
        }
    }
    if ok {
        let diverging = cases.iter().filter(|c| c.divergence.is_some()).count();
        println!(
            "ok    codegen: {agreed} programs compile as qcc compiles them; {diverging} diverge \
             as QC-27 says"
        );
    }
    Ok(ok)
}

/// `cargo xtask oracles codegen [--record]`: runs every case through `qcc`, checks the
/// outcomes against [`FIXTURE`] (or with `record` writes them there), then compares
/// `qc-codegen` with them.
pub(crate) fn check(root: &Path, oracles: &Oracles, record: bool) -> Result<bool, String> {
    let mut outcomes = BTreeMap::new();
    for case in &all_cases() {
        outcomes.insert(case.name.to_string(), oracle(oracles, case)?);
    }
    let mut ok = true;
    if record {
        let raw: BTreeMap<&String, String> =
            outcomes.iter().map(|(k, v)| (k, v.to_string())).collect();
        let text = serde_json::to_string_pretty(&raw).map_err(|e| e.to_string())?;
        let path = root.join(FIXTURE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, format!("{text}\n")).map_err(|e| format!("{}: {e}", path.display()))?;
        println!(
            "oracles: recorded {} codegen outcomes in {FIXTURE}",
            outcomes.len()
        );
    } else {
        let recorded = read_fixture(root).unwrap_or_else(|e| {
            println!("FAIL  {e}");
            BTreeMap::new()
        });
        if recorded.len() != outcomes.len() {
            ok = false;
            println!("FAIL  {FIXTURE} does not record every case (--record)");
        }
        for (name, outcome) in &outcomes {
            if recorded.get(name) != Some(outcome) {
                ok = false;
                println!("FAIL  codegen {name}: qcc gives {outcome}, not as {FIXTURE} records");
            }
        }
    }
    let work = oracles.work.join("codegen");
    Ok(compare(&outcomes, Some(&work))? && ok)
}
