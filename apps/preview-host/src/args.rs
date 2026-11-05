// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line (ADR-0008, BRIDGE-1.1): `-basedir PATH`, `-game NAME`, `-kpf PATH` and
//! `-bridge PATH`, the engine's own single-dash options, which the IDE launches the host with.

use std::path::PathBuf;

/// The engine-style options, each written with one dash as the engines and ADR-0008 write
/// them; clap reads them with two.
const ENGINE_OPTIONS: [&str; 4] = ["-basedir", "-game", "-kpf", "-bridge"];

/// `preview-host [-basedir PATH] [-game NAME] [-kpf PATH] [-bridge PATH] [map]`.
#[derive(clap::Parser, Debug, PartialEq, Eq)]
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
    /// The map to start on.
    #[arg(default_value = "start")]
    pub(crate) map: String,
}

impl Args {
    /// The process's command line.
    pub(crate) fn from_env() -> Self {
        <Self as clap::Parser>::parse_from(engine_style(std::env::args()))
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
