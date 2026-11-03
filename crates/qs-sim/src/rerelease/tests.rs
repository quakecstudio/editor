// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the rerelease's builtin patch (RRE-4.1), on progs qc-progs writes.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::{Function, Progs, ProgsFile};

use super::patch_builtins;

/// A progs whose functions are each name with its `first_statement`.
fn progs(functions: &[(&str, i32)]) -> Vec<u8> {
    let mut file = ProgsFile::default();
    let mut strings = vec![0_u8];
    for (name, first) in functions {
        let s_name = i32::try_from(strings.len()).unwrap();
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        file.functions.push(Function {
            first_statement: *first,
            s_name,
            ..Function::default()
        });
    }
    file.strings = strings;
    qc_progs::write(&file).unwrap()
}

fn first_statements(bytes: &[u8]) -> Vec<i32> {
    Progs::read(bytes)
        .unwrap()
        .functions()
        .iter()
        .map(|f| f.first_statement)
        .collect()
}

// spec RRE-4.1: a function of a name in the table whose first_statement is the row's is given
// the row's builtin; another first_statement, or another name, is left alone
#[test]
fn named_builtins_are_patched_to_their_numbers() {
    let mut bytes = progs(&[
        ("", 0),
        ("ex_bprint", 0),
        ("ex_localsound", 0),
        ("bprint", -91),
        ("sprint", -24),
        ("ex_draw_line", 5),
        ("ex_bot_followentity", 0),
        ("other", 0),
    ]);
    assert_eq!(patch_builtins(&mut bytes), 4);
    assert_eq!(first_statements(&bytes), [0, -23, -80, -23, -24, 5, -92, 0]);
}

// spec RRE-4.1: ED_FindFunction finds the first function of a name, so only it is patched
#[test]
fn only_the_first_function_of_a_name_is_patched() {
    let mut bytes = progs(&[("", 0), ("ex_sprint", 7), ("ex_sprint", 0)]);
    assert_eq!(patch_builtins(&mut bytes), 0);
    assert_eq!(first_statements(&bytes), [0, 7, 0]);
}

// spec RRE-4.1: bytes that are no progs are left as they are
#[test]
fn bytes_that_are_no_progs_are_left_alone() {
    let mut bytes = vec![1, 2, 3];
    assert_eq!(patch_builtins(&mut bytes), 0);
    assert_eq!(bytes, [1, 2, 3]);
}
