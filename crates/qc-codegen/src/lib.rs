// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC compiler's code generator: a module the semantic model resolved, lowered to a
//! version 6 `progs.dat` as id's `qcc` writes it.
//!
//! `qcc` emits code while it parses. This crate walks each compiled file's syntax tree, the
//! expansion the semantic model read (QC-9.16), in the order `qcc`'s parser meets its parts,
//! and takes the definition each name is from the semantic model (QC-12). It builds what `qcc`
//! builds as it goes, in the same order: the definition list and the global area, where each
//! definition is placed when first met (QC-21), literals shared by value (QC-20), the
//! statements each construct emits with a new temporary for every result (QC-14 to QC-19,
//! QC-22), the functions in the order their definitions end (QC-25.2) and the string table
//! (QC-25.4); then `progdefs.h`, whose CRC the header carries, and the tables of the progs
//! (QC-25). The code is ported from `qcc/pr_comp.c` and `qcc/qcc.c` at
//! id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`, function by function as
//! `docs/spec/inventory-l4.md` lists. For every program id's `qcc` compiles, the progs is the
//! one `qcc` writes, byte for byte, but where QC-27 says otherwise.
//!
//! The type rules of expressions are the compiler's (QC-16 to QC-19): an operator's opcode is
//! the first of its spelling whose operand types are the operands' basic types (QC-16.8,
//! QC-17.6), and a call's arguments must have exactly its parameters' types (QC-18.8). Their
//! errors are reported as `qcc` reports them (QCD-4).
//!
//! # Layer
//!
//! L4 bridges. It depends on `qc-sema` (L3), `qc-syntax` and `qc-lexer` (L2), and `qc-progs`,
//! `qc-types` and `qc-diag` (L0).
//!
//! # Spec sections
//!
//! Implements QC-13 (function definitions) to QC-22 (the construct to statement mapping),
//! QC-25 (program output), QC-26 (precache lists) and the code-generation divergences of
//! QC-27 of `docs/spec/language.md`, and the expression errors QCD-4.2 to QCD-4.7 of
//! `docs/spec/language-diagnostics.md`.

mod compile;
mod names;
mod ops;
mod precache;
mod program;
mod write;

#[cfg(test)]
mod tests;

use qc_diag::{Severity, Span};
use qc_lno::LnoFile;
use qc_progs::{ProgsFile, WriteError};
use qc_sema::lower::{FileDecls, lower_file};
use qc_sema::{Db, FileDiagnostic, FileId, ModuleInput, ResolveOptions, resolve_module};
use qc_syntax::{Dialect, Parse};

use crate::compile::Compiler;
use crate::names::Names;

pub use precache::{Precache, Precaches};

/// One compiled file: the name its functions record (`s_file`, QC-25.4) and its syntax tree,
/// the text the compiler reads after preprocessing (QC-9.16).
#[derive(Clone, Copy, Debug)]
pub struct Unit<'a> {
    /// The file's name as the progs records it.
    pub name: &'a str,
    /// The parse of its text.
    pub parse: &'a Parse,
}

/// How a module is compiled: the optimisations turned on (COMP-5), each off by default, where
/// the output is `qcc`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--opt fold`: an arithmetic operator of two float or vector literals is the literal of
    /// its value, computed as the VM computes it, rather than a statement.
    pub fold: bool,
}

/// A compiled program.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    /// The progs, in `qcc`'s layout (QC-25.1).
    pub progs: ProgsFile,
    /// The text of `progdefs.h` (QC-25.5).
    pub progdefs: String,
    /// The CRC of `progdefs.h` the progs header carries (QC-25.6).
    pub crc: u16,
    /// The precache lists (QC-26).
    pub precaches: Precaches,
    /// The line map written beside the progs (LNO-1): the source line of each statement, with
    /// the progs' counts it is checked against (LNO-4.1).
    pub lno: LnoFile,
    /// The warnings of the parser and the semantic model, which never change the progs
    /// (QCD-7).
    pub warnings: Vec<FileDiagnostic>,
}

/// Why a module was not compiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The module has errors: the preprocessor's, the parser's, the semantic model's or the
    /// compiler's. Nothing is emitted (QC-24.5, QC-24.6).
    Diagnostics(Vec<FileDiagnostic>),
    /// The compiler lowers only the vanilla dialect; the FTEQCC subset is P2-07's.
    Dialect(Dialect),
    /// A compiled file of the input that [`Db::analyze_module`] did not read.
    NotAnalysed(FileId),
    /// The progs does not fit the version 6 format: more globals, statements or strings than
    /// its integers hold (PROGS-22.4).
    Write(WriteError),
    /// The compiler met a construct the semantic model should have refused, or a table grew
    /// past what an offset can hold.
    Internal {
        /// The file it is in.
        file: FileId,
        /// Where.
        span: Span,
        /// What.
        internal: Internal,
    },
}

/// What the compiler could not lower, which no module the semantic model accepts under the
/// vanilla profile has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Internal {
    /// A name the semantic model resolved to no definition.
    Unresolved,
    /// A name used before the compiler made its definition.
    Unallocated,
    /// A definition id the semantic model does not have, or a vector component it did not make.
    NoDefinition,
    /// A construct of another dialect.
    Unsupported,
    /// A part of a construct the parser left out.
    Unlowered,
    /// A table or offset past what the format's integers hold.
    TooLarge,
}

/// Compiles `units`, a module's files in compilation order (QC-23.3), each already
/// preprocessed, under the vanilla profile.
///
/// The files are lowered and resolved by the semantic model (`qc_sema::lower_file`,
/// `qc_sema::resolve_module`); a parser or semantic error stops here. Every diagnostic is at a
/// span of its unit's text, the unit its [`FileId`] numbers.
///
/// # Errors
///
/// [`CompileError::Diagnostics`] when the module has errors, [`CompileError::Internal`] when a
/// construct cannot be lowered.
pub fn compile(units: &[Unit<'_>], options: Options) -> Result<Compiled, CompileError> {
    let decls: Vec<FileDecls> = units
        .iter()
        .map(|u| lower_file(u.parse, Dialect::Vanilla))
        .collect();
    let texts: Vec<Vec<u8>> = units.iter().map(|u| u.parse.syntax().text()).collect();
    let vanilla = ResolveOptions {
        dialect: Dialect::Vanilla,
        ..ResolveOptions::default()
    };
    compile_lowered(units, &decls, vanilla, options, |file, at| {
        texts.get(file.0).map_or(0, |text| line_at(text, at))
    })
}

/// [`compile`] of units whose parses are lowered already, `decls[i]` from `units[i]`.
fn compile_lowered(
    units: &[Unit<'_>],
    decls: &[FileDecls],
    resolve: ResolveOptions,
    options: Options,
    line_of: impl Fn(FileId, usize) -> i32,
) -> Result<Compiled, CompileError> {
    let index = resolve_module(decls, resolve);
    let mut errors: Vec<FileDiagnostic> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        errors.extend(unit.parse.diagnostics().iter().map(|d| FileDiagnostic {
            file: FileId(i),
            diagnostic: d.clone(),
        }));
    }
    errors.extend(index.diagnostics.iter().cloned());
    let (errors, warnings): (Vec<_>, Vec<_>) = errors
        .into_iter()
        .partition(|d| d.diagnostic.code.severity() == Severity::Error);
    if !errors.is_empty() {
        return Err(CompileError::Diagnostics(errors));
    }
    let names = Names::new(&index);
    let mut compiler = Compiler::new(&index, &names);
    compiler.fold = options.fold;
    for (i, unit) in units.iter().enumerate() {
        compiler
            .compile_file(FileId(i), unit.name, unit.parse)
            .map_err(|(internal, file, span)| CompileError::Internal {
                file,
                span,
                internal,
            })?;
    }
    if !compiler.errors.is_empty() {
        return Err(CompileError::Diagnostics(compiler.errors));
    }
    let (progdefs, crc) = write::progdefs(&compiler.program);
    let precaches = compiler.precaches;
    let lines: Vec<i32> = compiler
        .program
        .origins
        .iter()
        .map(|origin| origin.map_or(0, |(file, at)| line_of(file, at)))
        .collect();
    let progs = write::progs(compiler.program, crc).map_err(|internal| CompileError::Internal {
        file: FileId(0),
        span: Span::new(0, 0),
        internal,
    })?;
    // PROGS-22.4: a table or operand past the version 6 format's integers is refused here,
    // not when the caller writes the file.
    qc_progs::write(&progs).map_err(CompileError::Write)?;
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let lno = LnoFile {
        numglobaldefs: count(progs.global_defs.len()),
        numglobals: count(progs.globals.len()),
        numfielddefs: count(progs.field_defs.len()),
        lines,
        columns: None,
        trailing: Vec::new(),
    };
    Ok(Compiled {
        progs,
        progdefs,
        crc,
        precaches,
        lno,
        warnings,
    })
}

/// Compiles the module `input` of `db`, as the editor and the command line build it.
///
/// The module is read through [`Db::analyze_module`] (QC-9, QC-12); any error there stops the
/// build. Each compiled file is then lowered from the expansion the analysis read, named by
/// `name_of` in the progs (`s_file`: the name `progs.src` lists it by, as `qcc` records it run in
/// the source directory, QC-25.4). The compiler's own diagnostics are located in the source
/// files, as the analysis's are (QC-9.7).
///
/// # Errors
///
/// [`CompileError::Dialect`] when `db` is not in the vanilla dialect; otherwise as [`compile`].
pub fn compile_module(
    db: &mut Db,
    input: &ModuleInput,
    name_of: impl Fn(FileId) -> String,
    options: Options,
) -> Result<Compiled, CompileError> {
    if db.dialect() == Dialect::Gmqcc {
        return Err(CompileError::Dialect(db.dialect()));
    }
    let analysis = db.analyze_module(input);
    let (errors, warnings): (Vec<FileDiagnostic>, Vec<FileDiagnostic>) = analysis
        .diagnostics()
        .cloned()
        .partition(|d| d.diagnostic.code.severity() == Severity::Error);
    if !errors.is_empty() {
        return Err(CompileError::Diagnostics(errors));
    }
    let names: Vec<String> = input.compiled.iter().map(|&id| name_of(id)).collect();
    let mut units = Vec::with_capacity(input.compiled.len());
    let mut decls = Vec::with_capacity(input.compiled.len());
    for (&id, name) in input.compiled.iter().zip(&names) {
        let parse = db.expanded_parse(id).ok_or(CompileError::NotAnalysed(id))?;
        let lowered = db.expanded_decls(id).ok_or(CompileError::NotAnalysed(id))?;
        units.push(Unit { name, parse });
        decls.push(lowered.clone());
    }
    let mut compiled = compile_lowered(&units, &decls, analysis.options, options, |file, at| {
        // QC-27.19: the line in the source file the expansion's text came from (QC-9.7).
        let id = input.compiled.get(file.0).copied().unwrap_or(file);
        let (source, at) = db
            .expanded_origins(id)
            .and_then(|origins| origins.locate(Span::new(at, at)))
            .map_or((id, at), |location| (location.file, location.span.start));
        line_at(db.file_text(source), at)
    })
    .map_err(|error| locate(db, input, error))?;
    // The analysis's warnings are at their source spans.
    compiled.warnings = warnings;
    Ok(compiled)
}

/// `error` with each file and span of a unit's text located in its source file (QC-9.7).
fn locate(db: &Db, input: &ModuleInput, error: CompileError) -> CompileError {
    let at = |file: FileId, span: Span| {
        let id = input.compiled.get(file.0).copied().unwrap_or(file);
        db.expanded_origins(id)
            .and_then(|origins| origins.locate(span))
            .map_or((id, span), |location| (location.file, location.span))
    };
    match error {
        CompileError::Diagnostics(diagnostics) => CompileError::Diagnostics(
            diagnostics
                .into_iter()
                .map(|mut d| {
                    let (file, span) = at(d.file, d.diagnostic.span);
                    d.file = file;
                    d.diagnostic.span = span;
                    d
                })
                .collect(),
        ),
        CompileError::Internal {
            file,
            span,
            internal,
        } => {
            let (file, span) = at(file, span);
            CompileError::Internal {
                file,
                span,
                internal,
            }
        }
        CompileError::Dialect(_) | CompileError::NotAnalysed(_) | CompileError::Write(_) => error,
    }
}

/// The line of byte `at` of `text`, counted from 1 (LNO-3.1).
fn line_at(text: &[u8], at: usize) -> i32 {
    let before = text.get(..at).unwrap_or(text);
    let newlines = before.iter().filter(|&&b| b == b'\n').count();
    i32::try_from(newlines.saturating_add(1)).unwrap_or(i32::MAX)
}
