// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-engine`: the game in its own window, served to the IDE over the bridge (ADR-0004,
//! P3-02). The game itself is `qs-game`, which the IDE also runs in its own window (ADR-0028).

use std::process::ExitCode;

/// The log file's stem, beside the IDE's in the app's log directory.
const LOG_FILE_STEM: &str = "quakec-engine";

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports why the game could not start, or why logging is \
              off, to the person who ran it"
)]
fn main() -> ExitCode {
    let args = qs_game::Args::from_env();
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
