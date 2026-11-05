// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a map from the user's Quake install (P1-19): its server, its client, what the
//! renderer draws, and where a free camera stands in it.

use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader, Lit};
use qs_client::{Client, ModelInfo, Models, Random};
use qs_mdl::mdl::Mdl;
use qs_mdl::spr::Sprite;
use qs_render::{Camera, Extras, MapMesh, MeshBuilder, Palette, Pics, Placement, Play, Surface};
use qs_sim::localization::{Localization, RRE_LOC_FILE};
use qs_sim::rerelease::patch_builtins;
use qs_sim::{LoadOptions, World};
use qs_vfs::zip::Zip;
use qs_vfs::{LoadedFile, StdFs, Vfs};

use crate::args::{Args, Show};
use crate::audio::{Audio, Device};
use crate::game::Game;

/// The height of a player's eyes above its origin, `view_ofs` in id's QuakeC `client.qc`.
const EYE_HEIGHT: f32 = 22.0;
/// QuakeSpasm's `r_nolerp_list`: the models whose animation is not interpolated
/// (CLIENT-7.3).
const NOLERP: [&str; 9] = [
    "progs/flame.mdl",
    "progs/flame2.mdl",
    "progs/braztall.mdl",
    "progs/brazshrt.mdl",
    "progs/longtrch.mdl",
    "progs/flame_pyre.mdl",
    "progs/v_saw.mdl",
    "progs/v_xfist.mdl",
    "progs/h2stuff/newfire.mdl",
];
/// The 2D pictures the HUD reads from files rather than `gfx.wad` (HUD-2).
const LMPS: [&str; 4] = [
    "gfx/ranking.lmp",
    "gfx/complete.lmp",
    "gfx/inter.lmp",
    "gfx/finale.lmp",
];

/// A map read from an install: its name, its mesh, its play assets and where its player
/// starts.
pub(crate) struct Loaded {
    pub(crate) map: String,
    pub(crate) mesh: MapMesh,
    pub(crate) play: Play,
    pub(crate) camera: Camera,
    pub(crate) game: Game,
    pub(crate) audio: Audio,
    pub(crate) client: Client,
    pub(crate) bsp: Bsp<'static>,
    pub(crate) describer: Describer,
    pub(crate) rand: Rand,
}

/// A file of the install by name, `None` when it is not on the search path.
type Reader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;
/// The search-path element a file is found in, the first 0.
type Ranker = Box<dyn Fn(&str) -> Option<usize>>;

/// The models the client asks about, read from the install.
pub(crate) struct Describer {
    read: Reader,
}

impl Models for Describer {
    fn describe(&mut self, name: &str) -> Option<ModelInfo> {
        if name.starts_with('*') || name.ends_with(".bsp") {
            return Some(ModelInfo::brush());
        }
        let bytes = (self.read)(name)?;
        if name.ends_with(".spr") {
            return Some(ModelInfo::sprite(&Sprite::read(&bytes).ok()?));
        }
        let mut info = ModelInfo::alias(&Mdl::read(&bytes).ok()?);
        info.nolerp = NOLERP.contains(&name);
        Some(info)
    }
}

/// `rand ()` for the client: a linear congruential generator over C's `RAND_MAX`.
pub(crate) struct Rand(u32);

impl Rand {
    fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        i32::try_from((self.0 >> 1) & 0x7fff_ffff).unwrap_or(0)
    }
}

/// The client and the sound share the engine's one `rand ()`.
impl Random for Rand {
    fn rand(&mut self) -> i32 {
        self.next()
    }
}

impl qs_sound::Random for Rand {
    fn rand(&mut self) -> i32 {
        self.next()
    }
}

/// Reads the map `args` names from the install `-basedir` and `-game` name, with the engine
/// options after `--` passed on to the game filesystem; starts its server, connects its
/// client and reads the models the server precached.
#[expect(
    clippy::too_many_lines,
    reason = "one pass from the command line to a loaded map reads best in the order it runs"
)]
pub(crate) fn load(args: &Args) -> Result<Loaded, String> {
    let map = args.map.clone();
    let engine = args.engine();
    let show = args.show.map(|show| match show {
        Show::Water => Surface::Turbulent,
        Show::Sky => Surface::Sky,
    });
    let at: Option<[f32; 4]> = args
        .at
        .as_deref()
        .and_then(|values| <[f32; 4]>::try_from(values).ok());
    let zfix = !args.nozfix;
    let gamma = args.gamma;
    let vfs = Vfs::new(StdFs, &engine, ".").map_err(|e| format!("{e:?}"))?;
    let read = |name: &str| -> Result<Vec<u8>, String> {
        vfs.load(name.as_bytes())
            .map_err(|e| format!("{name}: {e:?}"))?
            .map(LoadedFile::into_bytes)
            .ok_or_else(|| format!("{name} is not on the search path; is --basedir the game root?"))
    };
    let palette = Palette::read(&read("gfx/palette.lmp")?)
        .map_err(|e| e.to_string())?
        .with_gamma(gamma);
    let name = format!("maps/{map}.bsp");
    let bytes = read(&name)?;
    let bsp = Bsp::read(&bytes, BspLoader::default()).map_err(|e| e.message(&name))?;
    let mut progs_bytes = read("progs.dat")?;
    // RRE-4.1: QuakeSpasm gives the rerelease's named functions their builtins as it loads the
    // progs, and RRE-4.2 its table then runs to 92.
    let builtins = patch_builtins(&mut progs_bytes);
    let progs = Progs::read(&progs_bytes).map_err(|e| format!("progs.dat: {e}"))?;
    // The world reads the other models the progs precaches, such as the health boxes, from the
    // same search path; so do the client and the renderer.
    let reader = || -> Result<Reader, String> {
        let files = Vfs::new(StdFs, &engine, ".").map_err(|e| format!("{e:?}"))?;
        Ok(Box::new(move |name: &str| {
            files
                .load(name.as_bytes())
                .ok()
                .flatten()
                .map(LoadedFile::into_bytes)
        }))
    };
    // The search-path element a file is found in, which decides whether an MD5 model
    // replaces its `.mdl` (REND-12.1).
    let ranker = || -> Result<Ranker, String> {
        let files = Vfs::new(StdFs, &engine, ".").map_err(|e| format!("{e:?}"))?;
        Ok(Box::new(move |name: &str| {
            files.find(name.as_bytes()).map(|file| file.element())
        }))
    };
    // RRE-1.6, RRE-1.10: the rerelease's strings, read before the map as QuakeSpasm's
    // `LOC_Init` reads them, from the search path, the base directory or `QuakeEX.kpf`.
    let text = localization(&vfs, args)?;
    let options = LoadOptions {
        localization: text
            .map(|text| Localization::loc_load_file(&text))
            .unwrap_or_default(),
        builtins,
        carried: None,
    };
    let mut world =
        World::load(&progs, &map, &bsp, reader()?, options).map_err(|e| e.to_string())?;
    world.settle(&progs).map_err(|e| e.to_string())?;
    world.connect(&progs).map_err(|e| e.to_string())?;
    let mut game = Game::new(world, progs_bytes.to_vec());
    game.announce("progs.dat", &map);
    // The client reads the serverinfo and the spawn messages the connection wrote.
    let mut client = Client::new(qs_client::Game::Standard);
    let mut describer = Describer { read: reader()? };
    let mut rand = Rand(1);
    let packets = game.world_mut().take_messages().packets;
    let events = client
        .frame(0.0, &packets, &mut describer, &mut rand)
        .map_err(|e| format!("client: {e}"))?;
    let sounds = reader()?;
    let output = if args.nosound {
        Device::None
    } else {
        Device::Default
    };
    let mut audio = Audio::open(Box::new(move |name: &str| sounds(name)), output);
    audio.handle(&events, &mut rand);
    let mut pics = Pics::new();
    pics.add_wad(&read("gfx.wad")?, &palette)
        .map_err(|e| e.to_string())?;
    for lmp in LMPS {
        if let Ok(bytes) = read(lmp) {
            pics.add_lmp(lmp, &bytes, &palette)
                .map_err(|e| e.to_string())?;
        }
    }
    let mut play = Play::new(palette.clone(), pics, bsp.entities());
    play.zfix = zfix;
    play.load_models(client.model_names(), &reader()?, &ranker()?)
        .map_err(|e| e.to_string())?;
    play.load_sky(&reader()?);
    // The world, then every brush submodel `*n` at the origin, so the mesh's model `n` is the
    // submodel `n`; then each entity showing a BSP of its own, such as a health box.
    let placements: Vec<Placement> = (0..bsp.models.len())
        .map(|model| Placement {
            model,
            ..Placement::WORLD
        })
        .collect();
    let mut others: Vec<(String, Vec<(Placement, usize)>)> = Vec::new();
    for entity in game.world().entities().into_iter().skip(1) {
        if entity.model.ends_with(".bsp") && !entity.model.starts_with("maps/") {
            let placement = Placement {
                model: 0,
                origin: entity.origin,
                angles: entity.angles,
            };
            let placed = (placement, entity.number);
            match others.iter_mut().find(|(name, _)| *name == entity.model) {
                Some((_, list)) => list.push(placed),
                None => others.push((entity.model, vec![placed])),
            }
        }
    }
    // A `.lit` beside the map lights it in colour (REND-6, BSP-18); one that is not usable is
    // reported and left out, as QuakeSpasm does. BSP-18.4's game-directory priority is not
    // checked here.
    let lit_bytes = read(&format!("maps/{map}.lit")).ok();
    let lit = lit_bytes.as_deref().and_then(|bytes| {
        Lit::read(bytes, bsp.lighting.len())
            .map_err(|error| tracing::warn!(%error, "maps/{map}.lit"))
            .ok()
    });
    let mut builder = MeshBuilder::new(&palette);
    let extras = Extras {
        lit: lit.as_ref(),
        ..Extras::default()
    };
    builder
        .add_with(&bsp, &extras, &placements)
        .map_err(|e| e.to_string())?;
    let mut next = placements.len();
    for (name, list) in &others {
        let bytes = read(name)?;
        let other = Bsp::read(&bytes, BspLoader::default()).map_err(|e| e.message(name))?;
        let list: Vec<Placement> = list.iter().map(|(p, _)| *p).collect();
        builder.add(&other, &list).map_err(|e| e.to_string())?;
    }
    for (_, list) in &others {
        for (_, entity) in list {
            play.add_instance(*entity, next);
            next = next.saturating_add(1);
        }
    }
    let mesh = builder.finish();
    let placed = at.map(|[x, y, z, yaw]| {
        let mut camera = Camera::new([x, y, z], yaw);
        camera.turn(0.0, 30.0);
        camera
    });
    let camera = placed
        .or_else(|| show.and_then(|surface| facing(&mesh, surface)))
        .or_else(|| start(bsp.entities()))
        .unwrap_or_else(|| {
            let [a, b, c] = mesh.mins;
            let [x, y, z] = mesh.maxs;
            Camera::new([(a + x) / 2.0, (b + y) / 2.0, (c + z) / 2.0], 0.0)
        });
    let bsp_bytes: &'static [u8] = Vec::leak(bytes.clone());
    let bsp = Bsp::read(bsp_bytes, BspLoader::default()).map_err(|e| e.message(&name))?;
    Ok(Loaded {
        map,
        mesh,
        play,
        game,
        audio,
        client,
        bsp,
        describer,
        rand,
        camera,
    })
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

/// The rerelease's strings (RRE-1.6): with `-kpf`, from the search path, else that archive
/// (ADR-0008); without it, wherever QuakeSpasm's `LOC_LoadFile` finds them (RRE-1.10).
fn localization(vfs: &Vfs<StdFs>, args: &Args) -> Result<Option<Vec<u8>>, String> {
    let Some(kpf) = &args.kpf else {
        return vfs
            .load_rerelease(RRE_LOC_FILE)
            .map_err(|e| format!("{RRE_LOC_FILE}: {e}"));
    };
    if let Some(file) = vfs
        .load(RRE_LOC_FILE.as_bytes())
        .map_err(|e| format!("{RRE_LOC_FILE}: {e}"))?
    {
        return Ok(Some(file.into_bytes()));
    }
    let file = std::fs::File::open(kpf).map_err(|e| format!("{}: {e}", kpf.display()))?;
    Zip::open(file)
        .and_then(|mut archive| archive.read(RRE_LOC_FILE))
        .map_err(|e| format!("{}: {e}", kpf.display()))
}

#[cfg(test)]
mod tests;
