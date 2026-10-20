// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the third P2-R2 readiness review (TEST-9), written by a reviewer who
//! did not write the preprocessor: macro expansion at depth after P2-65 (QC-9.17).

use std::fmt::Write as _;
use std::path::Path;

use super::{codes, tokens};
use crate::{Code, MemoryLoader, Preprocessor};

/// The tokens and diagnostic codes of `text` as `a.qc` under `dialect`, preprocessed on a
/// 2 MiB thread, the smallest stack the reviews use.
fn on_small_stack(text: String, dialect: qc_lexer::Dialect) -> (String, Vec<Code>) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let mut loader = MemoryLoader::new();
            loader.insert("a.qc", text.as_bytes());
            let mut pp = Preprocessor::new(loader).with_dialect(dialect);
            let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
            (tokens(&out), codes(&out))
        })
        .unwrap()
        .join()
        .unwrap()
}

const DIALECTS: [qc_lexer::Dialect; 2] = [qc_lexer::Dialect::Vanilla, qc_lexer::Dialect::Fteqcc];

// spec QC-9.17: a chain of 100000 function-like macros, each calling the one before with its
// argument, expands to its end under both dialects with no diagnostic
#[test]
fn reviewer_a_chain_of_function_macros_expands_to_its_end() {
    let n = 100_000_usize;
    let mut text = "#define F0(x) x\n".to_owned();
    for (before, i) in (1..n).enumerate() {
        writeln!(text, "#define F{i}(x) F{before}(x)").unwrap();
    }
    writeln!(text, "F{}(1)", n.saturating_sub(1)).unwrap();
    for dialect in DIALECTS {
        assert_eq!(
            on_small_stack(text.clone(), dialect),
            ("1".to_owned(), vec![])
        );
    }
}

// spec QC-9.17: a macro call nested 100000 deep in its own argument expands the outer call and
// leaves each inner one, which its hide set names, as written, with no diagnostic
#[test]
fn reviewer_a_call_nested_in_its_own_argument_expands_once() {
    let n = 100_000_usize;
    let mut text = "#define F(x) x\n".to_owned();
    text.push_str(&"F(".repeat(n));
    text.push('1');
    text.push_str(&")".repeat(n));
    text.push('\n');
    let inner = n.saturating_sub(1);
    let expected = format!("{}1{}", "F ( ".repeat(inner), " )".repeat(inner));
    for dialect in DIALECTS {
        assert_eq!(
            on_small_stack(text.clone(), dialect),
            (expected.clone(), vec![])
        );
    }
}

// spec QC-9.17: in a 10000-long chain whose every macro also names the chain's head, the head
// is hidden in each expansion (a name with live hide-set nodes is still looked up in the
// token's own set), so each use of it is left as written and the chain still reaches its end
// spec FTEF-14.28: under FTEQCC the head is expanded again in each chunk it is not open in,
// so the expansion outgrows the bound, which is reported once
#[test]
fn reviewer_a_chain_that_names_its_head_leaves_the_head_unexpanded() {
    let n = 10_000_usize;
    let head = n.saturating_sub(1);
    let mut text = "#define M0 1\n".to_owned();
    for (before, i) in (1..n).enumerate() {
        writeln!(text, "#define M{i} M{before} M{head}").unwrap();
    }
    writeln!(text, "M{head}").unwrap();
    let expected = format!("1{}", format!(" M{head}").repeat(head));
    assert_eq!(
        on_small_stack(text.clone(), qc_lexer::Dialect::Vanilla),
        (expected, vec![])
    );
    let (_, codes) = on_small_stack(text, qc_lexer::Dialect::Fteqcc);
    assert_eq!(codes, vec![Code::E0120]);
}

/// The tokens and diagnostic codes of `text` as `a.qc` under FTEQCC.
fn fte(text: &str) -> (String, Vec<Code>) {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    (tokens(&out), codes(&out))
}

// The cases below were compiled with the pinned FTEQCC (fte-team/fteqw@f937b9d, `-Tid`) by the
// fourth P2-R2 review, each as `void() f = USE;` after its definitions, and the profile agreed
// with every one through `cargo xtask oracles builtins`.

// spec FTEF-14.29: a `\` and line break between a name and its `(` in a body is dropped as the
// body is stored, so the `(` follows the name and opens its arguments
#[test]
fn reviewer_a_continued_line_in_a_body_still_opens_the_arguments() {
    assert_eq!(
        fte("#define M(x) x\n#define K M\\\n(#5)\nK"),
        ("# 5".to_owned(), vec![])
    );
}

// spec FTEF-14.14: one argument pasted to itself is its text twice, and `x##_a` given as an
// argument to another macro is pasted before that macro reads it
#[test]
fn reviewer_pastes_join_an_argument_to_itself_and_inside_an_argument() {
    assert_eq!(
        fte("#define E(n) #5:n##n\nE(ex)"),
        ("# 5 : exex".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define E(x) #5:x\n#define F(x) E(x##_a)\nF(ex)"),
        ("# 5 : ex_a".to_owned(), vec![])
    );
}

// spec FTEF-14.31: a string in an argument is one piece of it, so its comma splits nothing, and
// `#a` writes the string's own quotes escaped
#[test]
fn reviewer_a_string_s_comma_does_not_split_an_argument() {
    let (text, codes) = fte("#define S(a) #a\n:S(\"a,b\")");
    assert_eq!((text.as_str(), codes), (": \"\\\"a,b\\\"\"", vec![]));
}

// spec FTEF-14.5: white space around a parameter's name in the list is not part of it
#[test]
fn reviewer_spaces_around_a_parameter_name_are_dropped() {
    assert_eq!(
        fte("#define E( n ) :n\nE(ex_a)"),
        (": ex_a".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define E(n , m) n:m\nE(#5, ex_a)"),
        ("# 5 : ex_a".to_owned(), vec![])
    );
}

// spec FTEF-14.9: one empty argument more than the parameters is accepted and unused, two are
// E0116, and a variadic macro takes any number of empty ones
#[test]
fn reviewer_trailing_empty_arguments_count_as_fteqcc_counts_them() {
    assert_eq!(
        fte("#define E(n) n\nE(#5:ex_a,)"),
        ("# 5 : ex_a".to_owned(), vec![])
    );
    assert_eq!(fte("#define E(n) n\nE(#5:ex_a,,)").1, vec![Code::E0116]);
    assert_eq!(
        fte("#define E(a,...) #5:a\nE(ex_a, , ,)"),
        ("# 5 : ex_a".to_owned(), vec![])
    );
}

// spec FTEF-14.20: a macro named in an argument is expanded when the expansion is read, in the
// macro's chunk, and a function-like macro written with its arguments inside an argument is
// expanded there
#[test]
fn reviewer_a_macro_in_an_argument_expands_where_it_is_read() {
    assert_eq!(
        fte("#define E(x) #5:x\n#define ex_a ex_b\nE(ex_a)"),
        ("# 5 : ex_b".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define E(x) x\n#define G(x) #5:x\nE(G(ex_a))"),
        ("# 5 : ex_a".to_owned(), vec![])
    );
}
