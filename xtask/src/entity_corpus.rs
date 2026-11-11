// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask entity-corpus [--write] [--maps <dir>]... <codebase>...`: the entity definitions
//! `qc-sema` derives from each codebase's spawn functions (P5-03, MAP-1), as the FGD file
//! `project.entities` writes, compared with the golden file
//! `docs/spec/vectors/entities/<dir name>.fgd`; `--write` rewrites the golden file instead.
//!
//! With `--maps`, every map under each directory, a `.bsp` in a `.pak` or on its own, is read
//! with `qs-bsp` and its entity lump with `qs-map`, and each codebase must define a class for
//! every classname the maps use: the engine frees an entity whose classname names no function
//! (EDICT-8.5). The maps CI holds are LibreQuake's; a game folder with id's maps checks those.
//! Each map is also checked against each codebase as `map.check` does (MAP-8, P5-04), and the
//! diagnostics counted; and in each, the first key of a field is misspelt, and the check must
//! report it with that field as the fix (MAP-8.2).
//!
//! The codebases come from `cargo xtask corpus fetch id-1.01 rerelease`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use qc_sema::{EntitySchema, MapDiagnosticKind};
use qs_bsp::{Bsp, BspLoader};
use qs_map::MapFile;
use qs_pak::Pak;
use qs_workbench::entity_classes::PROJECT_ENTITIES;
use qs_workbench::{Config, Dialect, FsLoader, OPEN_PROJECT, Workbench, register};

/// Where the golden files are, under the repository root.
const GOLDEN: &str = "docs/spec/vectors/entities";

/// The codebase read in the vanilla dialect; every other is FTEQCC's.
const VANILLA: &str = "id-1.01";

/// `cargo xtask entity-corpus`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Write the golden files instead of comparing with them.
    #[arg(long)]
    write: bool,
    /// A directory of maps (`.pak` files and `.bsp` files, searched below it) whose
    /// classnames each codebase must define; repeated for several.
    #[arg(long = "maps")]
    maps: Vec<PathBuf>,
    /// Each codebase directory.
    #[arg(required = true)]
    codebases: Vec<PathBuf>,
}

pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut maps = Vec::new();
    for dir in &args.maps {
        for (label, map) in read_maps(dir)? {
            for classname in map.entities.iter().filter_map(|e| e.classname()) {
                let classname = String::from_utf8_lossy(classname).into_owned();
                used.entry(classname).or_default().insert(label.clone());
            }
            maps.push((label, map));
        }
    }
    let mut ok = true;
    for codebase in &args.codebases {
        ok &= check(root, codebase, args.write, &used)?;
        if !maps.is_empty() {
            ok &= check_maps(codebase, &maps)?;
        }
    }
    println!(
        "entity-corpus: {}",
        if ok { "all checks passed" } else { "FAILED" }
    );
    Ok(ok)
}

/// Checks one codebase: its FGD against the golden file and its classes against the maps'.
fn check(
    root: &Path,
    dir: &Path,
    write: bool,
    used: &BTreeMap<String, BTreeSet<String>>,
) -> Result<bool, String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no directory name", dir.display()))?
        .to_owned();
    let (fgd, classes) = entities(dir, dialect_of(dir)?)?;
    let mut ok = true;
    let golden = root.join(GOLDEN).join(format!("{name}.fgd"));
    if write {
        fs::create_dir_all(root.join(GOLDEN)).map_err(|e| format!("{GOLDEN}: {e}"))?;
        fs::write(&golden, &fgd).map_err(|e| format!("{}: {e}", golden.display()))?;
        println!("entity-corpus: {name}: wrote {}", golden.display());
    } else {
        let want = fs::read_to_string(&golden).map_err(|e| format!("{}: {e}", golden.display()))?;
        if want == fgd {
            println!(
                "ok    {name}: {} classes match {GOLDEN}/{name}.fgd",
                classes.len()
            );
        } else {
            ok = false;
            println!("FAIL  {name}: the classes differ from {GOLDEN}/{name}.fgd");
            for (n, (want, got)) in want.lines().zip(fgd.lines()).enumerate() {
                if want != got {
                    println!("      line {}: want {want:?}", n.saturating_add(1));
                    println!("      line {}: got  {got:?}", n.saturating_add(1));
                    break;
                }
            }
            println!("      (cargo xtask entity-corpus --write rewrites the golden file)");
        }
    }
    if !used.is_empty() {
        let missing: Vec<String> = used
            .iter()
            .filter(|(classname, _)| !classes.contains(*classname))
            .map(|(classname, maps)| {
                let maps: Vec<&str> = maps.iter().map(String::as_str).collect();
                format!("{classname} ({})", maps.join(", "))
            })
            .collect();
        if missing.is_empty() {
            println!(
                "ok    {name}: a class for each of the {} classnames the maps use",
                used.len()
            );
        } else {
            ok = false;
            println!(
                "FAIL  {name}: no class for {} classnames the maps use: {}",
                missing.len(),
                missing.join("; ")
            );
        }
    }
    Ok(ok)
}

/// The FGD of a codebase's classes and their names, through `project.entities` as the editor
/// runs it.
fn entities(dir: &Path, dialect: Dialect) -> Result<(String, BTreeSet<String>), String> {
    let mut registry = qs_commands::Registry::new();
    register(&mut registry).map_err(|e| e.to_string())?;
    let config = Config {
        dialect: Some(dialect),
        ..Config::default()
    };
    let mut session = qs_commands::Session::new(Workbench::new(FsLoader, config), registry);
    session
        .execute(OPEN_PROJECT, serde_json::json!({ "path": dir }))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let scratch = std::env::temp_dir().join(format!("xtask-entity-corpus-{}", std::process::id()));
    fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let fgd_path = scratch.join("classes.fgd");
    let found = session
        .execute(PROJECT_ENTITIES, serde_json::json!({ "fgd": fgd_path }))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let fgd = fs::read_to_string(&fgd_path).map_err(|e| format!("{}: {e}", fgd_path.display()))?;
    // The scratch directory holds nothing else.
    fs::remove_dir_all(&scratch).ok();
    let names = found
        .get("classes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|class| class.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect();
    Ok((fgd, names))
}

/// The dialect a codebase is read in: vanilla for [`VANILLA`], else FTEQCC's.
fn dialect_of(dir: &Path) -> Result<Dialect, String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no directory name", dir.display()))?;
    Ok(if name == VANILLA {
        Dialect::Vanilla
    } else {
        Dialect::Fteqcc
    })
}

/// Checks every map against one codebase (MAP-8): counts the diagnostics, and in each map
/// misspells the first key that names a field, which the check must report with that field
/// as the fix (MAP-8.2).
fn check_maps(dir: &Path, maps: &[(String, MapFile)]) -> Result<bool, String> {
    let config = Config {
        dialect: Some(dialect_of(dir)?),
        ..Config::default()
    };
    let mut workbench = Workbench::new(FsLoader, config);
    workbench
        .open_project(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let schema = workbench
        .entity_schema()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = dir.display();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut misses = Vec::new();
    for (label, map) in maps {
        for diagnostic in schema.check(map) {
            let kind = match diagnostic.kind {
                MapDiagnosticKind::NoClassname => "no classname",
                MapDiagnosticKind::UnknownClass { .. } => "unknown classname",
                MapDiagnosticKind::UnknownKey { .. } => "unknown key",
                MapDiagnosticKind::BadValue { .. } => "bad value",
                MapDiagnosticKind::NoTarget { .. } => "dangling target",
                MapDiagnosticKind::Untargeted { .. } => "untargeted targetname",
            };
            let count = counts.entry(kind).or_default();
            *count = count.saturating_add(1);
        }
        if let Some(miss) = misspelling_missed(&schema, map) {
            misses.push(format!("{label}: {miss}"));
        }
    }
    let found: Vec<String> = counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "info  {name}: map.check over {} maps: {}",
        maps.len(),
        if found.is_empty() {
            "nothing".to_owned()
        } else {
            found.join(", ")
        }
    );
    if misses.is_empty() {
        println!(
            "ok    {name}: a misspelt key in each of the {} maps is reported with its field",
            maps.len()
        );
        Ok(true)
    } else {
        println!(
            "FAIL  {name}: a misspelt key was not reported with its field: {}",
            misses.join("; ")
        );
        Ok(false)
    }
}

/// Misspells the first key of `map` that names a field of four bytes or more, as its last
/// byte doubled; what the check said instead of that field as the fix, or `None` when it did.
fn misspelling_missed(schema: &EntitySchema, map: &MapFile) -> Option<String> {
    let clean = schema.check(map);
    let (index, key) = map.entities.iter().enumerate().find_map(|(index, entity)| {
        entity
            .pairs()
            .map(|pair| String::from_utf8_lossy(pair.key()).into_owned())
            .find(|key| {
                key.len() >= 4
                    && !key.starts_with('_')
                    && !clean.iter().any(|d| {
                        d.entity == index
                            && matches!(&d.kind, MapDiagnosticKind::UnknownKey { key: k, .. } if k == key)
                    })
                    && schema.has_field(key)
            })
            .map(|key| (index, key))
    })?;
    let mut misspelt = key.clone();
    misspelt.push(misspelt.chars().last().unwrap_or('x'));
    let mut changed = map.clone();
    let entity = changed.entities.get_mut(index)?;
    entity
        .rename_key(key.as_bytes(), misspelt.as_bytes())
        .ok()?;
    let reported = schema
        .check(&changed)
        .into_iter()
        .find_map(|d| match d.kind {
            MapDiagnosticKind::UnknownKey { key: k, suggestion }
                if d.entity == index && k == misspelt =>
            {
                Some(suggestion)
            }
            _ => None,
        });
    match reported {
        Some(Some(fix)) if fix == key => None,
        Some(fix) => Some(format!("{misspelt} suggested {fix:?}, not {key}")),
        None => Some(format!("{misspelt} was not reported")),
    }
}

/// Each map under `dir`, its BSP's entity lump read as a `.map`.
fn read_maps(dir: &Path) -> Result<Vec<(String, MapFile)>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        let extension = path
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("pak") => {
                let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let pak = Pak::read(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                for entry in pak.entries() {
                    let name = String::from_utf8_lossy(entry.name()).into_owned();
                    if !name.to_ascii_lowercase().ends_with(".bsp") {
                        continue;
                    }
                    let data = pak
                        .entry_data(&bytes, entry)
                        .map_err(|e| format!("{}: {name}: {e}", path.display()))?;
                    let label = format!("{}:{name}", path.display());
                    found.push((label.clone(), entity_lump(&label, data)?));
                }
            }
            Some("bsp") => {
                let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let label = path.display().to_string();
                found.push((label.clone(), entity_lump(&label, &data)?));
            }
            _ => {}
        }
    }
    if found.is_empty() {
        return Err(format!("{}: no maps", dir.display()));
    }
    Ok(found)
}

/// A BSP's entities, its entity lump read as a `.map`.
fn entity_lump(label: &str, bsp: &[u8]) -> Result<MapFile, String> {
    let bsp = Bsp::read(bsp, BspLoader::default()).map_err(|e| format!("{label}: {e}"))?;
    MapFile::parse(bsp.entities()).map_err(|e| format!("{label}: {e}"))
}
