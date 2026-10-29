// SPDX-License-Identifier: GPL-3.0-or-later

//! What the examples share: reading a map from a Quake install and where to stand in it.

use qs_bsp::{Bsp, BspLoader};
use qs_render::{Camera, MapMesh, Palette};
use qs_vfs::{LoadedFile, StdFs, Vfs};

/// The height of a player's eyes above its origin, `view_ofs` in id's QuakeC `client.qc`.
const EYE_HEIGHT: f32 = 22.0;

/// A map read from an install: its name, its mesh and where its player starts.
pub(crate) struct Loaded {
    pub(crate) map: String,
    pub(crate) mesh: MapMesh,
    pub(crate) camera: Camera,
}

/// Reads the map the command line names (`e1m1` by default) from the install that `-basedir`
/// and `-game` name, the engine's own options, which every other argument is passed on as.
pub(crate) fn load(args: &[String]) -> Result<Loaded, String> {
    let mut map = "e1m1".to_owned();
    let mut engine = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg.starts_with('-') {
            engine.push(arg.clone());
            if let Some(value) = rest.next() {
                engine.push(value.clone());
            }
        } else {
            map.clone_from(arg);
        }
    }
    let vfs = Vfs::new(StdFs, &engine, ".").map_err(|e| format!("{e:?}"))?;
    let read = |name: &str| -> Result<Vec<u8>, String> {
        vfs.load(name.as_bytes())
            .map_err(|e| format!("{name}: {e:?}"))?
            .map(LoadedFile::into_bytes)
            .ok_or_else(|| format!("{name} is not on the search path; is -basedir the game root?"))
    };
    let palette = Palette::read(&read("gfx/palette.lmp")?).map_err(|e| e.to_string())?;
    let name = format!("maps/{map}.bsp");
    let bytes = read(&name)?;
    let bsp = Bsp::read(&bytes, BspLoader::default()).map_err(|e| e.message(&name))?;
    let mesh = MapMesh::build(&bsp, &palette).map_err(|e| e.to_string())?;
    let camera = start(bsp.entities()).unwrap_or_else(|| {
        let [a, b, c] = mesh.mins;
        let [x, y, z] = mesh.maxs;
        Camera::new([(a + x) / 2.0, (b + y) / 2.0, (c + z) / 2.0], 0.0)
    });
    Ok(Loaded { map, mesh, camera })
}

/// The first `info_player_start`'s origin, at eye height, and angle in the entity lump.
fn start(entities: &[u8]) -> Option<Camera> {
    let text = String::from_utf8_lossy(entities);
    for block in text.split('}') {
        let mut fields = block.split('"').skip(1).step_by(2);
        let mut classname = None;
        let mut origin = None;
        let mut angle = 0.0;
        while let (Some(key), Some(value)) = (fields.next(), fields.next()) {
            match key {
                "classname" => classname = Some(value.to_owned()),
                "origin" => {
                    let v: Vec<f32> = value
                        .split_whitespace()
                        .filter_map(|n| n.parse().ok())
                        .collect();
                    if let [x, y, z] = v.as_slice() {
                        origin = Some([*x, *y, *z + EYE_HEIGHT]);
                    }
                }
                "angle" => angle = value.parse().unwrap_or(0.0),
                _ => {}
            }
        }
        if classname.as_deref() == Some("info_player_start")
            && let Some(origin) = origin
        {
            return Some(Camera::new(origin, angle));
        }
    }
    None
}
