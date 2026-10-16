// SPDX-License-Identifier: GPL-3.0-or-later

//! FTE version 7 reading and writing (PROGS-19 to PROGS-21).

use crate::fixtures::{Vector, field, patch, vector, word};
use crate::{
    Def, Format, FteHeader, FteParts, PROGS_FTE_HEADER_SIZE, PROGS_FTE_SECONDARY16,
    PROGS_FTE_SECONDARY32, PROGS_FTE32_DEF_SIZE, PROGS_FTE32_STATEMENT_SIZE, PROGS_VERSION_FTE,
    Progs, ProgsFile, ReadError, Statement, StringSite, TableId, WriteError, write,
};

const OFSFILES: usize = 15;
const OFSLINENUMS: usize = 16;
const OFSBODYLESSFUNCS: usize = 17;
const NUMBODYLESSFUNCS: usize = 18;
const OFS_TYPES: usize = 19;
const NUMTYPES: usize = 20;
const BLOCKSCOMPRESSED: usize = 21;
const SECONDARYVERSION: usize = 22;

fn fte16() -> Vec<u8> {
    vector(Vector::Fte16Minimal)
}

fn fte32() -> Vec<u8> {
    vector(Vector::Fte32Minimal)
}

/// `table` appended to `bytes`, returning its offset.
fn append(bytes: &mut Vec<u8>, table: &[u8]) -> i32 {
    let offset = i32::try_from(bytes.len()).unwrap();
    bytes.extend_from_slice(table);
    offset
}

// spec PROGS-19.1: the 16-bit vector's 92-byte header decodes to its README values.
// spec PROGS-21.6: the version 7 header is followed by the same six tables.
#[test]
fn fte16_header() {
    let bytes = fte16();
    let progs = Progs::read(&bytes).unwrap();
    assert_eq!(PROGS_FTE_HEADER_SIZE, 92);
    assert_eq!(PROGS_VERSION_FTE, 7);
    assert_eq!(progs.format(), Format::Fte16);
    assert_eq!(progs.header().version, 7);
    assert_eq!(progs.header().ofs_strings, 92);
    assert_eq!(progs.header().ofs_globals, 156);
    assert_eq!(
        progs.fte_header(),
        Some(&FteHeader {
            secondaryversion: PROGS_FTE_SECONDARY16,
            ..FteHeader::default()
        })
    );
    assert_eq!(progs.globals().len(), 28);
    assert_eq!(progs.line_numbers().map(|table| table.len()), None);
    assert_eq!(progs.bodyless_functions().count(), 0);
    let v6 = vector(Vector::Minimal);
    assert_eq!(Progs::read(&v6).unwrap().fte_header(), None);
}

// spec PROGS-19.2: secondaryversion is "1FTE" ^ "PROG" for 16-bit and "1FTE" ^ "32B " for
// 32-bit, as little-endian i32.
#[test]
fn secondary_versions() {
    let tag = |text: &[u8; 4]| i32::from_le_bytes(*text);
    assert_eq!(tag(b"1FTE") ^ tag(b"PROG"), PROGS_FTE_SECONDARY16);
    assert_eq!(tag(b"1FTE") ^ tag(b"32B "), PROGS_FTE_SECONDARY32);
    assert_eq!(PROGS_FTE_SECONDARY16, 0x021B_1461);
    assert_eq!(PROGS_FTE_SECONDARY32, 0x6516_7402);
    assert_eq!(Progs::read(&fte32()).unwrap().format(), Format::Fte32);
}

// spec PROGS-19.3: any other secondaryversion, such as "UH27" or "KKQW", is unsupported.
#[test]
fn other_layouts_are_unsupported() {
    for tag in [*b"UH27", *b"KKQW", [0; 4]] {
        let mut bytes = fte16();
        let secondaryversion = i32::from_le_bytes(tag);
        patch(&mut bytes, SECONDARYVERSION, secondaryversion);
        let error = Progs::read(&bytes).unwrap_err();
        assert_eq!(error, ReadError::UnsupportedFteLayout { secondaryversion });
        assert!(
            error
                .to_string()
                .contains("is not FTE's 16-bit or 32-bit layout")
        );
    }
}

// spec PROGS-19.4: version 7 offsets and counts are bounded by the file like version 6.
#[test]
fn version_7_bounds() {
    let mut bytes = fte16();
    patch(&mut bytes, field::NUMGLOBALS, 29);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::Globals
        }
    );
    let mut bytes = fte32();
    patch(&mut bytes, field::OFS_STATEMENTS, -4);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::Statements
        }
    );
}

// spec PROGS-19.5: in the 16-bit version 7 layout operands are unsigned: ff ff is 65535.
#[test]
fn fte16_operands_are_unsigned() {
    let mut bytes = fte16();
    let ofs = usize::try_from(word(&bytes, field::OFS_STATEMENTS)).unwrap();
    bytes[ofs..ofs + 8].copy_from_slice(&[0x3d, 0, 0xff, 0xff, 0, 0, 0, 0]);
    let progs = Progs::read(&bytes).unwrap();
    assert_eq!(progs.statements().get(0).unwrap().a, 65535);
    assert_eq!(write(&progs.to_file()).unwrap(), bytes);
}

// spec PROGS-20.1: a 32-bit statement is four u32 at 0, 4, 8 and 12.
// spec PROGS-20.2: a 32-bit definition is type u32, ofs u32 and s_name i32; DEF_SAVEGLOBAL is
// still bit 15.
#[test]
fn fte32_records() {
    assert_eq!(PROGS_FTE32_STATEMENT_SIZE, 16);
    assert_eq!(PROGS_FTE32_DEF_SIZE, 12);
    let mut bytes = fte32();
    let ofs = usize::try_from(word(&bytes, field::OFS_STATEMENTS)).unwrap();
    let mut raw = Vec::new();
    for value in [70_000_u32, 0xFFFF_FFFF, 100_000, 3] {
        raw.extend(value.to_le_bytes());
    }
    bytes[ofs..ofs + 16].copy_from_slice(&raw);
    let ofs = usize::try_from(word(&bytes, field::OFS_GLOBALDEFS)).unwrap();
    let mut raw = Vec::new();
    for value in [0x0001_8002_u32, 70_000, 0] {
        raw.extend(value.to_le_bytes());
    }
    bytes[ofs..ofs + 12].copy_from_slice(&raw);
    let progs = Progs::read(&bytes).unwrap();
    assert_eq!(
        progs.statements().get(0),
        Some(Statement {
            op: 70_000,
            a: -1,
            b: 100_000,
            c: 3
        })
    );
    let def = progs.global_defs().get(0).unwrap();
    assert_eq!(
        def,
        Def {
            type_bits: 0x0001_8002,
            ofs: 70_000,
            s_name: 0
        }
    );
    assert!(def.is_save_global());
    assert_eq!(def.def_type().number(), 2);
    assert_eq!(write(&progs.to_file()).unwrap(), bytes);
}

// spec PROGS-20.3: functions, strings and globals keep the version 6 layout in the 32-bit
// layout.
#[test]
fn fte32_other_tables() {
    let bytes = fte32();
    let progs = Progs::read(&bytes).unwrap();
    let minimal = vector(Vector::Minimal);
    let v6 = Progs::read(&minimal).unwrap();
    assert_eq!(progs.functions().as_bytes(), v6.functions().as_bytes());
    assert_eq!(progs.strings(), v6.strings());
    assert_eq!(progs.globals().as_bytes(), v6.globals().as_bytes());
}

// spec PROGS-21.1: ofslinenums locates numstatements i32 line numbers.
// spec PROGS-21.2: the bodyless function table is numbodylessfuncs NUL-terminated names.
#[test]
fn line_numbers_and_bodyless_functions() {
    let mut bytes = fte16();
    let lines = append(&mut bytes, &42_i32.to_le_bytes());
    let names = append(&mut bytes, b"spawn\0\0remove\0");
    patch(&mut bytes, OFSLINENUMS, lines);
    patch(&mut bytes, OFSBODYLESSFUNCS, names);
    patch(&mut bytes, NUMBODYLESSFUNCS, 3);
    let progs = Progs::read(&bytes).unwrap();
    let table = progs.line_numbers().unwrap();
    assert_eq!(table.iter().collect::<Vec<_>>(), [42]);
    let bodyless: Vec<&[u8]> = progs.bodyless_functions().collect();
    assert_eq!(bodyless, [&b"spawn"[..], b"", b"remove"]);
    assert_eq!(write(&progs.to_file()).unwrap(), bytes);
}

// spec PROGS-21.3: bit n of blockscompressed marks table n compressed.
// spec PROGS-21.4: a file with any compressed table is rejected.
#[test]
fn compressed_tables_are_rejected() {
    for bit in [0, 4, 7, 31] {
        let mut bytes = fte16();
        let mask = 1_u32 << bit;
        patch(
            &mut bytes,
            BLOCKSCOMPRESSED,
            i32::from_le_bytes(mask.to_le_bytes()),
        );
        let error = Progs::read(&bytes).unwrap_err();
        assert_eq!(
            error,
            ReadError::Compressed {
                blockscompressed: mask
            }
        );
        assert!(error.to_string().contains("compressed tables"));
    }
}

// spec PROGS-21.5: ofsfiles and the type table are kept as stored and not decoded.
// spec PROGS-21.7: the embedded file records at ofsfiles are not read.
// spec PROGS-21.8: the type records at ofs_types are not read.
#[test]
fn opaque_tables_are_kept() {
    let mut bytes = fte16();
    let blob = append(&mut bytes, &[0x5A; 40]);
    patch(&mut bytes, OFSFILES, blob);
    patch(&mut bytes, OFS_TYPES, i32::MAX);
    patch(&mut bytes, NUMTYPES, 1000);
    let progs = Progs::read(&bytes).unwrap();
    let fte = progs.fte_header().unwrap();
    assert_eq!(fte.ofsfiles, u32::try_from(blob).unwrap());
    assert_eq!(fte.ofs_types, 0x7FFF_FFFF);
    assert_eq!(fte.numtypes, 1000);
    let file = progs.to_file();
    assert_eq!(write(&file).unwrap(), bytes);
    let without_layout = ProgsFile {
        layout: None,
        ..file
    };
    let error = write(&without_layout).unwrap_err();
    assert_eq!(error, WriteError::OpaqueTableWithoutLayout);
    assert!(error.to_string().contains("ofsfiles or ofs_types"));
    for (ofsfiles, ofs_types) in [(0, 5), (5, 0)] {
        let file = ProgsFile {
            format: Format::Fte16,
            fte: FteParts {
                ofsfiles,
                ofs_types,
                ..FteParts::default()
            },
            ..ProgsFile::default()
        };
        assert_eq!(
            write(&file).unwrap_err(),
            WriteError::OpaqueTableWithoutLayout
        );
    }
}

// spec PROGS-21.9: a version 7 file of exactly 92 bytes is long enough.
#[test]
fn header_only_version_7() {
    let mut bytes = fte16()[..92].to_vec();
    for (ofs, num) in [
        (field::OFS_STATEMENTS, field::NUMSTATEMENTS),
        (field::OFS_GLOBALDEFS, field::NUMGLOBALDEFS),
        (field::OFS_FIELDDEFS, field::NUMFIELDDEFS),
        (field::OFS_FUNCTIONS, field::NUMFUNCTIONS),
        (field::OFS_STRINGS, field::NUMSTRINGS),
        (field::OFS_GLOBALS, field::NUMGLOBALS),
    ] {
        patch(&mut bytes, ofs, 92);
        patch(&mut bytes, num, 0);
    }
    let progs = Progs::read(&bytes).unwrap();
    assert_eq!(progs.format(), Format::Fte16);
    assert_eq!(write(&progs.to_file()).unwrap(), bytes);
}

// spec PROGS-21.6: PROGS-15 applies to version 7 tables; opcodes above 65 still load.
#[test]
fn version_7_tables_are_checked() {
    let mut bytes = fte16();
    let ofs = usize::try_from(word(&bytes, field::OFS_GLOBALDEFS)).unwrap();
    bytes[ofs + 4] = 9;
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::StringOutOfRange {
            site: StringSite::GlobalDefName(0),
            offset: 9
        }
    );
    let mut bytes = fte16();
    let ofs = usize::try_from(word(&bytes, field::OFS_STATEMENTS)).unwrap();
    bytes[ofs] = 200;
    assert_eq!(
        Progs::read(&bytes).unwrap().statements().get(0).unwrap().op,
        200
    );
}

// spec PROGS-3.3: with FTE reading on, version 7 is accepted, so a version 7 file with a bad
// CRC reports the CRC instead of the version (a deliberate divergence behind the flag).
// spec PROGS-21.9: version 7 checks run size 60, version, size 92, CRC, secondaryversion,
// blockscompressed, the six tables, line numbers, bodyless names, then the rest.
#[test]
fn version_7_check_order() {
    let base = fte16();
    let mut short = base[..80].to_vec();
    patch(&mut short, field::CRC, 1);
    assert_eq!(
        Progs::read(&short).unwrap_err(),
        ReadError::TooShort {
            len: 80,
            needed: 92
        }
    );

    let mut bytes = base.clone();
    patch(&mut bytes, field::CRC, 0);
    patch(&mut bytes, SECONDARYVERSION, 1);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::HeaderCrc { crc: 0 }
    );
    patch(&mut bytes, field::CRC, 5927);
    patch(&mut bytes, BLOCKSCOMPRESSED, 1);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::UnsupportedFteLayout {
            secondaryversion: 1
        }
    );
    patch(&mut bytes, SECONDARYVERSION, PROGS_FTE_SECONDARY16);
    patch(&mut bytes, field::NUMGLOBALS, -1);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::Compressed {
            blockscompressed: 1
        }
    );
    patch(&mut bytes, BLOCKSCOMPRESSED, 0);
    patch(&mut bytes, OFSLINENUMS, 100_000);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::Globals
        }
    );
    patch(&mut bytes, field::NUMGLOBALS, 28);
    patch(&mut bytes, OFSBODYLESSFUNCS, 100_000);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::LineNumbers
        }
    );
    patch(&mut bytes, OFSLINENUMS, 0);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::BodylessFunctions
        }
    );

    // Names whose last NUL is missing, and a count past the names there are.
    let mut bytes = base.clone();
    let names = append(&mut bytes, b"one\0two");
    patch(&mut bytes, OFSBODYLESSFUNCS, names);
    patch(&mut bytes, NUMBODYLESSFUNCS, 2);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::BodylessFunctions
        }
    );
    patch(&mut bytes, NUMBODYLESSFUNCS, -1);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::BodylessFunctions
        }
    );
    patch(&mut bytes, NUMBODYLESSFUNCS, 1);
    assert!(Progs::read(&bytes).is_ok());

    // Line numbers past the end of the file, and a line number offset above i32::MAX.
    let mut bytes = base;
    let len = i32::try_from(bytes.len()).unwrap();
    patch(&mut bytes, OFSLINENUMS, len - 3);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::LineNumbers
        }
    );
    patch(&mut bytes, OFSLINENUMS, -1);
    assert_eq!(
        Progs::read(&bytes).unwrap_err(),
        ReadError::TableOutOfBounds {
            table: TableId::LineNumbers
        }
    );
}

// spec PROGS-22.2: without a layout, version 7 writes line numbers then bodyless names after
// the globals; no line numbers gives ofslinenums 0 and no names gives ofsbodylessfuncs 0.
// spec PROGS-22.3: secondaryversion comes from the format and blockscompressed is 0.
#[test]
fn version_7_qcc_order() {
    let file = ProgsFile {
        format: Format::Fte32,
        strings: vec![0],
        statements: vec![Statement::default(); 2],
        fte: FteParts {
            line_numbers: Some(vec![7, 8]),
            bodyless_functions: vec![b"spawn".to_vec()],
            ..FteParts::default()
        },
        ..ProgsFile::default()
    };
    let bytes = write(&file).unwrap();
    assert_eq!(word(&bytes, field::VERSION), 7);
    assert_eq!(word(&bytes, SECONDARYVERSION), PROGS_FTE_SECONDARY32);
    assert_eq!(word(&bytes, BLOCKSCOMPRESSED), 0);
    assert_eq!(word(&bytes, field::OFS_STRINGS), 92);
    assert_eq!(word(&bytes, field::OFS_STATEMENTS), 96);
    assert_eq!(word(&bytes, OFSLINENUMS), 128);
    assert_eq!(word(&bytes, OFSBODYLESSFUNCS), 136);
    assert_eq!(word(&bytes, NUMBODYLESSFUNCS), 1);
    assert_eq!(&bytes[136..], b"spawn\0");
    let reread = Progs::read(&bytes).unwrap().to_file();
    assert_eq!(reread.fte, file.fte);

    let bare = ProgsFile {
        format: Format::Fte16,
        ..ProgsFile::default()
    };
    let bytes = write(&bare).unwrap();
    assert_eq!(bytes.len(), 92);
    assert_eq!(word(&bytes, OFSLINENUMS), 0);
    assert_eq!(word(&bytes, OFSBODYLESSFUNCS), 0);
    assert_eq!(word(&bytes, SECONDARYVERSION), PROGS_FTE_SECONDARY16);
}

// spec PROGS-22.3: with a layout, a line number table the source did not have goes at the end.
#[test]
fn added_line_numbers_go_at_the_end() {
    let bytes = fte16();
    let mut file = Progs::read(&bytes).unwrap().to_file();
    file.fte.line_numbers = Some(vec![3]);
    let written = write(&file).unwrap();
    assert_eq!(&written[92..bytes.len()], &bytes[92..]);
    assert_eq!(
        word(&written, OFSLINENUMS),
        i32::try_from(bytes.len()).unwrap()
    );
    assert_eq!(&written[bytes.len()..], 3_i32.to_le_bytes());
}

// spec PROGS-22.4: the version 7 writer errors: 16-bit operands outside 0..65535, a line
// number count that is not the statement count, a NUL in a bodyless name, and FTE parts in
// a version 6 file.
#[test]
fn version_7_write_errors() {
    for statement in [
        Statement {
            a: -1,
            ..Statement::default()
        },
        Statement {
            c: 65536,
            ..Statement::default()
        },
    ] {
        let file = ProgsFile {
            format: Format::Fte16,
            statements: vec![statement],
            ..ProgsFile::default()
        };
        assert_eq!(
            write(&file).unwrap_err(),
            WriteError::StatementOutOfRange { index: 0 }
        );
    }
    let file = ProgsFile {
        format: Format::Fte16,
        global_defs: vec![Def {
            ofs: 65536,
            ..Def::default()
        }],
        ..ProgsFile::default()
    };
    assert_eq!(
        write(&file).unwrap_err(),
        WriteError::DefOutOfRange {
            table: TableId::GlobalDefs,
            index: 0
        }
    );

    let lines = ProgsFile {
        format: Format::Fte16,
        fte: FteParts {
            line_numbers: Some(vec![1]),
            ..FteParts::default()
        },
        ..ProgsFile::default()
    };
    let error = write(&lines).unwrap_err();
    assert_eq!(
        error,
        WriteError::LineNumberCount {
            line_numbers: 1,
            statements: 0
        }
    );
    assert_eq!(error.to_string(), "1 line numbers given for 0 statements");

    let names = ProgsFile {
        format: Format::Fte32,
        fte: FteParts {
            bodyless_functions: vec![b"ok".to_vec(), b"b\0d".to_vec()],
            ..FteParts::default()
        },
        ..ProgsFile::default()
    };
    let error = write(&names).unwrap_err();
    assert_eq!(error, WriteError::NulInBodylessName { index: 1 });
    assert_eq!(
        error.to_string(),
        "bodyless function name 1 contains a NUL byte"
    );

    let v6 = ProgsFile {
        fte: FteParts {
            ofsfiles: 4,
            ..FteParts::default()
        },
        ..ProgsFile::default()
    };
    let error = write(&v6).unwrap_err();
    assert_eq!(error, WriteError::FtePartsInV6);
    assert_eq!(
        error.to_string(),
        "a version 6 progs.dat cannot hold FTE tables"
    );
}
