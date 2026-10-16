// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap, index, overflow or panic is a failed test"
)]

#[cfg(feature = "fte")]
mod fte;

use proptest::prelude::*;
use qc_types::{DefType, Etype, Slot};

use crate::fixtures::{
    FixtureSpec, FunctionSpec, Vector, def, field, function, patch, statement, vector, word,
};
use crate::limits::{LIMIT_MAX_DEF_SLOT, LIMIT_MAX_DEF_TYPE, LIMIT_MAX_OPERAND_SLOT};
use crate::{
    Def, Format, Function, Header, PROGS_DEF_SIZE, PROGS_FUNCTION_SIZE, PROGS_GLOBAL_SIZE,
    PROGS_HEADER_CRC, PROGS_HEADER_SIZE, PROGS_STATEMENT_SIZE, PROGS_STRING_ALIGN, PROGS_VERSION,
    Progs, ReadError, ReadOptions, Statement, StringSite, TableId,
};

fn read(bytes: &[u8]) -> Result<Progs<'_>, ReadError> {
    Progs::read(bytes)
}

fn minimal() -> Vec<u8> {
    vector(Vector::Minimal)
}

fn small() -> Vec<u8> {
    vector(Vector::Small)
}

/// A file whose string table is `strings`, with one global definition per name offset.
fn with_names(strings: &[u8], names: &[i32]) -> Vec<u8> {
    FixtureSpec {
        strings: strings.to_vec(),
        global_defs: names.iter().map(|&name| def(2, 28, name)).collect(),
        ..FixtureSpec::default()
    }
    .build()
}

// spec PROGS-2.1: the minimal vector's header decodes to the 15 fields its README lists.
// spec PROGS-1.3: qcc's order: header, strings, statements, functions, globaldefs, fielddefs,
// globals, each directly after the previous one.
#[test]
fn minimal_header_decodes() {
    let bytes = minimal();
    let progs = read(&bytes).unwrap();
    assert_eq!(
        *progs.header(),
        Header {
            version: 6,
            crc: 5927,
            ofs_statements: 64,
            numstatements: 1,
            ofs_globaldefs: 108,
            numglobaldefs: 1,
            ofs_fielddefs: 116,
            numfielddefs: 1,
            ofs_functions: 72,
            numfunctions: 1,
            ofs_strings: 60,
            numstrings: 4,
            ofs_globals: 124,
            numglobals: 28,
            entityfields: 0,
        }
    );
    assert_eq!(progs.format(), Format::V6);
    assert_eq!(bytes.len(), 236);
}

// spec PROGS-2.1: the header is 60 bytes; records are 8, 8, 36 and 4 bytes.
#[test]
fn record_sizes() {
    assert_eq!(PROGS_HEADER_SIZE, 60);
    assert_eq!(PROGS_STATEMENT_SIZE, 8);
    assert_eq!(PROGS_DEF_SIZE, 8);
    assert_eq!(PROGS_FUNCTION_SIZE, 36);
    assert_eq!(PROGS_GLOBAL_SIZE, 4);
    assert_eq!(PROGS_STRING_ALIGN, 4);
    assert_eq!(PROGS_VERSION, 6);
    assert_eq!(PROGS_HEADER_CRC, 5927);
}

// spec PROGS-1.5: the first record of each table is reserved and qcc writes it as zeros.
// spec PROGS-11.2: offset 0 of the string table is the empty string.
// spec PROGS-11.5: the string table ends with qcc's NUL padding.
// spec PROGS-12.3: a file from qcc has at least the 28 reserved global slots.
#[test]
fn minimal_tables_hold_the_reserved_records() {
    let bytes = minimal();
    let progs = read(&bytes).unwrap();
    assert_eq!(
        progs.statements().iter().collect::<Vec<_>>(),
        [Statement::default()]
    );
    assert_eq!(
        progs.functions().iter().collect::<Vec<_>>(),
        [Function::default()]
    );
    assert_eq!(
        progs.global_defs().iter().collect::<Vec<_>>(),
        [Def::default()]
    );
    assert_eq!(
        progs.field_defs().iter().collect::<Vec<_>>(),
        [Def::default()]
    );
    assert_eq!(progs.strings(), [0, 0, 0, 0]);
    assert_eq!(progs.string(0), Some(&b""[..]));
    assert_eq!(progs.globals().len(), 28);
    assert!(progs.globals().iter().all(|slot| slot == Slot(0)));
}

// spec PROGS-1.5: the engine does not check the reserved records; non-zero ones load as
// stored.
#[test]
fn reserved_records_are_not_checked() {
    let bytes = FixtureSpec {
        statements: vec![statement(61, -1, 2, 3)],
        global_defs: vec![def(2, 7, 1)],
        field_defs: vec![def(3, 9, 1)],
        functions: vec![function(&FunctionSpec {
            first_statement: 5,
            s_name: 1,
            s_file: 1,
            ..FunctionSpec::default()
        })],
        strings: b"xy\0\0".to_vec(),
        ..FixtureSpec::default()
    }
    .build();
    let progs = read(&bytes).unwrap();
    assert_eq!(
        progs.statements().get(0),
        Some(Statement {
            op: 61,
            a: -1,
            b: 2,
            c: 3
        })
    );
    assert_eq!(progs.global_defs().get(0).unwrap().ofs, 7);
    assert_eq!(progs.field_defs().get(0).unwrap().ofs, 9);
    assert_eq!(progs.functions().get(0).unwrap().first_statement, 5);
    assert_eq!(progs.string(0), Some(&b"xy"[..]));
}

// spec PROGS-1.1: integers are little-endian: bytes 27 17 00 00 are 5927, and the bytes
// e7 ff ff ff of a first_statement are -25.
// spec PROGS-10.1: the small vector's functions decode field by field.
// spec PROGS-10.2: dprint is builtin 25, stored as -25.
#[test]
fn small_functions_decode() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    assert_eq!(&bytes[4..8], [0x27, 0x17, 0, 0]);
    assert_eq!(progs.header().crc, 5927);
    let functions: Vec<Function> = progs.functions().iter().collect();
    assert_eq!(
        functions,
        [
            Function::default(),
            Function {
                first_statement: -25,
                s_name: 9,
                s_file: 1,
                numparms: 1,
                parm_size: [1, 0, 0, 0, 0, 0, 0, 0],
                ..Function::default()
            },
            Function {
                first_statement: 1,
                parm_start: 41,
                locals: 3,
                s_name: 16,
                s_file: 1,
                numparms: 1,
                parm_size: [1, 0, 0, 0, 0, 0, 0, 0],
                ..Function::default()
            },
        ]
    );
    assert_eq!(functions[1].builtin_number(), Some(25));
    assert_eq!(functions[2].builtin_number(), None);
    assert_eq!(progs.string(functions[1].s_name), Some(&b"dprint"[..]));
}

// spec PROGS-10.2: a negative first_statement is a builtin number; 0 and above are QC code,
// and -2147483648 has no positive number.
#[test]
fn builtin_numbers() {
    let function = |first_statement| Function {
        first_statement,
        ..Function::default()
    };
    assert_eq!(function(-1).builtin_number(), Some(1));
    assert_eq!(function(-78).builtin_number(), Some(78));
    assert_eq!(function(0).builtin_number(), None);
    assert_eq!(function(i32::MIN).builtin_number(), None);
}

// spec PROGS-2.2: a table occupies count * record size bytes from its offset.
// spec PROGS-7.1: the small vector's statements decode as its README lists.
#[test]
fn small_statements_decode() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    let statements = progs.statements();
    assert_eq!(statements.len(), 7);
    assert_eq!(statements.as_bytes().len(), 7 * PROGS_STATEMENT_SIZE);
    assert_eq!(progs.global_defs().as_bytes().len(), 17 * PROGS_DEF_SIZE);
    assert_eq!(progs.functions().as_bytes().len(), 3 * PROGS_FUNCTION_SIZE);
    assert_eq!(progs.globals().as_bytes().len(), 44 * PROGS_GLOBAL_SIZE);
    assert_eq!(progs.strings().len(), 160);
    let expected = [
        (0, 0, 0, 0),
        (31, 41, 42, 0),
        (6, 33, 42, 43),
        (31, 43, 33, 0),
        (33, 37, 4, 0),
        (52, 39, 0, 0),
        (0, 0, 0, 0),
    ];
    for (statement, (op, a, b, c)) in statements.iter().zip(expected) {
        assert_eq!(statement, Statement { op, a, b, c });
    }
    assert_eq!(statements.get(7), None);
    assert_eq!(statements.iter().len(), 7);
}

// spec PROGS-7.1: bytes 1f 00 29 00 2a 00 00 00 are STORE_F 41 42 0.
// spec PROGS-7.2: operands are signed: 3d 00 ff ff 00 00 00 00 is GOTO -1.
// spec LIMIT-1.1: operand bytes ff 7f are slot 32767 and 00 80 are -32768.
#[test]
fn statement_vectors() {
    let decode = |bytes: [u8; 8]| {
        let file = FixtureSpec {
            statements: vec![bytes.to_vec()],
            ..FixtureSpec::default()
        }
        .build();
        read(&file).unwrap().statements().get(0).unwrap()
    };
    assert_eq!(
        decode([0x1f, 0, 0x29, 0, 0x2a, 0, 0, 0]),
        Statement {
            op: 31,
            a: 41,
            b: 42,
            c: 0
        }
    );
    assert_eq!(decode([0x3d, 0, 0xff, 0xff, 0, 0, 0, 0]).a, -1);
    let limits = decode([0, 0, 0xff, 0x7f, 0x00, 0x80, 0, 0]);
    assert_eq!(limits.a, i32::from(LIMIT_MAX_OPERAND_SLOT));
    assert_eq!(limits.a, 32767);
    assert_eq!(limits.b, -32768);
}

// spec PROGS-9.1: 02 80 21 00 42 00 00 00 is a saved float at 33 named 66, and
// 03 00 22 00 4a 00 00 00 a vector at 34 named 74.
// spec PROGS-9.2: ofs is unsigned: 00 00 ff ff 00 00 00 00 names slot 65535.
// spec LIMIT-1.2: a definition can name slots up to 65535.
#[test]
fn definition_vectors() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    let counter = progs.global_defs().get(6).unwrap();
    assert_eq!(
        counter,
        Def {
            type_bits: 0x8002,
            ofs: 33,
            s_name: 66
        }
    );
    assert!(counter.is_save_global());
    assert_eq!(counter.def_type(), DefType::Known(Etype::Float));
    let v = progs.global_defs().get(7).unwrap();
    assert_eq!((v.type_bits, v.ofs, v.s_name), (3, 34, 74));
    assert!(!v.is_save_global());
    assert_eq!(v.def_type(), DefType::Known(Etype::Vector));

    let file = FixtureSpec {
        global_defs: vec![vec![0, 0, 0xff, 0xff, 0, 0, 0, 0]],
        ..FixtureSpec::default()
    }
    .build();
    let top = read(&file).unwrap().global_defs().get(0).unwrap();
    assert_eq!(top.ofs, 65535);
    assert_eq!(top.ofs, u32::from(LIMIT_MAX_DEF_SLOT));
}

// spec LIMIT-1.3: once DEF_SAVEGLOBAL is removed a type has 15 bits, 0 to 32767.
// spec PROGS-9.3: DEF_SAVEGLOBAL is bit 15 and is stripped before the type is used.
#[test]
fn type_has_fifteen_bits() {
    let all = Def {
        type_bits: 0xFFFF,
        ..Def::default()
    };
    assert!(all.is_save_global());
    assert_eq!(all.def_type(), DefType::Unknown(LIMIT_MAX_DEF_TYPE));
    assert_eq!(LIMIT_MAX_DEF_TYPE, 32767);
    let unsaved = Def {
        type_bits: 0x7FFF,
        ..Def::default()
    };
    assert!(!unsaved.is_save_global());
    assert_eq!(unsaved.def_type().number(), 32767);
    assert_eq!(crate::PROGS_DEF_SAVEGLOBAL, 0x8000);
}

// spec PROGS-9.3: DEF_SAVEGLOBAL in a global definition is accepted.
// spec PROGS-9.4: v and v_x name the same slot; lookup by slot takes the first, v.
// spec PROGS-9.5: a field gets a field definition and a global of type field whose value is
// the field's slot index.
// spec PROGS-13.2: field slots count one per float field and three per vector field.
#[test]
fn small_definitions_follow_qcc() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    let at_34 = progs.ed_global_at_ofs(34).unwrap();
    assert_eq!(progs.string(at_34.s_name), Some(&b"v"[..]));
    let v_x = progs.ed_find_global(b"v_x").unwrap();
    assert_eq!(v_x.ofs, 34);

    let origin_global = progs.ed_find_global(b"origin").unwrap();
    assert_eq!(origin_global.def_type(), DefType::Known(Etype::Field));
    let origin_field = progs.ed_find_field(b"origin").unwrap();
    assert_eq!(origin_field.def_type(), DefType::Known(Etype::Vector));
    let value = progs.globals().get(29).unwrap();
    assert_eq!(origin_global.ofs, 29);
    assert_eq!(u32::try_from(value.as_i32()).unwrap(), origin_field.ofs);

    let slots: Vec<u32> = progs.field_defs().iter().map(|def| def.ofs).collect();
    assert_eq!(slots, [0, 0, 1, 1, 2, 3]);
    assert_eq!(progs.header().entityfields, 4);
}

// spec PROGS-9.6: lookups by name compare bytes exactly and take the first match in table
// order; lookups by slot take the first definition with that ofs.
#[test]
fn lookups_take_the_first_exact_match() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.ed_find_field(b"health").unwrap().ofs, 0);
    assert_eq!(progs.ed_find_field(b"Health"), None);
    assert_eq!(progs.ed_find_field(b"healt"), None);
    assert_eq!(progs.ed_find_field(b"health\0junk").unwrap().ofs, 0);
    assert_eq!(progs.ed_field_at_ofs(1).map(|def| def.type_bits), Some(3));
    assert_eq!(progs.ed_field_at_ofs(4), None);
    assert_eq!(progs.ed_global_at_ofs(1000), None);
    assert_eq!(progs.ed_find_global(b"missing"), None);
    assert_eq!(progs.ed_find_function(b"bump").unwrap().first_statement, 1);
    assert_eq!(
        progs.ed_find_function(b"dprint").unwrap().first_statement,
        -25
    );
    assert_eq!(progs.ed_find_function(b"nope"), None);
}

// spec PROGS-9.7: names are not unique; every IMMEDIATE definition is kept and a lookup
// finds the first.
// spec PROGS-14.2: loading keeps counts, order and duplicates.
#[test]
fn duplicate_names_are_kept() {
    let bytes = with_names(b"\0IMMEDIATE\0", &[0, 1, 1, 1]);
    let progs = read(&bytes).unwrap();
    let defs: Vec<Def> = progs.global_defs().iter().collect();
    assert_eq!(defs.len(), 4);
    assert_eq!(defs.iter().filter(|def| def.s_name == 1).count(), 3);
    let first = progs.ed_find_global(b"IMMEDIATE").unwrap();
    assert_eq!(first, defs[1]);
}

// spec PROGS-10.4: parm_size is single bytes, kept as stored, entries past numparms too.
// spec PROGS-10.6: profile is kept as stored.
// spec PROGS-10.5: numparms -1 (variadic) and 8 load.
#[test]
fn function_fields_kept_as_stored() {
    let spec = FunctionSpec {
        profile: 1234,
        numparms: -1,
        parm_size: [3, 1, 7, 0, 0, 0, 0, 9],
        ..FunctionSpec::default()
    };
    let eight = FunctionSpec {
        numparms: 8,
        ..FunctionSpec::default()
    };
    let bytes = FixtureSpec {
        functions: vec![function(&spec), function(&eight)],
        ..FixtureSpec::default()
    }
    .build();
    let progs = read(&bytes).unwrap();
    let loaded = progs.functions().get(0).unwrap();
    assert_eq!(loaded.profile, 1234);
    assert_eq!(loaded.numparms, -1);
    assert_eq!(loaded.parm_size, [3, 1, 7, 0, 0, 0, 0, 9]);
    assert_eq!(progs.functions().get(1).unwrap().numparms, 8);
}

// spec PROGS-11.1: a string reference is a byte offset; the string runs to the next NUL.
// spec PROGS-11.3: a reference may point into another string, and text may repeat.
// spec PROGS-11.5: the small vector's table ends with one padding NUL at byte 159.
#[test]
fn small_strings() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.string(1), Some(&b"test.qc"[..]));
    assert_eq!(progs.string(21), Some(&b"hi\n"[..]));
    assert_eq!(progs.string(3), Some(&b"st.qc"[..]));
    assert_eq!(progs.string(25), progs.string(118));
    assert_eq!(progs.string(150), Some(&b"origin_z"[..]));
    assert_eq!(progs.strings()[159], 0);
    assert_eq!(progs.string(159), Some(&b""[..]));
}

// spec PROGS-11.4: string bytes are uninterpreted: control and high-bit bytes load.
#[test]
fn string_bytes_are_uninterpreted() {
    let bytes = with_names(b"\0\x01\x7f\xff\x80\x0a\0\0", &[1]);
    let progs = read(&bytes).unwrap();
    let name = progs.global_defs().get(0).unwrap().s_name;
    assert_eq!(progs.string(name), Some(&b"\x01\x7f\xff\x80\x0a"[..]));
    assert_eq!(
        progs
            .ed_find_global(b"\x01\x7f\xff\x80\x0a")
            .map(|d| d.s_name),
        Some(1)
    );
}

// spec PROGS-11.6: a value outside the table is not resolved here; string() returns None.
#[test]
fn strings_outside_the_table_are_not_resolved() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.string(160), None);
    assert_eq!(progs.string(-1), None);
    assert_eq!(progs.string(i32::MAX), None);
    let unterminated = with_names(b"\0abc", &[0]);
    let progs = read(&unterminated).unwrap();
    assert_eq!(progs.string(1), None);
}

// spec PROGS-12.1: the globals hold every initial value as stored.
// spec PROGS-12.2: slots are untyped; the same table reads as floats or integers.
// spec PROGS-14.1: every value loads exactly as stored.
#[test]
fn small_globals() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    let globals: Vec<Slot> = progs.globals().iter().collect();
    let expected = [
        (28, 0x0000_0000),
        (29, 0x0000_0001),
        (30, 0x0000_0001),
        (31, 0x0000_0002),
        (32, 0x0000_0003),
        (34, 0x3F80_0000),
        (35, 0x4000_0000),
        (36, 0x4040_0000),
        (37, 0x0000_0015),
        (39, 0x0000_0001),
        (40, 0x0000_0002),
    ];
    for (slot, bits) in expected {
        assert_eq!(globals[slot], Slot(bits), "slot {slot}");
    }
    for zero in (0..28).chain([33, 38, 41, 42, 43]) {
        assert_eq!(globals[zero], Slot(0), "slot {zero}");
    }
    assert_eq!(globals[34].as_f32().to_bits(), 1.0_f32.to_bits());
    assert_eq!(globals[37].as_i32(), 21);
    assert_eq!(progs.string(globals[37].as_i32()), Some(&b"hi\n"[..]));
}

// spec PROGS-12.3: fewer than 28 globals is still a valid file for the engine.
// spec PROGS-13.1: entityfields is read as stored.
// spec PROGS-13.3: entityfields is not checked against the field definitions.
// spec PROGS-2.3: header fields are signed and not otherwise checked: entityfields -1 loads.
#[test]
fn unchecked_counts() {
    let bytes = FixtureSpec {
        globals: vec![7],
        field_defs: vec![def(2, 500, 0)],
        entityfields: -1,
        ..FixtureSpec::default()
    }
    .build();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.globals().len(), 1);
    assert_eq!(progs.header().entityfields, -1);
    assert_eq!(progs.field_defs().get(0).unwrap().ofs, 500);
}

// spec PROGS-1.2: tables are found only through the header: any order, gaps and overlap.
// spec PROGS-1.4: bytes no table covers are ignored, except by the file CRC.
#[test]
fn tables_in_any_order_with_gaps_and_overlap() {
    let plain = small();
    let gapped = {
        let progs = read(&plain).unwrap();
        let mut spec = FixtureSpec {
            statements: progs
                .statements()
                .as_bytes()
                .chunks(8)
                .map(<[u8]>::to_vec)
                .collect(),
            global_defs: progs
                .global_defs()
                .as_bytes()
                .chunks(8)
                .map(<[u8]>::to_vec)
                .collect(),
            field_defs: progs
                .field_defs()
                .as_bytes()
                .chunks(8)
                .map(<[u8]>::to_vec)
                .collect(),
            functions: progs
                .functions()
                .as_bytes()
                .chunks(36)
                .map(<[u8]>::to_vec)
                .collect(),
            strings: progs.strings().to_vec(),
            globals: progs.globals().iter().map(|slot| slot.0).collect(),
            entityfields: 4,
            ..FixtureSpec::default()
        };
        spec.gap = vec![0xAA, 0x55, 0x01];
        spec.build()
    };
    let a = read(&plain).unwrap();
    let b = read(&gapped).unwrap();
    assert_eq!(a.to_file().statements, b.to_file().statements);
    assert_eq!(a.to_file().globals, b.to_file().globals);
    assert_eq!(a.to_file().functions, b.to_file().functions);
    assert_ne!(a.file_crc(), b.file_crc());

    // Statements and global definitions pointing at the same bytes, and the field
    // definitions moved before the strings, still load.
    let mut overlapped = plain.clone();
    let ofs_statements = word(&plain, field::OFS_STATEMENTS);
    patch(&mut overlapped, field::OFS_GLOBALDEFS, ofs_statements);
    patch(&mut overlapped, field::NUMGLOBALDEFS, 1);
    let progs = read(&overlapped).unwrap();
    assert_eq!(
        progs.global_defs().as_bytes(),
        &progs.statements().as_bytes()[..8]
    );
    assert!(progs.layout().is_none());
}

// spec PROGS-6.1: the file CRC covers every byte as stored: 25104 for minimal, 36518 for
// small.
// spec PROGS-3.6: the header CRC identifies the system definitions, not the file: two
// different programs carry the same 5927.
#[test]
fn file_crc() {
    let minimal = minimal();
    let small = small();
    let (a, b) = (read(&minimal).unwrap(), read(&small).unwrap());
    assert_eq!(a.file_crc(), 25104);
    assert_eq!(b.file_crc(), 36518);
    assert_eq!(a.header().crc, b.header().crc);
}

// spec PROGS-3.1: a version other than 6 is rejected with id's message.
#[test]
fn wrong_version() {
    let mut bytes = minimal();
    patch(&mut bytes, field::VERSION, 5);
    let error = read(&bytes).unwrap_err();
    assert_eq!(error, ReadError::WrongVersion { version: 5 });
    assert_eq!(
        error.to_string(),
        "progs.dat has wrong version number (5 should be 6)"
    );
    patch(&mut bytes, field::VERSION, -6);
    assert_eq!(
        read(&bytes).unwrap_err(),
        ReadError::WrongVersion { version: -6 }
    );
}

// spec PROGS-3.2: a header CRC other than 5927 is rejected with id's message.
#[test]
fn wrong_header_crc() {
    let mut bytes = minimal();
    patch(&mut bytes, field::CRC, 5928);
    let error = read(&bytes).unwrap_err();
    assert_eq!(error, ReadError::HeaderCrc { crc: 5928 });
    assert_eq!(
        error.to_string(),
        "progs.dat system vars have been modified, progdefs.h is out of date"
    );
}

// spec PROGS-3.3: the version is checked before the CRC.
#[test]
fn version_before_crc() {
    let mut bytes = minimal();
    patch(&mut bytes, field::VERSION, 5);
    patch(&mut bytes, field::CRC, 0);
    assert_eq!(
        read(&bytes).unwrap_err(),
        ReadError::WrongVersion { version: 5 }
    );
}

// spec PROGS-3.3: version 7 with CRC 0 reports the version, as id's loader does, when FTE
// reading is off.
#[cfg(not(feature = "fte"))]
#[test]
fn version_7_is_wrong_without_fte() {
    let mut bytes = minimal();
    patch(&mut bytes, field::VERSION, 7);
    patch(&mut bytes, field::CRC, 0);
    let error = read(&bytes).unwrap_err();
    assert_eq!(
        error.to_string(),
        "progs.dat has wrong version number (7 should be 6)"
    );
}

// spec PROGS-3.4: a field definition with DEF_SAVEGLOBAL is rejected with id's message.
#[test]
fn saved_field_definition() {
    let mut bytes = small();
    let ofs = usize::try_from(word(&bytes, field::OFS_FIELDDEFS)).unwrap();
    bytes[ofs + 8..ofs + 10].copy_from_slice(&0x8002_u16.to_le_bytes());
    let error = read(&bytes).unwrap_err();
    assert_eq!(error, ReadError::FieldDefSaveGlobal { index: 1 });
    assert_eq!(
        error.to_string(),
        "PR_LoadProgs: pr_fielddefs[i].type & DEF_SAVEGLOBAL"
    );
}

// spec PROGS-3.5: the engine checks only version, CRC and field flags; opcodes, operands and
// string values in globals are not checked at load.
// spec PROGS-7.3: a function's first_statement is not checked against the statements.
// spec PROGS-7.4: an opcode above 65 loads.
// spec PROGS-15.7: opcodes, operands, first_statement, parm_start, locals, ofs and global
// values are not checked.
#[test]
fn execution_fields_are_not_checked() {
    let bytes = FixtureSpec {
        statements: vec![
            statement(0, 0, 0, 0),
            statement(u16::MAX, -32768, 32767, -1),
        ],
        functions: vec![function(&FunctionSpec {
            first_statement: 1_000_000,
            parm_start: -5,
            locals: i32::MAX,
            ..FunctionSpec::default()
        })],
        global_defs: vec![def(0x7FFF, u16::MAX, 0)],
        globals: vec![u32::MAX; 30],
        ..FixtureSpec::default()
    }
    .build();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.statements().get(1).unwrap().op, 65535);
    assert_eq!(progs.functions().get(0).unwrap().first_statement, 1_000_000);
    assert_eq!(progs.functions().get(0).unwrap().locals, i32::MAX);
    assert_eq!(progs.global_defs().get(0).unwrap().ofs, 65535);
    assert_eq!(progs.globals().get(0), Some(Slot(u32::MAX)));
}

// spec PROGS-15.1: a file shorter than 60 bytes is rejected.
#[test]
fn too_short() {
    for len in [0, 1, 59] {
        let bytes = vec![0; len];
        assert_eq!(
            read(&bytes).unwrap_err(),
            ReadError::TooShort { len, needed: 60 }
        );
    }
    let header_only = vec![0; 60];
    assert_eq!(
        read(&header_only).unwrap_err(),
        ReadError::WrongVersion { version: 0 }
    );
    let error = read(&[0; 10]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "progs.dat is 10 bytes, shorter than its 60-byte header"
    );
}

const TABLES: [(TableId, usize, usize); 6] = [
    (
        TableId::Statements,
        field::OFS_STATEMENTS,
        field::NUMSTATEMENTS,
    ),
    (
        TableId::GlobalDefs,
        field::OFS_GLOBALDEFS,
        field::NUMGLOBALDEFS,
    ),
    (
        TableId::FieldDefs,
        field::OFS_FIELDDEFS,
        field::NUMFIELDDEFS,
    ),
    (
        TableId::Functions,
        field::OFS_FUNCTIONS,
        field::NUMFUNCTIONS,
    ),
    (TableId::Strings, field::OFS_STRINGS, field::NUMSTRINGS),
    (TableId::Globals, field::OFS_GLOBALS, field::NUMGLOBALS),
];

// spec PROGS-15.2: a negative offset or count, an end past the file, or an end that
// overflows is rejected; an end exactly at the end of the file is accepted.
#[test]
fn table_bounds() {
    let base = small();
    let len = i32::try_from(base.len()).unwrap();
    for (table, ofs, num) in TABLES {
        let rejected = ReadError::TableOutOfBounds { table };
        let mut bytes = base.clone();
        patch(&mut bytes, ofs, -1);
        assert_eq!(read(&bytes).unwrap_err(), rejected, "{table} offset -1");

        let mut bytes = base.clone();
        patch(&mut bytes, num, -1);
        assert_eq!(read(&bytes).unwrap_err(), rejected, "{table} count -1");

        let mut bytes = base.clone();
        patch(&mut bytes, ofs, i32::MAX);
        patch(&mut bytes, num, i32::MAX);
        assert_eq!(read(&bytes).unwrap_err(), rejected, "{table} overflow");

        let mut bytes = base.clone();
        patch(&mut bytes, ofs, len + 1);
        patch(&mut bytes, num, 0);
        assert_eq!(read(&bytes).unwrap_err(), rejected, "{table} past the end");

        let mut bytes = base.clone();
        patch(&mut bytes, ofs, len);
        patch(&mut bytes, num, 0);
        if table == TableId::Strings {
            // An empty string table leaves every name out of range (PROGS-15.3).
            assert!(matches!(
                read(&bytes).unwrap_err(),
                ReadError::StringOutOfRange { .. }
            ));
        } else {
            assert!(read(&bytes).is_ok(), "{table} empty at the end");
        }
    }
    let error = ReadError::TableOutOfBounds {
        table: TableId::Globals,
    };
    assert_eq!(
        error.to_string(),
        "progs.dat globals table lies outside the file"
    );

    // The last table ending one byte past the file.
    let mut bytes = base.clone();
    bytes.pop();
    assert_eq!(
        read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::Globals
        }
    );
}

// spec PROGS-15.3: a name or file reference that is negative or not less than numstrings is
// rejected, in definitions and functions.
#[test]
fn string_references_in_range() {
    let names = |s_name| FixtureSpec {
        strings: b"\0ab\0".to_vec(),
        global_defs: vec![def(0, 0, s_name)],
        ..FixtureSpec::default()
    };
    for bad in [-1, 4, i32::MAX] {
        let error = read(&names(bad).build()).unwrap_err();
        assert_eq!(
            error,
            ReadError::StringOutOfRange {
                site: StringSite::GlobalDefName(0),
                offset: bad
            }
        );
    }
    assert!(read(&names(3).build()).is_ok());

    let fields = FixtureSpec {
        field_defs: vec![def(0, 0, 0), def(2, 0, 4)],
        ..FixtureSpec::default()
    };
    assert_eq!(
        read(&fields.build()).unwrap_err(),
        ReadError::StringOutOfRange {
            site: StringSite::FieldDefName(1),
            offset: 4
        }
    );
    let file = FixtureSpec {
        functions: vec![function(&FunctionSpec {
            s_file: -2,
            ..FunctionSpec::default()
        })],
        ..FixtureSpec::default()
    };
    let error = read(&file.build()).unwrap_err();
    assert_eq!(
        error,
        ReadError::StringOutOfRange {
            site: StringSite::FunctionFile(0),
            offset: -2
        }
    );
    assert_eq!(
        error.to_string(),
        "progs.dat the file name of function 0 is -2, outside the string table"
    );
}

// spec PROGS-15.4: a reference that reaches the end of the table without a NUL is rejected,
// even when the next table starts with a NUL.
#[test]
fn unterminated_strings() {
    let file = FixtureSpec {
        strings: b"\0abc".to_vec(),
        functions: vec![function(&FunctionSpec {
            s_name: 1,
            ..FunctionSpec::default()
        })],
        ..FixtureSpec::default()
    };
    let bytes = file.build();
    let error = read(&bytes).unwrap_err();
    assert_eq!(
        error,
        ReadError::UnterminatedString {
            site: StringSite::FunctionName(0),
            offset: 1
        }
    );
    assert_eq!(
        error.to_string(),
        "progs.dat the name of function 0 at 1 runs to the end of the string table without a NUL"
    );
    let ok = FixtureSpec {
        functions: vec![function(&FunctionSpec::default())],
        ..file
    };
    assert!(read(&ok.build()).is_ok());
}

// spec PROGS-15.6: a function with more than 8 parameters is rejected.
// spec PROGS-10.5: the engine reads only parm_size[0..numparms), so 9 cannot be honoured.
#[test]
fn too_many_parameters() {
    let file = FixtureSpec {
        functions: vec![
            function(&FunctionSpec::default()),
            function(&FunctionSpec {
                numparms: 9,
                ..FunctionSpec::default()
            }),
        ],
        ..FixtureSpec::default()
    };
    let error = read(&file.build()).unwrap_err();
    assert_eq!(
        error,
        ReadError::TooManyParms {
            index: 1,
            numparms: 9
        }
    );
    assert_eq!(
        error.to_string(),
        "progs.dat function 1 has 9 parameters, more than 8"
    );
}

// spec PROGS-15.5: checks run in order size, version, CRC, bounds, field flags, string
// references, parameter counts; the first failure is reported.
#[test]
fn check_order() {
    let bad_everything = FixtureSpec {
        version: 5,
        crc: 1,
        field_defs: vec![def(0x8002, 0, 99)],
        functions: vec![function(&FunctionSpec {
            numparms: 20,
            s_name: 99,
            ..FunctionSpec::default()
        })],
        ..FixtureSpec::default()
    };
    let mut bytes = bad_everything.clone().build();
    patch(&mut bytes, field::NUMGLOBALS, 1000);
    let mut expect = |expected: ReadError, fix: &mut dyn FnMut(&mut Vec<u8>)| {
        assert_eq!(read(&bytes).unwrap_err(), expected);
        fix(&mut bytes);
    };
    expect(ReadError::WrongVersion { version: 5 }, &mut |b| {
        patch(b, field::VERSION, 6);
    });
    expect(ReadError::HeaderCrc { crc: 1 }, &mut |b| {
        patch(b, field::CRC, 5927);
    });
    expect(
        ReadError::TableOutOfBounds {
            table: TableId::Globals,
        },
        &mut |b| patch(b, field::NUMGLOBALS, 28),
    );
    expect(ReadError::FieldDefSaveGlobal { index: 0 }, &mut |b| {
        let ofs = usize::try_from(word(b, field::OFS_FIELDDEFS)).unwrap();
        b[ofs + 1] = 0;
    });
    expect(
        ReadError::StringOutOfRange {
            site: StringSite::FieldDefName(0),
            offset: 99,
        },
        &mut |b| {
            let ofs = usize::try_from(word(b, field::OFS_FIELDDEFS)).unwrap();
            b[ofs + 4] = 0;
            let ofs = usize::try_from(word(b, field::OFS_FUNCTIONS)).unwrap();
            b[ofs + 16] = 0;
        },
    );
    expect(
        ReadError::TooManyParms {
            index: 0,
            numparms: 20,
        },
        &mut |_| {},
    );
    let short = &bad_everything.build()[..30];
    assert_eq!(
        read(short).unwrap_err(),
        ReadError::TooShort {
            len: 30,
            needed: 60
        }
    );
}

// spec PROGS-15.9: within a step, bounds go in header order, string references go global
// definitions, field definitions, then functions with s_name before s_file.
#[test]
fn order_within_a_step() {
    let mut bytes = small();
    patch(&mut bytes, field::NUMSTATEMENTS, -1);
    patch(&mut bytes, field::NUMGLOBALS, -1);
    assert_eq!(
        read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::Statements
        }
    );
    let mut bytes = small();
    patch(&mut bytes, field::NUMFIELDDEFS, -1);
    patch(&mut bytes, field::NUMSTRINGS, -1);
    assert_eq!(
        read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::FieldDefs
        }
    );

    let file = FixtureSpec {
        global_defs: vec![def(0, 0, 0), def(0, 0, 50)],
        field_defs: vec![def(0, 0, 50)],
        functions: vec![function(&FunctionSpec {
            s_name: 50,
            s_file: 51,
            ..FunctionSpec::default()
        })],
        ..FixtureSpec::default()
    };
    let site = |file: &FixtureSpec| match read(&file.build()).unwrap_err() {
        ReadError::StringOutOfRange { site, .. } => site,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(site(&file), StringSite::GlobalDefName(1));
    let file = FixtureSpec {
        global_defs: vec![def(0, 0, 0)],
        ..file
    };
    assert_eq!(site(&file), StringSite::FieldDefName(0));
    let file = FixtureSpec {
        field_defs: vec![def(0, 0, 0)],
        ..file
    };
    assert_eq!(site(&file), StringSite::FunctionName(0));

    let file = FixtureSpec {
        field_defs: vec![def(0x8000, 0, 0), def(0x8000, 0, 0)],
        functions: vec![
            function(&FunctionSpec {
                numparms: 9,
                ..FunctionSpec::default()
            }),
            function(&FunctionSpec {
                numparms: 10,
                ..FunctionSpec::default()
            }),
        ],
        ..FixtureSpec::default()
    };
    assert_eq!(
        read(&file.build()).unwrap_err(),
        ReadError::FieldDefSaveGlobal { index: 0 }
    );
    let file = FixtureSpec {
        field_defs: vec![def(0, 0, 0)],
        ..file
    };
    assert_eq!(
        read(&file.build()).unwrap_err(),
        ReadError::TooManyParms {
            index: 0,
            numparms: 9
        }
    );
}

// spec PROGS-15.8: the header CRC check can be skipped on request; by default it runs.
#[test]
fn skip_header_crc() {
    let mut bytes = minimal();
    patch(&mut bytes, field::CRC, 1234);
    assert!(ReadOptions::default().check_header_crc);
    assert!(read(&bytes).is_err());
    let options = ReadOptions {
        check_header_crc: false,
    };
    let progs = Progs::read_with(&bytes, &options).unwrap();
    assert_eq!(progs.header().crc, 1234);
    patch(&mut bytes, field::VERSION, 5);
    assert_eq!(
        Progs::read_with(&bytes, &options).unwrap_err(),
        ReadError::WrongVersion { version: 5 }
    );
}

// spec PROGS-14.1: tables give exactly what was stored; out-of-range indices give None.
#[test]
fn table_access() {
    let bytes = small();
    let progs = read(&bytes).unwrap();
    assert_eq!(progs.bytes(), &bytes[..]);
    let defs = progs.global_defs();
    assert!(!defs.is_empty());
    assert_eq!(defs.get(17), None);
    assert_eq!(defs.get(usize::MAX), None);
    assert_eq!(defs.into_iter().count(), 17);
    let mut iter = defs.iter();
    assert_eq!(iter.size_hint(), (17, Some(17)));
    iter.next();
    assert_eq!(iter.len(), 16);
    let empty = FixtureSpec {
        statements: Vec::new(),
        ..FixtureSpec::default()
    }
    .build();
    assert!(read(&empty).unwrap().statements().is_empty());
}

/// Bytes of a random valid version 6 file, built without the writer, and its records.
fn valid_file() -> impl Strategy<Value = Vec<u8>> {
    let strings = proptest::collection::vec(1_u8..=255, 0..12).prop_map(|mut text| {
        text.push(0);
        text
    });
    (
        strings,
        proptest::collection::vec(any::<[u8; 8]>(), 0..6),
        proptest::collection::vec((any::<u16>(), any::<u16>()), 0..6),
        proptest::collection::vec((any::<u16>(), any::<u16>()), 0..6),
        proptest::collection::vec((any::<[i32; 4]>(), -1_i32..=8, any::<[u8; 8]>()), 0..4),
        proptest::collection::vec(any::<u32>(), 0..40),
        proptest::collection::vec(any::<u8>(), 0..5),
        any::<i32>(),
    )
        .prop_map(
            |(strings, statements, globals, fields, functions, slots, gap, entityfields)| {
                let last = i32::try_from(strings.len()).unwrap() - 1;
                FixtureSpec {
                    statements: statements.iter().map(|s| s.to_vec()).collect(),
                    global_defs: globals.iter().map(|&(t, o)| def(t, o, last)).collect(),
                    field_defs: fields.iter().map(|&(t, o)| def(t & 0x7FFF, o, 0)).collect(),
                    functions: functions
                        .iter()
                        .map(|&([first, start, locals, profile], numparms, parm_size)| {
                            function(&FunctionSpec {
                                first_statement: first,
                                parm_start: start,
                                locals,
                                profile,
                                s_name: last,
                                s_file: 0,
                                numparms,
                                parm_size,
                            })
                        })
                        .collect(),
                    strings,
                    globals: slots,
                    gap,
                    entityfields,
                    ..FixtureSpec::default()
                }
                .build()
            },
        )
}

proptest! {
    // spec PROGS-14.1: every field of every record reads back exactly as the bytes hold it.
    // spec PROGS-1.1: every field decodes little-endian.
    #[test]
    fn valid_files_decode_as_stored(bytes in valid_file()) {
        let progs = read(&bytes).unwrap();
        let header = progs.header();
        let at = |ofs: i32| usize::try_from(ofs).unwrap();
        for (index, statement) in progs.statements().iter().enumerate() {
            let raw = &bytes[at(header.ofs_statements) + index * 8..][..8];
            prop_assert_eq!(statement.op, u32::from(u16::from_le_bytes([raw[0], raw[1]])));
            prop_assert_eq!(statement.a, i32::from(i16::from_le_bytes([raw[2], raw[3]])));
            prop_assert_eq!(statement.b, i32::from(i16::from_le_bytes([raw[4], raw[5]])));
            prop_assert_eq!(statement.c, i32::from(i16::from_le_bytes([raw[6], raw[7]])));
        }
        for (index, slot) in progs.globals().iter().enumerate() {
            let raw = &bytes[at(header.ofs_globals) + index * 4..][..4];
            prop_assert_eq!(slot.0, u32::from_le_bytes(raw.try_into().unwrap()));
        }
        for (index, def) in progs.global_defs().iter().enumerate() {
            let raw = &bytes[at(header.ofs_globaldefs) + index * 8..][..8];
            prop_assert_eq!(def.type_bits, u32::from(u16::from_le_bytes([raw[0], raw[1]])));
            prop_assert_eq!(def.ofs, u32::from(u16::from_le_bytes([raw[2], raw[3]])));
        }
    }

    // spec PROGS-15.1: arbitrary bytes never panic the reader; every rejection is a typed
    // error, and every accepted file's tables lie inside it.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let options = ReadOptions { check_header_crc: false };
        if let Ok(progs) = Progs::read_with(&bytes, &options) {
            prop_assert!(progs.statements().len() * 8 <= bytes.len());
            prop_assert_eq!(progs.to_file().layout, progs.layout());
        }
    }

    // spec PROGS-15.2: a corrupted header field never panics: the file loads or is rejected.
    #[test]
    fn corrupted_headers_never_panic(index in 2_usize..15, value in any::<i32>()) {
        let mut bytes = small();
        patch(&mut bytes, index, value);
        if let Ok(progs) = read(&bytes) {
            prop_assert_eq!(word(&bytes, index), value);
            prop_assert_eq!(progs.to_file().layout, progs.layout());
        }
    }
}
