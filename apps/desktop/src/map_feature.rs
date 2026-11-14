// SPDX-License-Identifier: GPL-3.0-or-later

//! The map's feature (P6-41): the map view, the map editor and what it last showed, the open
//! map's backups and disk watch, and the Entities outliner and Properties panel, which the
//! shell used to hold.

use std::path::PathBuf;

use qs_map_edit::MapRevision;

use crate::disk_watch::DiskWatch;
use crate::entities::{Outliner, Properties};
use crate::map_editor::MapEditor;
use crate::map_view::MapView;

/// The map's state.
#[derive(Default)]
pub(crate) struct MapFeature {
    /// The map view, once a map is shown.
    pub(crate) map: Option<MapView>,
    /// The map editor's view and key editor (P5-06).
    pub(crate) map_editor: MapEditor,
    /// The map the map editor last showed, to notice one newly opened.
    pub(crate) map_opened: Option<PathBuf>,
    /// When the open map was last backed up, in egui seconds, and at which of its edits
    /// (P6-20).
    pub(crate) backed_up: (f64, MapRevision),
    /// The modified times of the open map and the project's QuakeC, to notice another
    /// program's writes (P5-17).
    pub(crate) disk_watch: DiskWatch,
    /// The Entities outliner's filter.
    pub(crate) outliner: Outliner,
    /// The Properties panel's edits.
    pub(crate) properties: Properties,
}
