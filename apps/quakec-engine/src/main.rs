// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-engine`: the game in its own window, served to the IDE over the bridge (ADR-0004,
//! P3-02).

mod args;
mod audio;
mod bridge;
mod contents;
mod game;
mod headless;
mod keys;
mod load;
mod screenshot;
mod settings;
mod values;
mod view_script;
mod window;

use std::process::ExitCode;

/// A position's or a direction's axes.
pub(crate) const AXES: usize = 3;

/// The log file's stem, beside the IDE's in the app's log directory.
const LOG_FILE_STEM: &str = "quakec-engine";

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
        eprintln!("quakec-engine: logging is off: {error}");
    }
    match args.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the preview host stopped");
            eprintln!("quakec-engine: {error}");
            ExitCode::FAILURE
        }
    }
}

impl args::Args {
    fn run(&self) -> Result<(), String> {
        let args = self;
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
            let loaded = args.load_map(&args.map, None, None, None)?;
            let line = loaded.timerefresh(start.elapsed(), eye)?;
            let mut out = std::io::stdout().lock();
            std::io::Write::write_all(&mut out, format!("{line}\n").as_bytes())
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if let Some([script, audio]) = args
            .view_script
            .as_deref()
            .and_then(|values| <[std::path::PathBuf; 2]>::try_from(values.to_vec()).ok())
        {
            let text = std::fs::read_to_string(&script)
                .map_err(|error| format!("{}: {error}", script.display()))?;
            let script = view_script::ViewScript::parse(&text)?;
            // `load` keeps the sound for a view script (VIEWDIFF-1.3).
            let loaded = args.load_map(&script.map, None, None, None)?;
            let size = args
                .view_size
                .as_deref()
                .and_then(|values| <[u32; 2]>::try_from(values).ok())
                .unwrap_or(headless::VIEW_SIZE);
            return loaded.view_script(&script, &audio, size);
        }
        if args.headless {
            let args = args::Args {
                nosound: true,
                ..args.clone()
            };
            let loaded = args.load_map(&args.map, None, None, None)?;
            let path = args.bridge.as_deref().ok_or("--headless needs -bridge")?;
            return args.run_headless(loaded, listen(path)?);
        }
        let loaded = args.load_map(&args.map, None, None, None)?;
        let bridge = match &args.bridge {
            Some(path) => Some(listen(path)?),
            None => None,
        };
        args.run_window(loaded, bridge)
    }
}

/// Waits for the IDE at the bridge's socket or named pipe (BRIDGE-1.1).
fn listen(path: &std::path::Path) -> Result<bridge::Bridge, String> {
    tracing::info!(path = %path.display(), "waiting for the IDE");
    bridge::Bridge::listen(path).map_err(|e| format!("{}: {e}", path.display()))
}
