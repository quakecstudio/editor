// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps and QuakeC changed by another program (P5-17, MAP-32). A file watcher, or a test in its
//! place, tells `files.changed` which files changed; changed QuakeC is read again, so the
//! entity definitions derived from it follow, and the open map is read again or kept as the
//! map editor decides (`qs_map_edit::map_files_changed`).

use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasWorkbench, WorkbenchError};
use qs_map_edit::{MapOnDisk, map_files_changed};

/// The id of `files.changed`.
pub const FILES_CHANGED: &str = "files.changed";

/// The extensions of the files that make up a project's QuakeC.
const QUAKEC_EXTENSIONS: [&str; 4] = ["qc", "src", "qh", "toml"];

/// The parameters of `files.changed`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesChangedParams {
    /// The files another program wrote.
    pub paths: Vec<PathBuf>,
}

/// What `files.changed` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilesChanged {
    /// What happened to the open map.
    pub map: MapOnDisk,
    /// Whether the project's QuakeC was read again, and with it the entity definitions.
    pub quakec: bool,
}

/// Whether `path` is part of a project's QuakeC.
fn is_quakec(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            QUAKEC_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// `files.changed`.
pub(crate) struct FilesChangedCommand;

impl<C: HasWorkbench + 'static> Command<C> for FilesChangedCommand {
    const ID: &'static str = FILES_CHANGED;
    const SUMMARY: &'static str = "Takes files another program changed: the open map is read \
        again when it has no unsaved edits, else kept for map.resolve; changed QuakeC is read \
        again with the entity definitions derived from it.";
    type Params = FilesChangedParams;
    type Output = FilesChanged;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FilesChangedParams,
    ) -> Result<Outcome<FilesChanged, C>, WorkbenchError> {
        // The QuakeC first, so a map that no longer reads still leaves it read again.
        let workbench = context.workbench();
        let quakec = workbench.root().is_some() && params.paths.iter().any(|path| is_quakec(path));
        if quakec {
            workbench.disk_changed()?;
        }
        let map = map_files_changed(context, &params.paths)?;
        Ok(Outcome::outside_history(FilesChanged { map, quakec }))
    }
}

#[cfg(test)]
mod tests;
