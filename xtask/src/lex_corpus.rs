// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask lex-corpus <dir>`: lexes every `.qc` file of a QuakeC codebase and
//! preprocesses the files its `progs.src` lists, failing on any error (P1-20).
//!
//! The codebase comes from `cargo xtask corpus fetch id-1.01`, which writes id's QuakeC 1.01
//! at its pinned commit.

use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{LineIndex, lex};
use qc_preproc::{FsLoader, Preprocessor};

/// Runs the check on the codebase directory in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let [dir] = args else {
        return Err("usage: cargo xtask lex-corpus <codebase directory>".to_owned());
    };
    let dir = Path::new(dir);
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("qc")))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .qc files in {}", dir.display()));
    }
    let mut ok = true;
    let mut tokens = 0_usize;
    for path in &files {
        let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let lexed = lex(&text);
        tokens = tokens.saturating_add(lexed.tokens.len());
        let lines = LineIndex::new(&text);
        for d in lexed.diagnostics.iter().filter(|d| d.code.is_error()) {
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
    println!("lex-corpus: lexed {} files, {tokens} tokens", files.len());
    let progs_src = dir.join("progs.src");
    if progs_src.is_file() {
        ok &= preprocess(dir, &progs_src)?;
    }
    println!(
        "lex-corpus: {}",
        if ok { "no errors" } else { "errors found" }
    );
    Ok(ok)
}

/// The source files `progs.src` lists, in order, without the output file (QC-23.1).
pub(crate) fn progs_src_names(progs_src: &Path) -> Result<Vec<String>, String> {
    let list =
        fs::read_to_string(progs_src).map_err(|e| format!("{}: {e}", progs_src.display()))?;
    Ok(list
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .flat_map(str::split_whitespace)
        .map(|word| word.trim_matches('"').to_owned())
        .skip(1)
        .collect())
}

/// Preprocesses the source files `progs.src` lists, in order (QC-23.3).
fn preprocess(dir: &Path, progs_src: &Path) -> Result<bool, String> {
    let names = progs_src_names(progs_src)?;
    let mut pp = Preprocessor::new(FsLoader);
    let mut ok = true;
    for name in &names {
        let out = pp
            .preprocess_file(&dir.join(name))
            .map_err(|e| format!("{name}: {e}"))?;
        for d in out.diagnostics.iter().filter(|d| d.code.is_error()) {
            ok = false;
            let sources = pp.sources();
            let path = sources
                .file(d.span.file)
                .map_or_else(PathBuf::new, |f| f.path.clone());
            let line = sources.line(d.span).unwrap_or(0);
            println!(
                "FAIL  {}:{line}: {} {}",
                path.display(),
                d.code.as_str(),
                d.message
            );
        }
    }
    println!(
        "lex-corpus: preprocessed the {} files of {}",
        names.len(),
        progs_src.display()
    );
    Ok(ok)
}
