// SPDX-License-Identifier: GPL-3.0-or-later

//! The test vectors of `docs/spec/lno.md` ("Test vectors"), each checked against the reader,
//! the matcher and the writer, and citing the statements it pins.
//!
//! The vectors are built from the spec's `H(n)` header (the ordinary counts `numglobaldefs` 3,
//! `numglobals` 30, `numfielddefs` 1) and the hex bytes the spec gives. Every `.lno` the
//! reader accepts is written back byte for byte (LNO-6.2).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use crate::fixtures::{LnoSpec, header, header_with, push_i32_table};
use crate::{CountField, LnoFile, ReadError, WriteError};

/// `H(2) 00000000 05000000` (32 bytes, FTEQCC layout): lines `[0, 5]`, no columns.
fn vector_fteqcc_two() -> Vec<u8> {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[0, 5]);
    bytes
}

/// `H(2) 00000000 05000000 00000000 04000000` (40 bytes, GMQCC layout): lines `[0, 5]`,
/// columns `[0, 4]`.
fn vector_gmqcc_two() -> Vec<u8> {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[0, 5]);
    push_i32_table(&mut bytes, &[0, 4]);
    bytes
}

// spec LNO-2.1: a .lno starts with the 24-byte LNOF header and five 32-bit integers.
// spec LNO-3.1: the line table is numstatements i32 values, one per statement.
// spec LNO-3.3: FTEQCC writes no column table, so a 24 + 4n byte file has no columns.
// spec LNO-5.4: a file of 24 + 4n bytes (below 24 + 8n) has a line table and no column table.
#[test]
fn reads_the_fteqcc_layout_vector() {
    let lno = LnoFile::read(&vector_fteqcc_two()).unwrap();
    assert_eq!(lno.lines, [0, 5]);
    assert_eq!(lno.columns, None);
    assert_eq!(lno.trailing, Vec::<u8>::new());
    assert_eq!(lno.numglobaldefs, 3);
    assert_eq!(lno.numglobals, 30);
    assert_eq!(lno.numfielddefs, 1);
}

// spec LNO-3.3: GMQCC writes a column table straight after the line table.
// spec LNO-3.4: a column is GMQCC's own measure, kept as written and not converted.
// spec LNO-5.4: a file of at least 24 + 8n bytes, n above 0, has a column table of n entries.
#[test]
fn reads_the_gmqcc_layout_vector() {
    let lno = LnoFile::read(&vector_gmqcc_two()).unwrap();
    assert_eq!(lno.lines, [0, 5]);
    assert_eq!(lno.columns, Some(vec![0, 4]));
    assert_eq!(lno.trailing, Vec::<u8>::new());
}

// spec LNO-5.4: H(0) (24 bytes) has no line table and no column table.
#[test]
fn reads_an_empty_table() {
    let lno = LnoFile::read(&header(0)).unwrap();
    assert_eq!(lno.lines, Vec::<i32>::new());
    assert_eq!(lno.columns, None);
    assert_eq!(lno.trailing, Vec::<u8>::new());
}

// spec LNO-5.4: 24 + 4n bytes plus 4 trailing (below 24 + 8n) is a line table and trailing.
// spec LNO-5.5: bytes after the last table are kept as trailing bytes.
#[test]
fn reads_fteqcc_layout_with_trailing_bytes() {
    let mut bytes = vector_fteqcc_two();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.lines, [0, 5]);
    assert_eq!(lno.columns, None);
    assert_eq!(lno.trailing, [0xaa, 0xbb, 0xcc, 0xdd]);
}

// spec LNO-5.4: 24 + 8n bytes plus trailing is a line table, a column table and trailing.
// spec LNO-5.5: bytes after the last table are kept as trailing bytes.
#[test]
fn reads_gmqcc_layout_with_trailing_bytes() {
    let mut bytes = vector_gmqcc_two();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.lines, [0, 5]);
    assert_eq!(lno.columns, Some(vec![0, 4]));
    assert_eq!(lno.trailing, [0xaa, 0xbb, 0xcc, 0xdd]);
}

// spec LNO-3.5: line values are signed and kept unchanged; -1 is 0xffffffff.
// spec LNO-5.7: reading never fails on the values.
#[test]
fn reads_a_negative_line_value() {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[-1, 5]);
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.lines, [-1, 5]);
}

// spec LNO-5.1: a file shorter than 24 bytes is rejected with a typed error.
#[test]
fn rejects_a_file_shorter_than_the_header() {
    let short = &header(0)[..23];
    assert_eq!(LnoFile::read(short), Err(ReadError::TooShort(23)));
}

// spec LNO-5.2: a file whose first four bytes are not exactly LNOF is rejected.
#[test]
fn rejects_a_bad_magic() {
    let mut bytes = header(0);
    bytes[3] = b'f'; // "LNOf"
    assert_eq!(LnoFile::read(&bytes), Err(ReadError::BadMagic(*b"LNOf")));
}

// spec LNO-2.2: the only version any reader accepts is 1; there is no version 2.
// spec LNO-5.3: a version other than 1 is rejected, carrying the version.
#[test]
fn rejects_an_unsupported_version() {
    let mut bytes = header(0);
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(LnoFile::read(&bytes), Err(ReadError::UnsupportedVersion(2)));
}

// spec LNO-5.4: a file shorter than 24 + 4n bytes has a truncated line table.
#[test]
fn rejects_a_truncated_line_table() {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[0]); // one line, not two: 28 bytes
    assert_eq!(
        LnoFile::read(&bytes),
        Err(ReadError::LineTableTruncated {
            numstatements: 2,
            len: 28,
        })
    );
}

// spec LNO-5.4: numstatements is computed without overflow; H(0xffffffff) is 24 bytes.
#[test]
fn rejects_a_huge_count_without_overflow() {
    let bytes = header(0xffff_ffff);
    assert_eq!(bytes.len(), 24);
    assert_eq!(
        LnoFile::read(&bytes),
        Err(ReadError::LineTableTruncated {
            numstatements: 0xffff_ffff,
            len: 24,
        })
    );
}

// spec LNO-5.6: the checks run in order, and the first failure is reported: a bad magic is
// reported before the version, even when the version is also wrong.
#[test]
fn reports_the_first_failure_in_order() {
    // "LNOG" then 20 bytes with version 2.
    let mut bytes = header(0);
    bytes[3] = b'G';
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(LnoFile::read(&bytes), Err(ReadError::BadMagic(*b"LNOG")));
}

// spec LNO-4.1: a .lno matches a progs when all four counts equal the progs' header fields.
#[test]
fn matches_a_progs_with_equal_counts() {
    let lno = LnoFile::read(&vector_fteqcc_two()).unwrap();
    assert_eq!(lno.matches(3, 30, 1, 2), Ok(()));
}

// spec LNO-4.1: the counts are compared one by one.
// spec LNO-4.2: a mismatch names the field that differs.
#[test]
fn reports_a_numglobals_mismatch() {
    let mut bytes = vector_fteqcc_two();
    bytes[12..16].copy_from_slice(&0x1fu32.to_le_bytes()); // numglobals 31
    let lno = LnoFile::read(&bytes).unwrap();
    let err = lno.matches(3, 30, 1, 2).unwrap_err();
    assert_eq!(err.field, CountField::Globals);
    assert_eq!(err.lno, 31);
    assert_eq!(err.progs, 30);
}

// spec LNO-4.2: the mismatch reports the first field in header order; numglobaldefs comes
// before numstatements, so a file differing on both reports numglobaldefs.
#[test]
fn reports_the_first_field_in_header_order() {
    // numglobaldefs 4, numstatements 3 with a third line.
    let mut bytes = header_with(4, 30, 1, 3);
    push_i32_table(&mut bytes, &[0, 5, 7]);
    let lno = LnoFile::read(&bytes).unwrap();
    let err = lno.matches(3, 30, 1, 2).unwrap_err();
    assert_eq!(err.field, CountField::GlobalDefs);
    assert_eq!(err.lno, 4);
    assert_eq!(err.progs, 3);
}

// spec LNO-6.1: the writer produces the 32-byte FTEQCC-layout vector for lines [0, 5], no
// columns, and the ordinary counts.
#[test]
fn writes_the_fteqcc_layout_vector() {
    // The ordinary case (LnoSpec::default): the vector counts and lines [0, 5], no columns.
    let lno = LnoFile::from(LnoSpec::default());
    assert_eq!(lno.write(), Ok(vector_fteqcc_two()));
}

// spec LNO-6.1: the writer produces the 40-byte GMQCC-layout vector for lines [0, 5] and
// columns [0, 4].
#[test]
fn writes_the_gmqcc_layout_vector() {
    let lno = LnoFile::from(LnoSpec {
        columns: Some(vec![0, 4]),
        ..LnoSpec::default()
    });
    assert_eq!(lno.write(), Ok(vector_gmqcc_two()));
}

// spec LNO-6.3: the writer rejects a column table whose length differs from the line table's.
#[test]
fn rejects_a_mismatched_column_table() {
    let lno = LnoFile {
        lines: vec![0, 5],
        columns: Some(vec![0]),
        ..LnoFile::default()
    };
    assert_eq!(
        lno.write(),
        Err(WriteError::ColumnCountMismatch {
            lines: 2,
            columns: 1,
        })
    );
}

// spec LNO-6.3: the writer rejects a column table when the line table is empty.
#[test]
fn rejects_columns_without_lines() {
    let lno = LnoFile {
        lines: Vec::new(),
        columns: Some(Vec::new()),
        ..LnoFile::default()
    };
    assert_eq!(lno.write(), Err(WriteError::ColumnsWithoutLines));
}

/// Every `.lno` the reader accepts in the vectors above.
fn accepted_vectors() -> Vec<Vec<u8>> {
    let with_trailing = |mut bytes: Vec<u8>| {
        bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        bytes
    };
    let mut negative = header(2);
    push_i32_table(&mut negative, &[-1, 5]);
    vec![
        vector_fteqcc_two(),
        vector_gmqcc_two(),
        header(0),
        with_trailing(vector_fteqcc_two()),
        with_trailing(vector_gmqcc_two()),
        negative,
    ]
}

// spec LNO-6.2: a file the reader accepts is written back byte for byte, and reading what the
// writer wrote gives back the counts, tables and trailing bytes.
#[test]
fn every_accepted_vector_round_trips_byte_for_byte() {
    for bytes in accepted_vectors() {
        let lno = LnoFile::read(&bytes).unwrap();
        let written = lno.write().unwrap();
        assert_eq!(written, bytes, "round trip differs for {bytes:?}");
        assert_eq!(LnoFile::read(&written).unwrap(), lno);
    }
}
