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
//!
//! The codebases come from `cargo xtask corpus fetch id-1.01 rerelease`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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
    for dir in &args.maps {
        for (map, classnames) in map_classnames(dir)? {
            for classname in classnames {
                used.entry(classname).or_default().insert(map.clone());
            }
        }
    }
    let mut ok = true;
    for codebase in &args.codebases {
        ok &= check(root, codebase, args.write, &used)?;
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
    let dialect = if name == VANILLA {
        Dialect::Vanilla
    } else {
        Dialect::Fteqcc
    };
    let (fgd, classes) = entities(dir, dialect)?;
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

/// Each map under `dir` and the classnames its entities use.
fn map_classnames(dir: &Path) -> Result<Vec<(String, BTreeSet<String>)>, String> {
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
                    found.push((label.clone(), classnames(&label, data)?));
                }
            }
            Some("bsp") => {
                let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let label = path.display().to_string();
                found.push((label.clone(), classnames(&label, &data)?));
            }
            _ => {}
        }
    }
    if found.is_empty() {
        return Err(format!("{}: no maps", dir.display()));
    }
    Ok(found)
}

/// The classnames of a BSP's entities.
fn classnames(label: &str, bsp: &[u8]) -> Result<BTreeSet<String>, String> {
    let bsp = Bsp::read(bsp, BspLoader::default()).map_err(|e| format!("{label}: {e}"))?;
    let map = MapFile::parse(bsp.entities()).map_err(|e| format!("{label}: {e}"))?;
    Ok(map
        .entities
        .iter()
        .filter_map(|entity| entity.classname())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect())
}
