// SPDX-License-Identifier: GPL-3.0-or-later

//! What a map's entities mean to a QuakeC module: the entity classes its spawn functions
//! define and their export as an FGD file ([`entities`], P5-03, MAP-1), and a map's entities
//! checked against the progs as the engine reads them when the map loads ([`map_check`],
//! P5-04, MAP-8). Both read the module through `qc-sema`'s semantic model and its intermediate
//! layer; the map is `qs-map`'s.

pub mod entities;
pub mod map_check;

pub use entities::{EntityClass, Fgd, Found, Key, KeyType, Shape, SpawnFlag};
pub use map_check::{EntitySchema, FieldValue, MapDiagnostic, MapDiagnosticKind};
