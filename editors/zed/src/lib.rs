// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The Zed extension for QuakeC: it starts `quakec-lsp` for QuakeC files. Every feature is the
//! server's; highlighting is tree-sitter-quakec's, which `extension.toml` names.
//!
//! The binary is `lsp.quakec-lsp.binary.path` in Zed's settings when set, else `quakec-lsp` on
//! the worktree's `PATH`. `lsp.quakec-lsp.initialization_options` is passed to the server as
//! written, with the names `quakec.toml` uses (`apps/lsp/README.md`, Settings).

use zed_extension_api::settings::LspSettings;
use zed_extension_api::{self as zed, LanguageServerId, Result, Worktree};

/// The binary's name on `PATH`, and the server's name in `extension.toml` and Zed's settings.
const SERVER: &str = "quakec-lsp";

struct QuakecExtension;

impl zed::Extension for QuakecExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(SERVER, worktree)?.binary;
        let configured = binary.as_ref().and_then(|b| b.path.clone());
        let Some(command) = configured.or_else(|| worktree.which(SERVER)) else {
            return Err(format!(
                "{SERVER} is not on PATH: build it with `cargo build --release -p quakec-lsp` \
                 and put target/release on PATH, or set lsp.{SERVER}.binary.path"
            ));
        };
        let (args, env) = match binary {
            Some(b) => (
                b.arguments.unwrap_or_default(),
                b.env.unwrap_or_default().into_iter().collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        Ok(zed::Command { command, args, env })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(SERVER, worktree)?.initialization_options)
    }
}

zed::register_extension!(QuakecExtension);
