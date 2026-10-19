// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test code: an out-of-range index or a failed unwrap is a failed test"
)]

use std::collections::HashSet;

use qc_builtins::table::BUILTINS;
use qc_diag::{Code, Severity, Span};
use qc_project::Target;
use qc_sema::lower::lower_file;
use qc_sema::{FileId, ModuleIndex, ResolveOptions, resolve_module};
use qc_syntax::Parse;

use crate::check::builtin_number;
use crate::{BuiltinKey, Engine, FileDiagnostic, SOURCES, missing_builtins};

/// A module made of `files`, in compilation order: its parses and its resolved index.
fn module(files: &[&str]) -> (Vec<Parse>, ModuleIndex) {
    let parses: Vec<Parse> = files
        .iter()
        .map(|f| qc_syntax::parse(f.as_bytes()))
        .collect();
    let lowered: Vec<_> = parses
        .iter()
        .map(|p| lower_file(p, qc_syntax::Dialect::Vanilla))
        .collect();
    let index = resolve_module(&lowered, ResolveOptions::default());
    (parses, index)
}

/// The `W0311` warnings for `files` checked against `engine`.
fn check(engine: Engine, files: &[&str]) -> Vec<FileDiagnostic> {
    let (parses, index) = module(files);
    missing_builtins(engine, &parses, &index)
}

/// The messages of [`check`].
fn messages(engine: Engine, files: &[&str]) -> Vec<String> {
    check(engine, files)
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

const HASH_CALL: &str = "\
float(float tabsize, float defaulttype) hash_createtab = #287;
void() worldspawn = { hash_createtab(16, 0); };
";

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

// spec ENGT-1.1: the tables record each of the three fact sources with its repository, commit,
// file and license file, and their SHA-256
#[test]
fn the_tables_record_their_sources() {
    let ids: Vec<&str> = SOURCES.iter().map(|s| s.id).collect();
    assert_eq!(ids, ["fteqw", "darkplaces", "rerelease"]);
    for source in SOURCES {
        assert!(is_hex(source.commit, 40), "{}", source.id);
        assert!(is_hex(source.sha256, 64), "{}", source.id);
        assert!(is_hex(source.license_sha256, 64), "{}", source.id);
        assert!(source.repo.contains('/'), "{}", source.id);
        assert!(!source.path.is_empty() && !source.license_file.is_empty());
        assert!(source.license.starts_with("GPL-2.0"), "{}", source.id);
    }
}

// spec ENGT-3.1: Vanilla and QuakeSpasm are id's table; each engine's table holds each key
// once, and `builtin` agrees with `builtins`
#[test]
fn vanilla_and_quakespasm_are_ids_table() {
    for engine in [Engine::Vanilla, Engine::QuakeSpasm] {
        let numbers: Vec<BuiltinKey> = engine.builtins().iter().map(|b| b.key).collect();
        let ids: Vec<BuiltinKey> = BUILTINS
            .iter()
            .map(|b| BuiltinKey::Number(b.number))
            .collect();
        assert_eq!(numbers, ids);
        assert!(engine.builtin(BuiltinKey::Number(5)).is_none());
        assert!(engine.builtin(BuiltinKey::Number(79)).is_none());
        assert!(engine.builtin(BuiltinKey::Name("ex_bprint")).is_none());
    }
    for engine in Engine::ALL {
        let table = engine.builtins();
        let keys: HashSet<BuiltinKey> = table.iter().map(|b| b.key).collect();
        assert_eq!(keys.len(), table.len(), "{} repeats a key", engine.name());
        for builtin in &table {
            assert_eq!(engine.builtin(builtin.key), Some(*builtin));
        }
    }
}

// spec ENGT-3.1: Ironwail and vkQuake are id's table then the rerelease's, id's entry winning
// a shared number
#[test]
fn ironwail_and_vkquake_add_the_rerelease_builtins() {
    for engine in [Engine::Ironwail, Engine::VkQuake] {
        let bprint = engine.builtin(BuiltinKey::Number(23)).unwrap();
        assert_eq!(bprint.name, "bprint");
        assert_eq!(
            bprint,
            Engine::Vanilla.builtin(BuiltinKey::Number(23)).unwrap()
        );
        assert!(engine.builtin(BuiltinKey::Name("ex_bprint")).is_some());
        assert!(engine.builtin(BuiltinKey::Number(99)).is_some());
        assert!(engine.builtin(BuiltinKey::Number(287)).is_none());
        assert_eq!(engine.builtins()[0], Engine::Vanilla.builtins()[0]);
    }
}

// spec ENGT-3.1: FTEQW is its own file's server builtins; DarkPlaces is id's table then its
// own file's
#[test]
fn fteqw_and_darkplaces_tables() {
    let hash = Engine::Fteqw.builtin(BuiltinKey::Number(287)).unwrap();
    assert_eq!(hash.name, "hash_createtab");
    assert_eq!(hash.extensions, ["FTE_QC_HASHTABLES"]);
    assert!(Engine::Fteqw.builtin(BuiltinKey::Number(1)).is_some());
    assert!(
        Engine::Fteqw
            .builtin(BuiltinKey::Name("removeinstant"))
            .is_some()
    );
    let strlen = Engine::DarkPlaces.builtin(BuiltinKey::Number(114)).unwrap();
    assert_eq!(
        (strlen.name, strlen.signature),
        ("strlen", "float(string s)")
    );
    assert_eq!(
        Engine::DarkPlaces.builtin(BuiltinKey::Number(1)),
        Engine::Vanilla.builtin(BuiltinKey::Number(1))
    );
    assert!(
        Engine::DarkPlaces
            .builtin(BuiltinKey::Number(287))
            .is_none()
    );
}

// spec ENGT-3.2: KEX is the rerelease defs alone, with no number 23, 24 or 73 and the
// ex_ names in their place
#[test]
fn kex_is_the_rerelease_defs() {
    for number in [23, 24, 73] {
        assert!(
            Engine::Kex.builtin(BuiltinKey::Number(number)).is_none(),
            "#{number}"
        );
    }
    for name in ["ex_bprint", "ex_sprint", "ex_centerprint"] {
        assert!(
            Engine::Kex.builtin(BuiltinKey::Name(name)).is_some(),
            "{name}"
        );
    }
    let makevectors = Engine::Kex.builtin(BuiltinKey::Number(1)).unwrap();
    assert_eq!(makevectors.signature, "void(vector ang)");
}

// spec ENGT-3.3: FTEQW and DarkPlaces declare extension names; the other engines declare none
#[test]
fn extensions_per_engine() {
    assert!(Engine::Fteqw.extensions().contains(&"FTE_QC_HASHTABLES"));
    assert!(
        Engine::DarkPlaces
            .extensions()
            .contains(&"DP_QC_SINCOSSQRTPOW")
    );
    assert!(
        !Engine::DarkPlaces
            .extensions()
            .contains(&"FTE_QC_HASHTABLES")
    );
    for engine in [
        Engine::Vanilla,
        Engine::QuakeSpasm,
        Engine::Ironwail,
        Engine::VkQuake,
        Engine::Kex,
    ] {
        assert!(engine.extensions().is_empty(), "{}", engine.name());
    }
}

// spec ENGT-3.4: each engine has a display name and an id, and an unknown id gives none
#[test]
fn names_and_ids() {
    let names: Vec<&str> = Engine::ALL.map(Engine::name).to_vec();
    assert_eq!(
        names,
        [
            "Vanilla",
            "QuakeSpasm",
            "Ironwail",
            "vkQuake",
            "FTEQW",
            "DarkPlaces",
            "KEX"
        ]
    );
    let ids: Vec<&str> = Engine::ALL.map(Engine::id).to_vec();
    assert_eq!(
        ids,
        [
            "vanilla",
            "quakespasm",
            "ironwail",
            "vkquake",
            "fteqw",
            "darkplaces",
            "kex"
        ]
    );
    for engine in Engine::ALL {
        assert_eq!(Engine::from_id(engine.id()), Some(engine));
    }
    for unknown in ["", "Ironwail", "quake", "fte"] {
        assert_eq!(Engine::from_id(unknown), None);
    }
}

// spec ENGT-3.5: a project file's target selects an engine
#[test]
fn project_targets_select_engines() {
    assert_eq!(Engine::from_target(Target::Quake), Engine::Vanilla);
    assert_eq!(Engine::from_target(Target::Rerelease), Engine::Kex);
    assert_eq!(Engine::from_target(Target::Fte), Engine::Fteqw);
    assert_eq!(Engine::from_target(Target::Dp), Engine::DarkPlaces);
}

// spec ENGT-4.1, QCD-7.17: calling an FTE_ builtin while targeting Ironwail is W0311 at the
// callee's name, naming the engine and the extension
#[test]
fn an_fte_builtin_targeting_ironwail_is_w0311() {
    let found = check(Engine::Ironwail, &[HASH_CALL]);
    assert_eq!(found.len(), 1);
    let warning = &found[0];
    assert_eq!(warning.file, FileId(0));
    assert_eq!(warning.diagnostic.code, Code::W0311);
    assert_eq!(
        warning.diagnostic.message,
        "hash_createtab is builtin #287, which Ironwail does not have (FTE_QC_HASHTABLES)"
    );
    let start = HASH_CALL.rfind("hash_createtab").unwrap_or_default();
    assert_eq!(
        warning.diagnostic.span,
        Span::new(start, start.saturating_add("hash_createtab".len()))
    );
}

// spec ENGT-4.1: the extensions come from the first engine, in ENGT-3.1 order, that lists the
// number with some; a number every listing engine has without extensions adds none
#[test]
fn the_message_names_the_engine_and_extensions() {
    let strlen = "float(string s) strlen = #114;\nvoid() w = { strlen(\"a\"); };\n";
    assert_eq!(
        messages(Engine::QuakeSpasm, &[strlen]),
        ["strlen is builtin #114, which QuakeSpasm does not have \
          (FRIK_FILE, FTE_STRINGS, ZQ_QC_STRINGS)"]
    );
    let bprint = "void(string s) bprint = #23;\nvoid() w = { bprint(\"a\"); };\n";
    assert_eq!(
        messages(Engine::Kex, &[bprint]),
        ["bprint is builtin #23, which KEX does not have"]
    );
    let unknown = "void() nothing = #9999;\nvoid() w = { nothing(); };\n";
    assert_eq!(
        messages(Engine::Fteqw, &[unknown]),
        ["nothing is builtin #9999, which FTEQW does not have"]
    );
}

// spec ENGT-4.1: a builtin declared in one file and called in another is reported in the
// calling file, once per call, in file and source order
#[test]
fn warnings_are_per_call_and_per_file() {
    let decl = "float(float tabsize, float defaulttype) hash_createtab = #287;\n";
    let calls = "void() a = { hash_createtab(1, 0); hash_createtab(2, 0); };\n";
    let found = check(Engine::Vanilla, &[decl, calls]);
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|d| d.file == FileId(1)));
    assert!(found[0].diagnostic.span < found[1].diagnostic.span);
}

// spec ENGT-4.2: no W0311 when the engine has the number, for a call of a name that is not a
// builtin, for a use that is not a call's callee, or for a builtin never called
#[test]
fn no_warning_without_a_call_of_a_missing_builtin() {
    assert!(check(Engine::Fteqw, &[HASH_CALL]).is_empty());
    let has = "void(string s) dprint = #25;\nvoid() w = { dprint(\"a\"); };\n";
    for engine in Engine::ALL {
        assert!(check(engine, &[has]).is_empty(), "{}", engine.name());
    }
    let plain = "void() helper = {};\nvoid() w = { helper(); };\n";
    assert!(check(Engine::Vanilla, &[plain]).is_empty());
    let as_argument = "\
float(float tabsize, float defaulttype) hash_createtab = #287;
void(void() f) take = {};
void() w = { take(hash_createtab); };
";
    assert!(check(Engine::Vanilla, &[as_argument]).is_empty());
    let declared = "float(float tabsize, float defaulttype) hash_createtab = #287;\n";
    assert!(check(Engine::Vanilla, &[declared]).is_empty());
    let undefined = "void() w = { missing(); };\n";
    assert!(check(Engine::Vanilla, &[undefined]).is_empty());
}

// spec ENGT-4.3: the first #N initialiser of a name gives its number, in compilation order
#[test]
fn the_first_builtin_number_wins() {
    let first = "void() f = #287;\n";
    let second = "void() f = #1;\nvoid() w = { f(); };\n";
    assert_eq!(
        messages(Engine::Vanilla, &[first, second]),
        ["f is builtin #287, which Vanilla does not have (FTE_QC_HASHTABLES)"]
    );
    let swapped = "void() f = #1;\n";
    let later = "void() f = #287;\nvoid() w = { f(); };\n";
    assert!(check(Engine::Vanilla, &[swapped, later]).is_empty());
    let fraction = "void() f = #287.00;\nvoid() w = { f(); };\n";
    assert_eq!(check(Engine::Vanilla, &[fraction]).len(), 1);
    let frame = "$frame a\nvoid() f = #$a;\nvoid() w = { f(); };\n";
    assert!(check(Engine::Vanilla, &[frame]).is_empty());
}

// spec ENGT-4.3: a builtin number is decimal digits with an optional fraction of zeros, at
// least 1
#[test]
fn builtin_number_literals() {
    assert_eq!(builtin_number("1"), Some(1));
    assert_eq!(builtin_number("287"), Some(287));
    assert_eq!(builtin_number("12."), Some(12));
    assert_eq!(builtin_number("12.000"), Some(12));
    assert_eq!(builtin_number("4294967295"), Some(u32::MAX));
    for bad in [
        "",
        "0",
        "0.0",
        "1.5",
        ".5",
        "a",
        "1e2",
        "-1",
        "+1",
        "4294967296",
    ] {
        assert_eq!(builtin_number(bad), None, "{bad}");
    }
}

// spec ENGT-4.4, QCD-7.17: W0311 is a warning, written W0311
#[test]
fn w0311_is_a_warning() {
    assert_eq!(Code::W0311.as_str(), "W0311");
    assert_eq!(Code::W0311.severity(), Severity::Warning);
    assert!(!Code::W0311.is_error());
    let found = check(Engine::Ironwail, &[HASH_CALL]);
    assert!(found.iter().all(|d| !d.diagnostic.is_error()));
}
