// SPDX-License-Identifier: GPL-3.0-or-later

//! `preview-host`: the game in its own window, served to the IDE over the bridge (ADR-0004,
//! P3-02).

mod args;
mod audio;
mod bridge;
mod contents;
mod game;
mod headless;
mod load;
mod screenshot;
mod values;
mod window;

use std::process::ExitCode;

/// The log file's stem, beside the IDE's in the app's log directory.
const LOG_FILE_STEM: &str = "preview-host";

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports why the game could not start, or why logging is \
              off, to the person who ran it"
)]
fn main() -> ExitCode {
    let args = args::Args::from_env();
    let logging = qs_paths::current()
        .map_err(|error| error.to_string())
        .and_then(|dirs| {
            let config = qs_log::LogConfig {
                file_stem: LOG_FILE_STEM.to_owned(),
                ..qs_log::LogConfig::for_app(&dirs)
            };
            qs_log::init(&config).map_err(|error| error.to_string())
        });
    if let Err(error) = logging {
        eprintln!("preview-host: logging is off: {error}");
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the preview host stopped");
            eprintln!("preview-host: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &args::Args) -> Result<(), String> {
    if let Some(eye) = args
        .timerefresh
        .as_deref()
        .and_then(|values| <[f32; 3]>::try_from(values).ok())
    {
        let args = args::Args {
            nosound: true,
            ..args.clone()
        };
        let start = std::time::Instant::now();
        let loaded = load::load(&args, &args.map, None, None, None)?;
        let line = headless::timerefresh(loaded, start.elapsed(), eye)?;
        let mut out = std::io::stdout().lock();
        std::io::Write::write_all(&mut out, format!("{line}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    if args.headless {
        let args = args::Args {
            nosound: true,
            ..args.clone()
        };
        let loaded = load::load(&args, &args.map, None, None, None)?;
        let path = args.bridge.as_deref().ok_or("--headless needs -bridge")?;
        return headless::run(&args, loaded, listen(path)?);
    }
    let loaded = load::load(args, &args.map, None, None, None)?;
    let bridge = match &args.bridge {
        Some(path) => Some(listen(path)?),
        None => None,
    };
    window::run(args, loaded, bridge)
}

/// Waits for the IDE at the bridge's socket or named pipe (BRIDGE-1.1).
fn listen(path: &std::path::Path) -> Result<bridge::Bridge, String> {
    tracing::info!(path = %path.display(), "waiting for the IDE");
    bridge::Bridge::listen(path).map_err(|e| format!("{}: {e}", path.display()))
}
