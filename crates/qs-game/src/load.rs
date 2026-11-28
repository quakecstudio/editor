// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a map from the user's Quake install (P1-19): its server, its client, what the
//! renderer draws, and where a free camera stands in it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use qc_lno::LnoFile;
use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader, Lit};
use qs_client::{Client, DemoRecorder, Event, MENU_SAVEGAMES, ModelInfo, Models, Random};
use qs_mdl::mdl::Mdl;
use qs_mdl::spr::Sprite;
use qs_render::{
    Camera, Degrees, Extras, Look, MapMesh, MeshBuilder, Palette, Pics, Placement, Play, Surface,
};
use qs_sim::debug::{Debugger, locate};
use qs_sim::localization::{Localization, RRE_LOC_FILE};
use qs_sim::rerelease::patch_builtins;
use qs_sim::spawn::LoadError;
use qs_sim::{HostState, LoadOptions, Room, SaveGame, SpawnParms, World, WorldError};
use qs_vfs::zip::Zip;
use qs_vfs::{Element, LoadedFile, StdFs, Vfs};

use crate::AXES;
use crate::Error;
use crate::args::{Args, Show};
use crate::audio::{Audio, CdImage, Device};
use crate::demos::{Connection, Demos, Level};
use crate::game::{Game, HeldSpawn, HostAction, NextLevel, Reporting};
use crate::keys::{STARTUP, config_path, host_files};
use crate::map_bsp::MapBsp;

/// `Host_FindMaxClients`' slots for a `-listen` with no number, or one below 1 (NET-7.1).
const LISTEN_DEFAULT: i32 = 8;
/// `rand ()`'s linear congruential step, `next = next * 1103515245 + 12345`.
const RAND_MULTIPLIER: u32 = 1_103_515_245;
/// The step's increment.
const RAND_INCREMENT: u32 = 12345;
/// C's `RAND_MAX`, the mask of the value given.
const RAND_MAX: u32 = 0x7fff_ffff;
/// How far a viewpoint over a sky's middle rises (down) and looks, in units and degrees.
const SKY_VIEW: (f32, f32) = (-160.0, -55.0);
/// How far a viewpoint over water or a lit surface rises and looks down.
const SURFACE_VIEW: (f32, f32) = (96.0, 55.0);
/// How far a viewpoint stands back from a surface's middle.
const STAND_BACK: f32 = 96.0;
/// The pitch a start at a placed point looks down at.
const PLACED_PITCH: f32 = 30.0;

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
/// The 2D pictures the HUD and the console read from files rather than `gfx.wad` (HUD-2,
/// CON-3.2).
const LMPS: [&str; 40] = [
    "gfx/ranking.lmp",
    "gfx/complete.lmp",
    "gfx/inter.lmp",
    "gfx/finale.lmp",
    "gfx/conback.lmp",
    // The menus' (MENU).
    "gfx/qplaque.lmp",
    "gfx/ttl_main.lmp",
    "gfx/mainmenu.lmp",
    "gfx/ttl_sgl.lmp",
    "gfx/sp_menu.lmp",
    "gfx/menudot1.lmp",
    "gfx/menudot2.lmp",
    "gfx/menudot3.lmp",
    "gfx/menudot4.lmp",
    "gfx/menudot5.lmp",
    "gfx/menudot6.lmp",
    "gfx/help0.lmp",
    "gfx/help1.lmp",
    "gfx/help2.lmp",
    "gfx/help3.lmp",
    "gfx/help4.lmp",
    "gfx/help5.lmp",
    "gfx/box_tl.lmp",
    "gfx/box_ml.lmp",
    "gfx/box_bl.lmp",
    "gfx/box_tm.lmp",
    "gfx/box_mm.lmp",
    "gfx/box_mm2.lmp",
    "gfx/box_bm.lmp",
    "gfx/box_tr.lmp",
    "gfx/box_mr.lmp",
    "gfx/box_br.lmp",
    "gfx/p_option.lmp",
    "gfx/ttl_cstm.lmp",
    "gfx/p_load.lmp",
    "gfx/p_save.lmp",
    "gfx/bigbox.lmp",
    "gfx/menuplyr.lmp",
    "gfx/p_multi.lmp",
    "gfx/mp_menu.lmp",
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
    pub(crate) bsp: MapBsp,
    pub(crate) describer: Describer,
    pub(crate) rand: Rand,
    /// What the map's first client frame printed for the console: the server's version line,
    /// the divider and the level's name (CLIENT-1.7).
    pub(crate) printed: Vec<Vec<u8>>,
    /// The demos of a host that starts on this map: their recorder holds the messages of
    /// its connection, so `record` can start after it (CLIENT-10.2). A host that came here
    /// from another level keeps the demos it had.
    pub(crate) demos: Demos,
}

/// A file of the install by name, `None` when it is not on the search path.
type Reader = Rc<dyn Fn(&str) -> Option<Vec<u8>>>;
/// The search-path element a file is found in, the first 0.
type Ranker = Box<dyn Fn(&str) -> Option<usize>>;

/// The models the client asks about, read from the install.
pub(crate) struct Describer {
    read: Reader,
}

impl Describer {
    /// The file `name` of the game's search path, `None` when it is not on it.
    pub(crate) fn read(&self, name: &str) -> Option<Vec<u8>> {
        (self.read)(name)
    }
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
        self.0 = self
            .0
            .wrapping_mul(RAND_MULTIPLIER)
            .wrapping_add(RAND_INCREMENT);
        i32::try_from((self.0 >> 1) & RAND_MAX).unwrap_or(0)
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

/// Whether an entity's `model` is a brush model of a BSP of its own, drawn from that file:
/// any `.bsp`, the health and ammo boxes and the barrels of `maps/b_*.bsp` among them, but the
/// map's own, `world`, whose models are the mesh's.
fn is_external_brush_model(model: &str, world: &str) -> bool {
    model.ends_with(".bsp") && model != world
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
    let mut middle = [0.0_f32; AXES];
    for corner in &corners {
        for (m, c) in middle.iter_mut().zip(corner) {
            *m += c / n;
        }
    }
    let (rise, pitch) = match surface {
        Surface::Sky => SKY_VIEW,
        Surface::Turbulent | Surface::Lit => SURFACE_VIEW,
    };
    let mut camera = Camera::new(
        [middle[0] - STAND_BACK, middle[1], middle[2] + rise],
        Degrees::new(0.0),
    );
    camera.turn(Degrees::new(0.0), Degrees::new(pitch));
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
            return Some(Camera::new(origin, Degrees::new(angle)));
        }
    }
    None
}

/// The rerelease's strings (RRE-1.6): with `-kpf`, from the search path, else that archive
/// (ADR-0008); without it, wherever QuakeSpasm's `LOC_LoadFile` finds them (RRE-1.10).
fn localization(vfs: &Vfs<StdFs>, args: &Args) -> Result<Option<Vec<u8>>, Error> {
    let Some(kpf) = &args.kpf else {
        return vfs
            .load_rerelease(RRE_LOC_FILE)
            .map_err(|e| Error::Load(format!("{RRE_LOC_FILE}: {e}")));
    };
    if let Some(file) = vfs
        .load(RRE_LOC_FILE.as_bytes())
        .map_err(|e| Error::Load(format!("{RRE_LOC_FILE}: {e}")))?
    {
        return Ok(Some(file.into_bytes()));
    }
    let file =
        std::fs::File::open(kpf).map_err(|e| Error::Load(format!("{}: {e}", kpf.display())))?;
    Zip::open(file)
        .and_then(|mut archive| archive.read(RRE_LOC_FILE))
        .map_err(|e| Error::Load(format!("{}: {e}", kpf.display())))
}

/// The mesh of the map `map`'s `bsp`, read through `palette` in `look`: the world, then every
/// brush submodel `*n` at the origin, so the mesh's model `n` is the submodel `n`; then each of
/// `world`'s entities showing a BSP of its own, such as a health box, with each such entity and
/// its mesh model, for [`Play::set_instances`]. A
/// `.lit` beside the map lights it in colour (REND-6, BSP-18); one that is not usable is
/// reported and left out, as QuakeSpasm does. BSP-18.4's game-directory priority is not
/// checked here. A swap draws its new BSP with it too (BRIDGE-4.17).
pub(crate) fn map_mesh(
    (map, bsp): (&str, &Bsp<'_>),
    world: &World,
    read: &dyn Fn(&str) -> Result<Vec<u8>, Error>,
    (palette, look): (&Palette, Look),
) -> Result<(MapMesh, Vec<(qs_client::EntitySlot, usize)>), Error> {
    let name = format!("maps/{map}.bsp");
    let placements: Vec<Placement> = (0..bsp.models.len())
        .map(|model| Placement {
            model,
            ..Placement::WORLD
        })
        .collect();
    let mut others: Vec<(String, Vec<(Placement, usize)>)> = Vec::new();
    for entity in world.entities().into_iter().skip(1) {
        if is_external_brush_model(&entity.model, &name) {
            let placement = Placement {
                model: 0,
                origin: entity.origin,
                angles: entity.angles,
            };
            let placed = (placement, entity.number.index());
            match others.iter_mut().find(|(name, _)| *name == entity.model) {
                Some((_, list)) => list.push(placed),
                None => others.push((entity.model, vec![placed])),
            }
        }
    }
    let lit_bytes = read(&format!("maps/{map}.lit")).ok();
    let lit = lit_bytes.as_deref().and_then(|bytes| {
        Lit::read(bytes, bsp.lighting.len())
            .map_err(|error| tracing::warn!(%error, "maps/{map}.lit"))
            .ok()
    });
    let mut builder = MeshBuilder::new(palette, look);
    let extras = Extras {
        lit: lit.as_ref(),
        ..Extras::default()
    };
    builder
        .add_with(bsp, &extras, &placements)
        .map_err(|e| Error::Load(e.to_string()))?;
    let mut next = placements.len();
    for (name, list) in &others {
        let bytes = read(name)?;
        let other =
            Bsp::read(&bytes, BspLoader::default()).map_err(|e| Error::Load(e.message(name)))?;
        let list: Vec<Placement> = list.iter().map(|(p, _)| *p).collect();
        builder
            .add(&other, &list)
            .map_err(|e| Error::Load(e.to_string()))?;
    }
    let mut instances = Vec::new();
    for (_, list) in &others {
        for (_, entity) in list {
            instances.push((qs_client::EntitySlot::new(*entity), next));
            next = next.saturating_add(1);
        }
    }
    Ok((builder.finish(), instances))
}

#[cfg(test)]
mod tests;

/// The breakpoints a level loads with, and the level, which a stop in a spawn function holds
/// to load again once its spawning ends (WORLD-9.7).
pub(crate) struct Watched {
    /// The last level's debugger, from the start of the spawning.
    pub(crate) debugger: Debugger,
    /// The level.
    pub(crate) level: NextLevel,
}

/// What the host does after a host command (PHYS-13.6).
pub(crate) enum Acted {
    /// Plays this level now.
    Load(Box<Loaded>),
    /// Stops.
    Quit,
    /// Goes on as it was.
    Stay,
}

/// What a map load's host starts from.
pub(crate) enum Start {
    /// A new host, with or without its startup configuration (INPUT-3.7).
    New(Startup),
    /// The last level's host, kept (MOVE-6.3, PHYS-12.3).
    Kept(HostState),
}

impl Start {
    /// The kept host state for [`LoadOptions::host`] and the console text for
    /// [`LoadOptions::startup`]: `exec quake.rc` only for a new host that runs it (INPUT-3.7).
    pub(crate) fn host_and_startup(self) -> (Option<HostState>, Vec<u8>) {
        match self {
            Self::New(Startup::QuakeRc) => (None, STARTUP.to_vec()),
            Self::New(Startup::Bare) => (None, Vec::new()),
            Self::Kept(host) => (Some(host), Vec::new()),
        }
    }
}

/// Whether a new host runs `quake.rc` before its first map's settings are read (INPUT-3.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Startup {
    /// `exec quake.rc`, as QuakeSpasm's `Host_Init` runs it: the windowed host's.
    QuakeRc,
    /// Nothing: a headless run starts from the defaults, as a scenario needs.
    Bare,
}

impl Args {
    /// Tells the demos how a level asked for came out (CLIENT-10.8), and gives it back.
    fn loaded_or_stayed(
        mut acted: Acted,
        game: &mut Game,
        (demos, client): (&mut Demos, &mut Client),
    ) -> Acted {
        let level = match &mut acted {
            Acted::Load(loaded) => Level::Loaded(&mut loaded.client),
            Acted::Quit | Acted::Stay => Level::Stayed(client),
        };
        demos.after_level(level, game);
        acted
    }

    /// Carries out `action` (PHYS-13.6): a level loaded, the game saved or loaded as
    /// `Host_Savegame_f` and `Host_Loadgame_f` do, or the host stopped.
    pub(crate) fn act(
        &self,
        game: &mut Game,
        action: HostAction,
        (demos, client): (&mut Demos, &mut Client),
    ) -> Acted {
        match action {
            HostAction::Level(next) => {
                // CLIENT-10.6: no level follows the one a demo plays over.
                if demos.refuses(next.connection, game) {
                    return Acted::Stay;
                }
                let watched = game.level_debugger().map(|debugger| Watched {
                    debugger,
                    level: next.clone(),
                });
                // CLIENT-10.3: a new game ends a recording, and `record`'s map starts its own.
                demos.before_level(next.connection, game, client);
                let acted = self.next_level(next, watched, demos.recorder());
                Self::loaded_or_stayed(acted, game, (demos, client))
            }
            HostAction::Spawned(next) => self.next_level(next, None, demos.recorder()),
            HostAction::Save(name) => {
                let path = self.save_path(&name);
                game.console_line(&format!("Saving game to {}...\n", path.display()));
                if let Some(text) = game.save() {
                    match std::fs::write(&path, text) {
                        Ok(()) => game.console_line("done.\n"),
                        Err(_) => game.console_line("ERROR: couldn't open.\n"),
                    }
                }
                Acted::Stay
            }
            HostAction::Load(name) => {
                let path = self.save_path(&name);
                game.console_line(&format!("Loading game from {}...\n", path.display()));
                let Ok(text) = std::fs::read(&path) else {
                    game.console_line("ERROR: couldn't open.\n");
                    return Acted::Stay;
                };
                let save = match SaveGame::read(&text) {
                    Ok(save) => save,
                    Err(error) => {
                        game.console_line(&format!("{error}\n"));
                        return Acted::Stay;
                    }
                };
                let host = game.world().host_state();
                // `Host_Loadgame_f`: "`CL_Disconnect_f ();`" first.
                demos.before_level(Connection::New, game, client);
                let recorder = Some(demos.recorder());
                let acted = match self.load_map(
                    &save.map,
                    None,
                    Start::Kept(host),
                    (Some(&save), None),
                    recorder,
                ) {
                    Ok(loaded) => Acted::Load(Box::new(loaded)),
                    Err(error) => {
                        game.console_line(&format!("{error}\n"));
                        Acted::Stay
                    }
                };
                Self::loaded_or_stayed(acted, game, (demos, client))
            }
            HostAction::Quit => Acted::Quit,
        }
    }

    /// Loads `next`, its spawning watched by `watched`'s breakpoints when given (WORLD-9.7);
    /// stays on this level when it does not load.
    fn next_level(
        &self,
        next: NextLevel,
        watched: Option<Watched>,
        recorder: &mut DemoRecorder,
    ) -> Acted {
        match self.load_map(
            &next.map,
            next.carried,
            Start::Kept(next.host),
            (None, watched),
            Some(recorder),
        ) {
            Ok(loaded) => Acted::Load(Box::new(loaded)),
            Err(error) => {
                tracing::warn!(%error, map = next.map, "the next level did not load");
                Acted::Stay
            }
        }
    }

    /// The game directory, id's `com_gamedir`: the base directory's `-game` folder, else `id1`.
    fn game_dir(&self) -> PathBuf {
        let base = self.basedir.clone().unwrap_or_else(|| PathBuf::from("."));
        base.join(self.game.as_deref().unwrap_or("id1"))
    }

    /// Whether the game starts paused (BRIDGE-3.6): `--paused` with a bridge, whose client
    /// is all that can resume it.
    pub(crate) const fn starts_paused(&self) -> bool {
        self.paused && self.bridge.is_some()
    }

    /// The map the host starts on, loaded as [`Args::load_map`] loads it with `startup`,
    /// and paused when [`Args::starts_paused`]. A map loaded later, by a level change or a
    /// `map` command, is paused when the game was (`Game::carry_from`).
    pub(crate) fn first_map(&self, startup: Startup) -> Result<Loaded, Error> {
        let mut demos = Demos::new();
        let mut loaded = self.load_map(
            &self.map,
            None,
            Start::New(startup),
            (None, None),
            Some(demos.recorder()),
        )?;
        loaded.demos = demos;
        if self.starts_paused() {
            loaded.game.start_paused();
        }
        Ok(loaded)
    }

    /// Reads `map` from the install `-basedir` and `-game` name, with the engine options after
    /// `--` passed on to the game filesystem; starts its server with what the level before it
    /// `carried` (PHYS-10.4), connects its client and reads the models the server precached.
    #[expect(
        clippy::too_many_lines,
        reason = "one pass from the command line to a loaded map reads best in the order it runs"
    )]
    pub(crate) fn load_map(
        &self,
        map: &str,
        carried: Option<SpawnParms>,
        host: Start,
        (save, watched): (Option<&SaveGame>, Option<Watched>),
        recorder: Option<&mut DemoRecorder>,
    ) -> Result<Loaded, Error> {
        let map = map.to_owned();
        let engine = self.engine();
        let show = self.show.map(|show| match show {
            Show::Water => Surface::Turbulent,
            Show::Sky => Surface::Sky,
        });
        let at: Option<[f32; 4]> = self
            .at
            .as_deref()
            .and_then(|values| <[f32; 4]>::try_from(values).ok());
        let look = self.look.look();
        let gamma = self.gamma.unwrap_or_else(|| look.gamma());
        let vfs = Vfs::new(StdFs, &engine, ".").map_err(|e| Error::Load(format!("{e:?}")))?;
        let read = |name: &str| -> Result<Vec<u8>, Error> {
            vfs.load(name.as_bytes())
                .map_err(|e| Error::Load(format!("{name}: {e:?}")))?
                .map(LoadedFile::into_bytes)
                .ok_or_else(|| {
                    Error::Load(format!(
                        "{name} is not on the search path; is --basedir the game root?"
                    ))
                })
        };
        let palette = Palette::read(&read("gfx/palette.lmp")?)
            .map_err(|e| Error::Load(e.to_string()))?
            .with_gamma(gamma);
        let name = format!("maps/{map}.bsp");
        let bytes = read(&name)?;
        let bsp =
            Bsp::read(&bytes, BspLoader::default()).map_err(|e| Error::Load(e.message(&name)))?;
        let mut progs_bytes = read("progs.dat")?;
        // RRE-4.1: QuakeSpasm gives the rerelease's named functions their builtins as it loads the
        // progs, and RRE-4.2 its table then runs to 92.
        let builtins = patch_builtins(&mut progs_bytes);
        let progs =
            Progs::read(&progs_bytes).map_err(|e| Error::Load(format!("progs.dat: {e}")))?;
        // The world reads the other models the progs precaches, such as the health boxes, from the
        // same search path; so do the client and the renderer.
        let reader = || -> Result<Reader, Error> {
            let files = Vfs::new(StdFs, &engine, ".").map_err(|e| Error::Load(format!("{e:?}")))?;
            Ok(Rc::new(move |name: &str| {
                files
                    .load(name.as_bytes())
                    .ok()
                    .flatten()
                    .map(LoadedFile::into_bytes)
            }))
        };
        // INPUT-3.5: in QuakeSpasm's look the host's own `default.cfg` covers id1's.
        let files = || -> Result<Reader, Error> {
            let config = config_path();
            Ok(host_files(reader()?, look, move || {
                config.as_ref().and_then(|path| std::fs::read(path).ok())
            }))
        };
        // The search-path element a file is found in, which decides whether an MD5 model
        // replaces its `.mdl` (REND-12.1).
        let ranker = || -> Result<Ranker, Error> {
            let files = Vfs::new(StdFs, &engine, ".").map_err(|e| Error::Load(format!("{e:?}")))?;
            Ok(Box::new(move |name: &str| {
                files.find(name.as_bytes()).map(|file| file.element())
            }))
        };
        // RRE-1.6, RRE-1.10: the rerelease's strings, read before the map as QuakeSpasm's
        // `LOC_Init` reads them, from the search path, the base directory or `QuakeEX.kpf`.
        let text = localization(&vfs, self)?;
        let (host, startup) = host.host_and_startup();
        // NET-7.1: `-listen`'s slots, `Host_FindMaxClients`' 8 for none or fewer than 1; a new
        // game of several sets `deathmatch` 1, which a later level keeps in its host.
        let maxplayers = self.listen.map_or(qs_sim::MaxPlayers::SINGLE, |n| {
            qs_sim::MaxPlayers::new(if n < 1 { LISTEN_DEFAULT } else { n })
        });
        let settings = if maxplayers.is_multiplayer() && host.is_none() {
            vec![("deathmatch".to_owned(), "1".to_owned())]
        } else {
            Vec::new()
        };
        let options = LoadOptions {
            localization: text.map_or_default(|text| Localization::loc_load_file(&text)),
            builtins,
            carried,
            registration: vfs.registration(),
            host,
            // PHYS-13.7: `COM_Path_f`'s lines, an archive with its count of files.
            search_path: vfs
                .elements()
                .iter()
                .map(|element| match element {
                    Element::Archive(archive) => {
                        format!("{} ({} files)", archive.name(), archive.file_count())
                    }
                    Element::Directory(directory) => directory.name().to_owned(),
                })
                .collect(),
            seed: None,
            settings,
            // NET-6.11: QuakeSpasm's `-protocol`, which a later level's kept host overrides.
            protocol: self.protocol.version(),
            startup,
            // EDICT-1.10, LIMIT-6.3: the game has id's limits.
            room: Room::default(),
            maxplayers,
            profile: qs_sim::EngineProfile::Id,
        };
        // BRIDGE-3.4, WORLD-9.4: the line of each statement, from the `.lno` beside the progs when
        // it is one for this progs (LNO-4.1).
        let lines = read("progs.lno")
            .ok()
            .and_then(|bytes| LnoFile::read(&bytes).ok())
            .filter(|lno| lno.matches_header(progs.header()).is_ok())
            .map_or_default(|lno| lno.lines);
        // BRIDGE-3.4: a load the QuakeC stops says where, as a failed frame's qc/error does.
        let failed = |error: WorldError| Error::Load(load_failure(&progs, &lines, &error));
        let mut world = World::prepare(&progs, &map, &bsp, files()?, options)
            .map_err(|e| Error::Load(e.to_string()))?;
        // WORLD-9.6, WORLD-9.7: a level started from a debugged one spawns watched by its
        // breakpoints; a stop holds the load, and the rest of it waits for the spawning to end.
        let held = match watched {
            Some(Watched { debugger, level }) => HeldSpawn::spawn(&mut world, &progs, debugger)
                .map_err(failed)?
                .map(|spawn| (spawn, level)),
            None => {
                world.spawn_map(&progs).map_err(failed)?;
                None
            }
        };
        if held.is_none() {
            world.settle(&progs).map_err(failed)?;
            // PHYS-10.5: a saved game is read over the settled map before the client enters.
            if let Some(save) = save {
                world
                    .restore(&progs, save)
                    .map_err(|e| Error::Load(e.to_string()))?;
            }
            world.connect(&progs).map_err(failed)?;
        }
        let reporting = if self.bridge.is_some() {
            Reporting::Bridge
        } else {
            Reporting::Off
        };
        let mut game = Game::new(world, progs_bytes.clone(), reporting);
        // BOT-9.1: a game with room for bots has their graph made beside it.
        if maxplayers.is_multiplayer() {
            game.make_bot_graph(bytes.clone());
        }
        // INPUT-3.7: what the startup configuration printed, shown as the map starts.
        game.report_console();
        game.set_lines(lines);
        game.set_files(files()?);
        match held {
            Some((spawn, level)) => game.hold_load(spawn, level),
            None => game.announce("progs.dat", &map),
        }
        // The client reads the serverinfo and the spawn messages the connection wrote.
        let mut client = Client::new(qs_client::Game::Standard);
        let mut describer = Describer { read: reader()? };
        let mut rand = Rand(1);
        let packets = game
            .world_mut()
            .take_messages(qs_sim::ClientId::FIRST)
            .packets;
        // CLIENT-10.2: a demo being recorded takes the connection's messages as they are read.
        let events = match recorder {
            Some(recorder) => {
                // The messages kept for a recording started later are this connection's.
                recorder.clear_signons();
                client.frame_recorded(0.0, &packets, recorder, &mut describer, &mut rand)
            }
            None => client.frame(0.0, &packets, &mut describer, &mut rand),
        }
        .map_err(|e| Error::Load(format!("client: {e}")))?;
        let sounds = reader()?;
        let output = if self.view_script.is_some() {
            Device::Capture
        } else if self.nosound {
            Device::None
        } else {
            Device::Default
        };
        let printed = events
            .iter()
            .filter_map(|event| match event {
                Event::Print(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        // SND-6.4, SND-6.5: the CD's music, the mission pack's own CD for its game, never in a capture, which id's engine has none of to compare.
        let cd = match output {
            Device::Capture => None,
            Device::Default | Device::None if self.nocdaudio => None,
            Device::Default | Device::None => self
                .cd_image
                .clone()
                .or_else(|| {
                    let base = self.basedir.clone().unwrap_or_else(|| PathBuf::from("."));
                    Some(base.join(self.cd_sheet())).filter(|cue| cue.is_file())
                })
                .and_then(|cue| CdImage::open(&cue)),
        };
        // RRE-3.5: the music files, from the same search path, and never in a capture, as the CD.
        let music: qs_sound::Reader = match output {
            Device::Capture => Box::new(|_: &str| None),
            Device::Default | Device::None => {
                let music = Rc::clone(&sounds);
                Box::new(move |name: &str| music(name))
            }
        };
        let mut audio = Audio::open(Box::new(move |name: &str| sounds(name)), output, cd, music);
        audio.handle(&events, &mut rand);
        let mut pics = Pics::new();
        pics.add_wad(&read("gfx.wad")?, &palette)
            .map_err(|e| Error::Load(e.to_string()))?;
        for lmp in LMPS {
            if let Ok(bytes) = read(lmp) {
                pics.add_lmp(lmp, &bytes, &palette)
                    .map_err(|e| Error::Load(e.to_string()))?;
            }
        }
        let mut play = Play::new(palette.clone(), pics, bsp.entities());
        play.look = look;
        play.load_models(client.model_names(), &*reader()?, &ranker()?)
            .map_err(|e| Error::Load(e.to_string()))?;
        play.load_sky(&*reader()?);
        let (mesh, instances) = map_mesh((&map, &bsp), game.world(), &read, (&palette, look))?;
        play.set_instances(instances);
        let placed = at.map(|[x, y, z, yaw]| {
            let mut camera = Camera::new([x, y, z], Degrees::new(yaw));
            camera.turn(Degrees::new(0.0), Degrees::new(PLACED_PITCH));
            camera
        });
        let camera = placed
            .or_else(|| show.and_then(|surface| facing(&mesh, surface)))
            .or_else(|| start(bsp.entities()))
            .unwrap_or_else(|| {
                let [a, b, c] = mesh.mins;
                let [x, y, z] = mesh.maxs;
                Camera::new(
                    [(a + x) / 2.0, (b + y) / 2.0, (c + z) / 2.0],
                    Degrees::new(0.0),
                )
            });
        let bsp = MapBsp::read(bytes.clone()).map_err(|e| Error::Load(e.message(&name)))?;
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
            printed,
            demos: Demos::new(),
        })
    }

    /// The demo file `file` in the game directory, `q_snprintf (name, sizeof(name), "%s/%s",
    /// com_gamedir, Cmd_Argv(1))` (CLIENT-10.3).
    pub(crate) fn demo_path(&self, file: &str) -> PathBuf {
        let mut path = self.game_dir().into_os_string();
        path.push("/");
        path.push(file);
        PathBuf::from(path)
    }

    /// `<name>.sav` in the game directory, `COM_DefaultExtension (name, ".sav")`.
    fn save_path(&self, name: &str) -> PathBuf {
        let mut file = name.to_owned();
        if Path::new(name).extension().is_none() {
            file.push_str(".sav");
        }
        // `sprintf ("%s/%s", com_gamedir, name)`: the name stays under the game directory, even
        // one that is absolute on its own.
        let mut path = self.game_dir().into_os_string();
        path.push("/");
        path.push(file);
        PathBuf::from(path)
    }

    /// `M_ScanSaves`' files (MENU-6.1): the text of `s0.sav` to `s11.sav` in the game directory,
    /// `None` for each that does not open.
    pub(crate) fn saves(&self) -> [Option<Vec<u8>>; MENU_SAVEGAMES] {
        let dir = self.game_dir();
        core::array::from_fn(|n| std::fs::read(dir.join(format!("s{n}.sav"))).ok())
    }
}

/// What a load `error` stopped by the QuakeC says: its message, then the function it stopped in
/// and, with the `.lno`'s `lines`, its file and line (BRIDGE-3.4), as a failed frame's
/// `qc/error` names them, so the IDE can say where a spawn function failed.
pub(crate) fn load_failure(progs: &Progs<'_>, lines: &[i32], error: &WorldError) -> String {
    let run = match error {
        WorldError::Spawn(LoadError::Run(run)) | WorldError::Frame(run) => run,
        WorldError::Spawn(LoadError::Spawn(_)) | WorldError::Progs(_) => return error.to_string(),
    };
    let Some(inner) = run.trace.iter().find(|entry| entry.function > 0) else {
        return error.to_string();
    };
    let place = locate(progs, lines, inner.function, inner.statement);
    match place.source {
        Some((file, line)) if line > 0 && !file.is_empty() => {
            format!("{error}, in {} at {file}:{line}", place.function)
        }
        Some(_) | None => format!("{error}, in {}", place.function),
    }
}
