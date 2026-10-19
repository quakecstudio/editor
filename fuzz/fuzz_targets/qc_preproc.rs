// SPDX-License-Identifier: GPL-3.0-or-later

//! Preprocesses arbitrary bytes under the FTEQCC profile (P2-51): the `#if` reader, continued
//! directives, variadic macros, `#` and `##`, the built-in constants and the frame macros
//! (FTEF-14, FTEF-18.16, FTE-22.4).
//!
//! The input is split at its first two NULs into a compiled file `a.qc`, a file it may
//! `#include` (`b.qh`) and a second compiled file `c.qc`, so macros and frame macros carry
//! from one file to the next. The check: no panic, and every diagnostic and every token lies
//! in a file that was read, inside its text, in a first pass and in the second pass
//! `Preprocessor::restart` begins (FTE-22.3), which keeps the first pass's frame macros; and the
//! `#if` evaluator gives an answer for the first line under both profiles.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qc_lexer::Dialect;
use qc_preproc::{FileOutput, MemoryLoader, Preprocessor, SourceMap, cond};

fuzz_target!(|data: &[u8]| {
    let mut parts = data.splitn(3, |b| *b == 0);
    let a = parts.next().unwrap_or_default();
    let b = parts.next().unwrap_or_default();
    let c = parts.next().unwrap_or_default();
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", a);
    loader.insert("b.qh", b);
    loader.insert("c.qc", c);
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    run(&mut pp);
    pp.restart();
    run(&mut pp);

    let line = a.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    let macros = |name: &str| (name == "X").then(|| "1".to_owned());
    let _ = cond::eval(&line, &macros, Dialect::Fteqcc);
    let _ = cond::eval(&line, &macros, Dialect::Vanilla);
});

/// Preprocesses `a.qc` then `c.qc`, checking every span of each output.
fn run(pp: &mut Preprocessor<MemoryLoader>) {
    for path in ["a.qc", "c.qc"] {
        let Ok(out) = pp.preprocess_file(Path::new(path)) else {
            panic!("{path} is in the loader");
        };
        check_spans(pp.sources(), &out);
    }
}

/// Every token and diagnostic lies inside the text of a file that was read.
fn check_spans(sources: &SourceMap, out: &FileOutput) {
    for token in &out.tokens {
        assert!(
            sources.text(token.span).is_some(),
            "token outside its file: {token:?}"
        );
    }
    for d in &out.diagnostics {
        assert!(
            sources.text(d.span).is_some(),
            "diagnostic outside its file: {d:?}"
        );
    }
}
