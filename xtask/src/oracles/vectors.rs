// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the VM vectors of `docs/spec/vectors/vm/` (format in its README) through the VM
//! oracle and checks its trace against each vector's expectations: the acceptance criterion
//! of P1-11 that the oracle passes every vector without `oracle: no`, the VM-9.2 vectors on an
//! x86-64 host.
//!
//! A vector marked `builtins: engine` runs id's builtins instead of the stubs, for the argument
//! checks of P1-31 (`builtins.txt`).
//!
//! Each vector becomes a progs.dat (written by `qc-progs`) and an init file holding the
//! initial globals and fields, which the oracle resolves itself because entity and pointer
//! values depend on its edict layout. The oracle's binary trace is decoded by `qc-trace`'s
//! strict decoder, which also checks the oracle's C encoder: encoding the decoded trace must
//! give back the same bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::crc::crc_block;
use qc_progs::opcode::Opcode;
use qc_progs::{Function, Progs, ProgsFile, Statement, write};
use qc_trace::{Changes, Header, Record, Step, Trace, decode, encode};
use qc_types::Slot;

use super::Oracles;

const VECTORS: &str = "docs/spec/vectors/vm";
const ENTITY_FIELDS: u32 = 110;
const DEFAULT_GLOBALS: usize = 128;
const DEFAULT_EDICTS: u32 = 3;
const RUNAWAY: &str = "runaway loop error";

/// A value as a vector writes it (README, "Values").
#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Bits(u32),
    AnyNan,
    Entity(u32),
    Pointer(u32, u32),
    EngineString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Global(u32),
    Field(u32, u32),
}

/// What a line of `expect:` or of the trace is: a statement, or the start of a run the host
/// made (listed only in vectors with a `host:` block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    Step { statement: i32, opcode: u32 },
    Run(i32),
}

#[derive(Debug)]
struct ExpectLine {
    head: Head,
    changes: Vec<(Target, Value)>,
}

/// One line of a `host:` block (README, "Host actions").
#[derive(Clone, Debug, PartialEq)]
enum HostAction {
    Clock(f64),
    Run(i32),
    Think(u32, f64),
    Push(u32, f64),
    Physics(f64, u32),
    Frame(f64, u32),
    Edict(i32, u32),
}

#[derive(Debug, PartialEq)]
enum End {
    Done,
    Error(Option<String>),
}

#[derive(Debug, Default)]
struct Vector {
    file: String,
    name: String,
    spec: Vec<String>,
    initial: Vec<(Target, Value)>,
    strings: Vec<Vec<u8>>,
    numglobals: Option<usize>,
    edicts: Option<u32>,
    maxclients: Option<u32>,
    seed: Option<u32>,
    host: Vec<HostAction>,
    datagram: Option<Vec<u8>>,
    loading: bool,
    engine_builtins: bool,
    entry: Option<i32>,
    functions: BTreeMap<usize, Function>,
    steps: Option<usize>,
    after: Option<(i32, i32)>,
    oracle: bool,
    code: Vec<Statement>,
    partial: bool,
    lines: Vec<ExpectLine>,
    end: Option<End>,
}

/// One statement, or a host run, as the trace shows it, in the order the vectors list them.
#[derive(Debug)]
struct Line {
    head: Head,
    changes: BTreeMap<Target, u32>,
}

/// Runs every vector; returns whether all that must pass did.
pub(crate) fn check(root: &Path, oracles: &Oracles) -> Result<bool, String> {
    let dir = root.join(VECTORS);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("{VECTORS}: {e}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();
    let work = oracles.work.join("vectors");
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let x86_64 = cfg!(target_arch = "x86_64");
    let (mut passed, mut skipped, mut expected, mut failed) = (0_u32, 0_u32, 0_u32, 0_u32);
    for path in &files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let text = fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
        for vector in parse(&name, &text)? {
            if !vector.oracle {
                skipped = skipped.saturating_add(1);
                continue;
            }
            match verdict(&vector, run(oracles, &work, &vector), x86_64) {
                Verdict::Passed => passed = passed.saturating_add(1),
                Verdict::Expected(problem) => {
                    expected = expected.saturating_add(1);
                    println!(
                        "note  {}:{}: expected divergence on a host that is not x86 \
                         (differential.md DIFF-10.1, VM-9.2): {problem}",
                        vector.file, vector.name
                    );
                }
                Verdict::Failed(problem) => {
                    failed = failed.saturating_add(1);
                    println!("FAIL  {}:{}: {problem}", vector.file, vector.name);
                }
            }
        }
    }
    println!(
        "oracles: {passed} vectors passed, {expected} expected divergences, {failed} failed, \
         {skipped} not run against the oracle (oracle: no)"
    );
    if passed.saturating_add(failed).saturating_add(expected) == 0 {
        return Err(format!("no vectors found in {VECTORS}"));
    }
    Ok(failed == 0)
}

/// What one vector's run counts as.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Passed,
    /// A divergence `differential.md` DIFF-10.1 lists.
    Expected(String),
    /// Any other divergence (DIFF-10.3).
    Failed(String),
}

/// A failed run is an expected divergence only for a vector of VM-9.2's out-of-range float
/// conversions on a host that is not x86-64 (DIFF-10.1); any other is a failure (DIFF-10.3).
fn verdict(vector: &Vector, result: Result<(), String>, x86_64: bool) -> Verdict {
    match result {
        Ok(()) => Verdict::Passed,
        Err(problem) if !x86_64 && vector.spec.iter().any(|id| id == "VM-9.2") => {
            Verdict::Expected(problem)
        }
        Err(problem) => Verdict::Failed(problem),
    }
}

// --- Parsing the vector files ---

fn parse(file: &str, text: &str) -> Result<Vec<Vector>, String> {
    let mut vectors: Vec<Vector> = Vec::new();
    let mut block = "";
    for (raw, number) in text.lines().zip(1_u32..) {
        let at = |what: &str| format!("{file}:{number}: {what}");
        let line = if raw.starts_with("strings:") {
            raw
        } else {
            raw.split('#').next().unwrap_or_default()
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("== ") {
            vectors.push(Vector {
                file: file.to_owned(),
                name: name.trim().to_owned(),
                oracle: true,
                ..Vector::default()
            });
            block = "";
            continue;
        }
        let vector = vectors
            .last_mut()
            .ok_or_else(|| at("text before the first vector"))?;
        if let Some(body) = line.strip_prefix("  ") {
            match block {
                "code" => vector.code.push(parse_statement(body).map_err(|e| at(&e))?),
                "expect" => parse_expect_line(vector, body.trim()).map_err(|e| at(&e))?,
                "functions" => parse_function(vector, body.trim()).map_err(|e| at(&e))?,
                "host" => vector
                    .host
                    .push(parse_host_action(body.trim()).map_err(|e| at(&e))?),
                _ => return Err(at("indented line outside a block")),
            }
            continue;
        }
        let (key, rest) = line
            .split_once(':')
            .ok_or_else(|| at("expected `key: value`"))?;
        let rest = rest.trim();
        block = "";
        match key {
            "code" | "expect" | "functions" | "host" => block = key,
            "spec" => vector.spec = rest.split(',').map(|s| s.trim().to_owned()).collect(),
            "globals" => {
                for item in rest.split_whitespace() {
                    let (slot, value) = item.split_once('=').ok_or_else(|| at("bad global"))?;
                    let slot = slot.parse().map_err(|_| at("bad global slot"))?;
                    let value = parse_value(value, true).map_err(|e| at(&e))?;
                    vector.initial.push((Target::Global(slot), value));
                }
            }
            "fields" => {
                for item in rest.split_whitespace() {
                    let (target, value) = item.split_once('=').ok_or_else(|| at("bad field"))?;
                    let target = parse_target(target).map_err(|e| at(&e))?;
                    let value = parse_value(value, true).map_err(|e| at(&e))?;
                    vector.initial.push((target, value));
                }
            }
            "strings" => vector.strings = parse_strings(rest).map_err(|e| at(&e))?,
            "numglobals" => {
                vector.numglobals = Some(rest.parse().map_err(|_| at("bad numglobals"))?);
            }
            "edicts" => vector.edicts = Some(rest.parse().map_err(|_| at("bad edicts"))?),
            "maxclients" => {
                vector.maxclients = Some(rest.parse().map_err(|_| at("bad maxclients"))?);
            }
            "seed" => vector.seed = Some(rest.parse().map_err(|_| at("bad seed"))?),
            "datagram" => {
                let bytes: Result<Vec<u8>, _> = rest.split_whitespace().map(str::parse).collect();
                vector.datagram = Some(bytes.map_err(|_| at("bad datagram"))?);
            }
            "server" if rest == "loading" => vector.loading = true,
            "builtins" if rest == "engine" => vector.engine_builtins = true,
            "entry" => vector.entry = Some(rest.parse().map_err(|_| at("bad entry"))?),
            "steps" => vector.steps = Some(rest.parse().map_err(|_| at("bad steps"))?),
            "after" => vector.after = Some(parse_after(rest).map_err(|e| at(&e))?),
            "oracle" if rest == "no" => vector.oracle = false,
            _ => return Err(at(&format!("unknown key `{key}`"))),
        }
    }
    Ok(vectors)
}

/// `depth=<d> localstack=<n>` (README, "Format"): both, each once, in that order.
fn parse_after(rest: &str) -> Result<(i32, i32), String> {
    let bad = || format!("bad after `{rest}`: want `depth=<d> localstack=<n>`");
    let words: Vec<&str> = rest.split_whitespace().collect();
    let [depth, localstack] = words.as_slice() else {
        return Err(bad());
    };
    let number = |word: &str, key: &str| {
        word.strip_prefix(key)
            .and_then(|v| v.strip_prefix('='))
            .and_then(|v| v.parse::<i32>().ok())
            .ok_or_else(bad)
    };
    Ok((number(depth, "depth")?, number(localstack, "localstack")?))
}

fn opcode_number(word: &str) -> Result<u32, String> {
    if let Some(op) = Opcode::ALL.iter().find(|op| op.name() == word) {
        return Ok(u32::from(op.number()));
    }
    word.parse::<u32>()
        .map_err(|_| format!("unknown opcode {word}"))
}

pub(super) fn parse_statement(body: &str) -> Result<Statement, String> {
    let mut words = body.split_whitespace();
    let op = opcode_number(words.next().unwrap_or_default())?;
    let mut operands = [0_i32; 3];
    for slot in &mut operands {
        if let Some(word) = words.next() {
            *slot = word.parse().map_err(|_| format!("bad operand {word}"))?;
        }
    }
    if let Some(word) = words.next() {
        return Err(format!("text after the operands: {word}"));
    }
    let [a, b, c] = operands;
    Ok(Statement { op, a, b, c })
}

fn parse_function(vector: &mut Vector, body: &str) -> Result<(), String> {
    let (index, rest) = body.split_once(':').ok_or("bad function line")?;
    let index: usize = index.trim().parse().map_err(|_| "bad function index")?;
    let mut function = Function::default();
    for item in rest.split_whitespace() {
        let (key, value) = item.split_once('=').ok_or("bad function field")?;
        let number = |v: &str| v.parse::<i32>().map_err(|_| format!("bad {key}"));
        match key {
            "first" => function.first_statement = number(value)?,
            "parm_start" => function.parm_start = number(value)?,
            "locals" => function.locals = number(value)?,
            "builtin" => {
                function.first_statement = number(value)?.checked_neg().ok_or("bad builtin")?;
            }
            "parms" => {
                let sizes: Vec<&str> = value.split(',').collect();
                function.numparms = i32::try_from(sizes.len()).map_err(|_| "too many parms")?;
                for (slot, size) in function.parm_size.iter_mut().zip(&sizes) {
                    *slot = size.parse().map_err(|_| "bad parm size")?;
                }
            }
            _ => return Err(format!("unknown function field {key}")),
        }
    }
    vector.functions.insert(index, function);
    Ok(())
}

fn parse_host_action(body: &str) -> Result<HostAction, String> {
    let words: Vec<&str> = body.split_whitespace().collect();
    let seconds = |w: &str| w.parse::<f64>().map_err(|_| format!("bad seconds {w}"));
    let count = |w: &str| {
        w.parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("bad count {w}"))
    };
    let edict = |w: &str| {
        w.strip_prefix('e')
            .and_then(|n| n.parse::<u32>().ok())
            .ok_or_else(|| format!("bad edict {w}"))
    };
    match words.as_slice() {
        ["clock", t] => Ok(HostAction::Clock(seconds(t)?)),
        ["run", f] => Ok(HostAction::Run(f.parse().map_err(|_| "bad function")?)),
        ["think", e, t] => Ok(HostAction::Think(edict(e)?, seconds(t)?)),
        ["push", e, t] => Ok(HostAction::Push(edict(e)?, seconds(t)?)),
        ["physics", t] => Ok(HostAction::Physics(seconds(t)?, 1)),
        ["physics", t, n] => Ok(HostAction::Physics(seconds(t)?, count(n)?)),
        ["frame", t] => Ok(HostAction::Frame(seconds(t)?, 1)),
        ["frame", t, n] => Ok(HostAction::Frame(seconds(t)?, count(n)?)),
        ["edict", n, slot] => Ok(HostAction::Edict(
            n.parse().map_err(|_| "bad edict number")?,
            slot.parse().map_err(|_| "bad global slot")?,
        )),
        _ => Err(format!("unknown host action `{body}`")),
    }
}

fn parse_target(text: &str) -> Result<Target, String> {
    if let Some(rest) = text.strip_prefix('e') {
        let (edict, field) = rest.split_once('.').ok_or("bad field target")?;
        let edict = edict.parse().map_err(|_| "bad edict")?;
        let field = field.parse().map_err(|_| "bad field")?;
        return Ok(Target::Field(edict, field));
    }
    Ok(Target::Global(
        text.parse().map_err(|_| format!("bad target {text}"))?,
    ))
}

/// The forms of the README's "Values" table; `nan` is 0x7FC00000 as an initial value.
fn parse_value(text: &str, initial: bool) -> Result<Value, String> {
    if text == "es" && !initial {
        return Ok(Value::EngineString);
    }
    if text == "nan" {
        return Ok(if initial {
            Value::Bits(0x7fc0_0000)
        } else {
            Value::AnyNan
        });
    }
    if let Some(hex) = text.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16)
            .map(Value::Bits)
            .map_err(|_| format!("bad value {text}"));
    }
    if let Some(int) = text.strip_prefix('i').filter(|_| text != "inf") {
        return int
            .parse::<i32>()
            .map(|v| Value::Bits(v.cast_unsigned()))
            .map_err(|_| format!("bad value {text}"));
    }
    if let Some(rest) = text.strip_prefix('e') {
        return rest
            .parse()
            .map(Value::Entity)
            .map_err(|_| format!("bad value {text}"));
    }
    if let Some(rest) = text.strip_prefix('p') {
        let (edict, field) = rest.split_once('.').ok_or("bad pointer")?;
        let edict = edict.parse().map_err(|_| "bad pointer")?;
        let field = field.parse().map_err(|_| "bad pointer")?;
        return Ok(Value::Pointer(edict, field));
    }
    text.parse::<f32>()
        .map(|f| Value::Bits(f.to_bits()))
        .map_err(|_| format!("bad value {text}"))
}

fn parse_strings(text: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut strings = Vec::new();
    let mut rest = text.trim();
    while let Some(after) = rest.strip_prefix('"') {
        let end = after.find('"').ok_or("unterminated string")?;
        let body = after.get(..end).unwrap_or_default();
        let mut bytes = Vec::new();
        let mut chars = body.split("\\x");
        if let Some(first) = chars.next() {
            bytes.extend_from_slice(first.as_bytes());
        }
        for piece in chars {
            let hex = piece.get(..2).ok_or("bad \\x escape")?;
            bytes.push(u8::from_str_radix(hex, 16).map_err(|_| "bad \\x escape")?);
            bytes.extend_from_slice(piece.get(2..).unwrap_or_default().as_bytes());
        }
        strings.push(bytes);
        rest = after
            .get(end.saturating_add(1)..)
            .unwrap_or_default()
            .trim();
    }
    if !rest.is_empty() {
        return Err("bad strings line".to_owned());
    }
    Ok(strings)
}

/// The `"<message>"` of an `error "<message>"` line: one quoted string with no quote inside it
/// and nothing after it. The comment rule cuts a message at `#`, which leaves it unterminated,
/// so such a line is refused rather than read as a shorter message.
fn parse_error_message(text: &str) -> Result<String, String> {
    text.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|message| !message.contains('"'))
        .map(str::to_owned)
        .ok_or_else(|| format!("bad error line `error {text}`: want `error \"<message>\"`"))
}

fn parse_expect_line(vector: &mut Vector, body: &str) -> Result<(), String> {
    if body == "..." {
        vector.partial = true;
        return Ok(());
    }
    if body == "done" {
        vector.end = Some(End::Done);
        return Ok(());
    }
    if body == "error" {
        vector.end = Some(End::Error(None));
        return Ok(());
    }
    if let Some(rest) = body.strip_prefix("error ") {
        vector.end = Some(End::Error(Some(parse_error_message(rest)?)));
        return Ok(());
    }
    let mut words = body.split_whitespace();
    let first = words.next().unwrap_or_default();
    let head = if first == "run" {
        let function = words.next().unwrap_or_default();
        Head::Run(function.parse().map_err(|_| "bad run function")?)
    } else {
        let statement = first.parse().map_err(|_| "bad statement index")?;
        let opcode = opcode_number(words.next().unwrap_or_default())?;
        Head::Step { statement, opcode }
    };
    let mut changes = Vec::new();
    for item in words {
        let (target, value) = item.split_once('=').ok_or("bad change")?;
        changes.push((parse_target(target)?, parse_value(value, false)?));
    }
    vector.lines.push(ExpectLine { head, changes });
    Ok(())
}

// --- Building the progs and running the oracle ---

fn build_progs(vector: &Vector) -> Result<Vec<u8>, String> {
    let mut statements = vec![Statement::default()];
    statements.extend(vector.code.iter().copied());
    let mut strings = vec![0_u8];
    for s in &vector.strings {
        strings.extend_from_slice(s);
        strings.push(0);
    }
    let last = vector
        .functions
        .keys()
        .next_back()
        .copied()
        .unwrap_or(1)
        .max(1);
    let mut functions = vec![Function::default(); last.saturating_add(1)];
    if let Some(entry) = functions.get_mut(1) {
        entry.first_statement = 1;
    }
    for (index, function) in &vector.functions {
        if let Some(slot) = functions.get_mut(*index) {
            *slot = *function;
        }
    }
    let file = ProgsFile {
        entityfields: i32::try_from(ENTITY_FIELDS).map_err(|e| e.to_string())?,
        statements,
        functions,
        strings,
        globals: vec![Slot(0); vector.numglobals.unwrap_or(DEFAULT_GLOBALS)],
        ..ProgsFile::default()
    };
    write(&file).map_err(|e| format!("cannot write the progs: {e:?}"))
}

fn init_text(vector: &Vector) -> String {
    let mut text = format!("edicts {}\n", vector.edicts.unwrap_or(DEFAULT_EDICTS));
    if let Some(maxclients) = vector.maxclients {
        text.push_str(&format!("maxclients {maxclients}\n"));
    }
    if let Some(seed) = vector.seed {
        text.push_str(&format!("seed {seed}\n"));
    }
    if vector.loading {
        text.push_str("loading\n");
    }
    if vector.engine_builtins {
        text.push_str("builtins engine\n");
    }
    text.push_str(&format!("entry {}\n", vector.entry.unwrap_or(1)));
    // f64's Display is the shortest text that reads back as the same double, as strtod reads it.
    for action in &vector.host {
        text.push_str(&match action {
            HostAction::Clock(t) => format!("host clock {t}\n"),
            HostAction::Run(f) => format!("host run {f}\n"),
            HostAction::Think(e, t) => format!("host think {e} {t}\n"),
            HostAction::Push(e, t) => format!("host push {e} {t}\n"),
            HostAction::Physics(t, n) => format!("host physics {t} {n}\n"),
            HostAction::Frame(t, n) => format!("host frame {t} {n}\n"),
            HostAction::Edict(n, slot) => format!("host edict {n} {slot}\n"),
        });
    }
    for (target, value) in &vector.initial {
        let place = match target {
            Target::Global(slot) => format!("global {slot}"),
            Target::Field(edict, field) => format!("field {edict} {field}"),
        };
        let value = match value {
            Value::Bits(bits) => format!("bits {bits:08x}"),
            Value::AnyNan => "bits 7fc00000".to_owned(),
            // Only an expected value (parse_value).
            Value::EngineString => continue,
            Value::Entity(edict) => format!("entity {edict}"),
            Value::Pointer(edict, field) => format!("pointer {edict} {field}"),
        };
        text.push_str(&format!("{place} {value}\n"));
    }
    text
}

fn run(oracles: &Oracles, work: &Path, vector: &Vector) -> Result<(), String> {
    let progs = build_progs(vector)?;
    let progs_path = work.join("progs.dat");
    let init_path = work.join("init.txt");
    let trace_path = work.join("trace.bin");
    fs::write(&progs_path, &progs).map_err(|e| e.to_string())?;
    fs::write(&init_path, init_text(vector)).map_err(|e| e.to_string())?;
    let output = Command::new(&oracles.vm)
        .arg("vector")
        .arg(&progs_path)
        .arg(&init_path)
        .arg(&trace_path)
        .output()
        .map_err(|e| format!("cannot run the VM oracle: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "the VM oracle failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let bytes = fs::read(&trace_path).map_err(|e| e.to_string())?;
    let trace = decode(&bytes).map_err(|e| format!("the trace does not decode: {e:?}"))?;
    if encode(&trace) != bytes {
        return Err("the trace does not re-encode to the same bytes (TRACE-5.7)".to_owned());
    }
    check_header(&trace.header, &progs)?;
    let (lines, end) = trace_lines(&trace, vector)?;
    let known = Known {
        header: &trace.header,
        engine_strings: trace
            .records
            .iter()
            .filter_map(|record| match record {
                Record::String { value, .. } => Some(*value),
                _ => None,
            })
            .collect(),
    };
    compare(vector, &known, &lines, &end)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut report = stdout.lines();
    let state = report.next().unwrap_or_default();
    if let Some((depth, localstack)) = vector.after {
        let want = format!("depth={depth} localstack={localstack}");
        if state != want {
            return Err(format!("after the run: {state}, the vector says {want}"));
        }
    }
    if let Some(bytes) = &vector.datagram {
        let want: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let got = report
            .next()
            .and_then(|l| l.strip_prefix("datagram="))
            .unwrap_or_default();
        if got != want {
            return Err(format!(
                "the datagram holds `{got}`, the vector says `{want}`"
            ));
        }
    }
    Ok(())
}

fn check_header(header: &Header, progs: &[u8]) -> Result<(), String> {
    if header.progs_crc() != crc_block(progs) {
        return Err(format!(
            "header progs_crc {} is not the file's CRC {} (TRACE-2.1, PROGS-6.1)",
            header.progs_crc(),
            crc_block(progs)
        ));
    }
    let file = Progs::read(progs).map_err(|e| format!("cannot read the progs back: {e:?}"))?;
    let numstrings = u32::try_from(file.header().numstrings).ok();
    if header.entityfields() != ENTITY_FIELDS || numstrings != Some(header.numstrings()) {
        return Err(format!("header {header:?} does not describe the progs"));
    }
    Ok(())
}

// --- From records to the vectors' lines ---

/// A run being read: its function, its statements, and nested runs waiting for the `CALL`
/// that started them, with what the builtin changed before each.
struct Frame {
    function: i32,
    lines: Vec<Line>,
    nested: Vec<(Changes, Vec<Line>)>,
}

fn merge(into: &mut BTreeMap<Target, u32>, changes: &Changes) {
    for g in changes.globals() {
        into.insert(Target::Global(g.slot), g.bits);
    }
    for f in changes.fields() {
        into.insert(Target::Field(f.edict, f.field), f.bits);
    }
}

fn opcode_at(vector: &Vector, statement: i32) -> u32 {
    usize::try_from(statement)
        .ok()
        .and_then(|s| s.checked_sub(1))
        .and_then(|s| vector.code.get(s))
        .map_or(0, |st| st.op)
}

fn first_statement(vector: &Vector, function: i32) -> i32 {
    let Ok(index) = usize::try_from(function) else {
        return 0;
    };
    match vector.functions.get(&index) {
        Some(f) => f.first_statement,
        None if index == 1 => 1,
        None => 0,
    }
}

/// Places a step in its frame: a `CALL` whose builtin started nested runs comes before their
/// statements, carrying what the builtin changed (the README's order).
fn push_step(frame: &mut Frame, step: &Step) {
    let mut changes = BTreeMap::new();
    let nested = std::mem::take(&mut frame.nested);
    for (before, _) in &nested {
        merge(&mut changes, before);
    }
    merge(&mut changes, &step.changes);
    frame.lines.push(Line {
        head: Head::Step {
            statement: step.statement,
            opcode: u32::from(step.opcode),
        },
        changes,
    });
    for (_, lines) in nested {
        frame.lines.extend(lines);
    }
}

/// The builtin a `CALL` at `statement` calls, read from the function slot's value before the
/// statement ran, as TRACE-4.3 says the step records it.
fn expected_builtin(vector: &Vector, globals: &BTreeMap<u32, u32>, statement: i32) -> Option<u32> {
    let index = usize::try_from(statement).ok()?.checked_sub(1)?;
    let st = vector.code.get(index)?;
    let calls = u32::from(Opcode::Call0.number())..=u32::from(Opcode::Call8.number());
    if !calls.contains(&st.op) {
        return None;
    }
    let slot = u32::try_from(st.a).ok()?;
    let function = usize::try_from(globals.get(&slot).copied().unwrap_or(0)).ok()?;
    let first = vector.functions.get(&function)?.first_statement;
    let number = u32::try_from(first.checked_neg()?).ok()?;
    (number > 0 && number < 79).then_some(number)
}

fn trace_lines(trace: &Trace, vector: &Vector) -> Result<(Vec<Line>, End), String> {
    let mut stack: Vec<(Frame, Changes)> = Vec::new();
    let mut done: Vec<Line> = Vec::new();
    let mut runs = 0_u32;
    let mut globals: BTreeMap<u32, u32> = vector
        .initial
        .iter()
        .filter_map(|(target, value)| match (target, value) {
            (Target::Global(slot), Value::Bits(bits)) => Some((*slot, *bits)),
            _ => None,
        })
        .collect();
    for record in &trace.records {
        if let Record::Step(step) = record {
            let want = expected_builtin(vector, &globals, step.statement);
            if step.builtin != want {
                return Err(format!(
                    "statement {} records builtin {:?}, its CALL calls {want:?} (TRACE-4.3)",
                    step.statement, step.builtin
                ));
            }
        }
        if let Some(changes) = record.changes() {
            for g in changes.globals() {
                globals.insert(g.slot, g.bits);
            }
        }
        match record {
            Record::Run { function, changes } => {
                if stack.is_empty() {
                    // Without a `host:` block the host runs the entry function once.
                    if vector.host.is_empty() && runs > 0 {
                        return Err("a run after the entry run returned".to_owned());
                    }
                    runs = runs.saturating_add(1);
                    if !vector.host.is_empty() {
                        let mut run_changes = BTreeMap::new();
                        merge(&mut run_changes, changes);
                        done.push(Line {
                            head: Head::Run(*function),
                            changes: run_changes,
                        });
                    }
                }
                let frame = Frame {
                    function: *function,
                    lines: Vec::new(),
                    nested: Vec::new(),
                };
                stack.push((frame, changes.clone()));
            }
            Record::Step(step) => {
                let (frame, _) = stack.last_mut().ok_or("a step outside a run")?;
                push_step(frame, step);
            }
            Record::Exit { changes } => {
                if !changes.is_empty() {
                    return Err(format!("an exit record with changes {changes:?}"));
                }
                let (frame, before) = stack.pop().ok_or("an exit outside a run")?;
                match stack.last_mut() {
                    Some((parent, _)) => parent.nested.push((before, frame.lines)),
                    None => done.extend(frame.lines),
                }
            }
            Record::Error {
                function,
                statement,
                message,
                changes,
            } => {
                let message = String::from_utf8_lossy(message.as_bytes()).into_owned();
                done.extend(abort_lines(
                    vector, stack, *function, *statement, &message, changes,
                ));
                return Ok((done, End::Error(Some(message))));
            }
            Record::String { .. } => {}
        }
    }
    if !stack.is_empty() || runs == 0 {
        return Err("the trace ends inside a run, or has none".to_owned());
    }
    Ok((done, End::Done))
}

fn statement_of(line: &Line) -> i32 {
    match line.head {
        Head::Step { statement, .. } => statement,
        Head::Run(_) => 0,
    }
}

fn step_line(vector: &Vector, statement: i32, changes: BTreeMap<Target, u32>) -> Line {
    Line {
        head: Head::Step {
            statement,
            opcode: opcode_at(vector, statement),
        },
        changes,
    }
}

/// The lines of runs an error aborted: each nested run's `CALL` (which never finished, so has
/// no step record) before its statements, then the failing statement unless the error came
/// before a statement ran (TRACE-4.7) or stopped a runaway loop between statements.
fn abort_lines(
    vector: &Vector,
    stack: Vec<(Frame, Changes)>,
    function: i32,
    statement: i32,
    message: &str,
    changes: &Changes,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut runs = stack.into_iter().peekable();
    let mut innermost_function = None;
    let mut innermost_has_lines = false;
    while let Some((mut frame, _)) = runs.next() {
        let nested = std::mem::take(&mut frame.nested);
        frame.lines.extend(nested.into_iter().flat_map(|(_, l)| l));
        innermost_function = Some(frame.function);
        innermost_has_lines = !frame.lines.is_empty();
        let call = runs.peek().map(|(_, before)| {
            let at = frame.lines.last().map_or_else(
                || first_statement(vector, frame.function),
                |l| statement_of(l).saturating_add(1),
            );
            let mut call_changes = BTreeMap::new();
            merge(&mut call_changes, before);
            step_line(vector, at, call_changes)
        });
        lines.extend(frame.lines);
        lines.extend(call);
    }
    let stale = !innermost_has_lines && innermost_function != Some(function);
    if statement >= 0 && message != RUNAWAY && !stale {
        let mut error_changes = BTreeMap::new();
        merge(&mut error_changes, changes);
        lines.push(step_line(vector, statement, error_changes));
    }
    lines
}

// --- Comparing ---

/// What a trace says about its values: its edict layout and the engine strings it made.
struct Known<'a> {
    header: &'a Header,
    engine_strings: BTreeSet<u32>,
}

fn matches(known: &Known<'_>, value: Value, bits: u32) -> bool {
    let header = known.header;
    match value {
        Value::Bits(want) => bits == want,
        Value::AnyNan => f32::from_bits(bits).is_nan(),
        Value::Entity(edict) => header.entity_number(bits) == Some(edict),
        Value::Pointer(edict, field) => header.pointer_target(bits) == Some((edict, field)),
        Value::EngineString => known.engine_strings.contains(&bits),
    }
}

fn describe_head(head: Head) -> String {
    match head {
        Head::Step { statement, opcode } => format!("{statement} {opcode}"),
        Head::Run(function) => format!("run {function}"),
    }
}

fn describe(line: &Line) -> String {
    let mut text = describe_head(line.head);
    for (target, bits) in &line.changes {
        text.push_str(&match target {
            Target::Global(slot) => format!(" {slot}={bits:#010x}"),
            Target::Field(edict, field) => format!(" e{edict}.{field}={bits:#010x}"),
        });
    }
    text
}

fn compare_line(known: &Known<'_>, want: &ExpectLine, got: &Line) -> Result<(), String> {
    let same_changes = want.changes.len() == got.changes.len()
        && want.changes.iter().all(|(target, value)| {
            got.changes
                .get(target)
                .is_some_and(|bits| matches(known, *value, *bits))
        });
    if want.head == got.head && same_changes {
        return Ok(());
    }
    Err(format!(
        "expected `{} {:?}`, the oracle executed `{}`",
        describe_head(want.head),
        want.changes,
        describe(got)
    ))
}

fn compare(vector: &Vector, known: &Known<'_>, lines: &[Line], end: &End) -> Result<(), String> {
    if vector.partial {
        let steps = vector.steps.ok_or("a partial expectation without steps")?;
        let executed = lines
            .iter()
            .filter(|l| matches!(l.head, Head::Step { .. }))
            .count();
        if executed != steps {
            return Err(format!(
                "{executed} statements executed, the vector says {steps}"
            ));
        }
    } else if lines.len() != vector.lines.len() {
        let got: Vec<String> = lines.iter().map(describe).collect();
        return Err(format!(
            "{} lines in the trace, the vector lists {}: {got:?}",
            lines.len(),
            vector.lines.len()
        ));
    }
    let skip = lines.len().saturating_sub(vector.lines.len());
    for (index, (want, got)) in vector.lines.iter().zip(lines.iter().skip(skip)).enumerate() {
        compare_line(known, want, got)
            .map_err(|e| format!("line {}: {e}", index.saturating_add(skip).saturating_add(1)))?;
    }
    match (vector.end.as_ref(), end) {
        (Some(End::Done), End::Done) | (Some(End::Error(None)), End::Error(_)) => Ok(()),
        (Some(End::Error(Some(want))), End::Error(Some(got))) if want == got => Ok(()),
        (want, got) => Err(format!("ended with {got:?}, the vector says {want:?}")),
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{
        End, Head, HostAction, Known, Target, Value, Vector, Verdict, init_text, matches, parse,
        parse_host_action, parse_strings, parse_value, verdict,
    };
    use qc_trace::Header;

    fn vector(spec: &str) -> Vector {
        Vector {
            spec: vec![spec.to_owned()],
            ..Vector::default()
        }
    }

    // spec DIFF-10.1: a VM-9.2 vector that fails on a host that is not x86-64 is an expected
    // divergence, reported as such; on x86-64 it is a failure.
    #[test]
    fn an_out_of_range_conversion_is_expected_to_diverge_only_off_x86() {
        let bitand = vector("VM-9.2");
        let failure = || Err("line 1".to_owned());
        assert_eq!(
            verdict(&bitand, failure(), false),
            Verdict::Expected("line 1".to_owned())
        );
        assert_eq!(
            verdict(&bitand, failure(), true),
            Verdict::Failed("line 1".to_owned())
        );
        assert_eq!(verdict(&bitand, Ok(()), false), Verdict::Passed);
    }

    // spec DIFF-10.3: any divergence DIFF-10.1 does not list is a failure, on every host.
    #[test]
    fn any_other_divergence_is_a_failure() {
        for x86_64 in [false, true] {
            assert_eq!(
                verdict(&vector("VM-4.1"), Err("line 2".to_owned()), x86_64),
                Verdict::Failed("line 2".to_owned())
            );
        }
    }

    // spec DIFF-10.2: NaN payloads, the edict layout and engine string numbers are no
    // divergence: a value is compared through its reading.
    #[test]
    fn values_are_compared_through_their_readings() {
        let layouts = [
            Header::new(0, 516, 96, 105, 1000).unwrap(),
            Header::new(0, 528, 108, 105, 1000).unwrap(),
        ];
        for header in &layouts {
            let known = Known {
                header,
                engine_strings: [2000, 70000].into(),
            };
            let size = header.edict_size();
            assert!(matches(&known, Value::AnyNan, 0x7fc0_0000));
            assert!(matches(&known, Value::AnyNan, 0xffc0_0001));
            assert!(!matches(&known, Value::AnyNan, 0x7f80_0000));
            assert!(matches(&known, Value::Entity(2), size * 2));
            assert!(!matches(&known, Value::Entity(2), size * 3));
            let nextthink = size * 2 + header.field_offset() + 4 * 46;
            assert!(matches(&known, Value::Pointer(2, 46), nextthink));
            assert!(matches(&known, Value::EngineString, 2000));
            assert!(matches(&known, Value::EngineString, 70000));
            assert!(!matches(&known, Value::EngineString, 500));
            assert!(matches(&known, Value::Bits(0x8000_0000), 0x8000_0000));
            assert!(!matches(&known, Value::Bits(0x8000_0000), 0));
        }
    }

    #[test]
    fn parses_values_as_the_readme_defines_them() {
        assert_eq!(parse_value("1.5", false).unwrap(), Value::Bits(0x3fc0_0000));
        assert_eq!(parse_value("-0", false).unwrap(), Value::Bits(0x8000_0000));
        assert_eq!(parse_value("nan", false).unwrap(), Value::AnyNan);
        assert_eq!(parse_value("nan", true).unwrap(), Value::Bits(0x7fc0_0000));
        assert_eq!(parse_value("i-1", false).unwrap(), Value::Bits(u32::MAX));
        assert_eq!(parse_value("e2", false).unwrap(), Value::Entity(2));
        assert_eq!(
            parse_value("p1.105", false).unwrap(),
            Value::Pointer(1, 105)
        );
        assert_eq!(parse_value("es", false).unwrap(), Value::EngineString);
        assert!(parse_value("es", true).is_err());
        assert!(parse_value("x", false).is_err());
    }

    #[test]
    fn parses_strings_with_escapes() {
        let strings = parse_strings(r#""abc" "" "\x80\x7fz""#).unwrap();
        assert_eq!(
            strings,
            vec![b"abc".to_vec(), Vec::new(), vec![0x80, 0x7f, b'z']]
        );
        assert!(parse_strings(r#""open"#).is_err());
    }

    #[test]
    fn parses_a_vector() {
        let text = "== v\nspec: VM-1.1\nglobals: 100=e1\nfields: e1.30=2\noracle: no\n\
                    code:\n  STATE 100 101\n  66\nexpect:\n  1 STATE e1.30=nan\n  error\n";
        let vectors = parse("t.txt", text).unwrap();
        let v = vectors.first().unwrap();
        assert!(!v.oracle);
        assert!(!v.engine_builtins);
        assert_eq!(v.code.len(), 2);
        assert_eq!(v.initial.first().unwrap().0, Target::Global(100));
        assert_eq!(
            v.lines.first().unwrap().changes.first().unwrap().0,
            Target::Field(1, 30)
        );
        assert_eq!(v.end, Some(End::Error(None)));
        assert!(parse("t.txt", "loose\n").is_err());
        assert!(parse("t.txt", "== v\nwhat: 1\n").is_err());
        let engine = parse("t.txt", "== v\nbuiltins: engine\n").unwrap();
        assert!(engine.first().unwrap().engine_builtins);
        assert!(parse("t.txt", "== v\nbuiltins: stubs\n").is_err());
    }

    #[test]
    fn parses_host_actions() {
        assert_eq!(
            parse_host_action("clock 1.2000000000000002").unwrap(),
            HostAction::Clock(1.200_000_000_000_000_2)
        );
        assert_eq!(parse_host_action("run 3").unwrap(), HostAction::Run(3));
        assert_eq!(
            parse_host_action("think e1 0.1").unwrap(),
            HostAction::Think(1, 0.1)
        );
        assert_eq!(
            parse_host_action("push e2 0.1").unwrap(),
            HostAction::Push(2, 0.1)
        );
        assert_eq!(
            parse_host_action("physics 0.1").unwrap(),
            HostAction::Physics(0.1, 1)
        );
        assert_eq!(
            parse_host_action("frame 0.1 100").unwrap(),
            HostAction::Frame(0.1, 100)
        );
        assert_eq!(
            parse_host_action("edict -1 100").unwrap(),
            HostAction::Edict(-1, 100)
        );
        for bad in [
            "",
            "sleep 1",
            "clock",
            "clock soon",
            "think 1 0.1",
            "push e1",
            "frame 0.1 0",
            "physics 0.1 -1",
            "edict 1 -1",
            "run 1 2",
        ] {
            assert!(parse_host_action(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn parses_a_host_vector() {
        let text = "== v\nspec: DIFF-5.6\nedicts: 2\nmaxclients: 1\nseed: 7\n\
                    datagram: 64 255\nhost:\n  clock 1\n  run 1\ncode:\n  DONE\n\
                    expect:\n  run 1 31=1\n  1 DONE\n  done\n";
        let vectors = parse("t.txt", text).unwrap();
        let v = vectors.first().unwrap();
        assert_eq!(v.edicts, Some(2));
        assert_eq!(v.maxclients, Some(1));
        assert_eq!(v.seed, Some(7));
        assert_eq!(v.datagram, Some(vec![64, 255]));
        assert_eq!(v.host, [HostAction::Clock(1.0), HostAction::Run(1)]);
        assert_eq!(v.lines.first().unwrap().head, Head::Run(1));
        assert_eq!(
            v.lines.last().unwrap().head,
            Head::Step {
                statement: 1,
                opcode: 0
            }
        );
        let init = init_text(v);
        assert!(init.starts_with("edicts 2\nmaxclients 1\nseed 7\n"));
        assert!(init.contains("host clock 1\nhost run 1\n"));
        assert!(parse("t.txt", "== v\ndatagram: 256\n").is_err());
        assert!(parse("t.txt", "== v\nhost:\n  wait\n").is_err());
        assert!(parse("t.txt", "== v\nexpect:\n  run x\n").is_err());
        let plain = parse("t.txt", "== v\ncode:\n  DONE\n").unwrap();
        assert!(init_text(plain.first().unwrap()).starts_with("edicts 3\nentry 1\n"));
    }

    #[test]
    fn rejects_values_outside_32_bits() {
        assert_eq!(
            parse_value("0xffffffff", true).unwrap(),
            Value::Bits(u32::MAX)
        );
        assert!(parse_value("0x100000000", true).is_err());
        assert_eq!(
            parse_value("i-2147483648", true).unwrap(),
            Value::Bits(0x8000_0000)
        );
        assert!(parse_value("i2147483648", true).is_err());
        assert!(parse_value("p1", true).is_err());
        assert!(parse_value("e", true).is_err());
        assert_eq!(parse_value("inf", true).unwrap(), Value::Bits(0x7f80_0000));
    }

    #[test]
    fn rejects_malformed_string_escapes_and_trailing_text() {
        assert!(parse_strings(r#""\x4""#).is_err());
        assert!(parse_strings(r#""\xzz""#).is_err());
        assert!(parse_strings(r#""a" b"#).is_err());
        assert_eq!(parse_strings("").unwrap(), Vec::<Vec<u8>>::new());
        let hash = parse("t.txt", "== v\nstrings: \"a#b\"\n").unwrap();
        assert_eq!(hash.first().unwrap().strings, vec![b"a#b".to_vec()]);
    }

    #[test]
    fn keeps_a_quoted_error_message_and_a_bare_error() {
        let quoted = parse("t.txt", "== v\nexpect:\n  error \"NULL function\"\n").unwrap();
        assert_eq!(
            quoted.first().unwrap().end,
            Some(End::Error(Some("NULL function".to_owned())))
        );
        let bare = parse("t.txt", "== v\nexpect:\n  error\n").unwrap();
        assert_eq!(bare.first().unwrap().end, Some(End::Error(None)));
    }

    #[test]
    fn keeps_a_well_formed_after_line() {
        let vectors = parse("t.txt", "== v\nafter: depth=0 localstack=2\n").unwrap();
        assert_eq!(vectors.first().unwrap().after, Some((0, 2)));
        assert!(parse("t.txt", "== v\nafter: stack=2\n").is_err());
    }

    #[test]
    fn refuses_a_malformed_error_line() {
        for bad in ["error \"open", "errors", "error \"a # b\""] {
            let text = format!("== v\nexpect:\n  {bad}\n");
            assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
        }
        for bad in ["error \"a\" b", "error \"a\"b\"", "error x", "error \""] {
            let text = format!("== v\nexpect:\n  {bad}\n");
            assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
        }
        let empty = parse("t.txt", "== v\nexpect:\n  error \"\"\n").unwrap();
        assert_eq!(
            empty.first().unwrap().end,
            Some(End::Error(Some(String::new())))
        );
    }

    #[test]
    fn refuses_a_malformed_after_line() {
        for bad in ["depth=x localstack=2", "depth=0", "localstack=2"] {
            let text = format!("== v\nafter: {bad}\n");
            assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
        }
        for bad in [
            "",
            "localstack=2 depth=0",
            "depth=0 localstack=2 depth=1",
            "depth=0 localstack=y",
            "depth=0 localstack=",
            "depth0 localstack=2",
            "depth=99999999999 localstack=2",
        ] {
            let text = format!("== v\nafter: {bad}\n");
            assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn parses_every_vector_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(super::VECTORS);
        let mut count = 0_usize;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "txt") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(&path).unwrap();
                count = count.saturating_add(parse(&name, &text).unwrap().len());
            }
        }
        assert!(count > 0, "no vectors in {}", dir.display());
    }

    #[test]
    fn refuses_text_after_the_operands() {
        assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 2 3 4\n").is_err());
        assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 2 3\n").is_ok());
    }

    #[test]
    fn rejects_bad_operands_and_function_fields() {
        assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 x 3\n").is_err());
        assert!(parse("t.txt", "== v\ncode:\n  NOT_AN_OPCODE 1\n").is_err());
        assert!(parse("t.txt", "== v\nfunctions:\n  2: first=1 colour=3\n").is_err());
        assert!(parse("t.txt", "== v\nfunctions:\n  2: builtin=-2147483648\n").is_err());
        assert!(parse("t.txt", "== v\nfunctions:\n  x: first=1\n").is_err());
    }
}
