// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line (ADR-0008, BRIDGE-1.1): `-basedir PATH`, `-game NAME`, `-kpf PATH` and
//! `-bridge PATH`, the engine's own single-dash options, which the IDE launches the host with.

use std::path::PathBuf;

use qs_render::GAMMA;

/// The engine-style options, each written with one dash as the engines and ADR-0008 write
/// them; clap reads them with two.
const ENGINE_OPTIONS: [&str; 4] = ["-basedir", "-game", "-kpf", "-bridge"];

/// `preview-host [-basedir PATH] [-game NAME] [-kpf PATH] [-bridge PATH] [map]`.
#[derive(clap::Parser, Debug, PartialEq)]
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
    /// GLQuake's `-gamma` (REND-3.1).
    #[arg(long, default_value_t = GAMMA)]
    pub(crate) gamma: f32,
    /// Brush models without QuakeSpasm's `gl_zfix` (REND-1.12).
    #[arg(long)]
    pub(crate) nozfix: bool,
    /// Where a free camera starts: x, y, z and yaw.
    #[arg(long, num_args = 4, value_names = ["x", "y", "z", "yaw"], allow_negative_numbers = true)]
    pub(crate) at: Option<Vec<f32>>,
    /// Start a free camera at the map's first water or sky.
    #[arg(long, value_enum)]
    pub(crate) show: Option<Show>,
    /// A free camera instead of the player.
    #[arg(long)]
    pub(crate) fly: bool,
    /// No vsync, a free camera turning slowly, and after this many seconds the frame rate on
    /// stdout and exit (P2-152).
    #[arg(long, value_name = "seconds")]
    pub(crate) benchmark: Option<f32>,
    /// Where the window opens, to put it on a chosen display.
    #[arg(long, num_args = 2, value_names = ["x", "y"], allow_negative_numbers = true)]
    pub(crate) position: Option<Vec<f32>>,
    /// The map to start on.
    #[arg(default_value = "start")]
    pub(crate) map: String,
    /// Any other engine options for the game filesystem, after `--`: `-path`, `-proghack`
    /// and the like, as the engine reads them.
    #[arg(last = true)]
    pub(crate) engine: Vec<String>,
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
