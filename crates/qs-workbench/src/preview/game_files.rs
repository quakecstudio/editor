// SPDX-License-Identifier: GPL-3.0-or-later

//! What game data the preview can play (P3-21): the folder the modder chose, or the first
//! install `qs-install` finds, as found, partial or missing, with the files it lacks
//! (FS-6.5). The first-run panel draws it; the files are read where they are, never copied.

use std::path::{Path, PathBuf};

use qs_engine::Flavour;
use qs_install::{Install, MissingReason};
use qs_vfs::StdFs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether the game data can be played, and what it lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GameState {
    /// The original registered game, or for a project for the rerelease the rerelease: Play
    /// works.
    Found,
    /// The original game's first episode alone, with no `id1/pak1.pak`: Play works on its maps.
    SharewareOnly,
    /// The 2021 rerelease alone, as the Epic Games Store and Game Pass sell it: no original
    /// `id1`, so Play does not work on a project for the original game.
    RereleaseOnly,
    /// No game data.
    Missing,
}

impl GameState {
    /// Whether the preview can play this game data.
    #[must_use]
    pub const fn plays(self) -> bool {
        matches!(self, Self::Found | Self::SharewareOnly)
    }
}

/// The game data a folder holds, or the first install found holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GameFiles {
    /// Whether it can be played.
    pub state: GameState,
    /// The folder looked in, or the install found; `None` when nothing was found anywhere.
    pub folder: Option<PathBuf>,
    /// The original game's files it lacks, relative to the folder, such as `id1/pak1.pak`.
    pub missing: Vec<String>,
}

impl GameFiles {
    /// What `folder` holds, or with `None` the first install found on this machine, for a
    /// project for the game of `flavour` (ADR-0008).
    #[must_use]
    pub fn check(folder: Option<&Path>, flavour: Flavour) -> Self {
        let install = match folder {
            Some(folder) => qs_install::examine_folder(folder),
            None => qs_install::find_installs().into_iter().next(),
        };
        match install {
            Some(install) => Self::of(&install, flavour),
            None => Self {
                state: GameState::Missing,
                folder: folder.map(Path::to_path_buf),
                missing: vec!["id1/pak0.pak".to_owned(), "id1/pak1.pak".to_owned()],
            },
        }
    }

    /// What `install` holds: its missing files (FS-6.5) say whether it is partial. A project
    /// for the rerelease lacks nothing of an install with the rerelease.
    fn of(install: &Install, flavour: Flavour) -> Self {
        if flavour == Flavour::Rerelease && install.rerelease.is_some() {
            return Self {
                state: GameState::Found,
                folder: Some(install.root.clone()),
                missing: Vec::new(),
            };
        }
        let validation = install.validate(&StdFs);
        let reason = |wanted: MissingReason| validation.missing.iter().any(|m| m.reason == wanted);
        let state = if reason(MissingReason::RereleaseOnly) {
            GameState::RereleaseOnly
        } else if reason(MissingReason::SharewareOnly) {
            GameState::SharewareOnly
        } else {
            GameState::Found
        };
        Self {
            state,
            folder: Some(install.root.clone()),
            missing: validation
                .missing
                .iter()
                .map(|m| m.file.to_owned())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
