// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the third P2-R2 readiness review (TEST-9), written by a reviewer who
//! did not write the preprocessor: macro expansion at depth after P2-65 (QC-9.13).

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

// spec QC-9.13: a chain of 100000 function-like macros, each calling the one before with its
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

// spec QC-9.13: a macro call nested 100000 deep in its own argument expands the outer call and
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

// spec QC-9.13: in a 10000-long chain whose every macro also names the chain's head, the head
// is hidden in each expansion (a name with live hide-set nodes is still looked up in the
// token's own set), so each use of it is left as written and the chain still reaches its end
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
    for dialect in DIALECTS {
        assert_eq!(
            on_small_stack(text.clone(), dialect),
            (expected.clone(), vec![])
        );
    }
}
