// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line (ADR-0008, BRIDGE-1.1): `-basedir PATH`, `-game NAME`, `-kpf PATH` and
//! `-bridge PATH`, the engine's own single-dash options, which the IDE launches the host with.

use std::path::PathBuf;

use qs_render::Look;

/// The engine-style options, each written with one dash as the engines and ADR-0008 write
/// them; clap reads them with two.
const ENGINE_OPTIONS: [&str; 4] = ["-basedir", "-game", "-kpf", "-bridge"];

/// `preview-host [-basedir PATH] [-game NAME] [-kpf PATH] [-bridge PATH] [map]`.
#[derive(clap::Parser, Clone, Debug, PartialEq)]
#[command(name = "preview-host", about = "The QuakeC Studio preview host")]
pub(crate) struct Args {
    /// The game root holding `id1`.
    #[arg(long)]
    pub(crate) basedir: Option<PathBuf>,
    /// A mod: a single directory name under the game root.
    #[arg(long, value_parser = single_name)]
    pub(crate) game: Option<String>,
    /// The rerelease's `QuakeEX.kpf`, read from this path instead of the game root.
    #[arg(long)]
    pub(crate) kpf: Option<PathBuf>,
    /// The bridge's socket, where the IDE connects (BRIDGE-1.1).
    #[arg(long)]
    pub(crate) bridge: Option<PathBuf>,
    /// The mission pack `rogue`, the engine's `-rogue`.
    #[arg(long)]
    pub(crate) rogue: bool,
    /// The mission pack `hipnotic`, the engine's `-hipnotic`.
    #[arg(long)]
    pub(crate) hipnotic: bool,
    /// How frames look: QuakeSpasm's (gamma 1, trilinear filtering, glowing fullbrights,
    /// overbright light, the view widened to the window's shape) or GLQuake's (REND-13).
    #[arg(long = "look", value_enum, default_value_t = LookArg::Quakespasm)]
    pub(crate) look: LookArg,
    /// GLQuake's `-gamma` (REND-3.1); without it, the look's (REND-13.2).
    #[arg(long)]
    pub(crate) gamma: Option<f32>,
    /// Brush models without QuakeSpasm's `gl_zfix` (REND-1.12).
    #[arg(long)]
    pub(crate) nozfix: bool,
    /// Where a free camera starts: x, y, z and yaw.
    #[arg(long, num_args = 4, value_names = ["x", "y", "z", "yaw"], allow_negative_numbers = true)]
    pub(crate) at: Option<Vec<f32>>,
    /// Start a free camera at the map's first water or sky.
    #[arg(long, value_enum)]
    pub(crate) show: Option<Show>,
    /// No sound device, the engine's `-nosound`.
    #[arg(long)]
    pub(crate) nosound: bool,
    /// No window and no sound: frames of 1/72 of a second, the bridge served between them,
    /// and `screenshot` drawn offscreen; for scenario runs, CI and a client's tests; needs
    /// `-bridge`.
    #[arg(long, requires = "bridge")]
    pub(crate) headless: bool,
    /// A free camera instead of the player.
    #[arg(long)]
    pub(crate) fly: bool,
    /// No vsync, a free camera turning slowly, and after this many seconds the frame rate on
    /// stdout and exit (P2-152).
    #[arg(long, value_name = "seconds")]
    pub(crate) benchmark: Option<f32>,
    /// id's `timerefresh` (`R_TimeRefresh_f`) with no window, for `cargo xtask bench`: the
    /// map loaded and a second of server frames timed, then 128 frames drawn offscreen from this
    /// eye, the yaw at i/128 of a turn, pitch 0, with no server frame between them, 16 times;
    /// then id's line and a JSON line of the times on stdout (P3-29, ENG-17).
    #[arg(long, num_args = 3, value_names = ["x", "y", "z"], allow_negative_numbers = true)]
    pub(crate) timerefresh: Option<Vec<f32>>,
    /// Plays a view script with no window (P3-28): its map, frames, places and shots at 320 by
    /// 240 with no status bar or weapon, and the sound mixed meanwhile written to the second
    /// file, as the view oracle plays the same script in id's engine (VIEWDIFF-1).
    #[arg(long, num_args = 2, value_names = ["script", "audio"])]
    pub(crate) view_script: Option<Vec<std::path::PathBuf>>,
    /// Where the window opens, to put it on a chosen display.
    #[arg(long, num_args = 2, value_names = ["x", "y"], allow_negative_numbers = true)]
    pub(crate) position: Option<Vec<f32>>,
    /// The window's size when it opens, in points; 1280 by 800 without it.
    #[arg(long, num_args = 2, value_names = ["width", "height"])]
    pub(crate) size: Option<Vec<f32>>,
    /// The map to start on.
    #[arg(default_value = "start")]
    pub(crate) map: String,
    /// Any other engine options for the game filesystem, after `--`: `-path`, `-proghack`
    /// and the like, as the engine reads them.
    #[arg(last = true)]
    pub(crate) engine: Vec<String>,
}

/// What `--look` names (REND-13).
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LookArg {
    /// QuakeSpasm's look.
    #[default]
    Quakespasm,
    /// GLQuake's look.
    Glquake,
}

impl LookArg {
    /// The renderer's look.
    pub(crate) const fn look(self) -> Look {
        match self {
            Self::Quakespasm => Look::QuakeSpasm,
            Self::Glquake => Look::GlQuake,
        }
    }
}

/// What `--show` starts at.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Show {
    /// The map's first water.
    Water,
    /// The map's first sky.
    Sky,
}

impl Args {
    /// The process's command line.
    pub(crate) fn from_env() -> Self {
        <Self as clap::Parser>::parse_from(engine_style(std::env::args()))
    }

    /// The engine's command line for the game filesystem: `-basedir`, `-game`, `-rogue`,
    /// `-hipnotic` and the options after `--`.
    pub(crate) fn engine(&self) -> Vec<String> {
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
}

/// `-game`'s value, a single directory name: "gamedir should be a single directory name, not
/// a path", as QuakeSpasm says (ADR-0008).
fn single_name(text: &str) -> Result<String, String> {
    let plain = !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\', ':']);
    if plain {
        Ok(text.to_owned())
    } else {
        Err("gamedir should be a single directory name, not a path".to_owned())
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
