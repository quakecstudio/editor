// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cmd.c, Cbuf_Execute, Cbuf_InsertText, Cmd_ExecuteString, Cmd_Wait_f, Cmd_Echo_f, Cmd_Alias_f, Cmd_Exec_f, Cmd_StuffCmds_f, Cmd_ForwardToServer
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cvar.c, Cvar_Command, Cvar_SetValue
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_Status_f, Host_Version_f, Host_Name_f, Host_Color_f, Host_PreSpawn_f, Host_Spawn_f, Host_Begin_f, Host_Map_f, Host_Changelevel_f, Host_Restart_f, Host_Savegame_f, Host_Loadgame_f, Host_Quit_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Print, ED_PrintEdicts, ED_PrintEdict_f, ED_Count, PR_ValueString
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_Profile_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, COM_Path_f
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cmd.c, Cmd_List_f, Cmd_AddCommand

//! The console (PHYS-13): id's command buffer and `Cmd_ExecuteString` for text from the console
//! or the progs' `localcmd`, with one table of commands: `cmd.c`'s, the settings, the commands
//! a client's console forwards to the server, the host's, and the debug prints.

use qc_progs::Progs;
use qc_types::{DefType, Etype, Slot};
use qc_vm::convert::float_to_int;

use crate::host_commands::{Words, atoi, tokenize};
use crate::input::Button;
use crate::offsets::FieldOffset;
use crate::settings::{CLIENT_SETTINGS, SERVER_SETTINGS};
use crate::world::{ClientCommand, HostCommand, RawState as _, World};

/// `VERSION` (`WinQuake/quakedef.h`), which `version` and `status` print.
const VERSION: &str = "1.09";
/// `MAX_ALIAS_NAME` (`WinQuake/cmd.c`).
const MAX_ALIAS_NAME: usize = 32;

/// The commands a client's console sends on to the server, `Cmd_ForwardToServer` from a
/// command run with `cmd_source == src_command` (PHYS-13.4).
const FORWARDED: [&str; 11] = [
    "god", "notarget", "fly", "noclip", "give", "kill", "pause", "say", "say_team", "tell", "ping",
];

/// Every command [`World::execute_console`] runs, its own and the forwarded ones, in name
/// order, for completion (CON-4.3).
const COMMANDS: [&str; 37] = [
    "alias",
    "begin",
    "changelevel",
    "cmd",
    "cmdlist",
    "color",
    "echo",
    "edict",
    "edictcount",
    "edicts",
    "exec",
    "fly",
    "give",
    "god",
    "kick",
    "kill",
    "load",
    "map",
    "name",
    "noclip",
    "notarget",
    "path",
    "pause",
    "ping",
    "prespawn",
    "profile",
    "quit",
    "restart",
    "save",
    "say",
    "say_team",
    "spawn",
    "status",
    "stuffcmds",
    "tell",
    "version",
    "wait",
];

/// `MOVETYPE_STEP` (`WinQuake/server.h`), which `edictcount` counts.
const MOVETYPE_STEP: f32 = 4.0;

/// A count one more, as `ED_Count`'s `++`, which an edict count cannot overflow.
const fn add_one(count: usize) -> usize {
    count.saturating_add(1)
}

/// Every command's name, the server's and the client's (CON-6.1), in `strcmp` order, as
/// QuakeSpasm's `Cmd_AddCommand` keeps its list.
fn command_names() -> Vec<String> {
    let mut names: Vec<String> = COMMANDS
        .iter()
        .chain(ClientCommand::ALL.iter().map(|(name, _)| name))
        .chain(&KEY_COMMANDS)
        .map(|name| (*name).to_owned())
        .collect();
    for (button, _) in Button::ALL {
        names.push(format!("+{button}"));
        names.push(format!("-{button}"));
    }
    names.sort_unstable();
    names
}

/// The client's commands of `keys.c` and `cl_input.c` that take words (INPUT-1.2, INPUT-2.1).
const KEY_COMMANDS: [&str; 4] = ["bind", "unbind", "unbindall", "impulse"];

/// The client's command `name` with `words`, when it is one (CON-6.4, INPUT-1.2, INPUT-2.1).
fn client_command(name: &str, words: &Words) -> Option<ClientCommand> {
    if let Some((_, command)) = ClientCommand::ALL.iter().find(|(n, _)| *n == name) {
        return Some(command.clone());
    }
    // `KeyDown` and `KeyUp`: "`c = Cmd_Argv(1); if (c[0]) k = atoi(c); else k = -1;`".
    let key = || {
        words
            .argv
            .get(1)
            .filter(|word| !word.is_empty())
            .map(|word| atoi(word))
    };
    if let Some(button) = name.strip_prefix('+').and_then(Button::named) {
        return Some(ClientCommand::Press(button, key()));
    }
    if let Some(button) = name.strip_prefix('-').and_then(Button::named) {
        return Some(ClientCommand::Release(button, key()));
    }
    match name {
        "bind" => Some(ClientCommand::Bind(words.argv.clone())),
        "unbind" => Some(ClientCommand::Unbind(words.argv.clone())),
        "unbindall" => Some(ClientCommand::UnbindAll),
        "impulse" => {
            let n = words.argv.get(1).map_or(0, |word| atoi(word));
            Some(ClientCommand::Impulse(n.to_le_bytes()[0]))
        }
        _ => None,
    }
}

/// What `Cmd_ForwardToServer` sends: the command's name and its arguments, or for `cmd` the
/// arguments alone (PHYS-13.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Forward {
    /// The name, a space and the arguments.
    Named,
    /// The arguments alone.
    Args,
}

/// What a line of the buffer leaves the rest of the buffer to do.
enum Flow {
    /// Go on with the next line.
    Next,
    /// `wait`: the rest waits for the next frame (PHYS-13.2).
    Wait,
    /// A command the host carries out stops the buffer until it has (PHYS-13.6).
    Host(HostCommand),
}

/// `Cbuf_Execute`'s next line of `text` and the rest: up to a line feed, or a `;` outside a
/// quoted string, counting quotes from the start of the line (PHYS-13.1).
fn next_line(text: &[u8]) -> (&[u8], &[u8]) {
    let mut quotes = 0_u32;
    for (i, &c) in text.iter().enumerate() {
        if c == b'"' {
            quotes = quotes.wrapping_add(1);
        }
        if (quotes & 1 == 0 && c == b';') || c == b'\n' {
            let line = text.get(..i).unwrap_or_default();
            let rest = text.get(i.saturating_add(1)..).unwrap_or_default();
            return (line, rest);
        }
    }
    (text, &[])
}

/// C's `%5.1f`, as `PR_ValueString` prints a float.
fn float_text(value: f32) -> String {
    if value.is_nan() {
        return "  nan".to_owned();
    }
    format!("{value:5.1}")
}

impl World {
    /// Text for the console's buffer, `Cbuf_AddText`: run by the next
    /// [`World::execute_console`].
    pub fn console(&mut self, text: &str) {
        self.builtins
            .mock
            .engine_mut()
            .command_buffer
            .extend_from_slice(text.as_bytes());
    }

    /// `Cmd_CompleteCommand`, then `Cvar_CompleteVariable`: the first command, else the first
    /// setting, whose name starts with `partial`, in name order; none for an empty `partial`
    /// (CON-4.3).
    #[must_use]
    pub fn complete(&self, partial: &str) -> Option<String> {
        if partial.is_empty() {
            return None;
        }
        let mut settings: Vec<&str> = SERVER_SETTINGS
            .iter()
            .chain(&CLIENT_SETTINGS)
            .map(|(name, _)| *name)
            .collect();
        settings.sort_unstable();
        let commands = command_names();
        commands
            .iter()
            .map(String::as_str)
            .chain(settings)
            .find(|name| name.starts_with(partial))
            .map(str::to_owned)
    }

    /// The lines the console printed since the last call, each without its line feed; text
    /// after the last line feed waits for the rest of its line.
    pub fn take_console_output(&mut self) -> Vec<Vec<u8>> {
        let Some(last) = self.console_out.iter().rposition(|&c| c == b'\n') else {
            return Vec::new();
        };
        let rest = self.console_out.split_off(last.saturating_add(1));
        let done = core::mem::replace(&mut self.console_out, rest);
        done.split(|&c| c == b'\n')
            .take(done.iter().filter(|&&c| c == b'\n').count())
            .map(<[u8]>::to_vec)
            .collect()
    }

    /// `Cbuf_Execute` (PHYS-13.1): runs the buffer's lines in order until it is empty, a `wait`
    /// leaves the rest for the next frame, or a host command waits for the host (PHYS-13.6).
    pub fn execute_console(&mut self, progs: &Progs<'_>) {
        // PHYS-13.6: the rest waits until the host has carried out the command before it.
        if self.pending_host.is_some() {
            return;
        }
        loop {
            let buffer = core::mem::take(&mut self.builtins.mock.engine_mut().command_buffer);
            if buffer.is_empty() {
                return;
            }
            let (line, rest) = next_line(&buffer);
            let line = line.to_vec();
            self.builtins.mock.engine_mut().command_buffer = rest.to_vec();
            match self.execute_line(progs, &line) {
                Flow::Next => {}
                Flow::Wait => return,
                Flow::Host(command) => {
                    self.pending_host = Some(command);
                    return;
                }
            }
        }
    }

    /// QuakeSpasm's `Cmd_List_f` (CON-6.2): each command whose name starts with the first
    /// argument, case-sensitively, in name order, then the count.
    fn cmdlist(&mut self, words: &Words) {
        let partial = words
            .argv
            .get(1)
            .map(|w| String::from_utf8_lossy(w).into_owned());
        let mut count = 0_usize;
        for name in command_names() {
            if partial
                .as_ref()
                .is_some_and(|p| !name.starts_with(p.as_str()))
            {
                continue;
            }
            self.print(format!("   {name}\n"));
            count = count.saturating_add(1);
        }
        match partial {
            Some(partial) => self.print(format!("{count} commands beginning with \"{partial}\"\n")),
            None => self.print(format!("{count} commands\n")),
        }
    }

    /// `Con_Printf`.
    fn print(&mut self, text: impl AsRef<[u8]>) {
        self.console_out.extend_from_slice(text.as_ref());
    }

    /// `Cmd_ExecuteString (text, src_command)` (PHYS-13.1): the first word, ASCII
    /// case-insensitively, a command, else an alias, else a setting, else unknown.
    fn execute_line(&mut self, progs: &Progs<'_>, line: &[u8]) -> Flow {
        let words = tokenize(line);
        let Some(first) = words.argv.first().cloned() else {
            return Flow::Next;
        };
        let name = String::from_utf8_lossy(&first).to_ascii_lowercase();
        let arg = |n: usize| {
            words
                .argv
                .get(n)
                .map(|w| String::from_utf8_lossy(w).into_owned())
                .unwrap_or_default()
        };
        if let Some(command) = client_command(&name, &words) {
            self.client_commands.push(command);
            return Flow::Next;
        }
        match name.as_str() {
            "wait" => return Flow::Wait,
            "cmdlist" => self.cmdlist(&words),
            "echo" => self.echo(&words),
            "alias" => self.alias(&words),
            "exec" => self.exec(&words),
            // The preview host starts with no `+` command-line parameters to run.
            "stuffcmds" if words.argv.len() != 1 => {
                self.print("stuffcmds : execute command line parameters\n");
            }
            "stuffcmds" => {}
            "cmd" => self.forward(&words, Forward::Args),
            "name" => self.name(&words),
            "color" => self.color(&words),
            "status" => self.status(),
            "kick" => {}
            "version" => {
                self.print(format!("Version {VERSION}\n"));
                self.print(format!(
                    "Exe: QuakeC Studio {}\n",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            "prespawn" | "spawn" | "begin" => {
                self.print(format!("{name} is not valid from the console\n"));
            }
            "map" if words.argv.len() >= 2 => return Flow::Host(HostCommand::Map(arg(1))),
            "map" => {}
            "changelevel" if words.argv.len() == 2 => {
                return Flow::Host(HostCommand::Changelevel(arg(1)));
            }
            "changelevel" => {
                self.print("changelevel <levelname> : continue game on a new level\n");
            }
            "restart" => return Flow::Host(HostCommand::Restart),
            "save" if words.argv.len() == 2 && arg(1).contains("..") => {
                self.print("Relative pathnames are not allowed.\n");
            }
            "save" if words.argv.len() == 2 => return Flow::Host(HostCommand::Save(arg(1))),
            "save" => self.print("save <savename> : save a game\n"),
            "load" if words.argv.len() == 2 => return Flow::Host(HostCommand::Load(arg(1))),
            "load" => self.print("load <savename> : load a game\n"),
            "quit" => return Flow::Host(HostCommand::Quit),
            "edict" => self.print_edict_command(progs, &words),
            "edicts" => {
                let count = self.num_edicts();
                self.print(format!("{count} entities\n"));
                for n in 0..count {
                    self.print_edict(progs, n);
                }
            }
            "edictcount" => self.edict_count(),
            "profile" => self.profile(progs),
            "path" => self.path(),
            forwarded if FORWARDED.contains(&forwarded) => self.forward(&words, Forward::Named),
            _ => self.alias_or_setting(&words, &first),
        }
        Flow::Next
    }

    /// `Cmd_Echo_f`: each argument and a space, then a line feed.
    fn echo(&mut self, words: &Words) {
        let mut text = Vec::new();
        for word in words.argv.iter().skip(1) {
            text.extend_from_slice(word);
            text.push(b' ');
        }
        text.push(b'\n');
        self.print(text);
    }

    /// `Cmd_Alias_f` (PHYS-13.2): with no name the list; else the alias's text is each
    /// argument after the name and a space, then a line feed.
    fn alias(&mut self, words: &Words) {
        let Some(name) = words.argv.get(1) else {
            self.print("Current alias commands:\n");
            let list: Vec<String> = self
                .aliases
                .iter()
                .map(|(name, value)| format!("{name} : {value}\n"))
                .collect();
            for line in list {
                self.print(line);
            }
            return;
        };
        let name = String::from_utf8_lossy(name).into_owned();
        if name.len() >= MAX_ALIAS_NAME {
            self.print("Alias name is too long\n");
            return;
        }
        let mut value = String::new();
        for word in words.argv.iter().skip(2) {
            value.push_str(&String::from_utf8_lossy(word));
            value.push(' ');
        }
        value.push('\n');
        // A new alias goes first in the list, as id's `cmd_alias` puts it.
        match self.aliases.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => self.aliases.insert(0, (name, value)),
        }
    }

    /// `Cmd_Exec_f` (PHYS-13.2): the file's text goes before the rest of the buffer.
    fn exec(&mut self, words: &Words) {
        let [_, file] = words.argv.as_slice() else {
            self.print("exec <filename> : execute a script file\n");
            return;
        };
        let file = String::from_utf8_lossy(file).into_owned();
        let Some(text) = (self.builtins.files)(&file) else {
            self.print(format!("couldn't exec {file}\n"));
            return;
        };
        self.print(format!("execing {file}\n"));
        self.insert_text(&text);
    }

    /// `Cbuf_InsertText`: `text` before the rest of the buffer.
    fn insert_text(&mut self, text: &[u8]) {
        let buffer = &mut self.builtins.mock.engine_mut().command_buffer;
        let rest = core::mem::take(buffer);
        buffer.extend_from_slice(text);
        buffer.extend_from_slice(&rest);
    }

    /// An alias (`Cbuf_InsertText` of its text), else a setting (`Cvar_Command`), else
    /// "Unknown command" (PHYS-13.3).
    fn alias_or_setting(&mut self, words: &Words, first: &[u8]) {
        let name = String::from_utf8_lossy(first).into_owned();
        if let Some((_, value)) = self
            .aliases
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(&name))
        {
            let value = value.clone();
            self.insert_text(value.as_bytes());
            return;
        }
        let cvars = &mut self.builtins.mock.engine_mut().cvars;
        let Some(text) = cvars.text(&name).map(str::to_owned) else {
            self.print(format!("Unknown command \"{name}\"\n"));
            return;
        };
        match words.argv.get(1) {
            None => self.print(format!("\"{name}\" is \"{text}\"\n")),
            Some(value) => {
                let value = String::from_utf8_lossy(value).into_owned();
                cvars.set(&name, &value);
            }
        }
    }

    /// `Cmd_ForwardToServer` (PHYS-13.4): the command's name unless it is `cmd`, a space, and
    /// the rest of the line or a line feed, sent as the client's string command.
    fn forward(&mut self, words: &Words, form: Forward) {
        let args = (words.argv.len() > 1).then_some(words.args.as_slice());
        self.forward_args(words, form, args);
    }

    /// [`World::forward`] with `args` as `Cmd_Args`, `None` for a command alone.
    fn forward_args(&mut self, words: &Words, form: Forward, args: Option<&[u8]>) {
        let name = words.argv.first().cloned().unwrap_or_default();
        if !self.builtins.messages.client.active {
            self.print(format!(
                "Can't \"{}\", not connected\n",
                String::from_utf8_lossy(&name)
            ));
            return;
        }
        let mut text = Vec::new();
        if form == Forward::Named {
            text.extend_from_slice(&name);
            text.push(b' ');
        }
        match args {
            Some(args) => text.extend_from_slice(args),
            None => text.push(b'\n'),
        }
        self.commands.push(text);
    }

    /// `Host_Name_f` from the console (PHYS-13.4): with no argument it prints `_cl_name`; else
    /// the one argument, or the whole rest of the line, cut to 15 bytes, becomes `_cl_name`
    /// unless it already is, and goes on to the server when connected.
    fn name(&mut self, words: &Words) {
        let cvars = &self.builtins.mock.engine().cvars;
        let current = cvars.text("_cl_name").unwrap_or_default().to_owned();
        let new = match words.argv.as_slice() {
            [_] => {
                self.print(format!("\"name\" is \"{current}\"\n"));
                return;
            }
            [_, one] => one.clone(),
            _ => words.args.clone(),
        };
        // `newName[15] = 0`, which writes into `Cmd_Args` when the name is the rest of the
        // line, so only then does the server get the cut name.
        let cut = new.get(..15).map_or_else(|| new.clone(), <[u8]>::to_vec);
        let cut_text = String::from_utf8_lossy(&cut).into_owned();
        if cut_text == current {
            return;
        }
        self.builtins
            .mock
            .engine_mut()
            .cvars
            .set("_cl_name", &cut_text);
        let args = match words.argv.len() {
            2 => words.args.clone(),
            _ => cut,
        };
        if self.builtins.messages.client.active {
            self.forward_args(words, Forward::Named, Some(&args));
        }
    }

    /// `Host_Color_f` from the console (PHYS-13.4): with no argument it prints `_cl_color` as
    /// its two colours; else each colour is `atoi`, its low four bits, at most 13, and
    /// `top * 16 + bottom` becomes `_cl_color`, which goes on to the server when connected.
    fn color(&mut self, words: &Words) {
        let colour = |word: Option<&Vec<u8>>| {
            let value = word.map_or(0, |w| atoi(w)) & 15;
            value.min(13)
        };
        let (top, bottom) = match words.argv.as_slice() {
            [_] => {
                let value = self
                    .builtins
                    .mock
                    .engine()
                    .cvars
                    .value("_cl_color")
                    .unwrap_or(0.0);
                let value = float_to_int(value);
                self.print(format!(
                    "\"color\" is \"{} {}\"\ncolor <0-13> [0-13]\n",
                    value >> 4,
                    value & 15
                ));
                return;
            }
            [_, both] => (colour(Some(both)), colour(Some(both))),
            [_, top, bottom, ..] => (colour(Some(top)), colour(Some(bottom))),
            [] => return,
        };
        let player_colour = top.wrapping_mul(16).wrapping_add(bottom);
        // `Cvar_SetValue`: the value printed with `%f`.
        self.builtins
            .mock
            .engine_mut()
            .cvars
            .set("_cl_color", &format!("{player_colour}.000000"));
        if self.builtins.messages.client.active {
            self.forward(words, Forward::Named);
        }
    }

    /// `Host_Status_f` on the server's console (PHYS-13.5).
    fn status(&mut self) {
        let hostname = self
            .builtins
            .mock
            .engine()
            .cvars
            .text("hostname")
            .unwrap_or_default()
            .to_owned();
        let map = self.map().to_owned();
        let active = usize::from(self.builtins.messages.client.active);
        self.print(format!("host:    {hostname}\n"));
        self.print(format!("version: {VERSION}\n"));
        self.print(format!("map:     {map}\n"));
        self.print(format!("players: {active} active (1 max)\n\n"));
        if active == 1 {
            let name = String::from_utf8_lossy(&self.builtins.client_name).into_owned();
            let frags = float_to_int(self.field(1, "frags").as_f32());
            self.print(format!("#1  {name:<16.16}  {frags:3}   0:00:00\n"));
            // `loop_server->address`, the server's end of the loopback connection.
            self.print("   LOCAL\n");
        }
    }

    /// `edict <n>`, `ED_PrintEdict_f` (PHYS-13.7).
    fn print_edict_command(&mut self, progs: &Progs<'_>, words: &Words) {
        let n = words.argv.get(1).map_or(0, |w| atoi(w));
        match usize::try_from(n) {
            Ok(n) if n < self.num_edicts() => self.print_edict(progs, n),
            _ => self.print("Bad edict number\n"),
        }
    }

    /// `ED_Print` (PHYS-13.7): `FREE`, or the edict's number and each field whose slots are
    /// not all zero, its name padded to 15 and its `PR_ValueString`.
    fn print_edict(&mut self, progs: &Progs<'_>, n: usize) {
        if !self.in_use(n) {
            self.print("FREE\n");
            return;
        }
        self.print(format!("\nEDICT {n}:\n"));
        let defs: Vec<_> = progs.field_defs().iter().skip(1).collect();
        for def in defs {
            let Some(name) = progs.string(def.s_name).map(<[u8]>::to_vec) else {
                continue;
            };
            // "skip _x, _y, _z vars": any name whose second last byte is `_`.
            if name.len() >= 2 && name.get(name.len().saturating_sub(2)) == Some(&b'_') {
                continue;
            }
            let size = match def.def_type() {
                DefType::Known(Etype::Vector) => 3,
                _ => 1,
            };
            let first = usize::try_from(def.ofs).unwrap_or(usize::MAX);
            let slots: Vec<Slot> = (0..size)
                .map(|i| self.slot_of(n, FieldOffset::new(first.saturating_add(i))))
                .collect();
            if slots.iter().all(|s| s.as_i32() == 0) {
                continue;
            }
            let mut line = name.clone();
            while line.len() < 15 {
                line.push(b' ');
            }
            line.extend_from_slice(&self.value_string(progs, def.def_type(), &slots));
            line.push(b'\n');
            self.print(line);
        }
    }

    /// `PR_ValueString` (PHYS-13.7).
    fn value_string(&self, progs: &Progs<'_>, def_type: DefType, slots: &[Slot]) -> Vec<u8> {
        let first = slots.first().copied().unwrap_or(Slot::ZERO);
        let name_of = |offset: i32| progs.string(offset).map(<[u8]>::to_vec).unwrap_or_default();
        match def_type {
            DefType::Known(Etype::String) => self.string_value(progs, first).unwrap_or_default(),
            DefType::Known(Etype::Entity) => format!("entity {}", self.entity(first)).into_bytes(),
            DefType::Known(Etype::Function) => {
                let mut text = usize::try_from(first.as_i32())
                    .ok()
                    .and_then(|i| progs.functions().get(i))
                    .map(|f| name_of(f.s_name))
                    .unwrap_or_default();
                text.extend_from_slice(b"()");
                text
            }
            DefType::Known(Etype::Field) => {
                let mut text = b".".to_vec();
                text.extend(
                    u32::try_from(first.as_i32())
                        .ok()
                        .and_then(|ofs| progs.ed_field_at_ofs(ofs))
                        .map(|d| name_of(d.s_name))
                        .unwrap_or_default(),
                );
                text
            }
            DefType::Known(Etype::Void) => b"void".to_vec(),
            DefType::Known(Etype::Float) => float_text(first.as_f32()).into_bytes(),
            DefType::Known(Etype::Vector) => {
                let parts: Vec<String> = slots.iter().map(|s| float_text(s.as_f32())).collect();
                format!("'{}'", parts.join(" ")).into_bytes()
            }
            DefType::Known(Etype::Pointer) => b"pointer".to_vec(),
            DefType::Unknown(raw) => format!("bad type {raw}").into_bytes(),
        }
    }

    /// `ED_Count` (PHYS-13.7).
    fn edict_count(&mut self) {
        let total = self.num_edicts();
        let (mut active, mut models, mut solid, mut step) = (0, 0, 0, 0);
        for n in 0..total {
            if !self.in_use(n) {
                continue;
            }
            active = add_one(active);
            if self.field(n, "solid").as_f32() != 0.0 {
                solid = add_one(solid);
            }
            if self.field(n, "model").as_i32() != 0 {
                models = add_one(models);
            }
            #[expect(
                clippy::float_cmp_const,
                reason = "spec PHYS-13.7: id's exact movetype comparison"
            )]
            if self.field(n, "movetype").as_f32() == MOVETYPE_STEP {
                step = add_one(step);
            }
        }
        for (label, count) in [
            ("num_edicts", total),
            ("active    ", active),
            ("view      ", models),
            ("touch     ", solid),
            ("step      ", step),
        ] {
            self.print(format!("{label}:{count:3}\n"));
        }
    }

    /// `PR_Profile_f` (PHYS-13.7): the ten functions that ran the most statements since the
    /// last, most first, each `%7i` and its name; every count is then 0.
    fn profile(&mut self, progs: &Progs<'_>) {
        let mut counts: Vec<(u64, usize)> = self
            .builtins
            .profile
            .iter()
            .enumerate()
            .filter(|&(_, &count)| count > 0)
            .map(|(index, &count)| (count, index))
            .collect();
        // id picks the greatest each time, the first of equals by index.
        counts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(count, index) in counts.iter().take(10) {
            let name = progs
                .functions()
                .get(index)
                .and_then(|f| progs.string(f.s_name))
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .unwrap_or_default();
            self.print(format!("{count:7} {name}\n"));
        }
        self.builtins.profile.iter_mut().for_each(|c| *c = 0);
    }

    /// `COM_Path_f` (PHYS-13.7): the search path the host gave, most important first.
    fn path(&mut self) {
        self.print("Current search path:\n");
        let lines: Vec<String> = self.search_path.iter().map(|p| format!("{p}\n")).collect();
        for line in lines {
            self.print(line);
        }
    }
}

#[cfg(test)]
mod tests;
