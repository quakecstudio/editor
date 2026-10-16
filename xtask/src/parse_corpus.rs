// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask parse-corpus [--vanilla] [--deletions] <dir>...`: parses every `.qc` and `.qh`
//! file under each codebase directory and checks that the tree gives back the file byte for
//! byte (P1-21). With `--vanilla`, any lexical or syntax error in a file the codebase's
//! `progs.src` lists fails (the other files are not part of the program, QC-23.6); with
//! `--deletions`, each token of each file is deleted in turn and the damage must stay local
//! (`qc_syntax::recovery`).
//!
//! The codebases come from `cargo xtask corpus fetch <id>`.

use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::LineIndex;
use qc_syntax::parse;
use qc_syntax::recovery::check_deletions;

use crate::lex_corpus::progs_src_names;

#[derive(Debug, Default)]
struct Options {
    vanilla: bool,
    deletions: bool,
    dirs: Vec<PathBuf>,
}

const USAGE: &str = "usage: cargo xtask parse-corpus [--vanilla] [--deletions] <dir>...";

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--vanilla" => options.vanilla = true,
            "--deletions" => options.deletions = true,
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            dir => options.dirs.push(PathBuf::from(dir)),
        }
    }
    if options.dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(options)
}

/// Every `.qc` and `.qh` file under `dir`, sorted.
fn sources(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("qc") || x.eq_ignore_ascii_case("qh"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Runs the check on the directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let options = options(args)?;
    let mut ok = true;
    for dir in &options.dirs {
        let files = sources(dir)?;
        if files.is_empty() {
            return Err(format!("no .qc or .qh files under {}", dir.display()));
        }
        let progs_src = dir.join("progs.src");
        let listed: Vec<PathBuf> = if options.vanilla {
            if !progs_src.is_file() {
                return Err(format!("--vanilla needs {}", progs_src.display()));
            }
            progs_src_names(&progs_src)?
                .iter()
                .map(|name| dir.join(name))
                .collect()
        } else {
            Vec::new()
        };
        let (mut bytes, mut errors, mut deletions, mut silent) =
            (0_usize, 0_usize, 0_usize, 0_usize);
        for path in &files {
            let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            bytes = bytes.saturating_add(text.len());
            let parsed = parse(&text);
            if parsed.syntax().text() != text {
                ok = false;
                println!(
                    "FAIL  {}: the tree does not give back the file",
                    path.display()
                );
            }
            let lines = LineIndex::new(&text);
            for d in parsed.errors() {
                errors = errors.saturating_add(1);
                if listed.contains(path) {
                    ok = false;
                    let line = lines.line(d.span.start);
                    println!(
                        "FAIL  {}:{line}: {} {}",
                        path.display(),
                        d.code.as_str(),
                        d.message
                    );
                }
            }
            if options.deletions {
                let report = check_deletions(&text);
                deletions = report.deletions.saturating_add(deletions);
                silent = report.silent.saturating_add(silent);
                for failure in &report.failures {
                    ok = false;
                    println!("FAIL  {}: {failure}", path.display());
                }
            }
        }
        println!(
            "parse-corpus: {}: {} files, {bytes} bytes, round trip checked, {errors} errors",
            dir.display(),
            files.len()
        );
        if options.deletions {
            println!(
                "parse-corpus: {}: {deletions} single-token deletions, {silent} left no error",
                dir.display()
            );
        }
    }
    println!(
        "parse-corpus: {}",
        if ok { "no failures" } else { "failures found" }
    );
    Ok(ok)
}
