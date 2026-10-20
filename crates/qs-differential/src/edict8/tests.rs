// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for the EDICT-8 port: the `COM_Parse` tokenizer, the value conversions and the
//! inhibition test, each citing the statement it pins.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    reason = "test code: a failed unwrap, index, panic or overflow is a failed test"
)]

use qc_types::{Etype, Slot};

use super::{
    EDICT_SPAWNFLAG_NOT_DEATHMATCH, EDICT_SPAWNFLAG_NOT_EASY, EDICT_SPAWNFLAG_NOT_HARD,
    EDICT_SPAWNFLAG_NOT_MEDIUM, FieldDef, LoadError, Parser, SPAWNFLAGS_SLOT, SpawnError,
    SpawnHost, Token, c_atof, c_atoi, inhibited, load_from_file,
};

/// Collects every token id's `COM_Parse` yields from `data`, up to `End`.
fn tokens(data: &[u8]) -> Vec<Vec<u8>> {
    let mut parser = Parser::new(data);
    let mut out = Vec::new();
    while let Token::Word(word) = parser.next_token() {
        out.push(word);
    }
    out
}

// spec EDICT-8.8: COM_Parse reads `{`, keys and values, and `}` as single tokens and words.
#[test]
fn com_parse_reads_an_entity_block() {
    let data = b"{\n\"classname\" \"info_player_start\"\n\"angle\" \"90\"\n}\n";
    let got = tokens(data);
    let expected: Vec<&[u8]> = vec![
        b"{",
        b"classname",
        b"info_player_start",
        b"angle",
        b"90",
        b"}",
    ];
    assert_eq!(got.len(), expected.len());
    for (a, b) in got.iter().zip(expected) {
        assert_eq!(a.as_slice(), b);
    }
}

// spec EDICT-8.8: a `//` comment is skipped; a quoted string keeps its spaces; a single
// character `{` `}` is its own token.
#[test]
fn com_parse_skips_comments_and_keeps_quoted_spaces() {
    let data = b"// a comment\n{ \"a b\" }";
    let got = tokens(data);
    assert_eq!(got[0].as_slice(), b"{");
    assert_eq!(got[1].as_slice(), b"a b");
    assert_eq!(got[2].as_slice(), b"}");
}

// spec EDICT-8.8: a quoted string ended by the text (no closing quote) is still a token; a NUL
// byte ends the text, as the lump's trailing NUL does.
#[test]
fn com_parse_ends_at_the_text_or_a_nul() {
    assert_eq!(tokens(b"\"unterminated"), vec![b"unterminated".to_vec()]);
    // A NUL ends the text: what follows it is not read.
    assert_eq!(tokens(b"word\0more"), vec![b"word".to_vec()]);
}

// spec EDICT-8.2: C atof reads a leading sign, digits, one dot and an optional exponent.
#[test]
fn c_atof_reads_a_decimal() {
    assert_eq!(c_atof(b"1.5"), 1.5);
    assert_eq!(c_atof(b"-2"), -2.0);
    assert_eq!(c_atof(b"  3.25"), 3.25);
    assert_eq!(c_atof(b"1e2"), 100.0);
    assert_eq!(c_atof(b"1.2.3"), 1.2);
    assert_eq!(c_atof(b"abc"), 0.0);
    assert_eq!(c_atof(b""), 0.0);
}

// spec EDICT-8.2: an entity key value is atoi(value): a leading sign and digits, stopping at
// the first non-digit.
#[test]
fn c_atoi_reads_an_integer() {
    assert_eq!(c_atoi(b"3"), 3);
    assert_eq!(c_atoi(b"-7"), -7);
    assert_eq!(c_atoi(b"12x"), 12);
    assert_eq!(c_atoi(b"x"), 0);
    assert_eq!(c_atoi(b""), 0);
}

// spec EDICT-8.4: deathmatch inhibits SPAWNFLAG_NOT_DEATHMATCH; skill 0/1/2 inhibit
// NOT_EASY/NOT_MEDIUM/NOT_HARD.
#[test]
fn inhibition_follows_the_skill_and_deathmatch_flags() {
    let dm = EDICT_SPAWNFLAG_NOT_DEATHMATCH as f32;
    assert!(inhibited(dm, true, 0));
    assert!(!inhibited(dm, false, 0));
    assert!(inhibited(EDICT_SPAWNFLAG_NOT_EASY as f32, false, 0));
    assert!(inhibited(EDICT_SPAWNFLAG_NOT_MEDIUM as f32, false, 1));
    assert!(inhibited(EDICT_SPAWNFLAG_NOT_HARD as f32, false, 2));
    assert!(inhibited(EDICT_SPAWNFLAG_NOT_HARD as f32, false, 3));
    // Easy skill does not inhibit a NOT_HARD-only entity.
    assert!(!inhibited(EDICT_SPAWNFLAG_NOT_HARD as f32, false, 0));
}

/// A tiny host that records the fields it was told to write, for the pure loader tests.
#[derive(Default)]
struct RecordingHost {
    /// (edict, field, value) in write order.
    writes: Vec<(usize, usize, Slot)>,
    /// The next engine-string value to hand out (-1, -2, ...).
    next_string: i32,
    /// The function names the host knows, and their indices.
    functions: Vec<(&'static [u8], i32)>,
    /// The field names the host knows, and their (slot, type).
    fields: Vec<(&'static [u8], usize, Etype)>,
    /// How many edicts have been allocated (for the number to hand out).
    allocated: usize,
    /// Spawn and free calls, for the test to check.
    spawned: Vec<usize>,
    freed: Vec<usize>,
}

impl SpawnHost for RecordingHost {
    fn allocate(&mut self, first: bool) -> Result<usize, SpawnError> {
        if first {
            Ok(0)
        } else {
            self.allocated += 1;
            Ok(self.allocated)
        }
    }
    fn edict_value(&self, number: i32) -> Result<i32, SpawnError> {
        // EDICT-1.5, EDICT-1.7 with 105 field slots: an edict is 516 bytes, 600 of them.
        if (0..600).contains(&number) {
            Ok(number * 516)
        } else {
            Err(SpawnError::BadEdictNumber { number })
        }
    }
    fn find_field(&self, name: &[u8]) -> Option<FieldDef> {
        self.fields
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|&(_, slot, etype)| FieldDef { slot, etype })
    }
    fn find_function(&self, name: &[u8]) -> Option<i32> {
        self.functions
            .iter()
            .find(|(n, _)| *n == name)
            .map(|&(_, index)| index)
    }
    fn global(&self, _slot: usize) -> Option<Slot> {
        Some(Slot(0))
    }
    fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        self.writes.push((edict, field, value));
    }
    fn make_string(&mut self, _text: Vec<u8>) -> i32 {
        self.next_string -= 1;
        self.next_string + 1
    }
    fn spawnflags(&self, _edict: usize) -> f32 {
        // Read the spawnflags from the recorded writes (slot SPAWNFLAGS_SLOT).
        self.writes
            .iter()
            .rev()
            .find(|(_, field, _)| *field == SPAWNFLAGS_SLOT)
            .map_or(0.0, |(_, _, value)| value.as_f32())
    }
    fn spawn(&mut self, edict: usize, _function: i32) -> Result<(), qc_vm::RunError> {
        self.spawned.push(edict);
        Ok(())
    }
    fn free(&mut self, edict: usize) {
        self.freed.push(edict);
    }
}

// spec EDICT-8.2: the `angle` key is renamed `angles` and its value v becomes `0 v 0`.
#[test]
fn angle_key_is_renamed_and_expanded() {
    let mut host = RecordingHost {
        functions: vec![(b"worldspawn", 1)],
        fields: vec![
            (b"classname", 28, Etype::String),
            (b"angles", 19, Etype::Vector),
        ],
        ..RecordingHost::default()
    };
    let data = b"{\n\"classname\" \"worldspawn\"\n\"angle\" \"90\"\n}\n";
    load_from_file(&mut host, data, false, 0).expect("loads");
    // angles is a vector field at slot 19; the value "0 90 0" writes 0, 90, 0.
    let angles: Vec<f32> = host
        .writes
        .iter()
        .filter(|(_, field, _)| (19..=21).contains(field))
        .map(|(_, _, v)| v.as_f32())
        .collect();
    assert_eq!(angles, vec![0.0, 90.0, 0.0]);
    assert_eq!(host.spawned, vec![0]);
}

// spec EDICT-8.5: an entity with no classname is freed, not spawned.
#[test]
fn classless_entity_is_freed() {
    let mut host = RecordingHost {
        fields: vec![(b"targetname", 91, Etype::String)],
        ..RecordingHost::default()
    };
    let data = b"{\n\"targetname\" \"x\"\n}\n";
    load_from_file(&mut host, data, false, 0).expect("loads");
    assert!(host.spawned.is_empty());
    assert_eq!(host.freed, vec![0]);
}

// spec EDICT-8.8: text that is not `{` where an entity should start is a parse error.
#[test]
fn non_brace_start_is_an_error() {
    let mut host = RecordingHost::default();
    let err = load_from_file(&mut host, b"oops", false, 0).unwrap_err();
    match err {
        LoadError::Spawn(SpawnError::ExpectedBrace { token }) => assert_eq!(token, b"oops"),
        other => panic!("expected ExpectedBrace, got {other:?}"),
    }
}

// spec EDICT-8.8: running out of text inside an entity is an EOF error.
#[test]
fn eof_in_entity_is_an_error() {
    let mut host = RecordingHost::default();
    let err = load_from_file(&mut host, b"{\n\"classname\"", false, 0).unwrap_err();
    assert!(matches!(err, LoadError::Spawn(SpawnError::EofInEntity)));
}

// spec EDICT-8.8: a `}` where a value should be is a closing-brace-without-data error.
#[test]
fn brace_without_data_is_an_error() {
    let mut host = RecordingHost {
        fields: vec![(b"classname", 28, Etype::String)],
        ..RecordingHost::default()
    };
    let err = load_from_file(&mut host, b"{\n\"classname\" }\n", false, 0).unwrap_err();
    assert!(matches!(
        err,
        LoadError::Spawn(SpawnError::BraceWithoutData)
    ));
}

// spec EDICT-8.2: an unknown function or field name in a typed key is a parse error.
#[test]
fn unknown_function_key_is_a_parse_error() {
    let mut host = RecordingHost {
        // "touch" is a function field; its value names a function the host does not know.
        fields: vec![(b"touch", 42, Etype::Function)],
        ..RecordingHost::default()
    };
    let err = load_from_file(&mut host, b"{\n\"touch\" \"nope\"\n}\n", false, 0).unwrap_err();
    assert!(matches!(err, LoadError::Spawn(SpawnError::ParseError)));
}

// spec EDICT-8.2: a key starting with `_` is ignored.
#[test]
fn underscore_key_is_ignored() {
    let mut host = RecordingHost {
        functions: vec![(b"worldspawn", 1)],
        fields: vec![(b"classname", 28, Etype::String)],
        ..RecordingHost::default()
    };
    let data = b"{\n\"classname\" \"worldspawn\"\n\"_comment\" \"ignored\"\n}\n";
    load_from_file(&mut host, data, false, 0).expect("loads");
    // No field write for the `_comment` key; only the classname string was written.
    assert!(host.writes.iter().all(|(_, field, _)| *field == 28));
}

// spec EDICT-8.7: with the clock at 1, an inhibited entity is handled before the next
// allocation; here the inhibited entity is freed so a later entity can reuse its slot.
#[test]
fn an_inhibited_entity_is_freed() {
    let mut host = RecordingHost {
        functions: vec![(b"worldspawn", 1), (b"item_key", 2)],
        fields: vec![
            (b"classname", 28, Etype::String),
            (b"spawnflags", SPAWNFLAGS_SLOT, Etype::Float),
        ],
        ..RecordingHost::default()
    };
    // deathmatch on; the item_key has SPAWNFLAG_NOT_DEATHMATCH, so it is freed (EDICT-8.4).
    let data = b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"item_key\"\n\"spawnflags\" \"2048\"\n}\n";
    load_from_file(&mut host, data, true, 0).expect("loads");
    assert_eq!(host.spawned, vec![0]);
    assert_eq!(host.freed, vec![1]);
}
