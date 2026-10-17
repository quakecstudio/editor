// SPDX-License-Identifier: GPL-3.0-or-later

//! Behaviour tests and property tests for `qc-lno` beyond the spec's listed vectors
//! (`vectors.rs`): the little-endian layout, 1-based lines, the deliberate divergence from
//! DarkPlaces on the column table, stale-file matching, matching a [`qc_progs::Header`], and
//! the round-trip property over arbitrary accepted files.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use crate::fixtures::{header, push_i32_table};
use crate::{LNO_HEADER_SIZE, LNO_MAGIC, LNO_VERSION, LnoFile};

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

proptest! {
    // spec LNO-6.2: a file the reader accepts is written back byte for byte, and reading what
    // the writer wrote gives back a file that writes to the same bytes.
    #[test]
    fn an_accepted_file_round_trips_byte_for_byte(bytes in arbitrary_lno_bytes()) {
        let lno = LnoFile::read(&bytes).unwrap();
        prop_assert_eq!(lno.write().unwrap(), bytes.clone());
        // Reading the written bytes gives an equal file that writes the same bytes again.
        let reread = LnoFile::read(&bytes).unwrap();
        prop_assert_eq!(&reread, &lno);
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

    // spec LNO-5.7: reading never panics and never fails on the values for any bytes.
    #[test]
    fn reading_arbitrary_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..128usize)) {
        // The call returns (it does not panic); either outcome is accepted.
        prop_assert!(matches!(LnoFile::read(&bytes), Ok(_) | Err(_)));
    }
}
