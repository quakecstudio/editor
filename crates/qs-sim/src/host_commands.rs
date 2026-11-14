// SPDX-License-Identifier: GPL-3.0-or-later

//! The string commands a client sends, as `SV_ReadClientMessage` filters them and
//! `Cmd_ExecuteString` runs them on the server (PHYS-11): the cheats, `kill`, `give`, `name`,
//! `color` and `pause`, and the signon's `prespawn`, `spawn` and `begin`, which the world runs.

use qc_builtins::mock::BUILTIN_FL_NOTARGET;
use qc_types::Slot;
use qc_vm::RunError;
use qc_vm::convert::{float_to_int, int_to_float};

use crate::builtins::SimCall;
use crate::messages::SVC_PRINT;
use crate::physics::MoveType;
use crate::settings::Setting;
use crate::spawn::{Parser, Token};

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
/// `atoi`'s base.
const DECIMAL: i32 = 10;

/// C's `atoi`: leading whitespace, a sign, then decimal digits, stopping at anything else.
pub(crate) fn atoi(text: &[u8]) -> i32 {
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
            self.client_printf("Can't suicide -- allready dead!\n");
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
        self.client_printf(&format!("{what} {state}\n"));
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
        self.client_printf(&format!("{what} {state}\n"));
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
    /// is the client's name and its `netname`, and `svc_updatename` goes to every client.
    fn name(&mut self, ent: usize, words: &Words) {
        let mut name = match words.argv.len() {
            0 | 1 => return,
            2 => words.argv.get(1).cloned().unwrap_or_default(),
            _ => words.args.clone(),
        };
        name.truncate(NAME_LENGTH);
        self.sim.client_name.clone_from(&name);
        let value = self.sim.mock.engine_mut().strings.make(name.clone());
        self.set_entity_raw(ent, "netname", value);
        let message = &mut self.sim.messages.reliable_datagram;
        message.write_byte(SVC_UPDATENAME);
        message.write_byte(0);
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
        self.set_float(ent, "team", int_to_float(bottom.wrapping_add(1)));
        let message = &mut self.sim.messages.reliable_datagram;
        message.write_byte(SVC_UPDATECOLORS);
        message.write_byte(0);
        message.write_byte(top.wrapping_mul(COLOR_ROW).wrapping_add(bottom));
    }

    /// PHYS-11.8: `Host_Pause_f`: with `pausable` set, the game pauses or unpauses, saying so to
    /// every client, and `svc_setpause` goes to every client; otherwise "Pause not allowed.".
    fn pause(&mut self, ent: usize) {
        if self.setting(Setting::Pausable) == 0.0 {
            self.client_printf("Pause not allowed.\n");
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
        let message = &mut self.sim.messages.client.message;
        message.write_byte(SVC_PRINT);
        message.write_string(&said);
        let paused = i32::from(self.sim.paused);
        let reliable = &mut self.sim.messages.reliable_datagram;
        reliable.write_byte(SVC_SETPAUSE);
        reliable.write_byte(paused);
    }

    /// `SV_ClientPrintf`: `svc_print` and the text to the client.
    fn client_printf(&mut self, text: &str) {
        let message = &mut self.sim.messages.client.message;
        message.write_byte(SVC_PRINT);
        message.write_string(text.as_bytes());
    }
}

#[cfg(test)]
mod tests;
