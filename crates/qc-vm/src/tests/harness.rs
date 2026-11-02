// SPDX-License-Identifier: GPL-3.0-or-later

//! A hand-built progs.dat factory for the VM tests and the vector runner, and the value
//! parsing of the vectors' README.
//!
//! The vectors describe raw statements, functions, globals, strings and fields; this builds a
//! version 6 progs.dat holding them, laid out like `qc-progs`'s own fixtures, so the VM loads
//! it with [`Progs::read`](qc_progs::Progs::read) and runs it.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use qc_progs::opcode::Opcode;
use qc_types::Slot;

use crate::RunError;
use crate::exec::{Builtins, Context};

/// The stub builtins the vectors' README describes, shared by the vectors and the nested-run
/// tests: builtin 2 runs a nested `PR_ExecuteProgram` of the function whose index is in
/// `OFS_PARM0`; every other number writes the argument count as a float to slot 1 and copies
/// slot 4 to slot 2.
pub(crate) struct StubBuiltins;

impl Builtins for StubBuiltins {
    fn limit(&self) -> Option<u32> {
        Some(crate::LIMIT_NUM_BUILTINS)
    }

    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        if number == 2 {
            let function = context.machine().globals()[4].as_i32();
            context.execute_program(self, function)?;
            return Ok(());
        }
        let argc = context.machine().argc();
        let parm0 = context.machine().globals()[4];
        let argc_float = match u16::try_from(argc) {
            Ok(n) => f32::from(n),
            Err(_) => 0.0,
        };
        context
            .machine()
            .set_global_test(1, Slot::from_f32(argc_float));
        context.machine().set_global_test(2, parm0);
        Ok(())
    }

    fn engine_string(&self, _value: i32) -> Option<&[u8]> {
        None
    }
}

/// One statement, written as an opcode name (resolved to its number) or a raw number.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stmt {
    pub(crate) op: u16,
    pub(crate) a: i16,
    pub(crate) b: i16,
    pub(crate) c: i16,
}

impl Stmt {
    /// A statement with an opcode name, e.g. `op("ADD_F", 100, 101, 102)`. A name that is no
    /// vanilla opcode fails the test at the caller's line, naming it.
    #[track_caller]
    pub(crate) fn op(name: &str, a: i16, b: i16, c: i16) -> Self {
        let Some(op) = opcode_number(name) else {
            panic!("`{name}` is not a vanilla opcode name");
        };
        Self { op, a, b, c }
    }

    /// A statement with a raw opcode number, for an invalid opcode.
    pub(crate) fn raw(op: u16, a: i16, b: i16, c: i16) -> Self {
        Self { op, a, b, c }
    }
}

/// One function record override.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Func {
    pub(crate) first_statement: i32,
    pub(crate) parm_start: i32,
    pub(crate) locals: i32,
    pub(crate) numparms: i32,
    pub(crate) parm_size: [u8; 8],
    /// The string-table offset of the function's name (`s_name`), for stack-trace tests.
    pub(crate) s_name: i32,
    /// The string-table offset of the function's source file (`s_file`).
    pub(crate) s_file: i32,
}

/// The opcode number for a vanilla opcode name, from `qc-progs`; `None` for any other name.
pub(crate) fn opcode_number(name: &str) -> Option<u16> {
    Opcode::ALL
        .into_iter()
        .find(|op| op.name() == name)
        .map(Opcode::number)
}

/// Builds a version 6 progs.dat from raw tables, laid out as qc-progs's fixtures do.
#[derive(Clone, Debug)]
pub(crate) struct ProgsBuilder {
    pub(crate) entityfields: i32,
    pub(crate) numglobals: usize,
    pub(crate) statements: Vec<Stmt>,
    pub(crate) functions: Vec<Func>,
    pub(crate) strings: Vec<u8>,
    pub(crate) globals_init: Vec<(usize, u32)>,
}

impl Default for ProgsBuilder {
    fn default() -> Self {
        // The vectors' fixed environment: 128 globals (slots 0..127), 110 entity fields, a
        // string table holding only the empty string, function 1 starting at statement 1.
        Self {
            entityfields: 110,
            numglobals: 128,
            statements: Vec::new(),
            functions: vec![Func {
                first_statement: 1,
                ..Func::default()
            }],
            strings: vec![0],
            globals_init: Vec::new(),
        }
    }
}

impl ProgsBuilder {
    pub(crate) fn statements(mut self, statements: &[Stmt]) -> Self {
        self.statements = statements.to_vec();
        self
    }

    pub(crate) fn functions(mut self, functions: Vec<Func>) -> Self {
        self.functions = functions;
        self
    }

    pub(crate) fn numglobals(mut self, numglobals: usize) -> Self {
        self.numglobals = numglobals;
        self
    }

    pub(crate) fn strings(mut self, strings: Vec<u8>) -> Self {
        self.strings = strings;
        self
    }

    pub(crate) fn globals_init(mut self, globals: &[(usize, u32)]) -> Self {
        self.globals_init = globals.to_vec();
        self
    }

    /// The progs.dat bytes: a version 6 header and the tables in qcc's order.
    pub(crate) fn build(&self) -> Vec<u8> {
        // Statement 0 is reserved and all zero (PROGS-1.5); user statements start at 1.
        let mut statement_bytes = encode_statement(0, 0, 0, 0);
        for st in &self.statements {
            statement_bytes.extend(encode_statement(st.op, st.a, st.b, st.c));
        }

        // Function 0 is the null function; functions start at index 0.
        let mut function_bytes = Vec::new();
        // Index 0: the null function, all zero.
        function_bytes.extend(encode_function(&Func::default()));
        for func in &self.functions {
            function_bytes.extend(encode_function(func));
        }

        // Globals: numglobals slots, with the init overrides applied.
        let mut globals = vec![0_u32; self.numglobals];
        for (slot, value) in &self.globals_init {
            globals[*slot] = *value;
        }
        let global_bytes: Vec<u8> = globals.iter().flat_map(|g| g.to_le_bytes()).collect();

        // A string table padded to a multiple of 4 (PROGS-11), terminating NUL included.
        let mut strings = self.strings.clone();
        while !strings.len().is_multiple_of(4) {
            strings.push(0);
        }

        let mut bytes = vec![0_u8; 60];
        let mut place = |table: &[u8]| -> i32 {
            let offset = i32::try_from(bytes.len()).unwrap();
            bytes.extend_from_slice(table);
            offset
        };
        let ofs_strings = place(&strings);
        let ofs_statements = place(&statement_bytes);
        let ofs_functions = place(&function_bytes);
        // One global and one field definition, both the reserved zero record (PROGS-1.5), so
        // the loader's string checks have nothing to reject.
        let ofs_globaldefs = place(&encode_def(0, 0, 0));
        let ofs_fielddefs = place(&encode_def(0, 0, 0));
        let ofs_globals = place(&global_bytes);

        let num_statements = 1 + self.statements.len();
        let num_functions = 1 + self.functions.len();
        let header: [i32; 15] = [
            6,
            5927,
            ofs_statements,
            i32::try_from(num_statements).unwrap(),
            ofs_globaldefs,
            1,
            ofs_fielddefs,
            1,
            ofs_functions,
            i32::try_from(num_functions).unwrap(),
            ofs_strings,
            i32::try_from(strings.len()).unwrap(),
            ofs_globals,
            i32::try_from(self.numglobals).unwrap(),
            self.entityfields,
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

fn encode_function(func: &Func) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [
        func.first_statement,
        func.parm_start,
        func.locals,
        0, // profile
        func.s_name,
        func.s_file,
        func.numparms,
    ] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend(func.parm_size);
    bytes
}
