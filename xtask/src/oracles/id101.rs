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

use super::{Oracles, codegen, sha256};

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

/// Compiles the codebase in `codebase` with id's qcc in a scratch directory; returns the
/// progs.dat and the number of files copied from `codebase`.
pub(crate) fn compile(oracles: &Oracles, codebase: &Path) -> Result<(Vec<u8>, u32), String> {
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
    let output = oracles
        .exec
        .output(Command::new(&oracles.qcc).current_dir(&src))
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
    Ok((progs, copied))
}

pub(crate) fn check(oracles: &Oracles, codebase: &Path) -> Result<bool, String> {
    let (progs, copied) = compile(oracles, codebase)?;
    let mut ok = true;
    let digest = sha256::hex_digest(&progs);
    if digest == PROGS_SHA256 {
        println!("ok    qcc compiled {copied} files into progs.dat (sha256 {digest})");
    } else {
        ok = false;
        println!("FAIL  qcc's progs.dat has sha256 {digest}, expected {PROGS_SHA256}");
    }
    Progs::read(&progs).map_err(|e| format!("qc-progs refuses qcc's progs.dat: {e:?}"))?;

    let scenarios = oracles.work.join("id-1.01").join("scenarios");
    fs::create_dir_all(&scenarios).map_err(|e| e.to_string())?;
    fs::write(scenarios.join("small-map.ent"), ENTITIES).map_err(|e| e.to_string())?;
    for (name, text) in SCENARIOS {
        ok &= run_scenario(oracles, &scenarios, name, text, &progs)?;
    }
    ok &= check_ours(oracles, codebase, &progs, &scenarios)?;
    Ok(ok)
}

/// The compiler differential on id 1.01 (P2-06): `qc-codegen` compiles the codebase, its
/// progs.dat must be the bytes qcc wrote, and each scenario run on it must leave the records
/// qcc's leaves (`qc-trace`'s, the header's progs CRC aside).
fn check_ours(
    oracles: &Oracles,
    codebase: &Path,
    theirs: &[u8],
    their_scenarios: &Path,
) -> Result<bool, String> {
    let compiled = match codegen::compile_codebase(codebase) {
        Ok(compiled) => compiled,
        Err(e) => {
            println!(
                "FAIL  qc-codegen did not compile {}:\n{e}",
                codebase.display()
            );
            return Ok(false);
        }
    };
    let ours = codegen::bytes(&compiled)?;
    let mut ok = true;
    if ours == theirs {
        println!(
            "ok    qc-codegen's progs.dat is qcc's, byte for byte ({} bytes)",
            ours.len()
        );
    } else {
        ok = false;
        println!("FAIL  qc-codegen's progs.dat differs from qcc's:");
        for difference in codebase_differences(&ours, theirs)? {
            println!("      {difference}");
        }
    }
    let work = oracles.work.join("id-1.01").join("ours");
    let scenarios = work.join("scenarios");
    fs::create_dir_all(&scenarios).map_err(|e| e.to_string())?;
    fs::write(work.join("progs.dat"), &ours).map_err(|e| e.to_string())?;
    fs::write(scenarios.join("small-map.ent"), ENTITIES).map_err(|e| e.to_string())?;
    for (name, text) in SCENARIOS {
        if !run_scenario(oracles, &scenarios, name, text, &ours)? {
            ok = false;
            continue;
        }
        let records = |dir: &Path| -> Result<Vec<Record>, String> {
            let path = dir.join(format!("{name}.a.trace"));
            let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(decode(&bytes)
                .map_err(|e| format!("{}: {e:?}", path.display()))?
                .records)
        };
        if records(&scenarios)? == records(their_scenarios)? {
            println!("ok    scenario {name}: qc-codegen's progs leaves the records qcc's leaves");
        } else {
            ok = false;
            println!("FAIL  scenario {name}: qc-codegen's progs leaves other records than qcc's");
        }
    }
    Ok(ok)
}

/// [`codegen::differences`], or the reason the two do not load.
fn codebase_differences(ours: &[u8], theirs: &[u8]) -> Result<Vec<String>, String> {
    let mut found = codegen::differences(ours, theirs)?;
    if found.is_empty() {
        found.push("the bytes differ outside the tables".to_owned());
    }
    Ok(found)
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
        let output = oracles
            .exec
            .output(
                Command::new(&oracles.vm)
                    .arg("scenario")
                    .arg(&scenario)
                    .arg(&trace),
            )
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    use qc_progs::crc::crc_block;
    use qc_progs::{ProgsFile, Statement, write};
    use qc_trace::{Changes, Header, Record, Step, Text, Trace, encode};

    use super::super::testing::{fake_oracles, fresh_dir, last_arg};
    use super::{check, compile, run_scenario};
    use crate::exec::{Fake, Reply};

    /// A small progs, standing for the one id's qcc writes.
    fn progs() -> Vec<u8> {
        write(&ProgsFile {
            statements: vec![Statement::default(); 2],
            strings: vec![0],
            ..ProgsFile::default()
        })
        .unwrap()
    }

    /// A codebase directory with a `progs.src`, unless `with_src` is false.
    fn codebase(dir: &Path, with_src: bool) -> PathBuf {
        let codebase = dir.join("id-1.01");
        fs::create_dir_all(codebase.join("subdir")).unwrap();
        fs::write(codebase.join("world.qc"), "void() main = {};\n").unwrap();
        if with_src {
            fs::write(codebase.join("progs.src"), "../progs.dat\nworld.qc\n").unwrap();
        }
        codebase
    }

    /// A trace of `progs` that runs one statement, or ends in an error record.
    fn trace(progs: &[u8], error: bool) -> Vec<u8> {
        let mut records = vec![Record::Run {
            function: 2,
            changes: Changes::default(),
        }];
        if error {
            records.push(Record::Error {
                function: 2,
                statement: 1,
                message: Text::new(&b"no spot"[..]).unwrap(),
                changes: Changes::default(),
            });
        } else {
            records.push(Record::Step(Step {
                function: 2,
                statement: 1,
                opcode: 0,
                builtin: None,
                changes: Changes::default(),
            }));
            records.push(Record::String {
                value: 70000,
                text: Text::new(&b"start"[..]).unwrap(),
            });
            records.push(Record::Exit {
                changes: Changes::default(),
            });
        }
        encode(&Trace {
            header: Header::new(crc_block(progs), 548, 108, 110, 1).unwrap(),
            records,
        })
    }

    /// Fake qcc and VM oracle: qcc writes `progs` beside its source directory, and the oracle
    /// writes `trace` for every scenario.
    fn tools(progs: Vec<u8>, trace: Vec<u8>) -> impl Fn(&Command) -> Reply {
        move |command: &Command| {
            if command.get_program() == "qcc" {
                let src = command.get_current_dir().unwrap();
                return Reply {
                    stdout: "compiling world.qc".to_owned(),
                    files: vec![(src.parent().unwrap().join("progs.dat"), progs.clone())],
                    ..Reply::default()
                };
            }
            Reply {
                files: vec![(last_arg(command), trace.clone())],
                ..Reply::default()
            }
        }
    }

    #[test]
    fn id101_is_compiled_in_a_copy_and_each_scenario_traced_twice() {
        let dir = fresh_dir("id101-check");
        let codebase = codebase(&dir, true);
        let fake = Fake::new(tools(progs(), trace(&progs(), false)));
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let (bytes, copied) = compile(&oracles, &codebase).unwrap();
        assert_eq!(bytes, progs());
        assert_eq!(copied, 2, "the files, not the subdirectory");
        // This progs is not the one id's qcc writes, so the check fails on its digest and on
        // qc-codegen's progs, which is not the fake's; the scenarios still run, two runs each,
        // on qcc's progs and on ours.
        assert!(!check(&oracles, &codebase).unwrap());
        let seen = fake.seen.lock().unwrap();
        assert_eq!(
            seen.iter()
                .filter(|line| line.starts_with("vm-oracle scenario "))
                .count(),
            8
        );
        for scenarios in ["id-1.01/scenarios", "id-1.01/ours/scenarios"] {
            assert!(oracles.work.join(scenarios).join("small-map.ent").is_file());
        }
        let ours = fs::read(oracles.work.join("id-1.01/ours/progs.dat")).unwrap();
        assert!(
            qc_progs::Progs::read_with(
                &ours,
                &qc_progs::ReadOptions {
                    check_header_crc: false
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn a_codebase_qc_codegen_refuses_fails_the_check() {
        let dir = fresh_dir("id101-refused");
        let codebase = codebase(&dir, true);
        fs::write(codebase.join("world.qc"), "void() main = { x = 1; };\n").unwrap();
        let fake = Fake::new(tools(progs(), trace(&progs(), false)));
        let oracles = fake_oracles(&dir.join("work"), &fake);
        assert!(!check(&oracles, &codebase).unwrap());
        let seen = fake.seen.lock().unwrap();
        assert_eq!(
            seen.iter()
                .filter(|line| line.starts_with("vm-oracle scenario "))
                .count(),
            4,
            "ours is not traced"
        );
    }

    #[test]
    fn a_scenario_passes_with_two_equal_traces_of_its_progs() {
        let dir = fresh_dir("id101-scenario");
        let work = dir.join("work");
        fs::create_dir_all(&work).unwrap();
        let ok = Fake::new(tools(progs(), trace(&progs(), false)));
        let oracles = fake_oracles(&work, &ok);
        assert!(run_scenario(&oracles, &work, "s", "progs ../progs.dat\n", &progs()).unwrap());
        let stopped = Fake::new(tools(progs(), trace(&progs(), true)));
        let oracles = fake_oracles(&work, &stopped);
        // An error record ends a scenario normally, but this one executed no statement.
        assert!(!run_scenario(&oracles, &work, "s", "", &progs()).unwrap());
        // The header names another progs.
        let other = write(&ProgsFile {
            strings: vec![0],
            ..ProgsFile::default()
        })
        .unwrap();
        let wrong = Fake::new(tools(progs(), trace(&other, false)));
        let oracles = fake_oracles(&work, &wrong);
        assert!(!run_scenario(&oracles, &work, "s", "", &progs()).unwrap());
    }

    #[test]
    fn a_second_run_that_differs_fails_and_a_bad_trace_is_an_error() {
        let dir = fresh_dir("id101-runs");
        let work = dir.join("work");
        fs::create_dir_all(&work).unwrap();
        let runs = AtomicU32::new(0);
        let differs = Fake::new(|command: &Command| {
            let run = runs.fetch_add(1, Ordering::SeqCst);
            Reply {
                files: vec![(last_arg(command), trace(&progs(), run == 1))],
                ..Reply::default()
            }
        });
        let oracles = fake_oracles(&work, &differs);
        assert!(!run_scenario(&oracles, &work, "s", "", &progs()).unwrap());
        let garbage = Fake::new(|command: &Command| Reply {
            files: vec![(last_arg(command), b"QCTR".to_vec())],
            ..Reply::default()
        });
        let oracles = fake_oracles(&work, &garbage);
        assert!(
            run_scenario(&oracles, &work, "s", "", &progs())
                .unwrap_err()
                .contains("bad trace")
        );
        let fails = Fake::new(|_: &Command| Reply::exit(2));
        let oracles = fake_oracles(&work, &fails);
        assert!(
            run_scenario(&oracles, &work, "s", "", &progs())
                .unwrap_err()
                .contains("failed")
        );
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        let oracles = fake_oracles(&work, &cannot);
        assert!(run_scenario(&oracles, &work, "s", "", &progs()).is_err());
    }

    #[test]
    fn a_codebase_qcc_cannot_compile_is_an_error() {
        let dir = fresh_dir("id101-compile");
        let fake = Fake::new(tools(progs(), Vec::new()));
        let oracles = fake_oracles(&dir.join("work"), &fake);
        let error = compile(&oracles, &codebase(&dir, false)).unwrap_err();
        assert!(error.contains("no progs.src"), "{error}");
        let codebase = codebase(&dir, true);
        let errors = Fake::new(|_: &Command| Reply {
            stdout: "world.qc:1: ERROR: unknown value".to_owned(),
            ..Reply::default()
        });
        let oracles = fake_oracles(&dir.join("work"), &errors);
        assert!(
            compile(&oracles, &codebase)
                .unwrap_err()
                .contains("did not compile")
        );
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        let oracles = fake_oracles(&dir.join("work"), &cannot);
        assert!(
            compile(&oracles, &codebase)
                .unwrap_err()
                .contains("cannot run qcc")
        );
        assert!(compile(&oracles, &dir.join("nowhere")).is_err());
    }
}
