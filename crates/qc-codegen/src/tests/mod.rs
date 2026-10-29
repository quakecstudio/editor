// SPDX-License-Identifier: GPL-3.0-or-later

//! The compiler's unit tests, each citing the statement of `docs/spec/language.md` it pins.
//! The expected statements were read from id's `qcc` built by `cargo xtask oracles build`, with
//! `qcc -asm`, on the same programs; `cargo xtask oracles codegen` compiles every program of
//! these files with both compilers and compares the progs byte for byte.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code: a failed lookup is a failed test"
)]

mod api;
mod declarations;
mod expressions;
mod fteqcc;
mod output;
mod statements;

use std::path::PathBuf;

use qc_progs::opcode::Opcode;
use qc_progs::{ProgsFile, Statement};
use qc_sema::{Db, FileId, ModuleInput};

use crate::{CompileError, Compiled, Options, compile_module};

/// A program of one file, `t.qc`, compiled as `progs.src` lists it.
pub(crate) fn compile_source(source: &str) -> Result<Compiled, CompileError> {
    compile_files(&[("t.qc", source)])
}

/// A program of the files `files`, compiled in that order.
pub(crate) fn compile_files(files: &[(&str, &str)]) -> Result<Compiled, CompileError> {
    compile_with(files, Options::default())
}

/// [`compile_files`] with `options`.
pub(crate) fn compile_with(
    files: &[(&str, &str)],
    options: Options,
) -> Result<Compiled, CompileError> {
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (i, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(i), PathBuf::from(name), text.as_bytes().to_vec());
        input.compiled.push(FileId(i));
    }
    let names: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    compile_module(&mut db, &input, |id| names[id.0].clone(), options)
}

/// A program of one file, `t.qc`, compiled under the FTEQCC profile.
pub(crate) fn compile_fteqcc(source: &str) -> Result<Compiled, CompileError> {
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let input = ModuleInput {
        compiled: vec![FileId(0)],
        ..ModuleInput::default()
    };
    compile_module(&mut db, &input, |_| "t.qc".to_owned(), Options::default())
}

/// The progs of a program that must compile under the FTEQCC profile.
pub(crate) fn fte_progs(source: &str) -> ProgsFile {
    match compile_fteqcc(source) {
        Ok(compiled) => compiled.progs,
        Err(error) => panic!("{source:?} did not compile: {error:?}"),
    }
}

/// The progs of a program that must compile.
pub(crate) fn progs(source: &str) -> ProgsFile {
    match compile_source(source) {
        Ok(compiled) => compiled.progs,
        Err(error) => panic!("{source:?} did not compile: {error:?}"),
    }
}

/// The messages of a program that must not compile.
pub(crate) fn errors(source: &str) -> Vec<String> {
    match compile_source(source) {
        Ok(_) => panic!("{source:?} compiled"),
        Err(CompileError::Diagnostics(diagnostics)) => diagnostics
            .into_iter()
            .map(|d| d.diagnostic.message)
            .collect(),
        Err(other) => panic!("{source:?}: {other:?}"),
    }
}

/// The global offset of the first global definition named `name`.
pub(crate) fn global(progs: &ProgsFile, name: &str) -> i32 {
    let def = progs
        .global_defs
        .iter()
        .find(|d| string(progs, d.s_name) == name)
        .unwrap_or_else(|| panic!("no global {name}"));
    i32::try_from(def.ofs).unwrap()
}

/// The global offset of the `IMMEDIATE` float definition holding `value`.
pub(crate) fn immediate(progs: &ProgsFile, value: f32) -> i32 {
    immediate_where(progs, 2, |p, ofs| p.globals[ofs].bits() == value.to_bits())
}

/// The global offset of the `IMMEDIATE` string definition holding `text`.
pub(crate) fn immediate_string(progs: &ProgsFile, text: &str) -> i32 {
    immediate_where(progs, 1, |p, ofs| {
        string(p, i32::try_from(p.globals[ofs].bits()).unwrap()) == text
    })
}

/// The global offset of the `IMMEDIATE` vector definition holding `v`.
pub(crate) fn immediate_vector(progs: &ProgsFile, v: [f32; 3]) -> i32 {
    immediate_where(progs, 3, |p, ofs| {
        (0..3).all(|i| p.globals[ofs + i].bits() == v[i].to_bits())
    })
}

fn immediate_where(progs: &ProgsFile, ty: u32, holds: impl Fn(&ProgsFile, usize) -> bool) -> i32 {
    let def = progs
        .global_defs
        .iter()
        .find(|d| {
            d.type_bits == ty
                && string(progs, d.s_name) == "IMMEDIATE"
                && holds(progs, usize::try_from(d.ofs).unwrap())
        })
        .unwrap_or_else(|| panic!("no such IMMEDIATE"));
    i32::try_from(def.ofs).unwrap()
}

/// The string at offset `ofs` of the string table.
pub(crate) fn string(progs: &ProgsFile, ofs: i32) -> String {
    let start = usize::try_from(ofs).unwrap();
    let rest = &progs.strings[start..];
    let end = rest.iter().position(|&b| b == 0).unwrap();
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// The function named `name`.
pub(crate) fn function<'p>(progs: &'p ProgsFile, name: &str) -> &'p qc_progs::Function {
    progs
        .functions
        .iter()
        .find(|f| f.first_statement != 0 && string(progs, f.s_name) == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

/// The statements of QC function `name`, up to and including its `DONE`.
pub(crate) fn body(progs: &ProgsFile, name: &str) -> Vec<Statement> {
    let start = usize::try_from(function(progs, name).first_statement).unwrap();
    let mut out = Vec::new();
    for s in &progs.statements[start..] {
        out.push(*s);
        if s.op == u32::from(Opcode::Done.number()) {
            break;
        }
    }
    out
}

/// A statement written `op a b c`.
pub(crate) fn st(op: Opcode, a: i32, b: i32, c: i32) -> Statement {
    Statement {
        op: u32::from(op.number()),
        a,
        b,
        c,
    }
}

/// The words of the global area from `ofs`.
pub(crate) fn words(progs: &ProgsFile, ofs: i32, n: usize) -> Vec<u32> {
    let start = usize::try_from(ofs).unwrap();
    progs.globals[start..start + n]
        .iter()
        .map(|s| s.bits())
        .collect()
}
