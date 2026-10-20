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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{main, progs_src_names};
    use crate::test_dirs::fresh;

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = fresh(&format!("lex-corpus/{name}"));
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn run(dir: &Path) -> Result<bool, String> {
        main(Path::new("."), &[dir.display().to_string()])
    }

    #[test]
    fn a_clean_codebase_lexes_and_preprocesses() {
        let dir = codebase(
            "lex-clean",
            &[
                ("progs.src", "../progs.dat // out\n\"defs.qc\"\nworld.qc\n"),
                ("defs.qc", "float x;\n"),
                ("world.qc", "void() main = { x = 1; };\n"),
            ],
        );
        assert!(run(&dir).unwrap());
        assert_eq!(
            progs_src_names(&dir.join("progs.src")).unwrap(),
            ["defs.qc", "world.qc"]
        );
    }

    #[test]
    fn a_lexical_or_preprocessor_error_fails() {
        let lexical = codebase("lex-error", &[("bad.qc", "string s = \"open;\n")]);
        assert!(!run(&lexical).unwrap());
        let preprocessor = codebase(
            "lex-preprocess-error",
            &[("progs.src", "progs.dat\nbad.qc\n"), ("bad.qc", "#endif\n")],
        );
        assert!(!run(&preprocessor).unwrap());
        let missing = codebase(
            "lex-missing",
            &[
                ("progs.src", "progs.dat\nnowhere.qc\n"),
                ("a.qc", "float x;\n"),
            ],
        );
        assert!(run(&missing).is_err());
    }

    #[test]
    fn a_directory_without_qc_files_or_the_wrong_arguments_is_an_error() {
        let empty = codebase("lex-empty", &[("readme.txt", "x")]);
        assert!(run(&empty).unwrap_err().contains("no .qc files"));
        assert!(run(&empty.join("nowhere")).is_err());
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        assert!(progs_src_names(&empty.join("progs.src")).is_err());
    }
}
