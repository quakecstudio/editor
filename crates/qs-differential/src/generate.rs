// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! A well-formed progs generator for the `qc-vm` differential fuzz target (FND-10, P1-15).
//!
//! `docs/spec/differential.md` DIFF-1 compares the id VM oracle and `qc-vm` on a *scenario*
//! (DIFF-2), which names a progs. The end-to-end suite ([`crate::runner`]) runs progs the
//! oracle compilers built from the corpus; this module instead builds a progs from fuzzer
//! bytes, so a fuzz run can feed both VMs an endless supply of programs and flag any
//! divergence (FND-10: "Fuzzing feeds well-formed random progs to both VMs and flags
//! divergence").
//!
//! "Well-formed" is the point. FND-10 wants progs that are *structurally valid so both VMs
//! accept and execute them*, not arbitrary bytes (that is the `qc-progs` loader's own fuzz
//! target). So this builds a version 6 file that the loader of both runners accepts: id's
//! header CRC, the 64 system globals and 105 system fields named at their fixed slots
//! (EDICT-2.1, EDICT-3.1) so map spawning and the host entry points resolve, and a
//! `StartFrame` function whose body is a bounded run of safe opcodes over in-range operands.
//! The opcode set is restricted to operations both VMs carry out identically on any host and
//! that touch only the program's own scratch globals (VM-4 to VM-13): the arithmetic,
//! comparison, boolean and store opcodes, plus forward-only `IF`, `IFNOT` and `GOTO`, ending
//! at `DONE`. It emits no `CALL`, `STATE`, `ADDRESS` or store-through-pointer, so the
//! generated program never leaves its own globals, never runs away and always reaches `DONE`,
//! and no `BITAND` or `BITOR`, the only opcodes whose float-to-int conversion (VM-9.2) is
//! host-dependent (DIFF-10.1), so a program built here does not depend on the oracle's host.
//! The operand *values* the globals start with are arbitrary, so the two VMs' arithmetic and
//! NaN handling (VM-4.3) are exercised and compared (DIFF-9).
//!
//! The generator is deterministic in its input bytes (DIFF-1.4): the same bytes always build
//! the same progs and scenario, so a crashing input minimises and replays.
//!
//! Implements no new spec statement: it is a test fixture for FND-10, built from the progs
//! format (`docs/spec/progs.md`) and the VM (`docs/spec/vm.md`). The scenario it returns is a
//! [`Scenario`] of DIFF-2.

use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::opcode::Opcode;
use qc_types::Etype;

use crate::scenario::{DIFF_DEFAULT_FRAMETIME, DIFF_DEFAULT_MAP, DIFF_DEFAULT_SEED, Scenario};

/// The version 6 header's format version (PROGS-3).
const PROGS_VERSION: i32 = 6;
/// id's `progdefs.h` header CRC (PROGS-3).
const PROGS_HEADER_CRC: i32 = 5927;
/// Bytes in a version 6 header (PROGS-2).
const HEADER_SIZE: usize = 60;

/// The global slots a program keeps its own scratch values in: slot 28 up is the system
/// globals (EDICT-2.1, slots 28 to 91), so a program's own globals start at 92
/// (`end_sys_globals`).
const FIRST_FREE: u32 = 92;
/// How many scratch globals the generated progs has beyond the system globals: enough for a
/// wide operand window without a huge file.
const SCRATCH_GLOBALS: u32 = 64;
/// How many scratch slots an operand base may be, leaving two slots above it so a vector
/// opcode's three slots (`a`, `a+1`, `a+2`, VM-6.3) all stay inside the scratch window: a VM
/// reads or writes `a+2`, and reading past the globals is the VM-1.4 error id's C leaves
/// undefined, an expected divergence (DIFF-10.1) the generator avoids by construction so every
/// program is one both VMs execute identically (FND-10).
const OPERAND_SLOTS: u32 = SCRATCH_GLOBALS - 2;
/// The total global count: the system globals and the scratch window above them.
const NUM_GLOBALS: u32 = FIRST_FREE + SCRATCH_GLOBALS;
/// Entity fields per edict: the 105 system fields (EDICT-3.3) and room above, as the corpus
/// progs have.
const ENTITY_FIELDS: i32 = 110;

/// The most statements a generated `StartFrame` body may have before its `DONE`, so the file
/// stays small and a run stays short.
const MAX_BODY: usize = 96;

/// The server frames a generated scenario runs: a handful, enough to call `StartFrame` and
/// the think path several times without a long run.
const FRAMES: u32 = 4;

/// A generated well-formed progs and the scenario that runs it (FND-10).
#[derive(Clone, Debug)]
pub struct Generated {
    /// The progs.dat bytes, a version 6 file both VMs accept (PROGS-2 to PROGS-13).
    pub progs: Vec<u8>,
    /// The scenario that runs it: [`FRAMES`] server frames, each calling `StartFrame`
    /// (DIFF-2, DIFF-5).
    pub scenario: Scenario,
}

/// The opcodes the generator emits for a statement body. Each is deterministic across the two
/// VMs and reads and writes only the program's own globals: no `CALL` (no nested run), no
/// `STATE`, no `ADDRESS` or `STOREP_*` (no field-pointer dereference, VM-12.5), no `EQ_S`,
/// `NE_S` or `NOT_S` (these read the string table at the operand's value, which an arbitrary
/// value does not index, id's VM-7.3 undefined read), no `RETURN` or `DONE` (added by the
/// generator to end the body), and no `BITAND` or `BITOR`. `BITAND` and `BITOR` are the only
/// vanilla opcodes that convert a float to an integer (VM-9.2), and that conversion is
/// host-dependent for a NaN, an infinity or an out-of-range float: id's x86 oracle gives
/// `0x80000000` where an arm64 oracle saturates, the one divergence DIFF-10.1 expects and only
/// on a non-x86 oracle host. Leaving them out keeps every generated program one both VMs
/// execute identically on any host (FND-10), so any divergence the target flags is a real
/// defect; the differential suite (P1-13) covers `BITAND` and `BITOR` against the x86 oracle
/// with that expected divergence handled. `IF`, `IFNOT` and `GOTO` are emitted as forward-only
/// jumps so the body always makes progress toward its `DONE` (no runaway).
const BODY_OPCODES: &[Opcode] = &[
    Opcode::MulF,
    Opcode::MulV,
    Opcode::MulFv,
    Opcode::MulVf,
    Opcode::DivF,
    Opcode::AddF,
    Opcode::AddV,
    Opcode::SubF,
    Opcode::SubV,
    Opcode::EqF,
    Opcode::EqV,
    Opcode::EqE,
    Opcode::EqFnc,
    Opcode::NeF,
    Opcode::NeV,
    Opcode::NeE,
    Opcode::NeFnc,
    Opcode::Le,
    Opcode::Ge,
    Opcode::Lt,
    Opcode::Gt,
    Opcode::StoreF,
    Opcode::StoreV,
    Opcode::StoreS,
    Opcode::StoreEnt,
    Opcode::StoreFld,
    Opcode::StoreFnc,
    Opcode::NotF,
    Opcode::NotV,
    Opcode::NotEnt,
    Opcode::NotFnc,
    Opcode::And,
    Opcode::Or,
];

/// A statement the generator will emit, resolved to raw fields later.
#[derive(Clone, Copy, Debug)]
enum Insn {
    /// An opcode over three operand globals.
    Op { op: u16, a: i16, b: i16, c: i16 },
    /// `IF`/`IFNOT` on operand `a`, jumping forward `skip` statements.
    Branch { op: u16, a: i16, skip: i16 },
    /// `GOTO` jumping forward `skip` statements.
    Goto { skip: i16 },
}

/// A deterministic byte reader over the fuzzer's input: each draw consumes bytes, and runs out
/// to zero, so a short input still builds a valid (if small) progs.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    /// The next byte, or 0 at the end.
    fn byte(&mut self) -> u8 {
        let value = self.data.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        value
    }

    /// The next `u16`, little-endian, or 0 at the end.
    fn u16(&mut self) -> u16 {
        u16::from(self.byte()) | (u16::from(self.byte()) << 8)
    }

    /// The next `u32`, little-endian, or 0 at the end.
    fn u32(&mut self) -> u32 {
        u32::from(self.u16()) | (u32::from(self.u16()) << 16)
    }

    /// Whether there are bytes left to draw.
    fn more(&self) -> bool {
        self.at < self.data.len()
    }

    /// A value from 0 to `len - 1`, drawn from the next `u16`; 0 when `len` is 0. Uses
    /// `checked_rem`, so it never divides by zero (the conventions' no-modulo rule).
    fn bounded(&mut self, len: usize) -> usize {
        let span = u32::try_from(len).unwrap_or(u32::MAX);
        usize::try_from(u32::from(self.u16()).checked_rem(span).unwrap_or(0)).unwrap_or(0)
    }
}

/// Builds a well-formed progs and its scenario from `data` (FND-10).
///
/// The bytes choose the scratch globals' initial values and the opcode, operands and jumps of
/// the `StartFrame` body. The result is always a version 6 file both VMs accept and a scenario
/// that runs it; it never returns `None`, since even empty input builds the minimal valid
/// program (a `StartFrame` of one `DONE`).
#[must_use]
pub fn generate(data: &[u8]) -> Generated {
    let mut bytes = Bytes::new(data);

    // The scratch globals' initial values: arbitrary bits, so the two VMs' arithmetic, NaN
    // handling and float-to-int conversions are compared on real values (DIFF-9.4). The bytes
    // that are left after the body are read as values; a short input leaves them 0.
    let mut scratch = [0_u32; SCRATCH_GLOBALS as usize];

    // The body: a run of safe opcodes, ended by DONE. Each operand is a scratch global, so the
    // program never reads or writes outside its own globals (VM-1.4).
    let mut body = Vec::new();
    while bytes.more() && body.len() < MAX_BODY {
        let choice = bytes.byte();
        // One in eight statements is a forward branch or goto, so control flow is exercised
        // without ever jumping backward (no runaway, VM-3.1). `& 7` is `% 8` for a byte,
        // without the modulo the conventions forbid on an untrusted value.
        let remaining = MAX_BODY.saturating_sub(body.len());
        match choice & 7 {
            0 if remaining > 1 => {
                let op = if bytes.byte() & 1 == 0 {
                    Opcode::If.number()
                } else {
                    Opcode::IfNot.number()
                };
                let a = operand(&mut bytes);
                let skip = forward_skip(&mut bytes, body.len());
                body.push(Insn::Branch { op, a, skip });
            }
            1 if remaining > 1 => {
                let skip = forward_skip(&mut bytes, body.len());
                body.push(Insn::Goto { skip });
            }
            _ => {
                let op = pick_opcode(&mut bytes);
                let a = operand(&mut bytes);
                let b = operand(&mut bytes);
                let c = operand(&mut bytes);
                body.push(Insn::Op { op, a, b, c });
            }
        }
    }

    // Any bytes left set the scratch globals' initial values.
    for cell in &mut scratch {
        if !bytes.more() {
            break;
        }
        *cell = bytes.u32();
    }

    let progs = build_progs(&body, &scratch);
    let scenario = scenario();
    Generated { progs, scenario }
}

/// A scratch global slot for an operand, within the program's own window and at least two
/// slots below its top, so a vector opcode's `a+2` stays in range (VM-6.3, no VM-1.4 read).
fn operand(bytes: &mut Bytes<'_>) -> i16 {
    let index = u32::try_from(bytes.bounded(OPERAND_SLOTS as usize)).unwrap_or(0);
    let slot = FIRST_FREE.saturating_add(index);
    i16::try_from(slot).unwrap_or(i16::MAX)
}

/// A body opcode chosen by the next bytes.
fn pick_opcode(bytes: &mut Bytes<'_>) -> u16 {
    let index = bytes.bounded(BODY_OPCODES.len());
    BODY_OPCODES
        .get(index)
        .copied()
        .unwrap_or(Opcode::StoreF)
        .number()
}

/// A forward jump distance in statements from the statement at `at`, at least 1 and never past
/// the `DONE` the body ends with, so a jump always moves toward the end (no backward jump, no
/// runaway).
fn forward_skip(bytes: &mut Bytes<'_>, at: usize) -> i16 {
    // The body is at most MAX_BODY statements; a skip of 1 to the remaining count keeps the
    // target inside the body or on its trailing DONE.
    let remaining = MAX_BODY.saturating_sub(at).max(1);
    let skip = bytes.bounded(remaining).saturating_add(1);
    i16::try_from(skip).unwrap_or(i16::MAX)
}

/// The scenario that runs a generated progs: the default map, a few server frames, the
/// default seed and frametime (DIFF-2). `StartFrame` runs each frame (DIFF-5.3).
fn scenario() -> Scenario {
    Scenario {
        progs: std::path::PathBuf::from("progs.dat"),
        entities: None,
        map: DIFF_DEFAULT_MAP.to_owned(),
        maxclients: 1,
        clients: 0,
        submodels: 0,
        models: Vec::new(),
        cvars: Vec::new(),
        frames: FRAMES,
        frametime: DIFF_DEFAULT_FRAMETIME,
        seed: DIFF_DEFAULT_SEED,
        inputs: Vec::new(),
    }
}

/// Encodes the body and scratch values into a version 6 progs.dat both VMs accept.
///
/// Functions: the null function at index 0, then `noop` (index 1, an empty body of one
/// `DONE`) for every system function global, and `StartFrame` (index 2) with the generated
/// body. The system function globals (`main`, `StartFrame`, `PlayerPreThink`, ...) name
/// `noop` except `StartFrame`, which names its own function, so a server frame runs the body
/// (DIFF-5.3) and the client and spawn entry points are harmless no-ops.
fn build_progs(body: &[Insn], scratch: &[u32; SCRATCH_GLOBALS as usize]) -> Vec<u8> {
    // The string table: the empty string, then every name, each NUL-terminated.
    let mut strings = vec![0_u8];
    let intern = |strings: &mut Vec<u8>, name: &str| -> i32 {
        let offset = i32::try_from(strings.len()).unwrap_or(i32::MAX);
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        offset
    };

    // Global definitions: the system globals, named at their slots (EDICT-2.1).
    let mut global_defs = vec![encode_def(0, 0, 0)];
    for var in SYSTEM_GLOBALS {
        let s_name = intern(&mut strings, var.name);
        let slot = u16::try_from(var.slot).unwrap_or(u16::MAX);
        global_defs.push(encode_def(var.etype.number(), slot, s_name));
    }

    // Field definitions: the system fields, named at their slots (EDICT-3.1), then the two map
    // spawning reads (EDICT-8.2), so a world entity with `classname`/`spawnflags` resolves.
    let mut field_defs = vec![encode_def(0, 0, 0)];
    for var in SYSTEM_FIELDS {
        let s_name = intern(&mut strings, var.name);
        let slot = u16::try_from(var.slot).unwrap_or(u16::MAX);
        field_defs.push(encode_def(var.etype.number(), slot, s_name));
    }

    // Functions. Index 0 is the null function; index 1 is `noop`; index 2 is `StartFrame`.
    let noop_name = intern(&mut strings, "noop");
    let start_name = intern(&mut strings, "StartFrame");
    // Statement 0 is reserved; `noop` is one DONE at statement 1; `StartFrame` is the body
    // from statement 2, ending with its own DONE.
    let noop_first = 1_i32;
    let start_first = 2_i32;
    let functions = [
        encode_function(0, 0),
        encode_function(noop_first, noop_name),
        encode_function(start_first, start_name),
    ];

    // Statements: the reserved zero, `noop`'s DONE, then the body, then StartFrame's DONE.
    let mut statements = encode_statement(0, 0, 0, 0);
    statements.extend(encode_statement(Opcode::Done.number(), 0, 0, 0)); // noop at statement 1
    let body_start = 2_usize;
    let body_end = body_start.saturating_add(body.len());
    for (index, insn) in body.iter().enumerate() {
        let here = body_start.saturating_add(index);
        statements.extend(encode_insn(*insn, here, body_end));
    }
    statements.extend(encode_statement(Opcode::Done.number(), 0, 0, 0)); // StartFrame's DONE
    let num_statements = i32::try_from(body.len().saturating_add(3)).unwrap_or(i32::MAX);

    // Globals: NUM_GLOBALS slots. The system function globals name `noop` except `StartFrame`,
    // which names function 2 (PROGS-12.1); the scratch window holds the generated values.
    let mut global_words = vec![0_u32; NUM_GLOBALS as usize];
    for var in SYSTEM_GLOBALS {
        if var.etype == Etype::Function
            && let Some(cell) = global_words.get_mut(var.slot)
        {
            *cell = if var.name == "StartFrame" {
                2
            } else {
                noop_first.cast_unsigned()
            };
        }
    }
    for (index, value) in scratch.iter().enumerate() {
        let slot = (FIRST_FREE as usize).saturating_add(index);
        if let Some(cell) = global_words.get_mut(slot) {
            *cell = *value;
        }
    }
    let globals: Vec<u8> = global_words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();

    // Pad the string table to a multiple of 4 (PROGS-11).
    while !strings.len().is_multiple_of(4) {
        strings.push(0);
    }

    let mut out = vec![0_u8; HEADER_SIZE];
    let mut place = |table: &[u8]| -> i32 {
        let offset = i32::try_from(out.len()).unwrap_or(i32::MAX);
        out.extend_from_slice(table);
        offset
    };
    let ofs_strings = place(&strings);
    let ofs_statements = place(&statements);
    let ofs_functions = place(&functions.concat());
    let ofs_globaldefs = place(&global_defs.concat());
    let ofs_fielddefs = place(&field_defs.concat());
    let ofs_globals = place(&globals);

    let header: [i32; 15] = [
        PROGS_VERSION,
        PROGS_HEADER_CRC,
        ofs_statements,
        num_statements,
        ofs_globaldefs,
        i32::try_from(global_defs.len()).unwrap_or(i32::MAX),
        ofs_fielddefs,
        i32::try_from(field_defs.len()).unwrap_or(i32::MAX),
        ofs_functions,
        i32::try_from(functions.len()).unwrap_or(i32::MAX),
        ofs_strings,
        i32::try_from(strings.len()).unwrap_or(i32::MAX),
        ofs_globals,
        i32::try_from(NUM_GLOBALS).unwrap_or(i32::MAX),
        ENTITY_FIELDS,
    ];
    for (index, value) in header.into_iter().enumerate() {
        let at = index.saturating_mul(4);
        if let Some(slot) = out.get_mut(at..at.saturating_add(4)) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }
    out
}

/// Encodes one body instruction to version 6 statement bytes. A branch or goto is turned into
/// absolute-relative operands the VM reads as a jump; a forward skip is kept inside the body.
fn encode_insn(insn: Insn, here: usize, body_end: usize) -> Vec<u8> {
    match insn {
        Insn::Op { op, a, b, c } => encode_statement(op, a, b, c),
        Insn::Branch { op, a, skip } => {
            let target = jump_target(here, skip, body_end);
            encode_statement(op, a, target, 0)
        }
        Insn::Goto { skip } => {
            let target = jump_target(here, skip, body_end);
            encode_statement(Opcode::Goto.number(), target, 0, 0)
        }
    }
}

/// The signed relative jump a forward `skip` becomes: `IF`, `IFNOT` and `GOTO` add the operand
/// to the current statement index (VM-13.1, VM-13.2), so the operand is `+skip` clamped so the
/// target never passes the body's trailing `DONE` (it lands on a body statement or the `DONE`).
fn jump_target(here: usize, skip: i16, body_end: usize) -> i16 {
    let want = here.saturating_add(usize::try_from(skip).unwrap_or(0));
    let target = want.min(body_end);
    let offset = target.saturating_sub(here);
    i16::try_from(offset).unwrap_or(1).max(1)
}

/// Encodes one version 6 statement, little-endian (PROGS-7).
fn encode_statement(op: u16, a: i16, b: i16, c: i16) -> Vec<u8> {
    [
        op.to_le_bytes(),
        a.to_le_bytes(),
        b.to_le_bytes(),
        c.to_le_bytes(),
    ]
    .concat()
}

/// Encodes one version 6 definition, little-endian (PROGS-9).
fn encode_def(type_bits: u16, ofs: u16, s_name: i32) -> Vec<u8> {
    [
        &type_bits.to_le_bytes()[..],
        &ofs.to_le_bytes(),
        &s_name.to_le_bytes(),
    ]
    .concat()
}

/// Encodes one function, little-endian (PROGS-10): `first_statement`, zero locals, the name,
/// and no parameters.
fn encode_function(first: i32, s_name: i32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(36);
    // first_statement, parm_start, locals, profile, s_name, s_file, numparms.
    for field in [first, 0, 0, 0, s_name, 0, 0] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend([0_u8; 8]); // parm_size
    bytes
}

#[cfg(test)]
#[path = "generate/tests.rs"]
mod tests;
