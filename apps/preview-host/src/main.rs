// SPDX-License-Identifier: GPL-3.0-or-later

//! `preview-host`: the game in its own window, served to the IDE over the bridge (ADR-0004,
//! P3-02).

mod args;
mod bridge;
mod game;
mod values;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = args::Args::from_env();
    tracing::info!(?args, "preview host");
    ExitCode::SUCCESS
}
