// SPDX-License-Identifier: GPL-3.0-or-later

//! A hand-built progs.dat factory for the host-loop and EDICT-8 tests.
//!
//! The scenario reader has its own vectors; these tests need a *runnable* progs with named
//! fields and functions, so the EDICT-8 lookups (`ED_FindField`, `ED_FindFunction`) and the
//! engine entry points (`StartFrame`, a spawn function) resolve. The builder lays out a
//! version 6 file like `qc-progs`'s fixtures, with 128 globals (the 92 system globals and
//! room above), 110 entity fields (the 105 system fields and room above), the system globals
//! and fields named in the definition tables, and the caller's extra fields, functions and
//! statements.

#![expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "test code: a failed index, overflow, panic or cast is a failed test"
)]

use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::opcode::Opcode;
use qc_types::Etype;

/// One statement, by opcode name (resolved to its number) or a raw number.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stmt {
    pub(crate) op: u16,
    pub(crate) a: i16,
    pub(crate) b: i16,
    pub(crate) c: i16,
}

impl Stmt {
    pub(crate) fn op(name: &str, a: i16, b: i16, c: i16) -> Self {
        let op = Opcode::ALL
            .into_iter()
            .find(|o| o.name() == name)
            .unwrap_or_else(|| panic!("unknown opcode {name}"))
            .number();
        Self { op, a, b, c }
    }
}

/// A function to add to the table, with its name.
#[derive(Clone, Debug, Default)]
pub(crate) struct FuncSpec {
    pub(crate) name: &'static str,
    pub(crate) first_statement: i32,
    pub(crate) parm_start: i32,
    pub(crate) locals: i32,
    pub(crate) numparms: i32,
    pub(crate) parm_size: [u8; 8],
}

/// An extra field definition a test declares beyond the system fields (slot and type).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExtraField {
    pub(crate) name: &'static str,
    pub(crate) slot: u16,
    pub(crate) etype: Etype,
}

/// Builds a runnable version 6 progs.dat for the tests.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProgsBuilder {
    pub(crate) statements: Vec<Stmt>,
    pub(crate) functions: Vec<FuncSpec>,
    pub(crate) extra_fields: Vec<ExtraField>,
    /// Initial values of global slots, (slot, 32-bit pattern), applied after the function
    /// globals: a `CALL` operand holding a function index, say.
    pub(crate) globals: Vec<(usize, u32)>,
}

impl ProgsBuilder {
    pub(crate) fn statement(mut self, stmt: Stmt) -> Self {
        self.statements.push(stmt);
        self
    }

    pub(crate) fn function(mut self, func: FuncSpec) -> Self {
        self.functions.push(func);
        self
    }

    pub(crate) fn extra_field(mut self, field: ExtraField) -> Self {
        self.extra_fields.push(field);
        self
    }

    pub(crate) fn global(mut self, slot: usize, value: u32) -> Self {
        self.globals.push((slot, value));
        self
    }

    /// The progs.dat bytes.
    pub(crate) fn build(&self) -> Vec<u8> {
        let numglobals = 128_usize;
        let entityfields = 110_i32;

        // The string table: the empty string, then every name we need, each NUL-terminated.
        let mut strings = vec![0_u8];
        let intern = |strings: &mut Vec<u8>, name: &str| -> i32 {
            let offset = strings.len() as i32;
            strings.extend_from_slice(name.as_bytes());
            strings.push(0);
            offset
        };

        // Global definitions: the system globals, named, at their slots (EDICT-2.1).
        let mut global_defs = vec![encode_def(0, 0, 0)];
        for var in SYSTEM_GLOBALS {
            let s_name = intern(&mut strings, var.name);
            global_defs.push(encode_def(var.etype.number(), var.slot as u16, s_name));
        }

        // Field definitions: the system fields, named, at their slots (EDICT-3.1), then the
        // test's extra fields.
        let mut field_defs = vec![encode_def(0, 0, 0)];
        for var in SYSTEM_FIELDS {
            let s_name = intern(&mut strings, var.name);
            field_defs.push(encode_def(var.etype.number(), var.slot as u16, s_name));
        }
        for extra in &self.extra_fields {
            let s_name = intern(&mut strings, extra.name);
            field_defs.push(encode_def(extra.etype.number(), extra.slot, s_name));
        }

        // Functions: the null function at index 0, then the test's, each named.
        let mut functions = vec![encode_function(0, 0, 0, 0, 0)];
        let mut function_index: Vec<(&str, i32)> = Vec::new();
        for (i, func) in self.functions.iter().enumerate() {
            let s_name = intern(&mut strings, func.name);
            functions.push(encode_function_spec(func, s_name));
            // Function i of the spec is at table index i + 1 (index 0 is the null function).
            function_index.push((func.name, (i + 1) as i32));
        }

        // Statements: the reserved zero at index 0, then the test's.
        let mut statements = encode_statement(0, 0, 0, 0);
        for st in &self.statements {
            statements.extend(encode_statement(st.op, st.a, st.b, st.c));
        }

        // Globals: numglobals slots, zero, with each function-typed system global (StartFrame,
        // PlayerPreThink, ...) initialized to the index of the function of the same name, as
        // qcc initialises a function global (PROGS-12.1).
        let mut global_words = vec![0_u32; numglobals];
        for var in SYSTEM_GLOBALS {
            if var.etype == Etype::Function
                && let Some((_, index)) = function_index.iter().find(|(n, _)| *n == var.name)
                && let Some(cell) = global_words.get_mut(var.slot)
            {
                *cell = (*index).cast_unsigned();
            }
        }
        for &(slot, value) in &self.globals {
            global_words[slot] = value;
        }
        let globals: Vec<u8> = global_words.iter().flat_map(|w| w.to_le_bytes()).collect();

        // Pad the string table to a multiple of 4 (PROGS-11).
        while !strings.len().is_multiple_of(4) {
            strings.push(0);
        }

        let mut bytes = vec![0_u8; 60];
        let mut place = |table: &[u8]| -> i32 {
            let offset = bytes.len() as i32;
            bytes.extend_from_slice(table);
            offset
        };
        let ofs_strings = place(&strings);
        let ofs_statements = place(&statements);
        let ofs_functions = place(&functions.concat());
        let ofs_globaldefs = place(&global_defs.concat());
        let ofs_fielddefs = place(&field_defs.concat());
        let ofs_globals = place(&globals);

        let num_statements = 1 + self.statements.len();
        let header: [i32; 15] = [
            6,
            5927,
            ofs_statements,
            num_statements as i32,
            ofs_globaldefs,
            global_defs.len() as i32,
            ofs_fielddefs,
            field_defs.len() as i32,
            ofs_functions,
            functions.len() as i32,
            ofs_strings,
            strings.len() as i32,
            ofs_globals,
            numglobals as i32,
            entityfields,
        ];
        for (index, value) in header.into_iter().enumerate() {
            let at = index * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

fn encode_statement(op: u16, a: i16, b: i16, c: i16) -> Vec<u8> {
    [
        op.to_le_bytes(),
        a.to_le_bytes(),
        b.to_le_bytes(),
        c.to_le_bytes(),
    ]
    .concat()
}

fn encode_def(type_bits: u16, ofs: u16, s_name: i32) -> Vec<u8> {
    [
        &type_bits.to_le_bytes()[..],
        &ofs.to_le_bytes(),
        &s_name.to_le_bytes(),
    ]
    .concat()
}

fn encode_function(
    first: i32,
    parm_start: i32,
    locals: i32,
    s_name: i32,
    numparms: i32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [first, parm_start, locals, 0, s_name, 0, numparms] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend([0_u8; 8]);
    bytes
}

fn encode_function_spec(func: &FuncSpec, s_name: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [
        func.first_statement,
        func.parm_start,
        func.locals,
        0,
        s_name,
        0,
        func.numparms,
    ] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend(func.parm_size);
    bytes
}
