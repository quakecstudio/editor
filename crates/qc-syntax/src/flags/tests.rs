// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use qc_preproc::{CompilerFlags, FlagEdit, FteFlag, Priorities};

use super::{Block, Linkage, Reading, marks};

fn edit(set: &[(FteFlag, bool)]) -> FlagEdit {
    let flags = set
        .iter()
        .fold(CompilerFlags::default(), |flags, &(flag, on)| {
            flags.with(flag, on)
        });
    FlagEdit::of_flags(
        &set.iter().map(|&(flag, _)| flag).collect::<Vec<_>>(),
        flags,
    )
}

/// A block whose token after its last is the byte before `end`.
fn block(start: usize, end: usize, linkage: Linkage) -> Block {
    Block {
        start,
        lookahead: end.saturating_sub(1),
        end,
        linkage,
    }
}

/// The flags the lexer read with.
fn lexed(edits: &[(usize, FlagEdit)], blocks: &[Block]) -> Vec<(usize, CompilerFlags)> {
    marks(edits, blocks, Reading::Lexed)
}

// spec FTEF-21.1: a text with no change holds FTEQCC's defaults from byte 0
#[test]
fn no_change_holds_the_defaults() {
    assert_eq!(lexed(&[], &[]), vec![(0, CompilerFlags::default())]);
    let flags = CompilerFlags::default();
    assert!(flags.has(FteFlag::Qcfuncs));
    assert!(!flags.has(FteFlag::Cpriority));
    assert_eq!(flags.priorities(), Priorities::Qc);
}

// spec FTEF-21.2: `"C"` turns `qcfuncs` off and `cpriority`, `assumeint`, `assumevar` and
// `subscope` on, `assumedouble` left as it is; `"QC"` turns `qcfuncs` on; any other string sets
// nothing; each from the block's start to its end
#[test]
fn blocks_set_their_flags_where_they_hold() {
    let c = lexed(&[], &[block(10, 20, Linkage::C)]);
    let inside = CompilerFlags::at(&c, 10);
    assert!(!inside.has(FteFlag::Qcfuncs));
    for flag in [
        FteFlag::Cpriority,
        FteFlag::AssumeInteger,
        FteFlag::Assumevar,
        FteFlag::PrSubscopedlocals,
    ] {
        assert!(inside.has(flag), "{flag:?}");
    }
    assert!(!inside.has(FteFlag::AssumeDouble));
    assert_eq!(inside.priorities(), Priorities::Qc);
    assert_eq!(CompilerFlags::at(&c, 9), CompilerFlags::default());
    assert_eq!(CompilerFlags::at(&c, 19), inside);
    assert_eq!(CompilerFlags::at(&c, 20), CompilerFlags::default());
    let off = [(0, edit(&[(FteFlag::Qcfuncs, false)]))];
    let qc = lexed(&off, &[block(10, 20, Linkage::Qc)]);
    assert!(!CompilerFlags::at(&qc, 5).has(FteFlag::Qcfuncs));
    assert!(CompilerFlags::at(&qc, 10).has(FteFlag::Qcfuncs));
    assert!(!CompilerFlags::at(&qc, 20).has(FteFlag::Qcfuncs));
    let other = lexed(&off, &[block(10, 20, Linkage::Other)]);
    assert_eq!(CompilerFlags::at(&other, 15), CompilerFlags::at(&other, 5));
    assert_eq!(Linkage::of(b"c"), Linkage::C);
    assert_eq!(Linkage::of(b"qC"), Linkage::Qc);
    assert_eq!(Linkage::of(b"C++"), Linkage::Other);
    assert_eq!(Linkage::of(b""), Linkage::Other);
}

// spec FTEF-21.3: the six flags a block saved come back at its end, so a pragma in it that names
// one holds to the end only, and its change to any other flag holds after it
#[test]
fn a_blocks_end_restores_only_what_it_saved() {
    let edits = [
        (
            12,
            edit(&[(FteFlag::Qcfuncs, true), (FteFlag::Utf8strings, true)]),
        ),
        (14, edit(&[(FteFlag::AssumeDouble, true)])),
    ];
    let flags = lexed(&edits, &[block(10, 20, Linkage::C)]);
    assert!(!CompilerFlags::at(&flags, 11).has(FteFlag::Qcfuncs));
    assert!(CompilerFlags::at(&flags, 12).has(FteFlag::Qcfuncs));
    assert!(CompilerFlags::at(&flags, 14).has(FteFlag::AssumeDouble));
    let after = CompilerFlags::at(&flags, 20);
    assert!(after.has(FteFlag::Qcfuncs));
    assert!(!after.has(FteFlag::AssumeDouble));
    assert!(!after.has(FteFlag::AssumeInteger));
    assert!(after.has(FteFlag::Utf8strings));
}

// spec FTEF-21.1: at one byte a block's restore comes first, then a block's start, then the
// pragmas before the token
#[test]
fn events_at_one_byte_run_in_fteqccs_order() {
    let on = [(20, edit(&[(FteFlag::AssumeInteger, true)]))];
    let flags = lexed(&on, &[block(10, 20, Linkage::Qc)]);
    assert!(CompilerFlags::at(&flags, 20).has(FteFlag::AssumeInteger));
    let off = [(10, edit(&[(FteFlag::Qcfuncs, true)]))];
    let flags = lexed(&off, &[block(10, 20, Linkage::C)]);
    assert!(CompilerFlags::at(&flags, 10).has(FteFlag::Qcfuncs));
}

// spec FTEF-21.3: nested blocks restore inner first, so the outer block's saved flags hold after
// both, even when they end at one byte
#[test]
fn nested_blocks_restore_in_turn() {
    let flags = lexed(
        &[],
        &[block(10, 40, Linkage::C), block(20, 30, Linkage::Qc)],
    );
    assert!(!CompilerFlags::at(&flags, 15).has(FteFlag::Qcfuncs));
    let inner = CompilerFlags::at(&flags, 25);
    assert!(inner.has(FteFlag::Qcfuncs));
    assert!(inner.has(FteFlag::Assumevar));
    assert!(!CompilerFlags::at(&flags, 35).has(FteFlag::Qcfuncs));
    assert_eq!(CompilerFlags::at(&flags, 40), CompilerFlags::default());
    let same_end = lexed(
        &[],
        &[block(10, 30, Linkage::C), block(20, 30, Linkage::Qc)],
    );
    assert_eq!(CompilerFlags::at(&same_end, 30), CompilerFlags::default());
    assert!(!same_end.windows(2).any(|w| w[0].1 == w[1].1));
}

// spec FTEF-21.1: the parser reads the token after a block with the flags restored, after the
// pragmas before that token, which the restore overrides for the six flags it saved
#[test]
fn the_parser_reads_the_restored_flags_a_token_early() {
    let edits = [(
        19,
        edit(&[(FteFlag::Qcfuncs, true), (FteFlag::Utf8strings, true)]),
    )];
    let parsed = marks(&edits, &[block(10, 20, Linkage::C)], Reading::Parsed);
    let at = CompilerFlags::at(&parsed, 19);
    assert!(!at.has(FteFlag::AssumeInteger));
    assert!(at.has(FteFlag::Qcfuncs));
    assert!(at.has(FteFlag::Utf8strings));
    let off = [
        (0, edit(&[(FteFlag::Qcfuncs, false)])),
        (19, edit(&[(FteFlag::Qcfuncs, true)])),
    ];
    let parsed = marks(&off, &[block(10, 20, Linkage::Qc)], Reading::Parsed);
    assert!(!CompilerFlags::at(&parsed, 19).has(FteFlag::Qcfuncs));
    let lexed = lexed(&off, &[block(10, 20, Linkage::Qc)]);
    assert!(CompilerFlags::at(&lexed, 19).has(FteFlag::Qcfuncs));
}
