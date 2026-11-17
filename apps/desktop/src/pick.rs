// SPDX-License-Identifier: GPL-3.0-or-later

//! What a click in the map view can give a parameter the command line asks for (UI-23, #971):
//! read from the parameter's JSON schema, an index by the name of its definition and a point
//! by its shape.

use serde_json::Value;

/// What the map view answers the command line with while it asks for what a click can give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The value clicked, as it is typed.
    Value(String),
    /// Escape in the view: the command being entered is given up.
    GaveUp,
}

/// What a click in the map view can give the parameter the line asks for (UI-23).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// An entity: its index.
    Entity,
    /// A brush: its index in its entity.
    Brush,
    /// A face: its index in its brush.
    FaceIndex,
    /// A face: its entity, brush and face.
    Face,
    /// A point on the grid: its three coordinates.
    Point,
}

impl Pick {
    /// What the parameter `name` of a command whose parameters `schema` describes can be
    /// picked as.
    pub(crate) fn asked(schema: &Value, name: &str) -> Option<Self> {
        let pick = Self::of(schema.get(PROPERTIES)?.get(name)?)?;
        // Three numbers are a place only by name: a distance or a scale is not clicked.
        (pick != Self::Point || PLACES.contains(&name)).then_some(pick)
    }

    /// What the property `schema` of a command's parameters can be picked as: an index by
    /// the name of its definition, a point by its shape, three numbers; and so for a
    /// parameter that may be left out, which is one of those or nothing.
    fn of(schema: &Value) -> Option<Self> {
        if let Some(options) = schema.get(ANY_OF).and_then(Value::as_array) {
            return options.iter().find_map(Self::of);
        }
        let named = schema
            .get(REF)
            .and_then(Value::as_str)
            .and_then(|reference| reference.rsplit('/').next());
        match named {
            Some(ENTITY_INDEX) => return Some(Self::Entity),
            Some(BRUSH_INDEX) => return Some(Self::Brush),
            Some(FACE_INDEX) => return Some(Self::FaceIndex),
            Some(FACE_REF) => return Some(Self::Face),
            _ => {}
        }
        let count = |key: &str| schema.get(key).and_then(Value::as_u64);
        let numbers = schema
            .get(ITEMS)
            .and_then(|items| items.get(TYPE))
            .and_then(Value::as_str)
            == Some(NUMBER);
        (numbers && count(MIN_ITEMS) == Some(POINT) && count(MAX_ITEMS) == Some(POINT))
            .then_some(Self::Point)
    }
}

/// The words of a JSON schema a parameter is read by, and the definitions that are picked.
const REF: &str = "$ref";
const ANY_OF: &str = "anyOf";
const ITEMS: &str = "items";
const TYPE: &str = "type";
const NUMBER: &str = "number";
const MIN_ITEMS: &str = "minItems";
const MAX_ITEMS: &str = "maxItems";
const PROPERTIES: &str = "properties";
const ENTITY_INDEX: &str = "EntityIndex";
const BRUSH_INDEX: &str = "BrushIndex";
const FACE_INDEX: &str = "FaceIndex";
const FACE_REF: &str = "FaceRef";
/// The parameters that are places in the map: a point entity's origin, a box's corners, a
/// vertex and where it goes, the player's eye.
const PLACES: [&str; 6] = ["origin", "min", "max", "from", "to", "eye"];
/// How many coordinates a point has.
const POINT: u64 = 3;
