// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line (ADR-0008, BRIDGE-1.1): `-basedir PATH`, `-game NAME`, `-kpf PATH` and
//! `-bridge PATH`, the engine's own single-dash options, which the IDE launches the host with.

use std::path::PathBuf;

use crate::Error;
use crate::seats::MAX_LOCAL_PLAYERS;
use qs_render::Look;
use qs_sim::MaxPlayers;
use qs_sim::protocol::ProtocolVersion;

/// `Host_FindMaxClients`' slots for a `-listen` with no number, or one below 1 (NET-7.1).
const LISTEN_DEFAULT: i32 = 8;

/// The engine-style options, each written with one dash as the engines and ADR-0008 write
/// them; clap reads them with two.
const ENGINE_OPTIONS: [&str; 6] = [
    "-basedir",
    "-game",
    "-kpf",
    "-bridge",
    "-protocol",
    "-listen",
];

/// `quakec-engine [-basedir PATH] [-game NAME] [-kpf PATH] [-bridge PATH] [-protocol
/// 15|666|999] [map]`.
#[derive(clap::Parser, Clone, Debug, PartialEq)]
#[command(
    name = "quakec-engine",
    about = "The QuakeC Studio engine: the game the IDE previews in"
)]
pub struct Args {
    /// The game root holding `id1`.
    #[arg(long)]
    pub basedir: Option<PathBuf>,
    /// A mod: a single directory name under the game root.
    #[arg(long, value_parser = single_name)]
    pub game: Option<String>,
    /// The rerelease's `QuakeEX.kpf`, read from this path instead of the game root.
    #[arg(long)]
    pub kpf: Option<PathBuf>,
    /// The bridge's socket, where the IDE connects (BRIDGE-1.1).
    #[arg(long)]
    pub bridge: Option<PathBuf>,
    /// The mission pack `rogue`, the engine's `-rogue`.
    #[arg(long)]
    pub rogue: bool,
    /// The mission pack `hipnotic`, the engine's `-hipnotic`.
    #[arg(long)]
    pub hipnotic: bool,
    /// How frames look: QuakeSpasm's (gamma 1, trilinear filtering, glowing fullbrights,
    /// overbright light, the view widened to the window's shape) or GLQuake's (REND-13).
    #[arg(long = "look", value_enum, default_value_t = LookArg::Quakespasm)]
    pub look: LookArg,
    /// GLQuake's `-gamma` (REND-3.1); without it, the look's (REND-13.2).
    #[arg(long)]
    pub gamma: Option<f32>,
    /// The protocol the server writes its messages in, QuakeSpasm's `-protocol`, which sets
    /// `sv_protocol` when the host starts: FitzQuake's 666 (the default), id's 15 or RMQ's 999
    /// (NET-6.11).
    #[arg(long, value_enum, default_value_t = ProtocolArg::FitzQuake)]
    pub protocol: ProtocolArg,
    /// The engine's `-listen [n]`: a server of `n` client slots, 8 with no number or one below
    /// 1, at most 16, and `deathmatch` 1 (NET-7.1), with room for bots (`addbot`, BOT-1.2).
    #[arg(long, num_args = 0..=1, default_missing_value = "8", allow_negative_numbers = true)]
    pub listen: Option<i32>,
    /// Coop: `coop` 1 and `deathmatch` 0 for a new game, as the console's `coop 1` before a
    /// `map` sets them; with `-listen`, in place of its `deathmatch` 1.
    #[arg(long)]
    pub coop: bool,
    /// The local players, 1 to 4 (L-86): the first in the window or the Game tab, each other
    /// one in a window of its own, joined after the first; it needs as many slots
    /// (`-listen`).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=i64::from(MAX_LOCAL_PLAYERS)))]
    pub players: u8,
    /// Bots brought in as the first map starts, `addbot` typed once for each (BOT-1.2); they
    /// take the slots after the local players.
    #[arg(long, default_value_t = 0)]
    pub bots: u8,
    /// How well those bots play, 0 to 3 (BOT-1.4); the game's `skill` setting without it.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3))]
    pub bot_skill: Option<u8>,
    /// Where a free camera starts: x, y, z and yaw.
    #[arg(long, num_args = 4, value_names = ["x", "y", "z", "yaw"], allow_negative_numbers = true)]
    pub at: Option<Vec<f32>>,
    /// Start a free camera at the map's first water or sky.
    #[arg(long, value_enum)]
    pub show: Option<Show>,
    /// No sound device, the engine's `-nosound`.
    #[arg(long)]
    pub nosound: bool,
    /// A CD image's cue sheet whose audio tracks are the music (SND-6.4); without it,
    /// `game.cue` in the game root when there is one, as GOG's release keeps its CD, or a
    /// mission pack's `gamea.cue` or `gamed.cue` (SND-6.5).
    #[arg(long)]
    pub cd_image: Option<PathBuf>,
    /// No CD music, QuakeSpasm's `-nocdaudio`.
    #[arg(long)]
    pub nocdaudio: bool,
    /// No window and no sound: frames of 1/72 of a second, the bridge served between them,
    /// and `screenshot` drawn offscreen; for scenario runs, CI and a client's tests; needs
    /// `-bridge`.
    #[arg(long, requires = "bridge")]
    pub headless: bool,
    /// The game starts paused once the map has loaded, before a frame runs: every entity is
    /// where the map put it until the IDE resumes or steps the game, so a look at the map
    /// shows it before a monster has seen the player. It is the bridge's client that goes
    /// on, over a socket or the pipes of a game embedded in the app: with no bridge the word
    /// does nothing, since nothing could resume the game.
    #[arg(long)]
    pub paused: bool,
    /// A free camera instead of the player.
    #[arg(long)]
    pub fly: bool,
    /// No vsync, a free camera turning slowly, and after this many seconds the frame rate on
    /// stdout and exit (P2-152).
    #[arg(long, value_name = "seconds")]
    pub benchmark: Option<f32>,
    /// id's `timerefresh` (`R_TimeRefresh_f`) with no window, for `cargo xtask bench`: the
    /// map loaded and a second of server frames timed, then 128 frames drawn offscreen from this
    /// eye, the yaw at i/128 of a turn, pitch 0, with no server frame between them, 16 times;
    /// then id's line and a JSON line of the times on stdout (P3-29, ENG-17).
    #[arg(long, num_args = 3, value_names = ["x", "y", "z"], allow_negative_numbers = true)]
    pub timerefresh: Option<Vec<f32>>,
    /// Plays a view script with no window (P3-28): its map, frames, places and shots at 320 by
    /// 240 with no status bar or weapon, and the sound mixed meanwhile written to the second
    /// file, as the view oracle plays the same script in id's engine (VIEWDIFF-1).
    #[arg(long, num_args = 2, value_names = ["script", "audio"])]
    pub view_script: Option<Vec<std::path::PathBuf>>,
    /// The size `--view-script` draws its shots at, 320 by 240 without it (VIEWDIFF-2.1).
    #[arg(long, num_args = 2, value_names = ["width", "height"])]
    pub view_size: Option<Vec<u32>>,
    /// Where the window opens, to put it on a chosen display.
    #[arg(long, num_args = 2, value_names = ["x", "y"], allow_negative_numbers = true)]
    pub position: Option<Vec<f32>>,
    /// The window's size when it opens, in points; 1280 by 800 without it.
    #[arg(long, num_args = 2, value_names = ["width", "height"])]
    pub size: Option<Vec<f32>>,
    /// The map to start on.
    #[arg(default_value = "start")]
    pub map: String,
    /// Any other engine options for the game filesystem, after `--`: `-path`, `-proghack`
    /// and the like, as the engine reads them.
    #[arg(last = true)]
    pub engine: Vec<String>,
}

/// What `--look` names (REND-13).
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LookArg {
    /// QuakeSpasm's look.
    #[default]
    Quakespasm,
    /// GLQuake's look.
    Glquake,
}

impl LookArg {
    /// The renderer's look.
    pub const fn look(self) -> Look {
        match self {
            Self::Quakespasm => Look::QuakeSpasm,
            Self::Glquake => Look::GlQuake,
        }
    }
}

/// What `--protocol` takes (NET-6.1).
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProtocolArg {
    /// `PROTOCOL_NETQUAKE`, id's.
    #[value(name = "15")]
    NetQuake,
    /// `PROTOCOL_FITZQUAKE`.
    #[default]
    #[value(name = "666")]
    FitzQuake,
    /// `PROTOCOL_RMQ`.
    #[value(name = "999")]
    Rmq,
}

impl ProtocolArg {
    /// The protocol it names, the host's first `sv_protocol` (NET-6.11).
    pub const fn version(self) -> ProtocolVersion {
        match self {
            Self::NetQuake => ProtocolVersion::NetQuake,
            Self::FitzQuake => ProtocolVersion::FitzQuake,
            Self::Rmq => ProtocolVersion::Rmq,
        }
    }
}

/// What `--show` starts at.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// The map's first water.
    Water,
    /// The map's first sky.
    Sky,
}

impl Args {
    /// The process's command line.
    pub fn from_env() -> Self {
        <Self as clap::Parser>::parse_from(engine_style(std::env::args()))
    }

    /// The command line `words`, the host's name left out, as the host reads its own.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] with clap's message when the words do not parse.
    pub fn parse_words(words: impl IntoIterator<Item = String>) -> Result<Self, Error> {
        let line = core::iter::once("quakec-engine".to_owned()).chain(words);
        <Self as clap::Parser>::try_parse_from(engine_style(line))
            .map_err(|error| Error::Usage(error.to_string()))
    }

    /// The engine's command line for the game filesystem: `-basedir`, `-game`, `-rogue`,
    /// `-hipnotic` and the options after `--`.
    pub fn engine(&self) -> Vec<String> {
        let mut engine = Vec::new();
        if let Some(basedir) = &self.basedir {
            engine.extend(["-basedir".to_owned(), basedir.display().to_string()]);
        }
        if let Some(game) = &self.game {
            engine.extend(["-game".to_owned(), game.clone()]);
        }
        if self.rogue {
            engine.push("-rogue".to_owned());
        }
        if self.hipnotic {
            engine.push("-hipnotic".to_owned());
        }
        engine.extend(self.engine.iter().cloned());
        engine
    }

    /// The server's slots (NET-7.1): `-listen`'s count, `Host_FindMaxClients`' 8 for none or
    /// fewer than 1, else one.
    pub fn maxplayers(&self) -> MaxPlayers {
        self.listen.map_or(MaxPlayers::SINGLE, |n| {
            MaxPlayers::new(if n < 1 { LISTEN_DEFAULT } else { n })
        })
    }

    /// Whether `maxplayers` slots hold `--players` local players.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] when there are more players than slots.
    pub fn check_players(&self, maxplayers: MaxPlayers) -> Result<(), Error> {
        if usize::from(self.players) > maxplayers.get() {
            return Err(Error::Usage(format!(
                "--players {} needs as many slots: -listen {} or more",
                self.players, self.players
            )));
        }
        Ok(())
    }

    /// The settings a new game of `maxplayers` slots starts with: `deathmatch` 1 for more
    /// than one (NET-7.1), and with `--coop`, `coop` 1 and `deathmatch` 0 (DIFF-3.2).
    pub fn new_game_settings(&self, maxplayers: MaxPlayers) -> Vec<(String, String)> {
        let mut settings = Vec::new();
        if maxplayers.is_multiplayer() {
            settings.push(("deathmatch".to_owned(), "1".to_owned()));
        }
        if self.coop {
            settings.push(("coop".to_owned(), "1".to_owned()));
            settings.push(("deathmatch".to_owned(), "0".to_owned()));
        }
        settings
    }

    /// The console lines that bring `--bots` bots in as the first map starts, each `addbot`
    /// with `--bot-skill` when it is given (BOT-9.3).
    pub fn bot_lines(&self) -> Vec<String> {
        let skill = self
            .bot_skill
            .map_or_else(String::new, |skill| format!(" {skill}"));
        (0..self.bots).map(|_| format!("addbot{skill}\n")).collect()
    }

    /// The cue sheet in the game root whose CD the game's music plays from (SND-6.4, SND-6.5):
    /// GOG's `gamea.cue` for Scourge of Armagon (`-hipnotic`), `gamed.cue` for Dissolution of
    /// Eternity (`-rogue`), else `game.cue`, Quake's own.
    pub fn cd_sheet(&self) -> &'static str {
        let game = self.game.as_deref().map(str::to_ascii_lowercase);
        if self.hipnotic || game.as_deref() == Some("hipnotic") {
            "gamea.cue"
        } else if self.rogue || game.as_deref() == Some("rogue") {
            "gamed.cue"
        } else {
            "game.cue"
        }
    }
}

/// `-game`'s value, a single directory name: "gamedir should be a single directory name, not
/// a path", as QuakeSpasm says (ADR-0008).
fn single_name(text: &str) -> Result<String, Error> {
    let plain = !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\', ':']);
    if plain {
        Ok(text.to_owned())
    } else {
        Err(Error::Usage(
            "gamedir should be a single directory name, not a path".to_owned(),
        ))
    }
}

/// `args` with each of [`ENGINE_OPTIONS`] given its second dash, so clap reads it; every
/// other word is as it was.
pub(crate) fn engine_style(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter()
        .map(|arg| {
            if ENGINE_OPTIONS.contains(&arg.as_str()) {
                format!("-{arg}")
            } else {
                arg
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
