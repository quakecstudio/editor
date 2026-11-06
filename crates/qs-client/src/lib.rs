// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's client (P3-26, #240): the server's messages read into the entity
//! slots, the client data and the sounds (`client`), the particle system (`particles`),
//! the view (`view`), and the scene the renderer draws (`scene`).
//!
//! Implements `docs/spec/engine/client.md` CLIENT-1 to CLIENT-7,
//! `docs/spec/engine/particles.md` PART-1 to PART-3 and `docs/spec/engine/view.md` VIEW-1
//! to VIEW-4.

mod client;
mod console;
mod entity;
mod hud;
mod menu;
mod message;
mod models;
mod particles;
mod scene;
#[cfg(test)]
mod tests;
mod view;

pub use client::{
    CLIENT_BEAM_MODELS, CLIENT_BEAM_STEP, CLIENT_BEAM_TIME, CLIENT_DEFAULT_VIEWHEIGHT,
    CLIENT_LERP_MAX_GAP, CLIENT_LERP_MOVE_TIME, CLIENT_LERP_RESET_GAP, CLIENT_LERP_SINGLE_POSE,
    CLIENT_MAX_BEAMS, CLIENT_MAX_DLIGHTS, CLIENT_MAX_EDICTS, CLIENT_MAX_LIGHTSTYLES,
    CLIENT_MAX_MODELS, CLIENT_MAX_SCOREBOARD, CLIENT_MAX_SOUNDS, CLIENT_MAX_STATIC_ENTITIES,
    CLIENT_MAX_STATS, CLIENT_MAX_TEMP_ENTITIES, CLIENT_MAX_VISEDICTS, CLIENT_PROTOCOL_VERSION,
    CLIENT_ROTATE_SPEED, CLIENT_SIGNONS, CLIENT_TELEPORT_DISTANCE, CLIENT_TENT_SOUNDS, Client,
    ClientLimits, Event, Game, HUD_DEFAULT_LINES, Hud, LerpSettings, ParseError, Score, SoundRef,
    SoundStart, stat,
};
pub use console::{
    CON_CURSOR_SPEED, CON_HISTORY, CON_MAX_LINE, CON_NOTIFY_TIME, CON_NUM_TIMES, CON_SPEED,
    CON_TEXTSIZE, Console, ConsoleKey, KeyDest, Typed,
};
pub use entity::{Baseline, Entity, Lerp, LerpFlags};
pub use hud::{
    Anchor, CenterPrint, HUD_CENTER_LINE_LENGTH, HUD_CENTER_MAX, HUD_CENTERTIME, HUD_FLASH_FRAMES,
    HUD_GAME_DEATHMATCH, HUD_ITEMS, HUD_LOW_AMMO, HUD_LOW_ARMOR, HUD_LOW_HEALTH, HUD_NUM_WIDTH,
    HUD_PRINTSPEED, HUD_SBAR_HEIGHT, HUD_WEAPONS, HudInput, HudItem, Placed, color_for_map,
    intermission_overlay, sort_frags, status_bar,
};
pub use menu::{MENU_HELP_PAGES, Menu, MenuEvent, MenuKey, MenuState};
pub use message::Reader;
pub use models::{FrameInfo, ModelInfo, ModelKind, Models, Random, SYNCTYPE_RAND, flags};
pub use particles::{
    PART_ABSOLUTE_MIN_PARTICLES, PART_ENTITY_PARTICLES, PART_GRAVITY, PART_GRAVITY_SCALE,
    PART_MAX_PARTICLES, PART_RAMP1, PART_RAMP2, PART_RAMP3, Particle, ParticleType, Particles,
};
pub use scene::{DynamicLight, PoseBlend, Scene, SceneEntity, SceneParticle};
pub use view::{
    Contents, Refdef, Shift, VIEW_BOB, VIEW_BOBCYCLE, VIEW_BOBUP, VIEW_CENTERMOVE,
    VIEW_CENTERSPEED, VIEW_CSHIFTPERCENT, VIEW_FORWARDSPEED, VIEW_IDLE_CYCLES, VIEW_IDLE_LEVELS,
    VIEW_IDLESCALE, VIEW_KICKPITCH, VIEW_KICKROLL, VIEW_KICKTIME, VIEW_NUM_CSHIFTS, VIEW_ROLLANGLE,
    VIEW_ROLLSPEED, VIEW_VIEWSIZE, ViewInput, ViewSettings, ViewState, Weapon, angledelta,
    calc_bob, calc_roll, gamma_table,
};
