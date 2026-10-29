// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask codegen-corpus <dir>...`: compiles every module of each codebase with
//! `qc-codegen` under the FTEQCC profile (P2-07, COMP-3), as `fteqcc-profile` opens them, and
//! fails on the first module that does not compile, printing why at its file and line.

use std::fs;
use std::path::{Path, PathBuf};

use qc_codegen::{CompileError, Options, compile_module};
use qc_lexer::LineIndex;
use qc_project::{Dialect, Module, open};
use qc_sema::{Db, FileId, ModuleInput};

use crate::oracles::rerelease::autoprototypes;

const USAGE: &str = "usage: cargo xtask codegen-corpus <codebase directory>...";

/// Runs `cargo xtask codegen-corpus` with the arguments `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    if args.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut ok = true;
    for dir in args.iter().map(PathBuf::from) {
        let project = open(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let autoproto = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(autoprototypes);
        for module in &project.modules {
            match compile(module, autoproto)? {
                Ok(statements) => println!(
                    "ok    {} {}: {statements} statements",
                    dir.display(),
                    module.name
                ),
                Err(problem) => {
                    ok = false;
                    println!("FAIL  {} {}: {problem}", dir.display(), module.name);
                }
            }
        }
    }
    println!(
        "codegen-corpus: {}",
        if ok {
            "every module compiles"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Compiles `module` under the FTEQCC profile: the progs' statement count, or why it did not
/// compile. `Err` only when a file cannot be read.
fn compile(module: &Module, autoproto: bool) -> Result<Result<usize, String>, String> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc.into());
    db.set_autoproto(autoproto);
    for (index, file) in module.graph.files.iter().enumerate() {
        let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        db.set_file(FileId(index), file.path.clone(), text);
    }
    let names: Vec<String> = module
        .graph
        .files
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();
    let input = ModuleInput::from_module(module);
    let result = compile_module(
        &mut db,
        &input,
        |id| names.get(id.0).cloned().unwrap_or_default(),
        Options::default(),
    );
    Ok(match result {
        Ok(compiled) => Ok(compiled.progs.statements.len()),
        Err(error) => Err(describe(&db, &error)),
    })
}

/// Why a module did not compile, the first problem at its file and line.
fn describe(db: &Db, error: &CompileError) -> String {
    let place = |file: FileId, at: usize| {
        let path = db.file_path(file).unwrap_or_else(|| Path::new("?"));
        let line = LineIndex::new(db.file_text(file)).line(at);
        format!("{}:{line}", path.display())
    };
    match error {
        CompileError::Diagnostics(diagnostics) => {
            let count = diagnostics.len();
            diagnostics.first().map_or_else(
                || "no diagnostic".to_owned(),
                |d| {
                    format!(
                        "{} {} {} ({count} errors)",
                        place(d.file, d.diagnostic.span.start),
                        d.diagnostic.code.as_str(),
                        d.diagnostic.message
                    )
                },
            )
        }
        CompileError::Internal {
            file,
            span,
            internal,
        } => format!("{} {internal:?}", place(*file, span.start)),
        other => format!("{other:?}"),
    }
}
