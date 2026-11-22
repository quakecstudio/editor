// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask bridge-client <game root>`: `qs-engine`'s client against the real preview host
//! (P3-06). It builds the host, makes a run root over the game root's `id1` as ADR-0008 does,
//! launches the host in it with `--headless` on `id1` alone, and sends every command the host offers,
//! checking each answer, its offscreen `screenshot` included (P3-05), then that a method it
//! does not know is refused (BRIDGE-6.2) and that `shutdown` stops it (BRIDGE-2.5).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qs_engine::{
    Client, EntityTarget, Flavour, InspectWhat, LaunchSpec, StepKind, prepare_run_root,
};
use qs_map_edit::{EntityIndex, LiveChange};
use qs_vfs::{LoadedFile, StdFs, Vfs};
use qs_workbench::SendLive;
use qs_workbench::full_build::brush_models;
use qs_workbench::ids::Edict;
use serde_json::{Value, json};

/// How long an event may take to come.
const WITHIN: Duration = Duration::from_secs(20);
/// The mod the run root links, an empty build output.
const MOD: &str = "bridgetest";

/// `cargo xtask bridge-client <game root> [--map <name>] [--progs <progs.dat>]
/// [--exported <progs.dat>]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// A game root holding `id1`, such as LibreQuake's.
    basedir: PathBuf,
    /// The map to start on.
    #[arg(long, default_value = "start")]
    map: String,
    /// A progs, with its `.lno` beside it, to reload over the map and debug, such as the
    /// rerelease's QuakeC built by `quakec-ide` (P3-17): a breakpoint in its `PlayerPreThink`
    /// is set, hit, inspected and cleared.
    #[arg(long)]
    progs: Option<PathBuf>,
    /// The `progs.dat` of a mod exported by `project.export` and unzipped into a game folder
    /// (P4-28): reloaded over the map, it must keep the player in the game while frames run.
    #[arg(long)]
    exported: Option<PathBuf>,
}

/// Builds the host, runs the suite and reports each step.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--locked", "-p", "quakec-engine"])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p quakec-engine: {status}"));
    }
    let host = root
        .join("target/debug")
        .join(format!("quakec-engine{}", std::env::consts::EXE_SUFFIX));
    let install = qs_install::examine_folder(&args.basedir)
        .ok_or_else(|| format!("{}: no game here", args.basedir.display()))?;
    let base = install
        .original
        .ok_or_else(|| format!("{}: no original game here", args.basedir.display()))?;
    let project = root.join("target/bridge-client");
    let mut run_root =
        prepare_run_root(&project, Flavour::Original, &base, MOD).map_err(|e| e.to_string())?;
    // LibreQuake's `id1` is not the registered game, and id's engine refuses a mod on it
    // (FS-4.5), so the host plays the run root's `id1` alone.
    run_root.game = None;
    // The map's BSP as the host reads it, whose brush entities a swap names (P7-02).
    let words = [
        "-basedir".to_owned(),
        run_root.path.to_string_lossy().into_owned(),
    ];
    let vfs = Vfs::new(StdFs, &words, ".").map_err(|e| format!("{e:?}"))?;
    let name = format!("maps/{}.bsp", args.map);
    let map_bsp = vfs
        .load(name.as_bytes())
        .map_err(|e| format!("{name}: {e:?}"))?
        .map(LoadedFile::into_bytes)
        .ok_or_else(|| format!("{name} is not on the game's search path"))?;
    let models = brush_models(&map_bsp).map_err(|e| format!("{name}: {e}"))?;
    let spec = LaunchSpec {
        host,
        run_root,
        map: Some(args.map.clone()),
        socket: socket_path(),
        extra: vec![LaunchSpec::HEADLESS.to_owned()],
    };
    let mut running = spec
        .launch("cargo xtask bridge-client")
        .map_err(|e| e.to_string())?;
    let outcome = suite(
        &mut running.client,
        (&args.map, &models),
        args.progs.as_deref(),
        args.exported.as_deref(),
    );
    if outcome.is_err() {
        // The host is of no use once a step failed.
        running.child.kill().ok();
    }
    let passed = report(outcome);
    let status = running.child.wait().map_err(|e| e.to_string())?;
    if passed && !status.success() {
        println!("FAIL  the host stopped with {status}");
        return Ok(false);
    }
    Ok(passed)
}

fn report(outcome: Result<Vec<String>, String>) -> bool {
    match outcome {
        Ok(steps) => {
            for step in &steps {
                println!("ok    {step}");
            }
            println!("bridge-client: {} steps passed", steps.len());
            true
        }
        Err(error) => {
            println!("FAIL  {error}");
            false
        }
    }
}

/// The socket or named pipe the host listens at, unique to this run.
fn socket_path() -> PathBuf {
    let name = format!("qs-bridge-client-{}", std::process::id());
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

/// BRIDGE-2.10: the commands a headless host offers, and those it leaves out.
fn capabilities(client: &Client) -> Result<(), String> {
    for command in [
        "pause",
        "resume",
        "step",
        "field/set",
        "entity/spawn",
        "pick",
        "breakpoint/set",
        "breakpoint/clear",
        "evaluate",
        "console",
        "reload",
        "screenshot",
    ] {
        expect(
            client.offers(command),
            &format!("the host offers {command}"),
        )?;
    }
    // BRIDGE-4.10: with no window there is nothing to place.
    expect(
        !client.offers("window/place"),
        "a headless host leaves out window/place",
    )?;
    Ok(())
}

/// The base64 of a PNG's first bytes, `\x89PNG\r\n\x1a\n`.
const PNG_BASE64: &str = "iVBORw0KGgo";

/// P3-05, BRIDGE-4.9: a headless host draws its view offscreen at the window's first size, and
/// at the size asked; paused, the same frame twice is the same image.
fn screenshots(client: &mut Client) -> Result<(), String> {
    let shot = |client: &mut Client, size| {
        client
            .screenshot(size)
            .map_err(|e| format!("screenshot: {e}"))
    };
    let full = shot(client, None)?;
    expect(
        full.get("width") == Some(&json!(1280)) && full.get("height") == Some(&json!(800)),
        "a screenshot is 1280 by 800",
    )?;
    expect(
        text(&full, "png").starts_with(PNG_BASE64),
        "a screenshot is a PNG",
    )?;
    let small = shot(client, Some((320, 200)))?;
    expect(
        small.get("width") == Some(&json!(320)) && small.get("height") == Some(&json!(200)),
        "a screenshot is the size asked",
    )?;
    client.pause().map_err(|e| format!("pause: {e}"))?;
    let first = shot(client, None)?;
    let second = shot(client, None)?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    expect(
        text(&first, "png") == text(&second, "png"),
        "paused, the same frame draws the same image",
    )?;
    Ok(())
}

/// P4-03: the agent's engine tools, the functions `engine.inspect`, `engine.console` and
/// `engine.screenshot` run, against the real host.
fn engine_tools(client: &mut Client) -> Result<(), String> {
    use qs_workbench::engine::{
        CONSOLE_QUIET, EngineInspectParams, Inspected, ShotSize, console_output, inspect,
        screenshot_to,
    };
    let entities = inspect(
        client,
        &EngineInspectParams {
            what: Inspected::Entities,
            edict: None,
        },
    )
    .map_err(|e| format!("engine.inspect entities: {e}"))?;
    let listed = entities
        .get("entities")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    expect(listed > 0, "engine.inspect lists the map's edicts")?;
    let world = inspect(
        client,
        &EngineInspectParams {
            what: Inspected::Edict,
            edict: Some(Edict::new(0)),
        },
    )
    .map_err(|e| format!("engine.inspect edict 0: {e}"))?;
    expect(
        text(&world, "classname") == "worldspawn",
        "engine.inspect reads edict 0 as worldspawn",
    )?;
    let output = console_output(client, "nosuchtool", WITHIN, CONSOLE_QUIET)
        .map_err(|e| format!("engine.console: {e}"))?;
    expect(
        output.lines.iter().any(|line| line.contains("nosuchtool")),
        "engine.console answers the line the command printed",
    )?;
    let path = std::env::temp_dir().join(format!("qs-bridge-client-{}.png", std::process::id()));
    let size = ShotSize {
        width: 320,
        height: 200,
    };
    let shot =
        screenshot_to(client, &path, Some(size)).map_err(|e| format!("engine.screenshot: {e}"))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::remove_file(&path).ok();
    expect(
        (shot.width, shot.height) == (320, 200) && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "engine.screenshot writes a PNG of the size asked",
    )
}

/// Every step, each named when it passed; the first that fails stops the suite.
fn suite(
    client: &mut Client,
    (map, models): (&str, &[(usize, String)]),
    progs: Option<&Path>,
    exported: Option<&Path>,
) -> Result<Vec<String>, String> {
    let mut steps = Vec::new();
    let loaded = client
        .wait_event("map/loaded", WITHIN)
        .map_err(|e| format!("map/loaded: {e}"))?;
    expect(
        loaded.params.get("map") == Some(&json!(map)),
        "map/loaded names the map",
    )?;
    steps.push(format!("map/loaded {map}"));

    capabilities(client)?;
    steps.push("capabilities".to_owned());

    // BRIDGE-6.2: a method the host does not know is answered with -32601 (method not found).
    let unknown = client.request("entity/teleport", json!({}));
    expect(
        matches!(&unknown, Err(qs_engine::ClientError::Rpc { code, .. }) if *code == qs_engine::ErrorCode::MethodNotFound),
        "an unknown method is answered as not found",
    )?;
    steps.push("an unknown method is refused".to_owned());

    screenshots(client)?;
    steps.push("screenshot, offscreen".to_owned());

    // `wait_event` looks at the events already received first: those are dropped so the
    // event waited for is the one this command causes.
    client.take_events();
    client
        .console("nosuchcommand")
        .map_err(|e| format!("console: {e}"))?;
    let line = client
        .wait_event("console/line", WITHIN)
        .map_err(|e| format!("console/line: {e}"))?;
    expect(
        text(&line.params, "text").contains("nosuchcommand"),
        "console/line names the command",
    )?;
    steps.push("console".to_owned());

    engine_tools(client)?;
    steps.push("engine.inspect, engine.console and engine.screenshot".to_owned());

    console_moves_the_player(client)?;
    steps.push("+forward at the console moves the player".to_owned());

    // BRIDGE-4.5: a breakpoint at a line no statement is on is refused.
    let missing = client.request("breakpoint/set", json!({ "file": "nosuch.qc", "line": 1 }));
    expect(
        matches!(&missing, Err(qs_engine::ClientError::Rpc { code, .. }) if *code == qs_engine::ErrorCode::NoSuchBreakpoint),
        "a breakpoint at no statement is refused",
    )?;
    steps.push("breakpoint/set".to_owned());

    // BRIDGE-4.11, P3-10: a dry run says what the reload then reports. The game is paused
    // across both, since a frame between them can spawn or free an entity and change what
    // migrates.
    client.pause().map_err(|e| format!("pause: {e}"))?;
    let predicted = client
        .request("reload", json!({ "dry_run": true }))
        .map_err(|e| format!("reload dry_run: {e}"))?;
    // BRIDGE-4.9, WORLD-10: the running progs loaded again over the map migrates its values
    // and loses nothing.
    let reloaded = client
        .request("reload", json!({}))
        .map_err(|e| format!("reload: {e}"))?;
    expect(
        predicted.get("lost") == reloaded.get("lost")
            && predicted.get("added") == reloaded.get("added")
            && predicted.get("migrated") == reloaded.get("migrated"),
        "the dry run predicts the reload's report",
    )?;
    expect(
        reloaded
            .get("migrated")
            .and_then(Value::as_u64)
            .is_some_and(|n| n > 0)
            && reloaded.get("lost") == Some(&json!([])),
        "reload migrates the running progs and loses nothing",
    )?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    steps.push("reload, predicted by its dry run".to_owned());

    let monster = a_monster(client)?;

    client.pause().map_err(|e| format!("pause: {e}"))?;
    let before = time(
        &client
            .inspect(InspectWhat::Globals)
            .map_err(|e| e.to_string())?,
    )?;
    client
        .step(StepKind::Frame)
        .map_err(|e| format!("step: {e}"))?;
    let after = time(
        &client
            .inspect(InspectWhat::Globals)
            .map_err(|e| e.to_string())?,
    )?;
    expect(after > before, "a step runs one frame")?;
    let stack = client
        .inspect(InspectWhat::Stack)
        .map_err(|e| e.to_string())?;
    expect(
        stack.get("stack") == Some(&json!([])),
        "paused, the stack is empty",
    )?;
    steps.push("pause, step and inspect".to_owned());

    paused_health(client, monster)?;
    steps.push("a monster's health set while paused, then resume".to_owned());

    let spawned = client
        .spawn("info_notnull", &[("origin", "0 0 0")], None)
        .map_err(|e| format!("entity/spawn: {e}"))?;
    let edict = spawned
        .get("edict")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("entity/spawn gives an edict")?;
    let edit = client
        .edit_entity(
            EntityTarget::Edict(Edict::new(edict).into()),
            &[("origin", "8 16 24")],
        )
        .map_err(|e| format!("entity/edit: {e}"))?;
    expect(
        edit == json!({ "applied": "now" }),
        "running, entity/edit applies now",
    )?;
    let edited = client
        .inspect(InspectWhat::Edict(Edict::new(edict).into()))
        .map_err(|e| e.to_string())?;
    expect(
        field(&edited, "origin") == Some(json!({ "type": "vector", "value": [8.0, 16.0, 24.0] })),
        "entity/edit moves the entity",
    )?;
    client
        .remove(EntityTarget::Edict(Edict::new(edict).into()))
        .map_err(|e| format!("entity/remove: {e}"))?;
    let entities = client
        .inspect(InspectWhat::Entities)
        .map_err(|e| e.to_string())?;
    let listed = entities
        .get("entities")
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|e| e.get("edict") == Some(&json!(edict))));
    expect(!listed, "entity/remove frees the edict")?;
    steps.push("entity/spawn, entity/edit and entity/remove".to_owned());

    map_entities(client, monster)?;
    steps.push("a monster placed, moved and removed as the map editor does".to_owned());

    player_is_put_where_it_fits(client)?;
    let hit = client.pick(0.5, 0.5).map_err(|e| format!("pick: {e}"))?;
    expect(hit.get("point").is_some(), "pick says where the ray stops")?;
    steps.push("player/spawn_at and pick".to_owned());

    swap_keeps_the_game(client, map, models)?;
    steps.push("map/swap of the map's own BSP keeps the player and every entity".to_owned());

    if let Some(progs) = progs {
        other_progs(client, progs, &mut steps)?;
    }
    if let Some(progs) = exported {
        exported_progs(client, progs)?;
        steps.push(format!("the exported {} plays", progs.display()));
    }

    client.shutdown().map_err(|e| format!("shutdown: {e}"))?;
    steps.push("shutdown".to_owned());
    Ok(steps)
}

/// `map/swap` of the running map's own BSP, each brush entity by its place in the entity lump
/// (P7-02, BRIDGE-4.14, BRIDGE-4.15, BRIDGE-4.18): the player stands where they stood and every entity
/// keeps its place and model.
fn swap_keeps_the_game(
    client: &mut Client,
    map: &str,
    models: &[(usize, String)],
) -> Result<(), String> {
    let entities = |client: &mut Client| -> Result<Value, String> {
        client
            .inspect(InspectWhat::Entities)
            .map_err(|e| format!("engine.inspect entities: {e}"))
            .map(|found| found.get("entities").cloned().unwrap_or(Value::Null))
    };
    // Paused, so the monsters and the player stand still between the two looks.
    client.pause().map_err(|e| format!("pause: {e}"))?;
    let before = entities(client)?;
    let swapped = client
        .swap_map(&format!("maps/{map}.bsp"), models)
        .map_err(|e| format!("map/swap: {e}"))?;
    // Each listed entity in an edict takes its model; one the map inhibits is in none.
    let given = swapped
        .get("swapped")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    expect(
        given.is_some_and(|n| n > 0 && n <= models.len()),
        "map/swap gives the brush entities their models",
    )?;
    expect(
        entities(client)? == before,
        "map/swap keeps the player and every entity where they were",
    )?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    Ok(())
}

/// The map's lowest-numbered live monster, whose health is edited while paused (P3-14). A
/// monster is not spawned for it: its spawn function precaches its model and sounds, which
/// the engine allows only while the map loads (`PF_Precache_*`).
fn a_monster(client: &mut Client) -> Result<u32, String> {
    let entities = client
        .inspect(InspectWhat::Entities)
        .map_err(|e| e.to_string())?;
    entities
        .get("entities")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .filter(|e| text(e, "classname").starts_with("monster_"))
                .filter_map(|e| e.get("edict").and_then(Value::as_u64))
                .min()
        })
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| "the map has a live monster".to_owned())
}

/// BRIDGE-4.13, MAP-12, P5-07: a monster of the map's own class, placed after the world as the
/// map editor places one, spawns in the running game, its move reaches it, and its removal
/// frees it; the world is map entity 0. The changes go through the map editor's own
/// [`LiveChange::send`].
fn map_entities(client: &mut Client, monster: u32) -> Result<(), String> {
    let classname = client
        .inspect(InspectWhat::Edict(Edict::new(monster).into()))
        .map_err(|e| e.to_string())?
        .get("classname")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let place = EntityIndex::new(1);
    let spawned = LiveChange::Spawn {
        place,
        classname: classname.clone(),
        pairs: vec![("origin".to_owned(), "0 0 64".to_owned())],
    }
    .send(client)
    .map_err(|e| format!("placing a {classname}: {e}"))?;
    let edict = spawned
        .get("edict")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("entity/spawn gives an edict")?;
    let placed = client
        .inspect(InspectWhat::Edict(Edict::new(edict).into()))
        .map_err(|e| e.to_string())?;
    expect(
        text(&placed, "classname") == classname,
        "the placed monster is in the game",
    )?;
    LiveChange::Edit {
        place,
        pairs: vec![("origin".to_owned(), "4 8 64".to_owned())],
    }
    .send(client)
    .map_err(|e| format!("moving the placed {classname}: {e}"))?;
    let moved = client
        .inspect(InspectWhat::Edict(Edict::new(edict).into()))
        .map_err(|e| e.to_string())?;
    expect(
        field(&moved, "origin") == Some(json!({ "type": "vector", "value": [4.0, 8.0, 64.0] })),
        "the move reaches the placed monster",
    )?;
    LiveChange::Edit {
        place: EntityIndex::new(0),
        pairs: Vec::new(),
    }
    .send(client)
    .map_err(|e| format!("editing the world: {e}"))?;
    LiveChange::Remove { place }
        .send(client)
        .map_err(|e| format!("removing the placed {classname}: {e}"))?;
    let gone = client.inspect(InspectWhat::Edict(Edict::new(edict).into()));
    expect(gone.is_err(), "the removed monster is freed")?;
    Ok(())
}

/// BRIDGE-4.4, P3-14: paused, a monster's health set waits for the resume, then is the
/// monster's in game; running, one set is the monster's at once.
fn paused_health(client: &mut Client, monster: u32) -> Result<(), String> {
    let health = json!({ "type": "float", "value": 50.0 });
    let held = client
        .set_field(Edict::new(monster).into(), "health", &health)
        .map_err(|e| format!("field/set: {e}"))?;
    expect(
        held == json!({ "applied": "on_resume" }),
        "paused, field/set waits",
    )?;
    let waiting = client
        .inspect(InspectWhat::Edict(Edict::new(monster).into()))
        .map_err(|e| e.to_string())?;
    expect(
        field(&waiting, "health") != Some(health.clone()),
        "the field waits",
    )?;
    client.take_events();
    client.resume().map_err(|e| format!("resume: {e}"))?;
    let running = client
        .wait_event("state/changed", WITHIN)
        .map_err(|e| format!("state/changed: {e}"))?;
    expect(
        text(&running.params, "state") == "running",
        "resume says the game runs",
    )?;
    let edited = client
        .inspect(InspectWhat::Edict(Edict::new(monster).into()))
        .map_err(|e| e.to_string())?;
    expect(
        field(&edited, "health") == Some(health),
        "resumed, the monster's health is set",
    )?;
    let health = json!({ "type": "float", "value": 60.0 });
    let set = client
        .set_field(Edict::new(monster).into(), "health", &health)
        .map_err(|e| format!("field/set: {e}"))?;
    expect(
        set == json!({ "applied": "now" }),
        "running, field/set applies",
    )?;
    let edited = client
        .inspect(InspectWhat::Edict(Edict::new(monster).into()))
        .map_err(|e| e.to_string())?;
    expect(
        field(&edited, "health") == Some(health),
        "running, the monster's health is set at once",
    )?;
    Ok(())
}

/// How many frames the player walks for.
const WALK_FRAMES: u8 = 20;
/// The least the player walks in them, in map units, on the floor: a fraction of what
/// `cl_forwardspeed` covers, since the walk starts from rest.
const WALKED: f64 = 16.0;

/// The player's origin, as `inspect` reads it from its edict.
fn player_origin(client: &mut Client) -> Result<[f64; 3], String> {
    let entities = client
        .inspect(InspectWhat::Entities)
        .map_err(|e| e.to_string())?;
    let player = entities
        .get("entities")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|e| text(e, "classname") == "player"))
        .and_then(|e| e.get("edict").and_then(Value::as_u64))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("the player is in the game")?;
    let edict = client
        .inspect(InspectWhat::Edict(Edict::new(player).into()))
        .map_err(|e| e.to_string())?;
    field(&edict, "origin")
        .and_then(|origin| serde_json::from_value(origin.get("value")?.clone()).ok())
        .ok_or_else(|| "the player has an origin".to_owned())
}

/// A point outside every map: nothing but solid is there, or around it.
const NOWHERE: [f32; 3] = [30000.0, 30000.0, 30000.0];

/// BRIDGE-4.19: with `room` `find` the player is put where its box is free and the answer
/// says where: where it stands now, there; where there is no room, nowhere, and it stays.
/// The game is paused meanwhile, so nothing else moves the player.
fn player_is_put_where_it_fits(client: &mut Client) -> Result<(), String> {
    client.pause().map_err(|e| format!("pause: {e}"))?;
    let stands = player_origin(client)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the game holds positions in single precision"
    )]
    let at = stands.map(|c| c as f32);
    let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01);
    let placed = client
        .place_player_near(at, [0.0, 90.0, 0.0])
        .map_err(|e| format!("player/spawn_at: {e}"))?;
    expect(
        near(placed.map(f64::from), stands),
        "player/spawn_at answers where it put the player",
    )?;
    let refused = client.place_player_near(NOWHERE, [0.0; 3]);
    expect(
        matches!(&refused, Err(qs_engine::ClientError::Rpc { code, .. }) if *code == qs_engine::ErrorCode::InvalidParams),
        "player/spawn_at with no room is refused",
    )?;
    expect(
        near(player_origin(client)?, stands),
        "refused, the player stays",
    )?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    Ok(())
}

/// INPUT-2.1, CON-6.1: with no window the console's buttons are the player's: `+forward` typed
/// there moves the player, and `-forward` lets the button go.
fn console_moves_the_player(client: &mut Client) -> Result<(), String> {
    let walk = |client: &mut Client| -> Result<f64, String> {
        let before = player_origin(client)?;
        for _ in 0..WALK_FRAMES {
            client
                .step(StepKind::Frame)
                .map_err(|e| format!("step: {e}"))?;
        }
        let after = player_origin(client)?;
        let (x, y) = (after[0] - before[0], after[1] - before[1]);
        Ok(x.hypot(y))
    };
    client.pause().map_err(|e| format!("pause: {e}"))?;
    // With no button the player stands: what moves it below is the button.
    let standing = walk(client)?;
    client
        .console("+forward")
        .map_err(|e| format!("console: {e}"))?;
    let walked = walk(client)?;
    client
        .console("-forward")
        .map_err(|e| format!("console: {e}"))?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    expect(standing < 1.0, "with no button the player stands")?;
    expect(
        walked > WALKED,
        "+forward at the console moves the player across the floor",
    )
}

/// P4-28: the exported mod's progs reloaded over the map plays: the reload loses nothing of
/// the player, frames advance the game's time, and the player is still in the game after them.
fn exported_progs(client: &mut Client, progs: &Path) -> Result<(), String> {
    let path = progs.to_str().ok_or("--exported is not Unicode")?;
    client
        .request("reload", json!({ "progs": path }))
        .map_err(|e| format!("reload {path}: {e}"))?;
    client.pause().map_err(|e| format!("pause: {e}"))?;
    let globals = |client: &mut Client| {
        client
            .inspect(InspectWhat::Globals)
            .map_err(|e| e.to_string())
            .and_then(|answer| time(&answer))
    };
    let before = globals(client)?;
    for _ in 0..10 {
        client
            .step(StepKind::Frame)
            .map_err(|e| format!("step: {e}"))?;
    }
    let after = globals(client)?;
    expect(after > before, "the exported progs runs frames")?;
    let entities = client
        .inspect(InspectWhat::Entities)
        .map_err(|e| e.to_string())?;
    let players = entities
        .get("entities")
        .and_then(Value::as_array)
        .map_or(0, |list| {
            list.iter()
                .filter(|e| text(e, "classname") == "player")
                .count()
        });
    expect(
        players > 0,
        "the player is in the game the exported progs runs",
    )?;
    client.resume().map_err(|e| format!("resume: {e}"))?;
    Ok(())
}

/// The progs at `progs` reloaded over the map and debugged (P3-17, BRIDGE-4.8): its
/// `PlayerPreThink`, which runs each frame for the player, stops at a breakpoint set by file
/// and line, the stack is inspected there, and the breakpoint is cleared.
fn other_progs(client: &mut Client, progs: &Path, steps: &mut Vec<String>) -> Result<(), String> {
    let path = progs.to_str().ok_or("--progs is not Unicode")?;
    let reloaded = client
        .request("reload", json!({ "progs": path }))
        .map_err(|e| format!("reload {path}: {e}"))?;
    expect(
        reloaded.get("lost").is_some(),
        "reload of another progs reports what it migrated and lost",
    )?;
    steps.push(format!("reload {}", progs.display()));

    // The first line after `PlayerPreThink`'s header with a statement on it (BRIDGE-4.5).
    let source = progs
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|project| project.join("client.qc"))
        .ok_or("--progs is not in a project's build directory")?;
    let code =
        std::fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let header = code
        .lines()
        .position(|line| line.contains("PlayerPreThink") && line.contains("void"))
        .ok_or("client.qc has no PlayerPreThink")?;
    let mut set = None;
    for offset in 1..40 {
        let line = u32::try_from(header.saturating_add(1).saturating_add(offset)).unwrap_or(0);
        if let Ok(answer) = client.set_breakpoint("client.qc", line) {
            set = Some(answer);
            break;
        }
    }
    let set = set.ok_or("no line of PlayerPreThink takes a breakpoint")?;
    let id = set
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("breakpoint/set gives an id")?;
    client.take_events();
    let hit = client
        .wait_event("breakpoint/hit", WITHIN)
        .map_err(|e| format!("breakpoint/hit: {e}"))?;
    let location = hit.params.get("location").cloned().unwrap_or_default();
    expect(
        text_of(&location, "function") == "PlayerPreThink"
            && text_of(&location, "file") == "client.qc",
        "the breakpoint stops in PlayerPreThink in client.qc",
    )?;
    let stack = client
        .inspect(InspectWhat::Stack)
        .map_err(|e| e.to_string())?;
    let frames = stack
        .get("stack")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    expect(frames > 0, "stopped, the stack has PlayerPreThink's frame")?;
    // BRIDGE-4.6, BRIDGE-7.1: stopped, an edict is read, and a command that changes the world
    // is refused with its code while the connection stays open.
    let player = client
        .inspect(InspectWhat::Edict(Edict::new(1).into()))
        .map_err(|e| format!("inspect edict 1: {e}"))?;
    expect(
        text(&player, "classname") == "player",
        "stopped, the player's edict is read",
    )?;
    let refused = client.place_player_near([0.0; 3], [0.0; 3]);
    expect(
        matches!(&refused, Err(qs_engine::ClientError::Rpc { code, .. }) if *code == qs_engine::ErrorCode::StoppedInFrame),
        "stopped, player/spawn_at is refused as stopped in frame",
    )?;
    client
        .inspect(InspectWhat::Stack)
        .map_err(|e| format!("inspect stack after the refusal: {e}"))?;
    // P3-16: the Debugger tab's watch and step, against the stopped frame.
    let watched = client
        .evaluate("self.health")
        .map_err(|e| format!("evaluate: {e}"))?;
    expect(
        watched.pointer("/value/type") == Some(&json!("float")),
        "a watch of self.health reads a float",
    )?;
    // BRIDGE-3.3: a step ends with state/changed, paused for the step, at the new location;
    // the step's own answer holds only the time.
    client.take_events();
    client
        .step(StepKind::Statement)
        .map_err(|e| format!("step: {e}"))?;
    let stepped = client
        .wait_event("state/changed", WITHIN)
        .map_err(|e| format!("state/changed after the step: {e}"))?;
    expect(
        text(&stepped.params, "reason") == "step"
            && !text_of(
                stepped.params.get("location").unwrap_or(&Value::Null),
                "function",
            )
            .is_empty(),
        "a step stops at the next statement and says where",
    )?;
    client
        .clear_breakpoint(qs_engine::BreakpointId::new(id))
        .map_err(|e| format!("breakpoint/clear: {e}"))?;
    client.take_events();
    client.resume().map_err(|e| format!("resume: {e}"))?;
    let running = client
        .wait_event("state/changed", WITHIN)
        .map_err(|e| format!("state/changed: {e}"))?;
    expect(
        text(&running.params, "state") == "running",
        "cleared and resumed, the game runs",
    )?;
    steps.push(
        "breakpoint/set, hit, inspect the stack, watch, step and clear in client.qc".to_owned(),
    );
    Ok(())
}

/// The string `key` of `value`, empty when it is not one.
fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn expect(holds: bool, what: &str) -> Result<(), String> {
    if holds {
        Ok(())
    } else {
        Err(format!("expected: {what}"))
    }
}

fn text<'a>(params: &'a Value, key: &str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn time(answer: &Value) -> Result<f64, String> {
    answer
        .get("time")
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("an answer with no time: {answer}"))
}

/// The bridge value of the field `name` in an `inspect` of an edict.
fn field(answer: &Value, name: &str) -> Option<Value> {
    answer
        .get("fields")?
        .as_array()?
        .iter()
        .find(|f| f.get("name") == Some(&json!(name)))?
        .get("value")
        .cloned()
}
