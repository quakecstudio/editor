// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's game (ADR-0004, ADR-0028): the game loop on `qs-sim`, `qs-client`,
//! `qs-render` and `qs-sound`, its window, its headless runs and its bridge server. The
//! `quakec-engine` binary runs it in a process of its own, and the IDE runs it in its own
//! window for the embedded game view.

mod args;
mod audio;
mod bridge;
mod contents;
mod error;
mod game;
mod headless;
mod keys;
mod load;
mod map_bsp;
mod screenshot;
mod settings;
mod values;
mod view_script;
mod window;

pub use args::{Args, LookArg, Show};
pub use error::Error;
pub use window::{GameView, Running};

/// A position's or a direction's axes.
pub(crate) const AXES: usize = 3;

impl args::Args {
    /// Runs the host as its command line says: `--timerefresh`, a view script, headless, or
    /// the game in its window, with the bridge served when `-bridge` names its socket.
    ///
    /// # Errors
    ///
    /// Why the game could not load or stopped.
    pub fn run(&self) -> Result<(), Error> {
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
            let loaded = args.load_map(&args.map, None, None, None, None)?;
            let line = loaded.timerefresh(start.elapsed(), eye)?;
            let mut out = std::io::stdout().lock();
            std::io::Write::write_all(&mut out, format!("{line}\n").as_bytes())
                .map_err(|e| Error::Script(e.to_string()))?;
            return Ok(());
        }
        if let Some([script, audio]) = args
            .view_script
            .as_deref()
            .and_then(|values| <[std::path::PathBuf; 2]>::try_from(values.to_vec()).ok())
        {
            let text = std::fs::read_to_string(&script)
                .map_err(|error| format!("{}: {error}", script.display()))
                .map_err(Error::Load)?;
            let script = view_script::ViewScript::parse(&text)?;
            // `load` keeps the sound for a view script (VIEWDIFF-1.3).
            let loaded = args.load_map(&script.map, None, None, None, None)?;
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
            let loaded = args.load_map(&args.map, None, None, None, None)?;
            let path = args
                .bridge
                .as_deref()
                .ok_or_else(|| Error::Usage("--headless needs -bridge".to_owned()))?;
            return args.run_headless(loaded, listen(path)?);
        }
        let loaded = args.load_map(&args.map, None, None, None, None)?;
        let bridge = match &args.bridge {
            Some(path) => Some(listen(path)?),
            None => None,
        };
        args.run_window(loaded, bridge)
    }
}

/// Waits for the IDE at the bridge's socket or named pipe (BRIDGE-1.1).
fn listen(path: &std::path::Path) -> Result<bridge::Bridge, Error> {
    tracing::info!(path = %path.display(), "waiting for the IDE");
    bridge::Bridge::listen(path).map_err(|e| Error::Bridge(format!("{}: {e}", path.display())))
}
