// SPDX-License-Identifier: GPL-3.0-or-later

//! Building and checking the open project: `project.build` compiles each module with
//! `qc-codegen` into its program and line map (PRJ-3.2, LNO-1), and `project.check` reports
//! its problems, so the toolbar, the CLI and the agent build and check it the same way.

use std::path::{Path, PathBuf};

use qc_codegen::{CompileError, Compiled, compile_module};
use qc_engines::{Engine, Unloadable};
use qc_ide::ModuleBuild;
use qc_progs::{Progs, ReadOptions};
use qc_project::Loader;
use qc_sema::FileDiagnostic;
use serde::{Deserialize, Serialize};

use crate::commands::Place;
use crate::{Location, Workbench, WorkbenchError};

/// How serious a reported problem is (QCD-1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gravity {
    /// It stops the build.
    Error,
    /// It does not.
    Warning,
}

/// One problem a build or a check reports: where it is, when it is in a file, how serious it
/// is, its code and its message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    /// Where it is; `None` for a problem of the whole module.
    pub place: Option<Place>,
    /// How serious it is.
    pub gravity: Gravity,
    /// Its code (QCD-1.1), or `internal` for the compiler's own failure.
    pub code: String,
    /// What it says.
    pub message: String,
}

/// What building one module did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleBuilt {
    /// The module's name (PRJ-2.4).
    pub name: String,
    /// Where its program was written, or would have been (PRJ-3.2).
    pub output: PathBuf,
    /// Whether it compiled and was written.
    pub built: bool,
    /// Its errors and warnings.
    pub problems: Vec<Reported>,
}

impl<L: Loader> Workbench<L> {
    /// Compiles each module of the open project, the editor's text of each open file
    /// included, and writes its program to its output and its line map beside it, the
    /// output with the extension `lno` (PRJ-3.2, LNO-1). A module with errors writes nothing.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`] when no project is open, and [`WorkbenchError::Save`]
    /// when a program or a line map cannot be written.
    pub fn build(&mut self) -> Result<Vec<ModuleBuilt>, WorkbenchError> {
        if self.root.is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let mut built = Vec::new();
        for module in self.host.project_modules() {
            built.push(build_module(module)?);
        }
        Ok(built)
    }

    /// Every problem of the open project, as the Problems panel lists them.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`] when no project is open.
    pub fn check(&self) -> Result<Vec<Reported>, WorkbenchError> {
        if self.root.is_none() {
            return Err(WorkbenchError::NoProject);
        }
        Ok(self
            .problems
            .iter()
            .map(|problem| Reported {
                place: Some(Place {
                    path: problem.path.clone(),
                    start: problem.diagnostic.span.start,
                    end: problem.diagnostic.span.end,
                    location: problem.location,
                }),
                gravity: gravity(problem.diagnostic.is_error()),
                code: problem.diagnostic.code.as_str().to_owned(),
                message: problem.diagnostic.message.clone(),
            })
            .collect())
    }
}

/// [`Gravity::Error`] for an error, else [`Gravity::Warning`].
const fn gravity(is_error: bool) -> Gravity {
    if is_error {
        Gravity::Error
    } else {
        Gravity::Warning
    }
}

/// Compiles `module` and writes what it compiled to.
fn build_module(module: ModuleBuild<'_>) -> Result<ModuleBuilt, WorkbenchError> {
    let ModuleBuild {
        name,
        output,
        db,
        input,
        names,
        engine,
    } = module;
    let compiled = compile_module(db, input, |id| {
        names.get(id.index()).cloned().unwrap_or_default()
    });
    let report = |diagnostic: &FileDiagnostic| {
        let text = db.file_text(diagnostic.file);
        Reported {
            place: db.file_path(diagnostic.file).map(|path| Place {
                path: path.to_path_buf(),
                start: diagnostic.diagnostic.span.start,
                end: diagnostic.diagnostic.span.end,
                location: Location::of(text, diagnostic.diagnostic.span.start),
            }),
            gravity: gravity(diagnostic.diagnostic.is_error()),
            code: diagnostic.diagnostic.code.as_str().to_owned(),
            message: diagnostic.diagnostic.message.clone(),
        }
    };
    let (built, problems) = match compiled {
        Ok(compiled) => {
            let mut problems: Vec<Reported> = compiled.warnings.iter().map(report).collect();
            let progs = program_bytes(output, &compiled)?;
            match unloadable(engine, &progs) {
                // ENGT-3.12: a program its engine cannot load is not written.
                Some(why) => {
                    problems.push(Reported {
                        place: None,
                        gravity: Gravity::Error,
                        code: "target".to_owned(),
                        message: why.to_string(),
                    });
                    (false, problems)
                }
                None => {
                    write_program(output, &progs, &compiled)?;
                    (true, problems)
                }
            }
        }
        Err(CompileError::Diagnostics(errors)) => (false, errors.iter().map(report).collect()),
        Err(error) => (
            false,
            vec![Reported {
                place: None,
                gravity: Gravity::Error,
                code: "internal".to_owned(),
                message: format!("the compiler stopped: {error:?}"),
            }],
        ),
    };
    Ok(ModuleBuilt {
        name: name.to_owned(),
        output: output.to_path_buf(),
        built,
        problems,
    })
}

/// `compiled`'s program as `output` will hold it.
fn program_bytes(output: &Path, compiled: &Compiled) -> Result<Vec<u8>, WorkbenchError> {
    qc_progs::write(&compiled.progs).map_err(|e| WorkbenchError::Save {
        path: output.to_path_buf(),
        error: std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:?}")),
    })
}

/// Why `engine` cannot load `progs`, the bytes of a program (ENGT-3.12).
fn unloadable(engine: Engine, progs: &[u8]) -> Option<Unloadable> {
    // The engine's own checks, the header's CRC among them, are not this one's.
    let options = ReadOptions {
        check_header_crc: false,
    };
    let read = Progs::read_with(progs, &options).ok()?;
    let version = read.header().version;
    engine.unloadable(version, read.statements().iter().map(|s| s.op))
}

/// Writes `progs`, `compiled`'s program, to `output` and its line map beside it, making the
/// directory.
fn write_program(output: &Path, progs: &[u8], compiled: &Compiled) -> Result<(), WorkbenchError> {
    let save = |path: &Path, error| WorkbenchError::Save {
        path: path.to_path_buf(),
        error,
    };
    let invalid = |error: String| std::io::Error::new(std::io::ErrorKind::InvalidData, error);
    let lno_path = output.with_extension("lno");
    let lno = compiled
        .lno
        .write()
        .map_err(|e| save(&lno_path, invalid(format!("{e:?}"))))?;
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir).map_err(|e| save(dir, e))?;
    }
    std::fs::write(output, progs).map_err(|e| save(output, e))?;
    std::fs::write(&lno_path, lno).map_err(|e| save(&lno_path, e))
}

#[cfg(test)]
mod tests;
