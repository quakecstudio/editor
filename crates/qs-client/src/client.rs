// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_main.c, CL_ClearState, CL_AllocDlight, CL_DecayLights, CL_LerpPoint, CL_RelinkEntities, CL_ReadFromServer
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_parse.c, CL_EntityNum, CL_ParseStartSoundPacket, CL_ParseServerInfo, CL_ParseUpdate, CL_ParseBaseline, CL_ParseClientdata, CL_NewTranslation, CL_ParseStatic, CL_ParseStaticSound, CL_ParseServerMessage
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_tent.c, CL_InitTEnts, CL_ParseBeam, CL_ParseTEnt, CL_NewTempEntity, CL_UpdateTEnts
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_parse.c, CL_ParseUpdate
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/r_alias.c, R_SetupAliasFrame, R_SetupEntityTransform

//! The client's state and frame (`docs/spec/engine/client.md`, CLIENT-1 to CLIENT-7): the
//! server's messages read into the entity slots, the client data and the sounds; the
//! entities relinked and interpolated each frame; the dynamic lights, temporary entities
//! and beams; and the scene handed to the renderer.
//!
//! Implements CLIENT-1 to CLIENT-7, with PART and VIEW through their modules.

use qc_builtins::math::{anglemod, makevectors, vector_normalize};
use qc_vm::convert::{double_to_float, double_to_int, int_to_float};

use crate::entity::{Baseline, Entity};
use crate::hud::{CenterPrint, HudInput, HudItem, Placed, intermission_overlay, status_bar};
use crate::message::Reader;
use crate::models::{FrameInfo, ModelInfo, ModelKind, Models, Random, flags};
use crate::particles::{PART_GRAVITY, Particles};
use crate::scene::{DynamicLight, PoseBlend, Scene, SceneEntity, SceneParticle};
use crate::view::{Contents, Refdef, ViewInput, ViewSettings, ViewState};

/// `PROTOCOL_VERSION` (CLIENT-1.1).
pub const CLIENT_PROTOCOL_VERSION: i32 = 15;
/// `MAX_SCOREBOARD` (CLIENT-1.1).
pub const CLIENT_MAX_SCOREBOARD: usize = 16;
/// `MAX_MODELS` (CLIENT-1.1).
pub const CLIENT_MAX_MODELS: usize = 256;
/// `MAX_SOUNDS` (CLIENT-1.1).
pub const CLIENT_MAX_SOUNDS: usize = 256;
/// `MAX_STATIC_ENTITIES` (CLIENT-1.3).
pub const CLIENT_MAX_STATIC_ENTITIES: usize = 128;
/// `MAX_VISEDICTS` (CLIENT-5.6).
pub const CLIENT_MAX_VISEDICTS: usize = 256;
/// `SIGNONS` (CLIENT-1.5).
pub const CLIENT_SIGNONS: i32 = 4;
/// `MAX_LIGHTSTYLES` (CLIENT-1.4).
pub const CLIENT_MAX_LIGHTSTYLES: usize = 64;
/// `MAX_EDICTS` (CLIENT-1.2).
pub const CLIENT_MAX_EDICTS: usize = 600;
/// `MAX_DLIGHTS` (CLIENT-5.7).
pub const CLIENT_MAX_DLIGHTS: usize = 32;
/// `CL_LerpPoint`'s dropped-packet gap (CLIENT-5.2).
pub const CLIENT_LERP_MAX_GAP: f32 = 0.1;
/// A move larger than this is a teleport (CLIENT-5.4).
pub const CLIENT_TELEPORT_DISTANCE: f32 = 100.0;
/// `bobjrotate`'s degrees per second (CLIENT-5.3).
pub const CLIENT_ROTATE_SPEED: f32 = 100.0;
/// `MAX_BEAMS` (CLIENT-6.2).
pub const CLIENT_MAX_BEAMS: usize = 24;
/// `MAX_TEMP_ENTITIES` (CLIENT-6.4).
pub const CLIENT_MAX_TEMP_ENTITIES: usize = 64;
/// How long a beam lives (CLIENT-6.2).
pub const CLIENT_BEAM_TIME: f32 = 0.2;
/// The units between a beam's segments (CLIENT-6.4).
pub const CLIENT_BEAM_STEP: f32 = 30.0;
/// `DEFAULT_VIEWHEIGHT` (CLIENT-3.1).
pub const CLIENT_DEFAULT_VIEWHEIGHT: f32 = 22.0;
/// `MAX_CL_STATS` (CLIENT-1.4).
pub const CLIENT_MAX_STATS: usize = 32;
/// `sb_lines` at the default view size of 100: the bar and the inventory (HUD-1.1).
pub const HUD_DEFAULT_LINES: i32 = 48;
/// The gap after which an update restarts the animation lerp (CLIENT-7.1).
pub const CLIENT_LERP_RESET_GAP: f32 = 0.2;
/// A single pose's lerp time (CLIENT-7.3).
pub const CLIENT_LERP_SINGLE_POSE: f32 = 0.1;
/// A step's move lerp time (CLIENT-7.4).
pub const CLIENT_LERP_MOVE_TIME: f32 = 0.1;

/// `STAT_HEALTH` to `STAT_MONSTERS`: the indices into the stats (CLIENT-3).
pub mod stat {
    /// `STAT_HEALTH`.
    pub const HEALTH: usize = 0;
    /// `STAT_FRAGS`.
    pub const FRAGS: usize = 1;
    /// `STAT_WEAPON`.
    pub const WEAPON: usize = 2;
    /// `STAT_AMMO`.
    pub const AMMO: usize = 3;
    /// `STAT_ARMOR`.
    pub const ARMOR: usize = 4;
    /// `STAT_WEAPONFRAME`.
    pub const WEAPONFRAME: usize = 5;
    /// `STAT_SHELLS`.
    pub const SHELLS: usize = 6;
    /// `STAT_NAILS`.
    pub const NAILS: usize = 7;
    /// `STAT_ROCKETS`.
    pub const ROCKETS: usize = 8;
    /// `STAT_CELLS`.
    pub const CELLS: usize = 9;
    /// `STAT_ACTIVEWEAPON`.
    pub const ACTIVEWEAPON: usize = 10;
    /// `STAT_TOTALSECRETS`.
    pub const TOTALSECRETS: usize = 11;
    /// `STAT_TOTALMONSTERS`.
    pub const TOTALMONSTERS: usize = 12;
    /// `STAT_SECRETS`.
    pub const SECRETS: usize = 13;
    /// `STAT_MONSTERS`.
    pub const MONSTERS: usize = 14;
}

const SVC_NOP: i32 = 1;
const SVC_DISCONNECT: i32 = 2;
const SVC_UPDATESTAT: i32 = 3;
const SVC_VERSION: i32 = 4;
const SVC_SETVIEW: i32 = 5;
const SVC_SOUND: i32 = 6;
const SVC_TIME: i32 = 7;
const SVC_PRINT: i32 = 8;
const SVC_STUFFTEXT: i32 = 9;
const SVC_SETANGLE: i32 = 10;
const SVC_SERVERINFO: i32 = 11;
const SVC_LIGHTSTYLE: i32 = 12;
const SVC_UPDATENAME: i32 = 13;
const SVC_UPDATEFRAGS: i32 = 14;
const SVC_CLIENTDATA: i32 = 15;
const SVC_STOPSOUND: i32 = 16;
const SVC_UPDATECOLORS: i32 = 17;
const SVC_PARTICLE: i32 = 18;
const SVC_DAMAGE: i32 = 19;
const SVC_SPAWNSTATIC: i32 = 20;
const SVC_SPAWNBASELINE: i32 = 22;
const SVC_TEMP_ENTITY: i32 = 23;
const SVC_SETPAUSE: i32 = 24;
const SVC_SIGNONNUM: i32 = 25;
const SVC_CENTERPRINT: i32 = 26;
const SVC_KILLEDMONSTER: i32 = 27;
const SVC_FOUNDSECRET: i32 = 28;
const SVC_SPAWNSTATICSOUND: i32 = 29;
const SVC_INTERMISSION: i32 = 30;
const SVC_FINALE: i32 = 31;
const SVC_CDTRACK: i32 = 32;
const SVC_SELLSCREEN: i32 = 33;
const SVC_CUTSCENE: i32 = 34;

const U_MOREBITS: i32 = 1 << 0;
const U_ORIGIN1: i32 = 1 << 1;
const U_ORIGIN2: i32 = 1 << 2;
const U_ORIGIN3: i32 = 1 << 3;
const U_ANGLE2: i32 = 1 << 4;
const U_STEP: i32 = 1 << 5;
const U_FRAME: i32 = 1 << 6;
const U_ANGLE1: i32 = 1 << 8;
const U_ANGLE3: i32 = 1 << 9;
const U_MODEL: i32 = 1 << 10;
const U_COLORMAP: i32 = 1 << 11;
const U_SKIN: i32 = 1 << 12;
const U_EFFECTS: i32 = 1 << 13;
const U_LONGENTITY: i32 = 1 << 14;

const SU_VIEWHEIGHT: i32 = 1 << 0;
const SU_IDEALPITCH: i32 = 1 << 1;
const SU_PUNCH1: i32 = 1 << 2;
const SU_VELOCITY1: i32 = 1 << 5;
const SU_ONGROUND: i32 = 1 << 10;
const SU_INWATER: i32 = 1 << 11;
const SU_WEAPONFRAME: i32 = 1 << 12;
const SU_ARMOR: i32 = 1 << 13;
const SU_WEAPON: i32 = 1 << 14;

const SND_VOLUME: i32 = 1;
const SND_ATTENUATION: i32 = 2;
const SND_PACKET_VOLUME: i32 = 255;
const SND_PACKET_ATTENUATION: f32 = 1.0;

const EF_BRIGHTFIELD: i32 = 1;
const EF_MUZZLEFLASH: i32 = 2;
const EF_BRIGHTLIGHT: i32 = 4;
const EF_DIMLIGHT: i32 = 8;

const TE_SPIKE: i32 = 0;
const TE_SUPERSPIKE: i32 = 1;
const TE_GUNSHOT: i32 = 2;
const TE_EXPLOSION: i32 = 3;
const TE_TAREXPLOSION: i32 = 4;
const TE_LIGHTNING1: i32 = 5;
const TE_LIGHTNING2: i32 = 6;
const TE_WIZSPIKE: i32 = 7;
const TE_KNIGHTSPIKE: i32 = 8;
const TE_LIGHTNING3: i32 = 9;
const TE_LAVASPLASH: i32 = 10;
const TE_TELEPORT: i32 = 11;
const TE_EXPLOSION2: i32 = 12;
const TE_BEAM: i32 = 13;

/// The sounds the temporary entities play, precached by the client (CLIENT-6.1).
pub const CLIENT_TENT_SOUNDS: [&str; 7] = [
    "wizard/hit.wav",
    "hknight/hit.wav",
    "weapons/tink1.wav",
    "weapons/ric1.wav",
    "weapons/ric2.wav",
    "weapons/ric3.wav",
    "weapons/r_exp3.wav",
];

/// The beam models (CLIENT-6.2).
pub const CLIENT_BEAM_MODELS: [&str; 4] = [
    "progs/bolt.mdl",
    "progs/bolt2.mdl",
    "progs/bolt3.mdl",
    "progs/beam.mdl",
];

/// Why a message could not be read: id's `Host_Error` and `Sys_Error` texts (CLIENT-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// `CL_ParseServerMessage: Bad server message`: a read ran past the end.
    BadMessage,
    /// `CL_ParseServerMessage: Illegible server message`: an unknown command.
    Illegible(i32),
    /// `Server returned version <n>, not 15`.
    Version(i32),
    /// `Bad maxclients (<n>) from server`.
    MaxClients(i32),
    /// `Server sent too many model precaches` or sound precaches.
    TooManyPrecaches,
    /// `Model <name> not found`.
    ModelNotFound(String),
    /// `CL_EntityNum: <n> is an invalid number`.
    EntityNumber(i32),
    /// `CL_ParseModel: bad modnum`.
    BadModel(i32),
    /// `i >= cl.maxclients`: a colormap past the client slots.
    BadColormap(i32),
    /// `Too many static entities`.
    TooManyStatics,
    /// `CL_ParseStartSoundPacket: ent = <n>`.
    SoundEntity(i32),
    /// `svc_lightstyle > MAX_LIGHTSTYLES`.
    LightStyle(i32),
    /// `svc_updatename > MAX_SCOREBOARD` and its siblings.
    Scoreboard(i32),
    /// `Received signon <n> when at <m>`.
    Signon(i32, i32),
    /// `svc_updatestat: <n> is invalid`.
    Stat(i32),
    /// `CL_ParseTEnt: bad type`.
    TempEntity(i32),
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadMessage => write!(f, "CL_ParseServerMessage: Bad server message"),
            Self::Illegible(cmd) => {
                write!(f, "CL_ParseServerMessage: Illegible server message ({cmd})")
            }
            Self::Version(v) => write!(f, "Server returned version {v}, not 15"),
            Self::MaxClients(n) => write!(f, "Bad maxclients ({n}) from server"),
            Self::TooManyPrecaches => write!(f, "Server sent too many precaches"),
            Self::ModelNotFound(name) => write!(f, "Model {name} not found"),
            Self::EntityNumber(n) => write!(f, "CL_EntityNum: {n} is an invalid number"),
            Self::BadModel(n) => write!(f, "CL_ParseModel: bad modnum ({n})"),
            Self::BadColormap(n) => write!(f, "i >= cl.maxclients ({n})"),
            Self::TooManyStatics => write!(f, "Too many static entities"),
            Self::SoundEntity(n) => write!(f, "CL_ParseStartSoundPacket: ent = {n}"),
            Self::LightStyle(n) => write!(f, "svc_lightstyle > MAX_LIGHTSTYLES ({n})"),
            Self::Scoreboard(n) => write!(f, "CL_ParseServerMessage: slot {n} > MAX_SCOREBOARD"),
            Self::Signon(n, at) => write!(f, "Received signon {n} when at {at}"),
            Self::Stat(n) => write!(f, "svc_updatestat: {n} is invalid"),
            Self::TempEntity(n) => write!(f, "CL_ParseTEnt: bad type ({n})"),
        }
    }
}

impl core::error::Error for ParseError {}

/// A sound the client wants started, as `S_StartSound` is called (CLIENT-4.1, CLIENT-6.1).
#[derive(Clone, Debug, PartialEq)]
pub struct SoundStart {
    /// The entity, -1 for a temporary entity's sound.
    pub entity: i32,
    /// The entity channel.
    pub channel: i32,
    /// The sound: a precache index, or a name the client precaches itself (CLIENT-6.1).
    pub sound: SoundRef,
    /// Where it is.
    pub origin: [f32; 3],
    /// Its volume, 0 to 1.
    pub volume: f32,
    /// Its attenuation.
    pub attenuation: f32,
}

/// Which sound a [`SoundStart`] names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundRef {
    /// An entry of the server's sound precache list.
    Index(usize),
    /// A name the client precaches itself, one of [`CLIENT_TENT_SOUNDS`].
    Name(&'static str),
}

/// What a message told the client that someone else acts on.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `svc_print`: text for the console.
    Print(Vec<u8>),
    /// `svc_stufftext`: text for the command buffer.
    StuffText(Vec<u8>),
    /// `svc_centerprint`, and the finale's and cutscene's text (HUD-3).
    CenterPrint(Vec<u8>),
    /// A sound to start (CLIENT-4.1, CLIENT-6.1).
    Sound(SoundStart),
    /// `svc_stopsound`: the entity and channel (SND-2.6).
    StopSound {
        /// The entity.
        entity: i32,
        /// The channel.
        channel: i32,
    },
    /// `svc_spawnstaticsound` (CLIENT-4.2): the origin, the sound index, the volume byte
    /// and the attenuation byte, for SND-4.1.
    StaticSound {
        /// Where it is.
        origin: [f32; 3],
        /// The sound precache index.
        sound: usize,
        /// The volume byte.
        volume: i32,
        /// The attenuation byte.
        attenuation: i32,
    },
    /// `svc_serverinfo` read: a new map, with the model and sound names in precache order
    /// from index 1 (CLIENT-1.1).
    NewMap {
        /// The level name the server printed.
        level: Vec<u8>,
        /// The model names.
        models: Vec<String>,
        /// The sound names.
        sounds: Vec<String>,
    },
    /// `svc_signonnum`: the stage the server reached; the host answers it (CLIENT-1.5).
    Signon(i32),
    /// `svc_disconnect`.
    Disconnect,
    /// `svc_cdtrack`: the track and the loop track (RRE-3).
    CdTrack {
        /// The track.
        track: i32,
        /// The loop track.
        loop_track: i32,
    },
    /// `svc_setpause`.
    Pause(bool),
    /// `svc_sellscreen`.
    SellScreen,
}

/// What is drawn over the view this frame (HUD-1 to HUD-3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hud {
    /// The status bar, the inventory and the frag list, or the scoreboard (HUD-1, HUD-2.1).
    pub bar: Vec<Placed>,
    /// The intermission, deathmatch or finale overlay (HUD-2.2, HUD-2.3).
    pub overlay: Vec<Placed>,
    /// The centre print's characters, on the screen (HUD-3).
    pub center: Vec<HudItem>,
}

/// One scoreboard entry, `scoreboard_t` (CLIENT-1.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Score {
    /// The player's name.
    pub name: Vec<u8>,
    /// The frags.
    pub frags: i32,
    /// The colours byte: shirt high, trousers low (CLIENT-1.6).
    pub colors: i32,
}

/// A dynamic light, `dlight_t` (CLIENT-5.7).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct DLight {
    origin: [f32; 3],
    radius: f32,
    die: f32,
    decay: f32,
    minlight: f32,
    key: i32,
}

/// A beam, `beam_t` (CLIENT-6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Beam {
    entity: i32,
    /// The beam model as a [`CLIENT_BEAM_MODELS`] index plus 1, 0 for none.
    model: usize,
    endtime: f32,
    start: [f32; 3],
    end: [f32; 3],
}

/// The game type (CLIENT-3.2): standard Quake, or Rogue and Hipnotic, which send the
/// active weapon as a bit index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Game {
    /// `standard_quake`.
    #[default]
    Standard,
    /// `-rogue` or `-hipnotic`.
    MissionPack,
}

/// Whether the client interpolates animation and stepping moves (CLIENT-7): the FitzQuake
/// lineage's `r_lerpmodels` and `r_lerpmove`, on by default under ADR-0012.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LerpSettings {
    /// `r_lerpmodels`.
    pub models: bool,
    /// `r_lerpmove`.
    pub moves: bool,
}

impl Default for LerpSettings {
    fn default() -> Self {
        Self {
            models: true,
            moves: true,
        }
    }
}

/// The client's state, `client_state_t` and the arrays beside it (CLIENT-1 to CLIENT-7).
#[derive(Debug)]
pub struct Client {
    game: Game,
    lerp: LerpSettings,
    /// `cl.time` and `cl.oldtime` (CLIENT-5.1).
    time: f32,
    oldtime: f32,
    /// `cl.mtime` (CLIENT-1.4).
    mtime: [f32; 2],
    /// `cl.mvelocity` and `cl.velocity` (CLIENT-3.1, CLIENT-5.3).
    mvelocity: [[f32; 3]; 2],
    velocity: [f32; 3],
    /// `cl.viewangles`.
    viewangles: [f32; 3],
    viewheight: f32,
    idealpitch: f32,
    punchangle: [f32; 3],
    items: i32,
    item_gettime: [f32; 32],
    onground: bool,
    inwater: bool,
    stats: [i32; CLIENT_MAX_STATS],
    maxclients: usize,
    gametype: i32,
    levelname: Vec<u8>,
    models: Vec<Option<ModelInfo>>,
    model_names: Vec<String>,
    sound_count: usize,
    viewentity: usize,
    lightstyles: Vec<Vec<u8>>,
    scores: Vec<Score>,
    entities: Vec<Entity>,
    statics: Vec<Entity>,
    dlights: [DLight; CLIENT_MAX_DLIGHTS],
    /// Where an impossible light index would land, never read.
    spare_light: DLight,
    beams: [Beam; CLIENT_MAX_BEAMS],
    particles: Particles,
    view: ViewState,
    view_settings: ViewSettings,
    signon: i32,
    intermission: i32,
    completed_time: f32,
    paused: bool,
    cdtrack: (i32, i32),
    /// The frame's visible entities and the temporary entities (CLIENT-5.6, CLIENT-6.4).
    visedicts: Vec<SceneEntity>,
    /// The weapon's lerp state (CLIENT-7.3).
    weapon: Entity,
    /// The contents the eye is in, which the host tells from the world (VIEW-4.3).
    contents: Contents,
    /// `sv_gravity` for the particles (PART-3.1).
    gravity: f32,
    /// The command's `forwardmove`, for the pitch drift (VIEW-2.2).
    forwardmove: f32,
    /// The centre print (HUD-3).
    center: CenterPrint,
    /// This frame's centre print characters.
    center_items: Vec<HudItem>,
    /// The screen's size in pixels, for the centre print and the frag list.
    screen: (i32, i32),
    /// `+showscores` (HUD-1.1).
    showscores: bool,
    /// `sb_lines`: how many rows of the bar the view size leaves (HUD-1.1).
    bar_lines: i32,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(Game::Standard)
    }
}

impl Client {
    /// A client with nothing connected, `CL_ClearState` (CLIENT-1.1).
    #[must_use]
    pub fn new(game: Game) -> Self {
        Self {
            game,
            lerp: LerpSettings::default(),
            time: 0.0,
            oldtime: 0.0,
            mtime: [0.0; 2],
            mvelocity: [[0.0; 3]; 2],
            velocity: [0.0; 3],
            viewangles: [0.0; 3],
            viewheight: CLIENT_DEFAULT_VIEWHEIGHT,
            idealpitch: 0.0,
            punchangle: [0.0; 3],
            items: 0,
            item_gettime: [0.0; 32],
            onground: false,
            inwater: false,
            stats: [0; CLIENT_MAX_STATS],
            maxclients: 0,
            gametype: 0,
            levelname: Vec::new(),
            models: Vec::new(),
            model_names: vec![String::new()],
            sound_count: 0,
            viewentity: 0,
            lightstyles: vec![Vec::new(); CLIENT_MAX_LIGHTSTYLES],
            scores: Vec::new(),
            entities: Vec::new(),
            statics: Vec::new(),
            dlights: [DLight::default(); CLIENT_MAX_DLIGHTS],
            spare_light: DLight::default(),
            beams: [Beam::default(); CLIENT_MAX_BEAMS],
            particles: Particles::default(),
            view: ViewState::default(),
            view_settings: ViewSettings::default(),
            signon: 0,
            intermission: 0,
            completed_time: 0.0,
            paused: false,
            cdtrack: (0, 0),
            visedicts: Vec::new(),
            weapon: Entity::new(),
            contents: Contents::Empty,
            gravity: PART_GRAVITY,
            forwardmove: 0.0,
            center: CenterPrint::default(),
            center_items: Vec::new(),
            screen: (320, 200),
            showscores: false,
            bar_lines: HUD_DEFAULT_LINES,
        }
    }

    /// `CL_ClearState`: every slot, light, style, temporary entity and beam zeroed
    /// (CLIENT-1.1); the particles cleared as a new map does (PART-1.1).
    fn clear_state(&mut self) {
        let game = self.game;
        let lerp = self.lerp;
        let view_settings = self.view_settings;
        let gravity = self.gravity;
        let screen = self.screen;
        let bar_lines = self.bar_lines;
        *self = Self::new(game);
        self.lerp = lerp;
        self.view_settings = view_settings;
        self.gravity = gravity;
        self.screen = screen;
        self.bar_lines = bar_lines;
        self.particles.clear();
    }

    /// Changes the interpolation settings.
    pub const fn set_lerp(&mut self, lerp: LerpSettings) {
        self.lerp = lerp;
    }

    /// Changes the view settings.
    pub const fn set_view_settings(&mut self, settings: ViewSettings) {
        self.view_settings = settings;
    }

    /// The view settings.
    #[must_use]
    pub const fn view_settings(&self) -> &ViewSettings {
        &self.view_settings
    }

    /// `sv_gravity`, which the particles fall by (PART-3.1).
    pub const fn set_gravity(&mut self, gravity: f32) {
        self.gravity = gravity;
    }

    /// The contents of the leaf the eye is in, for the contents shift (VIEW-4.3); the host
    /// tells it from the world each frame.
    pub const fn set_contents(&mut self, contents: Contents) {
        self.contents = contents;
    }

    /// The command's forward move, for the pitch drift (VIEW-2.2).
    pub const fn set_forwardmove(&mut self, forwardmove: f32) {
        self.forwardmove = forwardmove;
    }

    /// The screen's size in pixels, which places the centre print and the frag list.
    pub const fn set_screen(&mut self, width: i32, height: i32) {
        self.screen = (width, height);
    }

    /// `+showscores` and `-showscores` (HUD-1.1).
    pub const fn set_showscores(&mut self, shown: bool) {
        self.showscores = shown;
    }

    /// `sb_lines`: 0 at a view size of 120, 24 at 110, else 48 (HUD-1.1).
    pub const fn set_bar_lines(&mut self, lines: i32) {
        self.bar_lines = lines;
    }

    /// The status bar, the overlays and the centre print for this frame (HUD-1 to HUD-3).
    #[must_use]
    pub fn hud(&self) -> Hud {
        let input = HudInput {
            time: self.time,
            items: self.items,
            item_gettime: &self.item_gettime,
            stats: &self.stats,
            faceanimtime: self.view.faceanimtime,
            maxclients: self.maxclients,
            gametype: self.gametype,
            scores: &self.scores,
            viewentity: self.viewentity,
            levelname: &self.levelname,
            intermission: self.intermission,
            completed_time: self.completed_time,
            showscores: self.showscores,
            lines: self.bar_lines,
        };
        let (bar, overlay) = if self.intermission == 0 {
            (status_bar(&input, self.screen.1), Vec::new())
        } else {
            (Vec::new(), intermission_overlay(&input))
        };
        Hud {
            bar,
            overlay,
            center: self.center_items.clone(),
        }
    }

    /// `cl.viewangles`.
    #[must_use]
    pub const fn viewangles(&self) -> [f32; 3] {
        self.viewangles
    }

    /// Sets `cl.viewangles`, as the host's input does.
    pub const fn set_viewangles(&mut self, angles: [f32; 3]) {
        self.viewangles = angles;
    }

    /// `V_StartPitchDrift` (VIEW-2.1), the `centerview` command.
    pub fn start_pitch_drift(&mut self) {
        self.view.start_pitch_drift(self.time);
    }

    /// `V_StopPitchDrift` (VIEW-2.1): the player looked for themselves.
    pub fn stop_pitch_drift(&mut self) {
        self.view.stop_pitch_drift(self.time);
    }

    /// `cl.time`.
    #[must_use]
    pub const fn time(&self) -> f32 {
        self.time
    }

    /// `cl.mtime[0]`, the time of the latest server message.
    #[must_use]
    pub const fn message_time(&self) -> f32 {
        self.mtime[0]
    }

    /// `cls.signon`, 0 to `CLIENT_SIGNONS` (CLIENT-1.5).
    #[must_use]
    pub const fn signon(&self) -> i32 {
        self.signon
    }

    /// `cl.stats`.
    #[must_use]
    pub const fn stats(&self) -> &[i32; CLIENT_MAX_STATS] {
        &self.stats
    }

    /// `cl.items`.
    #[must_use]
    pub const fn items(&self) -> i32 {
        self.items
    }

    /// `cl.item_gettime`: when each item bit was last set (CLIENT-3.2).
    #[must_use]
    pub const fn item_gettime(&self) -> &[f32; 32] {
        &self.item_gettime
    }

    /// `cl.maxclients`.
    #[must_use]
    pub const fn maxclients(&self) -> usize {
        self.maxclients
    }

    /// `cl.gametype`.
    #[must_use]
    pub const fn gametype(&self) -> i32 {
        self.gametype
    }

    /// The level name (CLIENT-1.1).
    #[must_use]
    pub fn levelname(&self) -> &[u8] {
        &self.levelname
    }

    /// The scoreboard.
    #[must_use]
    pub fn scores(&self) -> &[Score] {
        &self.scores
    }

    /// `cl.viewentity`.
    #[must_use]
    pub const fn viewentity(&self) -> usize {
        self.viewentity
    }

    /// `cl.intermission`: 0, or 1 to 3 (CLIENT-1.4).
    #[must_use]
    pub const fn intermission(&self) -> i32 {
        self.intermission
    }

    /// `cl.completed_time`.
    #[must_use]
    pub const fn completed_time(&self) -> f32 {
        self.completed_time
    }

    /// `cl.paused`.
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// `cl.onground`.
    #[must_use]
    pub const fn onground(&self) -> bool {
        self.onground
    }

    /// `cl.inwater`.
    #[must_use]
    pub const fn inwater(&self) -> bool {
        self.inwater
    }

    /// `cl.idealpitch`.
    #[must_use]
    pub const fn idealpitch(&self) -> f32 {
        self.idealpitch
    }

    /// The entity slots, `cl_entities` up to `num_entities`.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The static entities (CLIENT-1.3).
    #[must_use]
    pub fn statics(&self) -> &[Entity] {
        &self.statics
    }

    /// The models the server named, by precache index; `None` for index 0 and past the list.
    #[must_use]
    pub fn model(&self, index: usize) -> Option<&ModelInfo> {
        self.models.get(index).and_then(Option::as_ref)
    }

    /// The light styles' strings (CLIENT-1.4).
    #[must_use]
    pub fn lightstyles(&self) -> &[Vec<u8>] {
        &self.lightstyles
    }

    /// The view state (VIEW).
    #[must_use]
    pub const fn view(&self) -> &ViewState {
        &self.view
    }

    /// The particles (PART).
    #[must_use]
    pub const fn particles(&self) -> &Particles {
        &self.particles
    }

    /// `CL_EntityNum (n)` (CLIENT-1.2).
    fn entity_num(&mut self, num: i32) -> Result<usize, ParseError> {
        let n = usize::try_from(num)
            .ok()
            .filter(|&n| n < CLIENT_MAX_EDICTS)
            .ok_or(ParseError::EntityNumber(num))?;
        while self.entities.len() <= n {
            self.entities.push(Entity::new());
        }
        Ok(n)
    }

    // ---- the frame (CLIENT-5.1) ----

    /// `CL_ReadFromServer` and the frame's work after it: the clock advances by `frametime`,
    /// each of `packets` is read (CLIENT-1.4), the entities are relinked (CLIENT-5), the
    /// temporary entities built (CLIENT-6.4), the lights decayed (CLIENT-5.7), the
    /// particles moved (PART-3.1) and the view built (VIEW-3, VIEW-4). The events the
    /// messages raised are returned for the host.
    ///
    /// # Errors
    ///
    /// [`ParseError`] when a message cannot be read; the client's state is then as far as
    /// the message got, as id's `Host_Error` leaves it.
    pub fn frame(
        &mut self,
        frametime: f32,
        packets: &[Vec<u8>],
        models: &mut dyn Models,
        rand: &mut dyn Random,
    ) -> Result<Vec<Event>, ParseError> {
        self.oldtime = self.time;
        self.time += frametime;
        let mut events = Vec::new();
        for packet in packets {
            self.read_message(packet, models, rand, &mut events)?;
        }
        self.relink_entities(rand);
        self.update_temp_entities(rand);
        self.decay_lights();
        let frametime = self.time - self.oldtime;
        self.particles.update(self.time, frametime, self.gravity);
        self.update_palette(frametime);
        for event in &events {
            if let Event::CenterPrint(text) = event {
                self.center.set(text, self.time);
            }
        }
        let (width, height) = self.screen;
        self.center_items =
            self.center
                .draw(frametime, self.time, self.intermission != 0, width, height);
        Ok(events)
    }

    /// The scene after [`Client::frame`]: what the renderer draws.
    #[must_use]
    pub fn scene(&mut self) -> Scene {
        let refdef = self.render_view();
        let weapon_poses = if refdef.weapon.model == 0 {
            None
        } else {
            self.weapon.model = refdef.weapon.model;
            self.weapon.frame = refdef.weapon.frame;
            self.alias_poses_for_weapon()
        };
        let statics = self
            .statics
            .iter()
            .map(|e| SceneEntity {
                model: e.model,
                origin: e.origin,
                angles: e.angles,
                frame: e.frame,
                colormap: 0,
                skin: e.skin,
                syncbase: 0.0,
                poses: None,
            })
            .collect();
        Scene {
            time: self.time,
            entities: self.visedicts.clone(),
            statics,
            view: refdef,
            weapon_poses,
            blend: self.view.blend(self.view_settings.cshiftpercent),
            lights: self
                .dlights
                .iter()
                .filter(|l| l.die >= self.time && l.radius != 0.0)
                .map(|l| DynamicLight {
                    origin: l.origin,
                    radius: l.radius,
                    minlight: l.minlight,
                })
                .collect(),
            particles: self
                .particles
                .active()
                .iter()
                .map(|p| SceneParticle {
                    origin: p.origin,
                    color: u8::try_from(p.color & 255).unwrap_or(0),
                })
                .collect(),
            lightstyles: self.lightstyles.clone(),
        }
    }

    /// `V_RenderView` (VIEW-3.9): nothing before the map's entities arrive.
    fn render_view(&mut self) -> Refdef {
        let Some(entity) = self.entities.get(self.viewentity).copied() else {
            return Refdef::default();
        };
        self.view.drift_pitch(
            self.time - self.oldtime,
            self.onground,
            self.forwardmove,
            self.idealpitch,
            &mut self.viewangles,
        );
        let input = ViewInput {
            time: self.time,
            frametime: self.time - self.oldtime,
            origin: entity.origin,
            entity_angles: entity.angles,
            viewangles: self.viewangles,
            velocity: self.velocity,
            viewheight: self.viewheight,
            punchangle: self.punchangle,
            onground: self.onground,
            health: self.stats[stat::HEALTH],
            intermission: self.intermission != 0,
            weapon: (
                usize::try_from(self.stats[stat::WEAPON]).unwrap_or(0),
                self.stats[stat::WEAPONFRAME],
            ),
        };
        let refdef = self.view.render(&input, &self.view_settings);
        if let Some(entity) = self.entities.get_mut(self.viewentity) {
            entity.angles = refdef.entity_angles;
        }
        refdef
    }

    /// `V_UpdatePalette`'s state each frame (VIEW-4.3 to VIEW-4.5).
    fn update_palette(&mut self, frametime: f32) {
        self.view.set_contents(self.contents);
        self.view.powerup_shift(self.items);
        self.view.fade(frametime);
    }

    // ---- reading a message (CLIENT-1.4) ----

    /// `CL_ParseServerMessage`: one packet's commands (CLIENT-1.4).
    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per server command, in protocol.h's order, reads best whole"
    )]
    fn read_message(
        &mut self,
        bytes: &[u8],
        models: &mut dyn Models,
        rand: &mut dyn Random,
        events: &mut Vec<Event>,
    ) -> Result<(), ParseError> {
        self.onground = false;
        let mut r = Reader::new(bytes);
        loop {
            if r.bad() {
                return Err(ParseError::BadMessage);
            }
            if r.done() {
                return Ok(());
            }
            let cmd = r.byte();
            if cmd & 128 != 0 {
                self.parse_update(&mut r, cmd & 127, rand)?;
                continue;
            }
            match cmd {
                SVC_NOP => {}
                SVC_TIME => {
                    self.mtime[1] = self.mtime[0];
                    self.mtime[0] = r.float();
                }
                SVC_CLIENTDATA => {
                    let bits = r.short();
                    self.parse_clientdata(&mut r, bits);
                }
                SVC_VERSION => {
                    let v = r.long();
                    if v != CLIENT_PROTOCOL_VERSION {
                        return Err(ParseError::Version(v));
                    }
                }
                SVC_DISCONNECT => {
                    events.push(Event::Disconnect);
                    return Ok(());
                }
                SVC_PRINT => events.push(Event::Print(r.string())),
                SVC_CENTERPRINT => events.push(Event::CenterPrint(r.string())),
                SVC_STUFFTEXT => events.push(Event::StuffText(r.string())),
                SVC_DAMAGE => {
                    let armor = r.byte();
                    let blood = r.byte();
                    let from = [r.coord(), r.coord(), r.coord()];
                    let entity = self
                        .entities
                        .get(self.viewentity)
                        .copied()
                        .unwrap_or_default();
                    self.view
                        .damage(self.time, armor, blood, from, entity.origin, entity.angles);
                }
                SVC_SERVERINFO => self.parse_serverinfo(&mut r, models, events)?,
                SVC_SETANGLE => {
                    self.viewangles = [r.angle(), r.angle(), r.angle()];
                }
                SVC_SETVIEW => {
                    self.viewentity = usize::try_from(r.short()).unwrap_or(0);
                }
                SVC_LIGHTSTYLE => {
                    let i = r.byte();
                    let slot = usize::try_from(i)
                        .ok()
                        .and_then(|i| self.lightstyles.get_mut(i))
                        .ok_or(ParseError::LightStyle(i))?;
                    *slot = r.string();
                }
                SVC_SOUND => {
                    let sound = self.parse_sound(&mut r)?;
                    events.push(Event::Sound(sound));
                }
                SVC_STOPSOUND => {
                    let i = r.short();
                    events.push(Event::StopSound {
                        entity: i >> 3,
                        channel: i & 7,
                    });
                }
                SVC_UPDATENAME => {
                    let i = r.byte();
                    let name = r.string();
                    self.score_mut(i)?.name = name;
                }
                SVC_UPDATEFRAGS => {
                    let i = r.byte();
                    let frags = r.short();
                    self.score_mut(i)?.frags = frags;
                }
                SVC_UPDATECOLORS => {
                    let i = r.byte();
                    let colors = r.byte();
                    self.score_mut(i)?.colors = colors;
                }
                SVC_PARTICLE => {
                    let org = [r.coord(), r.coord(), r.coord()];
                    let dir = [r.char(), r.char(), r.char()]
                        .map(|c| double_to_float(f64::from(c) * (1.0 / 16.0)));
                    let msgcount = r.byte();
                    let color = r.byte();
                    let count = if msgcount == 255 { 1024 } else { msgcount };
                    self.particles
                        .run_effect(rand, self.time, org, dir, color, count);
                }
                SVC_SPAWNBASELINE => {
                    let i = r.short();
                    let n = self.entity_num(i)?;
                    let baseline = parse_baseline(&mut r);
                    if let Some(entity) = self.entities.get_mut(n) {
                        entity.baseline = baseline;
                    }
                }
                SVC_SPAWNSTATIC => self.parse_static(&mut r)?,
                SVC_TEMP_ENTITY => self.parse_temp_entity(&mut r, rand, events)?,
                SVC_SETPAUSE => {
                    self.paused = r.byte() != 0;
                    events.push(Event::Pause(self.paused));
                }
                SVC_SIGNONNUM => {
                    let i = r.byte();
                    if i <= self.signon {
                        return Err(ParseError::Signon(i, self.signon));
                    }
                    self.signon = i;
                    events.push(Event::Signon(i));
                }
                SVC_KILLEDMONSTER => {
                    self.stats[stat::MONSTERS] = self.stats[stat::MONSTERS].wrapping_add(1);
                }
                SVC_FOUNDSECRET => {
                    self.stats[stat::SECRETS] = self.stats[stat::SECRETS].wrapping_add(1);
                }
                SVC_UPDATESTAT => {
                    let i = r.byte();
                    let value = r.long();
                    let slot = usize::try_from(i)
                        .ok()
                        .and_then(|i| self.stats.get_mut(i))
                        .ok_or(ParseError::Stat(i))?;
                    *slot = value;
                }
                SVC_SPAWNSTATICSOUND => {
                    let origin = [r.coord(), r.coord(), r.coord()];
                    let sound = usize::try_from(r.byte()).unwrap_or(0);
                    let volume = r.byte();
                    let attenuation = r.byte();
                    events.push(Event::StaticSound {
                        origin,
                        sound,
                        volume,
                        attenuation,
                    });
                }
                SVC_CDTRACK => {
                    self.cdtrack = (r.byte(), r.byte());
                    events.push(Event::CdTrack {
                        track: self.cdtrack.0,
                        loop_track: self.cdtrack.1,
                    });
                }
                SVC_INTERMISSION => {
                    self.intermission = 1;
                    self.completed_time = self.time;
                }
                SVC_FINALE | SVC_CUTSCENE => {
                    self.intermission = if cmd == SVC_FINALE { 2 } else { 3 };
                    self.completed_time = self.time;
                    events.push(Event::CenterPrint(r.string()));
                }
                SVC_SELLSCREEN => events.push(Event::SellScreen),
                other => return Err(ParseError::Illegible(other)),
            }
        }
    }

    fn score_mut(&mut self, i: i32) -> Result<&mut Score, ParseError> {
        usize::try_from(i)
            .ok()
            .filter(|&i| i < self.maxclients)
            .and_then(|i| self.scores.get_mut(i))
            .ok_or(ParseError::Scoreboard(i))
    }

    /// `CL_ParseServerInfo` (CLIENT-1.1).
    fn parse_serverinfo(
        &mut self,
        r: &mut Reader<'_>,
        models: &mut dyn Models,
        events: &mut Vec<Event>,
    ) -> Result<(), ParseError> {
        self.clear_state();
        let version = r.long();
        if version != CLIENT_PROTOCOL_VERSION {
            return Err(ParseError::Version(version));
        }
        let maxclients = r.byte();
        self.maxclients = usize::try_from(maxclients)
            .ok()
            .filter(|&n| (1..=CLIENT_MAX_SCOREBOARD).contains(&n))
            .ok_or(ParseError::MaxClients(maxclients))?;
        self.scores = vec![Score::default(); self.maxclients];
        self.gametype = r.byte();
        self.levelname = r.string();
        let mut model_names = Vec::new();
        loop {
            let name = r.string();
            if name.is_empty() {
                break;
            }
            if model_names.len().saturating_add(1) >= CLIENT_MAX_MODELS {
                return Err(ParseError::TooManyPrecaches);
            }
            model_names.push(String::from_utf8_lossy(&name).into_owned());
        }
        let mut sound_names = Vec::new();
        loop {
            let name = r.string();
            if name.is_empty() {
                break;
            }
            if sound_names.len().saturating_add(1) >= CLIENT_MAX_SOUNDS {
                return Err(ParseError::TooManyPrecaches);
            }
            sound_names.push(String::from_utf8_lossy(&name).into_owned());
        }
        self.models = vec![None];
        self.model_names = vec![String::new()];
        self.model_names.extend(model_names.iter().cloned());
        for name in &model_names {
            let info = models
                .describe(name)
                .ok_or_else(|| ParseError::ModelNotFound(name.clone()))?;
            self.models.push(Some(info));
        }
        self.sound_count = sound_names.len().saturating_add(1);
        // The world is model 1; CL_EntityNum (0) holds it.
        self.entity_num(0)?;
        if let Some(world) = self.entities.first_mut() {
            world.model = 1;
        }
        events.push(Event::NewMap {
            level: self.levelname.clone(),
            models: model_names,
            sounds: sound_names,
        });
        Ok(())
    }

    /// `CL_ParseUpdate (bits)` (CLIENT-2), with the FitzQuake lineage's lerp flags
    /// (CLIENT-7.1).
    fn parse_update(
        &mut self,
        r: &mut Reader<'_>,
        bits: i32,
        rand: &mut dyn Random,
    ) -> Result<(), ParseError> {
        if self.signon == CLIENT_SIGNONS - 1 {
            self.signon = CLIENT_SIGNONS;
        }
        let mut bits = bits;
        if bits & U_MOREBITS != 0 {
            bits |= r.byte() << 8;
        }
        let num = if bits & U_LONGENTITY != 0 {
            r.short()
        } else {
            r.byte()
        };
        let n = self.entity_num(num)?;
        let maxclients = self.maxclients;
        let mtime = self.mtime;
        let Some(ent) = self.entities.get_mut(n) else {
            return Err(ParseError::EntityNumber(num));
        };
        #[expect(
            clippy::float_cmp,
            reason = "spec CLIENT-2.2: a slot whose msgtime is not exactly the previous message's time is forced"
        )]
        let mut forcelink = ent.msgtime != mtime[1];
        if ent.msgtime + CLIENT_LERP_RESET_GAP < mtime[0] {
            ent.lerpflags.reset_anim = true;
        }
        ent.msgtime = mtime[0];
        let modnum = if bits & U_MODEL != 0 {
            let modnum = r.byte();
            usize::try_from(modnum)
                .ok()
                .filter(|&m| m < CLIENT_MAX_MODELS)
                .ok_or(ParseError::BadModel(modnum))?
        } else {
            ent.baseline.modelindex
        };
        let model = self.models.get(modnum).and_then(Option::as_ref);
        if modnum != ent.model {
            ent.model = modnum;
            ent.lerpflags.reset_anim = true;
            match model {
                Some(info) => {
                    ent.syncbase = if info.random_sync {
                        double_to_float(f64::from(rand.rand() & 0x7fff) / f64::from(0x7fff))
                    } else {
                        0.0
                    };
                }
                None => forcelink = true,
            }
        }
        ent.frame = if bits & U_FRAME != 0 {
            r.byte()
        } else {
            ent.baseline.frame
        };
        let colormap = if bits & U_COLORMAP != 0 {
            r.byte()
        } else {
            ent.baseline.colormap
        };
        if colormap != 0 && usize::try_from(colormap).unwrap_or(usize::MAX) > maxclients {
            return Err(ParseError::BadColormap(colormap));
        }
        ent.colormap = colormap;
        ent.skin = if bits & U_SKIN != 0 {
            r.byte()
        } else {
            ent.baseline.skin
        };
        ent.effects = if bits & U_EFFECTS != 0 {
            r.byte()
        } else {
            ent.baseline.effects
        };
        ent.msg_origins[1] = ent.msg_origins[0];
        ent.msg_angles[1] = ent.msg_angles[0];
        let mut origin = ent.baseline.origin;
        let mut angles = ent.baseline.angles;
        for (i, (origin_bit, angle_bit)) in [
            (U_ORIGIN1, U_ANGLE1),
            (U_ORIGIN2, U_ANGLE2),
            (U_ORIGIN3, U_ANGLE3),
        ]
        .into_iter()
        .enumerate()
        {
            if bits & origin_bit != 0
                && let Some(o) = origin.get_mut(i)
            {
                *o = r.coord();
            }
            if bits & angle_bit != 0
                && let Some(a) = angles.get_mut(i)
            {
                *a = r.angle();
            }
        }
        ent.msg_origins[0] = origin;
        ent.msg_angles[0] = angles;
        // CLIENT-7.1: U_STEP, id's U_NOLERP, marks a stepping entity and forces it.
        if bits & U_STEP != 0 {
            ent.lerpflags.movestep = true;
            ent.forcelink = true;
        } else {
            ent.lerpflags.movestep = false;
        }
        if forcelink {
            ent.msg_origins[1] = ent.msg_origins[0];
            ent.origin = ent.msg_origins[0];
            ent.msg_angles[1] = ent.msg_angles[0];
            ent.angles = ent.msg_angles[0];
            ent.forcelink = true;
        }
        Ok(())
    }

    /// `CL_ParseClientdata (bits)` (CLIENT-3).
    fn parse_clientdata(&mut self, r: &mut Reader<'_>, bits: i32) {
        self.viewheight = if bits & SU_VIEWHEIGHT != 0 {
            int_to_float(r.char())
        } else {
            CLIENT_DEFAULT_VIEWHEIGHT
        };
        self.idealpitch = if bits & SU_IDEALPITCH != 0 {
            int_to_float(r.char())
        } else {
            0.0
        };
        self.mvelocity[1] = self.mvelocity[0];
        for i in 0..3 {
            let punch = if bits & (SU_PUNCH1 << i) != 0 {
                int_to_float(r.char())
            } else {
                0.0
            };
            let velocity = if bits & (SU_VELOCITY1 << i) != 0 {
                int_to_float(r.char().wrapping_mul(16))
            } else {
                0.0
            };
            if let Some(p) = self.punchangle.get_mut(i) {
                *p = punch;
            }
            if let Some(v) = self.mvelocity[0].get_mut(i) {
                *v = velocity;
            }
        }
        let items = r.long();
        if self.items != items {
            for (j, gettime) in self.item_gettime.iter_mut().enumerate() {
                let bit = 1_i32.wrapping_shl(u32::try_from(j).unwrap_or(0));
                if items & bit != 0 && self.items & bit == 0 {
                    *gettime = self.time;
                }
            }
            self.items = items;
        }
        self.onground = bits & SU_ONGROUND != 0;
        self.inwater = bits & SU_INWATER != 0;
        self.stats[stat::WEAPONFRAME] = if bits & SU_WEAPONFRAME != 0 {
            r.byte()
        } else {
            0
        };
        self.stats[stat::ARMOR] = if bits & SU_ARMOR != 0 { r.byte() } else { 0 };
        self.stats[stat::WEAPON] = if bits & SU_WEAPON != 0 { r.byte() } else { 0 };
        self.stats[stat::HEALTH] = r.short();
        self.stats[stat::AMMO] = r.byte();
        for i in 0..4 {
            let value = r.byte();
            if let Some(slot) = self.stats.get_mut(stat::SHELLS.saturating_add(i)) {
                *slot = value;
            }
        }
        let weapon = r.byte();
        self.stats[stat::ACTIVEWEAPON] = match self.game {
            Game::Standard => weapon,
            Game::MissionPack => 1_i32.wrapping_shl(u32::try_from(weapon).unwrap_or(0)),
        };
    }

    /// `CL_ParseStatic` (CLIENT-1.3).
    fn parse_static(&mut self, r: &mut Reader<'_>) -> Result<(), ParseError> {
        if self.statics.len() >= CLIENT_MAX_STATIC_ENTITIES {
            return Err(ParseError::TooManyStatics);
        }
        let baseline = parse_baseline(r);
        let mut ent = Entity::new();
        ent.baseline = baseline;
        ent.model = baseline.modelindex;
        ent.frame = baseline.frame;
        ent.colormap = 0;
        ent.skin = baseline.skin;
        ent.effects = baseline.effects;
        ent.origin = baseline.origin;
        ent.angles = baseline.angles;
        self.statics.push(ent);
        Ok(())
    }

    /// `CL_ParseStartSoundPacket` (CLIENT-4.1).
    fn parse_sound(&self, r: &mut Reader<'_>) -> Result<SoundStart, ParseError> {
        let field_mask = r.byte();
        let volume = if field_mask & SND_VOLUME != 0 {
            r.byte()
        } else {
            SND_PACKET_VOLUME
        };
        let attenuation = if field_mask & SND_ATTENUATION != 0 {
            double_to_float(f64::from(r.byte()) / 64.0)
        } else {
            SND_PACKET_ATTENUATION
        };
        let channel = r.short();
        let sound_num = r.byte();
        let ent = channel >> 3;
        let channel = channel & 7;
        if usize::try_from(ent).unwrap_or(usize::MAX) > CLIENT_MAX_EDICTS {
            return Err(ParseError::SoundEntity(ent));
        }
        let origin = [r.coord(), r.coord(), r.coord()];
        Ok(SoundStart {
            entity: ent,
            channel,
            sound: SoundRef::Index(usize::try_from(sound_num).unwrap_or(0)),
            origin,
            volume: double_to_float(f64::from(volume) / 255.0),
            attenuation,
        })
    }

    /// `CL_ParseTEnt` (CLIENT-6.1 to CLIENT-6.3).
    fn parse_temp_entity(
        &mut self,
        r: &mut Reader<'_>,
        rand: &mut dyn Random,
        events: &mut Vec<Event>,
    ) -> Result<(), ParseError> {
        let kind = r.byte();
        let time = self.time;
        let sound = |name: &'static str, pos: [f32; 3]| {
            Event::Sound(SoundStart {
                entity: -1,
                channel: 0,
                sound: SoundRef::Name(name),
                origin: pos,
                volume: 1.0,
                attenuation: 1.0,
            })
        };
        match kind {
            TE_WIZSPIKE | TE_KNIGHTSPIKE | TE_SPIKE | TE_SUPERSPIKE | TE_GUNSHOT => {
                let pos = [r.coord(), r.coord(), r.coord()];
                let (color, count) = match kind {
                    TE_WIZSPIKE => (20, 30),
                    TE_KNIGHTSPIKE => (226, 20),
                    TE_SPIKE => (0, 10),
                    _ => (0, 20),
                };
                self.particles
                    .run_effect(rand, time, pos, [0.0; 3], color, count);
                match kind {
                    TE_WIZSPIKE => events.push(sound("wizard/hit.wav", pos)),
                    TE_KNIGHTSPIKE => events.push(sound("hknight/hit.wav", pos)),
                    TE_SPIKE | TE_SUPERSPIKE => {
                        if rand.rand().checked_rem(5).unwrap_or(0) != 0 {
                            events.push(sound("weapons/tink1.wav", pos));
                        } else {
                            let name = match rand.rand() & 3 {
                                1 => "weapons/ric1.wav",
                                2 => "weapons/ric2.wav",
                                _ => "weapons/ric3.wav",
                            };
                            events.push(sound(name, pos));
                        }
                    }
                    _ => {}
                }
            }
            TE_EXPLOSION | TE_EXPLOSION2 => {
                let pos = [r.coord(), r.coord(), r.coord()];
                if kind == TE_EXPLOSION {
                    self.particles.explosion(rand, time, pos);
                } else {
                    let color_start = r.byte();
                    let color_length = r.byte();
                    self.particles
                        .explosion2(rand, time, pos, color_start, color_length);
                }
                let dl = self.alloc_dlight(0);
                dl.origin = pos;
                dl.radius = 350.0;
                dl.die = time + 0.5;
                dl.decay = 300.0;
                events.push(sound("weapons/r_exp3.wav", pos));
            }
            TE_TAREXPLOSION => {
                let pos = [r.coord(), r.coord(), r.coord()];
                self.particles.blob_explosion(rand, time, pos);
                events.push(sound("weapons/r_exp3.wav", pos));
            }
            TE_LIGHTNING1 | TE_LIGHTNING2 | TE_LIGHTNING3 | TE_BEAM => {
                let model = match kind {
                    TE_LIGHTNING1 => 1,
                    TE_LIGHTNING2 => 2,
                    TE_LIGHTNING3 => 3,
                    _ => 4,
                };
                self.parse_beam(r, model);
            }
            TE_LAVASPLASH => {
                let pos = [r.coord(), r.coord(), r.coord()];
                self.particles.lava_splash(rand, time, pos);
            }
            TE_TELEPORT => {
                let pos = [r.coord(), r.coord(), r.coord()];
                self.particles.teleport_splash(rand, time, pos);
            }
            other => return Err(ParseError::TempEntity(other)),
        }
        Ok(())
    }

    /// `CL_ParseBeam (m)` (CLIENT-6.2).
    fn parse_beam(&mut self, r: &mut Reader<'_>, model: usize) {
        let ent = r.short();
        let start = [r.coord(), r.coord(), r.coord()];
        let end = [r.coord(), r.coord(), r.coord()];
        let time = self.time;
        let set = |b: &mut Beam| {
            b.entity = ent;
            b.model = model;
            b.endtime = time + CLIENT_BEAM_TIME;
            b.start = start;
            b.end = end;
        };
        if let Some(b) = self.beams.iter_mut().find(|b| b.entity == ent) {
            set(b);
            return;
        }
        if let Some(b) = self
            .beams
            .iter_mut()
            .find(|b| b.model == 0 || b.endtime < time)
        {
            set(b);
        }
        // Else: `beam list overflow!`, a console message.
    }

    // ---- lights (CLIENT-5.7) ----

    /// `CL_AllocDlight (key)`.
    fn alloc_dlight(&mut self, key: i32) -> &mut DLight {
        let time = self.time;
        let index = if key != 0 {
            self.dlights.iter().position(|dl| dl.key == key)
        } else {
            None
        };
        let index = index
            .or_else(|| self.dlights.iter().position(|dl| dl.die < time))
            .unwrap_or(0)
            .min(CLIENT_MAX_DLIGHTS.saturating_sub(1));
        // The index is within the array; the fallback never runs.
        let fallback = &mut self.spare_light;
        let dl = self.dlights.get_mut(index).unwrap_or(fallback);
        *dl = DLight {
            key,
            ..DLight::default()
        };
        dl
    }

    /// `CL_DecayLights`.
    fn decay_lights(&mut self) {
        let time = self.time - self.oldtime;
        for dl in &mut self.dlights {
            if dl.die < self.time || dl.radius == 0.0 {
                continue;
            }
            dl.radius = (dl.radius - time * dl.decay).max(0.0);
        }
    }

    // ---- relinking (CLIENT-5) ----

    /// `CL_LerpPoint` (CLIENT-5.2): the client interpolates even with a local server, a
    /// deliberate divergence.
    fn lerp_point(&mut self) -> f32 {
        let mut f = self.mtime[0] - self.mtime[1];
        if f == 0.0 {
            self.time = self.mtime[0];
            return 1.0;
        }
        if f > CLIENT_LERP_MAX_GAP {
            self.mtime[1] = self.mtime[0] - CLIENT_LERP_MAX_GAP;
            f = CLIENT_LERP_MAX_GAP;
        }
        let mut frac = (self.time - self.mtime[1]) / f;
        if frac < 0.0 {
            if frac < -0.01 {
                self.time = self.mtime[1];
            }
            frac = 0.0;
        } else if frac > 1.0 {
            if frac > 1.01 {
                self.time = self.mtime[0];
            }
            frac = 1.0;
        }
        frac
    }

    /// `CL_RelinkEntities` (CLIENT-5.3 to CLIENT-5.6), with the move and animation lerps of
    /// CLIENT-7.
    #[expect(
        clippy::too_many_lines,
        reason = "the relink is one pass over the slots, in id's order, and reads best whole"
    )]
    fn relink_entities(&mut self, rand: &mut dyn Random) {
        let frac = self.lerp_point();
        self.visedicts.clear();
        for (v, (m0, m1)) in self
            .velocity
            .iter_mut()
            .zip(self.mvelocity[0].iter().zip(self.mvelocity[1].iter()))
        {
            *v = m1 + frac * (m0 - m1);
        }
        let bobjrotate = anglemod(CLIENT_ROTATE_SPEED * self.time);
        let time = self.time;
        let viewentity = self.viewentity;
        let mtime0 = self.mtime[0];
        for i in 1..self.entities.len() {
            let Some(ent) = self.entities.get_mut(i) else {
                break;
            };
            if !ent.has_model() {
                continue;
            }
            #[expect(
                clippy::float_cmp,
                reason = "spec CLIENT-5.4: a slot not in the latest message, by its exact time, loses its model"
            )]
            if ent.msgtime != mtime0 {
                ent.model = 0;
                ent.lerpflags.reset_move = true;
                ent.lerpflags.reset_anim = true;
                continue;
            }
            let oldorg = ent.origin;
            if ent.forcelink {
                ent.origin = ent.msg_origins[0];
                ent.angles = ent.msg_angles[0];
            } else {
                let mut f = frac;
                let mut delta = [0.0_f32; 3];
                for (d, (new, old)) in delta
                    .iter_mut()
                    .zip(ent.msg_origins[0].iter().zip(ent.msg_origins[1].iter()))
                {
                    *d = new - old;
                    if *d > CLIENT_TELEPORT_DISTANCE || *d < -CLIENT_TELEPORT_DISTANCE {
                        f = 1.0;
                    }
                }
                // CLIENT-7.2: a stepping entity under r_lerpmove is moved by its own lerp.
                if ent.lerpflags.movestep && self.lerp.moves {
                    f = 1.0;
                }
                #[expect(
                    clippy::float_cmp,
                    reason = "spec CLIENT-7.2: a teleport, f exactly 1 from a large delta, resets the move lerp"
                )]
                if f == 1.0 && delta.iter().any(|&d| d.abs() > CLIENT_TELEPORT_DISTANCE) {
                    ent.lerpflags.reset_move = true;
                }
                for j in 0..3 {
                    let (Some(o), Some(d), Some(old)) = (
                        ent.origin.get_mut(j),
                        delta.get(j),
                        ent.msg_origins[1].get(j),
                    ) else {
                        break;
                    };
                    *o = old + f * d;
                    let (Some(a), Some(new), Some(old)) = (
                        ent.angles.get_mut(j),
                        ent.msg_angles[0].get(j),
                        ent.msg_angles[1].get(j),
                    ) else {
                        break;
                    };
                    let mut d = new - old;
                    if d > 180.0 {
                        d -= 360.0;
                    } else if d < -180.0 {
                        d += 360.0;
                    }
                    *a = old + f * d;
                }
            }
            let model = self.models.get(ent.model).and_then(Option::as_ref);
            let model_flags = model.map_or(0, |m| m.flags);
            if model_flags & flags::ROTATE != 0 {
                ent.angles[1] = bobjrotate;
            }
            let (origin, angles, effects, forcelink) =
                (ent.origin, ent.angles, ent.effects, ent.forcelink);
            ent.forcelink = false;
            let n = i32::try_from(i).unwrap_or(0);
            if effects & EF_BRIGHTFIELD != 0 {
                self.particles.entity_field(rand, time, origin);
            }
            if effects & EF_MUZZLEFLASH != 0 {
                let (forward, _, _) = makevectors(angles);
                let radius = 200.0 + int_to_float(rand.rand() & 31);
                let dl = self.alloc_dlight(n);
                dl.origin = [
                    origin[0] + 18.0 * forward[0],
                    origin[1] + 18.0 * forward[1],
                    origin[2] + 16.0 + 18.0 * forward[2],
                ];
                dl.radius = radius;
                dl.minlight = 32.0;
                dl.die = time + 0.1;
                if let Some(ent) = self.entities.get_mut(i) {
                    ent.lerpflags.reset_anim = true;
                    ent.lerpflags.reset_anim2 = true;
                }
                if i == viewentity {
                    self.weapon.lerpflags.reset_anim = true;
                    self.weapon.lerpflags.reset_anim2 = true;
                }
            }
            if effects & EF_BRIGHTLIGHT != 0 {
                let radius = 400.0 + int_to_float(rand.rand() & 31);
                let dl = self.alloc_dlight(n);
                dl.origin = [origin[0], origin[1], origin[2] + 16.0];
                dl.radius = radius;
                dl.die = time + 0.001;
            }
            if effects & EF_DIMLIGHT != 0 {
                let radius = 200.0 + int_to_float(rand.rand() & 31);
                let dl = self.alloc_dlight(n);
                dl.origin = origin;
                dl.radius = radius;
                dl.die = time + 0.001;
            }
            if model_flags & flags::GIB != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 2);
            } else if model_flags & flags::ZOMGIB != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 4);
            } else if model_flags & flags::TRACER != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 3);
            } else if model_flags & flags::TRACER2 != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 5);
            } else if model_flags & flags::ROCKET != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 0);
                let dl = self.alloc_dlight(n);
                dl.origin = origin;
                dl.radius = 200.0;
                dl.die = time + 0.01;
            } else if model_flags & flags::GRENADE != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 1);
            } else if model_flags & flags::TRACER3 != 0 {
                self.particles.rocket_trail(rand, time, oldorg, origin, 6);
            }
            let _ = forcelink;
            if i == viewentity {
                continue;
            }
            if self.visedicts.len() < CLIENT_MAX_VISEDICTS {
                let scene = self.scene_entity(i);
                self.visedicts.push(scene);
            }
        }
    }

    /// Entity slot `i` as the renderer draws it, with its lerps set up (CLIENT-7.3,
    /// CLIENT-7.4).
    fn scene_entity(&mut self, i: usize) -> SceneEntity {
        let time = self.time;
        let lerp = self.lerp;
        let info = self
            .entities
            .get(i)
            .and_then(|e| self.models.get(e.model))
            .and_then(Option::as_ref)
            .cloned();
        let Some(ent) = self.entities.get_mut(i) else {
            return SceneEntity::default();
        };
        let (origin, angles) = setup_entity_transform(ent, time, lerp.moves);
        let poses = info
            .as_ref()
            .filter(|m| matches!(m.kind, ModelKind::Alias { .. }))
            .map(|m| setup_alias_frame(ent, m, time, lerp.models));
        SceneEntity {
            model: ent.model,
            origin,
            angles,
            frame: ent.frame,
            colormap: ent.colormap,
            skin: ent.skin,
            syncbase: ent.syncbase,
            poses,
        }
    }

    /// The weapon's poses (CLIENT-7.3), after `cl.viewent`'s model and frame are set.
    fn alias_poses_for_weapon(&mut self) -> Option<PoseBlend> {
        let info = self.models.get(self.weapon.model)?.as_ref()?.clone();
        matches!(info.kind, ModelKind::Alias { .. })
            .then(|| setup_alias_frame(&mut self.weapon, &info, self.time, self.lerp.models))
    }

    /// `CL_UpdateTEnts` (CLIENT-6.4): a temporary entity every 30 units along each live
    /// beam, walked with its own index (a deliberate divergence from id's undefined read).
    fn update_temp_entities(&mut self, rand: &mut dyn Random) {
        let time = self.time;
        let mut count = 0;
        let viewentity_origin = self
            .entities
            .get(self.viewentity)
            .map(|e| e.origin)
            .unwrap_or_default();
        let viewentity = i32::try_from(self.viewentity).unwrap_or(-1);
        let beam_models: Vec<usize> = (1..=CLIENT_BEAM_MODELS.len())
            .map(|m| self.beam_model_index(m))
            .collect();
        for b in &mut self.beams {
            if b.model == 0 || b.endtime < time {
                continue;
            }
            if b.entity == viewentity {
                b.start = viewentity_origin;
            }
            let [sx, sy, sz] = b.start;
            let [ex, ey, ez] = b.end;
            let dist = [ex - sx, ey - sy, ez - sz];
            let (yaw, pitch) = if dist[1] == 0.0 && dist[0] == 0.0 {
                (0.0, if dist[2] > 0.0 { 90.0 } else { 270.0 })
            } else {
                let yaw =
                    f64::from(dist[1]).atan2(f64::from(dist[0])) * 180.0 / core::f64::consts::PI;
                let mut yaw = int_to_float(double_to_int(yaw));
                if yaw < 0.0 {
                    yaw += 360.0;
                }
                let forward =
                    double_to_float(f64::from(dist[0] * dist[0] + dist[1] * dist[1]).sqrt());
                let pitch =
                    f64::from(dist[2]).atan2(f64::from(forward)) * 180.0 / core::f64::consts::PI;
                let mut pitch = int_to_float(double_to_int(pitch));
                if pitch < 0.0 {
                    pitch += 360.0;
                }
                (yaw, pitch)
            };
            let mut org = b.start;
            let (unit, mut d) = vector_normalize(dist);
            let model = beam_models
                .get(b.model.wrapping_sub(1))
                .copied()
                .unwrap_or(0);
            while d > 0.0 {
                if self.visedicts.len() >= CLIENT_MAX_VISEDICTS || count >= CLIENT_MAX_TEMP_ENTITIES
                {
                    return;
                }
                count = count.saturating_add(1);
                let roll = int_to_float(rand.rand().checked_rem(360).unwrap_or(0));
                self.visedicts.push(SceneEntity {
                    model,
                    origin: org,
                    angles: [pitch, yaw, roll],
                    frame: 0,
                    colormap: 0,
                    skin: 0,
                    syncbase: 0.0,
                    poses: Some(PoseBlend {
                        previous: 0,
                        current: 0,
                        blend: 1.0,
                    }),
                });
                for (o, u) in org.iter_mut().zip(unit) {
                    *o += u * CLIENT_BEAM_STEP;
                }
                d -= CLIENT_BEAM_STEP;
            }
        }
    }

    /// The precache index a beam's model has, found by name among the server's models;
    /// the host precaches the four beam models with the map, so one is always there.
    fn beam_model_index(&self, beam: usize) -> usize {
        let name = CLIENT_BEAM_MODELS
            .get(beam.wrapping_sub(1))
            .copied()
            .unwrap_or_default();
        self.model_names.iter().position(|n| n == name).unwrap_or(0)
    }
}

/// `CL_ParseBaseline` (CLIENT-1.3).
fn parse_baseline(r: &mut Reader<'_>) -> Baseline {
    let modelindex = usize::try_from(r.byte()).unwrap_or(0);
    let frame = r.byte();
    let colormap = r.byte();
    let skin = r.byte();
    let mut origin = [0.0; 3];
    let mut angles = [0.0; 3];
    for (o, a) in origin.iter_mut().zip(angles.iter_mut()) {
        *o = r.coord();
        *a = r.angle();
    }
    Baseline {
        origin,
        angles,
        modelindex,
        frame,
        colormap,
        skin,
        effects: 0,
    }
}

/// `R_SetupAliasFrame` (CLIENT-7.3): the poses an entity blends this frame.
fn setup_alias_frame(
    ent: &mut Entity,
    model: &ModelInfo,
    time: f32,
    lerp_models: bool,
) -> PoseBlend {
    let frame = model
        .frame(usize::try_from(ent.frame).unwrap_or(usize::MAX))
        .unwrap_or(FrameInfo {
            first_pose: 0,
            poses: 1,
            interval: 0.0,
        });
    let mut posenum = frame.first_pose;
    let numposes = frame.poses;
    if numposes > 1 {
        ent.lerp.time = frame.interval;
        let step = double_to_int(f64::from(time / ent.lerp.time));
        let step = usize::try_from(step).unwrap_or(0);
        posenum = posenum.saturating_add(step.checked_rem(numposes).unwrap_or(0));
    } else {
        ent.lerp.time = CLIENT_LERP_SINGLE_POSE;
    }
    if ent.lerpflags.reset_anim {
        ent.lerp.start = 0.0;
        ent.lerp.previous_pose = posenum;
        ent.lerp.current_pose = posenum;
        ent.lerpflags.reset_anim = false;
    } else if ent.lerp.current_pose != posenum {
        if ent.lerpflags.reset_anim2 {
            ent.lerp.start = 0.0;
            ent.lerp.previous_pose = posenum;
            ent.lerp.current_pose = posenum;
            ent.lerpflags.reset_anim2 = false;
        } else {
            ent.lerp.start = f64::from(time);
            ent.lerp.previous_pose = ent.lerp.current_pose;
            ent.lerp.current_pose = posenum;
        }
    }
    if lerp_models && !model.nolerp {
        let blend = double_to_float((f64::from(time) - ent.lerp.start) / f64::from(ent.lerp.time));
        let blend = blend.clamp(0.0, 1.0);
        #[expect(
            clippy::float_cmp,
            reason = "spec CLIENT-7.3: a blend of exactly 1 makes the previous pose the current"
        )]
        if blend == 1.0 {
            ent.lerp.previous_pose = ent.lerp.current_pose;
        }
        PoseBlend {
            previous: ent.lerp.previous_pose,
            current: ent.lerp.current_pose,
            blend,
        }
    } else {
        PoseBlend {
            previous: posenum,
            current: posenum,
            blend: 1.0,
        }
    }
}

/// `R_SetupEntityTransform` (CLIENT-7.4): where an entity is drawn this frame.
fn setup_entity_transform(ent: &mut Entity, time: f32, lerp_moves: bool) -> ([f32; 3], [f32; 3]) {
    if ent.lerpflags.reset_move {
        ent.lerp.move_start = 0.0;
        ent.lerp.previous_origin = ent.origin;
        ent.lerp.current_origin = ent.origin;
        ent.lerp.previous_angles = ent.angles;
        ent.lerp.current_angles = ent.angles;
        ent.lerpflags.reset_move = false;
    } else if moved(ent) {
        ent.lerp.move_start = f64::from(time);
        ent.lerp.previous_origin = ent.lerp.current_origin;
        ent.lerp.current_origin = ent.origin;
        ent.lerp.previous_angles = ent.lerp.current_angles;
        ent.lerp.current_angles = ent.angles;
    }
    if lerp_moves && ent.lerpflags.movestep {
        let blend = double_to_float(
            (f64::from(time) - ent.lerp.move_start) / f64::from(CLIENT_LERP_MOVE_TIME),
        )
        .clamp(0.0, 1.0);
        let mut origin = [0.0; 3];
        let mut angles = [0.0; 3];
        for i in 0..3 {
            let (Some(o), Some(p), Some(c)) = (
                origin.get_mut(i),
                ent.lerp.previous_origin.get(i),
                ent.lerp.current_origin.get(i),
            ) else {
                break;
            };
            *o = p + (c - p) * blend;
            let (Some(a), Some(p), Some(c)) = (
                angles.get_mut(i),
                ent.lerp.previous_angles.get(i),
                ent.lerp.current_angles.get(i),
            ) else {
                break;
            };
            let mut d = c - p;
            if d > 180.0 {
                d -= 360.0;
            }
            if d < -180.0 {
                d += 360.0;
            }
            *a = p + d * blend;
        }
        (origin, angles)
    } else {
        (ent.origin, ent.angles)
    }
}

/// `VectorCompare`: whether the origin or the angles differ from the current lerp targets
/// exactly (CLIENT-7.4).
#[expect(
    clippy::float_cmp,
    reason = "spec CLIENT-7.4: an origin or angles that differ at all start a new move lerp"
)]
fn moved(ent: &Entity) -> bool {
    ent.origin != ent.lerp.current_origin || ent.angles != ent.lerp.current_angles
}
