// SPDX-License-Identifier: GPL-3.0-or-later

//! Behaviour tests and property tests for `qc-lno` beyond the spec's listed vectors
//! (`vectors.rs`): the little-endian layout, 1-based lines, the deliberate divergence from
//! DarkPlaces on the column table, stale-file matching, matching a [`qc_progs::Header`], and
//! the round-trip property over arbitrary accepted files.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use proptest::prelude::*;

use crate::fixtures::{header, push_i32_table};
use crate::{LNO_HEADER_SIZE, LNO_MAGIC, LNO_VERSION, LnoFile, ReadError};

// spec LNO-2.3: the four counts are copied from the progs; the reader keeps them and the
// writer writes them back as given.
#[test]
fn the_counts_survive_a_round_trip() {
    let lno = LnoFile {
        numglobaldefs: 7,
        numglobals: 99,
        numfielddefs: 2,
        lines: vec![1, 2, 3],
        columns: None,
        trailing: Vec::new(),
    };
    let bytes = lno.write().unwrap();
    // Header counts are at offsets 8, 12, 16 as little-endian u32.
    assert_eq!(&bytes[8..12], &7u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &99u32.to_le_bytes());
    assert_eq!(&bytes[16..20], &2u32.to_le_bytes());
    assert_eq!(&bytes[20..24], &3u32.to_le_bytes());
    assert_eq!(LnoFile::read(&bytes).unwrap(), lno);
}

// spec LNO-2.4: this spec reads and writes little-endian only; the magic and every count and
// line is little-endian, as FTEQCC and GMQCC produce on their x86 and ARM hosts.
#[test]
fn the_layout_is_little_endian() {
    let mut bytes = header(1);
    push_i32_table(&mut bytes, &[0x0403_0201]);
    assert_eq!(&bytes[0..4], &LNO_MAGIC);
    assert_eq!(&bytes[4..8], &LNO_VERSION.to_le_bytes());
    let lno = LnoFile::read(&bytes).unwrap();
    // The line bytes 01 02 03 04 decode little-endian to 0x04030201.
    assert_eq!(lno.lines, [0x0403_0201]);
    assert_eq!(&bytes[LNO_HEADER_SIZE..LNO_HEADER_SIZE + 4], &[1, 2, 3, 4]);
}

// spec LNO-3.2: a line number is 1-based, and the value 0 means the writer had no line, as for
// statement 0 and global initialiser statements.
#[test]
fn line_zero_means_no_line_and_lines_are_1_based() {
    let mut bytes = header(3);
    push_i32_table(&mut bytes, &[0, 1, 42]);
    let lno = LnoFile::read(&bytes).unwrap();
    // Statement 0 carries line 0 (no line); later statements carry 1-based source lines.
    assert_eq!(lno.lines[0], 0);
    assert_eq!(lno.lines[1], 1);
    assert_eq!(lno.lines[2], 42);
}

// spec LNO-3.6: this spec deliberately differs from DarkPlaces: a file of at least 24 + 8n
// bytes has a column table, which is the layout GMQCC writes, even though DarkPlaces ignores a
// column table exactly that long.
#[test]
fn a_gmqcc_length_file_is_read_with_its_columns() {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[0, 5]);
    push_i32_table(&mut bytes, &[0, 4]);
    assert_eq!(bytes.len(), LNO_HEADER_SIZE + 8 * 2);
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.columns, Some(vec![0, 4]));
}

// spec LNO-3.7: no writer puts anything after the tables; a file with trailing bytes keeps
// them, and the writer reproduces the file without adding or dropping any.
#[test]
fn trailing_bytes_are_preserved_not_invented() {
    let mut bytes = header(1);
    push_i32_table(&mut bytes, &[5]);
    // A file with no trailing bytes keeps none and writes none back.
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.trailing, Vec::<u8>::new());
    assert_eq!(lno.write().unwrap(), bytes);
}

// spec LNO-4.3: a stale .lno whose counts happen to equal the new progs' passes LNO-4.1; the
// format cannot tell it apart, so matching succeeds even when the lines are wrong.
#[test]
fn a_stale_lno_with_equal_counts_still_matches() {
    let mut bytes = header(2);
    // Lines that would be wrong for the new progs, but the counts still match.
    push_i32_table(&mut bytes, &[999, 1000]);
    let lno = LnoFile::read(&bytes).unwrap();
    assert_eq!(lno.matches(3, 30, 1, 2), Ok(()));
}

// spec LNO-4.1: matching a qc_progs::Header compares the four counts as unsigned 32-bit.
#[test]
fn matches_a_qc_progs_header() {
    let mut bytes = header(2);
    push_i32_table(&mut bytes, &[0, 5]);
    let lno = LnoFile::read(&bytes).unwrap();
    let header = qc_progs::Header {
        numglobaldefs: 3,
        numglobals: 30,
        numfielddefs: 1,
        numstatements: 2,
        ..qc_progs::Header::default()
    };
    assert_eq!(lno.matches_header(&header), Ok(()));
}

/// An arbitrary unambiguously accepted `.lno`: ordinary-sized tables, with or without columns,
/// with or without trailing bytes. A no-column file keeps fewer than `4n` trailing bytes so it
/// cannot be mistaken for a `24 + 8n` byte column file (LNO-5.4); that ambiguity is the one
/// case where `read` and `write` are inverse only at the byte level (LNO-6.2), covered by
/// [`an_accepted_file_round_trips_byte_for_byte`].
fn arbitrary_lno() -> impl Strategy<Value = LnoFile> {
    let counts = (any::<u32>(), any::<u32>(), any::<u32>());
    let lines = prop::collection::vec(any::<i32>(), 0..32usize);
    (
        counts,
        lines,
        any::<bool>(),
        prop::collection::vec(any::<u8>(), 0..8usize),
    )
        .prop_map(
            |((numglobaldefs, numglobals, numfielddefs), lines, with_columns, trailing)| {
                // A column table is only valid for a non-empty line table (LNO-6.3).
                let columns: Option<Vec<i32>> = (with_columns && !lines.is_empty())
                    .then(|| lines.iter().map(|line| line.wrapping_mul(2)).collect());
                // For a no-column file with n > 0, trailing of 4n bytes or more reads back as a
                // column table (LNO-5.4); keep it shorter so the struct round-trips exactly.
                let trailing = if columns.is_none() && !lines.is_empty() {
                    let max_keep = lines
                        .len()
                        .checked_mul(4)
                        .and_then(|limit| limit.checked_sub(1))
                        .unwrap_or(0);
                    let keep = trailing.len().min(max_keep);
                    trailing.get(..keep).unwrap_or_default().to_vec()
                } else {
                    trailing
                };
                LnoFile {
                    numglobaldefs,
                    numglobals,
                    numfielddefs,
                    lines,
                    columns,
                    trailing,
                }
            },
        )
}

/// An arbitrary accepted `.lno` built from bytes: an ordinary-sized header, line table and,
/// for a long-enough file, a column table, with or without trailing bytes. Generating bytes
/// (rather than an [`LnoFile`]) and keeping only those the reader accepts matches LNO-6.2,
/// which is about the file the reader accepts being written back byte for byte: a FTEQCC-layout
/// file with exactly `4n` trailing bytes and a GMQCC-layout file are the same length (LNO-5.4),
/// so only the bytes, not a reconstructed struct, round-trip.
fn arbitrary_lno_bytes() -> impl Strategy<Value = Vec<u8>> {
    let counts = (any::<u32>(), any::<u32>(), any::<u32>());
    let lines = prop::collection::vec(any::<i32>(), 0..32usize);
    let extra = prop::collection::vec(any::<u8>(), 0..24usize);
    (counts, lines, extra).prop_map(|((ngd, ng, nfd), lines, extra)| {
        let numstatements = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&LNO_MAGIC);
        bytes.extend_from_slice(&LNO_VERSION.to_le_bytes());
        bytes.extend_from_slice(&ngd.to_le_bytes());
        bytes.extend_from_slice(&ng.to_le_bytes());
        bytes.extend_from_slice(&nfd.to_le_bytes());
        bytes.extend_from_slice(&numstatements.to_le_bytes());
        push_i32_table(&mut bytes, &lines);
        bytes.extend_from_slice(&extra);
        bytes
    })
}

/// Bytes that pass or just miss each check of LNO-5.6: the magic or four other bytes, version
/// 1 or another, any three counts, a small or any `numstatements`, and a tail of any bytes, cut
/// to any length so the header itself may be short.
fn near_lno_bytes() -> impl Strategy<Value = Vec<u8>> {
    let magic = prop_oneof![3 => Just(LNO_MAGIC), 1 => any::<[u8; 4]>()];
    let version = prop_oneof![3 => Just(LNO_VERSION), 1 => any::<u32>()];
    let numstatements = prop_oneof![3 => 0_u32..8, 1 => any::<u32>()];
    let tail = prop::collection::vec(any::<u8>(), 0..80usize);
    (
        magic,
        version,
        any::<[u32; 3]>(),
        numstatements,
        tail,
        any::<prop::sample::Index>(),
    )
        .prop_map(|(magic, version, counts, numstatements, tail, cut)| {
            let mut bytes = magic.to_vec();
            for word in [version, counts[0], counts[1], counts[2], numstatements] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            bytes.extend_from_slice(&tail);
            let keep = if cut.index(4) == 0 {
                cut.index(bytes.len() + 1)
            } else {
                bytes.len()
            };
            bytes.truncate(keep);
            bytes
        })
}

proptest! {
    // spec LNO-6.2: a file the reader accepts is written back byte for byte, and reading what
    // the writer wrote gives back a file that writes to the same bytes.
    #[test]
    fn an_accepted_file_round_trips_byte_for_byte(bytes in arbitrary_lno_bytes()) {
        let lno = LnoFile::read(&bytes).unwrap();
        let written = lno.write().unwrap();
        prop_assert_eq!(&written, &bytes);
        // Reading the written bytes gives an equal file.
        prop_assert_eq!(&LnoFile::read(&written).unwrap(), &lno);
    }

    // spec LNO-6.2: a file the writer produces reads back to the same counts, tables and
    // trailing bytes it was given (write then read is the identity on accepted structs).
    #[test]
    fn write_then_read_is_identity(lno in arbitrary_lno()) {
        let bytes = lno.write().unwrap();
        let read = LnoFile::read(&bytes).unwrap();
        prop_assert_eq!(&read, &lno);
        prop_assert_eq!(read.write().unwrap(), bytes);
    }

    // spec LNO-5.7: reading never panics and never fails on the values for any bytes: an
    // accepted file keeps the header's counts, its tables hold numstatements values each, and
    // header, tables and trailing bytes account for every byte; a refused one fails the first
    // check of LNO-5.6 that its bytes fail.
    #[test]
    fn reading_arbitrary_bytes_never_panics(
        bytes in prop_oneof![
            prop::collection::vec(any::<u8>(), 0..128usize),
            near_lno_bytes(),
        ]
    ) {
        let word = |at: usize| {
            bytes
                .get(at..at + 4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
        };
        match LnoFile::read(&bytes) {
            Ok(lno) => {
                prop_assert_eq!(bytes.get(..4), Some(LNO_MAGIC.as_slice()));
                prop_assert_eq!(word(4), Some(LNO_VERSION));
                prop_assert_eq!(
                    (Some(lno.numglobaldefs), Some(lno.numglobals), Some(lno.numfielddefs)),
                    (word(8), word(12), word(16))
                );
                prop_assert_eq!(Some(lno.numstatements()), word(20));
                let n = lno.lines.len();
                let columns = lno.columns.as_ref().map_or(0, Vec::len);
                prop_assert!(columns == 0 || columns == n);
                prop_assert_eq!(
                    LNO_HEADER_SIZE + 4 * n + 4 * columns + lno.trailing.len(),
                    bytes.len()
                );
            }
            Err(ReadError::TooShort(len)) => {
                prop_assert_eq!(len, bytes.len());
                prop_assert!(len < LNO_HEADER_SIZE);
            }
            Err(ReadError::BadMagic(magic)) => {
                prop_assert!(bytes.len() >= LNO_HEADER_SIZE);
                prop_assert_ne!(magic, LNO_MAGIC);
            }
            Err(ReadError::UnsupportedVersion(version)) => {
                prop_assert_eq!(bytes.get(..4), Some(LNO_MAGIC.as_slice()));
                prop_assert_eq!(Some(version), word(4));
                prop_assert_ne!(version, LNO_VERSION);
            }
            Err(ReadError::LineTableTruncated { numstatements, len }) => {
                prop_assert_eq!(Some(numstatements), word(20));
                prop_assert_eq!(len, bytes.len());
                prop_assert!(u64::from(numstatements) * 4 + 24 > len as u64);
            }
        }
    }
}

// Adversarial reviewer tests (P1-R0, TEST-9 and TEST-2): added by the independent readiness
// review of the L0 formats layer to kill the surviving mutants the Readiness mutation run
// found in qc-lno (76.3% caught before these). They pin the error Display strings (whose
// `fmt` bodies survived being replaced with `Ok(())`) and the mismatch path of
// `matches_header` (which survived being replaced with `Ok(())`).

// spec LNO-5.1, LNO-5.2, LNO-5.3, LNO-5.4: each ReadError variant prints its spec message,
// so its `fmt` body is not an empty string.
#[test]
fn reviewer_read_error_messages() {
    use crate::ReadError;
    assert_eq!(
        ReadError::TooShort(3).to_string(),
        "the .lno is 3 bytes, shorter than the 24-byte header"
    );
    assert_eq!(
        ReadError::BadMagic(*b"XXXX").to_string(),
        "the .lno does not start with LNOF: found [88, 88, 88, 88]"
    );
    assert_eq!(
        ReadError::UnsupportedVersion(7).to_string(),
        "the .lno has version 7, not 1"
    );
    assert_eq!(
        ReadError::LineTableTruncated {
            numstatements: 5,
            len: 30,
        }
        .to_string(),
        "the .lno is 30 bytes: too short for a 5-statement line table"
    );
}

// spec LNO-6.3: each WriteError variant prints the message the spec gives for that refusal.
#[test]
fn reviewer_write_error_messages() {
    use crate::WriteError;
    assert_eq!(
        WriteError::ColumnCountMismatch {
            lines: 2,
            columns: 3,
        }
        .to_string(),
        "the column table has 3 entries, the line table has 2"
    );
    assert_eq!(
        WriteError::ColumnsWithoutLines.to_string(),
        "a column table was given for an empty line table"
    );
    assert_eq!(
        WriteError::TooManyStatements(9).to_string(),
        "the line table has 9 entries, more than the 2147483647 a .lno can record"
    );
}

// spec LNO-4.2: a MatchError names the first differing field and both values, and the four
// CountField names print as the header fields they stand for.
#[test]
fn reviewer_match_error_messages() {
    use crate::{CountField, MatchError};
    assert_eq!(
        MatchError {
            field: CountField::Globals,
            lno: 99,
            progs: 100,
        }
        .to_string(),
        "the .lno's numglobals is 99, the progs.dat's is 100"
    );
    for (field, name) in [
        (CountField::GlobalDefs, "numglobaldefs"),
        (CountField::Globals, "numglobals"),
        (CountField::FieldDefs, "numfielddefs"),
        (CountField::Statements, "numstatements"),
    ] {
        assert_eq!(field.to_string(), name);
    }
}

// spec LNO-4.1, LNO-4.2: matches_header reports the first differing field, so a header that
// disagrees on a count is a MatchError, not Ok. This kills the mutant that made the whole
// function return Ok(()).
#[test]
fn reviewer_matches_header_reports_a_mismatch() {
    use crate::{CountField, MatchError};
    let lno = LnoFile {
        numglobaldefs: 3,
        numglobals: 30,
        numfielddefs: 1,
        lines: vec![0, 5],
        columns: None,
        trailing: Vec::new(),
    };
    // Equal counts match.
    let ok = qc_progs::Header {
        numglobaldefs: 3,
        numglobals: 30,
        numfielddefs: 1,
        numstatements: 2,
        ..qc_progs::Header::default()
    };
    assert_eq!(lno.matches_header(&ok), Ok(()));
    // A differing numglobals is reported, in header order, with both values (LNO-4.2).
    let bad = qc_progs::Header {
        numglobals: 31,
        ..ok
    };
    assert_eq!(
        lno.matches_header(&bad),
        Err(MatchError {
            field: CountField::Globals,
            lno: 30,
            progs: 31,
        })
    );
    // The first differing field wins: numglobaldefs before numglobals.
    let both = qc_progs::Header {
        numglobaldefs: 4,
        numglobals: 31,
        ..ok
    };
    assert_eq!(
        lno.matches_header(&both).unwrap_err().field,
        CountField::GlobalDefs
    );
}
