// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-profile <dir>...`: the acceptance check for the FTEQCC dialect profile
//! (P2-03). For each rerelease codebase directory it opens the project, and for every module
//! parses and resolves the module's files in compilation order under the FTEQCC profile
//! (`qc_sema::Db` with `Dialect::Fteqcc`), failing if any file has a lexical, syntax or
//! semantic error. It then checks the vanilla profile reports each extension as a diagnostic,
//! so the two profiles are observably different (the issue's acceptance criterion).
//!
//! The codebases come from `cargo xtask corpus fetch rerelease rerelease-hipnotic
//! rerelease-rogue rerelease-mg1 rerelease-ctf rerelease-mg3`. The FTEQCC subset each one uses
//! is recorded in `docs/spec/vectors/dialect-fteqcc-rerelease/coverage.txt` and pinned by the
//! coverage check (`fteqcc_coverage`); this check adds that they parse and resolve clean.

use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::LineIndex;
use qc_project::{Dialect, open};
use qc_sema::{Db, FileId};
use qc_syntax::parse_with;

const USAGE: &str = "usage: cargo xtask fteqcc-profile <dir>...";

/// Runs the check on the rerelease codebase directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let dirs: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut ok = true;
    for dir in &dirs {
        ok &= profile_one(dir)?;
    }
    // The vanilla profile must reject every extension with a diagnostic, so the profiles
    // differ (the second half of the acceptance criterion).
    ok &= vanilla_rejects_every_extension();
    println!(
        "fteqcc-profile: {}",
        if ok {
            "every rerelease module parses and resolves under the FTEQCC profile"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Parses and resolves every module of the codebase in `dir` under the FTEQCC profile.
fn profile_one(dir: &Path) -> Result<bool, String> {
    let project = open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut ok = true;
    let mut modules = 0_usize;
    for module in &project.modules {
        modules = modules.saturating_add(1);
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc.into());
        let mut ids = Vec::new();
        for (index, file) in module.graph.files.iter().enumerate() {
            let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
            let id = FileId(index);
            db.set_file_text(id, text);
            ids.push(id);
        }
        // Per-file parse errors.
        for (index, file) in module.graph.files.iter().enumerate() {
            let text = db.file_text(FileId(index)).to_vec();
            let parsed = parse_with(&text, Dialect::Fteqcc.into());
            let lines = LineIndex::new(&text);
            for d in parsed.errors() {
                ok = false;
                let line = lines.line(d.span.start);
                println!(
                    "FAIL  {}:{line}: parse {} {}",
                    file.path.display(),
                    d.code.as_str(),
                    d.message
                );
            }
        }
        // Whole-module resolution errors.
        let index = db.module_index(&ids);
        for d in &index.diagnostics {
            if !d.code.is_error() {
                continue;
            }
            ok = false;
            println!(
                "FAIL  module {}: resolve {} {} @{}..{}",
                module.name,
                d.code.as_str(),
                d.message,
                d.span.start,
                d.span.end
            );
        }
    }
    println!(
        "fteqcc-profile: {}: {modules} modules checked",
        dir.display()
    );
    Ok(ok)
}

/// Each FTEQCC extension, under the vanilla profile, must produce at least one diagnostic, so
/// the vanilla and FTEQCC profiles are observably different (the acceptance criterion).
fn vanilla_rejects_every_extension() -> bool {
    let cases: &[(&str, &str)] = &[
        (
            "for",
            "void() f = { for (a = 0; a < 3; a = a + 1) b = 1; };",
        ),
        ("switch", "void() f = { switch (a) { case 0: b = 1; } };"),
        ("break", "void() f = { while (a) break; };"),
        ("continue", "void() f = { while (a) continue; };"),
        ("++", "void() f = { i++; };"),
        ("+=", "void() f = { a += 1; };"),
        ("%", "float a = b % c;"),
        ("?:", "float a = b ? c : d;"),
        ("const", "const float A = 1;"),
        ("var", "var float a = 0;"),
        ("nosave", "nosave float a;"),
        ("array", "float a[3];"),
        ("typedef struct", "typedef struct { float f; } S;"),
        ("enum", "enum { A, B };"),
        ("#pragma", "#pragma warning disable F317"),
    ];
    let mut ok = true;
    for (name, source) in cases {
        let vanilla_has_error = parse_with(source.as_bytes(), qc_syntax::Dialect::Vanilla)
            .errors()
            .next()
            .is_some();
        // `#pragma` is a preprocessor directive the parser treats as trivia, so it has no
        // parser error; it is rejected by the vanilla preprocessor (QC-9.9), checked in
        // qc-preproc. Every other extension must be a parser or lexer diagnostic.
        if *name == "#pragma" {
            continue;
        }
        if !vanilla_has_error {
            ok = false;
            println!("FAIL  vanilla profile accepts the {name} extension");
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{main, vanilla_rejects_every_extension};

    /// Builds a throwaway codebase directory with a `progs.src` and files.
    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/xtask-tests")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_clean_fteqcc_codebase_passes() {
        let dir = codebase(
            "fteqcc-profile-ok",
            &[
                ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
                (
                    "defs.qc",
                    "const float MAX = 3;\n\
                     enum { IT_AXE, IT_SHOTGUN };\n\
                     var float num_players = 0;\n\
                     nosave float load_flag;\n\
                     const string names[] = { \"a\", \"b\", \"c\" };\n",
                ),
                (
                    "world.qc",
                    "float have, i, sum;\n\
                     void() f = {\n\
                         have = IT_AXE;\n\
                         for (float j = 0; j < MAX; j++) {\n\
                             sum += j % 2;\n\
                             switch (j) { case 0: continue; default: break; }\n\
                         }\n\
                         have = num_players ? 1 : 0;\n\
                         load_flag = sum;\n\
                     };\n",
                ),
            ],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
    }

    #[test]
    fn a_syntax_error_fails() {
        let dir = codebase(
            "fteqcc-profile-bad",
            &[
                ("progs.src", "progs.dat\nbad.qc\n"),
                ("bad.qc", "void() f = { a = ; };\n"),
            ],
        );
        assert!(!main(Path::new("."), &[dir.display().to_string()]).unwrap());
    }

    #[test]
    fn bad_arguments_are_an_error() {
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
    }

    #[test]
    fn the_vanilla_profile_rejects_every_extension() {
        assert!(vanilla_rejects_every_extension());
    }
}
