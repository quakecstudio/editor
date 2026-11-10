// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use qc_edict::system::{SYSTEM_GLOBALS, defs_source};
use qc_project::MemoryLoader;
use qc_types::Etype;
use serde_json::json;

use super::{VmRunError, VmRunParams, run};
use crate::{Config, Workbench};

/// id's system globals and fields (EDICT-2.1, EDICT-3.1), which the VM checks a program has,
/// and an empty body for each function the engine calls.
pub(crate) fn system_defs() -> String {
    let mut text = defs_source();
    for var in SYSTEM_GLOBALS
        .iter()
        .filter(|var| var.etype == Etype::Function)
    {
        text.push_str(&format!("void() {} = {{}};\n", var.name));
    }
    text
}

/// The QuakeC every test builds after the system's: a weapon function that calls builtin 1
/// (`makevectors`), one that reads a global, and one that fails at run time.
const CODE: &str = "\
float shots;\n\
void(vector ang) makevectors = #1;\n\
void() W_FireShotgun = {\n\
\tif (self.ammo_shells < 1) return;\n\
\tself.ammo_shells = self.ammo_shells - 1;\n\
\tmakevectors(self.angles);\n\
\tshots = shots + 1;\n\
};\n\
void() Hurt = { self.health = self.health - time; };\n\
void() Broken = { local entity e; e.health = 1; };\n";

/// The program the project of [`CODE`] builds, in a fresh directory for test `name`.
fn built(name: &str) -> (PathBuf, Vec<u8>) {
    let root =
        std::env::temp_dir().join(format!("qs-workbench-vm-run-{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let mut loader = MemoryLoader::new();
    loader.insert(root.join("progs.src"), b"progs.dat\nweapons.qc\n".to_vec());
    loader.insert(root.join("weapons.qc"), (system_defs() + CODE).into_bytes());
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(&root)).unwrap();
    let modules = workbench.build().unwrap();
    let output = modules.into_iter().find(|m| m.built).unwrap().output;
    let bytes = std::fs::read(&output).unwrap();
    (output, bytes)
}

fn params(
    function: &str,
    globals: &[(&str, serde_json::Value)],
    fields: &[(&str, serde_json::Value)],
) -> VmRunParams {
    let map = |pairs: &[(&str, serde_json::Value)]| -> BTreeMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    };
    VmRunParams {
        function: function.to_owned(),
        globals: map(globals),
        self_fields: map(fields),
    }
}

// No spec statement: P4-02, a weapon function runs on a self edict set from the parameters,
// its builtin answered by the mock engine, and the answer lists each value it changed
#[test]
fn a_weapon_function_runs_over_the_mock_engine() {
    let (progs, bytes) = built("weapon");
    let started = Instant::now();
    let ran = run(
        progs,
        &bytes,
        &params(
            "W_FireShotgun",
            &[],
            &[("ammo_shells", json!(5)), ("angles", json!([0, 90, 0]))],
        ),
    )
    .unwrap();
    assert!(started.elapsed().as_millis() < 50, "P4-02: under 50 ms");
    assert_eq!(ran.error, None);
    let places: Vec<&str> = ran.changes.iter().map(|c| c.place.as_str()).collect();
    assert!(places.contains(&"global shots"), "{places:?}");
    assert!(
        places.contains(&"global v_forward"),
        "makevectors ran: {places:?}"
    );
    let shells = ran
        .changes
        .iter()
        .find(|c| c.place.ends_with(".ammo_shells"))
        .unwrap();
    assert_eq!(shells.before, json!({ "type": "float", "value": 5.0 }));
    assert_eq!(shells.after, json!({ "type": "float", "value": 4.0 }));
}

// No spec statement: P4-02, globals are set before the call; with no shells nothing changes
#[test]
fn globals_are_set_and_an_early_return_changes_nothing() {
    let (progs, bytes) = built("globals");
    let hurt = run(
        progs.clone(),
        &bytes,
        &params("Hurt", &[("time", json!(30))], &[("health", json!(100))]),
    )
    .unwrap();
    let health = hurt
        .changes
        .iter()
        .find(|c| c.place.ends_with(".health"))
        .unwrap();
    assert_eq!(health.after, json!({ "type": "float", "value": 70.0 }));
    let empty = run(
        progs,
        &bytes,
        &params("W_FireShotgun", &[], &[("ammo_shells", json!(0))]),
    )
    .unwrap();
    assert!(empty.changes.is_empty(), "{:?}", empty.changes);
}

// No spec statement: P4-02, a run error is answered with the changes before it, and a missing
// function, a missing name or a value of the wrong type is an error
#[test]
fn errors_are_answered() {
    let (progs, bytes) = built("errors");
    let broken = run(progs.clone(), &bytes, &params("Broken", &[], &[])).unwrap();
    assert!(broken.error.is_some());
    assert!(matches!(
        run(progs.clone(), &bytes, &params("Nope", &[], &[])),
        Err(VmRunError::NoFunction(_))
    ));
    assert!(matches!(
        run(
            progs.clone(),
            &bytes,
            &params("Hurt", &[("nope", json!(1))], &[])
        ),
        Err(VmRunError::Bad(_))
    ));
    assert!(matches!(
        run(
            progs.clone(),
            &bytes,
            &params("Hurt", &[], &[("angles", json!(1))])
        ),
        Err(VmRunError::Bad(_))
    ));
    assert!(matches!(
        run(progs, b"not a progs", &params("Hurt", &[], &[])),
        Err(VmRunError::Progs(_))
    ));
}
