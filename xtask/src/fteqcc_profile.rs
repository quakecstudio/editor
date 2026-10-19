// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-profile <dir>...`: the acceptance check for the FTEQCC dialect profile
//! (P2-03). For each rerelease codebase directory it opens the project, and for every module
//! preprocesses the module's files in compilation order (`qc_preproc`, FTEQCC dialect), then
//! parses and resolves the expanded files under the FTEQCC profile (`qc_sema::Db` with
//! `Dialect::Fteqcc`, and autoprototyping for the codebase FTEQCC builds with
//! `-Fautoproto`), failing on any preprocessor, lexical, syntax or semantic error. It then
//! checks the vanilla profile reports each extension as a diagnostic, so the two profiles are
//! observably different (the issue's acceptance criterion). Errors are printed at the file and
//! line of the source token; a semantic error has only its offset in the expanded file.
//!
//! The codebases come from `cargo xtask corpus fetch rerelease rerelease-hipnotic
//! rerelease-rogue rerelease-mg1 rerelease-ctf rerelease-mg3`. The FTEQCC subset each one uses
//! is recorded in `docs/spec/vectors/dialect-fteqcc-rerelease/coverage.txt` and pinned by the
//! coverage check (`fteqcc_coverage`); this check adds that they parse and resolve clean.

use std::path::{Path, PathBuf};

use qc_lexer::TokenKind;
use qc_preproc::{FileSpan, FsLoader, Loader, PpToken, Preprocessor};
use qc_project::{Dialect, Form, Module, Via, open};
use qc_sema::{Db, FileId};
use qc_syntax::parse_with;

use crate::oracles::rerelease::autoprototypes;

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

/// One compiled file after preprocessing: the expanded text the parser reads, and for each
/// of its tokens the byte offset it starts at and where it came from.
struct Expanded {
    text: Vec<u8>,
    origins: Vec<(usize, FileSpan)>,
}

impl Expanded {
    /// Renders a preprocessed file as text: each token's own bytes (a resolved frame
    /// reference as its number), a line break wherever the token's origin moves to another
    /// line, and a space otherwise. A token that starts its line indented stays indented,
    /// since the parser's recovery treats a token in column 0 as the start of a top-level
    /// declaration (QCD-1.4).
    fn render<L: Loader>(pp: &Preprocessor<L>, tokens: &[PpToken]) -> Self {
        let mut out = Self {
            text: Vec::new(),
            origins: Vec::new(),
        };
        let mut last_line = None;
        for token in tokens {
            let origin = pp.origin(token);
            let line = pp.sources().line(origin);
            if line != last_line {
                if !out.text.is_empty() {
                    out.text.push(b'\n');
                }
                let file = pp.sources().file(origin.file);
                let indented = file
                    .and_then(|f| origin.span.start.checked_sub(1).and_then(|i| f.text.get(i)))
                    .is_some_and(|b| *b != b'\n');
                if indented {
                    out.text.push(b' ');
                }
            } else {
                out.text.push(b' ');
            }
            last_line = line;
            out.origins.push((out.text.len(), origin));
            let own = pp.sources().text(token.span).unwrap_or_default();
            match token.kind {
                TokenKind::Float(value) if own.first() == Some(&b'$') => {
                    out.text.extend_from_slice(value.to_string().as_bytes());
                }
                _ => out.text.extend_from_slice(own),
            }
        }
        out
    }

    /// The file and line of the token an offset of the rendered text belongs to.
    fn locate<L: Loader>(&self, pp: &Preprocessor<L>, offset: usize) -> String {
        let at = self.origins.partition_point(|(start, _)| *start <= offset);
        let Some((_, origin)) = at.checked_sub(1).and_then(|i| self.origins.get(i)) else {
            return "?".to_owned();
        };
        let path = pp
            .sources()
            .file(origin.file)
            .map_or_else(PathBuf::new, |f| f.path.clone());
        let line = pp.sources().line(*origin).unwrap_or(0);
        format!("{}:{line}", path.display())
    }
}

/// Parses and resolves every module of the codebase in `dir` under the FTEQCC profile.
///
/// The parser and the semantic model read each file as FTEQCC compiles it: after the
/// preprocessor (QC-9) has expanded its macros, dropped the untaken side of each conditional
/// and inlined its `#include`s. A file as written can use a macro as a statement
/// (`INHIBIT_COOP` in `rerelease-mg1`), which no parser of the unexpanded text accepts. The
/// macros of a source-form `progs.src` (PRJ-4.4) are defined by preprocessing it first; its
/// own lines are the graph's (PRJ-5), so its tokens are not parsed.
///
/// A codebase FTEQCC compiles with `-Fautoproto` (`rerelease-mg3`, FTE-1.2) is preprocessed
/// twice, as FTEQCC reads it twice (FTE-22.4), and resolved with every top-level name visible
/// to every function body (FTE-22.3).
fn profile_one(dir: &Path) -> Result<bool, String> {
    let project = open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let autoproto = dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(autoprototypes);
    let mut ok = true;
    let mut modules = 0_usize;
    for module in &project.modules {
        modules = modules.saturating_add(1);
        if !module.defines.is_empty() {
            return Err(format!(
                "{}: module {} has manifest defines, which this check does not apply",
                dir.display(),
                module.name
            ));
        }
        let mut pp = Preprocessor::new(FsLoader).with_dialect(Dialect::Fteqcc.into());
        if autoproto {
            preprocess_module(&mut pp, module)?;
            pp.restart();
        }
        let (expanded, pp_ok) = preprocess_module(&mut pp, module)?;
        ok &= pp_ok;
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc.into());
        db.set_autoproto(autoproto);
        let mut ids = Vec::new();
        for file_expanded in &expanded {
            let id = FileId(ids.len());
            db.set_file_text(id, file_expanded.text.clone());
            ids.push(id);
        }
        // Per-file parse errors.
        for file_expanded in &expanded {
            let parsed = parse_with(&file_expanded.text, Dialect::Fteqcc.into());
            for d in parsed.errors() {
                ok = false;
                println!(
                    "FAIL  {}: parse {} {}",
                    file_expanded.locate(&pp, d.span.start),
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

/// Preprocesses the files of `module` in compilation order, printing each preprocessor
/// error; returns the expanded files to parse and whether there was no error.
fn preprocess_module(
    pp: &mut Preprocessor<FsLoader>,
    module: &Module,
) -> Result<(Vec<Expanded>, bool), String> {
    let mut ok = true;
    let mut expanded = Vec::new();
    for file in &module.graph.files {
        if file.via == Via::Include {
            continue;
        }
        let out = pp
            .preprocess_file(&file.path)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        if file.via == Via::Root && module.graph.form == Some(Form::Source) {
            continue;
        }
        for d in out.diagnostics.iter().filter(|d| d.code.is_error()) {
            ok = false;
            let path = pp
                .sources()
                .file(d.span.file)
                .map_or_else(PathBuf::new, |f| f.path.clone());
            let line = pp.sources().line(d.span).unwrap_or(0);
            println!(
                "FAIL  {}:{line}: preprocess {} {}",
                path.display(),
                d.code.as_str(),
                d.message
            );
        }
        expanded.push(Expanded::render(pp, &out.tokens));
    }
    Ok((expanded, ok))
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
        ("C-style function", "void f(float a);"),
        ("body without =", "void() f { };"),
        ("named builtin", "void() f = #0:ex_f;"),
        ("... after parameters", "void(string s, ...) f;"),
        ("default parameter", "void(float a = 0) f;"),
        ("->", "void() f = { a = e->g; };"),
        ("(+)", "void() f = { a (+) b; };"),
        ("&~=", "void() f = { a &~= b; };"),
        ("^", "float a = b ^ c;"),
        ("<<", "void() f = { a = b << c; };"),
        ("unary -", "void() f = { a = -b; };"),
        ("a -1", "void() f = { a = b -1; };"),
        ("cast", "void() f = { a = (float) b; };"),
        (".5", "float a = .5;"),
        ("\\t", "string s = \"\\t\";"),
        ("bare local", "void() f = {\n\tfloat x = 1;\n};"),
        ("empty block", "void() f = { if (a) { } };"),
        ("hanging ;", "void() f = { a = 1;; };"),
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

    // spec FTE-21.5: the check parses each file after preprocessing, as FTEQCC compiles it, so
    // a macro used as a statement and the `;` after it are an empty statement, not an error;
    // a source-form `progs.src` defines macros for the files it lists.
    #[test]
    fn files_are_checked_after_preprocessing() {
        let dir = codebase(
            "fteqcc-profile-macros",
            &[
                (
                    "progs.src",
                    "#output \"progs.dat\"\n#define USE_COOP\n#includelist\ndefs.qc\nworld.qc\n#endlist\n",
                ),
                (
                    "defs.qc",
                    "float coop;\n\
                     #ifdef USE_COOP\n\
                     #define INHIBIT_COOP if (coop) return;\n\
                     #else\n\
                     #define INHIBIT_COOP this is not QuakeC\n\
                     #endif\n",
                ),
                (
                    "world.qc",
                    "float x;\nvoid() f =\n{\n\tINHIBIT_COOP\n\tx = 1;\n\tINHIBIT_COOP;\n};\n",
                ),
            ],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
    }

    // spec FTE-1.2: of the six codebases only `rerelease-mg3` is compiled with `-Fautoproto`,
    // so only it is resolved with autoprototyping.
    #[test]
    fn only_mg3_autoprototypes() {
        use crate::oracles::rerelease::autoprototypes;
        assert!(autoprototypes("rerelease-mg3"));
        for id in [
            "rerelease",
            "rerelease-hipnotic",
            "rerelease-rogue",
            "rerelease-mg1",
            "rerelease-ctf",
            "copper",
        ] {
            assert!(!autoprototypes(id), "{id}");
        }
    }

    // spec FTE-22.3: a codebase named `rerelease-mg3` resolves a use of a later file's names,
    // and frames a later file defines (FTE-22.4); another codebase with the same files fails.
    #[test]
    fn mg3_is_checked_with_autoprototyping() {
        let files: &[(&str, &str)] = &[
            ("progs.src", "progs.dat\nai.qc\nmonsters.qc\n"),
            (
                "ai.qc",
                "float out;\nvoid() f = { out = CORPSE_FLAG_A + $CORPSE_1; OrbCheckAttack(); };\n",
            ),
            (
                "monsters.qc",
                "$frame idle CORPSE_1\nconst float CORPSE_FLAG_A = 1;\nvoid() OrbCheckAttack = { };\n",
            ),
        ];
        let mg3 = codebase("rerelease-mg3", files);
        assert!(main(Path::new("."), &[mg3.display().to_string()]).unwrap());
        let other = codebase("fteqcc-profile-no-autoproto", files);
        assert!(!main(Path::new("."), &[other.display().to_string()]).unwrap());
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
