// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs every map of an install through its game's progs, the player standing still, then
//! saves it, loads the save over a fresh spawn of the map and plays on, and reports each map's
//! first QuakeC error: the record for P3-24's acceptance, "id1 and rerelease
//! progs run on every corpus map without a QC error".
//!
//! ```text
//! cargo run --release -p qs-sim --example sweep -- --basedir <game root> [--game <dir>] [--seconds <s>]
//! ```

use std::io::Write as _;
use std::rc::Rc;

use qc_progs::Progs;
use qs_sim::localization::{Localization, RRE_LOC_FILE};
use qs_sim::rerelease::{BuiltinTable, patch_builtins};
use qs_sim::{LoadOptions, SaveError, SaveGame, UserCmd, World};
use qs_vfs::{LoadedFile, StdFs, Vfs};

/// The frame the host runs at, `sys_ticrate`'s 0.05 doubled to the 0.1 a frame can last.
const FRAME: f64 = 0.1;

/// Runs every map of an install through its game's progs.
#[derive(clap::Parser)]
struct Args {
    /// The game's root, the engine's `-basedir`.
    #[arg(long)]
    basedir: String,
    /// The game directory over `id1`, the engine's `-game`.
    #[arg(long)]
    game: Option<String>,
    /// The seconds each map runs before it is saved and loaded.
    #[arg(long, default_value_t = 60.0)]
    seconds: f64,
}

fn main() -> Result<(), String> {
    let Args {
        basedir,
        game,
        seconds,
    } = <Args as clap::Parser>::parse();
    let mut engine = vec!["-basedir".to_owned(), basedir];
    if let Some(game) = game {
        engine.extend(["-game".to_owned(), game]);
    }
    let vfs = Vfs::new(StdFs, &engine, ".").map_err(|e| format!("{e:?}"))?;
    let read = |name: &str| {
        vfs.load(name.as_bytes())
            .ok()
            .flatten()
            .map(LoadedFile::into_bytes)
    };
    let mut progs_bytes = read("progs.dat").ok_or("progs.dat is not on the search path")?;
    let builtins = patch_builtins(&mut progs_bytes);
    let progs = Progs::read(&progs_bytes).map_err(|e| format!("progs.dat: {e}"))?;
    let localization = vfs
        .load_rerelease(RRE_LOC_FILE)
        .ok()
        .flatten()
        .map(|text| Localization::loc_load_file(&text))
        .unwrap_or_default();
    let mut maps: Vec<String> = vfs
        .files()
        .iter()
        .filter_map(|file| {
            let name = core::str::from_utf8(file.name()).ok()?;
            let map = name.strip_prefix("maps/")?.strip_suffix(".bsp")?;
            (!map.contains('/') && !map.starts_with("b_")).then(|| map.to_owned())
        })
        .collect();
    maps.sort();
    maps.dedup();
    let mut failed = 0_usize;
    let mut out = std::io::stdout().lock();
    for map in &maps {
        let result = run(
            &progs,
            map,
            &read,
            &engine,
            localization.clone(),
            builtins,
            seconds,
        );
        match result {
            Ok(()) => writeln!(out, "ok    {map}"),
            Err(error) => {
                failed = failed.saturating_add(1);
                writeln!(out, "FAIL  {map}: {error}")
            }
        }
        .map_err(|e| e.to_string())?;
    }
    writeln!(out, "sweep: {} maps, {failed} with an error", maps.len())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Loads `map`, settles it, connects the player and runs `seconds` of frames.
fn run(
    progs: &Progs<'_>,
    map: &str,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    engine: &[String],
    localization: Localization,
    builtins: BuiltinTable,
    seconds: f64,
) -> Result<(), String> {
    let name = format!("maps/{map}.bsp");
    let bytes = read(&name).ok_or_else(|| format!("{name} is missing"))?;
    let bsp =
        qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::default()).map_err(|e| e.message(&name))?;
    let models = || -> Result<qs_sim::ModelFiles, String> {
        let files = Vfs::new(StdFs, engine, ".").map_err(|e| format!("{e:?}"))?;
        Ok(Rc::new(move |name: &str| {
            files
                .load(name.as_bytes())
                .ok()
                .flatten()
                .map(LoadedFile::into_bytes)
        }))
    };
    let registration = Vfs::new(StdFs, engine, ".")
        .map_err(|e| format!("{e:?}"))?
        .registration();
    let options = LoadOptions {
        localization,
        builtins,
        carried: None,
        registration,
        host: None,
        search_path: Vec::new(),
    };
    let mut world =
        World::load(progs, map, &bsp, models()?, options.clone()).map_err(|e| e.to_string())?;
    world.settle(progs).map_err(|e| e.to_string())?;
    world.connect(progs).map_err(|e| e.to_string())?;
    play(&mut world, progs, seconds)?;
    // PHYS-10.5: saved, read back over a fresh spawn of the map, and played on.
    let saved = match world.save_game(progs) {
        Ok(saved) => saved,
        // A dead player cannot be saved; the map ran.
        Err(SaveError::DeadPlayer) => return Ok(()),
        Err(error) => return Err(format!("saving: {error}")),
    };
    let game = SaveGame::read(&saved).map_err(|e| format!("reading its save: {e}"))?;
    let mut back =
        World::load(progs, &game.map, &bsp, models()?, options).map_err(|e| e.to_string())?;
    back.settle(progs).map_err(|e| e.to_string())?;
    back.restore(progs, &game)
        .map_err(|e| format!("restoring its save: {e}"))?;
    back.connect(progs).map_err(|e| e.to_string())?;
    play(&mut back, progs, 5.0).map_err(|e| format!("after loading: {e}"))?;
    Ok(())
}

/// `seconds` of frames with the player standing still.
fn play(world: &mut World, progs: &Progs<'_>, seconds: f64) -> Result<(), String> {
    let mut time = 0.0;
    while time < seconds {
        world.set_command(UserCmd::default());
        world
            .frame(progs, FRAME)
            .map_err(|e| format!("at {time:.1} s: {e}"))?;
        world.take_messages();
        time += FRAME;
    }
    Ok(())
}
