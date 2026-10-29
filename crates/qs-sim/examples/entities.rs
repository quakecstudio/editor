// SPDX-License-Identifier: GPL-3.0-or-later

//! Lists the entities a map spawns through its game's progs: a development check of
//! `qs-sim`, not a tool.
//!
//! ```text
//! cargo run -p qs-sim --example entities -- <maps/x.bsp> <progs.dat>
//! ```

use std::collections::BTreeMap;
use std::io::Write as _;

use qs_sim::World;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [bsp_path, progs] = args.as_slice() else {
        return Err("usage: entities <maps/x.bsp> <progs.dat>".to_owned());
    };
    let read = |path: &String| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
    let (bsp_bytes, progs_bytes) = (read(bsp_path)?, read(progs)?);
    let map = std::path::Path::new(bsp_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("map");
    let bsp =
        qs_bsp::Bsp::read(&bsp_bytes, qs_bsp::BspLoader::default()).map_err(|e| e.message(map))?;
    let progs = qc_progs::Progs::read(&progs_bytes).map_err(|e| e.to_string())?;
    // Other models are read beside the map, as `maps/b_bh25.bsp` is in `maps/`.
    let root = std::path::Path::new(bsp_path)
        .parent()
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let files = Box::new(move |name: &str| std::fs::read(root.join(name)).ok());
    let world = World::load(&progs, map, &bsp, files).map_err(|e| e.to_string())?;
    let mut models: BTreeMap<String, usize> = BTreeMap::new();
    for entity in world.entities() {
        let count = models.entry(entity.model).or_default();
        *count = count.saturating_add(1);
    }
    let mut out = std::io::stdout().lock();
    for (model, count) in models {
        writeln!(out, "{count:4}  {model}").map_err(|e| e.to_string())?;
    }
    Ok(())
}
