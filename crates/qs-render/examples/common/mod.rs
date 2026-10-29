// SPDX-License-Identifier: GPL-3.0-or-later

//! What the examples share: reading a map from a Quake install and where to stand in it.

use qs_bsp::{Bsp, BspLoader};
use qs_render::{Camera, GAMMA, MapMesh, Palette, Surface};
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
    let mut show = None;
    let mut gamma = GAMMA;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "-gamma" {
            // GLQuake's `-gamma` (REND-3.1).
            gamma = rest
                .next()
                .and_then(|g| g.parse().ok())
                .ok_or("-gamma takes a number")?;
        } else if arg == "-show" {
            show = match rest.next().map(String::as_str) {
                Some("water") => Some(Surface::Turbulent),
                Some("sky") => Some(Surface::Sky),
                other => return Err(format!("-show takes water or sky, not {other:?}")),
            };
        } else if arg.starts_with('-') {
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
    let palette = Palette::read(&read("gfx/palette.lmp")?)
        .map_err(|e| e.to_string())?
        .with_gamma(gamma);
    let name = format!("maps/{map}.bsp");
    let bytes = read(&name)?;
    let bsp = Bsp::read(&bytes, BspLoader::default()).map_err(|e| e.message(&name))?;
    let mesh = MapMesh::build(&bsp, &palette).map_err(|e| e.to_string())?;
    let camera = show
        .and_then(|surface| facing(&mesh, surface))
        .or_else(|| start(bsp.entities()))
        .unwrap_or_else(|| {
            let [a, b, c] = mesh.mins;
            let [x, y, z] = mesh.maxs;
            Camera::new([(a + x) / 2.0, (b + y) / 2.0, (c + z) / 2.0], 0.0)
        });
    Ok(Loaded { map, mesh, camera })
}

/// A camera looking at the middle of the first batch of `surface` in the map: above water
/// looking down, below sky looking up.
fn facing(mesh: &MapMesh, surface: Surface) -> Option<Camera> {
    let batch = mesh.batches.iter().find(|b| b.surface == surface)?;
    let first = usize::try_from(batch.first).ok()?;
    let count = usize::try_from(batch.count).ok()?;
    let corners: Vec<[f32; 3]> = mesh
        .indices
        .get(first..first.checked_add(count)?)?
        .iter()
        .filter_map(|&i| mesh.vertices.get(usize::try_from(i).ok()?))
        .map(|v| v.position)
        .collect();
    let n = f32::from(u16::try_from(corners.len()).ok()?);
    let mut middle = [0.0_f32; 3];
    for corner in &corners {
        for (m, c) in middle.iter_mut().zip(corner) {
            *m += c / n;
        }
    }
    let (rise, pitch) = match surface {
        Surface::Sky => (-160.0, -55.0),
        Surface::Turbulent | Surface::Lit => (96.0, 55.0),
    };
    let mut camera = Camera::new([middle[0] - 96.0, middle[1], middle[2] + rise], 0.0);
    camera.turn(0.0, pitch);
    Some(camera)
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
