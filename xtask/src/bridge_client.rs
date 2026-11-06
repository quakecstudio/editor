// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask bridge-client <game root>`: `qs-engine`'s client against the real preview host
//! (P3-06). It builds the host, makes a run root over the game root's `id1` as ADR-0008 does,
//! launches the host in it with `--headless` on `id1` alone, and sends every command the host offers,
//! checking each answer, its offscreen `screenshot` included (P3-05), then that a method it
//! does not know is refused (BRIDGE-6.2) and that `shutdown` stops it (BRIDGE-2.5).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qs_engine::{Client, Flavour, InspectWhat, LaunchSpec, StepKind, launch, prepare_run_root};
use serde_json::{Value, json};

/// How long an event may take to come.
const WITHIN: Duration = Duration::from_secs(20);
/// The mod the run root links, an empty build output.
const MOD: &str = "bridgetest";

/// `cargo xtask bridge-client <game root> [--map <name>]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// A game root holding `id1`, such as LibreQuake's.
    basedir: PathBuf,
    /// The map to start on.
    #[arg(long, default_value = "start")]
    map: String,
}

/// Builds the host, runs the suite and reports each step.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--locked", "-p", "preview-host"])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p preview-host: {status}"));
    }
    let host = root
        .join("target/debug")
        .join(format!("preview-host{}", std::env::consts::EXE_SUFFIX));
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
    let spec = LaunchSpec {
        host,
        run_root,
        map: Some(args.map.clone()),
        socket: socket_path(),
        extra: vec!["--headless".to_owned()],
    };
    let mut running = launch(&spec, "cargo xtask bridge-client").map_err(|e| e.to_string())?;
    let outcome = suite(&mut running.client, &args.map);
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

/// BRIDGE-2.6: the commands a headless host offers, and those it leaves out.
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

/// Every step, each named when it passed; the first that fails stops the suite.
fn suite(client: &mut Client, map: &str) -> Result<Vec<String>, String> {
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
        matches!(&unknown, Err(qs_engine::ClientError::Rpc { code, .. }) if code.code() == -32601),
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

    // BRIDGE-4.5: a breakpoint at a line no statement is on is refused.
    let missing = client.request("breakpoint/set", json!({ "file": "nosuch.qc", "line": 1 }));
    expect(
        matches!(&missing, Err(qs_engine::ClientError::Rpc { code, .. }) if code.code() == -32007),
        "a breakpoint at no statement is refused",
    )?;
    steps.push("breakpoint/set".to_owned());

    // BRIDGE-4.9, WORLD-10: the running progs loaded again over the map migrates its values
    // and loses nothing.
    let reloaded = client
        .request("reload", json!({}))
        .map_err(|e| format!("reload: {e}"))?;
    expect(
        reloaded
            .get("migrated")
            .and_then(Value::as_u64)
            .is_some_and(|n| n > 0)
            && reloaded.get("lost") == Some(&json!([])),
        "reload migrates the running progs and loses nothing",
    )?;
    steps.push("reload".to_owned());

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

    // BRIDGE-4.4: paused, a field set waits for the resume.
    let health = json!({ "type": "float", "value": 50.0 });
    let held = client
        .set_field(1, "health", health.clone())
        .map_err(|e| format!("field/set: {e}"))?;
    expect(
        held == json!({ "applied": "on_resume" }),
        "paused, field/set waits",
    )?;
    let player = client
        .inspect(InspectWhat::Edict(1))
        .map_err(|e| e.to_string())?;
    expect(
        field(&player, "health") != Some(health.clone()),
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
    let player = client
        .inspect(InspectWhat::Edict(1))
        .map_err(|e| e.to_string())?;
    expect(
        field(&player, "health") == Some(health),
        "resumed, the field is set",
    )?;
    steps.push("field/set while paused, then resume".to_owned());

    let spawned = client
        .spawn("info_notnull", &[("origin", "0 0 0")])
        .map_err(|e| format!("entity/spawn: {e}"))?;
    let edict = spawned
        .get("edict")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("entity/spawn gives an edict")?;
    let edit = client
        .edit_entity(edict, &[("origin", "8 16 24")])
        .map_err(|e| format!("entity/edit: {e}"))?;
    expect(
        edit == json!({ "applied": "now" }),
        "running, entity/edit applies now",
    )?;
    let edited = client
        .inspect(InspectWhat::Edict(edict))
        .map_err(|e| e.to_string())?;
    expect(
        field(&edited, "origin") == Some(json!({ "type": "vector", "value": [8.0, 16.0, 24.0] })),
        "entity/edit moves the entity",
    )?;
    client
        .remove(edict)
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

    client
        .spawn_player_at([0.0, 0.0, 64.0], [0.0, 90.0, 0.0])
        .map_err(|e| format!("player/spawn_at: {e}"))?;
    let hit = client.pick(0.5, 0.5).map_err(|e| format!("pick: {e}"))?;
    expect(hit.get("point").is_some(), "pick says where the ray stops")?;
    steps.push("player/spawn_at and pick".to_owned());

    client.shutdown().map_err(|e| format!("shutdown: {e}"))?;
    steps.push("shutdown".to_owned());
    Ok(steps)
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
