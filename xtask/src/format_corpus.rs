// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask format-corpus [--vanilla] <dir>...`: the QuakeC formatter's acceptance check
//! (P4-34, UI-18). Every file of each module of each codebase is formatted
//! (`qc_ide::format`), and formatting it again changes nothing; then each module is compiled
//! from its files as they are and as formatted, and the two `progs.dat` are the same bytes.
//! Nothing is written to the codebase.

use std::fs;
use std::path::{Path, PathBuf};

use qc_codegen::compile_module;
use qc_project::{Dialect, Module, open};
use qc_sema::{Db, FileId, ModuleInput};

use crate::oracles::rerelease::autoprototypes;

/// `cargo xtask format-corpus [--vanilla] <codebase directory>...`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Read and compile the codebases in the vanilla dialect, as id 1.01 is; FTEQCC's without.
    #[arg(long)]
    vanilla: bool,
    /// The codebase directories.
    #[arg(required = true)]
    dirs: Vec<PathBuf>,
}

/// Runs `cargo xtask format-corpus` with the arguments `args`.
pub(crate) fn main(_root: &Path, args: &Args) -> Result<bool, String> {
    let bare = if args.vanilla {
        Dialect::Vanilla
    } else {
        Dialect::Fteqcc
    };
    let mut ok = true;
    for dir in &args.dirs {
        let declared = dir.join("quakec.toml").is_file();
        let project = open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let autoproto = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(autoprototypes);
        for module in &project.modules {
            let dialect = if declared { module.dialect } else { bare };
            let (files, changed, unstable) = formatted(module, dialect)?;
            for path in &unstable {
                ok = false;
                println!("FAIL  {}: formatting it again changes it", path.display());
            }
            let before = compile(module, dialect, autoproto, &files, false)?;
            let after = compile(module, dialect, autoproto, &files, true)?;
            let same = before == after;
            ok &= same && before.is_ok();
            println!(
                "{}  {} {}: {} files, {changed} changed by formatting, {}",
                if same && before.is_ok() {
                    "ok  "
                } else {
                    "FAIL"
                },
                dir.display(),
                module.name,
                files.len(),
                match (&before, same) {
                    (Err(problem), _) => format!("does not compile: {problem}"),
                    (Ok(bytes), true) => format!("progs.dat the same {} bytes", bytes.len()),
                    (Ok(_), false) => "progs.dat differs".to_owned(),
                }
            );
        }
    }
    println!(
        "format-corpus: {}",
        if ok {
            "formatting is stable and changes no progs.dat"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Each file of `module`, as it is and formatted; how many formatting changed, and the files
/// formatting again changes.
#[expect(clippy::type_complexity, reason = "a private result read in one place")]
fn formatted(
    module: &Module,
    dialect: Dialect,
) -> Result<(Vec<(PathBuf, Vec<u8>, Vec<u8>)>, usize, Vec<PathBuf>), String> {
    let mut files = Vec::new();
    let mut changed = 0_usize;
    let mut unstable = Vec::new();
    for file in &module.graph.files {
        let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        let once = qc_ide::format(&text, dialect.into());
        let twice = qc_ide::format(&once, dialect.into());
        if twice != once {
            unstable.push(file.path.clone());
        }
        if once != text {
            changed = changed.saturating_add(1);
        }
        files.push((file.path.clone(), text, once));
    }
    Ok((files, changed, unstable))
}

/// `module` compiled in `dialect` from `files`, formatted or not: the `progs.dat` bytes, or
/// why it did not compile.
fn compile(
    module: &Module,
    dialect: Dialect,
    autoproto: bool,
    files: &[(PathBuf, Vec<u8>, Vec<u8>)],
    formatted: bool,
) -> Result<Result<Vec<u8>, String>, String> {
    let mut db = Db::new();
    db.set_dialect(dialect.into());
    db.set_autoproto(autoproto);
    for (index, (path, text, once)) in files.iter().enumerate() {
        let bytes = if formatted { once } else { text };
        db.set_file(FileId::new(index), path.clone(), bytes.clone());
    }
    let names: Vec<String> = files
        .iter()
        .map(|(path, _, _)| path.display().to_string())
        .collect();
    let input = ModuleInput::from_module(module);
    let result = compile_module(&mut db, &input, |id| {
        names.get(id.index()).cloned().unwrap_or_default()
    });
    Ok(match result {
        Ok(compiled) => qc_progs::write(&compiled.progs).map_err(|e| e.to_string()),
        Err(error) => Err(format!("{error:?}").chars().take(200).collect()),
    })
}
