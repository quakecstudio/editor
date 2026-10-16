// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: the spec's test vectors, and a factory that lays out hand-built files
//! without going through the writer.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

/// One of the whole-file vectors in `docs/spec/vectors/progs/`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Vector {
    Minimal,
    Small,
    #[cfg(feature = "fte")]
    Fte16Minimal,
    #[cfg(feature = "fte")]
    Fte32Minimal,
}

/// Every vector the build can read.
pub(crate) const VECTORS: &[Vector] = &[
    Vector::Minimal,
    Vector::Small,
    #[cfg(feature = "fte")]
    Vector::Fte16Minimal,
    #[cfg(feature = "fte")]
    Vector::Fte32Minimal,
];

/// The bytes of a vector, rebuilt from its `xxd` dump as `xxd -r` would.
pub(crate) fn vector(vector: Vector) -> Vec<u8> {
    let dump = match vector {
        Vector::Minimal => include_str!("../../../docs/spec/vectors/progs/minimal.txt"),
        Vector::Small => include_str!("../../../docs/spec/vectors/progs/small.txt"),
        #[cfg(feature = "fte")]
        Vector::Fte16Minimal => {
            include_str!("../../../docs/spec/vectors/progs/fte16-minimal.txt")
        }
        #[cfg(feature = "fte")]
        Vector::Fte32Minimal => {
            include_str!("../../../docs/spec/vectors/progs/fte32-minimal.txt")
        }
    };
    let mut bytes = Vec::new();
    for line in dump.lines() {
        let (_, rest) = line.split_once(": ").expect("an xxd line has an offset");
        let hex: String = rest.get(..39).unwrap_or(rest).split_whitespace().collect();
        for pair in hex.as_bytes().chunks(2) {
            let pair = core::str::from_utf8(pair).unwrap();
            bytes.push(u8::from_str_radix(pair, 16).unwrap());
        }
    }
    bytes
}

/// Index of each header field, in 4-byte words from the start of the file (PROGS-2.1).
pub(crate) mod field {
    pub(crate) const VERSION: usize = 0;
    pub(crate) const CRC: usize = 1;
    pub(crate) const OFS_STATEMENTS: usize = 2;
    pub(crate) const NUMSTATEMENTS: usize = 3;
    pub(crate) const OFS_GLOBALDEFS: usize = 4;
    pub(crate) const NUMGLOBALDEFS: usize = 5;
    pub(crate) const OFS_FIELDDEFS: usize = 6;
    pub(crate) const NUMFIELDDEFS: usize = 7;
    pub(crate) const OFS_FUNCTIONS: usize = 8;
    pub(crate) const NUMFUNCTIONS: usize = 9;
    pub(crate) const OFS_STRINGS: usize = 10;
    pub(crate) const NUMSTRINGS: usize = 11;
    pub(crate) const OFS_GLOBALS: usize = 12;
    pub(crate) const NUMGLOBALS: usize = 13;
    pub(crate) const ENTITYFIELDS: usize = 14;
}

/// Sets the 4-byte header word `index` of `bytes` to `value`.
pub(crate) fn patch(bytes: &mut [u8], index: usize, value: i32) {
    let at = index * 4;
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// The 4-byte word `index` of `bytes`.
pub(crate) fn word(bytes: &[u8], index: usize) -> i32 {
    let at = index * 4;
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// A version 6 statement's bytes.
pub(crate) fn statement(op: u16, a: i16, b: i16, c: i16) -> Vec<u8> {
    [
        op.to_le_bytes(),
        a.to_le_bytes(),
        b.to_le_bytes(),
        c.to_le_bytes(),
    ]
    .concat()
}

/// A version 6 definition's bytes.
pub(crate) fn def(type_bits: u16, ofs: u16, s_name: i32) -> Vec<u8> {
    [
        &type_bits.to_le_bytes()[..],
        &ofs.to_le_bytes(),
        &s_name.to_le_bytes(),
    ]
    .concat()
}

/// A function's fields, for [`function`].
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FunctionSpec {
    pub(crate) first_statement: i32,
    pub(crate) parm_start: i32,
    pub(crate) locals: i32,
    pub(crate) profile: i32,
    pub(crate) s_name: i32,
    pub(crate) s_file: i32,
    pub(crate) numparms: i32,
    pub(crate) parm_size: [u8; 8],
}

/// A function's bytes.
pub(crate) fn function(spec: &FunctionSpec) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [
        spec.first_statement,
        spec.parm_start,
        spec.locals,
        spec.profile,
        spec.s_name,
        spec.s_file,
        spec.numparms,
    ] {
        bytes.extend(field.to_le_bytes());
    }
    bytes.extend(spec.parm_size);
    bytes
}

/// A hand-built version 6 file, laid out by [`FixtureSpec::build`] without the writer.
///
/// The default is the spec's `minimal` vector: each table holds its reserved zero record, the
/// string table is one NUL padded to 4 bytes and the globals are the 28 reserved slots.
#[derive(Clone, Debug)]
pub(crate) struct FixtureSpec {
    pub(crate) version: i32,
    pub(crate) crc: i32,
    pub(crate) entityfields: i32,
    /// Encoded statements, one entry per record.
    pub(crate) statements: Vec<Vec<u8>>,
    pub(crate) global_defs: Vec<Vec<u8>>,
    pub(crate) field_defs: Vec<Vec<u8>>,
    pub(crate) functions: Vec<Vec<u8>>,
    pub(crate) strings: Vec<u8>,
    pub(crate) globals: Vec<u32>,
    /// Bytes inserted after each table, in qcc's order (PROGS-1.4); empty for none.
    pub(crate) gap: Vec<u8>,
}

impl Default for FixtureSpec {
    fn default() -> Self {
        Self {
            version: 6,
            crc: 5927,
            entityfields: 0,
            statements: vec![statement(0, 0, 0, 0)],
            global_defs: vec![def(0, 0, 0)],
            field_defs: vec![def(0, 0, 0)],
            functions: vec![function(&FunctionSpec::default())],
            strings: vec![0; 4],
            globals: vec![0; 28],
            gap: Vec::new(),
        }
    }
}

impl FixtureSpec {
    /// The file's bytes: the header, then the tables in qcc's order (PROGS-1.3), each
    /// followed by [`FixtureSpec::gap`].
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut bytes = vec![0_u8; 60];
        let mut place = |table: &[u8]| -> i32 {
            let offset = i32::try_from(bytes.len()).unwrap();
            bytes.extend_from_slice(table);
            bytes.extend_from_slice(&self.gap);
            offset
        };
        let count = |len: usize| i32::try_from(len).unwrap();
        let ofs_strings = place(&self.strings);
        let ofs_statements = place(&self.statements.concat());
        let ofs_functions = place(&self.functions.concat());
        let ofs_globaldefs = place(&self.global_defs.concat());
        let ofs_fielddefs = place(&self.field_defs.concat());
        let globals: Vec<u8> = self.globals.iter().flat_map(|g| g.to_le_bytes()).collect();
        let ofs_globals = place(&globals);
        let header = [
            self.version,
            self.crc,
            ofs_statements,
            count(self.statements.len()),
            ofs_globaldefs,
            count(self.global_defs.len()),
            ofs_fielddefs,
            count(self.field_defs.len()),
            ofs_functions,
            count(self.functions.len()),
            ofs_strings,
            count(self.strings.len()),
            ofs_globals,
            count(self.globals.len()),
            self.entityfields,
        ];
        for (index, value) in header.into_iter().enumerate() {
            patch(&mut bytes, index, value);
        }
        bytes
    }
}
