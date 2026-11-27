// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use qc_engines::{Bound, Bsp2, Engine, MapFormat, MapLimit, Measure, Past};
use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_map_edit::{LumpCounts, MAP_ISSUES, MAP_OPEN, MAP_SET_KEY, MapIssues, Written};
use serde_json::json;

use super::{
    BUDGET_NEAR_SHARE, BudgetLine, EngineBudget, LineBound, LumpSource, MAP_BUDGET, MapBudget,
    Measured, Spawned, Standing, Used, engine_budget, issues, room, worst_first,
};
use crate::map_compile::MAP_COMPILE;
use crate::map_compile::tests::Context;
use crate::map_filter::Skill;
use crate::preview::{Preview, PreviewConfig};
use crate::vm_run::tests::system_defs;
use crate::{Config, OPEN_PROJECT, Workbench, register, register_preview};

/// A program whose `info_thing` takes an edict and precaches a model and a sound, and whose
/// world sets light styles 0 and 5.
const CODE: &str = r#"
string(string s) precache_model = #20;
string(string s) precache_sound = #19;
void(float style, string value) lightstyle = #35;
void() worldspawn = { precache_model("progs/player.mdl"); lightstyle(0, "m"); lightstyle(5, "a"); };
void() info_thing = { precache_model("progs/thing.mdl"); precache_sound("thing.wav"); };
void() info_other = { precache_sound("other.wav"); };
"#;

/// The world's entity, a sealed box with a player start inside.
const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n\
    {\n( -64 -64 -16 ) ( -64 -63 -16 ) ( -64 -64 -15 ) wall 0 0 0 1 1\n\
    ( 64 64 16 ) ( 64 64 17 ) ( 64 65 16 ) wall 0 0 0 1 1\n\
    ( -64 -64 -16 ) ( -64 -64 -15 ) ( -63 -64 -16 ) wall 0 0 0 1 1\n\
    ( 64 64 16 ) ( 65 64 16 ) ( 64 64 17 ) wall 0 0 0 1 1\n\
    ( -64 -64 -16 ) ( -63 -64 -16 ) ( -64 -63 -16 ) wall 0 0 0 1 1\n\
    ( 64 64 16 ) ( 64 65 16 ) ( 65 64 16 ) wall 0 0 0 1 1\n}\n}\n";

/// A project file of `target`.
fn manifest(target: &str) -> String {
    format!(
        "[project]\ndialect = \"vanilla\"\ntarget = \"{target}\"\n\n\
         [[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n"
    )
}

/// What a test project holds.
struct ProjectSpec<'a> {
    name: &'a str,
    /// The target, `None` for no project file (id's engine).
    target: Option<&'a str>,
    /// The entities after the world.
    entities: String,
    /// Whether a project is opened.
    project: Opened,
}

/// Whether a test opens the project.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opened {
    Project,
    MapOnly,
}

impl Default for ProjectSpec<'_> {
    fn default() -> Self {
        Self {
            name: "budget",
            target: Some("quakespasm"),
            entities: entity("info_thing", ""),
            project: Opened::Project,
        }
    }
}

fn entity(classname: &str, extra: &str) -> String {
    format!("{{\n\"classname\" \"{classname}\"\n\"origin\" \"0 0 0\"\n{extra}}}\n")
}

/// `count` entities of `classname`.
fn entities(classname: &str, count: usize) -> String {
    let mut text = String::new();
    for _ in 0..count {
        _ = write!(text, "{}", entity(classname, ""));
    }
    text
}

/// A session with the project of `spec` and its map open.
fn open_session(spec: &ProjectSpec<'_>) -> (Session<Context>, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("qs-wb-budget-{}-{}", spec.name, std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("defs.qc"), system_defs()).unwrap();
    std::fs::write(dir.join("world.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    if let Some(target) = spec.target {
        std::fs::write(dir.join("quakec.toml"), manifest(target)).unwrap();
    }
    let map = dir.join("maps/test.map");
    std::fs::write(&map, format!("{WORLD}{}", spec.entities)).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context::new(
        Workbench::new(FsLoader, Config::default()),
        Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    );
    let mut session = Session::new(context, registry);
    if spec.project == Opened::Project {
        session
            .execute(OPEN_PROJECT, json!({ "path": dir }))
            .unwrap();
    }
    session.execute(MAP_OPEN, json!({ "path": map })).unwrap();
    (session, dir)
}

fn budget(session: &mut Session<Context>) -> MapBudget {
    serde_json::from_value(session.execute(MAP_BUDGET, json!({})).unwrap()).unwrap()
}

fn line(budget: &EngineBudget, limit: MapLimit) -> &BudgetLine {
    budget
        .lines
        .iter()
        .find(|line| line.limit == limit)
        .unwrap()
}

fn count(line: &BudgetLine) -> usize {
    match line.used {
        Used::Count { count, .. } => count,
        Used::NotMeasured { ref why } => panic!("{:?} not measured: {why}", line.limit),
    }
}

fn issue_messages(session: &mut Session<Context>) -> Vec<String> {
    let found: MapIssues =
        serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
    found
        .issues
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

/// What a map measures as: every lump and spawn count `count`, in `format`.
fn measured(count: usize, format: MapFormat) -> Measured {
    let lumps = LumpCounts {
        models: count,
        entities: count,
        entity_text: count,
        planes: count,
        nodes: count,
        clipnodes: count,
        leafs: count,
        visleafs: count,
        vertexes: count,
        faces: count,
        marksurfaces: count,
        texinfo: count,
        edges: count,
        surfedges: count,
        textures: count,
        lighting: count,
        visibility: count,
        face_extent: count,
        fitz_face_extent: count,
    };
    let at = (count, Some(Skill::Easy));
    Measured {
        lumps: Ok((lumps, LumpSource::Bsp)),
        format,
        spawned: Ok(Spawned {
            edicts: at,
            models: at,
            sounds: at,
            styles: at,
        }),
    }
}

// spec BUDGET-1.1: map.budget is in the registry, reads nothing it changes, needs an open map
// and refuses a parameter
#[test]
fn the_command_needs_a_map() {
    let (mut session, _dir) = open_session(&ProjectSpec::default());
    assert!(session.execute(MAP_BUDGET, json!({ "skill": 1 })).is_err());
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut bare = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    let error = bare.execute(MAP_BUDGET, json!({})).unwrap_err();
    assert!(error.to_string().contains("map"), "{error}");
}

// spec BUDGET-1.2: the target's budget, then id's; with no project id's alone, and the spawn
// counts not measured (BUDGET-2.5)
#[test]
fn the_target_then_ids() {
    let (mut session, _dir) = open_session(&ProjectSpec::default());
    let found = budget(&mut session);
    let names: Vec<&str> = found.engines.iter().map(|e| e.engine.as_str()).collect();
    assert_eq!(names, ["quakespasm", "vanilla"]);
    assert_eq!(found.engines[0].bsp2, Bsp2::Reads);
    assert_eq!(found.engines[1].bsp2, Bsp2::DoesNotRead);
    assert_eq!(found.engines[0].lines.len(), MapLimit::ALL.len());
    let (mut alone, _dir) = open_session(&ProjectSpec {
        name: "alone",
        project: Opened::MapOnly,
        ..ProjectSpec::default()
    });
    let found = budget(&mut alone);
    assert_eq!(found.engines.len(), 1);
    assert_eq!(found.engines[0].engine, "vanilla");
    let edicts = line(&found.engines[0], MapLimit::Edicts);
    assert!(matches!(edicts.used, Used::NotMeasured { .. }));
    assert_eq!(edicts.standing, Standing::Unknown);
    // A target of id's engine answers once.
    let (mut quake, _dir) = open_session(&ProjectSpec {
        name: "quake",
        target: Some("quake"),
        ..ProjectSpec::default()
    });
    assert_eq!(budget(&mut quake).engines.len(), 1);
}

// spec BUDGET-2.1: before a compile the lumps are not measured; after one they are the
// report's, but for lighting and visibility; BUDGET-2.2: the format is the compile's
#[test]
fn the_lumps_come_from_the_compile() {
    let (mut session, _dir) = open_session(&ProjectSpec::default());
    let before = budget(&mut session);
    assert_eq!(before.format, None);
    let clipnodes = line(&before.engines[0], MapLimit::Clipnodes);
    assert!(matches!(&clipnodes.used, Used::NotMeasured { why } if why.contains("compiled")));
    session.execute(MAP_COMPILE, json!({})).unwrap();
    let after = budget(&mut session);
    assert_eq!(after.format, Some(Written::Bsp29));
    for engine in &after.engines {
        assert!(count(line(engine, MapLimit::Faces)) > 0);
        assert_eq!(count(line(engine, MapLimit::Models)), 1);
        assert!(count(line(engine, MapLimit::FaceExtent)) > 0);
        let lit = line(engine, MapLimit::Lighting);
        assert!(matches!(&lit.used, Used::NotMeasured { why } if why.contains("lights")));
    }
}

// spec BUDGET-2.3: the spawn counts edicts, precaches after entry 0 and light styles set, the
// most of the skills with the first skill that has it
#[test]
fn the_spawn_counts() {
    // One info_thing is not on easy (256): easy has one edict and one model fewer.
    let text = format!(
        "{}{}{}",
        entity("info_thing", "\"spawnflags\" \"256\"\n"),
        entity("info_thing", ""),
        entity("info_other", "")
    );
    let (mut session, _dir) = open_session(&ProjectSpec {
        name: "skills",
        entities: text,
        ..ProjectSpec::default()
    });
    let found = budget(&mut session);
    let target = &found.engines[0];
    let edicts = line(target, MapLimit::Edicts);
    // The world, one client slot and three entities.
    assert_eq!(
        edicts.used,
        Used::Count {
            count: 5,
            skill: Some(Skill::Normal)
        }
    );
    // The world model, player.mdl and thing.mdl.
    assert_eq!(count(line(target, MapLimit::ModelPrecaches)), 3);
    assert_eq!(count(line(target, MapLimit::SoundPrecaches)), 2);
    assert_eq!(
        line(target, MapLimit::SoundPrecaches).used,
        Used::Count {
            count: 2,
            skill: Some(Skill::Easy)
        }
    );
    // Styles 0 and 5 set: two styles.
    assert_eq!(count(line(target, MapLimit::LightStyles)), 2);
}

// spec BUDGET-2.4: a map of 601 edicts is spawned and counted: past id's 600 and inside
// QuakeSpasm's 8192, and id's past joins map.issues only for an id target
#[test]
fn six_hundred_and_one_edicts() {
    // The world and one client slot take edicts 0 and 1.
    let (mut session, _dir) = open_session(&ProjectSpec {
        name: "601",
        entities: entities("info_thing", 599),
        ..ProjectSpec::default()
    });
    let found = budget(&mut session);
    let quakespasm = line(&found.engines[0], MapLimit::Edicts);
    let vanilla = line(&found.engines[1], MapLimit::Edicts);
    assert_eq!(count(quakespasm), 601);
    assert_eq!(quakespasm.standing, Standing::Within);
    assert_eq!(vanilla.standing, Standing::Past);
    assert!(matches!(
        vanilla.bound,
        LineBound::At {
            most: 600,
            past: Past::Refused,
            ..
        }
    ));
    // The worst comes first.
    assert_eq!(found.engines[1].lines[0].limit, MapLimit::Edicts);
    let (mut id, _dir) = open_session(&ProjectSpec {
        name: "601-id",
        target: Some("quake"),
        entities: entities("info_thing", 599),
        ..ProjectSpec::default()
    });
    budget(&mut id);
    let messages = issue_messages(&mut id);
    assert!(
        messages
            .iter()
            .any(|m| m == "601 edicts is past Vanilla's limit of 600: Vanilla refuses it"),
        "{messages:?}"
    );
}

// spec BUDGET-2.4: the spawn's room is one past the largest bound of the engines answered for,
// id's when none has one
#[test]
fn the_spawn_has_room() {
    let roomy = room(&[Engine::QuakeSpasm, Engine::Vanilla]);
    assert_eq!(roomy.edicts.get(), 8193);
    let precaches = roomy.precaches.unwrap();
    assert_eq!((precaches.models, precaches.sounds), (4097, 2049));
    let kex = room(&[Engine::Kex, Engine::Vanilla]);
    assert_eq!(kex.edicts.get(), 601);
    assert_eq!(kex.precaches.unwrap().models, 257);
}

// spec BUDGET-2.5: a program that does not build leaves the spawn counts not measured
#[test]
fn a_program_that_does_not_build() {
    let (mut session, dir) = open_session(&ProjectSpec {
        name: "broken",
        ..ProjectSpec::default()
    });
    std::fs::write(dir.join("world.qc"), "void() worldspawn = {").unwrap();
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    let found = budget(&mut session);
    let edicts = line(&found.engines[0], MapLimit::Edicts);
    assert!(
        matches!(edicts.used, Used::NotMeasured { .. }),
        "{edicts:?}"
    );
}

// spec ENGT-5.7, BUDGET-1.3, BUDGET-3.1: for every engine and every limit with a bound, a map at the bound
// is not past it and one above it is, and the issue names the limit and the engine
#[test]
fn each_limit_at_and_above_its_bound() {
    for engine in Engine::ALL {
        for limit in MapLimit::ALL {
            let Bound::At(fact) = engine.bound(limit, MapFormat::Bsp29) else {
                continue;
            };
            let at = engine_budget(engine, &measured(fact.most, MapFormat::Bsp29));
            let at_line = line(&at, limit);
            assert_ne!(at_line.standing, Standing::Past, "{engine:?} {limit:?}");
            assert_eq!(at_line.standing, Standing::Near, "{engine:?} {limit:?}");
            let above = engine_budget(engine, &measured(fact.most + 1, MapFormat::Bsp29));
            assert_eq!(
                line(&above, limit).standing,
                Standing::Past,
                "{engine:?} {limit:?}"
            );
            let said = issues(&above, engine, None);
            let wanted = format!(
                "{} {} is past {}'s limit of {}",
                fact.most + 1,
                limit.name(),
                engine.name(),
                fact.most
            );
            assert!(
                said.iter().any(|m| m.starts_with(&wanted)),
                "{wanted}: {said:?}"
            );
        }
    }
}

// spec BUDGET-1.3: the share and standing: near from BUDGET_NEAR_SHARE, within below, within
// with no share where the engine sets no bound, unknown for KEX
#[test]
fn shares_and_standings() {
    let near = engine_budget(Engine::Vanilla, &measured(540, MapFormat::Bsp29));
    let edicts = line(&near, MapLimit::Edicts);
    assert_eq!(edicts.share, Some(0.9));
    assert!(edicts.share.unwrap() >= BUDGET_NEAR_SHARE);
    assert_eq!(edicts.standing, Standing::Near);
    let within = engine_budget(Engine::Vanilla, &measured(539, MapFormat::Bsp29));
    assert_eq!(line(&within, MapLimit::Edicts).standing, Standing::Within);
    let unlimited = engine_budget(Engine::DarkPlaces, &measured(70000, MapFormat::Bsp29));
    let clipnodes = line(&unlimited, MapLimit::Clipnodes);
    assert_eq!(
        (clipnodes.share, clipnodes.standing),
        (None, Standing::Within)
    );
    let kex = engine_budget(Engine::Kex, &measured(1, MapFormat::Bsp29));
    assert!(kex.lines.iter().all(|l| l.standing == Standing::Unknown));
    assert!(kex.lines.iter().all(|l| l.bound == LineBound::Unknown));
    // Near is an issue too.
    let said = issues(&near, Engine::Vanilla, None);
    assert!(
        said.contains(&"540 edicts is 90% of Vanilla's limit of 600".to_owned()),
        "{said:?}"
    );
}

// spec BUDGET-1.4: past first, then near, within and unknown, each by share, largest first
#[test]
fn the_worst_comes_first() {
    let mut measured = measured(100, MapFormat::Bsp29);
    if let Ok((lumps, _)) = &mut measured.lumps {
        lumps.clipnodes = 32700;
        lumps.faces = 70000;
        lumps.entities = 1000;
    }
    measured.spawned = Err("no project".to_owned());
    let budget = engine_budget(Engine::Vanilla, &measured);
    let order: Vec<MapLimit> = budget.lines.iter().take(3).map(|l| l.limit).collect();
    assert_eq!(
        order,
        [MapLimit::Faces, MapLimit::Clipnodes, MapLimit::Entities]
    );
    let standings: Vec<Standing> = budget.lines.iter().map(|l| l.standing).collect();
    let mut sorted = standings.clone();
    sorted.sort();
    assert_eq!(standings, sorted);
    assert_eq!(budget.lines.last().unwrap().standing, Standing::Unknown);
    let mut lines = budget.lines.clone();
    lines.reverse();
    worst_first(&mut lines);
    assert_eq!(lines, budget.lines);
}

// spec BUDGET-3.2: a map past 32767 clipnodes, compiled as BSP2, is offered BSP2 for an engine
// that reads it, and is an error naming the lump for one that does not
#[test]
fn past_bsp29_is_offered_bsp2() {
    let mut measured = measured(10, MapFormat::Bsp2);
    if let Ok((lumps, _)) = &mut measured.lumps {
        lumps.clipnodes = 40000;
    }
    let written = Written::Bsp2 {
        lump: "clipnodes".to_owned(),
    };
    let quakespasm = engine_budget(Engine::QuakeSpasm, &measured);
    // QuakeSpasm's 32767 is BSP29's loader's: a BSP2 map is within.
    assert_eq!(
        line(&quakespasm, MapLimit::Clipnodes).standing,
        Standing::Within
    );
    let said = issues(&quakespasm, Engine::QuakeSpasm, Some(&written));
    assert_eq!(
        said,
        [
            "the clipnodes are too many for BSP29, so the map is compiled as BSP2, which \
          QuakeSpasm reads"
        ]
    );
    let vanilla = engine_budget(Engine::Vanilla, &measured);
    let said = issues(&vanilla, Engine::Vanilla, Some(&written));
    assert!(
        said.contains(
            &"40000 clipnodes is past Vanilla's limit of 32767: id's compiler cannot write it"
                .to_owned()
        )
    );
    assert!(
        said.contains(
            &"the clipnodes are too many for BSP29, so the map is compiled as BSP2, which Vanilla \
          does not read: it will not load"
                .to_owned()
        )
    );
    let kex = engine_budget(Engine::Kex, &measured);
    let said = issues(&kex, Engine::Kex, Some(&written));
    assert_eq!(
        said,
        [
            "the clipnodes are too many for BSP29, so the map is compiled as BSP2, and whether \
          KEX reads BSP2 is not known"
        ]
    );
    // A BSP opened as BSP2 names no lump.
    let opened = Written::Bsp2 {
        lump: String::new(),
    };
    assert_eq!(
        issues(&quakespasm, Engine::QuakeSpasm, Some(&opened)),
        ["the map is BSP2, which QuakeSpasm reads"]
    );
}

// spec ENGT-5.5, BUDGET-3.1: each past says what the engine does
#[test]
fn what_each_past_says() {
    let measured = measured(1_000_000, MapFormat::Bsp29);
    let said = issues(
        &engine_budget(Engine::QuakeSpasm, &measured),
        Engine::QuakeSpasm,
        None,
    );
    for end in [
        "QuakeSpasm refuses it",
        "QuakeSpasm loads it with a warning",
    ] {
        assert!(said.iter().any(|m| m.ends_with(end)), "{end}: {said:?}");
    }
    let said = issues(
        &engine_budget(Engine::Vanilla, &measured),
        Engine::Vanilla,
        None,
    );
    for end in [
        "id's compiler cannot write it",
        "Vanilla writes past its array, which is undefined",
    ] {
        assert!(said.iter().any(|m| m.ends_with(end)), "{end}: {said:?}");
    }
}

// spec BUDGET-3.3: the issues join map.issues until the map changes
#[test]
fn the_issues_are_kept_while_the_map_is_the_same() {
    let (mut session, _dir) = open_session(&ProjectSpec {
        name: "kept",
        target: Some("quake"),
        entities: entities("info_thing", 545),
        ..ProjectSpec::default()
    });
    budget(&mut session);
    let near = |messages: &[String]| messages.iter().any(|m| m.contains("edicts is 91%"));
    assert!(
        near(&issue_messages(&mut session)),
        "{:?}",
        issue_messages(&mut session)
    );
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "origin", "value": "8 0 0" }),
        )
        .unwrap();
    assert!(!near(&issue_messages(&mut session)));
}

// spec ENGT-5.4: the spawn limits are the four of a spawn
#[test]
fn the_spawn_limits_are_measured_in_a_spawn() {
    let at = measured(3, MapFormat::Bsp29);
    for limit in MapLimit::ALL {
        let used = at.used(limit, Engine::QuakeSpasm);
        let skill = match used {
            Used::Count { skill, .. } => skill,
            Used::NotMeasured { .. } => None,
        };
        assert_eq!(
            skill.is_some(),
            limit.measured_by() == Measure::Spawn,
            "{limit:?}"
        );
    }
}
