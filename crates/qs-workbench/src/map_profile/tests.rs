// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use serde_json::json;

use super::{
    ExpandError, Expansions, FAST_PROFILE, FULL_PROFILE, LightSpeed, MAP_CHOOSE_PROFILE,
    MAP_PROFILES, MAP_RUN_PROFILE, PROFILES_FILE, Profile, ProfileRun, Task, TaskError, TaskResult,
    Variable, VisSpeed,
};
use crate::OPEN_PROJECT;
use crate::map_compile::tests::{Context, open};
use qs_commands::Session;
use qs_map_edit::MAP_OPEN;
use qs_map_edit::Seal;
use qs_map_edit::testing::{Opening, room};

fn expansions() -> Expansions {
    Expansions::new(
        PathBuf::from("/maps/start.map"),
        Some(PathBuf::from("/proj")),
        None,
    )
}

// No spec statement: P6-20: a task's path expands each `${NAME}` to its variable's value
#[test]
fn paths_expand_their_variables() {
    let e = expansions();
    assert_eq!(
        e.expand("${MAP_DIR}/${MAP_NAME}.lit from ${MAP} into ${PROJECT}/id1, ${BSP}")
            .unwrap(),
        "/maps/start.lit from /maps/start.map into /proj/id1, /maps/start.bsp"
    );
    assert_eq!(e.expand("no names").unwrap(), "no names");
    assert_eq!(e.expand("").unwrap(), "");
    assert_eq!(
        e.expand("${MOD}"),
        Err(ExpandError::Unknown {
            name: "MOD".to_owned()
        })
    );
    assert_eq!(
        e.expand("${GAME}/id1"),
        Err(ExpandError::NoValue {
            name: "GAME".to_owned()
        })
    );
    assert_eq!(
        e.expand("${MAP"),
        Err(ExpandError::Unclosed {
            text: "${MAP".to_owned()
        })
    );
    for variable in Variable::ALL {
        let written = format!("${{{}}}", variable.name());
        assert_eq!(e.expand(&written).ok(), e.value(variable), "{written}");
    }
}

/// A session with `name`'s sealed room open, a light added, in a project made of the room's folder holding
/// `profiles` as its profiles file when given; the room's path.
fn project(name: &str, profiles: Option<&str>) -> (Session<Context>, PathBuf) {
    let map = room(name, "wall", Opening::Sealed);
    let mut text = std::fs::read(&map).unwrap();
    text.extend_from_slice(b"{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n}\n");
    std::fs::write(&map, text).unwrap();
    let dir = map.parent().unwrap().to_path_buf();
    std::fs::write(
        dir.join("quakec.toml"),
        "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(dir.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    if let Some(profiles) = profiles {
        std::fs::write(dir.join(PROFILES_FILE), profiles).unwrap();
    }
    let mut session = open(&map);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session.execute(MAP_OPEN, json!({ "path": map })).unwrap();
    (session, map)
}

fn profiles(session: &mut Session<Context>) -> Vec<Profile> {
    serde_json::from_value(session.execute(MAP_PROFILES, json!({})).unwrap()).unwrap()
}

fn run(session: &mut Session<Context>, profile: &str) -> ProfileRun {
    serde_json::from_value(
        session
            .execute(MAP_RUN_PROFILE, json!({ "profile": profile }))
            .unwrap(),
    )
    .unwrap()
}

// No spec statement: P6-20: the built-in profiles come first; a project's profiles file adds
// its own and replaces a built-in of the same name; a file that is not profiles is an error
#[test]
fn a_project_adds_and_replaces_profiles() {
    let (mut session, _) = project("profiles-none", None);
    let names: Vec<String> = profiles(&mut session).into_iter().map(|p| p.name).collect();
    assert_eq!(names, [FULL_PROFILE, FAST_PROFILE]);
    let file = "[[profile]]\nname = \"Fast\"\ntasks = [{ task = \"bsp\" }]\n\n\
                [[profile]]\nname = \"Ship\"\ntasks = [{ task = \"bsp\" }, \
                { task = \"vis\", speed = \"fast\" }, { task = \"light\" }, \
                { task = \"copy\", from = \"${BSP}\", to = \"${PROJECT}/out/${MAP_NAME}.bsp\" }]\n";
    let (mut session, _) = project("profiles-file", Some(file));
    let found = profiles(&mut session);
    let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, [FULL_PROFILE, FAST_PROFILE, "Ship"]);
    assert_eq!(found[1].tasks, [Task::Bsp]);
    assert_eq!(
        found[2].tasks[1..3],
        [
            Task::Vis {
                speed: VisSpeed::Fast
            },
            Task::Light {
                speed: LightSpeed::Full
            },
        ]
    );
    let error = session
        .execute(MAP_CHOOSE_PROFILE, json!({ "profile": "Nope" }))
        .unwrap_err();
    assert!(error.to_string().contains("\"Nope\""), "{error}");
    assert_eq!(
        session.execute(MAP_CHOOSE_PROFILE, json!({})).unwrap(),
        json!(FULL_PROFILE)
    );
    let (mut session, _) = project("profiles-bad", Some("[[profile]]\nname = 3\n"));
    let error = session.execute(MAP_PROFILES, json!({})).unwrap_err();
    assert!(error.to_string().contains(PROFILES_FILE), "{error}");
    let error = session
        .execute(MAP_RUN_PROFILE, json!({ "profile": "Nope" }))
        .unwrap_err();
    assert!(error.to_string().contains(PROFILES_FILE), "{error}");
}

// No spec statement: P6-20: a profile compiles the map with qs-map-bsp, gives it visibility
// with qs-map-vis and lights it with qs-map-light, each task timed, then copies the BSP where
// it says
#[test]
fn a_profile_compiles_vis_and_lights_the_map() {
    let file = "[[profile]]\nname = \"Ship\"\ntasks = [{ task = \"bsp\" }, \
                { task = \"vis\" }, { task = \"light\", speed = \"fast\" }, \
                { task = \"copy\", from = \"${BSP}\", to = \"${PROJECT}/out/${MAP_NAME}.bsp\" }]\n";
    let (mut session, map) = project("profile-run", Some(file));
    let chosen = session
        .execute(MAP_CHOOSE_PROFILE, json!({ "profile": "Ship" }))
        .unwrap();
    assert_eq!(chosen, json!("Ship"));
    // With no profile named, the one chosen runs.
    let done: ProfileRun =
        serde_json::from_value(session.execute(MAP_RUN_PROFILE, json!({})).unwrap()).unwrap();
    assert!(done.succeeded(), "{done:?}");
    assert_eq!(done.profile, "Ship");
    assert_eq!(done.tasks.len(), 4);
    assert!(done.tasks.iter().all(|task| task.seconds >= 0.0));
    assert!(done.seconds >= done.tasks.iter().map(|task| task.seconds).sum::<f64>() * 0.5);
    assert_eq!(
        done.report.as_ref().map(|report| report.seal),
        Some(Seal::Sealed)
    );
    let bytes = std::fs::read(map.with_extension("bsp")).unwrap();
    let bsp = Bsp::read(&bytes, BspLoader::default()).unwrap();
    assert!(!bsp.visibility.is_empty(), "vis ran");
    assert!(!bsp.lighting.is_empty(), "light ran");
    assert!(map.with_extension("prt").exists());
    let copied = map.parent().unwrap().join("out").join("profile-run.bsp");
    assert_eq!(std::fs::read(copied).unwrap(), bytes);
}

// No spec statement: P6-20: a task that fails stops the profile, the tasks after it not run; a
// program's status other than success fails its task, and one that succeeds is done
#[test]
fn a_failed_task_stops_the_profile() {
    let me = std::env::current_exe().unwrap();
    let me = me.to_string_lossy();
    let file = format!(
        "[[profile]]\nname = \"Tools\"\ntasks = [\
         {{ task = \"tool\", program = {me:?}, args = [\"--list\", \"${{MAP_NAME}}\"] }}, \
         {{ task = \"copy\", from = \"${{MAP_DIR}}/missing.bsp\", to = \"${{MAP_DIR}}/x.bsp\" }}, \
         {{ task = \"bsp\" }}]\n\n\
         [[profile]]\nname = \"Bad tool\"\ntasks = [\
         {{ task = \"tool\", program = {me:?}, args = [\"--no-such-flag\"] }}]\n\n\
         [[profile]]\nname = \"No program\"\ntasks = [\
         {{ task = \"tool\", program = \"${{MAP_DIR}}/nothing-here\" }}]\n"
    );
    let (mut session, map) = project("profile-fail", Some(&file));
    let stopped = run(&mut session, "Tools");
    assert!(!stopped.succeeded());
    assert_eq!(stopped.tasks.len(), 2, "{stopped:?}");
    assert_eq!(stopped.tasks[0].result, TaskResult::Done);
    let TaskResult::Failed { message } = &stopped.tasks[1].result else {
        panic!("the copy fails: {stopped:?}");
    };
    assert!(message.contains("missing.bsp"), "{message}");
    assert!(!map.with_extension("bsp").exists(), "bsp did not run");
    for name in ["Bad tool", "No program"] {
        let failed = run(&mut session, name);
        assert!(
            matches!(
                failed.tasks[..],
                [super::TaskRun {
                    result: TaskResult::Failed { .. },
                    ..
                }]
            ),
            "{name}: {failed:?}"
        );
    }
    let error = session
        .execute(MAP_RUN_PROFILE, json!({ "profile": "Nope" }))
        .unwrap_err();
    assert!(error.to_string().contains("\"Nope\""), "{error}");
}

// No spec statement: P6-20: the built-in Full profile runs bsp, full vis and full light, then
// plays; Fast the same with fast vis and fast light
#[test]
fn the_built_in_profiles_compile_then_play() {
    let built = Profile::built_in();
    let tasks = |speed_vis, speed_light| {
        vec![
            Task::Bsp,
            Task::Vis { speed: speed_vis },
            Task::Light { speed: speed_light },
            Task::Launch,
        ]
    };
    assert_eq!(built[0].tasks, tasks(VisSpeed::Full, LightSpeed::Full));
    assert_eq!(built[1].tasks, tasks(VisSpeed::Fast, LightSpeed::Fast));
    // A task's name is its tag in a profiles file.
    for task in &built[0].tasks {
        let written = serde_json::to_value(task).unwrap();
        assert_eq!(written["task"], json!(task.name()));
    }
    let tool = Task::Tool {
        program: String::new(),
        args: Vec::new(),
    };
    let copy = Task::Copy {
        from: String::new(),
        to: String::new(),
    };
    for task in [tool, copy] {
        assert_eq!(
            serde_json::to_value(&task).unwrap()["task"],
            json!(task.name())
        );
    }
    assert!(Path::new(PROFILES_FILE).extension().is_some());
}

// No spec statement: P6-20: an expansion's errors and a task's say what went wrong, and a task's
// keeps the error under it
#[test]
fn expansion_and_task_errors_say_why() {
    let e = expansions();
    let said = |text: &str| e.expand(text).unwrap_err().to_string();
    assert_eq!(said("${MOD}"), "no variable is called ${MOD}");
    assert_eq!(said("${GAME}"), "${GAME} has no value here");
    assert_eq!(said("a ${MAP"), "a ${ has no } in \"a ${MAP\"");
    let io = TaskError::Io {
        path: PathBuf::from("/x/y.bsp"),
        error: std::io::Error::from(std::io::ErrorKind::NotFound),
    };
    assert!(io.to_string().starts_with("/x/y.bsp: "), "{io}");
    assert!(std::error::Error::source(&io).is_some());
    let expand = TaskError::Expand(e.expand("${MOD}").unwrap_err());
    assert_eq!(expand.to_string(), "no variable is called ${MOD}");
    assert!(std::error::Error::source(&expand).is_some());
}

// No spec statement: P6-20: a profiles file that is there but cannot be read is an error, not
// taken as no file
#[test]
fn an_unreadable_profiles_file_is_an_error() {
    let (mut session, map) = project("profiles-unreadable", None);
    std::fs::create_dir_all(map.parent().unwrap().join(PROFILES_FILE)).unwrap();
    let error = session.execute(MAP_PROFILES, json!({})).unwrap_err();
    assert!(error.to_string().contains("cannot read"), "{error}");
}

// No spec statement: P6-20: a file an earlier compile left that this one does not write is
// removed, a .lit with no coloured light among them; one that cannot be removed stops the task
#[test]
fn files_of_an_earlier_compile_are_removed() {
    let file = "[[profile]]\nname = \"Lit\"\ntasks = [{ task = \"bsp\" }, \
                { task = \"light\", speed = \"fast\" }]\n";
    let (mut session, map) = project("profile-stale", Some(file));
    let stale = map.with_extension("lit");
    std::fs::write(&stale, b"old").unwrap();
    let done = run(&mut session, "Lit");
    assert!(done.succeeded(), "{done:?}");
    assert!(!stale.exists(), "a grey light leaves no .lit");
    // A folder where the .lit would be cannot be removed: the light task stops.
    std::fs::create_dir_all(&stale).unwrap();
    let stopped = run(&mut session, "Lit");
    assert_eq!(stopped.tasks.len(), 2, "{stopped:?}");
    assert!(matches!(stopped.tasks[1].result, TaskResult::Failed { .. }));
}

// No spec statement: P7-08: a name pattern of TrenchBroom's Delete Files takes `*` for any run
// of characters, none included, and `?` for any one; anything else must match exactly
#[test]
fn a_name_pattern_matches_with_stars_and_marks() {
    let matches = |pattern: &str, name: &str| super::NamePattern(pattern.to_owned()).matches(name);
    for (pattern, name) in [
        ("*.prt", "e1m1.prt"),
        ("*", ""),
        ("e?m1.*", "e1m1.bsp"),
        ("*m*", "m"),
        ("a*b*c", "aXXbYc"),
        ("exact.log", "exact.log"),
    ] {
        assert!(matches(pattern, name), "{pattern} {name}");
    }
    for (pattern, name) in [
        ("*.prt", "e1m1.bsp"),
        ("e?m1", "e11m1"),
        ("?", ""),
        ("a*b", "ab c"),
        ("exact.log", "exact.logs"),
    ] {
        assert!(!matches(pattern, name), "{pattern} {name}");
    }
}

// No spec statement: P7-08: a profile renames a file, its folder made and a relative path taken
// from the map's folder, and deletes the files a pattern names, none matching being no error;
// a rename of a file that is not there fails its task
#[test]
fn a_profile_renames_and_deletes_files() {
    let file = "[[profile]]\nname = \"Tidy\"\ntasks = [\
         { task = \"rename\", from = \"a.log\", to = \"logs/b.log\" }, \
         { task = \"delete\", target = \"${MAP_DIR}/*.tmp\" }, \
         { task = \"delete\", target = \"none-*.here\" }]\n\n\
         [[profile]]\nname = \"Missing\"\ntasks = [\
         { task = \"rename\", from = \"gone.log\", to = \"b.log\" }]\n";
    let (mut session, map) = project("profile-tidy", Some(file));
    let dir = map.parent().unwrap();
    std::fs::write(dir.join("a.log"), b"a").unwrap();
    for name in ["one.tmp", "two.tmp", "keep.txt"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let ran = run(&mut session, "Tidy");
    assert!(ran.succeeded(), "{ran:?}");
    assert!(!dir.join("a.log").exists());
    assert_eq!(std::fs::read(dir.join("logs/b.log")).unwrap(), b"a");
    assert!(!dir.join("one.tmp").exists() && !dir.join("two.tmp").exists());
    assert!(dir.join("keep.txt").exists());
    let missing = run(&mut session, "Missing");
    assert!(!missing.succeeded(), "{missing:?}");
    assert!(
        format!("{missing:?}").contains("cannot rename"),
        "said as a rename: {missing:?}"
    );
    assert_eq!(
        Task::Delete {
            target: String::new()
        }
        .name(),
        "delete"
    );
    assert_eq!(
        Task::Rename {
            from: String::new(),
            to: String::new()
        }
        .name(),
        "rename"
    );
}
