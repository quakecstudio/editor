// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use proptest::prelude::*;
use qc_types::Slot;

use crate::fixtures::{
    FixtureSpec, FunctionSpec, VECTORS, Vector, def, field, function, patch, vector, word,
};
use crate::{Def, Function, Progs, ProgsFile, ReadError, Statement, TableId, WriteError, write};

fn round_trip(bytes: &[u8]) -> Vec<u8> {
    write(&Progs::read(bytes).unwrap().to_file()).unwrap()
}

fn canonical(bytes: &[u8]) -> Vec<u8> {
    let file = ProgsFile {
        layout: None,
        ..Progs::read(bytes).unwrap().to_file()
    };
    write(&file).unwrap()
}

/// The records of a file, with the layout left out.
fn records(bytes: &[u8]) -> ProgsFile {
    ProgsFile {
        layout: None,
        ..Progs::read(bytes).unwrap().to_file()
    }
}

/// The small vector with its tables in reverse order and bytes before, between and after
/// them.
fn reordered_small() -> Vec<u8> {
    let small = vector(Vector::Small);
    let progs = Progs::read(&small).unwrap();
    let tables: [(usize, &[u8]); 6] = [
        (field::OFS_GLOBALS, progs.globals().as_bytes()),
        (field::OFS_FIELDDEFS, progs.field_defs().as_bytes()),
        (field::OFS_GLOBALDEFS, progs.global_defs().as_bytes()),
        (field::OFS_FUNCTIONS, progs.functions().as_bytes()),
        (field::OFS_STATEMENTS, progs.statements().as_bytes()),
        (field::OFS_STRINGS, progs.strings()),
    ];
    let mut bytes = small[..60].to_vec();
    bytes.extend([9, 9, 9]);
    for (index, (ofs, table)) in tables.into_iter().enumerate() {
        let offset = i32::try_from(bytes.len()).unwrap();
        patch(&mut bytes, ofs, offset);
        bytes.extend_from_slice(table);
        bytes.extend(vec![0xA5; index]);
    }
    bytes.extend([1, 2, 3, 4, 5]);
    bytes
}

// spec PROGS-22.1: every spec vector reads and writes back byte for byte.
#[test]
fn vectors_round_trip() {
    for &vector_id in VECTORS {
        let bytes = vector(vector_id);
        assert_eq!(round_trip(&bytes), bytes, "{vector_id:?}");
    }
}

// spec PROGS-22.1: tables in another order, with bytes before, between and after them, are
// written back where they were.
#[test]
fn layout_is_kept() {
    let bytes = reordered_small();
    let progs = Progs::read(&bytes).unwrap();
    assert!(progs.layout().is_some());
    assert_eq!(round_trip(&bytes), bytes);
    assert_eq!(records(&bytes), records(&vector(Vector::Small)));

    let gapped = FixtureSpec {
        gap: vec![0xEE, 0x00, 0x7F],
        ..FixtureSpec::default()
    }
    .build();
    assert_eq!(round_trip(&gapped), gapped);
}

// spec PROGS-22.1: an empty table keeps the offset the file stored, even inside another
// table's bytes or the header.
#[test]
fn empty_tables_keep_their_offset() {
    let mut bytes = vector(Vector::Minimal);
    patch(&mut bytes, field::NUMSTATEMENTS, 0);
    patch(&mut bytes, field::NUMFUNCTIONS, 0);
    patch(&mut bytes, field::OFS_FUNCTIONS, 3);
    assert_eq!(round_trip(&bytes), bytes);
}

// spec PROGS-22.2: without a layout, tables go in qcc's order, which is how the vectors are
// laid out.
// spec PROGS-1.3: qcc's order and alignment reproduce the minimal and small vectors.
#[test]
fn qcc_order() {
    for &vector_id in VECTORS {
        let bytes = vector(vector_id);
        assert_eq!(canonical(&bytes), bytes, "{vector_id:?}");
    }
    let reordered = reordered_small();
    assert_eq!(canonical(&reordered), vector(Vector::Small));
}

// spec PROGS-22.2: the string table is padded with NULs to a multiple of 4 and numstrings
// counts the padding; empty tables sit where they would start.
#[test]
fn qcc_order_pads_strings() {
    let file = ProgsFile {
        strings: b"\0ab\0x\0".to_vec(),
        ..ProgsFile::default()
    };
    let bytes = write(&file).unwrap();
    assert_eq!(word(&bytes, field::OFS_STRINGS), 60);
    assert_eq!(word(&bytes, field::NUMSTRINGS), 8);
    assert_eq!(&bytes[60..68], b"\0ab\0x\0\0\0");
    for ofs in [
        field::OFS_STATEMENTS,
        field::OFS_FUNCTIONS,
        field::OFS_GLOBALDEFS,
        field::OFS_FIELDDEFS,
        field::OFS_GLOBALS,
    ] {
        assert_eq!(word(&bytes, ofs), 68);
    }
    assert_eq!(bytes.len(), 68);

    let empty = write(&ProgsFile::default()).unwrap();
    assert_eq!(empty.len(), 60);
    assert_eq!(word(&empty, field::VERSION), 6);
    assert_eq!(word(&empty, field::CRC), 5927);
    assert_eq!(word(&empty, field::NUMSTRINGS), 0);
}

// spec PROGS-22.3: counts and offsets come from the tables; crc and entityfields as given.
// A table that grows pushes the later tables along and keeps the bytes between them.
#[test]
fn header_follows_the_tables() {
    let bytes = reordered_small();
    let mut file = Progs::read(&bytes).unwrap().to_file();
    file.crc = 77;
    file.entityfields = 9;
    file.globals.push(Slot::from_bits(0xDEAD_BEEF));
    let written = write(&file).unwrap();
    assert_eq!(written.len(), bytes.len() + 4);
    assert_eq!(word(&written, field::CRC), 77);
    assert_eq!(word(&written, field::ENTITYFIELDS), 9);
    assert_eq!(word(&written, field::NUMGLOBALS), 45);
    assert_eq!(word(&written, field::OFS_GLOBALS), 63);
    assert_eq!(
        word(&written, field::OFS_FIELDDEFS),
        word(&bytes, field::OFS_FIELDDEFS) + 4
    );
    assert_eq!(written[..3 + 60][60..], [9, 9, 9]);
    assert_eq!(written[written.len() - 5..], [1, 2, 3, 4, 5]);
    let options = crate::ReadOptions {
        check_header_crc: false,
    };
    let reread = Progs::read_with(&written, &options).unwrap();
    assert_eq!(reread.globals().get(44), Some(Slot::from_bits(0xDEAD_BEEF)));
    assert_eq!(reread.layout(), file.layout);
}

// spec PROGS-22.3: a table the source had empty, and that now has records, goes after the
// source's last byte.
#[test]
fn new_records_go_at_the_end() {
    let mut bytes = vector(Vector::Minimal);
    patch(&mut bytes, field::NUMSTATEMENTS, 0);
    let mut file = Progs::read(&bytes).unwrap().to_file();
    file.statements.push(Statement {
        op: 61,
        a: -1,
        b: 0,
        c: 0,
    });
    let written = write(&file).unwrap();
    assert_eq!(&written[60..bytes.len()], &bytes[60..]);
    assert_eq!(
        word(&written, field::OFS_STATEMENTS),
        i32::try_from(bytes.len()).unwrap()
    );
    assert_eq!(&written[bytes.len()..], [61, 0, 0xff, 0xff, 0, 0, 0, 0]);

    // A table that stays empty keeps its stored offset when another one grows.
    patch(&mut bytes, field::NUMFUNCTIONS, 0);
    patch(&mut bytes, field::OFS_FUNCTIONS, 3);
    let mut file = Progs::read(&bytes).unwrap().to_file();
    file.statements.push(Statement::default());
    let written = write(&file).unwrap();
    assert_eq!(word(&written, field::OFS_FUNCTIONS), 3);
    assert_eq!(
        word(&written, field::OFS_STATEMENTS),
        i32::try_from(bytes.len()).unwrap()
    );
}

// spec PROGS-22.4: a version 6 opcode above 65535 or operand outside -32768..32767 is
// rejected.
// spec LIMIT-1.1: a version 6 operand cannot name a slot above 32767.
#[test]
fn statement_out_of_range() {
    let bad = [
        Statement {
            op: 65536,
            ..Statement::default()
        },
        Statement {
            a: 32768,
            ..Statement::default()
        },
        Statement {
            b: -32769,
            ..Statement::default()
        },
        Statement {
            c: i32::MAX,
            ..Statement::default()
        },
    ];
    for statement in bad {
        let file = ProgsFile {
            statements: vec![Statement::default(), statement],
            ..ProgsFile::default()
        };
        let error = write(&file).unwrap_err();
        assert_eq!(error, WriteError::StatementOutOfRange { index: 1 });
        assert_eq!(
            error.to_string(),
            "statement 1 does not fit the progs.dat format"
        );
    }
    let edge = ProgsFile {
        statements: vec![Statement {
            op: 65535,
            a: 32767,
            b: -32768,
            c: 0,
        }],
        ..ProgsFile::default()
    };
    assert!(write(&edge).is_ok());
}

// spec PROGS-22.4: a version 6 definition type or ofs above 65535 is rejected.
// spec LIMIT-1.2: a version 6 definition cannot name a slot above 65535.
#[test]
fn def_out_of_range() {
    let big_ofs = Def {
        ofs: 65536,
        ..Def::default()
    };
    let big_type = Def {
        type_bits: 0x1_0000,
        ..Def::default()
    };
    let file = ProgsFile {
        global_defs: vec![big_ofs],
        ..ProgsFile::default()
    };
    assert_eq!(
        write(&file).unwrap_err(),
        WriteError::DefOutOfRange {
            table: TableId::GlobalDefs,
            index: 0
        }
    );
    let file = ProgsFile {
        field_defs: vec![Def::default(), big_type],
        ..ProgsFile::default()
    };
    let error = write(&file).unwrap_err();
    assert_eq!(
        error,
        WriteError::DefOutOfRange {
            table: TableId::FieldDefs,
            index: 1
        }
    );
    assert_eq!(
        error.to_string(),
        "field definitions entry 1 does not fit the progs.dat format"
    );
    let edge = ProgsFile {
        global_defs: vec![Def {
            type_bits: 0xFFFF,
            ofs: 65535,
            s_name: -1,
        }],
        ..ProgsFile::default()
    };
    assert!(write(&edge).is_ok());
    let error = WriteError::TooLarge {
        table: TableId::Strings,
    };
    assert_eq!(
        error.to_string(),
        "the strings table is too large for progs.dat"
    );
}

// spec PROGS-22.5: a written file whose records pass PROGS-15 reads back as given; one that
// does not is rejected by the reader, not changed by the writer.
#[test]
fn invalid_records_are_written_as_given() {
    let file = ProgsFile {
        strings: vec![0],
        global_defs: vec![Def {
            s_name: 5,
            ..Def::default()
        }],
        ..ProgsFile::default()
    };
    let bytes = write(&file).unwrap();
    assert!(matches!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::StringOutOfRange { offset: 5, .. }
    ));
}

/// A random valid set of version 6 records.
fn valid_records() -> impl Strategy<Value = ProgsFile> {
    let strings = proptest::collection::vec(1_u8..=255, 0..12).prop_map(|mut text| {
        text.extend([0; 4]);
        text
    });
    (
        strings,
        proptest::collection::vec((any::<u16>(), any::<[i16; 3]>()), 0..6),
        proptest::collection::vec((any::<u16>(), any::<u16>()), 0..6),
        proptest::collection::vec((0_u16..0x8000, any::<u16>()), 0..6),
        proptest::collection::vec((any::<[i32; 4]>(), -1_i32..=8, any::<[u8; 8]>()), 0..4),
        proptest::collection::vec(any::<u32>(), 0..40),
        any::<(i32, i32)>(),
    )
        .prop_map(
            |(strings, statements, globals, fields, functions, slots, (crc, entityfields))| {
                let name = i32::try_from(strings.len()).unwrap() - 1;
                ProgsFile {
                    crc,
                    entityfields,
                    statements: statements
                        .into_iter()
                        .map(|(op, [a, b, c])| Statement {
                            op: u32::from(op),
                            a: i32::from(a),
                            b: i32::from(b),
                            c: i32::from(c),
                        })
                        .collect(),
                    global_defs: globals
                        .into_iter()
                        .map(|(type_bits, ofs)| Def {
                            type_bits: u32::from(type_bits),
                            ofs: u32::from(ofs),
                            s_name: name,
                        })
                        .collect(),
                    field_defs: fields
                        .into_iter()
                        .map(|(type_bits, ofs)| Def {
                            type_bits: u32::from(type_bits),
                            ofs: u32::from(ofs),
                            s_name: 0,
                        })
                        .collect(),
                    functions: functions
                        .into_iter()
                        .map(
                            |([first, start, locals, profile], numparms, parm_size)| Function {
                                first_statement: first,
                                parm_start: start,
                                locals,
                                profile,
                                s_name: name,
                                s_file: 0,
                                numparms,
                                parm_size,
                            },
                        )
                        .collect(),
                    strings,
                    globals: slots.into_iter().map(Slot::from_bits).collect(),
                    ..ProgsFile::default()
                }
            },
        )
}

proptest! {
    // spec PROGS-22.5: records that pass PROGS-15 read back exactly as written.
    #[test]
    fn records_read_back_as_written(file in valid_records()) {
        let bytes = write(&file).unwrap();
        let options = crate::ReadOptions { check_header_crc: false };
        let progs = Progs::read_with(&bytes, &options).unwrap();
        let mut reread = progs.to_file();
        prop_assert!(reread.layout.is_some());
        reread.layout = None;
        let mut expected = file.clone();
        while !expected.strings.len().is_multiple_of(4) {
            expected.strings.push(0);
        }
        prop_assert_eq!(reread, expected);
    }

    // spec PROGS-22.1: a hand-built file with gaps and random contents round-trips byte for
    // byte.
    #[test]
    fn hand_built_files_round_trip(
        gap in proptest::collection::vec(any::<u8>(), 0..6),
        globals in proptest::collection::vec(any::<u32>(), 0..30),
        statements in proptest::collection::vec(any::<[u8; 8]>(), 0..5),
        numparms in -3_i32..=8,
    ) {
        let bytes = FixtureSpec {
            gap,
            globals,
            statements: statements.iter().map(|s| s.to_vec()).collect(),
            functions: vec![function(&FunctionSpec { numparms, ..FunctionSpec::default() })],
            global_defs: vec![def(0x8003, 99, 0)],
            ..FixtureSpec::default()
        }
        .build();
        prop_assert_eq!(round_trip(&bytes), bytes);
    }

    // spec PROGS-22.1: any accepted file without overlapping tables writes back unchanged;
    // one with overlaps writes back with the same records.
    #[test]
    fn accepted_mutations_round_trip(
        flips in proptest::collection::vec((0_usize..744, any::<u8>()), 1..4),
    ) {
        let mut bytes = vector(Vector::Small);
        for (at, value) in flips {
            bytes[at] = value;
        }
        let options = crate::ReadOptions { check_header_crc: false };
        if let Ok(progs) = Progs::read_with(&bytes, &options) {
            let file = progs.to_file();
            let written = write(&file).unwrap();
            if file.layout.is_some() {
                prop_assert_eq!(written, bytes);
            } else {
                let reread = Progs::read_with(&written, &options).unwrap().to_file();
                prop_assert_eq!(reread.statements, file.statements);
                prop_assert_eq!(reread.globals, file.globals);
            }
        }
    }
}
