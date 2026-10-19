// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-profile <dir>...`: the acceptance check for the FTEQCC dialect profile
//! (P2-03, P2-32, P2-26). For each codebase directory it opens the project, and opens every
//! module through the semantic model as the editor does: each file of the module's graph goes
//! into a `qc_sema::Db` as written, under the FTEQCC profile (`Dialect::Fteqcc`, and
//! autoprototyping for the codebase FTEQCC builds with `-Fautoproto`), and the Db preprocesses
//! the module, parses and resolves each file's expansion and maps every location back to the
//! source (`Db::analyze_module`). It fails on any preprocessor, lexical, syntax or semantic
//! error, each printed at its source file and line, and on a definition, reference or
//! diagnostic outside the bytes of its source file. It then checks the vanilla profile reports
//! each extension as a diagnostic, so the two profiles are observably different (the issue's
//! acceptance criterion).
//!
//! The rerelease codebases come from `cargo xtask corpus fetch rerelease rerelease-hipnotic
//! rerelease-rogue rerelease-mg1 rerelease-ctf rerelease-mg3`. The FTEQCC subset each one uses
//! is recorded in `docs/spec/vectors/dialect-fteqcc-rerelease/coverage.txt` and pinned by the
//! coverage check (`fteqcc_coverage`); this check adds that they parse and resolve clean. The
//! FTEQCC corpus mods of the full dialect (`docs/spec/dialect-fteqcc.md`, FTEF-1.1) are checked
//! in the tree `cargo xtask corpus fetch freecs-tree` lays out: FreeCS (`cstrike/src`),
//! Rad-Therapy (`valve/src`, whose menu module builds Nuclide's `menu-vgui`) and Nuclide's own
//! `src/menu-fn`. Copper and Arcane Dimensions (`cargo xtask corpus fetch copper
//! arcane-dimensions`) are checked too (P2-38): FTEQCC builds them from list-form `progs.src`
//! files, Copper's with `#pragma autoproto` (FTEF-12.9).

use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::LineIndex;
use qc_project::{Dialect, Module, open};
use qc_sema::{Db, FileId, Location, ModuleInput};
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
            "every module parses and resolves under the FTEQCC profile"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Parses and resolves every module of the codebase in `dir` under the FTEQCC profile.
///
/// Each module is opened through the semantic model as the editor opens it: every file of
/// the module's graph is put in a `qc_sema::Db` with its path and its text as written, and
/// `Db::analyze_module` preprocesses the module (QC-9), parses and resolves each file's
/// expansion, and reports every diagnostic at a span of a source file (QC-9.7). A file as
/// written can use a macro as a statement (`INHIBIT_COOP` in `rerelease-mg1`), which no parser
/// of the unexpanded text accepts. The macros of a source-form `progs.src` (PRJ-4.4) are
/// defined by preprocessing it first; its own lines are the graph's (PRJ-5), so its tokens are
/// not parsed.
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
        ok &= analyze(module, autoproto)?;
    }
    println!(
        "fteqcc-profile: {}: {modules} modules checked",
        dir.display()
    );
    Ok(ok)
}

/// Opens one module through the semantic model and prints every error at the file and line
/// it is in; fails on an error, or on a definition, reference or diagnostic outside the
/// bytes of its source file.
fn analyze(module: &Module, autoproto: bool) -> Result<bool, String> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc.into());
    db.set_autoproto(autoproto);
    for (index, file) in module.graph.files.iter().enumerate() {
        let text = fs::read(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        db.set_file(FileId(index), file.path.clone(), text);
    }
    let analysis = db.analyze_module(&ModuleInput::from_graph(&module.graph));
    let mut ok = true;
    for d in analysis.diagnostics() {
        let at = Location {
            file: d.file,
            span: d.diagnostic.span,
        };
        if !in_source(&db, at) {
            ok = false;
            println!(
                "FAIL  module {}: {} outside its source file",
                module.name,
                d.diagnostic.code.as_str()
            );
        }
        if d.diagnostic.code.is_error() {
            ok = false;
            println!(
                "FAIL  {}: {} {}",
                place(&db, at),
                d.diagnostic.code.as_str(),
                d.diagnostic.message
            );
        }
    }
    let index = &analysis.index;
    let locations = index
        .defs
        .iter()
        .map(|d| d.location)
        .chain(index.refs.iter().map(|r| r.location));
    let outside = locations.filter(|&at| !in_source(&db, at)).count();
    if outside > 0 {
        ok = false;
        println!(
            "FAIL  module {}: {outside} definitions or references outside their source file",
            module.name
        );
    }
    Ok(ok)
}

/// Whether a location is a span of the bytes of its source file.
fn in_source(db: &Db, at: Location) -> bool {
    db.file_path(at.file).is_some()
        && at.span.start <= at.span.end
        && at.span.end <= db.file_text(at.file).len()
}

/// The path and 1-based line of a location.
fn place(db: &Db, at: Location) -> String {
    let path = db.file_path(at.file).unwrap_or_else(|| Path::new("?"));
    let line = LineIndex::new(db.file_text(at.file)).line(at.span.start);
    format!("{}:{line}", path.display())
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

    // spec FTEF-1.1: a codebase in the full dialect, a class with members defined outside it, a
    // pointer, an accessor and the FTEQCC preprocessor, parses and resolves with no error.
    #[test]
    fn a_full_dialect_codebase_passes() {
        let dir = codebase(
            "fteqcc-profile-full",
            &[
                (
                    "progs.src",
                    "#pragma target fte_5768\n#define SERVER\n#includelist\ndefs.h\nthing.qc\n#endlist\n",
                ),
                (
                    "defs.h",
                    "#pragma noref 1\nentity self;\nfloat time;\n.float nextthink;\n\
                     entity() spawn = #14;\naccessor strbuf : float;\n#pragma noref 0\n\
                     #if defined(SERVER) && !defined(CLIENT)\n#define LOG(...) dprint(sprintf(__VA_ARGS__))\n#endif\n\
                     void(string s, ...) dprint = #25;\nstring(string f, ...) sprintf = #627;\n\
                     class Thing\n{\n\tfloat m_flTime;\n\tvirtual void Think(void);\n\tnonvirtual void Reset(void);\n};\n",
                ),
                (
                    "thing.qc",
                    "void\nThing::Reset(void)\n{\n\tm_flTime = 0;\n}\n\nvoid\nThing::Think(void)\n{\n\
                     \tThing *self_ptr = &this;\n\tint i = 0x10i;\n\tif not (m_flTime)\n\t\tReset();\n\
                     \tnextthink = time + 0.1f;\n\tLOG(\"%d\\n\", i);\n}\n",
                ),
            ],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
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
