// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask map-corpus <dir>...`: reads every `.map` file under each directory with
//! `qs-map` and checks that it writes back byte for byte (P5-02, MAP-1 to MAP-7). It also
//! reports what id's qbsp would make of each file (MAP-5.5: a Valve 220 map stops it) and how
//! many diagnostics the reader gives, which do not fail the run.
//!
//! The maps come from `cargo xtask corpus fetch <id>...`, which writes them to
//! `<cache>/map/<id>.map`.

use std::fs;
use std::path::{Path, PathBuf};

use qs_map::{MapFile, TexDef, qbsp};

const USAGE: &str = "usage: cargo xtask map-corpus <dir>...";

/// Every `.map` file under `dir`, sorted.
fn maps(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("map"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Checks one map; `Err` says why it fails.
fn check(path: &Path) -> Result<String, String> {
    let text = fs::read(path).map_err(|e| e.to_string())?;
    let map = MapFile::parse(&text).map_err(|e| e.to_string())?;
    if map.to_bytes() != text {
        return Err("does not write back byte for byte".to_owned());
    }
    let mut brushes = 0_usize;
    let mut faces = 0_usize;
    let mut valve = 0_usize;
    for entity in &map.entities {
        for brush in entity.brushes() {
            brushes = brushes.saturating_add(1);
            faces = faces.saturating_add(brush.faces.len());
            valve = valve.saturating_add(
                brush
                    .faces
                    .iter()
                    .filter(|face| matches!(face.texdef(), TexDef::Valve { .. }))
                    .count(),
            );
        }
    }
    let qbsp = match qbsp::load_map_file(&text) {
        Ok(read) => format!("qbsp reads it ({} warnings)", read.warnings.len()),
        Err(error) => format!("qbsp stops: {error}"),
    };
    Ok(format!(
        "{} entities, {brushes} brushes, {faces} faces ({valve} Valve 220), {} diagnostics; {qbsp}",
        map.entities.len(),
        map.check().len()
    ))
}

/// Runs the check on the directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    if args.is_empty() || args.iter().any(|arg| arg.starts_with("--")) {
        return Err(USAGE.to_owned());
    }
    let mut ok = true;
    let mut count = 0_usize;
    for dir in args {
        let files = maps(Path::new(dir))?;
        if files.is_empty() {
            return Err(format!("no .map files under {dir}"));
        }
        for path in &files {
            count = count.saturating_add(1);
            match check(path) {
                Ok(summary) => println!("ok    {}: {summary}", path.display()),
                Err(error) => {
                    ok = false;
                    println!("FAIL  {}: {error}", path.display());
                }
            }
        }
    }
    println!(
        "map-corpus: {count} maps, {}",
        if ok {
            "every one round-trips"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::main;
    use crate::test_dirs::fresh;

    fn dir(name: &str) -> PathBuf {
        let dir = fresh(&format!("map-corpus/{name}"));
        fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    fn vector(name: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../docs/spec/vectors/map")
                .join(name),
        )
        .unwrap()
    }

    fn run(dir: &Path) -> Result<bool, String> {
        main(Path::new("."), &[dir.display().to_string()])
    }

    #[test]
    fn maps_of_both_formats_round_trip() {
        let maps = dir("map-corpus-ok");
        fs::write(maps.join("standard.map"), vector("standard.txt")).unwrap();
        fs::write(maps.join("sub/valve.MAP"), vector("valve220.txt")).unwrap();
        assert!(run(&maps).unwrap());
    }

    #[test]
    fn a_map_that_does_not_parse_fails() {
        let maps = dir("map-corpus-bad");
        fs::write(maps.join("standard.map"), vector("standard.txt")).unwrap();
        fs::write(maps.join("broken.map"), vector("eof-in-entity.txt")).unwrap();
        assert!(!run(&maps).unwrap());
    }

    #[test]
    fn no_maps_or_a_flag_is_an_error() {
        let maps = dir("map-corpus-empty");
        assert!(run(&maps).unwrap_err().contains("no .map files"));
        assert!(run(&maps.join("nowhere")).is_err());
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        assert!(main(Path::new("."), &["--all".to_owned()]).is_err());
    }
}
