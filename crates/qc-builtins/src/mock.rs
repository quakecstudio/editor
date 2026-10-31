// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_precache_model, PF_precache_sound, PF_lightstyle, PF_cvar, PF_cvar_set

//! The deterministic mock engine of `differential.md` (DIFF-6, DIFF-7).
//!
//! Most of this is a design of this project, not a port: the fixture and the world builtins'
//! answers are decisions made so two independent runners produce the same trace (DIFF-6
//! Source). Where the mock does what id's server does, the method cites the function.
//!
//! The fixture holds the engine state a class `state` builtin reads or writes as plain data
//! (BUILTIN-4.2): the precache lists and model bounds, the lightstyles, the cvars, the
//! `changelevel` flag, the command buffer and the sizes of the three message buffers
//! (DIFF-6.5), and the DIFF-6.2 random source.

/// The settings the mock keeps, with the texts of DIFF-2.4.
///
/// `skill`, `deathmatch`, `coop` and `teamplay` start at `1`, `0`, `0`, `0`; a scenario's
/// `cvar` line replaces a text. Any other name reads as 0 and `cvar_set` of it changes nothing
/// (BUILTIN-11.8, DIFF-2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cvars {
    entries: Vec<(String, String)>,
}

impl Default for Cvars {
    fn default() -> Self {
        Self {
            entries: vec![
                ("skill".to_string(), "1".to_string()),
                ("deathmatch".to_string(), "0".to_string()),
                ("coop".to_string(), "0".to_string()),
                ("teamplay".to_string(), "0".to_string()),
            ],
        }
    }
}

impl Cvars {
    /// The text of the setting named `name`, or `None` when there is none (BUILTIN-11.7).
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Sets the text of `name` when it is a known setting, else does nothing (BUILTIN-11.8,
    /// DIFF-2.4). Returns whether the setting existed.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|(key, _)| key == name) {
            entry.1 = value.to_string();
            true
        } else {
            false
        }
    }

    /// Adds the setting `name` with `value` unless it exists, vkQuake's `Cvar_Create`
    /// (BUILTIN-18.6).
    pub fn create(&mut self, name: &str, value: &str) {
        if self.text(name).is_none() {
            self.entries.push((name.to_owned(), value.to_owned()));
        }
    }
}

/// The bounds of a model list entry, for `setmodel` (BUILTIN-9.4, DIFF-6.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBounds {
    /// The model's mins.
    pub mins: [f32; 3],
    /// The model's maxs.
    pub maxs: [f32; 3],
}

/// The deterministic random source of DIFF-6.2: a 32-bit linear congruential state.
///
/// `state = state * 1103515245 + 12345` modulo 2^32; each draw returns `(state >> 16) & 0x7fff`.
/// `random` (BUILTIN-6.6) is the only mock builtin that draws from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u32,
}

/// The multiplier of DIFF-6.2, `DIFF_RAND_MULTIPLIER`.
pub const DIFF_RAND_MULTIPLIER: u32 = 1_103_515_245;
/// The increment of DIFF-6.2, `DIFF_RAND_INCREMENT`.
pub const DIFF_RAND_INCREMENT: u32 = 12345;

impl Rng {
    /// A source seeded with the scenario's `seed` (DIFF-3.2, DIFF-2.2).
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// One draw: advances the state and returns `(state >> 16) & 0x7fff` (DIFF-6.2).
    pub fn next_rand(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(DIFF_RAND_MULTIPLIER)
            .wrapping_add(DIFF_RAND_INCREMENT);
        (self.state >> 16) & 0x7fff
    }
}

/// The three output buffers whose sizes the mock keeps (DIFF-6.5).
///
/// The mock keeps only the byte count of each: the unreliable datagram and the reliable
/// datagram (`LIMIT_MAX_DATAGRAM`, 1024) and the signon buffer (`BUILTIN_SIGNON_SIZE`, 8192).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buffers {
    /// Bytes in the unreliable datagram (`MSG_BROADCAST`).
    pub unreliable: usize,
    /// Bytes in the reliable datagram (`MSG_ALL`).
    pub reliable: usize,
    /// Bytes in the signon buffer (`MSG_INIT`).
    pub signon: usize,
}

/// `LIMIT_MAX_DATAGRAM`, the datagram size (BUILTIN-13.3).
pub const LIMIT_MAX_DATAGRAM: usize = 1024;
/// `BUILTIN_SIGNON_SIZE`, the signon buffer size (BUILTIN-13.3).
pub const BUILTIN_SIGNON_SIZE: usize = 8192;
/// `BUILTIN_CMD_TEXT_SIZE`, the command buffer size (BUILTIN-11.5).
pub const BUILTIN_CMD_TEXT_SIZE: usize = 8192;

/// `LIMIT_MAX_SOUNDS`, the sound precache list size (LIMIT-6.2), id's `MAX_SOUNDS`.
pub const LIMIT_MAX_SOUNDS: usize = 256;
/// `LIMIT_MAX_MODELS`, the model precache list size (LIMIT-6.2), id's `MAX_MODELS`.
pub const LIMIT_MAX_MODELS: usize = 256;

/// vkQuake's `MAX_SOUNDS` (BUILTIN-18.3).
pub const VKQUAKE_MAX_SOUNDS: usize = 2048;
/// vkQuake's `MAX_MODELS` (BUILTIN-18.3).
pub const VKQUAKE_MAX_MODELS: usize = 8192;

/// Whose rules the mock follows where engines differ (BUILTIN-18).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EngineProfile {
    /// id's engine: its builtin table and its limits.
    #[default]
    Id,
    /// vkQuake's: every builtin number reaches the dispatch and its precache lists are longer
    /// (BUILTIN-18.1, BUILTIN-18.3).
    VkQuake,
}

impl EngineProfile {
    /// The builtin numbers the engine's table has, as [`qc_vm::Builtins::limit`] gives them
    /// (VM-14.7): id's [`qc_vm::LIMIT_NUM_BUILTINS`], or none for vkQuake (BUILTIN-18.1).
    #[must_use]
    pub const fn builtin_limit(self) -> Option<u32> {
        match self {
            Self::Id => Some(qc_vm::LIMIT_NUM_BUILTINS),
            Self::VkQuake => None,
        }
    }

    /// The size of precache list `kind` (LIMIT-6.2, BUILTIN-18.3).
    #[must_use]
    pub const fn precache_limit(self, kind: Precache) -> usize {
        match (self, kind) {
            (Self::Id, Precache::Sound) => LIMIT_MAX_SOUNDS,
            (Self::Id, Precache::Model) => LIMIT_MAX_MODELS,
            (Self::VkQuake, Precache::Sound) => VKQUAKE_MAX_SOUNDS,
            (Self::VkQuake, Precache::Model) => VKQUAKE_MAX_MODELS,
        }
    }
}

/// One of the two precache lists (BUILTIN-10, LIMIT-6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precache {
    /// The sound list, `precache_sound`.
    Sound,
    /// The model list, `precache_model`.
    Model,
}

/// An append that did not fit: a full precache list (LIMIT-6.2), message buffer
/// (BUILTIN-13.3) or command buffer (BUILTIN-11.5). Nothing was appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow;

/// A network message destination id's `WriteDest` resolves `to` to (BUILTIN-13.1).
///
/// A client slot's reliable message ([`MessageDest::One`]) is not kept by the mock and never
/// overflows (DIFF-6.5); the other three are counted in [`Buffers`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageDest {
    /// 0 `MSG_BROADCAST`: the unreliable datagram (BUILTIN-13.1).
    Broadcast,
    /// 1 `MSG_ONE`: the reliable message of `msg_entity`'s client slot (BUILTIN-13.1).
    One,
    /// 2 `MSG_ALL`: the reliable datagram (BUILTIN-13.1).
    All,
    /// 3 `MSG_INIT`: the signon buffer (BUILTIN-13.1).
    Init,
}

/// The mock engine fixture (DIFF-6). It answers the class `world` builtins by the
/// deterministic rules of DIFF-7.
#[derive(Clone, Debug, PartialEq)]
pub struct MockEngine {
    /// The sound precache list, entry 0 the empty string (DIFF-3.3, BUILTIN-10.3).
    pub sound_precache: Vec<String>,
    /// The model precache list: entry 0 empty, 1 the world, 2.. the submodels (DIFF-3.3).
    pub model_precache: Vec<String>,
    /// The bounds of each named model for `setmodel` (DIFF-6.3), when a `model` directive gave
    /// them; a name not here uses the DIFF-6.3 default rule.
    pub model_bounds: Vec<(String, ModelBounds)>,
    /// The 64 lightstyles, each a string or none (BUILTIN-12.6, DIFF-3.2).
    pub lightstyles: Vec<Option<String>>,
    /// The settings (DIFF-2.4).
    pub cvars: Cvars,
    /// Whether a `changelevel` was issued since the map started (BUILTIN-11.6, DIFF-6.4).
    pub changelevel_issued: bool,
    /// The command buffer `localcmd` and `changelevel` append to; never run (DIFF-6.4). Kept
    /// as bytes so an overflow is detected (BUILTIN-11.5).
    pub command_buffer: Vec<u8>,
    /// The three message buffer sizes (DIFF-6.5).
    pub buffers: Buffers,
    /// The random source (DIFF-6.2).
    pub rng: Rng,
    /// The engine-made strings, by their value (EDICT-13.1, EDICT-13.3).
    pub strings: EngineStrings,
    /// `checkclient`'s last choice and the clock at which it was made (DIFF-7.3).
    pub checkclient: CheckClient,
    /// Whether each client slot (1 to `maxclients`) has a player in it (DIFF-4.2).
    ///
    /// Index `n - 1` is client slot `n`; a slot that is active and spawned receives the
    /// printing and network builtins (BUILTIN-11, BUILTIN-12.7). The VM test environment that
    /// runs a single builtin leaves them all empty.
    pub clients_active: Vec<bool>,
    /// Whether the server is active, id's `sv.active` (BUILTIN-11.8, BUILTIN-12.7).
    ///
    /// `false` while the map loads (DIFF-3.2) and `true` from the settle frames on (DIFF-3.6);
    /// it decides whether `cvar_set` broadcasts and whether `lightstyle` sends.
    pub server_active: bool,
    /// Whose rules the mock follows (BUILTIN-18); id's unless the caller chooses another.
    pub profile: EngineProfile,
    /// The server clock `sv.time`, a `double` (DIFF-3.2): 1.0 at map start.
    ///
    /// `spawn` and `remove` read it for the edict reuse policy and `freetime` (EDICT-6,
    /// EDICT-7); `checkclient` reads it for its reconsider interval (DIFF-7.3). The runner
    /// advances it each frame (DIFF-5.2); the VM test environment leaves it at 1.0.
    pub clock: f64,
}

/// The lightstyle count id does not range-check against, `LIMIT_MAX_LIGHTSTYLES` (BUILTIN-12.6).
pub const LIMIT_MAX_LIGHTSTYLES: usize = 64;

/// A client slot's reliable message is `LIMIT_MAX_MSGLEN` bytes, id's `MAX_MSGLEN` (BUILTIN-13.4).
///
/// The mock does not keep it (DIFF-6.5), so this names the size for documentation only.
pub const LIMIT_MAX_MSGLEN: usize = 8000;

/// The engine-made strings the mock keeps, by their value (EDICT-13.1, EDICT-13.3).
///
/// An engine-made string is a value that cannot be a string-table offset; the mock uses the
/// negative values -1, -2, ... in the order the strings are made, and the differential
/// comparison compares their text, not their numbers (EDICT-13.3, DIFF-9.4). `ftos` and `vtos`
/// share one buffer value, so each call overwrites the previous text (BUILTIN-7.4).
///
/// It also notes when the buffer's text changes, so the differential host can write a string
/// record for it (TRACE-4.10, DIFF-3.9), as the oracle's `qs_engine_string_touched` marks the
/// buffer for its next change-carrying record ([`EngineStrings::take_temp_change`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStrings {
    /// The `ftos`/`vtos` buffer value, once allocated (BUILTIN-7.4).
    temp_value: Option<i32>,
    /// Every engine string's text, by value; the temp buffer's is updated in place.
    entries: Vec<(i32, Vec<u8>)>,
    /// Whether the buffer was allocated or given a different text since the last
    /// `take_temp_change` (DIFF-3.9).
    temp_changed: bool,
    /// The strings `strzone` made that `strunzone` has not freed (BUILTIN-18.9).
    zoned: Vec<i32>,
    /// The strings `strunzone` freed, which are no strings any more (BUILTIN-18.9).
    freed: Vec<i32>,
}

impl EngineStrings {
    /// Sets the one `ftos`/`vtos` temporary buffer to `text` and returns its value (BUILTIN-7.4).
    ///
    /// The first call allocates the value -1; later calls overwrite its text and return the
    /// same value, so an earlier result's text changes (BUILTIN-7.4).
    pub fn set_temp(&mut self, text: Vec<u8>) -> i32 {
        let value = match self.temp_value {
            Some(value) => value,
            None => {
                let value = self.next_value();
                self.temp_value = Some(value);
                self.entries.push((value, Vec::new()));
                self.temp_changed = true;
                value
            }
        };
        if let Some(entry) = self.entries.iter_mut().find(|(key, _)| *key == value)
            && entry.1 != text
        {
            entry.1 = text;
            self.temp_changed = true;
        }
        value
    }

    /// Makes a new engine string holding `text`, with a fresh value (EDICT-13.1).
    ///
    /// Used for `mapname`, `netname` and the world's `model`; its value never changes.
    pub fn make(&mut self, text: Vec<u8>) -> i32 {
        let value = self.next_value();
        self.entries.push((value, text));
        value
    }

    /// Makes a new engine string holding `text` that [`Self::unzone`] may free, `strzone`'s
    /// (BUILTIN-18.9).
    pub fn zone(&mut self, text: Vec<u8>) -> i32 {
        let value = self.make(text);
        self.zoned.push(value);
        value
    }

    /// Frees the string `value` when [`Self::zone`] made it and it is not freed yet; returns
    /// whether it did (BUILTIN-18.9).
    pub fn unzone(&mut self, value: i32) -> bool {
        match self.zoned.iter().position(|&v| v == value) {
            Some(at) => {
                self.zoned.swap_remove(at);
                self.freed.push(value);
                true
            }
            None => false,
        }
    }

    /// The temp buffer's value and text when it was allocated or given a different text since
    /// the last call, else `None`; the note is then cleared (DIFF-3.9).
    pub fn take_temp_change(&mut self) -> Option<(i32, Vec<u8>)> {
        if !core::mem::take(&mut self.temp_changed) {
            return None;
        }
        let value = self.temp_value?;
        Some((value, self.text(value)?.to_vec()))
    }

    /// The text of the engine string `value`, or `None` when no engine string has it.
    #[must_use]
    pub fn text(&self, value: i32) -> Option<&[u8]> {
        if self.freed.contains(&value) {
            return None;
        }
        self.entries
            .iter()
            .find(|(key, _)| *key == value)
            .map(|(_, text)| text.as_slice())
    }

    /// The next free engine-string value: -1, then -2, and so on (EDICT-13.3).
    fn next_value(&mut self) -> i32 {
        let next = self
            .entries
            .len()
            .checked_add(1)
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(1);
        next.checked_neg().unwrap_or(i32::MIN)
    }
}

/// `checkclient`'s stored choice and the clock at which it was made (DIFF-7.3).
///
/// Both start at 0 (DIFF-3.2): the last choice is edict 0 (the world) and the time is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CheckClient {
    /// The edict number of the last client `checkclient` chose, id's `sv.lastcheck`.
    pub last_check: i32,
    /// The clock at the last choice, id's `sv.lastchecktime` (a `double`).
    pub last_check_time: f64,
}

/// The `checkclient` reconsider interval, id's `0.1` seconds (DIFF-7.3, BUILTIN-14.3).
pub const BUILTIN_CHECKCLIENT_INTERVAL: f64 = 0.1;

/// `FL_NOTARGET`, the flag `checkclient` skips (DIFF-7.3), id's `FL_NOTARGET`.
pub const BUILTIN_FL_NOTARGET: i32 = 128;

/// `FL_ONGROUND`, one of the flags `walkmove` and `movetogoal` need (DIFF-7.7).
pub const BUILTIN_FL_ONGROUND: i32 = 512;
/// `FL_FLY`, one of the flags `walkmove` and `movetogoal` need (DIFF-7.7).
pub const BUILTIN_FL_FLY: i32 = 1;
/// `FL_SWIM`, one of the flags `walkmove` and `movetogoal` need (DIFF-7.7).
pub const BUILTIN_FL_SWIM: i32 = 2;

impl MockEngine {
    /// A fresh fixture for a map, before any spawn function runs (DIFF-3.2, DIFF-3.3).
    ///
    /// The sound list holds entry 0; the model list holds entry 0, `maps/<map>.bsp` and the
    /// `submodels` names `*1`..`*n`. The lightstyles are empty, the buffers empty, the
    /// `changelevel` flag clear, and the random source is seeded.
    #[must_use]
    pub fn new(map: &str, submodels: usize, seed: u32) -> Self {
        let mut model_precache = vec![String::new(), format!("maps/{map}.bsp")];
        for n in 1..=submodels {
            model_precache.push(format!("*{n}"));
        }
        Self {
            sound_precache: vec![String::new()],
            model_precache,
            model_bounds: Vec::new(),
            lightstyles: vec![None; LIMIT_MAX_LIGHTSTYLES],
            cvars: Cvars::default(),
            changelevel_issued: false,
            command_buffer: Vec::new(),
            buffers: Buffers::default(),
            rng: Rng::new(seed),
            strings: EngineStrings::default(),
            checkclient: CheckClient::default(),
            clients_active: Vec::new(),
            server_active: false,
            profile: EngineProfile::Id,
            clock: 1.0,
        }
    }

    /// Whether client slot `number` (1 to `maxclients`) has a player (DIFF-4.2).
    ///
    /// A slot outside 1 to `clients_active.len()` is never active.
    #[must_use]
    pub fn client_active(&self, number: usize) -> bool {
        number
            .checked_sub(1)
            .and_then(|index| self.clients_active.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// The list `kind` names.
    fn precache_list(&self, kind: Precache) -> &Vec<String> {
        match kind {
            Precache::Sound => &self.sound_precache,
            Precache::Model => &self.model_precache,
        }
    }

    /// The entry number of `name` in precache list `kind`, looked up from entry 0, or `None`
    /// (BUILTIN-9.3, BUILTIN-12.2). Entry 0 is the empty string.
    #[must_use]
    pub fn precache_index(&self, kind: Precache, name: &str) -> Option<usize> {
        self.precache_list(kind)
            .iter()
            .position(|entry| entry == name)
    }

    /// Appends `name` to precache list `kind` unless it is already there (BUILTIN-10.3).
    ///
    /// # Errors
    ///
    /// [`Overflow`] when the list already has [`EngineProfile::precache_limit`] entries
    /// (LIMIT-6.2, BUILTIN-18.3).
    pub fn precache(&mut self, kind: Precache, name: &str) -> Result<(), Overflow> {
        if self.precache_index(kind, name).is_some() {
            return Ok(());
        }
        let list = match kind {
            Precache::Sound => &mut self.sound_precache,
            Precache::Model => &mut self.model_precache,
        };
        if list.len() >= self.profile.precache_limit(kind) {
            return Err(Overflow);
        }
        list.push(name.to_owned());
        Ok(())
    }

    /// Sets lightstyle `style` to `value` (BUILTIN-12.6); `style` is already range-checked by
    /// the caller to 0 to [`LIMIT_MAX_LIGHTSTYLES`] - 1.
    pub fn set_lightstyle(&mut self, style: usize, value: Vec<u8>) {
        if let Some(cell) = self.lightstyles.get_mut(style) {
            *cell = Some(String::from_utf8_lossy(&value).into_owned());
        }
    }

    /// Appends `len` bytes to message buffer `dest` (DIFF-6.5).
    ///
    /// `dest` is 0 (`MSG_BROADCAST`, the unreliable datagram), 2 (`MSG_ALL`, the reliable
    /// datagram) or 3 (`MSG_INIT`, the signon buffer); a client's reliable message
    /// (`MSG_ONE`) is not kept and never overflows (DIFF-6.5).
    ///
    /// # Errors
    ///
    /// [`Overflow`] when the bytes do not fit, id's `SZ_GetSpace: overflow` (BUILTIN-13.3); the
    /// size is not changed.
    pub fn append_message(&mut self, dest: MessageDest, len: usize) -> Result<(), Overflow> {
        let (used, limit) = match dest {
            MessageDest::Broadcast => (&mut self.buffers.unreliable, LIMIT_MAX_DATAGRAM),
            MessageDest::All => (&mut self.buffers.reliable, LIMIT_MAX_DATAGRAM),
            MessageDest::Init => (&mut self.buffers.signon, BUILTIN_SIGNON_SIZE),
            MessageDest::One => return Ok(()),
        };
        match used.checked_add(len) {
            Some(new) if new <= limit => {
                *used = new;
                Ok(())
            }
            _ => Err(Overflow),
        }
    }

    /// Appends `text` to the command buffer `localcmd` and `changelevel` use (BUILTIN-11.5).
    ///
    /// # Errors
    ///
    /// [`Overflow`] when the contents plus `text` would reach [`BUILTIN_CMD_TEXT_SIZE`]; `text`
    /// is dropped (BUILTIN-11.5).
    pub fn append_command(&mut self, text: &[u8]) -> Result<(), Overflow> {
        let fits = self
            .command_buffer
            .len()
            .checked_add(text.len())
            .is_some_and(|total| total < BUILTIN_CMD_TEXT_SIZE);
        if fits {
            self.command_buffer.extend_from_slice(text);
            Ok(())
        } else {
            Err(Overflow)
        }
    }

    /// The bounds for a model list entry's name, by the DIFF-6.3 rule.
    ///
    /// Entry 0 is `'0 0 0'`/`'0 0 0'`; a name a `model` directive gave uses those bounds; any
    /// other name ending in `.mdl` is `'-16 -16 -16'`/`'16 16 16'`; every other name (the
    /// world, submodels and sprites among them) is `'0 0 0'`/`'0 0 0'`.
    #[must_use]
    pub fn model_bounds_for(&self, name: &str) -> ModelBounds {
        if name.is_empty() {
            return ModelBounds {
                mins: [0.0; 3],
                maxs: [0.0; 3],
            };
        }
        if let Some((_, bounds)) = self.model_bounds.iter().find(|(key, _)| key == name) {
            return *bounds;
        }
        if name.as_bytes().ends_with(b".mdl") {
            return ModelBounds {
                mins: [-16.0; 3],
                maxs: [16.0; 3],
            };
        }
        ModelBounds {
            mins: [0.0; 3],
            maxs: [0.0; 3],
        }
    }
}
