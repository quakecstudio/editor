// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario tests of the VM oracle's own C (P1-35, `oracles/README.md`, Readiness): small
//! programs, built here with the scenario, map or init file around them, that the VM oracle
//! runs, each with what `docs/spec/differential.md` and `docs/spec/trace.md` say the run does:
//! its exit status and the records and values its trace holds. Each case in [`cases`] cites the
//! statements it pins; `cargo xtask oracles` runs every case after the vectors and probes.
//!
//! The expectations are written from the spec, not from the C, so a fault in the C fails a
//! case (the reason the oracles take no mutation score, `ratchets.toml`). Every trace is also
//! decoded by `qc-trace`'s strict decoder and must re-encode to the same bytes.

mod cases;
mod errors;
mod frames;
#[cfg(test)]
mod tests;
mod trace;
mod world;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::crc::crc_block;
use qc_progs::{Def, Function, ProgsFile, Statement, write};
use qc_trace::{Record, StringValue, Trace, decode, encode};
use qc_types::Slot;

use super::Oracles;
use super::vectors::parse_statement;

/// The return slot and the parameter slots of a call (`vm.md`).
pub(super) const RETURN: u32 = 1;
pub(super) const PARMS: [u32; 8] = [4, 7, 10, 13, 16, 19, 22, 25];

/// System globals, `docs/spec/edicts.md` EDICT-2.1.
pub(super) mod global {
    pub(crate) const SELF: u32 = 28;
    pub(crate) const OTHER: u32 = 29;
    pub(crate) const WORLD: u32 = 30;
    pub(crate) const TIME: u32 = 31;
    pub(crate) const FRAMETIME: u32 = 32;
    pub(crate) const FORCE_RETOUCH: u32 = 33;
    pub(crate) const MAPNAME: u32 = 34;
    pub(crate) const DEATHMATCH: u32 = 35;
    pub(crate) const COOP: u32 = 36;
    pub(crate) const SERVERFLAGS: u32 = 38;
    pub(crate) const PARM1: u32 = 43;
    pub(crate) const V_FORWARD: u32 = 59;
    pub(crate) const TRACE_ALLSOLID: u32 = 68;
    pub(crate) const TRACE_STARTSOLID: u32 = 69;
    pub(crate) const TRACE_FRACTION: u32 = 70;
    pub(crate) const TRACE_ENDPOS: u32 = 71;
    pub(crate) const TRACE_PLANE_NORMAL: u32 = 74;
    pub(crate) const TRACE_PLANE_DIST: u32 = 77;
    pub(crate) const TRACE_ENT: u32 = 78;
    pub(crate) const TRACE_INOPEN: u32 = 79;
    pub(crate) const TRACE_INWATER: u32 = 80;
    pub(crate) const MSG_ENTITY: u32 = 81;
    pub(crate) const MAIN: u32 = 82;
    pub(crate) const START_FRAME: u32 = 83;
    pub(crate) const PLAYER_PRE_THINK: u32 = 84;
    pub(crate) const PLAYER_POST_THINK: u32 = 85;
    pub(crate) const CLIENT_CONNECT: u32 = 87;
    pub(crate) const PUT_CLIENT_IN_SERVER: u32 = 88;
    pub(crate) const CLIENT_DISCONNECT: u32 = 89;
    pub(crate) const SET_NEW_PARMS: u32 = 90;
    pub(crate) const SET_CHANGE_PARMS: u32 = 91;
}

/// System fields, `docs/spec/edicts.md` EDICT-3.1.
pub(super) mod field {
    pub(crate) const MODELINDEX: u32 = 0;
    pub(crate) const ABSMIN: u32 = 1;
    pub(crate) const ABSMAX: u32 = 4;
    pub(crate) const LTIME: u32 = 7;
    pub(crate) const MOVETYPE: u32 = 8;
    pub(crate) const SOLID: u32 = 9;
    pub(crate) const ORIGIN: u32 = 10;
    pub(crate) const VELOCITY: u32 = 16;
    pub(crate) const ANGLES: u32 = 19;
    pub(crate) const CLASSNAME: u32 = 28;
    pub(crate) const MODEL: u32 = 29;
    pub(crate) const EFFECTS: u32 = 32;
    pub(crate) const MINS: u32 = 33;
    pub(crate) const MAXS: u32 = 36;
    pub(crate) const SIZE: u32 = 39;
    pub(crate) const TOUCH: u32 = 42;
    pub(crate) const THINK: u32 = 44;
    pub(crate) const NEXTTHINK: u32 = 46;
    pub(crate) const GROUNDENTITY: u32 = 47;
    pub(crate) const HEALTH: u32 = 48;
    pub(crate) const BUTTON0: u32 = 65;
    pub(crate) const BUTTON2: u32 = 67;
    pub(crate) const IMPULSE: u32 = 68;
    pub(crate) const V_ANGLE: u32 = 70;
    pub(crate) const NETNAME: u32 = 74;
    pub(crate) const FLAGS: u32 = 76;
    pub(crate) const COLORMAP: u32 = 77;
    pub(crate) const TEAM: u32 = 78;
    pub(crate) const IDEAL_YAW: u32 = 85;
    pub(crate) const SPAWNFLAGS: u32 = 89;
    pub(crate) const MESSAGE: u32 = 99;
}

/// The types of a definition (`progs.md` PROGS-9).
pub(super) mod ty {
    pub(crate) const STRING: u32 = 1;
    pub(crate) const FLOAT: u32 = 2;
    pub(crate) const VECTOR: u32 = 3;
    pub(crate) const ENTITY: u32 = 4;
    pub(crate) const FIELD: u32 = 5;
    pub(crate) const FUNCTION: u32 = 6;
}

/// The first global slot a program keeps its own constants and temporaries in; the slots
/// below are the return, parameter and system globals.
const FIRST_FREE: u32 = 100;

/// A progs.dat for scenario mode: id's header CRC, 128 globals and 110 fields by default,
/// function 1 `noop` and function 2 `worldspawn` (each an empty function), every system
/// function global naming `noop`, and the field definitions map spawning reads
/// (`classname`, `spawnflags`).
#[derive(Clone, Debug)]
pub(super) struct Program {
    statements: Vec<Statement>,
    functions: Vec<Function>,
    strings: Vec<u8>,
    globals: Vec<u32>,
    field_defs: Vec<Def>,
    next_slot: u32,
    /// Statements that did not parse; [`Program::bytes`] fails with them.
    errors: Vec<String>,
    pub(super) entityfields: i32,
    pub(super) crc: i32,
}

/// The function every system function global names by default.
pub(super) const NOOP: i32 = 1;
/// The spawn function of the default map's only entity.
pub(super) const WORLDSPAWN: i32 = 2;

impl Program {
    pub(super) fn new() -> Self {
        let mut program = Self {
            statements: vec![Statement::default()],
            functions: vec![Function::default()],
            strings: vec![0],
            globals: vec![0; 128],
            field_defs: Vec::new(),
            next_slot: FIRST_FREE,
            errors: Vec::new(),
            entityfields: 110,
            crc: qc_progs::PROGS_HEADER_CRC,
        };
        program.func("noop", &["DONE".to_owned()]);
        program.func("worldspawn", &["DONE".to_owned()]);
        for slot in global::MAIN..=global::SET_CHANGE_PARMS {
            program.set(slot, NOOP.cast_unsigned());
        }
        program.field_def("classname", ty::STRING, field::CLASSNAME);
        program.field_def("spawnflags", ty::FLOAT, field::SPAWNFLAGS);
        program
    }

    /// The offset of `text` in the string table, adding it.
    pub(super) fn text(&mut self, text: &str) -> i32 {
        let offset = i32::try_from(self.strings.len()).unwrap_or(i32::MAX);
        self.strings.extend_from_slice(text.as_bytes());
        self.strings.push(0);
        offset
    }

    /// Adds a QC function with these statements (each `OPCODE a b c`, as the VM vectors
    /// write them); returns its index.
    pub(super) fn func(&mut self, name: &str, code: &[String]) -> i32 {
        let first = self.code(code);
        self.function(name, first)
    }

    /// Gives function `index` these statements in place of its own.
    pub(super) fn body(&mut self, index: i32, code: &[String]) {
        let first = self.code(code);
        let slot = usize::try_from(index).unwrap_or(usize::MAX);
        match self.functions.get_mut(slot) {
            Some(function) => function.first_statement = first,
            None => self.errors.push(format!("no function {index}")),
        }
    }

    /// The index the next statement added will have.
    pub(super) fn next_statement(&self) -> i32 {
        i32::try_from(self.statements.len()).unwrap_or(i32::MAX)
    }

    /// The index the next function added will have.
    pub(super) fn next_function(&self) -> i32 {
        i32::try_from(self.functions.len()).unwrap_or(i32::MAX)
    }

    /// Appends statements; returns the index of the first.
    fn code(&mut self, code: &[String]) -> i32 {
        let first = i32::try_from(self.statements.len()).unwrap_or(i32::MAX);
        for line in code {
            match parse_statement(line) {
                Ok(statement) => self.statements.push(statement),
                Err(error) => {
                    self.errors.push(format!("{line:?}: {error}"));
                    self.statements.push(Statement::default());
                }
            }
        }
        first
    }

    /// Adds a function, QC (`first` above 0) or a builtin (`first` minus its number).
    fn function(&mut self, name: &str, first: i32) -> i32 {
        let s_name = self.text(name);
        self.functions.push(Function {
            first_statement: first,
            s_name,
            ..Function::default()
        });
        i32::try_from(self.functions.len().saturating_sub(1)).unwrap_or(i32::MAX)
    }

    /// Adds builtin `number` as a function; returns a global slot holding the function, the
    /// operand of a `CALL`.
    pub(super) fn builtin(&mut self, name: &str, number: i32) -> u32 {
        let index = self.function(name, number.saturating_neg());
        self.int(index.cast_unsigned())
    }

    /// A global slot holding the function index `index`, for a `CALL`.
    pub(super) fn fref(&mut self, index: i32) -> u32 {
        self.int(index.cast_unsigned())
    }

    /// A new global slot holding `bits`.
    pub(super) fn int(&mut self, bits: u32) -> u32 {
        let slot = self.temp();
        self.set(slot, bits);
        slot
    }

    /// A new global slot holding the float `value`.
    pub(super) fn float(&mut self, value: f32) -> u32 {
        self.int(value.to_bits())
    }

    /// Three new global slots holding a vector; returns the first.
    pub(super) fn vector(&mut self, value: [f32; 3]) -> u32 {
        let first = self.next_slot;
        for component in value {
            self.float(component);
        }
        first
    }

    /// A new global slot holding the string `text` of the string table.
    pub(super) fn string(&mut self, text: &str) -> u32 {
        let offset = self.text(text);
        self.int(offset.cast_unsigned())
    }

    /// A new global slot holding 0.
    pub(super) fn temp(&mut self) -> u32 {
        let slot = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        let size = usize::try_from(self.next_slot).unwrap_or(usize::MAX);
        if self.globals.len() < size {
            self.globals.resize(size, 0);
        }
        slot
    }

    /// Sets global `slot` to `bits` in the progs file.
    pub(super) fn set(&mut self, slot: u32, bits: u32) {
        let index = usize::try_from(slot).unwrap_or(usize::MAX);
        if let Some(value) = self.globals.get_mut(index) {
            *value = bits;
        }
    }

    /// Makes the system function global `slot` name `function`.
    pub(super) fn system(&mut self, slot: u32, function: i32) {
        self.set(slot, function.cast_unsigned());
    }

    /// Adds a field definition, which map spawning looks keys up in (EDICT-8.2).
    pub(super) fn field_def(&mut self, name: &str, ty: u32, slot: u32) {
        let s_name = self.text(name);
        self.field_defs.push(Def {
            type_bits: ty,
            ofs: slot,
            s_name,
        });
    }

    /// Makes the progs `globals` slots long, fewer than the 128 a program starts with.
    pub(super) fn truncate_globals(&mut self, globals: usize) {
        self.globals.truncate(globals);
    }

    /// Makes the progs at least `slots` globals long, and its next new slot at least `slots`.
    pub(super) fn reserve_globals(&mut self, slots: u32) {
        let size = usize::try_from(slots).unwrap_or(usize::MAX);
        if self.globals.len() < size {
            self.globals.resize(size, 0);
        }
        self.next_slot = self.next_slot.max(slots);
    }

    /// Gives function `index` `locals` slots from `parm_start`, which a call saves and its
    /// return restores (VM-15).
    pub(super) fn set_function_locals(&mut self, index: i32, parm_start: u32, locals: i32) {
        let slot = usize::try_from(index).unwrap_or(usize::MAX);
        match self.functions.get_mut(slot) {
            Some(function) => {
                function.parm_start = parm_start.cast_signed();
                function.locals = locals;
            }
            None => self.errors.push(format!("no function {index}")),
        }
    }

    /// The progs.dat.
    pub(super) fn bytes(&self) -> Result<Vec<u8>, String> {
        if !self.errors.is_empty() {
            return Err(format!("bad program: {}", self.errors.join("; ")));
        }
        let file = ProgsFile {
            crc: self.crc,
            entityfields: self.entityfields,
            statements: self.statements.clone(),
            field_defs: self.field_defs.clone(),
            functions: self.functions.clone(),
            strings: self.strings.clone(),
            globals: self.globals.iter().map(|bits| Slot(*bits)).collect(),
            ..ProgsFile::default()
        };
        write(&file).map_err(|e| format!("cannot write the progs: {e:?}"))
    }

    /// The text at `offset` of the string table, up to its NUL.
    fn table_text(&self, offset: u32) -> Option<&[u8]> {
        let start = usize::try_from(offset).ok()?;
        let rest = self.strings.get(start..)?;
        let end = rest.iter().position(|byte| *byte == 0)?;
        rest.get(..end)
    }

    /// The value of global `slot` in the progs file.
    fn initial(&self, slot: u32) -> u32 {
        usize::try_from(slot)
            .ok()
            .and_then(|index| self.globals.get(index))
            .copied()
            .unwrap_or(0)
    }

    // --- Code: each returns statements for `func` ---

    /// `CALLn function`, after storing each argument into its parameter slots: a vector
    /// argument is three slots, stored with `STORE_V`.
    pub(super) fn call(&mut self, function: u32, args: &[Arg]) -> Vec<String> {
        let mut code = Vec::new();
        for (arg, parm) in args.iter().zip(PARMS) {
            code.push(match arg {
                Arg::F(slot) => format!("STORE_F {slot} {parm}"),
                Arg::V(slot) => format!("STORE_V {slot} {parm}"),
            });
        }
        code.push(format!("CALL{} {function}", args.len()));
        code
    }

    /// `entity.field = value` for the entity in global `entity`, through a pointer: one slot,
    /// copied as 32 bits.
    pub(super) fn write_field(&mut self, entity: u32, field: u32, value: u32) -> Vec<String> {
        self.write_through(entity, field, value, "STOREP_F")
    }

    /// `entity.field = value` for a vector field.
    pub(super) fn write_vector_field(
        &mut self,
        entity: u32,
        field: u32,
        value: u32,
    ) -> Vec<String> {
        self.write_through(entity, field, value, "STOREP_V")
    }

    fn write_through(&mut self, entity: u32, field: u32, value: u32, store: &str) -> Vec<String> {
        let field = self.int(field);
        let pointer = self.temp();
        vec![
            format!("ADDRESS {entity} {field} {pointer}"),
            format!("{store} {value} {pointer}"),
        ]
    }

    /// `out = entity.field`, a float.
    pub(super) fn read_field(&mut self, entity: u32, field: u32, out: u32) -> Vec<String> {
        let field = self.int(field);
        vec![format!("LOAD_F {entity} {field} {out}")]
    }
}

/// One argument of [`Program::call`]: a float (or entity or string) slot, or a vector's first
/// slot.
#[derive(Clone, Copy, Debug)]
pub(super) enum Arg {
    F(u32),
    V(u32),
}

/// What the oracle reads.
#[derive(Clone, Debug)]
pub(super) enum Input {
    /// Scenario mode: the bytes of the scenario file `case.scn`, and other files, each a path
    /// relative to the case's directory and its bytes.
    Scenario {
        text: Vec<u8>,
        files: Vec<(String, Vec<u8>)>,
    },
    /// Vector mode: the init file's text.
    Vector { init: String },
    /// Scenario mode, with the oracle run from the case's directory on `case.scn` and
    /// `trace.bin`, named without a directory.
    Relative(Vec<u8>),
    /// The command line after the program's name, as given.
    Args(Vec<String>),
}

/// A scenario file: `progs progs.dat`, then `lines`.
pub(super) fn scenario(lines: &str) -> Input {
    Input::Scenario {
        text: format!("progs progs.dat\n{lines}").into_bytes(),
        files: Vec::new(),
    }
}

/// A scenario with `map.ent` as its entities file.
pub(super) fn scenario_with_map(lines: &str, entities: &str) -> Input {
    Input::Scenario {
        text: format!("progs progs.dat\nentities map.ent\n{lines}").into_bytes(),
        files: vec![("map.ent".to_owned(), entities.as_bytes().to_vec())],
    }
}

/// A scenario file of exactly these bytes, with these other files.
pub(super) fn raw_scenario(text: &[u8], files: &[(&str, &[u8])]) -> Input {
    Input::Scenario {
        text: text.to_vec(),
        files: files
            .iter()
            .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
            .collect(),
    }
}

/// A value a check expects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Val {
    /// These bits exactly.
    Bits(u32),
    /// This float, bit for bit.
    F(f32),
    /// The entity value of this edict, through the trace's header (TRACE-7.1).
    Entity(u32),
    /// A string value naming this text, in the string table or as an engine string
    /// (TRACE-7.3).
    Text(&'static str),
    /// An engine string with this text: a value outside the string table that a string
    /// record names (DIFF-3.8).
    EngineText(&'static str),
}

/// A record a check looks for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Want {
    /// A run record of this function.
    Run(i32),
    /// An exit record.
    Exit,
    /// A step record of this statement.
    Step(i32),
    /// A step record of this function and statement.
    StepIn(i32, i32),
    /// A step record calling this builtin.
    Builtin(u32),
    /// A step record with no builtin, of this statement.
    StepWithoutBuiltin(i32),
    /// An error record with this function, statement and message.
    Error(i32, i32, &'static str),
    /// An error record with this message.
    ErrorMessage(&'static str),
    /// A string record with this text.
    String(&'static str),
    /// A change-carrying record whose change set gives global `slot` this value.
    Global(u32, Val),
    /// A change-carrying record whose change set gives this field of this edict this value.
    Field(u32, u32, Val),
    /// A change-carrying record whose change set lists this global, whatever its value.
    GlobalChanged(u32),
    /// A change-carrying record whose change set lists any field of this edict.
    EdictChanged(u32),
    /// A change-carrying record whose change set lists this field of this edict.
    FieldChanged(u32, u32),
    /// One record that is each of these.
    All(Vec<Want>),
}

/// One expectation of a case.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Check {
    /// The exit status, and text the standard error holds. A refusal (status 2) also means no
    /// trace file was written.
    Exit(i32, &'static str),
    /// Text the standard output holds.
    Stdout(&'static str),
    /// These records, in this order, not necessarily adjacent.
    Seq(Vec<Want>),
    /// No record is this.
    Never(Want),
    /// Exactly this many records are this.
    Count(Want, usize),
    /// The last record is this.
    Last(Want),
    /// The value of a global at the end of the trace (its last change, or its value in the
    /// progs file).
    FinalGlobal(u32, Val),
    /// The value of a field at the end of the trace (its last change, or 0).
    FinalField(u32, u32, Val),
    /// The header's numbers: `entityfields` and `numstrings`; `progs_crc` is always the
    /// file's.
    Header { entityfields: u32, numstrings: u32 },
    /// A second run writes the same bytes (DIFF-1.4).
    SameTwice,
    /// These bytes start the trace file.
    Prefix(Vec<u8>),
}

/// Where the trace goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TracePath {
    /// In the case's directory.
    Normal,
    /// In a directory that does not exist, so it cannot be opened.
    Missing,
    /// This file, which can be opened but not written: Linux's `/dev/full`.
    Device(&'static str),
}

/// One scenario test.
#[derive(Clone, Debug)]
pub(super) struct Case {
    pub(super) name: String,
    pub(super) program: Program,
    /// Bytes written as progs.dat in place of the program's.
    pub(super) progs: Option<Vec<u8>>,
    pub(super) input: Input,
    pub(super) trace: TracePath,
    /// Runs the oracle with `--console`, which copies the console to standard error.
    pub(super) console: bool,
    pub(super) checks: Vec<Check>,
}

impl Case {
    pub(super) fn new(name: &str, program: Program, input: Input) -> Self {
        Self {
            name: name.to_owned(),
            program,
            progs: None,
            input,
            trace: TracePath::Normal,
            console: false,
            checks: Vec::new(),
        }
    }

    pub(super) fn check(mut self, check: Check) -> Self {
        self.checks.push(check);
        self
    }
}

/// What one run of the oracle gave.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub(super) status: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
    /// The trace file, if one was written.
    pub(super) trace: Option<Vec<u8>>,
    /// The trace of a second run, for [`Check::SameTwice`].
    pub(super) again: Option<Vec<u8>>,
}

/// Every case, in the order `cargo xtask oracles` runs them.
pub(super) fn all_cases() -> Vec<Case> {
    cases::all()
}

/// Runs every case, or only the cases `names` names; `Ok(false)` when one fails.
pub(crate) fn check(oracles: &Oracles, names: Option<&[&str]>) -> Result<bool, String> {
    let work = oracles.work.join("scenarios");
    let (mut passed, mut failed) = (0_u32, 0_u32);
    let mut cases = all_cases();
    if let Some(names) = names.filter(|names| !names.is_empty()) {
        cases.retain(|case| names.contains(&case.name.as_str()));
        if cases.len() != names.len() {
            return Err(format!("no scenario test named one of {names:?}"));
        }
    }
    for case in cases {
        let result = run(oracles, &work, &case).and_then(|outcome| verify(&case, &outcome));
        match result {
            Ok(()) => passed = passed.saturating_add(1),
            Err(problem) => {
                failed = failed.saturating_add(1);
                println!("FAIL  scenario test {}: {problem}", case.name);
            }
        }
    }
    println!("oracles: {passed} scenario tests passed, {failed} failed");
    Ok(failed == 0)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes the case's files in a fresh directory and runs the oracle on them.
fn run(oracles: &Oracles, work: &Path, case: &Case) -> Result<Outcome, String> {
    let dir = work.join(&case.name);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let progs = match &case.progs {
        Some(bytes) => bytes.clone(),
        None => case.program.bytes()?,
    };
    write_file(&dir.join("progs.dat"), &progs)?;
    let trace = match case.trace {
        TracePath::Normal => dir.join("trace.bin"),
        TracePath::Missing => dir.join("missing").join("trace.bin"),
        TracePath::Device(path) => PathBuf::from(path),
    };
    let mut args: Vec<PathBuf> = Vec::new();
    if case.console {
        args.push("--console".into());
    }
    args.extend(match &case.input {
        Input::Scenario { text, files } => {
            write_file(&dir.join("case.scn"), text)?;
            for (path, bytes) in files {
                let path = dir.join(path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                write_file(&path, bytes)?;
            }
            vec!["scenario".into(), dir.join("case.scn"), trace.clone()]
        }
        Input::Vector { init } => {
            write_file(&dir.join("init.txt"), init.as_bytes())?;
            vec![
                "vector".into(),
                dir.join("progs.dat"),
                dir.join("init.txt"),
                trace.clone(),
            ]
        }
        Input::Relative(text) => {
            write_file(&dir.join("case.scn"), text)?;
            vec!["scenario".into(), "case.scn".into(), "trace.bin".into()]
        }
        Input::Args(words) => words.iter().map(PathBuf::from).collect(),
    });
    let cwd = if matches!(case.input, Input::Relative(_)) {
        dir.clone()
    } else {
        work.to_path_buf()
    };
    let readable = match case.trace {
        TracePath::Normal | TracePath::Missing => Some(trace.as_path()),
        TracePath::Device(_) => None,
    };
    let mut outcome = invoke(oracles, &cwd, &args, readable)?;
    if case.checks.contains(&Check::SameTwice) {
        outcome.again = invoke(oracles, &cwd, &args, readable)?.trace;
    }
    Ok(outcome)
}

/// One run of the oracle, from `cwd`, which is not the case's directory, so that a path in
/// a scenario is read relative to the scenario (DIFF-2.1). `trace` is the file the run's trace
/// is read from, removed first, or `None` for a device, which is neither.
fn invoke(
    oracles: &Oracles,
    cwd: &Path,
    args: &[PathBuf],
    trace: Option<&Path>,
) -> Result<Outcome, String> {
    if let Some(trace) = trace.filter(|trace| trace.exists()) {
        fs::remove_file(trace).map_err(|e| format!("{}: {e}", trace.display()))?;
    }
    let output = oracles
        .exec
        .output(Command::new(&oracles.vm).args(args).current_dir(cwd))
        .map_err(|e| format!("cannot run the VM oracle: {e}"))?;
    Ok(Outcome {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        trace: trace.and_then(|trace| fs::read(trace).ok()),
        again: None,
    })
}

/// Whether `outcome` meets every check of `case`.
pub(super) fn verify(case: &Case, outcome: &Outcome) -> Result<(), String> {
    let mut status_checked = false;
    for check in &case.checks {
        if let Check::Exit(status, text) = check {
            status_checked = true;
            if outcome.status != Some(*status) {
                return Err(format!(
                    "exit status {:?}, expected {status}: {}",
                    outcome.status,
                    outcome.stderr.trim()
                ));
            }
            if !outcome.stderr.contains(text) {
                return Err(format!(
                    "standard error does not say {text:?}: {}",
                    outcome.stderr.trim()
                ));
            }
            if *status == EXIT_REFUSED && outcome.trace.is_some() {
                return Err("a refused run wrote a trace (DIFF-2.9, DIFF-3.1)".to_owned());
            }
        }
    }
    if !status_checked && outcome.status != Some(0) {
        return Err(format!(
            "exit status {:?}, expected 0: {}",
            outcome.status,
            outcome.stderr.trim()
        ));
    }
    let wants_trace = case
        .checks
        .iter()
        .any(|check| !matches!(check, Check::Exit(..) | Check::Stdout(_) | Check::Prefix(_)));
    let trace = match &outcome.trace {
        Some(bytes) => {
            let trace = decode(bytes).map_err(|e| format!("the trace does not decode: {e:?}"))?;
            if encode(&trace) != *bytes {
                return Err("the trace does not re-encode to the same bytes (TRACE-5.7)".to_owned());
            }
            Some(trace)
        }
        None if wants_trace => return Err("no trace was written".to_owned()),
        None => None,
    };
    let progs = match &case.progs {
        Some(bytes) => bytes.clone(),
        None => case.program.bytes()?,
    };
    for check in &case.checks {
        verify_one(case, outcome, trace.as_ref(), &progs, check)?;
    }
    Ok(())
}

const EXIT_REFUSED: i32 = 2;

fn verify_one(
    case: &Case,
    outcome: &Outcome,
    trace: Option<&Trace>,
    progs: &[u8],
    check: &Check,
) -> Result<(), String> {
    let reader = Reader {
        program: &case.program,
        trace,
    };
    match check {
        Check::Exit(..) => Ok(()),
        Check::Stdout(text) => {
            if outcome.stdout.contains(text) {
                Ok(())
            } else {
                Err(format!(
                    "standard output does not say {text:?}: {}",
                    outcome.stdout
                ))
            }
        }
        Check::Prefix(bytes) => match &outcome.trace {
            Some(trace) if trace.starts_with(bytes) => Ok(()),
            Some(trace) => Err(format!(
                "the trace starts {:02x?}, expected {bytes:02x?}",
                trace.get(..bytes.len()).unwrap_or(trace)
            )),
            None => Err("no trace was written".to_owned()),
        },
        Check::SameTwice => {
            if outcome.again.is_some() && outcome.again == outcome.trace {
                Ok(())
            } else {
                Err("a second run wrote different bytes (DIFF-1.4)".to_owned())
            }
        }
        Check::Seq(wants) => reader.seq(wants),
        Check::Never(want) => match reader.count(want)? {
            0 => Ok(()),
            n => Err(format!("{n} records are {want:?}")),
        },
        Check::Count(want, expected) => match reader.count(want)? {
            n if n == *expected => Ok(()),
            n => Err(format!("{n} records are {want:?}, expected {expected}")),
        },
        Check::Last(want) => {
            let trace = reader.trace()?;
            let last = trace.records.len().saturating_sub(1);
            match trace.records.last() {
                Some(record) if reader.matches(last, record, want) => Ok(()),
                other => Err(format!("the last record is {other:?}, expected {want:?}")),
            }
        }
        Check::FinalGlobal(slot, val) => {
            let bits = reader.final_global(*slot)?;
            reader.expect_value(
                reader.trace()?.records.len(),
                bits,
                *val,
                &format!("global {slot}"),
            )
        }
        Check::FinalField(edict, field, val) => {
            let bits = reader.final_field(*edict, *field)?;
            reader.expect_value(
                reader.trace()?.records.len(),
                bits,
                *val,
                &format!("e{edict}.{field}"),
            )
        }
        Check::Header {
            entityfields,
            numstrings,
        } => {
            let header = reader.trace()?.header;
            let crc = crc_block(progs);
            if header.progs_crc() != crc
                || header.entityfields() != *entityfields
                || header.numstrings() != *numstrings
            {
                return Err(format!(
                    "header {header:?}, expected crc {crc}, entityfields {entityfields}, \
                     numstrings {numstrings}"
                ));
            }
            Ok(())
        }
    }
}

/// Reads a case's trace for the checks.
pub(super) struct Reader<'a> {
    program: &'a Program,
    trace: Option<&'a Trace>,
}

impl<'a> Reader<'a> {
    fn trace(&self) -> Result<&'a Trace, String> {
        self.trace.ok_or_else(|| "no trace was written".to_owned())
    }

    /// Whether `bits`, just before record `before`, is `val`.
    fn is(&self, before: usize, bits: u32, val: Val) -> bool {
        match val {
            Val::Bits(want) => bits == want,
            Val::F(want) => bits == want.to_bits(),
            Val::Entity(edict) => {
                self.trace
                    .and_then(|trace| trace.header.entity_number(bits))
                    == Some(edict)
            }
            Val::Text(text) => match self.trace.map(|trace| trace.string_value(before, bits)) {
                Some(StringValue::Table(offset)) => {
                    self.program.table_text(offset) == Some(text.as_bytes())
                }
                Some(StringValue::Engine(engine)) => engine.as_bytes() == text.as_bytes(),
                Some(StringValue::Unknown) | None => false,
            },
            Val::EngineText(text) => match self.trace.map(|trace| trace.string_value(before, bits))
            {
                Some(StringValue::Engine(engine)) => engine.as_bytes() == text.as_bytes(),
                Some(StringValue::Table(_) | StringValue::Unknown) | None => false,
            },
        }
    }

    fn expect_value(&self, before: usize, bits: u32, val: Val, what: &str) -> Result<(), String> {
        if self.is(before, bits, val) {
            Ok(())
        } else {
            Err(format!(
                "{what} is {bits:#010x} at the end, expected {val:?}"
            ))
        }
    }

    pub(super) fn matches(&self, index: usize, record: &Record, want: &Want) -> bool {
        match (want, record) {
            (Want::Run(f), Record::Run { function, .. }) => function == f,
            (Want::Exit, Record::Exit { .. }) => true,
            (Want::Step(s), Record::Step(step)) => step.statement == *s,
            (Want::StepIn(f, s), Record::Step(step)) => step.function == *f && step.statement == *s,
            (Want::Builtin(b), Record::Step(step)) => step.builtin == Some(*b),
            (Want::StepWithoutBuiltin(s), Record::Step(step)) => {
                step.statement == *s && step.builtin.is_none()
            }
            (
                Want::Error(f, s, m),
                Record::Error {
                    function,
                    statement,
                    message,
                    ..
                },
            ) => function == f && statement == s && message.as_bytes() == m.as_bytes(),
            (Want::ErrorMessage(m), Record::Error { message, .. }) => {
                message.as_bytes() == m.as_bytes()
            }
            (Want::String(t), Record::String { text, .. }) => text.as_bytes() == t.as_bytes(),
            (Want::Global(slot, val), record) => record.changes().is_some_and(|changes| {
                changes
                    .globals()
                    .iter()
                    .any(|g| g.slot == *slot && self.is(index, g.bits, *val))
            }),
            (Want::Field(edict, field, val), record) => record.changes().is_some_and(|changes| {
                changes
                    .fields()
                    .iter()
                    .any(|f| f.edict == *edict && f.field == *field && self.is(index, f.bits, *val))
            }),
            (Want::GlobalChanged(slot), record) => record
                .changes()
                .is_some_and(|changes| changes.globals().iter().any(|g| g.slot == *slot)),
            (Want::EdictChanged(edict), record) => record
                .changes()
                .is_some_and(|changes| changes.fields().iter().any(|f| f.edict == *edict)),
            (Want::FieldChanged(edict, field), record) => record.changes().is_some_and(|changes| {
                changes
                    .fields()
                    .iter()
                    .any(|f| f.edict == *edict && f.field == *field)
            }),
            (Want::All(wants), record) => {
                !wants.is_empty() && wants.iter().all(|want| self.matches(index, record, want))
            }
            _ => false,
        }
    }

    fn seq(&self, wants: &[Want]) -> Result<(), String> {
        let trace = self.trace()?;
        let mut records = trace.records.iter().enumerate();
        for (number, want) in wants.iter().enumerate() {
            if !records.any(|(index, record)| self.matches(index, record, want)) {
                return Err(format!(
                    "no record is {want:?} after the first {number} of the sequence {wants:?}"
                ));
            }
        }
        Ok(())
    }

    fn count(&self, want: &Want) -> Result<usize, String> {
        let trace = self.trace()?;
        Ok(trace
            .records
            .iter()
            .enumerate()
            .filter(|(index, record)| self.matches(*index, record, want))
            .count())
    }

    fn final_global(&self, slot: u32) -> Result<u32, String> {
        let mut bits = self.program.initial(slot);
        for record in &self.trace()?.records {
            if let Some(change) = record
                .changes()
                .and_then(|changes| changes.globals().iter().find(|g| g.slot == slot))
            {
                bits = change.bits;
            }
        }
        Ok(bits)
    }

    fn final_field(&self, edict: u32, field: u32) -> Result<u32, String> {
        let mut bits = 0;
        for record in &self.trace()?.records {
            if let Some(change) = record.changes().and_then(|changes| {
                changes
                    .fields()
                    .iter()
                    .find(|f| f.edict == edict && f.field == field)
            }) {
                bits = change.bits;
            }
        }
        Ok(bits)
    }
}
