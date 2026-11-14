// SPDX-License-Identifier: GPL-3.0-or-later

//! An entity class as the map editor shows it: its keys, typed, and its spawnflags. The
//! project a map belongs to says what its classes are (`MapProject`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One entity class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClassInfo {
    /// The classname, its spawn function's name.
    pub name: String,
    /// Whether a `/*QUAKED*/` comment documents it ("documented") or only the engine calls
    /// it ("unreferenced").
    pub found: String,
    /// "point" or "brush".
    pub shape: String,
    /// Its colour, each component 0 to 1.
    pub color: Option<[f32; 3]>,
    /// Its box, the smallest corner first.
    pub size: Option<[[f32; 3]; 2]>,
    /// Its model.
    pub model: Option<String>,
    /// Its comment's text, by line.
    pub doc: Vec<String>,
    /// The keys it reads.
    pub keys: Vec<KeyInfo>,
    /// Its spawnflags, by value.
    pub spawnflags: Vec<FlagInfo>,
}

/// A key a class reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KeyInfo {
    /// The key, a field's name.
    pub name: String,
    /// Its type: float, vector, string, entity, function or other.
    pub ty: String,
    /// The value the class gives it when the map does not.
    pub default: Option<String>,
}

/// A spawnflag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FlagInfo {
    /// Its value.
    pub value: u32,
    /// Its name, when the code or the comment gives one.
    pub name: Option<String>,
}
