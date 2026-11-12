// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC compiler's code generator: a module the semantic model resolved, lowered to a
//! version 6 `progs.dat` as id's `qcc` writes it.
//!
//! The semantic model lowers each function body into its intermediate layer as it checks it
//! (`qc_sema::ir`, `TypedBody::generated`): the statements `qcc` or FTEQCC emits for it, in
//! order, emulations included, with operands that name values. This crate reads each compiled
//! file's declarations, the expansion the semantic model read (QC-9.16), in the order `qcc`'s
//! parser meets them, and takes the definition each name is from the semantic model (QC-12). It
//! builds what `qcc` builds, in the same order: the definition list and the global area, where
//! each definition is placed when first met (QC-21), literals shared by value (QC-20), each
//! body written from the layer by the passes its dialect's profile chooses (`back_end.rs`:
//! allocation, QC-21.5, and layout, QC-21.4 and FTE-21.9), the functions in the order their
//! definitions end (QC-25.2) and the string table (QC-25.4); then `progdefs.h`, whose CRC the
//! header carries, and the tables of the progs (QC-25). The behaviour is ported from
//! `qcc/pr_comp.c` and `qcc/qcc.c` at
//! id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`, function by function as
//! `docs/spec/inventory-l4.md` lists. For every program id's `qcc` compiles, the progs is the
//! one `qcc` writes, byte for byte, but where QC-27 says otherwise.
//!
//! The type rules of expressions, and the errors they give, are the semantic model's (QC-16 to
//! QC-19, QCD-4).
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

mod back_end;
mod compile;
mod names;
mod ops;
mod precache;
mod program;
mod write;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use qc_diag::{Severity, Span};
use qc_lexer::LineIndex;
use qc_lno::LnoFile;
use qc_progs::{Format, ProgsFile, WriteError};
use qc_sema::lower::{FileDecls, lower_file};
use qc_sema::{Db, FileDiagnostic, FileId, ModuleInput, ResolveOptions, resolve_module};
use qc_syntax::{Dialect, Parse};

use crate::compile::Compiler;
use crate::names::Names;

pub use precache::Precache;
pub(crate) use precache::Precaches;

/// One compiled file: the name its functions record (`s_file`, QC-25.4) and its syntax tree,
/// the text the compiler reads after preprocessing (QC-9.16).
#[derive(Clone, Copy, Debug)]
pub struct Unit<'a> {
    /// The file's name as the progs records it.
    pub name: &'a str,
    /// The parse of its text.
    pub parse: &'a Parse,
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

impl Compiled {
    /// The name of the function holding `statement`, the last whose first statement is at or
    /// before it, and the source line the line map gives the statement (LNO-1).
    #[must_use]
    pub fn function_and_line(&self, statement: usize) -> (Option<String>, Option<i32>) {
        let progs = &self.progs;
        let function = i32::try_from(statement).ok().and_then(|at| {
            progs
                .functions
                .iter()
                .filter(|f| f.first_statement > 0 && f.first_statement <= at)
                .max_by_key(|f| f.first_statement)
        });
        let name = function
            .and_then(|f| usize::try_from(f.s_name).ok())
            .and_then(|at| progs.strings.get(at..))
            .and_then(|rest| rest.split(|&b| b == 0).next())
            .map(|name| String::from_utf8_lossy(name).into_owned());
        (name, self.lno.lines.get(statement).copied())
    }
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
    /// A body the semantic model did not follow in full, which no statement of the
    /// intermediate layer writes (`docs/design/back-end.md`).
    Unfollowed,
    /// A call that passes, or stops before, a parameter default this compiler keeps and the
    /// semantic model did not know of (FTE-18.7, FTE-18.8).
    DefaultUnknown,
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
pub fn compile(units: &[Unit<'_>]) -> Result<Compiled, CompileError> {
    let decls: Vec<FileDecls> = units
        .iter()
        .map(|u| lower_file(u.parse, Dialect::Vanilla))
        .collect();
    let vanilla = ResolveOptions {
        dialect: Dialect::Vanilla,
        ..ResolveOptions::default()
    };
    // Each unit is indexed by line once, the first time a statement comes from it.
    let mut lines: HashMap<FileId, LineIndex> = HashMap::new();
    compile_lowered(units, &decls, vanilla, |file, at| {
        let Some(unit) = units.get(file.index()) else {
            return 0;
        };
        let index = lines
            .entry(file)
            .or_insert_with(|| LineIndex::new(&unit.parse.syntax().text()));
        line_at(index, at)
    })
}

/// [`compile`] of units whose parses are lowered already, `decls[i]` from `units[i]`.
fn compile_lowered(
    units: &[Unit<'_>],
    decls: &[FileDecls],
    resolve: ResolveOptions,
    mut line_of: impl FnMut(FileId, usize) -> i32,
) -> Result<Compiled, CompileError> {
    let index = resolve_module(decls, resolve);
    let mut errors: Vec<FileDiagnostic> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        errors.extend(unit.parse.diagnostics().iter().map(|d| FileDiagnostic {
            file: FileId::new(i),
            diagnostic: d.clone(),
        }));
    }
    errors.extend(index.diagnostics().iter().cloned());
    let (errors, warnings): (Vec<_>, Vec<_>) = errors
        .into_iter()
        .partition(|d| d.diagnostic.code.severity() == Severity::Error);
    if !errors.is_empty() {
        return Err(CompileError::Diagnostics(errors));
    }
    let names = Names::new(&index);
    let mut compiler = Compiler::new(&index, &names);
    compiler.dialect = resolve.dialect;
    compiler.back_end = back_end::BackEnd::of(resolve.dialect);
    compiler.autoproto = resolve.autoproto;
    if resolve.dialect.has_fteqcc_extensions() && resolve.autoproto {
        for (i, unit) in units.iter().enumerate() {
            compiler
                .prototype_file(FileId::new(i), unit.parse)
                .map_err(|(internal, file, span)| CompileError::Internal {
                    file,
                    span,
                    internal,
                })?;
        }
    }
    if resolve.dialect == Dialect::Fteqcc {
        compiler.program.literal_sharing = program::LiteralSharing::Constants;
    }
    for (i, unit) in units.iter().enumerate() {
        compiler
            .compile_file(FileId::new(i), unit.name, unit.parse)
            .map_err(|(internal, file, span)| CompileError::Internal {
                file,
                span,
                internal,
            })?;
    }
    // FTE-21.9: every function's block of private locals is laid out after the globals,
    // statics and immediates of the module, before the array functions' own; under id's layout
    // there is none.
    compiler
        .program
        .resolve_blocks()
        .map_err(|internal| CompileError::Internal {
            file: FileId::new(0),
            span: Span::new(0, 0),
            internal,
        })?;
    // FTE-21.8: then the module's shared temporaries, as `QCC_FinaliseTemps` follows
    // `QCC_UnmarshalLocals`.
    compiler
        .program
        .resolve_shared_temps()
        .map_err(|internal| CompileError::Internal {
            file: FileId::new(0),
            span: Span::new(0, 0),
            internal,
        })?;
    compiler
        .emit_array_functions()
        .map_err(|(internal, file, span)| CompileError::Internal {
            file,
            span,
            internal,
        })?;
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
    let records = if compiler.dialect == Dialect::Fteqcc {
        write::Records::Fteqcc
    } else {
        write::Records::Qcc
    };
    let mut progs = write::progs(compiler.program, crc, &index, records).map_err(|internal| {
        CompileError::Internal {
            file: FileId::new(0),
            span: Span::new(0, 0),
            internal,
        }
    })?;
    if compiler.dialect == Dialect::Fteqcc {
        if write::needs_fte32(&progs) {
            progs.format = Format::Fte32;
        } else {
            write::wrap_short_operands(&mut progs);
        }
    }
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
    // Each source file is indexed by line once, the first time a statement comes from it.
    let mut lines: HashMap<FileId, LineIndex> = HashMap::new();
    let mut compiled = compile_lowered(&units, &decls, analysis.options, |file, at| {
        // QC-27.19: the line in the source file the expansion's text came from (QC-9.7).
        let id = input.compiled.get(file.index()).copied().unwrap_or(file);
        let (source, at) = db
            .expanded_origins(id)
            .and_then(|origins| origins.locate(Span::new(at, at)))
            .map_or((id, at), |location| (location.file, location.span.start));
        let index = lines
            .entry(source)
            .or_insert_with(|| LineIndex::new(db.file_text(source)));
        line_at(index, at)
    })
    .map_err(|error| locate(db, input, error))?;
    // The analysis's warnings are at their source spans.
    compiled.warnings = warnings;
    Ok(compiled)
}

/// `error` with each file and span of a unit's text located in its source file (QC-9.7).
fn locate(db: &Db, input: &ModuleInput, error: CompileError) -> CompileError {
    let at = |file: FileId, span: Span| {
        let id = input.compiled.get(file.index()).copied().unwrap_or(file);
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

/// The line of byte `at` of the file `index` indexes, counted from 1 (LNO-3.1), as the .lno
/// holds it.
fn line_at(index: &LineIndex, at: usize) -> i32 {
    i32::try_from(index.line(at)).unwrap_or(i32::MAX)
}
