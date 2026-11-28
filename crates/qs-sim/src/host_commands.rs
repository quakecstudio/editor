// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_Name_f, Host_Color_f, Host_Pause_f, Host_Say, Host_Tell_f, Host_Status_f, Host_Ping_f, Host_Kick_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host.c, SV_ClientPrintf

//! The string commands a client sends, as `SV_ReadClientMessage` filters them and
//! `Cmd_ExecuteString` runs them on the server (PHYS-11): the cheats, `kill`, `give`, `name`,
//! `color`, `pause`, `say`, `say_team`, `tell`, `status`, `ping` and `kick`, and the signon's
//! `prespawn`, `spawn` and `begin`, which the world runs.

use qc_builtins::mock::BUILTIN_FL_NOTARGET;
use qc_types::Slot;
use qc_vm::RunError;
use qc_vm::convert::{float_to_int, int_to_float};

use crate::builtins::SimCall;
use crate::clients::{ClientId, SlotKind, UNCONNECTED};
use crate::console::VERSION;
use crate::messages::SVC_PRINT;
use crate::physics::MoveType;
use crate::settings::Setting;
use crate::spawn::{Parser, Token, c_atof};

/// `FL_GODMODE` (`WinQuake/server.h`), which `god` toggles (PHYS-11.3).
const FL_GODMODE: i32 = 64;
/// `IT_SHOTGUN` (`WinQuake/quakedef.h`), the item `give 2` gives; `give n` gives the item
/// `n - 2` places up (PHYS-11.6).
const IT_SHOTGUN: i32 = 1;
/// `svc_updatename`, `svc_updatecolors` and `svc_setpause` (`WinQuake/protocol.h`).
const SVC_UPDATENAME: i32 = 13;
const SVC_UPDATECOLORS: i32 = 17;
const SVC_SETPAUSE: i32 = 24;
/// `MAX_ARGS`, the words `Cmd_TokenizeString` keeps.
const MAX_ARGS: usize = 80;
/// The longest name `Host_Name_f` keeps: `newName[15] = 0`.
const NAME_LENGTH: usize = 15;
/// `Host_Say`'s `text[64]` and the 2 bytes it keeps for the line feed and the terminator.
const SAY_BUFFER: usize = 64;
const SAY_TAIL: usize = 2;
/// `tell`'s fewest words, `Cmd_Argc () < 3`: the command, a name and the text.
const TELL_WORDS: usize = 3;
/// The byte before a `say`'s name that turns on the console's second colour set.
const SAY_COLOR: u8 = 1;

/// The command names `SV_ReadClientMessage` lets a client run, each as the prefix it compares
/// case-insensitively with `Q_strncasecmp` (PHYS-11.1).
const ALLOWED: [&[u8]; 19] = [
    b"status",
    b"god",
    b"notarget",
    b"fly",
    b"name",
    b"noclip",
    b"say",
    b"say_team",
    b"tell",
    b"color",
    b"kill",
    b"pause",
    b"spawn",
    b"begin",
    b"prespawn",
    b"kick",
    b"ping",
    b"give",
    b"ban",
];

/// A signon stage a client's command asks the world for (PHYS-10.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignonCommand {
    /// `prespawn`.
    Prespawn,
    /// `spawn`.
    Spawn,
    /// `begin`.
    Begin,
}

/// One command's words and, after the first, the rest of its text, `Cmd_TokenizeString`'s
/// `cmd_argv` and `cmd_args` (PHYS-11.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Words {
    pub(crate) argv: Vec<Vec<u8>>,
    pub(crate) args: Vec<u8>,
}

/// `Cmd_TokenizeString (text)`: the words of the text's first line, each read by `COM_Parse`;
/// a `;` separates nothing.
pub(crate) fn tokenize(text: &[u8]) -> Words {
    let mut words = Words::default();
    let mut at = 0_usize;
    loop {
        // "skip whitespace up to a /n"
        while let Some(&c) = text.get(at) {
            if c == 0 || c == b'\n' || c.cast_signed() > b' '.cast_signed() {
                break;
            }
            at = at.saturating_add(1);
        }
        match text.get(at) {
            None | Some(0 | b'\n') => return words,
            Some(_) => {}
        }
        if words.argv.len() == 1 {
            words.args = text
                .get(at..)
                .unwrap_or_default()
                .split(|&c| c == 0)
                .next()
                .unwrap_or_default()
                .to_vec();
        }
        let rest = text.get(at..).unwrap_or_default();
        let mut parser = Parser::new(rest);
        let Token::Word(word) = parser.next_token() else {
            return words;
        };
        if words.argv.len() < MAX_ARGS {
            words.argv.push(word);
        }
        at = at.saturating_add(parser.position().max(1));
    }
}

/// A player colour's bits, `& 15`, the highest colour a player may wear, 13, and a colours
/// byte's rows, `top * 16 + bottom` (PHYS-11.7, PHYS-13.4).
pub(crate) const COLOR_MASK: i32 = 15;
pub(crate) const MAX_COLOR: i32 = 13;
pub(crate) const COLOR_ROW: i32 = 16;
/// `CL_SignonReply`'s top colour, `(int)cl_color.value >> 4` (PHYS-10.8).
pub(crate) const COLOR_SHIFT: u32 = 4;
/// `atoi`'s base.
const DECIMAL: i32 = 10;

/// C's `atoi`: leading whitespace, a sign, then decimal digits, stopping at anything else.
#[must_use]
pub fn atoi(text: &[u8]) -> i32 {
    let mut bytes = text
        .iter()
        .skip_while(|c| c.is_ascii_whitespace())
        .peekable();
    let negative = match bytes.peek() {
        Some(b'-') => {
            bytes.next();
            true
        }
        Some(b'+') => {
            bytes.next();
            false
        }
        _ => false,
    };
    let mut value = 0_i32;
    for &c in bytes {
        if !c.is_ascii_digit() {
            break;
        }
        value = value
            .wrapping_mul(DECIMAL)
            .wrapping_add(i32::from(c.wrapping_sub(b'0')));
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// Whether the client has spawned, `host_client->spawned`, which refuses a second
/// `prespawn` or `spawn` (PHYS-10.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientStage {
    /// Not yet spawned.
    Connecting,
    /// Spawned.
    Spawned,
}

impl SimCall<'_, '_, '_> {
    /// A string command client edict `ent` sent, as `SV_ReadClientMessage` filters it and
    /// `Cmd_ExecuteString` runs it (PHYS-11); a signon stage is left for the world to run.
    pub(crate) fn client_command(
        &mut self,
        ent: usize,
        text: &[u8],
        stage: ClientStage,
    ) -> Result<Option<SignonCommand>, RunError> {
        // PHYS-11.1: a command none of the prefixes match is not run.
        let allowed = ALLOWED.iter().any(|prefix| {
            text.get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        });
        if !allowed {
            return Ok(None);
        }
        let words = tokenize(text);
        let Some(name) = words.argv.first().map(|w| w.to_ascii_lowercase()) else {
            return Ok(None);
        };
        // PHYS-11.2: in deathmatch, no client is privileged.
        let cheat = self.global("deathmatch").as_f32() == 0.0;
        match name.as_slice() {
            b"god" if cheat => self.toggle_flag(ent, FL_GODMODE, "godmode"),
            b"notarget" if cheat => self.toggle_flag(ent, BUILTIN_FL_NOTARGET, "notarget"),
            b"noclip" if cheat => self.toggle_movetype(ent, MoveType::Noclip, "noclip"),
            b"fly" if cheat => self.toggle_movetype(ent, MoveType::Fly, "flymode"),
            b"give" if cheat => self.give(ent, &words),
            b"kill" => self.kill(ent)?,
            b"name" => self.name(ent, &words),
            b"color" => self.color(ent, &words),
            b"pause" => self.pause(ent),
            b"say" => self.say(ent, &words, SayTo::All),
            b"say_team" => self.say(ent, &words, SayTo::Team),
            b"tell" => self.tell(ent, &words),
            b"status" => self.status(ent),
            b"ping" => self.ping(ent),
            b"kick" => {
                if let Some(sender) = ClientId::of_edict(ent) {
                    self.kick(KickBy::Client(sender), &words)?;
                }
            }
            // PHYS-10.7: "prespawn not valid -- allready spawned", and the same for spawn.
            b"prespawn" if stage == ClientStage::Connecting => {
                return Ok(Some(SignonCommand::Prespawn));
            }
            b"spawn" if stage == ClientStage::Connecting => return Ok(Some(SignonCommand::Spawn)),
            b"begin" => return Ok(Some(SignonCommand::Begin)),
            _ => {}
        }
        Ok(None)
    }

    /// PHYS-11.4: `Host_Kill_f`.
    fn kill(&mut self, ent: usize) -> Result<(), RunError> {
        if self.float(ent, "health") <= 0.0 {
            self.client_printf(ent, b"Can't suicide -- allready dead!\n");
            return Ok(());
        }
        self.set_time(self.clock());
        let value = self.value_of(ent);
        self.set_global("self", Slot::from_i32(value));
        let function = self.global("ClientKill").as_i32();
        self.run(function)
    }

    /// PHYS-11.3: `Host_God_f` and `Host_Notarget_f`: the flag toggled, then
    /// `"<what> ON"` or `"<what> OFF"` printed to the client.
    fn toggle_flag(&mut self, ent: usize, flag: i32, what: &str) {
        let flags = self.flags(ent) ^ flag;
        self.set_flags(ent, flags);
        let state = if flags & flag == 0 { "OFF" } else { "ON" };
        self.client_printf(ent, format!("{what} {state}\n").as_bytes());
    }

    /// PHYS-11.3: `Host_Noclip_f` and `Host_Fly_f`: `movetype` becomes `to`, or `MOVETYPE_WALK`
    /// when it is already `to`, then `"<what> ON"` or `"<what> OFF"` printed to the client.
    fn toggle_movetype(&mut self, ent: usize, to: MoveType, what: &str) {
        let (next, state) = if self.movetype(ent) == to {
            (MoveType::Walk, "OFF")
        } else {
            (to, "ON")
        };
        self.set_float(ent, "movetype", next.to_float());
        self.client_printf(ent, format!("{what} {state}\n").as_bytes());
    }

    /// PHYS-11.6: `Host_Give_f`, the original game's: `give <n>` for a digit 2 to 9 adds the
    /// weapon `IT_SHOTGUN << (n - 2)`, and `give s`, `n`, `r`, `h` and `c` set the shells,
    /// nails, rockets, health or cells to `atoi` of the second word.
    fn give(&mut self, ent: usize, words: &Words) {
        let t = words.argv.get(1).cloned().unwrap_or_default();
        let v = words.argv.get(2).map_or(0, |w| atoi(w));
        #[expect(
            clippy::cast_precision_loss,
            reason = "spec PHYS-11.6: id assigns the int to a float field"
        )]
        let value = v as f32;
        match t.first() {
            Some(&digit @ b'2'..=b'9') => {
                let items = self.float(ent, "items");
                let shift = u32::from(digit.wrapping_sub(b'2'));
                let given = float_to_int(items) | (IT_SHOTGUN << shift);
                self.set_float(ent, "items", int_to_float(given));
            }
            Some(b's') => self.set_float(ent, "ammo_shells", value),
            Some(b'n') => self.set_float(ent, "ammo_nails", value),
            Some(b'r') => self.set_float(ent, "ammo_rockets", value),
            Some(b'h') => self.set_float(ent, "health", value),
            Some(b'c') => self.set_float(ent, "ammo_cells", value),
            _ => {}
        }
    }

    /// PHYS-11.7: `Host_Name_f`: one word, or with more the rest of the text, cut to 15 bytes,
    /// is the client's name and its `netname`, and `svc_updatename` goes to every client; a
    /// client with a name already, not "unconnected", that changes it is reported on the
    /// host's console (NET-7.6).
    fn name(&mut self, ent: usize, words: &Words) {
        let mut name = match words.argv.len() {
            0 | 1 => return,
            2 => words.argv.get(1).cloned().unwrap_or_default(),
            _ => words.args.clone(),
        };
        name.truncate(NAME_LENGTH);
        let Some(id) = ClientId::of_edict(ent) else {
            return;
        };
        let Some(slot) = self.sim.messages.clients.get_mut(id) else {
            return;
        };
        let old = core::mem::replace(&mut slot.name, name.clone());
        if !old.is_empty() && old != UNCONNECTED && old != name {
            let mut line = old;
            line.extend_from_slice(b" renamed to ");
            line.extend_from_slice(&name);
            line.push(b'\n');
            self.sim.console_out.extend_from_slice(&line);
        }
        let value = self.sim.mock.engine_mut().strings.make(name.clone());
        self.set_entity_raw(ent, "netname", value);
        let message = &mut self.sim.messages.reliable_datagram;
        message.write_byte(SVC_UPDATENAME);
        message.write_byte(id.byte());
        message.write_string(&name);
    }

    /// PHYS-11.7: `Host_Color_f`: the top and bottom colours, each `& 15` and at most 13, one
    /// word giving both; `team` becomes bottom + 1, and `svc_updatecolors` goes to every
    /// client.
    fn color(&mut self, ent: usize, words: &Words) {
        let (top, bottom) = match words.argv.len() {
            0 | 1 => return,
            2 => {
                let both = words.argv.get(1).map_or(0, |w| atoi(w));
                (both, both)
            }
            _ => (
                words.argv.get(1).map_or(0, |w| atoi(w)),
                words.argv.get(2).map_or(0, |w| atoi(w)),
            ),
        };
        let top = (top & COLOR_MASK).min(MAX_COLOR);
        let bottom = (bottom & COLOR_MASK).min(MAX_COLOR);
        let colors = top.wrapping_mul(COLOR_ROW).wrapping_add(bottom);
        let Some(id) = ClientId::of_edict(ent) else {
            return;
        };
        if let Some(slot) = self.sim.messages.clients.get_mut(id) {
            slot.colors = colors;
        }
        self.set_float(ent, "team", int_to_float(bottom.wrapping_add(1)));
        let message = &mut self.sim.messages.reliable_datagram;
        message.write_byte(SVC_UPDATECOLORS);
        message.write_byte(id.byte());
        message.write_byte(colors);
    }

    /// PHYS-11.8: `Host_Pause_f`: with `pausable` set, the game pauses or unpauses, saying so to
    /// every client, and `svc_setpause` goes to every client; otherwise "Pause not allowed.".
    fn pause(&mut self, ent: usize) {
        if self.setting(Setting::Pausable) == 0.0 {
            self.client_printf(ent, b"Pause not allowed.\n");
            return;
        }
        self.sim.paused = !self.sim.paused;
        let netname = self.string_text(self.entity_raw(ent, "netname"));
        let how = if self.sim.paused {
            "paused"
        } else {
            "unpaused"
        };
        let mut said = netname;
        said.extend_from_slice(format!(" {how} the game\n").as_bytes());
        self.broadcast_print(&said);
        let paused = i32::from(self.sim.paused);
        let reliable = &mut self.sim.messages.reliable_datagram;
        reliable.write_byte(SVC_SETPAUSE);
        reliable.write_byte(paused);
    }

    /// `Host_Say` (NET-7.6): the rest of the text, a leading `"` and the last byte dropped
    /// when it starts with one, after "^A<name>: ", cut to fit 64 bytes with its line feed,
    /// printed to every active, spawned client; with `say_team` and `teamplay` set, only to
    /// those whose `team` is the sender's.
    fn say(&mut self, ent: usize, words: &Words, to: SayTo) {
        if words.argv.len() < 2 {
            return;
        }
        let Some(id) = ClientId::of_edict(ent) else {
            return;
        };
        let name = self
            .sim
            .messages
            .clients
            .get(id)
            .map_or_default(|slot| slot.name.clone());
        let mut text = vec![SAY_COLOR];
        text.extend_from_slice(&name);
        text.extend_from_slice(b": ");
        append_said(&mut text, &words.args);
        let team_only = to == SayTo::Team && self.setting(Setting::Teamplay) != 0.0;
        let team = self.float(ent, "team");
        let heard: Vec<ClientId> = self
            .sim
            .messages
            .clients
            .iter()
            .filter(|(_, slot)| slot.active && slot.spawned)
            .map(|(other, _)| other)
            .collect();
        for other in heard {
            // `client->edict->v.team != save->edict->v.team`, compared as floats.
            #[expect(
                clippy::float_cmp,
                reason = "spec NET-7.6: Host_Say compares team exactly"
            )]
            let other_team = self.float(other.edict(), "team") != team;
            if !(team_only && other_team) {
                self.client_printf(other.edict(), &text);
            }
        }
    }

    /// `Host_Tell_f` (NET-7.6): "<name>: " and the text from the second word on, as `say`
    /// cuts it, printed to the first active, spawned client whose name is the first word,
    /// compared with `Q_strcasecmp`.
    fn tell(&mut self, ent: usize, words: &Words) {
        if words.argv.len() < TELL_WORDS {
            return;
        }
        let Some(id) = ClientId::of_edict(ent) else {
            return;
        };
        let clients = &self.sim.messages.clients;
        let mut text = clients.get(id).map_or_default(|slot| slot.name.clone());
        text.extend_from_slice(b": ");
        append_said(&mut text, &words.args);
        let to = words.argv.get(1).cloned().unwrap_or_default();
        let found = clients
            .iter()
            .find(|(_, slot)| slot.active && slot.spawned && slot.name.eq_ignore_ascii_case(&to))
            .map(|(other, _)| other);
        if let Some(other) = found {
            self.client_printf(other.edict(), &text);
        }
    }

    /// `Host_Status_f` from a client (NET-7.6): `status_lines`, each printed to the client.
    fn status(&mut self, ent: usize) {
        for line in self.status_lines() {
            self.client_printf(ent, &line);
        }
    }

    /// `Host_Status_f`'s lines (PHYS-13.10, NET-7.6): the host's name, the version, the map,
    /// how many clients are active of how many slots, and for each active one its number, name
    /// cut to 16, frags and time connected, then its address; a local client's is `LOCAL`, the
    /// loopback connection's, and a bot's `BOT`, and every one has been connected 0:00:00, as
    /// the preview host keeps no wall clock (NET-7.6).
    pub(crate) fn status_lines(&self) -> Vec<Vec<u8>> {
        let hostname = self
            .sim
            .mock
            .engine()
            .cvars
            .text("hostname")
            .unwrap_or_default()
            .to_owned();
        let map = self.string_text(self.global("mapname").as_i32());
        let clients = &self.sim.messages.clients;
        let mut lines = vec![
            format!("host:    {hostname}\n").into_bytes(),
            format!("version: {VERSION}\n").into_bytes(),
        ];
        let mut map_line = b"map:     ".to_vec();
        map_line.extend_from_slice(&map);
        map_line.push(b'\n');
        lines.push(map_line);
        lines.push(
            format!(
                "players: {} active ({} max)\n\n",
                clients.active_count(),
                clients.max().get()
            )
            .into_bytes(),
        );
        for (id, slot) in clients.iter() {
            if !slot.active {
                continue;
            }
            let name = String::from_utf8_lossy(&slot.name).into_owned();
            let frags = float_to_int(self.float(id.edict(), "frags"));
            lines.push(
                format!(
                    "#{:<2} {name:<16.16}  {frags:3}   0:00:00\n",
                    id.slot().saturating_add(1)
                )
                .into_bytes(),
            );
            let address = match slot.kind {
                SlotKind::Local => "LOCAL",
                SlotKind::Bot => "BOT",
            };
            lines.push(format!("   {address}\n").into_bytes());
        }
        lines
    }

    /// `Host_Ping_f` (NET-7.6): "Client ping times:", then for each active client the mean of
    /// its last 16 ping times in milliseconds, truncated, and its name.
    fn ping(&mut self, ent: usize) {
        self.client_printf(ent, b"Client ping times:\n");
        let lines: Vec<Vec<u8>> = self
            .sim
            .messages
            .clients
            .iter()
            .filter(|(_, slot)| slot.active)
            .map(|(_, slot)| {
                let mut line = format!("{:4} ", slot.mean_ping_ms()).into_bytes();
                line.extend_from_slice(&slot.name);
                line.push(b'\n');
                line
            })
            .collect();
        for line in lines {
            self.client_printf(ent, &line);
        }
    }

    /// `Host_Kick_f` (NET-7.6): from a client, refused in deathmatch, as no client is
    /// privileged; otherwise the client named, by `# <number>` or by its name compared with
    /// `Q_strcasecmp`, is told "Kicked by <who>" and the reason when one follows, then
    /// dropped. A client cannot kick itself; the host's console names no client of its own
    /// (`host_client` is past the slots when its commands run), so it can kick any, its own
    /// player too, and `who` is `_cl_name`.
    pub(crate) fn kick(&mut self, by: KickBy, words: &Words) -> Result<(), RunError> {
        if matches!(by, KickBy::Client(_)) && self.global("deathmatch").as_f32() != 0.0 {
            return Ok(());
        }
        let clients = &self.sim.messages.clients;
        let named = if words.argv.len() > 2 && words.argv.get(1).is_some_and(|w| w == b"#") {
            KickNamed::ByNumber
        } else {
            KickNamed::ByName
        };
        let target = if named == KickNamed::ByNumber {
            let n = words.argv.get(2).map_or(0.0, |w| c_atof(w));
            usize::try_from(float_to_int(n - 1.0))
                .ok()
                .and_then(ClientId::new)
                .filter(|&id| clients.is_active(id))
        } else {
            let name = words.argv.get(1).cloned().unwrap_or_default();
            clients
                .iter()
                .find(|(_, slot)| slot.active && slot.name.eq_ignore_ascii_case(&name))
                .map(|(id, _)| id)
        };
        let Some(target) = target else {
            return Ok(());
        };
        let who = match by {
            // "can't kick yourself!"
            KickBy::Client(sender) if sender == target => return Ok(()),
            KickBy::Client(sender) => clients.get(sender).map_or_default(|slot| slot.name.clone()),
            KickBy::Console => self
                .sim
                .mock
                .engine()
                .cvars
                .text("_cl_name")
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
        };
        let mut text = b"Kicked by ".to_vec();
        text.extend_from_slice(&who);
        if let Some(reason) = words.kick_reason(named) {
            text.extend_from_slice(b": ");
            text.extend_from_slice(&reason);
        }
        text.push(b'\n');
        self.client_printf(target.edict(), &text);
        self.drop_client(target)
    }

    /// `SV_ClientPrintf`: `svc_print` and the text to client edict `ent`.
    pub(crate) fn client_printf(&mut self, ent: usize, text: &[u8]) {
        if let Some(slot) = self.sim.messages.clients.of_edict_mut(ent) {
            slot.message.write_byte(SVC_PRINT);
            slot.message.write_string(text);
        }
    }
}

/// Who a `kick` comes from (NET-7.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KickBy {
    /// A client's string command.
    Client(ClientId),
    /// The host's console, `cmd_source == src_command`.
    Console,
}

/// Who a `say` reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SayTo {
    /// `say`: every client.
    All,
    /// `say_team`: with `teamplay` set, the sender's team.
    Team,
}

/// `Host_Say`'s and `Host_Tell_f`'s text after its prefix (NET-7.6): `Cmd_Args`, a leading `"`
/// and the last byte dropped when it starts with one, cut so the text and its line feed fit
/// 64 bytes with the terminator, then the line feed.
fn append_said(text: &mut Vec<u8>, args: &[u8]) {
    let mut said = args;
    if let Some(rest) = said.strip_prefix(b"\"") {
        said = rest.split_last().map_or(rest, |(_, kept)| kept);
    }
    let room = SAY_BUFFER
        .saturating_sub(SAY_TAIL)
        .saturating_sub(text.len());
    text.extend_from_slice(said.get(..room).unwrap_or(said));
    text.push(b'\n');
}

/// How `kick` names its client (NET-7.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KickNamed {
    /// `kick # <n>`.
    ByNumber,
    /// `kick <name>`.
    ByName,
}

impl Words {
    /// `Host_Kick_f`'s reason (NET-7.6): with more than two words, `COM_Parse (Cmd_Args())`'s
    /// rest past the first word; by number, past the `#`, the spaces after it and the number's
    /// length too; then the spaces before it. `None` with two words or fewer.
    fn kick_reason(&self, named: KickNamed) -> Option<Vec<u8>> {
        if self.argv.len() <= 2 {
            return None;
        }
        let mut parser = Parser::new(&self.args);
        let _first = parser.next_token();
        let mut rest = self.args.get(parser.position()..).unwrap_or_default();
        if named == KickNamed::ByNumber {
            // `message++`, the `#`; the spaces; the number's length.
            rest = rest.get(1..).unwrap_or_default();
            while let Some(after) = rest.strip_prefix(b" ") {
                rest = after;
            }
            let number = self.argv.get(2).map_or(0, Vec::len);
            rest = rest.get(number..).unwrap_or_default();
        }
        while let Some(after) = rest.strip_prefix(b" ") {
            rest = after;
        }
        Some(rest.split(|&c| c == 0).next().unwrap_or_default().to_vec())
    }
}

#[cfg(test)]
mod tests;
