// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, MSG_WriteChar, MSG_WriteByte, MSG_WriteShort, MSG_WriteLong, MSG_WriteFloat, MSG_WriteString, MSG_WriteCoord, MSG_WriteAngle, SZ_GetSpace
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_FatPVS, SV_AddToFatPVS, SV_WriteEntitiesToClient, SV_WriteClientdataToMessage, SV_SendClientDatagram, SV_UpdateToReliableMessages, SV_SendClientMessages, SV_CleanupEnts, SV_CreateBaseline, SV_ModelIndex, SV_SendServerinfo, SV_StartSound, SV_StartParticle, SV_BroadcastPrintf
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_PreSpawn_f, Host_Spawn_f, Host_Begin_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_user.c, SV_SetIdealPitch
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_bprint, PF_sprint, PF_centerprint, PF_stuffcmd, PF_sound, PF_ambientsound, PF_particle, PF_lightstyle, PF_makestatic, PF_WriteByte, PF_WriteChar, PF_WriteShort, PF_WriteLong, PF_WriteCoord, PF_WriteAngle, PF_WriteString, PF_WriteEntity
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/pr_cmds.c, PF_localsound
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/sv_main.c, SV_LocalSound
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/pr_edict.c, NUM_FOR_EDICT

//! The messages the server sends its client (`docs/spec/engine/net.md`, NET-1 to NET-5):
//! the writers and buffers, the baselines and the signon sequence, each frame's datagram of
//! time, client data and entity updates, the reliable messages, and the bytes the builtins
//! write beside the mock engine's counts.
//!
//! Implements NET-1 to NET-5.

use qc_builtins::mock::Precache;
use qc_edict::EdictNumber;
use qc_vm::convert::{double_to_float, float_to_int, int_to_float};
use qc_vm::{Builtins as _, RunError};
use qs_bsp::Bsp;

use crate::builtins::{FL_ONGROUND, SimCall};
use crate::builtins::{PARM0, PARM1, PARM2, PARM3, PARM4};
use crate::client::wire_angle;
use crate::collision::{CONTENTS_SOLID, MoveKind};
use crate::physics::MoveType;
use crate::settings::Setting;
use crate::spawn::c_atof;
use crate::{HALF, POINT};

/// `MAX_DATAGRAM`: the unreliable and reliable datagrams' size (NET-1.2).
pub const NET_MAX_DATAGRAM: usize = 1024;
/// `MAX_MSGLEN`: a client's reliable message's size (NET-1.2).
pub const NET_MAX_MSGLEN: usize = 8000;
/// The signon buffer's size (NET-1.2).
pub const NET_SIGNON_SIZE: usize = 8192;
/// `PROTOCOL_VERSION` (NET-2.2).
pub const NET_PROTOCOL_VERSION: i32 = 15;
/// `VERSION` as `%4.2f` prints it (NET-2.2).
pub const NET_VERSION: &str = "1.09";
/// `GAME_COOP` (NET-2.2).
pub const NET_GAME_COOP: i32 = 0;
/// `GAME_DEATHMATCH` (NET-2.2).
pub const NET_GAME_DEATHMATCH: i32 = 1;
/// How far from a node's plane the fat PVS walk visits both sides (NET-3.1).
pub const NET_FAT_PVS_DISTANCE: f32 = 8.0;
/// The datagram bytes an entity update needs (NET-3.2).
pub const NET_PACKET_SLACK: usize = 16;
/// `MAX_FORWARD`: the traces `SV_SetIdealPitch` makes (NET-4.2).
pub const NET_MAX_FORWARD: usize = 6;
/// `MAX_LIGHTSTYLES`, the styles the spawn message lists (NET-2.4).
const MAX_LIGHTSTYLES: usize = 64;
/// `MSG_WriteCoord`'s eighths of a unit (NET-1.1).
const COORD_SCALE: f32 = 8.0;
/// `MSG_WriteAngle`: a turn of 360 degrees in 256 steps, and the byte kept (NET-1.1).
const ANGLE_STEPS: i32 = 256;
const FULL_TURN_DEGREES: i32 = 360;
const BYTE_MASK: i32 = 0xff;
/// A PVS row's bytes: `(numleafs + 31) >> 3`, a leaf a bit (NET-3.1).
const ROW_ROUND_UP: usize = 31;
/// A leaf's byte in a PVS row, `leaf >> 3`, and its bit, `1 << (leaf & 7)` (NET-3.2).
pub(crate) const BYTE_SHIFT: usize = 3;
pub(crate) const BIT_MASK: usize = 7;
/// The bytes `SV_LocalSound` writes, which must fit (RRE-2.4).
const LOCALSOUND_BYTES: usize = 4;
/// `sound`'s and `ambientsound`'s volume, `volume * 255`, and attenuation, `attenuation * 64`
/// (BUILTIN-12.3, BUILTIN-12.4).
const VOLUME_SCALE: f32 = 255.0;
const ATTENUATION_SCALE: f32 = 64.0;
/// How far a sound's entity is shifted above its channel, `(ent << 3) | channel`
/// (BUILTIN-12.3).
const SOUND_CHANNEL_SHIFT: i32 = 3;
/// `particle`'s direction, `dir * 16` as a signed byte (BUILTIN-12.5).
const PARTICLE_DIR_SCALE: f32 = 16.0;
/// The `Write*` builtins by number (BUILTIN-13.2).
pub(crate) const WRITE_BYTE: u32 = 52;
const WRITE_CHAR: u32 = 53;
const WRITE_SHORT: u32 = 54;
const WRITE_LONG: u32 = 55;
const WRITE_COORD: u32 = 56;
const WRITE_ANGLE: u32 = 57;
const WRITE_STRING: u32 = 58;
pub(crate) const WRITE_ENTITY: u32 = 59;
/// `svc_signonnum`'s last stage, sent with the spawn message (NET-2.4).
const SIGNON_BEGIN: i32 = 3;
/// `SV_WriteEntitiesToClient`: an origin within a tenth of a unit of the baseline is not sent
/// (NET-3.3).
const ORIGIN_TOLERANCE: f64 = 0.1;
/// From entity 256 the number is a short, and from bit 8 the update bits take a second byte
/// (NET-3.3).
const LONG_ENTITY: usize = 256;
const MORE_BITS: i32 = 256;
const SECOND_BYTE_SHIFT: i32 = 8;
/// `SV_WriteClientdataToMessage`: velocity in sixteenths, `items2` above bit 23, the
/// `serverflags` above bit 28, and the item bits a mission pack's weapon is found in
/// (NET-4.4, NET-4.5).
const VELOCITY_SCALE: f32 = 16.0;
const ITEMS2_SHIFT: i32 = 23;
const SERVERFLAGS_SHIFT: i32 = 28;
const ITEM_BITS: i32 = 32;
/// `SV_SetIdealPitch`: the probes' spacing and first step, and how far down each looks
/// (NET-4.2).
const PROBE_SPACING: f32 = 12.0;
const PROBE_FIRST: i32 = 3;
const PROBE_DEPTH: f32 = 160.0;
/// `DEFAULT_VIEWHEIGHT` (NET-4.4).
const DEFAULT_VIEWHEIGHT: f32 = 22.0;
/// `EF_MUZZLEFLASH` (NET-1.3).
const EF_MUZZLEFLASH: i32 = 2;

const STAT_TOTALSECRETS: i32 = 11;
const STAT_TOTALMONSTERS: i32 = 12;
const STAT_SECRETS: i32 = 13;
const STAT_MONSTERS: i32 = 14;

const SVC_UPDATESTAT: i32 = 3;
const SVC_SETVIEW: i32 = 5;
const SVC_SOUND: i32 = 6;
const SVC_TIME: i32 = 7;
pub(crate) const SVC_PRINT: i32 = 8;
const SVC_STUFFTEXT: i32 = 9;
/// `svc_localsound`, the rerelease's (RRE-2.1).
const SVC_LOCALSOUND: i32 = 56;
const SVC_SETANGLE: i32 = 10;
const SVC_SERVERINFO: i32 = 11;
const SVC_LIGHTSTYLE: i32 = 12;
const SVC_UPDATENAME: i32 = 13;
const SVC_UPDATEFRAGS: i32 = 14;
const SVC_CLIENTDATA: i32 = 15;
const SVC_UPDATECOLORS: i32 = 17;
const SVC_PARTICLE: i32 = 18;
const SVC_DAMAGE: i32 = 19;
const SVC_SPAWNSTATIC: i32 = 20;
const SVC_SPAWNBASELINE: i32 = 22;
const SVC_SIGNONNUM: i32 = 25;
pub(crate) const SVC_CENTERPRINT: i32 = 26;
const SVC_SPAWNSTATICSOUND: i32 = 29;
const SVC_CDTRACK: i32 = 32;

const U_MOREBITS: i32 = 1 << 0;
const U_ORIGIN1: i32 = 1 << 1;
const U_ANGLE2: i32 = 1 << 4;
const U_NOLERP: i32 = 1 << 5;
const U_FRAME: i32 = 1 << 6;
const U_SIGNAL: i32 = 1 << 7;
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
const SU_ITEMS: i32 = 1 << 9;
const SU_ONGROUND: i32 = 1 << 10;
const SU_INWATER: i32 = 1 << 11;
const SU_WEAPONFRAME: i32 = 1 << 12;
const SU_ARMOR: i32 = 1 << 13;
const SU_WEAPON: i32 = 1 << 14;

const SND_VOLUME: i32 = 1;
const SND_ATTENUATION: i32 = 2;
const DEFAULT_SOUND_PACKET_VOLUME: i32 = 255;

/// What a [`Message`] does with a write that does not fit: `allowoverflow` (BUILTIN-13.3,
/// BUILTIN-13.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    /// The buffer is emptied and marked overflowed.
    Allowed,
    /// The write is dropped, the fatal error the mock engine reports first.
    #[default]
    Fatal,
}

/// Which game the server runs, which changes how the weapon is sent (NET-4.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameKind {
    /// Standard Quake: the weapon's item bits.
    #[default]
    Standard,
    /// `-rogue` or `-hipnotic`: the weapon's bit number.
    MissionPack,
}

/// A `sizebuf_t`: bytes written with id's `MSG_Write*` (NET-1.1), up to a size (NET-1.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    bytes: Vec<u8>,
    max: usize,
    overflow: Overflow,
    overflowed: bool,
}

impl Message {
    /// An empty buffer of `max` bytes; one that may overflow is emptied and marked when a
    /// write does not fit (BUILTIN-13.4), one that may not drops the write, which the mock
    /// engine reports as the fatal error first (BUILTIN-13.3).
    #[must_use]
    pub const fn new(max: usize, overflow: Overflow) -> Self {
        Self {
            bytes: Vec::new(),
            max,
            overflow,
            overflowed: false,
        }
    }

    /// The bytes written so far.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes are written, `cursize`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether nothing is written.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The size, `maxsize`.
    #[must_use]
    pub const fn max(&self) -> usize {
        self.max
    }

    /// Whether a write overflowed since the flag was last reset (BUILTIN-13.4); `SZ_Clear`
    /// leaves it, [`Message::reset_overflow`] clears it.
    #[must_use]
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// `overflowed = false`, as `SV_SendClientMessages` does after dropping the client.
    pub const fn reset_overflow(&mut self) {
        self.overflowed = false;
    }

    /// `SZ_Clear`.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// The bytes, leaving the buffer empty.
    pub fn take(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.bytes)
    }

    /// `SZ_GetSpace` then the copy: `data` appended when it fits; else an overflowing buffer
    /// is emptied, marked and takes `data`, and any other drops it (NET-1.2).
    pub fn write_bytes(&mut self, data: &[u8]) {
        let fits = self
            .bytes
            .len()
            .checked_add(data.len())
            .is_some_and(|total| total <= self.max);
        if !fits {
            if self.overflow == Overflow::Fatal || data.len() > self.max {
                return;
            }
            self.overflowed = true;
            self.bytes.clear();
        }
        self.bytes.extend_from_slice(data);
    }

    /// `MSG_WriteChar` and `MSG_WriteByte`: the low 8 bits (NET-1.1).
    pub fn write_byte(&mut self, c: i32) {
        let [low, ..] = c.to_le_bytes();
        self.write_bytes(&[low]);
    }

    /// `MSG_WriteShort`: the low 16 bits, little-endian (NET-1.1).
    pub fn write_short(&mut self, c: i32) {
        let [a, b, ..] = c.to_le_bytes();
        self.write_bytes(&[a, b]);
    }

    /// `MSG_WriteLong`: all 32 bits, little-endian (NET-1.1).
    pub fn write_long(&mut self, c: i32) {
        self.write_bytes(&c.to_le_bytes());
    }

    /// `MSG_WriteFloat`: the float's bytes, little-endian (NET-1.1).
    pub fn write_float(&mut self, f: f32) {
        self.write_bytes(&f.to_le_bytes());
    }

    /// `MSG_WriteString`: the text and its NUL (NET-1.1).
    pub fn write_string(&mut self, s: &[u8]) {
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        let mut text = s.get(..end).unwrap_or_default().to_vec();
        text.push(0);
        self.write_bytes(&text);
    }

    /// `MSG_WriteCoord`: the short `(int) (f * 8)` (NET-1.1).
    pub fn write_coord(&mut self, f: f32) {
        self.write_short(float_to_int(f * COORD_SCALE));
    }

    /// `MSG_WriteAngle`: the byte `((int) f * 256 / 360) & 255` (NET-1.1).
    pub fn write_angle(&mut self, f: f32) {
        self.write_byte(
            float_to_int(f)
                .wrapping_mul(ANGLE_STEPS)
                .wrapping_div(FULL_TURN_DEGREES)
                & BYTE_MASK,
        );
    }
}

/// What the world keeps of its BSP for the fat PVS (NET-3.1): the nodes, their planes, each
/// leaf's contents and each leaf's PVS, decompressed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Vis {
    /// Each node's plane and children: a node when 0 or more, else leaf `-1 - c`.
    nodes: Vec<(usize, [i32; 2])>,
    planes: Vec<([f32; 3], f32)>,
    /// Each leaf's contents.
    leafs: Vec<i32>,
    /// Each leaf's PVS row.
    pvs: Vec<Vec<u8>>,
    row: usize,
}

impl Vis {
    pub(crate) fn new(bsp: &Bsp<'_>) -> Self {
        // `fatbytes = (numleafs + 31) >> 3`; a map without visibility sees everything.
        let row = bsp.leafs.len().saturating_add(ROW_ROUND_UP) >> BYTE_SHIFT;
        let pvs = (0..bsp.leafs.len())
            .map(|i| {
                let mut found = bsp.leaf_pvs(i).unwrap_or_default();
                if found.is_empty() {
                    found = vec![0xFF; row];
                }
                found.resize(row, 0);
                found
            })
            .collect();
        Self {
            nodes: bsp.nodes.iter().map(|n| (n.planenum, n.children)).collect(),
            planes: bsp.planes.iter().map(|p| (p.normal, p.dist)).collect(),
            leafs: bsp.leafs.iter().map(|l| l.contents).collect(),
            pvs,
            row,
        }
    }

    /// `Mod_PointInLeaf (p, sv.worldmodel)`: the leaf `p` is in, walking to a node's front
    /// child when `p` is strictly in front of its plane (BSP-21.1); `None` with no nodes.
    pub(crate) fn point_in_leaf(&self, p: [f32; 3]) -> Option<usize> {
        let mut child = 0_i32;
        for _ in 0..=self.nodes.len() {
            let Ok(node) = usize::try_from(child) else {
                return usize::try_from((-1_i32).wrapping_sub(child)).ok();
            };
            let &(planenum, children) = self.nodes.get(node)?;
            let (normal, dist) = self.planes.get(planenum).copied()?;
            let d = p[0] * normal[0] + p[1] * normal[1] + p[2] * normal[2] - dist;
            child = if d > 0.0 { children[0] } else { children[1] };
        }
        None
    }

    /// Leaf `leaf`'s PVS row, `Mod_LeafPVS` (BSP-6.2): every leaf's for leaf 0.
    pub(crate) fn leaf_row(&self, leaf: usize) -> Option<&[u8]> {
        self.pvs.get(leaf).map(Vec::as_slice)
    }

    /// `SV_FatPVS (org)`: the PVS of every non-solid leaf within 8 units of `org` (NET-3.1).
    pub(crate) fn fat_pvs(&self, org: [f32; 3]) -> Vec<u8> {
        let mut fat = vec![0; self.row];
        if self.nodes.is_empty() {
            return fat;
        }
        let mut stack = vec![0_i32];
        while let Some(child) = stack.pop() {
            let Ok(node) = usize::try_from(child) else {
                // A leaf: `-1 - c`.
                let leaf = usize::try_from((-1_i32).wrapping_sub(child)).unwrap_or(usize::MAX);
                if self.leafs.get(leaf).copied() != Some(CONTENTS_SOLID)
                    && let Some(row) = self.pvs.get(leaf)
                {
                    for (byte, bit) in fat.iter_mut().zip(row) {
                        *byte |= bit;
                    }
                }
                continue;
            };
            let Some(&(planenum, children)) = self.nodes.get(node) else {
                continue;
            };
            let (normal, dist) = self.planes.get(planenum).copied().unwrap_or_default();
            let d = org[0] * normal[0] + org[1] * normal[1] + org[2] * normal[2] - dist;
            if d > NET_FAT_PVS_DISTANCE {
                stack.push(children[0]);
            } else if d < -NET_FAT_PVS_DISTANCE {
                stack.push(children[1]);
            } else {
                stack.push(children[1]);
                stack.push(children[0]);
            }
        }
        fat
    }
}

/// An edict's `entity_state_t` baseline (NET-2.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Baseline {
    origin: [f32; 3],
    angles: [f32; 3],
    modelindex: i32,
    frame: i32,
    colormap: i32,
    skin: i32,
    /// Never set by `SV_CreateBaseline`, so 0.
    effects: i32,
}

/// The one client slot, `client_t` (NET-1.3, NET-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientSlot {
    pub(crate) active: bool,
    pub(crate) spawned: bool,
    sendsignon: bool,
    name: Vec<u8>,
    colors: i32,
    old_frags: i32,
    /// Its reliable message, which may overflow (BUILTIN-13.4).
    pub(crate) message: Message,
}

impl Default for ClientSlot {
    fn default() -> Self {
        Self {
            active: false,
            spawned: false,
            sendsignon: false,
            name: b"player".to_vec(),
            colors: 0,
            old_frags: 0,
            message: Message::new(NET_MAX_MSGLEN, Overflow::Allowed),
        }
    }
}

/// The server's message state: the three shared buffers, the client slot, the baselines,
/// the fat PVS data and what has been sent (NET-1.2).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ServerMessages {
    pub(crate) datagram: Message,
    pub(crate) reliable_datagram: Message,
    pub(crate) signon: Message,
    pub(crate) client: ClientSlot,
    pub(crate) baselines: Vec<Baseline>,
    pub(crate) vis: Vis,
    /// `standard_quake`: not `-rogue` or `-hipnotic` (NET-4.4).
    pub(crate) game: GameKind,
    /// The progs CRC the serverinfo names (NET-2.2).
    pub(crate) crc: i32,
    /// The messages sent, oldest first, each one packet.
    pub(crate) outbox: Vec<Vec<u8>>,
    /// The last `svc_setangle`, as the client reads it (NET-4.3).
    pub(crate) set_angle: Option<[f32; 3]>,
}

impl ServerMessages {
    pub(crate) fn new(bsp: &Bsp<'_>, crc: i32) -> Self {
        Self {
            datagram: Message::new(NET_MAX_DATAGRAM, Overflow::Fatal),
            reliable_datagram: Message::new(NET_MAX_DATAGRAM, Overflow::Fatal),
            signon: Message::new(NET_SIGNON_SIZE, Overflow::Fatal),
            client: ClientSlot::default(),
            baselines: Vec::new(),
            vis: Vis::new(bsp),
            game: GameKind::Standard,
            crc,
            outbox: Vec::new(),
            set_angle: None,
        }
    }
}

impl SimCall<'_, '_, '_> {
    /// The text of a QuakeC string value: a progs string, or an engine string.
    pub(crate) fn string_text(&self, value: i32) -> Vec<u8> {
        if value >= 0 {
            self.context
                .progs()
                .string(value)
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        } else {
            self.sim
                .engine_string(value)
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        }
    }

    /// The joined text of the string parameters from `first` (BUILTIN-7.3); with a
    /// localization loaded, `PF_VarString`'s localized text (RRE-1.9, RRE-1.6).
    fn joined_text(&self, first: u16) -> Vec<u8> {
        let argc = u16::try_from(self.context.machine_view().argc()).unwrap_or(u16::MAX);
        let parms: Vec<Vec<u8>> = (first..argc)
            .map(|n| self.string_text(self.context.parm_raw(n)))
            .collect();
        let localization = &self.sim.mock.engine().localization;
        if localization.is_empty() {
            return parms.concat();
        }
        localization.pf_var_string(&parms)
    }

    /// `WriteString`'s string: localized when a localization is loaded (RRE-1.9, RRE-1.6).
    fn written_text(&self, value: i32) -> Vec<u8> {
        let text = self.string_text(value);
        let localization = &self.sim.mock.engine().localization;
        if localization.is_empty() {
            return text;
        }
        localization.loc_get_string(&text).to_vec()
    }

    /// `SV_ModelIndex`: 0 for an empty name, else the precache entry; a name not precached
    /// is the fatal error (NET-2.1).
    fn model_index(&self, name: &[u8]) -> Result<i32, RunError> {
        if name.is_empty() {
            return Ok(0);
        }
        let text = String::from_utf8_lossy(name).into_owned();
        self.sim
            .mock
            .engine()
            .precache_index(Precache::Model, &text)
            .and_then(|i| i32::try_from(i).ok())
            .ok_or_else(|| {
                self.context.run_error(qc_vm::ErrorKind::Builtin {
                    message: format!("SV_ModelIndex: model {text} not precached"),
                })
            })
    }

    /// The centre of edict `n`: `origin + 0.5 * (mins + maxs)` (BUILTIN-12.3, NET-4.1).
    fn centre(&self, n: usize) -> [f32; 3] {
        let origin = self.vector(n, "origin");
        let mins = self.vector(n, "mins");
        let maxs = self.vector(n, "maxs");
        let [ox, oy, oz] = origin;
        let [ax, ay, az] = mins;
        let [bx, by, bz] = maxs;
        [
            ox + HALF * (ax + bx),
            oy + HALF * (ay + by),
            oz + HALF * (az + bz),
        ]
    }

    fn maxclients(&self) -> usize {
        self.sim.mock.engine().clients_active.len()
    }

    /// Whether edict number `n` is a client slot, 1 to `maxclients`.
    fn is_client_slot(&self, n: usize) -> bool {
        n >= 1 && n <= self.maxclients()
    }

    // ---- the builtins' bytes (NET-5.1) ----

    /// `bprint`: `svc_print` and the joined text to every active, spawned client
    /// (BUILTIN-11.1).
    pub(crate) fn mirror_bprint(&mut self) {
        let text = self.joined_text(0);
        let client = &mut self.sim.messages.client;
        if client.active && client.spawned {
            client.message.write_byte(SVC_PRINT);
            client.message.write_string(&text);
        }
    }

    /// `sprint` and `centerprint`: `svc` and the joined text from PARM1 to the client slot
    /// PARM0 names, when it is one (BUILTIN-11.2).
    pub(crate) fn mirror_client_print(&mut self, svc: i32) {
        let value = self.context.parm_raw(PARM0);
        let n = self.edict_of(value);
        if n != 1 || !self.is_client_slot(n) {
            return;
        }
        let text = self.joined_text(1);
        let message = &mut self.sim.messages.client.message;
        message.write_byte(svc);
        message.write_string(&text);
    }

    /// `localsound (client, sample)`, `PF_localsound` and `SV_LocalSound` (RRE-2.1): to a
    /// client, `svc_localsound`, a field byte and the sample's precache index as a byte; an
    /// edict that is no client, a sample not precached, an index of 256 or more under
    /// protocol 15, or a message with fewer than 4 bytes free sends nothing.
    ///
    /// # Errors
    ///
    /// The host error `NUM_FOR_EDICT: bad pointer` for an entity outside the edicts in use
    /// (RRE-2.4).
    pub(crate) fn local_sound(&mut self) -> Result<(), RunError> {
        let n = self.num_for_edict(self.context.parm_raw(PARM0))?;
        let sample = self.string_text(self.context.parm_raw(PARM1));
        if !self.is_client_slot(n) {
            // "tried to localsound to a non-client"
            return Ok(());
        }
        let name = String::from_utf8_lossy(&sample).into_owned();
        let Some(index) = self
            .sim
            .mock
            .engine()
            .precache_index(Precache::Sound, &name)
            .filter(|&i| i >= 1)
        else {
            // "SV_LocalSound: <sample> not precached"
            return Ok(());
        };
        // PROTOCOL_NETQUAKE has no SND_LARGESOUND.
        let Ok(index) = u8::try_from(index) else {
            return Ok(());
        };
        let message = &mut self.sim.messages.client.message;
        if message.len() > message.max().saturating_sub(LOCALSOUND_BYTES) {
            return Ok(());
        }
        message.write_byte(SVC_LOCALSOUND);
        message.write_byte(0);
        message.write_byte(i32::from(index));
        Ok(())
    }

    /// QuakeSpasm's `G_EDICTNUM` (RRE-2.4): the entity's byte offset divided by the edict size,
    /// truncated as C's `/` is, with no check that it falls on an edict; a number below 0 or at
    /// or past `num_edicts` is the host error `NUM_FOR_EDICT: bad pointer`.
    fn num_for_edict(&self, value: i32) -> Result<usize, RunError> {
        let edicts = self.context.machine_view().edicts();
        value
            .checked_div(edicts.layout().edict_size())
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n < edicts.num_edicts())
            .ok_or_else(|| {
                self.context.run_error(qc_vm::ErrorKind::Host {
                    message: "NUM_FOR_EDICT: bad pointer".to_owned(),
                })
            })
    }

    /// `stuffcmd`: `svc_stufftext` and PARM1 to the client (BUILTIN-11.4).
    pub(crate) fn mirror_stuffcmd(&mut self) {
        let value = self.context.parm_raw(PARM0);
        if self.edict_of(value) != 1 {
            return;
        }
        let text = self.string_text(self.context.parm_raw(PARM1));
        let message = &mut self.sim.messages.client.message;
        message.write_byte(SVC_STUFFTEXT);
        message.write_string(&text);
    }

    /// `sound`'s bytes to the unreliable datagram (BUILTIN-12.3), once the mock has
    /// accepted the call.
    pub(crate) fn mirror_sound(&mut self) {
        let ent = self.edict_of(self.context.parm_raw(PARM0));
        let channel = float_to_int(self.context.parm_float(PARM1));
        let sample = self.string_text(self.context.parm_raw(PARM2));
        let volume = float_to_int(self.context.parm_float(PARM3) * VOLUME_SCALE);
        let attenuation = self.context.parm_float(PARM4);
        let name = String::from_utf8_lossy(&sample).into_owned();
        let Some(sound_num) = self
            .sim
            .mock
            .engine()
            .precache_index(Precache::Sound, &name)
            .filter(|&i| i >= 1)
            .and_then(|i| i32::try_from(i).ok())
        else {
            return;
        };
        let centre = self.centre(ent);
        let mut field_mask = 0;
        if volume != DEFAULT_SOUND_PACKET_VOLUME {
            field_mask |= SND_VOLUME;
        }
        #[expect(
            clippy::float_cmp,
            reason = "spec BUILTIN-12.3: the attenuation flag is set when atten is not 1.0 exactly"
        )]
        if attenuation != 1.0 {
            field_mask |= SND_ATTENUATION;
        }
        let number = i32::try_from(ent).unwrap_or(0);
        let datagram = &mut self.sim.messages.datagram;
        datagram.write_byte(SVC_SOUND);
        datagram.write_byte(field_mask);
        if field_mask & SND_VOLUME != 0 {
            datagram.write_byte(volume);
        }
        if field_mask & SND_ATTENUATION != 0 {
            datagram.write_byte(float_to_int(attenuation * ATTENUATION_SCALE));
        }
        datagram.write_short((number << SOUND_CHANNEL_SHIFT) | channel);
        datagram.write_byte(sound_num);
        for c in centre {
            datagram.write_coord(c);
        }
    }

    /// `ambientsound`'s bytes to the signon buffer (BUILTIN-12.4).
    pub(crate) fn mirror_ambientsound(&mut self) {
        let pos = self.context.parm_vector(PARM0);
        let sample = self.string_text(self.context.parm_raw(PARM1));
        let vol = self.context.parm_float(PARM2);
        let atten = self.context.parm_float(PARM3);
        let name = String::from_utf8_lossy(&sample).into_owned();
        let Some(sound_num) = self
            .sim
            .mock
            .engine()
            .precache_index(Precache::Sound, &name)
            .and_then(|i| i32::try_from(i).ok())
        else {
            return;
        };
        let signon = &mut self.sim.messages.signon;
        signon.write_byte(SVC_SPAWNSTATICSOUND);
        for c in pos {
            signon.write_coord(c);
        }
        signon.write_byte(sound_num);
        signon.write_byte(float_to_int(vol * VOLUME_SCALE));
        signon.write_byte(float_to_int(atten * ATTENUATION_SCALE));
    }

    /// `particle`'s bytes to the unreliable datagram (BUILTIN-12.5).
    pub(crate) fn mirror_particle(&mut self) {
        let org = self.context.parm_vector(PARM0);
        let dir = self.context.parm_vector(PARM1);
        let color = float_to_int(self.context.parm_float(PARM2));
        let count = float_to_int(self.context.parm_float(PARM3));
        let datagram = &mut self.sim.messages.datagram;
        datagram.write_byte(SVC_PARTICLE);
        for c in org {
            datagram.write_coord(c);
        }
        for d in dir {
            datagram.write_byte(
                float_to_int(d * PARTICLE_DIR_SCALE).clamp(i32::from(i8::MIN), i32::from(i8::MAX)),
            );
        }
        datagram.write_byte(count);
        datagram.write_byte(color);
    }

    /// `lightstyle`'s bytes to every active or spawned client while the server is active
    /// (BUILTIN-12.7).
    pub(crate) fn mirror_lightstyle(&mut self) {
        if !self.sim.mock.engine().server_active {
            return;
        }
        let style = float_to_int(self.context.parm_float(PARM0));
        let value = self.string_text(self.context.parm_raw(PARM1));
        let client = &mut self.sim.messages.client;
        if client.active || client.spawned {
            client.message.write_byte(SVC_LIGHTSTYLE);
            client.message.write_byte(style);
            client.message.write_string(&value);
        }
    }

    /// `makestatic`'s bytes to the signon buffer (BUILTIN-12.8), read from the edict before
    /// the mock frees it.
    pub(crate) fn static_bytes(&self) -> Result<Vec<u8>, RunError> {
        let ent = self.edict_of(self.context.parm_raw(PARM0));
        let model = self.string_text(self.entity_raw(ent, "model"));
        let mut out = Message::new(NET_SIGNON_SIZE, Overflow::Fatal);
        out.write_byte(SVC_SPAWNSTATIC);
        out.write_byte(self.model_index(&model)?);
        out.write_byte(float_to_int(self.float(ent, "frame")));
        out.write_byte(float_to_int(self.float(ent, "colormap")));
        out.write_byte(float_to_int(self.float(ent, "skin")));
        let origin = self.vector(ent, "origin");
        let angles = self.vector(ent, "angles");
        for (o, a) in origin.into_iter().zip(angles) {
            out.write_coord(o);
            out.write_angle(a);
        }
        Ok(out.take())
    }

    /// The buffer a `Write*` builtin's PARM0 names (BUILTIN-13.1), `None` for a client
    /// other than ours.
    fn write_dest(&mut self) -> Option<&mut Message> {
        let to = float_to_int(self.context.parm_float(PARM0));
        let msg_entity = self.edict_of(self.global("msg_entity").as_i32());
        let messages = &mut self.sim.messages;
        match to {
            0 => Some(&mut messages.datagram),
            1 => (msg_entity == 1).then_some(&mut messages.client.message),
            2 => Some(&mut messages.reliable_datagram),
            3 => Some(&mut messages.signon),
            _ => None,
        }
    }

    /// A `Write*` builtin's bytes (BUILTIN-13.2), `number` its builtin number.
    pub(crate) fn mirror_write(&mut self, number: u32) {
        let value = self.context.parm_float(PARM1);
        let raw = self.context.parm_raw(PARM1);
        let text = (number == WRITE_STRING).then(|| self.written_text(raw));
        let entity =
            (number == WRITE_ENTITY).then(|| i32::try_from(self.edict_of(raw)).unwrap_or(0));
        let Some(message) = self.write_dest() else {
            return;
        };
        match number {
            WRITE_BYTE | WRITE_CHAR => message.write_byte(float_to_int(value)),
            WRITE_SHORT => message.write_short(float_to_int(value)),
            WRITE_LONG => message.write_long(float_to_int(value)),
            WRITE_COORD => message.write_coord(value),
            WRITE_ANGLE => message.write_angle(value),
            WRITE_STRING => message.write_string(&text.unwrap_or_default()),
            WRITE_ENTITY => message.write_short(entity.unwrap_or(0)),
            _ => {}
        }
    }

    // ---- connecting (NET-2) ----

    /// `SV_CreateBaseline` (NET-2.1): every edict in use that is a client slot or has a
    /// model, into the signon buffer.
    pub(crate) fn create_baselines(&mut self) -> Result<(), RunError> {
        let count = self.context.machine_view().edicts().num_edicts();
        let player = self.model_index(b"progs/player.mdl");
        let mut baselines = vec![Baseline::default(); count];
        for n in 0..count {
            if self
                .context
                .machine_view()
                .edicts()
                .is_free(EdictNumber::new(n))
                != Some(false)
            {
                continue;
            }
            let client = self.is_client_slot(n);
            if n > self.maxclients() && self.float(n, "modelindex") == 0.0 {
                continue;
            }
            let (colormap, modelindex) = if client {
                (i32::try_from(n).unwrap_or(0), player.clone()?)
            } else {
                (
                    0,
                    self.model_index(&self.string_text(self.entity_raw(n, "model")))?,
                )
            };
            let baseline = Baseline {
                origin: self.vector(n, "origin"),
                angles: self.vector(n, "angles"),
                modelindex,
                frame: float_to_int(self.float(n, "frame")),
                colormap,
                skin: float_to_int(self.float(n, "skin")),
                effects: 0,
            };
            if let Some(slot) = baselines.get_mut(n) {
                *slot = baseline;
            }
            let signon = &mut self.sim.messages.signon;
            signon.write_byte(SVC_SPAWNBASELINE);
            signon.write_short(i32::try_from(n).unwrap_or(0));
            signon.write_byte(baseline.modelindex);
            signon.write_byte(baseline.frame);
            signon.write_byte(baseline.colormap);
            signon.write_byte(baseline.skin);
            for (o, a) in baseline.origin.into_iter().zip(baseline.angles) {
                signon.write_coord(o);
                signon.write_angle(a);
            }
        }
        self.sim.messages.baselines = baselines;
        Ok(())
    }

    /// `SV_SendServerinfo` (NET-2.2), to the client of edict `client`.
    pub(crate) fn send_serverinfo(&mut self, client: usize) {
        let engine = self.sim.mock.engine();
        let cvar = |name: &str| {
            engine
                .cvars
                .text(name)
                .map_or(0.0, |t| c_atof(t.as_bytes()))
        };
        let game = if cvar("coop") == 0.0 && cvar("deathmatch") != 0.0 {
            NET_GAME_DEATHMATCH
        } else {
            NET_GAME_COOP
        };
        let maxclients = i32::try_from(self.maxclients()).unwrap_or(0);
        let models: Vec<Vec<u8>> = engine
            .model_precache
            .iter()
            .skip(1)
            .take_while(|m| !m.is_empty())
            .map(|m| m.as_bytes().to_vec())
            .collect();
        let sounds: Vec<Vec<u8>> = engine
            .sound_precache
            .iter()
            .skip(1)
            .take_while(|s| !s.is_empty())
            .map(|s| s.as_bytes().to_vec())
            .collect();
        let crc = self.sim.messages.crc;
        let message_text = self.string_text(self.entity_raw(0, "message"));
        let track = float_to_int(self.float(0, "sounds"));
        let slot = &mut self.sim.messages.client;
        let message = &mut slot.message;
        message.write_byte(SVC_PRINT);
        message.write_string(format!("\x02\nVERSION {NET_VERSION} SERVER ({crc} CRC)").as_bytes());
        message.write_byte(SVC_SERVERINFO);
        message.write_long(NET_PROTOCOL_VERSION);
        message.write_byte(maxclients);
        message.write_byte(game);
        message.write_string(&message_text);
        for model in &models {
            message.write_string(model);
        }
        message.write_byte(0);
        for sound in &sounds {
            message.write_string(sound);
        }
        message.write_byte(0);
        message.write_byte(SVC_CDTRACK);
        message.write_byte(track);
        message.write_byte(track);
        message.write_byte(SVC_SETVIEW);
        message.write_short(i32::try_from(client).unwrap_or(0));
        message.write_byte(SVC_SIGNONNUM);
        message.write_byte(1);
        slot.sendsignon = true;
        slot.spawned = false;
    }

    /// `Host_PreSpawn_f` (NET-2.3).
    pub(crate) fn prespawn(&mut self) {
        let signon = self.sim.messages.signon.bytes().to_vec();
        let slot = &mut self.sim.messages.client;
        slot.message.write_bytes(&signon);
        slot.message.write_byte(SVC_SIGNONNUM);
        slot.message.write_byte(2);
        slot.sendsignon = true;
    }

    /// A signon stage's message sent, as `SV_SendClientMessages` sends it at the end of the
    /// stage's frame when `sendsignon` is set (NET-1.3).
    pub(crate) fn flush_signon(&mut self) {
        let slot = &mut self.sim.messages.client;
        if slot.sendsignon && !slot.message.is_empty() {
            let packet = slot.message.take();
            self.sim.messages.outbox.push(packet);
        }
        slot.sendsignon = false;
    }

    /// `Host_Spawn_f`'s message, after the client has entered (NET-2.4).
    pub(crate) fn spawn_message(&mut self, client: usize) -> Result<(), RunError> {
        let time = double_to_float(self.clock());
        let styles: Vec<Vec<u8>> = (0..MAX_LIGHTSTYLES)
            .map(|i| {
                self.sim
                    .mock
                    .engine()
                    .lightstyles
                    .get(i)
                    .and_then(|s| s.as_ref())
                    .map(|s| s.as_bytes().to_vec())
                    .unwrap_or_default()
            })
            .collect();
        let stats = [
            (STAT_TOTALSECRETS, "total_secrets"),
            (STAT_TOTALMONSTERS, "total_monsters"),
            (STAT_SECRETS, "found_secrets"),
            (STAT_MONSTERS, "killed_monsters"),
        ]
        .map(|(stat, name)| (stat, float_to_int(self.global(name).as_f32())));
        let angles = self.vector(client, "angles");
        let maxclients = self.maxclients();
        {
            let slot = &mut self.sim.messages.client;
            let (name, colors, old_frags) = (slot.name.clone(), slot.colors, slot.old_frags);
            let message = &mut slot.message;
            // "send all current names, colors, and frag counts": `SZ_Clear` first, so what
            // `ClientConnect` and `PutClientInServer` wrote to the client is dropped.
            message.clear();
            message.write_byte(SVC_TIME);
            message.write_float(time);
            for i in 0..maxclients {
                let i = i32::try_from(i).unwrap_or(0);
                message.write_byte(SVC_UPDATENAME);
                message.write_byte(i);
                message.write_string(&name);
                message.write_byte(SVC_UPDATEFRAGS);
                message.write_byte(i);
                message.write_short(old_frags);
                message.write_byte(SVC_UPDATECOLORS);
                message.write_byte(i);
                message.write_byte(colors);
            }
            for (i, style) in styles.iter().enumerate() {
                message.write_byte(SVC_LIGHTSTYLE);
                message.write_byte(i32::try_from(i).unwrap_or(0));
                message.write_string(style);
            }
            for (stat, value) in stats {
                message.write_byte(SVC_UPDATESTAT);
                message.write_byte(stat);
                message.write_long(value);
            }
            message.write_byte(SVC_SETANGLE);
            message.write_angle(angles[0]);
            message.write_angle(angles[1]);
            message.write_angle(0.0);
        }
        let mut data = Message::new(NET_MAX_MSGLEN, Overflow::Allowed);
        self.write_clientdata(client, &mut data)?;
        let slot = &mut self.sim.messages.client;
        slot.message.write_bytes(data.bytes());
        slot.message.write_byte(SVC_SIGNONNUM);
        slot.message.write_byte(SIGNON_BEGIN);
        slot.sendsignon = true;
        Ok(())
    }

    /// `Host_Begin_f` (NET-2.5), and the signon messages sent as one reliable packet: the
    /// local client answered each stage at once, so the datagrams start with the next
    /// frame.
    pub(crate) fn begin(&mut self) {
        let slot = &mut self.sim.messages.client;
        slot.spawned = true;
        slot.sendsignon = false;
        if !slot.message.is_empty() {
            let packet = slot.message.take();
            self.sim.messages.outbox.push(packet);
        }
    }

    // ---- each frame (NET-1, NET-3, NET-4) ----

    /// `SV_SendClientMessages` (NET-1.3) for the one client: the reliable updates, its
    /// datagram when it is spawned, its message when it holds anything, then the muzzle
    /// flashes cleared.
    pub(crate) fn send_client_messages(&mut self) -> Result<(), RunError> {
        self.update_to_reliable_messages();
        let slot = &self.sim.messages.client;
        if slot.active {
            if slot.spawned {
                let datagram = self.client_datagram(1)?;
                self.sim.messages.outbox.push(datagram);
            }
            let slot = &mut self.sim.messages.client;
            if slot.spawned || slot.sendsignon {
                if slot.message.overflowed() {
                    // `SV_DropClient` and `overflowed = false`: the one client stays, its
                    // message starts over.
                    slot.message.clear();
                    slot.message.reset_overflow();
                } else if !slot.message.is_empty() {
                    let packet = slot.message.take();
                    self.sim.messages.outbox.push(packet);
                }
                slot.sendsignon = false;
            }
        }
        // SV_CleanupEnts.
        let count = self.context.machine_view().edicts().num_edicts();
        // `ent->v.effects = (int)ent->v.effects & ~EF_MUZZLEFLASH` for every edict: a
        // fractional `effects` is truncated whether or not the flash is set.
        for n in 1..count {
            let effects = self.float(n, "effects");
            let cleaned = int_to_float(float_to_int(effects) & !EF_MUZZLEFLASH);
            if cleaned.to_bits() != effects.to_bits() {
                self.set_float(n, "effects", cleaned);
            }
        }
        Ok(())
    }

    /// `SV_UpdateToReliableMessages` (NET-1.4).
    fn update_to_reliable_messages(&mut self) {
        let frags = float_to_int(self.float(1, "frags"));
        let reliable = self.sim.messages.reliable_datagram.take();
        let slot = &mut self.sim.messages.client;
        if slot.old_frags != frags && slot.active {
            slot.message.write_byte(SVC_UPDATEFRAGS);
            slot.message.write_byte(0);
            slot.message.write_short(frags);
        }
        slot.old_frags = frags;
        if slot.active {
            slot.message.write_bytes(&reliable);
        }
    }

    /// `SV_SendClientDatagram` (NET-1.5): the datagram for the client of edict `client`.
    fn client_datagram(&mut self, client: usize) -> Result<Vec<u8>, RunError> {
        let mut msg = Message::new(NET_MAX_DATAGRAM, Overflow::Fatal);
        msg.write_byte(SVC_TIME);
        msg.write_float(double_to_float(self.clock()));
        self.write_clientdata(client, &mut msg)?;
        self.write_entities(client, &mut msg);
        let datagram = &self.sim.messages.datagram;
        if msg.len().saturating_add(datagram.len()) < msg.max() {
            msg.write_bytes(datagram.bytes());
        }
        Ok(msg.take())
    }

    /// `SV_WriteEntitiesToClient` (NET-3.2, NET-3.3).
    fn write_entities(&mut self, client: usize, msg: &mut Message) {
        let origin = self.vector(client, "origin");
        let view_ofs = self.vector(client, "view_ofs");
        let [ox, oy, oz] = origin;
        let [vx, vy, vz] = view_ofs;
        let eye = [ox + vx, oy + vy, oz + vz];
        let pvs = self.sim.messages.vis.fat_pvs(eye);
        let count = self.context.machine_view().edicts().num_edicts();
        for e in 1..count {
            if e != client {
                if self.float(e, "modelindex") == 0.0
                    || self.string_text(self.entity_raw(e, "model")).is_empty()
                {
                    continue;
                }
                let visible = self.sim.collision.leafs_of(e).iter().any(|&leaf| {
                    usize::try_from(leaf).ok().is_some_and(|leaf| {
                        pvs.get(leaf >> BYTE_SHIFT)
                            .is_some_and(|byte| byte & (1 << (leaf & BIT_MASK)) != 0)
                    })
                });
                if !visible {
                    continue;
                }
            }
            if msg.max().saturating_sub(msg.len()) < NET_PACKET_SLACK {
                return;
            }
            let baseline = self
                .sim
                .messages
                .baselines
                .get(e)
                .copied()
                .unwrap_or_default();
            let origin = self.vector(e, "origin");
            let angles = self.vector(e, "angles");
            let mut bits = 0;
            for (i, (o, b)) in origin.into_iter().zip(baseline.origin).enumerate() {
                let miss = o - b;
                if f64::from(miss) < -ORIGIN_TOLERANCE || f64::from(miss) > ORIGIN_TOLERANCE {
                    bits |= U_ORIGIN1 << i;
                }
            }
            #[expect(
                clippy::float_cmp,
                reason = "spec NET-3.3: an angle and a field are sent when they differ from the baseline exactly"
            )]
            {
                if angles[0] != baseline.angles[0] {
                    bits |= U_ANGLE1;
                }
                if angles[1] != baseline.angles[1] {
                    bits |= U_ANGLE2;
                }
                if angles[2] != baseline.angles[2] {
                    bits |= U_ANGLE3;
                }
                if self.movetype(e) == MoveType::Step {
                    bits |= U_NOLERP;
                }
                if int_to_float(baseline.colormap) != self.float(e, "colormap") {
                    bits |= U_COLORMAP;
                }
                if int_to_float(baseline.skin) != self.float(e, "skin") {
                    bits |= U_SKIN;
                }
                if int_to_float(baseline.frame) != self.float(e, "frame") {
                    bits |= U_FRAME;
                }
                if int_to_float(baseline.effects) != self.float(e, "effects") {
                    bits |= U_EFFECTS;
                }
                if int_to_float(baseline.modelindex) != self.float(e, "modelindex") {
                    bits |= U_MODEL;
                }
            }
            if e >= LONG_ENTITY {
                bits |= U_LONGENTITY;
            }
            if bits >= MORE_BITS {
                bits |= U_MOREBITS;
            }
            msg.write_byte(bits | U_SIGNAL);
            if bits & U_MOREBITS != 0 {
                msg.write_byte(bits >> SECOND_BYTE_SHIFT);
            }
            let number = i32::try_from(e).unwrap_or(0);
            if bits & U_LONGENTITY != 0 {
                msg.write_short(number);
            } else {
                msg.write_byte(number);
            }
            for (bit, field) in [
                (U_MODEL, "modelindex"),
                (U_FRAME, "frame"),
                (U_COLORMAP, "colormap"),
                (U_SKIN, "skin"),
                (U_EFFECTS, "effects"),
            ] {
                if bits & bit != 0 {
                    msg.write_byte(float_to_int(self.float(e, field)));
                }
            }
            let axes = origin
                .into_iter()
                .zip(angles)
                .zip([U_ANGLE1, U_ANGLE2, U_ANGLE3]);
            for (i, ((o, a), angle_bit)) in axes.enumerate() {
                if bits & (U_ORIGIN1 << i) != 0 {
                    msg.write_coord(o);
                }
                if bits & angle_bit != 0 {
                    msg.write_angle(a);
                }
            }
        }
    }

    /// `SV_WriteClientdataToMessage` (NET-4).
    pub(crate) fn write_clientdata(
        &mut self,
        ent: usize,
        msg: &mut Message,
    ) -> Result<(), RunError> {
        // NET-4.1: the damage message.
        let dmg_take = self.float(ent, "dmg_take");
        let dmg_save = self.float(ent, "dmg_save");
        if dmg_take != 0.0 || dmg_save != 0.0 {
            let other = self.edict_of(self.entity_raw(ent, "dmg_inflictor"));
            let centre = self.centre(other);
            msg.write_byte(SVC_DAMAGE);
            msg.write_byte(float_to_int(dmg_save));
            msg.write_byte(float_to_int(dmg_take));
            for c in centre {
                msg.write_coord(c);
            }
            self.set_float(ent, "dmg_take", 0.0);
            self.set_float(ent, "dmg_save", 0.0);
        }
        // NET-4.2.
        self.set_ideal_pitch(ent)?;
        // NET-4.3: a fixangle, once.
        if self.float(ent, "fixangle") != 0.0 {
            msg.write_byte(SVC_SETANGLE);
            let angles = self.vector(ent, "angles");
            for angle in angles {
                msg.write_angle(angle);
            }
            self.sim.messages.set_angle = Some(angles.map(wire_angle));
            self.set_float(ent, "fixangle", 0.0);
        }
        // NET-4.4: the bits.
        let view_ofs = self.vector(ent, "view_ofs");
        let idealpitch = self.float(ent, "idealpitch");
        let punchangle = self.vector(ent, "punchangle");
        let velocity = self.vector(ent, "velocity");
        let weaponframe = self.float(ent, "weaponframe");
        let armorvalue = self.float(ent, "armorvalue");
        let mut bits = 0;
        let [_, _, viewheight] = view_ofs;
        #[expect(
            clippy::float_cmp_const,
            reason = "spec NET-4.4: SU_VIEWHEIGHT is set when view_ofs.z is not exactly 22"
        )]
        if viewheight != DEFAULT_VIEWHEIGHT {
            bits |= SU_VIEWHEIGHT;
        }
        if idealpitch != 0.0 {
            bits |= SU_IDEALPITCH;
        }
        // NET-4.5: the items.
        let items = float_to_int(self.float(ent, "items"));
        let items = match self.progs_field(ent, "items2") {
            Some(items2) => items | (float_to_int(items2.as_f32()) << ITEMS2_SHIFT),
            None => {
                items | (float_to_int(self.global("serverflags").as_f32()) << SERVERFLAGS_SHIFT)
            }
        };
        bits |= SU_ITEMS;
        if self.flags(ent) & FL_ONGROUND != 0 {
            bits |= SU_ONGROUND;
        }
        if self.float(ent, "waterlevel") >= 2.0 {
            bits |= SU_INWATER;
        }
        for (i, (p, v)) in punchangle.into_iter().zip(velocity).enumerate() {
            if p != 0.0 {
                bits |= SU_PUNCH1 << i;
            }
            if v != 0.0 {
                bits |= SU_VELOCITY1 << i;
            }
        }
        if weaponframe != 0.0 {
            bits |= SU_WEAPONFRAME;
        }
        if armorvalue != 0.0 {
            bits |= SU_ARMOR;
        }
        bits |= SU_WEAPON;
        msg.write_byte(SVC_CLIENTDATA);
        msg.write_short(bits);
        if bits & SU_VIEWHEIGHT != 0 {
            msg.write_byte(float_to_int(viewheight));
        }
        if bits & SU_IDEALPITCH != 0 {
            msg.write_byte(float_to_int(idealpitch));
        }
        for (i, (p, v)) in punchangle.into_iter().zip(velocity).enumerate() {
            if bits & (SU_PUNCH1 << i) != 0 {
                msg.write_byte(float_to_int(p));
            }
            if bits & (SU_VELOCITY1 << i) != 0 {
                msg.write_byte(float_to_int(v / VELOCITY_SCALE));
            }
        }
        msg.write_long(items);
        if bits & SU_WEAPONFRAME != 0 {
            msg.write_byte(float_to_int(weaponframe));
        }
        if bits & SU_ARMOR != 0 {
            msg.write_byte(float_to_int(armorvalue));
        }
        if bits & SU_WEAPON != 0 {
            let model = self.string_text(self.entity_raw(ent, "weaponmodel"));
            msg.write_byte(self.model_index(&model)?);
        }
        msg.write_short(float_to_int(self.float(ent, "health")));
        for field in [
            "currentammo",
            "ammo_shells",
            "ammo_nails",
            "ammo_rockets",
            "ammo_cells",
        ] {
            msg.write_byte(float_to_int(self.float(ent, field)));
        }
        let weapon = float_to_int(self.float(ent, "weapon"));
        if self.sim.messages.game == GameKind::Standard {
            msg.write_byte(weapon);
        } else if let Some(bit) = (0..ITEM_BITS).find(|&i| weapon & (1 << i) != 0) {
            msg.write_byte(bit);
        }
        Ok(())
    }

    /// `SV_SetIdealPitch` (NET-4.2).
    fn set_ideal_pitch(&mut self, ent: usize) -> Result<(), RunError> {
        if self.flags(ent) & FL_ONGROUND == 0 {
            return Ok(());
        }
        let [ox, oy, oz] = self.vector(ent, "origin");
        let [_, _, view_z] = self.vector(ent, "view_ofs");
        let [_, yaw, _] = self.vector(ent, "angles");
        let angleval = double_to_float(
            f64::from(yaw) * core::f64::consts::PI * 2.0 / f64::from(FULL_TURN_DEGREES),
        );
        let sinval = double_to_float(f64::from(angleval).sin());
        let cosval = double_to_float(f64::from(angleval).cos());
        let mut z = [0.0_f32; NET_MAX_FORWARD];
        for (i, zi) in z.iter_mut().enumerate() {
            // `cosval*(i+3)*12`: the float times the int, then times 12, left to right.
            let steps = int_to_float(i32::try_from(i).unwrap_or(0).saturating_add(PROBE_FIRST));
            let top_z = oz + view_z;
            let top = [
                ox + cosval * steps * PROBE_SPACING,
                oy + sinval * steps * PROBE_SPACING,
                top_z,
            ];
            let bottom_z = top_z - PROBE_DEPTH;
            let bottom = [
                ox + cosval * steps * PROBE_SPACING,
                oy + sinval * steps * PROBE_SPACING,
                bottom_z,
            ];
            let trace = self.trace((top, POINT, POINT, bottom), MoveKind::NoMonsters, ent)?;
            if trace.allsolid {
                return Ok(());
            }
            #[expect(
                clippy::float_cmp,
                reason = "spec NET-4.2: a trace whose fraction is exactly 1 is a dropoff"
            )]
            if trace.fraction == 1.0 {
                return Ok(());
            }
            *zi = top_z + trace.fraction * (bottom_z - top_z);
        }
        let mut dir = 0_i32;
        let mut steps = 0_i32;
        for pair in z.windows(2) {
            let (Some(&previous), Some(&next)) = (pair.first(), pair.get(1)) else {
                break;
            };
            let step = float_to_int(next - previous);
            if step == 0 {
                continue;
            }
            if dir != 0 && step != dir {
                return Ok(());
            }
            steps = steps.saturating_add(1);
            dir = step;
        }
        if dir == 0 {
            self.set_float(ent, "idealpitch", 0.0);
            return Ok(());
        }
        if steps < 2 {
            return Ok(());
        }
        let scale = self.setting(Setting::IdealPitchScale);
        self.set_float(ent, "idealpitch", -int_to_float(dir) * scale);
        Ok(())
    }
}

/// A world's messages to its client, in sending order (NET-1.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientMessages {
    /// Each packet: a reliable message or a datagram, oldest first.
    pub packets: Vec<Vec<u8>>,
}

#[cfg(test)]
mod tests;
