// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-lsp`, the QuakeC language server (LANG-7, LANG-9).
//!
//! The same editor features QuakeC Studio shows, from the same crates, over the Language
//! Server Protocol 3.18 (`specs/lsp/3.18/`), so VS Code, Zed, Neovim and any other client get
//! them: go-to-definition, find-references, rename, hover with the engine-target database's
//! entry for a builtin, completion, semantic tokens, parameter-name inlay hints and
//! diagnostics. The analysis is `qc-ide`'s; this crate is the protocol: the base protocol's
//! framing ([`transport`]), the lifecycle and one handler per request ([`server`]), and the
//! conversion of byte offsets and paths to the protocol's positions ([`lines`]) and URIs
//! ([`uri`]). This is rust-analyzer's split of its `rust-analyzer` binary from its `ide` crate.
//!
//! The server speaks over standard input and output. A project with no `quakec.toml`, such as
//! the rerelease, is given its dialect, target engine and autoprototyping in the `initialize`
//! request's `initializationOptions`: `{"dialect": "fteqcc", "target": "rerelease",
//! "autoproto": false}`, the names a `quakec.toml` uses (PRJ-2.8, PRJ-2.9).
//!
//! # Layer
//!
//! apps. It depends on `qc-ide` (L3), `qc-project` (L2), and `qs-log` and `qs-paths` (L0).

pub mod lines;
pub mod proto;
mod render;
pub mod server;
pub mod transport;
pub mod uri;

#[cfg(test)]
mod tests;

use std::io::{BufRead, Write};

use qc_project::Loader;
use qs_log::LogConfig;
use qs_paths::AppDirs;

pub use server::{Exit, Server};

/// The server's log file's name without its extension: its own, since the IDE rotates
/// `quakec-studio.log` in the same directory.
pub const LOG_FILE_STEM: &str = "quakec-lsp";

/// Where the server logs (QA-7): `quakec-lsp.log` in the application's log directory, with
/// directives from `RUST_LOG`, and a crash record there when it panics.
#[must_use]
pub fn log_config(dirs: &AppDirs) -> LogConfig {
    LogConfig {
        file_stem: LOG_FILE_STEM.to_owned(),
        ..LogConfig::for_app(dirs)
    }
}
pub use transport::TransportError;

/// Serves one client: reads messages from `input` and writes answers and notifications to
/// `output` until `exit` or the end of the input, which is a failure (LSP 3.18, "Exit
/// Notification").
///
/// # Errors
///
/// [`TransportError`] when a message cannot be read or an answer written.
pub fn run<L: Loader, R: BufRead, W: Write>(
    loader: L,
    mut input: R,
    output: W,
) -> Result<Exit, TransportError> {
    let mut server = Server::new(loader, output);
    while let Some(content) = transport::read_message(&mut input)? {
        if let Some(exit) = server.handle(&content)? {
            return Ok(exit);
        }
    }
    Ok(Exit::Failure)
}
