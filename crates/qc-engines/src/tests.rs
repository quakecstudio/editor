// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test code: an out-of-range index or a failed unwrap is a failed test"
)]

use std::collections::HashSet;
use std::path::Path;

use qc_builtins::table::BUILTINS;
use qc_diag::{Code, Severity, Span};
use qc_project::{MemoryLoader, ModuleKind, Target, open_dir};
use qc_sema::lower::lower_file;
use qc_sema::{FileId, ModuleIndex, ResolveOptions, resolve_module};
use qc_syntax::{Dialect, Parse};

use crate::check::builtin_number;
use crate::{BuiltinKey, Engine, FileDiagnostic, SOURCES, missing_builtins, tables};

/// A module made of `files`, in compilation order and in `dialect`: its parses and its
/// resolved index.
fn module(dialect: Dialect, files: &[&str]) -> (Vec<Parse>, ModuleIndex) {
    let parses: Vec<Parse> = files
        .iter()
        .map(|f| qc_syntax::parse_with(f.as_bytes(), dialect))
        .collect();
    let lowered: Vec<_> = parses.iter().map(|p| lower_file(p, dialect)).collect();
    let options = ResolveOptions {
        dialect,
        ..ResolveOptions::default()
    };
    let index = resolve_module(&lowered, options);
    (parses, index)
}

/// The `W0311` messages for a module of `kind` made of FTEQCC `files`, checked against
/// `engine`.
fn check_kind(engine: Engine, kind: ModuleKind, files: &[&str]) -> Vec<String> {
    let (parses, index) = module(Dialect::Fteqcc, files);
    missing_builtins(engine, kind, &parses, &index)
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

/// The `W0311` warnings for a server module of `files`, in `dialect`, checked against
/// `engine`.
fn check_in(dialect: Dialect, engine: Engine, files: &[&str]) -> Vec<FileDiagnostic> {
    let (parses, index) = module(dialect, files);
    missing_builtins(engine, ModuleKind::Ssqc, &parses, &index)
}

/// The `W0311` warnings for vanilla `files` checked against `engine`.
fn check(engine: Engine, files: &[&str]) -> Vec<FileDiagnostic> {
    check_in(Dialect::Vanilla, engine, files)
}

/// The messages of [`check_in`].
fn messages_in(dialect: Dialect, engine: Engine, files: &[&str]) -> Vec<String> {
    check_in(dialect, engine, files)
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

/// The messages of [`check`].
fn messages(engine: Engine, files: &[&str]) -> Vec<String> {
    messages_in(Dialect::Vanilla, engine, files)
}

/// The messages of FTEQCC `files` checked against `engine`.
fn fte_messages(engine: Engine, files: &[&str]) -> Vec<String> {
    messages_in(Dialect::Fteqcc, engine, files)
}

const BPRINT_CALL: &str = "\
void(string s, ...) bprint = #0:ex_bprint;
void() worldspawn = { bprint(\"hello\\n\"); };
";

const HASH_CALL: &str = "\
float(float tabsize, float defaulttype) hash_createtab = #287;
void() worldspawn = { hash_createtab(16, 0); };
";

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

// spec ENGT-1.5: the tables record each of the ten fact sources with its repository, commit,
// file and license file, and their SHA-256
#[test]
fn the_tables_record_their_sources() {
    let ids: Vec<&str> = SOURCES.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        [
            "fteqw",
            "darkplaces",
            "darkplaces-client",
            "darkplaces-menu",
            "rerelease",
            "quakespasm",
            "quakespasm-rerelease",
            "ironwail",
            "vkquake",
            "vkquake-ext",
        ]
    );
    for source in SOURCES {
        assert!(is_hex(source.commit, 40), "{}", source.id);
        assert!(is_hex(source.sha256, 64), "{}", source.id);
        assert!(is_hex(source.license_sha256, 64), "{}", source.id);
        assert!(source.repo.contains('/'), "{}", source.id);
        assert!(!source.path.is_empty() && !source.license_file.is_empty());
        assert!(source.license.starts_with("GPL-2.0"), "{}", source.id);
    }
}

// spec ENGT-3.9: Vanilla is id's table; each engine's table holds each key once, and
// `builtin` agrees with `builtins`
#[test]
fn vanilla_is_ids_table() {
    let numbers: Vec<BuiltinKey> = Engine::Vanilla
        .builtins(ModuleKind::Ssqc)
        .iter()
        .map(|b| b.key)
        .collect();
    let ids: Vec<BuiltinKey> = BUILTINS
        .iter()
        .map(|b| BuiltinKey::Number(b.number))
        .collect();
    assert_eq!(numbers, ids);
    for key in [
        BuiltinKey::Number(5),
        BuiltinKey::Number(79),
        BuiltinKey::Name("ex_bprint"),
    ] {
        assert!(Engine::Vanilla.builtin(ModuleKind::Ssqc, key).is_none());
    }
    for engine in Engine::ALL {
        let table = engine.builtins(ModuleKind::Ssqc);
        let keys: HashSet<BuiltinKey> = table.iter().map(|b| b.key).collect();
        assert_eq!(keys.len(), table.len(), "{} repeats a key", engine.name());
        for builtin in &table {
            assert_eq!(
                engine.builtin(ModuleKind::Ssqc, builtin.key),
                Some(*builtin)
            );
        }
    }
}

// spec ENGT-3.9: id's layer adds no key to the QuakeSpasm lineage: each engine's own server
// table has every number of id's, and id's entry names it
#[test]
fn the_lineage_has_every_id_number() {
    let own = [
        (Engine::QuakeSpasm, tables::QUAKESPASM_SSQC_BUILTINS),
        (Engine::Ironwail, tables::IRONWAIL_SSQC_BUILTINS),
        (Engine::VkQuake, tables::VKQUAKE_SSQC_BUILTINS),
    ];
    for (engine, table) in own {
        for id in BUILTINS {
            let key = BuiltinKey::Number(id.number);
            assert!(
                table.iter().any(|b| b.key == key),
                "{} lacks #{}",
                engine.name(),
                id.number
            );
        }
    }
    for engine in [Engine::QuakeSpasm, Engine::Ironwail] {
        assert_eq!(
            engine.builtin(ModuleKind::Ssqc, BuiltinKey::Number(23)),
            Engine::Vanilla.builtin(ModuleKind::Ssqc, BuiltinKey::Number(23))
        );
    }
}

// spec ENGT-3.9, ENGT-2.14, ENGT-2.16: QuakeSpasm has id's builtins, the rerelease's numbers
// it implements and the rerelease's `ex_` names it patches to an implemented number
#[test]
fn quakespasm_is_its_own_table() {
    let has = |key| Engine::QuakeSpasm.builtin(ModuleKind::Ssqc, key);
    let finale = has(BuiltinKey::Number(79)).unwrap();
    assert_eq!((finale.name, finale.signature), ("PF_finalefinished", ""));
    for number in [80, 90, 91] {
        assert!(has(BuiltinKey::Number(number)).is_some(), "#{number}");
    }
    for number in [5, 81, 89, 92, 114] {
        assert!(has(BuiltinKey::Number(number)).is_none(), "#{number}");
    }
    for name in [
        "ex_bprint",
        "ex_sprint",
        "ex_centerprint",
        "ex_finaleFinished",
        "ex_walkpathtogoal",
    ] {
        assert!(has(BuiltinKey::Name(name)).is_some(), "{name}");
    }
    for name in [
        "ex_draw_point",
        "ex_draw_cylinder",
        "ex_bot_movetopoint",
        "centerprint",
    ] {
        assert!(has(BuiltinKey::Name(name)).is_none(), "{name}");
    }
}

// spec ENGT-3.9, ENGT-2.15: Ironwail is id's table then its own pr_builtindefs, numbered rows
// by number and the rest by name, with each row's extension
#[test]
fn ironwail_is_its_own_table() {
    let has = |key| Engine::Ironwail.builtin(ModuleKind::Ssqc, key);
    let stof = has(BuiltinKey::Number(81)).unwrap();
    assert_eq!((stof.name, stof.extensions), ("stof", &["FRIK_FILE"][..]));
    assert_eq!(has(BuiltinKey::Number(114)).unwrap().name, "strlen");
    assert!(has(BuiltinKey::Number(99)).is_some());
    assert!(has(BuiltinKey::Name("ex_bprint")).is_some());
    assert!(has(BuiltinKey::Name("ftoi")).is_some());
    // Ironwail spells it ex_finalefinished; its name lookup is strcmp, so the rerelease's
    // ex_finaleFinished is not it.
    assert!(has(BuiltinKey::Name("ex_finalefinished")).is_some());
    assert!(has(BuiltinKey::Name("ex_finaleFinished")).is_none());
    for number in [5, 82, 287, 300, 316] {
        assert!(has(BuiltinKey::Number(number)).is_none(), "#{number}");
    }
}

// spec ENGT-3.9, ENGT-2.15: vkQuake is its extension builtins, then id's table, then its own
// array, so an extension builtin with a documented number takes that number's place
#[test]
fn vkquake_is_its_own_table() {
    let has = |key| Engine::VkQuake.builtin(ModuleKind::Ssqc, key);
    let angles = has(BuiltinKey::Number(51)).unwrap();
    assert_eq!(
        (angles.name, angles.signature),
        ("vectoangles2", "vector(vector fwd, optional vector up)")
    );
    assert_eq!(has(BuiltinKey::Number(80)).unwrap().name, "infokey");
    assert_eq!(
        has(BuiltinKey::Number(23)),
        Engine::Vanilla.builtin(ModuleKind::Ssqc, BuiltinKey::Number(23))
    );
    assert_eq!(
        has(BuiltinKey::Number(79)).unwrap().name,
        "PF_sv_finalefinished"
    );
    let multicast = has(BuiltinKey::Number(82)).unwrap();
    assert_eq!(multicast.signature, "void(vector where, float set)");
    for name in ["ex_bprint", "ex_finaleFinished", "etof"] {
        assert!(has(BuiltinKey::Name(name)).is_some(), "{name}");
    }
    assert!(has(BuiltinKey::Name("ex_draw_point")).is_none());
    assert!(has(BuiltinKey::Number(534)).is_none());
    assert!(has(BuiltinKey::Number(287)).is_none());
    assert!(
        Engine::VkQuake
            .builtin(ModuleKind::Csqc, BuiltinKey::Number(534))
            .is_some()
    );
}

// spec ENGT-3.9: FTEQW is its own file's server builtins; DarkPlaces is id's table then its
// own file's
#[test]
fn fteqw_and_darkplaces_tables() {
    let hash = Engine::Fteqw
        .builtin(ModuleKind::Ssqc, BuiltinKey::Number(287))
        .unwrap();
    assert_eq!(hash.name, "hash_createtab");
    assert_eq!(hash.extensions, ["FTE_QC_HASHTABLES"]);
    assert!(
        Engine::Fteqw
            .builtin(ModuleKind::Ssqc, BuiltinKey::Number(1))
            .is_some()
    );
    assert!(
        Engine::Fteqw
            .builtin(ModuleKind::Ssqc, BuiltinKey::Name("removeinstant"))
            .is_some()
    );
    let strlen = Engine::DarkPlaces
        .builtin(ModuleKind::Ssqc, BuiltinKey::Number(114))
        .unwrap();
    assert_eq!(
        (strlen.name, strlen.signature),
        ("strlen", "float(string s)")
    );
    assert_eq!(
        Engine::DarkPlaces.builtin(ModuleKind::Ssqc, BuiltinKey::Number(1)),
        Engine::Vanilla.builtin(ModuleKind::Ssqc, BuiltinKey::Number(1))
    );
    assert!(
        Engine::DarkPlaces
            .builtin(ModuleKind::Ssqc, BuiltinKey::Number(287))
            .is_none()
    );
}

// spec ENGT-3.2: KEX is the rerelease defs alone, with no number 23, 24 or 73 and the
// ex_ names in their place
#[test]
fn kex_is_the_rerelease_defs() {
    for number in [23, 24, 73] {
        assert!(
            Engine::Kex
                .builtin(ModuleKind::Ssqc, BuiltinKey::Number(number))
                .is_none(),
            "#{number}"
        );
    }
    for name in ["ex_bprint", "ex_sprint", "ex_centerprint"] {
        assert!(
            Engine::Kex
                .builtin(ModuleKind::Ssqc, BuiltinKey::Name(name))
                .is_some(),
            "{name}"
        );
    }
    let makevectors = Engine::Kex
        .builtin(ModuleKind::Ssqc, BuiltinKey::Number(1))
        .unwrap();
    assert_eq!(makevectors.signature, "void(vector ang)");
}

// spec ENGT-3.10: FTEQW, DarkPlaces and vkQuake declare extension names; the other engines
// declare none
#[test]
fn extensions_per_engine() {
    let vkquake = Engine::VkQuake.extensions(ModuleKind::Ssqc);
    assert!(vkquake.contains(&"FRIK_FILE") && vkquake.contains(&"DP_QC_SINCOSSQRTPOW"));
    assert!(!vkquake.contains(&"FTE_QC_HASHTABLES"));
    assert_eq!(vkquake, Engine::VkQuake.extensions(ModuleKind::Csqc));
    assert!(
        Engine::Fteqw
            .extensions(ModuleKind::Ssqc)
            .contains(&"FTE_QC_HASHTABLES")
    );
    assert!(
        Engine::DarkPlaces
            .extensions(ModuleKind::Ssqc)
            .contains(&"DP_QC_SINCOSSQRTPOW")
    );
    assert!(
        !Engine::DarkPlaces
            .extensions(ModuleKind::Ssqc)
            .contains(&"FTE_QC_HASHTABLES")
    );
    for engine in [
        Engine::Vanilla,
        Engine::QuakeSpasm,
        Engine::Ironwail,
        Engine::Kex,
    ] {
        assert!(
            engine.extensions(ModuleKind::Ssqc).is_empty(),
            "{}",
            engine.name()
        );
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

// spec ENGT-3.6: quakespasm, ironwail and vkquake select their engines, and every engine is
// some target's
#[test]
fn every_engine_is_some_targets() {
    assert_eq!(Engine::from_target(Target::QuakeSpasm), Engine::QuakeSpasm);
    assert_eq!(Engine::from_target(Target::Ironwail), Engine::Ironwail);
    assert_eq!(Engine::from_target(Target::VkQuake), Engine::VkQuake);
    let reached: Vec<Engine> = Target::ALL.map(Engine::from_target).to_vec();
    for engine in Engine::ALL {
        assert!(reached.contains(&engine), "{}", engine.name());
    }
}

// spec ENGT-3.6, ENGT-4.1: a quakec.toml targeting ironwail opens, and calling an FTE_ builtin
// in its module is W0311 naming Ironwail
#[test]
fn an_ironwail_project_reports_an_fte_builtin() {
    let manifest =
        "[project]\ntarget = \"ironwail\"\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n";
    let mut loader = MemoryLoader::new();
    loader.insert("proj/quakec.toml", manifest.as_bytes());
    loader.insert("proj/progs.src", b"progs.dat\nworld.qc\n");
    loader.insert("proj/world.qc", HASH_CALL.as_bytes());
    let project = open_dir(&mut loader, Path::new("proj")).unwrap();
    let module = &project.modules[0];
    assert_eq!(module.target, Target::Ironwail);
    let paths: Vec<&Path> = module
        .graph
        .files
        .iter()
        .map(|f| f.path.as_path())
        .collect();
    assert_eq!(paths, [Path::new("proj/world.qc")]);
    assert_eq!(
        messages(Engine::from_target(module.target), &[HASH_CALL]),
        ["hash_createtab is builtin #287, which Ironwail does not have (FTE_QC_HASHTABLES)"]
    );
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

// spec ENGT-4.1: the extensions come from the first engine, in ENGT-3.9 order, that lists the
// number with some; a number every listing engine has without extensions adds none
#[test]
fn the_message_names_the_engine_and_extensions() {
    let strlen = "float(string s) strlen = #114;\nvoid() w = { strlen(\"a\"); };\n";
    assert_eq!(
        messages(Engine::QuakeSpasm, &[strlen]),
        ["strlen is builtin #114, which QuakeSpasm does not have (FRIK_FILE)"]
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

// spec ENGT-4.5, QCD-7.17: calling `bprint = #0:ex_bprint` targeting FTEQW is W0311 naming
// FTEQW at the callee's name; KEX, QuakeSpasm, Ironwail and vkQuake have ex_bprint
#[test]
fn a_named_builtin_targeting_fteqw_is_w0311() {
    let found = check_in(Dialect::Fteqcc, Engine::Fteqw, &[BPRINT_CALL]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].diagnostic.code, Code::W0311);
    assert_eq!(
        found[0].diagnostic.message,
        "bprint is builtin #0:ex_bprint, which FTEQW does not have"
    );
    let start = BPRINT_CALL.rfind("bprint(").unwrap();
    assert_eq!(
        found[0].diagnostic.span,
        Span::new(start, start.saturating_add("bprint".len()))
    );
    for engine in [
        Engine::Kex,
        Engine::QuakeSpasm,
        Engine::Ironwail,
        Engine::VkQuake,
    ] {
        assert!(
            check_in(Dialect::Fteqcc, engine, &[BPRINT_CALL]).is_empty(),
            "{}",
            engine.name()
        );
    }
    for engine in [Engine::Vanilla, Engine::DarkPlaces] {
        assert_eq!(
            fte_messages(engine, &[BPRINT_CALL]),
            [format!(
                "bprint is builtin #0:ex_bprint, which {} does not have",
                engine.name()
            )]
        );
    }
}

// spec ENGT-4.5: the extensions of a named builtin come from the first engine whose table has
// the name with some, and an engine with the name has no W0311
#[test]
fn a_named_builtin_carries_its_extensions() {
    let source = "\
void(entity client, float flags, string s) stuffcmdflags = #0:stuffcmdflags;
void() w = { stuffcmdflags(world, 0, \"x\"); };
";
    assert_eq!(
        fte_messages(Engine::Kex, &[source]),
        [
            "stuffcmdflags is builtin #0:stuffcmdflags, which KEX does not have \
          (FTE_QC_STUFFCMDFLAGS)"
        ]
    );
    assert!(fte_messages(Engine::Fteqw, &[source]).is_empty());
}

// spec ENGT-4.5: a name with a #N initialiser keeps that number; otherwise its first #0:NAME
// gives the engine name, compared exactly; a zero written 0.0 counts as zero
#[test]
fn the_number_wins_over_an_engine_name() {
    let named = "void() f = #0:ex_bprint;\n";
    let numbered = "void() f = #287;\nvoid() w = { f(); };\n";
    assert_eq!(
        fte_messages(Engine::Kex, &[named, numbered]),
        ["f is builtin #287, which KEX does not have (FTE_QC_HASHTABLES)"]
    );
    let first = "void() f = #0:ex_bprint;\n";
    let second = "void() f = #0:nothing;\nvoid() w = { f(); };\n";
    assert!(fte_messages(Engine::Kex, &[first, second]).is_empty());
    assert_eq!(
        fte_messages(Engine::Kex, &[second, first]),
        ["f is builtin #0:nothing, which KEX does not have"]
    );
    let upper = "void() f = #0:EX_BPRINT;\nvoid() w = { f(); };\n";
    assert_eq!(
        fte_messages(Engine::Kex, &[upper]),
        ["f is builtin #0:EX_BPRINT, which KEX does not have"]
    );
    let fraction = "void() f = #0.0:nothing;\nvoid() w = { f(); };\n";
    assert_eq!(fte_messages(Engine::Kex, &[fraction]).len(), 1);
}

// spec ENGT-4.9: the `0` of `#0:NAME` is any literal FTEQCC reads as the value 0, the hex
// `0x0` and the integer `0i` included; `0.5` and `0x1` are no zero, so they give no engine name
// and no W0311
#[test]
fn only_a_zero_literal_takes_an_engine_name() {
    for zero in ["00", ".0", "0.00", "0x0", "0X00", "0i"] {
        let source = format!("void() f = #{zero}:nothing;\nvoid() w = {{ f(); }};\n");
        assert_eq!(
            fte_messages(Engine::Kex, &[source.as_str()]),
            ["f is builtin #0:nothing, which KEX does not have"],
            "#{zero}"
        );
    }
    for other in ["0.5", "0x1"] {
        let source = format!("void() f = #{other}:nothing;\nvoid() w = {{ f(); }};\n");
        assert!(
            fte_messages(Engine::Kex, &[source.as_str()]).is_empty(),
            "#{other}"
        );
    }
}

// spec ENGT-4.6: `#N:NAME` with N at least 1 is checked by its number alone
#[test]
fn a_numbered_builtin_with_a_name_is_checked_by_number() {
    let source = "void() f = #287:ex_bprint;\nvoid() w = { f(); };\n";
    assert_eq!(
        fte_messages(Engine::Kex, &[source]),
        ["f is builtin #287, which KEX does not have (FTE_QC_HASHTABLES)"]
    );
    assert!(fte_messages(Engine::Fteqw, &[source]).is_empty());
}

// spec ENGT-4.7: under the vanilla dialect `#0:NAME` is not a builtin initialiser, so it
// gives no W0311
#[test]
fn a_named_builtin_in_vanilla_gives_none() {
    for engine in Engine::ALL {
        assert!(
            check(engine, &[BPRINT_CALL]).is_empty(),
            "{}",
            engine.name()
        );
    }
}

const CLEARSCENE_CALL: &str = "void() clearscene = #300;\nvoid() w = { clearscene(); };\n";
const SETSPAWNPARMS_CALL: &str =
    "void(entity e) setspawnparms = #78;\nvoid(entity e) w = { setspawnparms(e); };\n";
const GETMOUSEPOS_CALL: &str = "vector() getmousepos = #66;\nvoid() w = { getmousepos(); };\n";

// spec ENGT-2.10, ENGT-3.11: FTEQW has a client and a menu QuakeC table, read from fteextensions.qc
// with CSQC and MENU defined, which differ from its server table
#[test]
fn fteqw_has_client_and_menu_tables() {
    let csqc = Engine::Fteqw.builtins(ModuleKind::Csqc);
    let menu = Engine::Fteqw.builtins(ModuleKind::Menuqc);
    assert!(!csqc.is_empty() && !menu.is_empty());
    let clearscene = Engine::Fteqw
        .builtin(ModuleKind::Csqc, BuiltinKey::Number(300))
        .unwrap();
    assert_eq!(clearscene.name, "clearscene");
    assert!(
        Engine::Fteqw
            .builtin(ModuleKind::Ssqc, BuiltinKey::Number(300))
            .is_none()
    );
    assert!(
        Engine::Fteqw
            .builtin(ModuleKind::Csqc, BuiltinKey::Number(78))
            .is_none()
    );
    let menu_bprint = Engine::Fteqw
        .builtin(ModuleKind::Menuqc, BuiltinKey::Number(5))
        .unwrap();
    assert_eq!(menu_bprint.name, "bprint");
    let server_five = Engine::Fteqw
        .builtin(ModuleKind::Ssqc, BuiltinKey::Number(5))
        .unwrap();
    assert_eq!(server_five.name, "lightstylestatic");
    for kind in [ModuleKind::Csqc, ModuleKind::Menuqc] {
        let table = Engine::Fteqw.builtins(kind);
        let keys: HashSet<BuiltinKey> = table.iter().map(|b| b.key).collect();
        assert_eq!(keys.len(), table.len());
        for builtin in &table {
            assert_eq!(Engine::Fteqw.builtin(kind, builtin.key), Some(*builtin));
        }
    }
    let csqc_extensions = Engine::Fteqw.extensions(ModuleKind::Csqc);
    assert!(csqc_extensions.contains(&"FTE_QC_HASHTABLES"));
    assert!(!Engine::Fteqw.extensions(ModuleKind::Menuqc).is_empty());
}

// spec ENGT-3.11: Ironwail, vkQuake, FTEQW and DarkPlaces have client QuakeC tables, only FTEQW
// and DarkPlaces menu QuakeC tables, and every engine a server table
#[test]
fn which_engines_have_client_and_menu_tables() {
    for engine in Engine::ALL {
        assert!(engine.has_table(ModuleKind::Ssqc), "{}", engine.name());
        for kind in [ModuleKind::Csqc, ModuleKind::Menuqc] {
            let other = match kind {
                ModuleKind::Csqc => matches!(
                    engine,
                    Engine::Ironwail | Engine::VkQuake | Engine::Fteqw | Engine::DarkPlaces
                ),
                ModuleKind::Ssqc | ModuleKind::Menuqc => {
                    matches!(engine, Engine::Fteqw | Engine::DarkPlaces)
                }
            };
            assert_eq!(engine.has_table(kind), other, "{}", engine.name());
            if !other {
                assert!(engine.builtins(kind).is_empty(), "{}", engine.name());
                assert!(engine.builtin(kind, BuiltinKey::Number(1)).is_none());
                assert!(engine.extensions(kind).is_empty(), "{}", engine.name());
            }
        }
    }
}

// spec ENGT-4.8: a CSQC module targeting FTEQW is checked against its client builtins, and a
// server module against its server builtins
#[test]
fn a_module_is_checked_against_its_kinds_table() {
    assert!(check_kind(Engine::Fteqw, ModuleKind::Csqc, &[CLEARSCENE_CALL]).is_empty());
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Ssqc, &[CLEARSCENE_CALL]),
        ["clearscene is builtin #300, which FTEQW does not have"]
    );
    assert!(check_kind(Engine::Fteqw, ModuleKind::Ssqc, &[SETSPAWNPARMS_CALL]).is_empty());
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Csqc, &[SETSPAWNPARMS_CALL]),
        ["setspawnparms is builtin #78, which FTEQW does not have"]
    );
    assert!(check_kind(Engine::Fteqw, ModuleKind::Menuqc, &[GETMOUSEPOS_CALL]).is_empty());
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Csqc, &[GETMOUSEPOS_CALL]).len(),
        1
    );
}

// spec ENGT-4.8: the extensions in a message come from the tables of the module's kind, so a
// server builtin's extensions are not named for a menu module
#[test]
fn extensions_come_from_the_kinds_tables() {
    let source = "\
void(entity client, float flags, string s) stuffcmdflags = #0:stuffcmdflags;
void() w = { stuffcmdflags(world, 0, \"x\"); };
";
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Menuqc, &[source]),
        ["stuffcmdflags is builtin #0:stuffcmdflags, which FTEQW does not have"]
    );
    assert_eq!(
        check_kind(Engine::Kex, ModuleKind::Ssqc, &[source]),
        [
            "stuffcmdflags is builtin #0:stuffcmdflags, which KEX does not have \
          (FTE_QC_STUFFCMDFLAGS)"
        ]
    );
    let shader = "void() shaderforname = #238;\nvoid() w = { shaderforname(); };\n";
    assert!(check_kind(Engine::Fteqw, ModuleKind::Csqc, &[shader]).is_empty());
    assert_eq!(
        Engine::Fteqw
            .builtin(ModuleKind::Csqc, BuiltinKey::Number(238))
            .unwrap()
            .extensions,
        ["FTE_FORCESHADER"]
    );
}

// spec ENGT-4.8: a module of a kind its engine has no table for is not checked
#[test]
fn a_kind_without_a_table_is_not_checked() {
    for engine in Engine::ALL {
        for kind in [ModuleKind::Csqc, ModuleKind::Menuqc] {
            if engine.has_table(kind) {
                continue;
            }
            for source in [HASH_CALL, CLEARSCENE_CALL, SETSPAWNPARMS_CALL] {
                assert!(
                    check_kind(engine, kind, &[source]).is_empty(),
                    "{}",
                    engine.name()
                );
            }
        }
    }
}

const PARTICLE_CALL: &str = "\
float(float max_themes) initparticlespawner = #522;
void() w = { initparticlespawner(8); };
";
const CHR_CALL: &str = "string(float ascii) chr = #78;\nvoid() w = { chr(65); };\n";

// spec ENGT-2.10, ENGT-3.11: DarkPlaces' client table is csprogsdefs.qc's and its menu table
// menudefs.qc's, each from #1 with no layer of id's table, with their own extension names
#[test]
fn darkplaces_has_client_and_menu_tables() {
    let client = Engine::DarkPlaces.builtins(ModuleKind::Csqc);
    let menu = Engine::DarkPlaces.builtins(ModuleKind::Menuqc);
    assert_eq!(client[0].key, BuiltinKey::Number(1));
    assert_eq!(client[0].name, "makevectors");
    assert_eq!(menu[0].key, BuiltinKey::Number(1));
    assert_eq!(menu[0].name, "checkextension");
    let server_six = Engine::DarkPlaces
        .builtin(ModuleKind::Ssqc, BuiltinKey::Number(6))
        .unwrap();
    let client_six = Engine::DarkPlaces
        .builtin(ModuleKind::Csqc, BuiltinKey::Number(6))
        .unwrap();
    assert_eq!(server_six.name, "break");
    assert_eq!(client_six.name, "break_to_debugger");
    assert!(
        Engine::DarkPlaces
            .builtin(ModuleKind::Csqc, BuiltinKey::Number(78))
            .is_none()
    );
    let chr = Engine::DarkPlaces
        .builtin(ModuleKind::Menuqc, BuiltinKey::Number(78))
        .unwrap();
    assert_eq!((chr.name, chr.signature), ("chr", "string(float ascii)"));
    let particles = Engine::DarkPlaces
        .builtin(ModuleKind::Csqc, BuiltinKey::Number(522))
        .unwrap();
    assert_eq!(particles.extensions, ["DP_CSQC_SPAWNPARTICLE"]);
    for kind in [ModuleKind::Csqc, ModuleKind::Menuqc] {
        let table = Engine::DarkPlaces.builtins(kind);
        let keys: HashSet<BuiltinKey> = table.iter().map(|b| b.key).collect();
        assert_eq!(keys.len(), table.len());
        for builtin in &table {
            assert_eq!(
                Engine::DarkPlaces.builtin(kind, builtin.key),
                Some(*builtin)
            );
        }
    }
    let client_extensions = Engine::DarkPlaces.extensions(ModuleKind::Csqc);
    assert!(client_extensions.contains(&"DP_CSQC_SPAWNPARTICLE"));
    assert!(client_extensions.contains(&"DP_CSQC_ENTITYTRANSPARENTSORTING_OFFSET"));
    let menu_extensions = Engine::DarkPlaces.extensions(ModuleKind::Menuqc);
    assert!(menu_extensions.contains(&"DP_QC_URI_GET"));
    assert_ne!(client_extensions, menu_extensions);
}

// spec ENGT-3.11, ENGT-4.8: a CSQC module targeting DarkPlaces that calls an FTE-only client
// builtin is W0311 naming DarkPlaces and FTEQW's extension; DarkPlaces' own client builtins
// are not
#[test]
fn a_darkplaces_client_module_reports_an_fte_builtin() {
    assert_eq!(
        check_kind(Engine::DarkPlaces, ModuleKind::Csqc, &[HASH_CALL]),
        ["hash_createtab is builtin #287, which DarkPlaces does not have (FTE_QC_HASHTABLES)"]
    );
    assert!(check_kind(Engine::DarkPlaces, ModuleKind::Csqc, &[CLEARSCENE_CALL]).is_empty());
    assert!(check_kind(Engine::DarkPlaces, ModuleKind::Csqc, &[PARTICLE_CALL]).is_empty());
    assert_eq!(
        check_kind(Engine::DarkPlaces, ModuleKind::Csqc, &[SETSPAWNPARMS_CALL]),
        ["setspawnparms is builtin #78, which DarkPlaces does not have"]
    );
}

// spec ENGT-4.8: a DarkPlaces-only client builtin targeting FTEQW is W0311 with the extension
// DarkPlaces' client table gives it
#[test]
fn an_fteqw_client_module_reports_a_darkplaces_builtin() {
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Csqc, &[PARTICLE_CALL]),
        [
            "initparticlespawner is builtin #522, which FTEQW does not have \
          (DP_CSQC_SPAWNPARTICLE)"
        ]
    );
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Ssqc, &[PARTICLE_CALL]),
        ["initparticlespawner is builtin #522, which FTEQW does not have"]
    );
}

// spec ENGT-3.11, ENGT-4.8: a menu module targeting DarkPlaces is checked against menudefs.qc's
// builtins
#[test]
fn a_darkplaces_menu_module_uses_its_menu_table() {
    assert!(check_kind(Engine::DarkPlaces, ModuleKind::Menuqc, &[GETMOUSEPOS_CALL]).is_empty());
    assert!(check_kind(Engine::DarkPlaces, ModuleKind::Menuqc, &[CHR_CALL]).is_empty());
    assert_eq!(
        check_kind(Engine::Fteqw, ModuleKind::Menuqc, &[CHR_CALL]),
        ["chr is builtin #78, which FTEQW does not have"]
    );
    assert_eq!(
        check_kind(Engine::DarkPlaces, ModuleKind::Menuqc, &[CLEARSCENE_CALL]),
        ["clearscene is builtin #300, which DarkPlaces does not have"]
    );
}

// spec ENGT-3.11, ENGT-4.8: a quakec.toml targeting dp opens with a client module, and calling
// an FTE-only client builtin in it is W0311 naming DarkPlaces
#[test]
fn a_dp_project_client_module_reports_an_fte_builtin() {
    let manifest =
        "[project]\ntarget = \"dp\"\n[[module]]\nkind = \"csqc\"\nsrc = \"csprogs.src\"\n";
    let mut loader = MemoryLoader::new();
    loader.insert("proj/quakec.toml", manifest.as_bytes());
    loader.insert("proj/csprogs.src", b"csprogs.dat\nmain.qc\n");
    loader.insert("proj/main.qc", HASH_CALL.as_bytes());
    let project = open_dir(&mut loader, Path::new("proj")).unwrap();
    let module = &project.modules[0];
    assert_eq!(module.target, Target::Dp);
    assert_eq!(module.kind, ModuleKind::Csqc);
    assert_eq!(
        check_kind(
            Engine::from_target(module.target),
            module.kind,
            &[HASH_CALL]
        ),
        ["hash_createtab is builtin #287, which DarkPlaces does not have (FTE_QC_HASHTABLES)"]
    );
}

// spec ENGT-4.5: (P2-R3 review) a zero is any literal whose digits are all zeros, so `#00`
// and `#0.00` are keyed by the name after the `:`, and `#0.5`, whose digits are not, is no
// key and gives no W0311 (ENGT-4.3)
#[test]
fn reviewer_only_all_zero_digits_key_by_name() {
    let call = |number: &str| {
        let source = format!("void() f = #{number}:nothing;\nvoid() w = {{ f(); }};\n");
        fte_messages(Engine::Kex, &[source.as_str()])
    };
    for zero in ["0", "00", "0.0", "0.00", "00.0"] {
        assert_eq!(
            call(zero),
            ["f is builtin #0:nothing, which KEX does not have"],
            "{zero}"
        );
    }
    for not_zero in ["0.5", "0.01", "00.10"] {
        assert!(call(not_zero).is_empty(), "{not_zero}");
    }
}

// spec ENGT-4.5: (P2-R3 review) the name of `#0:NAME` is the one after the `:`, however the
// initialiser is spaced
#[test]
fn reviewer_the_name_follows_the_colon() {
    let source = "void() f = # 0 : nothing ;\nvoid() w = { f(); };\n";
    assert_eq!(
        fte_messages(Engine::Kex, &[source]),
        ["f is builtin #0:nothing, which KEX does not have"]
    );
}

// spec ENGT-3.9, ENGT-4.2: a QuakeSpasm-only builtin is no W0311 targeting QuakeSpasm, and
// still one targeting Vanilla
#[test]
fn a_quakespasm_builtin_targeting_quakespasm_is_not_w0311() {
    let finale = "float() finaleFinished = #79;\nvoid() w = { finaleFinished(); };\n";
    assert!(check(Engine::QuakeSpasm, &[finale]).is_empty());
    assert_eq!(
        messages(Engine::Vanilla, &[finale]),
        ["finaleFinished is builtin #79, which Vanilla does not have (QW_ENGINE)"]
    );
    let strlen = "float(string s) strlen = #114;\nvoid() w = { strlen(\"a\"); };\n";
    for engine in [Engine::Ironwail, Engine::VkQuake] {
        assert!(check(engine, &[strlen]).is_empty(), "{}", engine.name());
    }
    let draw = "void() draw_point = #0:ex_draw_point;\nvoid() w = { draw_point(); };\n";
    for engine in [Engine::QuakeSpasm, Engine::VkQuake] {
        assert_eq!(
            fte_messages(engine, &[draw]),
            [format!(
                "draw_point is builtin #0:ex_draw_point, which {} does not have",
                engine.name()
            )]
        );
    }
    assert!(fte_messages(Engine::Kex, &[draw]).is_empty());
}

// spec ENGT-3.11, ENGT-4.8: client modules targeting Ironwail and vkQuake are checked against
// their client builtins
#[test]
fn ironwail_and_vkquake_client_modules_are_checked() {
    let drawpic = "\
float(vector position, string pic, vector size, vector rgb, float alpha, float flag) drawpic = #322;
void() w = { drawpic('0 0 0', \"a\", '1 1 0', '1 1 1', 1, 0); };
";
    for engine in [Engine::Ironwail, Engine::VkQuake] {
        assert!(
            check_kind(engine, ModuleKind::Csqc, &[drawpic]).is_empty(),
            "{}",
            engine.name()
        );
        assert_eq!(
            check_kind(engine, ModuleKind::Csqc, &[HASH_CALL]),
            [format!(
                "hash_createtab is builtin #287, which {} does not have (FTE_QC_HASHTABLES)",
                engine.name()
            )]
        );
        assert_eq!(
            check_kind(engine, ModuleKind::Csqc, &[SETSPAWNPARMS_CALL]),
            [format!(
                "setspawnparms is builtin #78, which {} does not have",
                engine.name()
            )]
        );
    }
    assert_eq!(
        check_kind(Engine::Ironwail, ModuleKind::Ssqc, &[drawpic]),
        ["drawpic is builtin #322, which Ironwail does not have"]
    );
}
