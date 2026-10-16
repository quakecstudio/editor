// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles id101 <dir>`: compiles id's QuakeC 1.01 with id's qcc and runs the VM
//! oracle on it (the P1-11 acceptance criterion "Running the oracle on id 1.01 progs produces
//! traces").
//!
//! The progs.dat must be byte-identical to the one id's qcc wrote when this check was made, on
//! every host, so a change to the build of qcc shows. Each scenario's trace must decode with
//! `qc-trace`'s strict decoder, re-encode to the same bytes, carry the file's CRC, hold at least
//! one executed statement, and be the same bytes when the scenario is run again
//! (differential.md DIFF-1.4).

use std::fs;
use std::path::Path;
use std::process::Command;

use qc_progs::Progs;
use qc_progs::crc::crc_block;
use qc_trace::{Record, decode, encode};

use super::{Oracles, sha256};

/// The SHA-256 of the progs.dat id's qcc writes for QuakeC 1.01 (corpus `id-1.01`, Quake-Tools
/// at the commit in `oracles/sources.toml`), recorded from the first build of these oracles.
const PROGS_SHA256: &str = "ede90183beed15e1fd22dc5cec384d173199bd8cfcd5b62a533458e967901123";

/// A small map's entities: the world, a player start, items, monsters (one inhibited on easy
/// skill), a door and a trigger on brush models, and an entity with no spawn function.
const ENTITIES: &str = r#"{
"classname" "worldspawn"
"message" "Oracle test"
"sounds" "1"
"worldtype" "0"
}
{
"classname" "info_player_start"
"origin" "0 0 24"
"angle" "90"
}
{
"classname" "light_torch_small_walltorch"
"origin" "64 0 64"
}
{
"classname" "item_shells"
"origin" "32 32 0"
}
{
"classname" "weapon_supershotgun"
"origin" "-32 32 0"
}
{
"classname" "monster_army"
"origin" "256 0 24"
"angle" "180"
}
{
"classname" "monster_dog"
"origin" "256 128 24"
"spawnflags" "256"
}
{
"classname" "func_door"
"model" "*1"
"angle" "-1"
"targetname" "t1"
}
{
"classname" "trigger_once"
"model" "*2"
"target" "t1"
}
{
"classname" "info_notnull"
"_comment" "an underscore key is ignored"
}
"#;

/// The scenarios run on id 1.01: the default map of only a worldspawn, where the player finds
/// no start and `PutClientInServer` stops the map with an error (so the trace ends in an error
/// record), and the small map with a player who fires.
const SCENARIOS: [(&str, &str); 2] = [
    ("worldspawn-only", "progs ../progs.dat\nclients 1\n"),
    (
        "small-map",
        "progs ../progs.dat\nentities small-map.ent\nmap e1m1\nsubmodels 2\n\
         model *1 -64 -8 0 64 8 128\nmaxclients 2\nclients 1\ncvar skill 0\nframes 200\n\
         input 5 1 impulse 2\ninput 20 1 button0 1\ninput 30 1 button0 0\n\
         input 40 1 v_angle 0 45 0\nseed 7\n",
    ),
];

pub(crate) fn check(oracles: &Oracles, codebase: &Path) -> Result<bool, String> {
    let work = oracles.work.join("id-1.01");
    let src = work.join("src");
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    fs::create_dir_all(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut copied = 0_u32;
    for entry in fs::read_dir(codebase).map_err(|e| format!("{}: {e}", codebase.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if let Some(name) = path.file_name().filter(|_| path.is_file()) {
            fs::copy(&path, src.join(name)).map_err(|e| format!("{}: {e}", path.display()))?;
            copied = copied.saturating_add(1);
        }
    }
    if !src.join("progs.src").is_file() {
        return Err(format!("no progs.src in {}", codebase.display()));
    }

    // id's qcc reads progs.src in the current directory and writes ../progs.dat.
    let output = Command::new(&oracles.qcc)
        .current_dir(&src)
        .output()
        .map_err(|e| format!("cannot run qcc: {e}"))?;
    let log = String::from_utf8_lossy(&output.stdout);
    let progs_path = work.join("progs.dat");
    if log.contains("ERROR") || !progs_path.is_file() {
        println!("{log}");
        return Err(format!(
            "qcc did not compile {} ({})",
            codebase.display(),
            output.status
        ));
    }
    let progs = fs::read(&progs_path).map_err(|e| e.to_string())?;
    let mut ok = true;
    let digest = sha256::hex_digest(&progs);
    if digest == PROGS_SHA256 {
        println!("ok    qcc compiled {copied} files into progs.dat (sha256 {digest})");
    } else {
        ok = false;
        println!("FAIL  qcc's progs.dat has sha256 {digest}, expected {PROGS_SHA256}");
    }
    Progs::read(&progs).map_err(|e| format!("qc-progs refuses qcc's progs.dat: {e:?}"))?;

    let scenarios = work.join("scenarios");
    fs::create_dir_all(&scenarios).map_err(|e| e.to_string())?;
    fs::write(scenarios.join("small-map.ent"), ENTITIES).map_err(|e| e.to_string())?;
    for (name, text) in SCENARIOS {
        ok &= run_scenario(oracles, &scenarios, name, text, &progs)?;
    }
    Ok(ok)
}

fn run_scenario(
    oracles: &Oracles,
    dir: &Path,
    name: &str,
    text: &str,
    progs: &[u8],
) -> Result<bool, String> {
    let scenario = dir.join(format!("{name}.scn"));
    fs::write(&scenario, text).map_err(|e| e.to_string())?;
    let mut traces = Vec::new();
    for run in ["a", "b"] {
        let trace = dir.join(format!("{name}.{run}.trace"));
        let output = Command::new(&oracles.vm)
            .arg("scenario")
            .arg(&scenario)
            .arg(&trace)
            .output()
            .map_err(|e| format!("cannot run the VM oracle: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "scenario {name}: the VM oracle failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        traces.push(fs::read(&trace).map_err(|e| e.to_string())?);
    }
    let [first, second] = traces.as_slice() else {
        return Err("two runs expected".to_owned());
    };
    let trace = decode(first).map_err(|e| format!("scenario {name}: bad trace: {e:?}"))?;
    let mut problems = Vec::new();
    if encode(&trace) != *first {
        problems.push("the trace does not re-encode to the same bytes (TRACE-5.7)".to_owned());
    }
    if first != second {
        problems.push("a second run wrote different bytes (DIFF-1.4)".to_owned());
    }
    if trace.header.progs_crc() != crc_block(progs) {
        problems.push("the header's progs_crc is not the file's CRC (TRACE-2.1)".to_owned());
    }
    let (mut runs, mut steps, mut strings) = (0_u32, 0_u32, 0_u32);
    let mut statements = std::collections::BTreeSet::new();
    let mut error = None;
    for record in &trace.records {
        match record {
            Record::Run { .. } => runs = runs.saturating_add(1),
            Record::Step(step) => {
                steps = steps.saturating_add(1);
                statements.insert(step.statement);
            }
            Record::String { .. } => strings = strings.saturating_add(1),
            Record::Error {
                function,
                statement,
                message,
                ..
            } => {
                error = Some(format!(
                    "{} at function {function} statement {statement}",
                    String::from_utf8_lossy(message.as_bytes())
                ));
            }
            Record::Exit { .. } => {}
        }
    }
    if steps == 0 {
        problems.push("no statement was executed".to_owned());
    }
    let ending = error.map_or_else(|| "ran to the end".to_owned(), |e| format!("stopped: {e}"));
    if problems.is_empty() {
        println!(
            "ok    scenario {name}: {} bytes, {runs} runs, {steps} statements ({} distinct), \
             {strings} engine strings; {ending}",
            first.len(),
            statements.len()
        );
        return Ok(true);
    }
    for problem in problems {
        println!("FAIL  scenario {name}: {problem}");
    }
    Ok(false)
}
