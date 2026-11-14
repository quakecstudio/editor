// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the readers of `qc-builtins` that take outside text (P2-159): the
//! rerelease's localization file (RRE-1.7) and `PF_VarString` over it (RRE-1.9), the
//! tokenizers of client command text (BUILTIN-18.23, BUILTIN-18.24), and vkQuake's `sprintf`
//! directive reader (BUILTIN-18.8) and C's `atof` behind `stof` (BUILTIN-18.25), run as
//! builtins of `MockBuiltins` on the vkQuake profile. Fails on a panic, and on a result outside
//! the bounds the statements give.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qc_builtins::localization::{Localization, RRE_VARSTRING_LEN};
use qc_builtins::tokenize::{MAX_QC_TOKENS, MAX_SEPARATORS, TokenIndex, Tokens};
use qc_builtins::{EngineProfile, MockBuiltins, MockEngine};
use qc_progs::opcode::Opcode;
use qc_progs::{Function, Progs, ProgsFile, Statement, write};
use qc_types::{PROGS_OFS_PARM0, PROGS_OFS_RETURN, PROGS_PARM_SLOTS, Slot};
use qc_vm::{Machine, NoObserver, ServerState};

/// The longest text a temporary string holds, `STRINGTEMP_LENGTH` less its terminator.
const TEMP_TEXT: usize = 1023;

/// The global the entry's `CALL` reads its function from, past the system globals.
const CALL_TARGET_SLOT: i32 = 92;

/// The most parameters a call passes.
const MAX_PARMS: usize = 8;

/// `sprintf` (#627) and `stof` (#81).
const SPRINTF: u32 = 627;
const STOF: u32 = 81;

/// The input's parts: the bytes before the first 0xFF, and the bytes after it split at each
/// zero into at most eight parameters.
fn parts(data: &[u8]) -> (&[u8], Vec<Vec<u8>>) {
    let (head, tail) = match data.iter().position(|&b| b == 0xFF) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &data[data.len()..]),
    };
    let parms = tail
        .split(|&b| b == 0)
        .take(MAX_PARMS)
        .map(<[u8]>::to_vec)
        .collect();
    (head, parms)
}

/// RRE-1.7, RRE-1.9: any file loads; a lookup and `PF_VarString` over the parameters stay in
/// their buffers.
fn localization(file: &[u8], parms: &[Vec<u8>]) {
    let loc = Localization::loc_load_file(file);
    for parm in parms {
        if let Some(value) = loc.loc_get_raw_string(parm) {
            assert!(!value.contains(&0));
        }
        if loc.is_empty() {
            assert!(!loc.loc_has_placeholders(parm));
        }
    }
    let text = loc.pf_var_string(parms);
    assert!(text.len() < RRE_VARSTRING_LEN, "{}", text.len());
    if loc.is_empty() {
        let joined: Vec<u8> = parms.concat();
        assert_eq!(text, joined[..joined.len().min(TEMP_TEXT)]);
    }
}

/// BUILTIN-18.23, BUILTIN-18.24: every token lies inside the text, in order, at most 64 of
/// them, and `argv` gives at most 1023 bytes, a negative index counting from the end.
fn check_tokens(tokens: &Tokens, count: usize, text: &[u8]) {
    assert!(count <= MAX_QC_TOKENS, "{count}");
    let count = i32::try_from(count).unwrap();
    let len = i64::try_from(text.len()).unwrap();
    let mut last = 0;
    for index in 0..count {
        let (start, end) = (tokens.start(TokenIndex::new(index)), tokens.end(TokenIndex::new(index)));
        assert!(
            last <= start && start <= end && end <= len,
            "{start} {end} {len}"
        );
        last = end;
        let word = tokens.argv(TokenIndex::new(index)).unwrap();
        assert!(word.len() <= TEMP_TEXT);
        assert_eq!(tokens.argv(TokenIndex::new(index - count)), Some(word));
    }
    for index in [count, -count - 1, i32::MAX, i32::MIN] {
        assert_eq!(tokens.argv(TokenIndex::new(index)), None);
        assert_eq!((tokens.start(TokenIndex::new(index)), tokens.end(TokenIndex::new(index))), (-1, -1));
    }
}

fn tokenizers(text: &[u8], separators: &[Vec<u8>]) {
    let mut tokens = Tokens::default();
    let count = tokens.tokenize(text);
    check_tokens(&tokens, count, text);
    let separators = &separators[..separators.len().min(MAX_SEPARATORS)];
    let count = tokens.split(text, separators);
    check_tokens(&tokens, count, text);
    for index in 0..i32::try_from(count).unwrap() {
        let start = usize::try_from(tokens.start(TokenIndex::new(index))).unwrap();
        let end = usize::try_from(tokens.end(TokenIndex::new(index))).unwrap();
        let slice = text[start..end].split(|&b| b == 0).next().unwrap();
        assert_eq!(
            tokens.argv(TokenIndex::new(index)).unwrap(),
            slice[..slice.len().min(TEMP_TEXT)]
        );
    }
}

/// A progs whose entry calls builtin `number` with `argc` parameters, with `strings` as its
/// string table.
fn progs_bytes(number: u32, argc: usize, strings: Vec<u8>) -> Vec<u8> {
    let mut globals = vec![Slot::ZERO; 128];
    globals[usize::try_from(CALL_TARGET_SLOT).unwrap()] = Slot::from_i32(2);
    let call = Opcode::from_number(51 + u32::try_from(argc).unwrap()).unwrap();
    let statements = vec![
        Statement {
            op: u32::from(call.number()),
            a: CALL_TARGET_SLOT,
            b: 0,
            c: 0,
        },
        Statement {
            op: u32::from(Opcode::Done.number()),
            a: i32::from(PROGS_OFS_RETURN),
            b: 0,
            c: 0,
        },
    ];
    let functions = vec![
        Function::default(),
        Function::default(),
        Function {
            first_statement: -i32::try_from(number).unwrap(),
            ..Function::default()
        },
    ];
    write(&ProgsFile {
        entityfields: 105,
        statements,
        functions,
        globals,
        strings,
        ..ProgsFile::default()
    })
    .unwrap()
}

/// Runs builtin `number` on `parms`: a parameter that starts with 1 is its next twelve bytes
/// as three words, any other one a string. Returns the return slot, or `None` for a run error.
fn run(number: u32, parms: &[Vec<u8>]) -> Option<(MockBuiltins, Slot)> {
    let mut strings = vec![0_u8];
    let mut values = Vec::new();
    for parm in parms {
        if let [1, words @ ..] = parm.as_slice() {
            let mut slots = [Slot::ZERO; 3];
            for (slot, word) in slots.iter_mut().zip(words.as_chunks::<4>().0) {
                *slot = Slot::from_i32(i32::from_le_bytes(*word));
            }
            values.push(slots);
        } else {
            values.push([
                Slot::from_i32(i32::try_from(strings.len()).unwrap()),
                Slot::ZERO,
                Slot::ZERO,
            ]);
            strings.extend_from_slice(parm);
            strings.push(0);
        }
    }
    while !strings.len().is_multiple_of(4) {
        strings.push(0);
    }
    let bytes = progs_bytes(number, parms.len(), strings);
    let progs = Progs::read(&bytes).unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    for (n, value) in values.iter().enumerate() {
        let base =
            i32::from(PROGS_OFS_PARM0) + i32::from(PROGS_PARM_SLOTS) * i32::try_from(n).unwrap();
        for (offset, slot) in (0..).zip(value) {
            machine.set_global(base + offset, *slot).unwrap();
        }
    }
    let mut engine = MockEngine::new("fuzz", 0, 1);
    engine.profile = EngineProfile::VkQuake;
    let mut builtins = MockBuiltins::with_engine(engine);
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut builtins,
            &mut NoObserver,
            1,
        )
        .ok()?;
    let result = machine.global(i32::from(PROGS_OFS_RETURN)).unwrap();
    Some((builtins, result))
}

/// BUILTIN-18.8, BUILTIN-18.25: `sprintf` of any format and parameters makes a string of at
/// most 1023 bytes, and `stof` of any string runs.
fn builtins(parms: &[Vec<u8>]) {
    if parms.is_empty() {
        return;
    }
    if let Some((builtins, result)) = run(SPRINTF, parms) {
        let text = builtins.engine().strings.text(result.as_i32()).unwrap();
        assert!(text.len() <= TEMP_TEXT, "{}", text.len());
    }
    let _ = run(STOF, &parms[..1]);
}

fuzz_target!(|data: &[u8]| {
    let (head, parms) = parts(data);
    localization(head, &parms);
    tokenizers(head, &parms);
    builtins(&parms);
});
