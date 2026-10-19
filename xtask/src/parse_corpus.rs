// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask parse-corpus [--vanilla] [--fteqcc] [--deletions] <dir>...`: parses every `.qc`
//! and `.qh` file under each codebase directory and checks that the tree gives back the file
//! byte for byte (P1-21). With `--vanilla`, any lexical or syntax error in a file the
//! codebase's `progs.src` lists fails (the other files are not part of the program, QC-23.6);
//! with `--fteqcc`, each file is parsed under the FTEQCC profile rather than the vanilla one
//! (P2-51); with `--deletions`, each token of each file is deleted in turn and the damage must
//! stay local (`qc_syntax::recovery`), in the profile the files are parsed in.
//!
//! The codebases come from `cargo xtask corpus fetch <id>`.

use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{Dialect, LineIndex};
use qc_syntax::parse_with;
use qc_syntax::recovery::check_deletions_with;

use crate::lex_corpus::progs_src_names;

#[derive(Debug, Default)]
struct Options {
    vanilla: bool,
    fteqcc: bool,
    deletions: bool,
    dirs: Vec<PathBuf>,
}

const USAGE: &str = "usage: cargo xtask parse-corpus [--vanilla] [--fteqcc] [--deletions] <dir>...";

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--vanilla" => options.vanilla = true,
            "--fteqcc" => options.fteqcc = true,
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
    let dialect = if options.fteqcc {
        Dialect::Fteqcc
    } else {
        Dialect::Vanilla
    };
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
            let parsed = parse_with(&text, dialect);
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
                let report = check_deletions_with(&text, dialect);
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::main;

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/xtask-tests")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(dir.join("sub")).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn run(flags: &[&str], dir: &Path) -> Result<bool, String> {
        let mut args: Vec<String> = flags.iter().map(|flag| (*flag).to_owned()).collect();
        args.push(dir.display().to_string());
        main(Path::new("."), &args)
    }

    #[test]
    fn a_codebase_round_trips_and_its_deletions_stay_local() {
        let dir = codebase(
            "parse-clean",
            &[
                ("progs.src", "progs.dat\nworld.qc\n"),
                ("world.qc", "float x;\nvoid() main = { x = 1; };\n"),
                ("sub/defs.qh", "float y;\n"),
            ],
        );
        assert!(run(&["--vanilla", "--deletions"], &dir).unwrap());
        assert!(run(&[], &dir).unwrap());
    }

    #[test]
    fn a_syntax_error_fails_only_a_listed_file_under_vanilla() {
        let dir = codebase(
            "parse-error",
            &[
                ("progs.src", "progs.dat\nbad.qc\n"),
                ("bad.qc", "void() main = { x = ; };\n"),
                ("unlisted.qc", "float = ;\n"),
            ],
        );
        assert!(!run(&["--vanilla"], &dir).unwrap());
        assert!(
            run(&[], &dir).unwrap(),
            "without --vanilla an error is counted, not failed"
        );
        fs::write(dir.join("progs.src"), "progs.dat\n").unwrap();
        assert!(
            run(&["--vanilla"], &dir).unwrap(),
            "the error is in no listed file"
        );
    }

    #[test]
    fn fteqcc_files_parse_and_delete_under_the_fteqcc_profile() {
        let dir = codebase(
            "parse-fteqcc",
            &[
                ("progs.src", "progs.dat\nworld.qc\n"),
                (
                    "world.qc",
                    "float x;\nvoid() main = { for (int i = 0; i < 3; i++) x += i; };\n",
                ),
            ],
        );
        assert!(run(&["--fteqcc", "--deletions"], &dir).unwrap());
        assert!(run(&["--fteqcc", "--vanilla"], &dir).unwrap());
        assert!(
            !run(&["--vanilla"], &dir).unwrap(),
            "the vanilla profile rejects the FTEQCC forms"
        );
    }

    #[test]
    fn bad_arguments_and_empty_directories_are_errors() {
        let dir = codebase("parse-empty", &[("readme.txt", "x")]);
        assert!(run(&[], &dir).unwrap_err().contains("no .qc or .qh"));
        assert!(run(&["--fast"], &dir).unwrap_err().contains("unknown flag"));
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        let unlisted = codebase("parse-no-src", &[("a.qc", "float x;\n")]);
        assert!(
            run(&["--vanilla"], &unlisted)
                .unwrap_err()
                .contains("needs")
        );
        assert!(run(&[], &dir.join("nowhere")).is_err());
    }
}
