// SPDX-License-Identifier: GPL-3.0-or-later

//! A map's entities checked against the progs, as the engine reads them when the map loads
//! (MAP-8, P5-04): each entity's `classname` must name a function (EDICT-8.5), each key a field,
//! each value something its field's type reads, and each `target` an entity's `targetname`
//! (EDICT-8.2).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use qs_map::{Entity, MapFile};

use crate::{Db, DefKind, ModuleInput, SimpleType, Type};

/// Keys the compilers and map editors write that are no field (MAP-8.2): qbsp's texture WADs
/// (MAP-6.3) and the editors' format version.
const COMPILER_KEYS: [&str; 2] = ["wad", "mapversion"];

/// The most edits a suggested name may be from the name written (MAP-8.1, MAP-8.2).
const MAX_SUGGESTION_EDITS: usize = 2;

/// How the engine reads a field's value (EDICT-8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldValue {
    /// A string, any text.
    String,
    /// A float, C `atof`.
    Float,
    /// A vector, three `atof` values.
    Vector,
    /// An entity, C `atoi`.
    Entity,
    /// A function, by its name.
    Function,
    /// A field, by its name.
    Field,
    /// Any other type, which the engine reads as nothing a map can write.
    Other,
}

impl FieldValue {
    /// How the engine reads a field of type `ty`, the field's own type `.T`.
    fn of(ty: &Type) -> Self {
        let Type::Field(inner) = ty else {
            return Self::Other;
        };
        match &**inner {
            Type::Simple(SimpleType::String) => Self::String,
            Type::Simple(SimpleType::Float) => Self::Float,
            Type::Simple(SimpleType::Vector) => Self::Vector,
            Type::Simple(SimpleType::Entity) => Self::Entity,
            Type::Function(_) => Self::Function,
            Type::Field(_) => Self::Field,
            _ => Self::Other,
        }
    }
}

/// What the engine looks a map up against: the progs' fields and functions, and the keys each
/// entity class reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntitySchema {
    fields: BTreeMap<String, FieldValue>,
    functions: BTreeSet<String>,
    class_keys: BTreeMap<String, Vec<String>>,
    /// The keys whose value names entities to fire, read from the code (MAP-8.8).
    targeting: BTreeSet<String>,
}

/// One problem MAP-8 finds in a map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapDiagnostic {
    /// The entity, by its place in the file from 0, the world first.
    pub entity: usize,
    /// The 1-based line of the key, or of the entity when it has no such key.
    pub line: u32,
    /// What it is.
    pub kind: MapDiagnosticKind,
}

/// What MAP-8 finds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapDiagnosticKind {
    /// An entity with no `classname` (MAP-8.1).
    NoClassname,
    /// A `classname` naming no function, and the closest one (MAP-8.1).
    UnknownClass {
        /// The classname.
        classname: String,
        /// The function to name instead.
        suggestion: Option<String>,
    },
    /// A key naming no field, and the closest field (MAP-8.2).
    UnknownKey {
        /// The key as written.
        key: String,
        /// The key to write instead.
        suggestion: Option<String>,
    },
    /// A value its field's type does not read (MAP-8.3, MAP-8.4).
    BadValue {
        /// The key.
        key: String,
        /// The value.
        value: String,
        /// How the field reads it.
        expected: FieldValue,
    },
    /// A targeting key naming no entity's `targetname` (MAP-8.9).
    NoTarget {
        /// The key.
        key: String,
        /// The name it targets.
        name: String,
    },
    /// A `targetname` no targeting key names (MAP-8.9).
    Untargeted {
        /// The name.
        name: String,
    },
}

impl fmt::Display for MapDiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suggest = |f: &mut fmt::Formatter<'_>, suggestion: &Option<String>| match suggestion {
            Some(name) => write!(f, "; did you mean {name}?"),
            None => Ok(()),
        };
        match self {
            Self::NoClassname => f.write_str("an entity with no classname, which the game frees"),
            Self::UnknownClass {
                classname,
                suggestion,
            } => {
                write!(
                    f,
                    "no spawn function {classname}, so the game frees the entity"
                )?;
                suggest(f, suggestion)
            }
            Self::UnknownKey { key, suggestion } => {
                write!(f, "{key} is not a field, so the game ignores it")?;
                suggest(f, suggestion)
            }
            Self::BadValue {
                key,
                value,
                expected,
            } => {
                let what = match expected {
                    FieldValue::Float => "a number",
                    FieldValue::Vector => "three numbers",
                    FieldValue::Entity => "a whole number",
                    FieldValue::Function => "the name of a function",
                    FieldValue::Field => "the name of a field",
                    FieldValue::String | FieldValue::Other => "a value of its type",
                };
                write!(f, "{key} is \"{value}\", not {what}")
            }
            Self::NoTarget { key, name } => {
                write!(f, "{key} names {name}, which no entity's targetname holds")
            }
            Self::Untargeted { name } => write!(f, "nothing targets the targetname {name}"),
        }
    }
}

impl Db {
    /// The fields and functions of the module `input` names, and the keys its entity classes
    /// read (MAP-8).
    pub fn entity_schema(&mut self, input: &ModuleInput) -> EntitySchema {
        let mut schema = EntitySchema::default();
        for class in self.entity_classes(input) {
            let keys = class.keys.iter().map(|key| key.name.clone()).collect();
            schema.class_keys.insert(class.name, keys);
        }
        schema.targeting = self.targeting_keys(input).into_iter().collect();
        let analysis = self.analyze_module(input);
        for def in analysis.index.defs() {
            match def.kind {
                DefKind::Field => {
                    schema
                        .fields
                        .entry(def.name.clone())
                        .or_insert_with(|| FieldValue::of(&def.ty));
                }
                DefKind::Function => {
                    schema.functions.insert(def.name.clone());
                }
                DefKind::Global | DefKind::Param | DefKind::Local => {}
            }
        }
        schema
    }
}

impl EntitySchema {
    /// Adds another module's fields, functions and classes, the first module's kept on a clash.
    pub fn extend(&mut self, other: Self) {
        for (name, value) in other.fields {
            self.fields.entry(name).or_insert(value);
        }
        self.functions.extend(other.functions);
        for (name, keys) in other.class_keys {
            self.class_keys.entry(name).or_insert(keys);
        }
        self.targeting.extend(other.targeting);
    }

    /// Whether the code fires the entities whose `targetname` the key `key` names (MAP-8.8).
    #[must_use]
    pub fn is_targeting(&self, key: &str) -> bool {
        self.targeting.contains(key)
    }

    /// The keys the code fires entities by, sorted (MAP-8.8).
    #[must_use]
    pub fn targeting_keys(&self) -> Vec<String> {
        self.targeting.iter().cloned().collect()
    }

    /// Whether the progs has a field named `name`, which a map's key sets (EDICT-8.2).
    #[must_use]
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.contains_key(name)
    }

    /// Every MAP-8 diagnostic of `map`, entity by entity in file order, then the targetnames
    /// nothing targets.
    #[must_use]
    pub fn check(&self, map: &MapFile) -> Vec<MapDiagnostic> {
        let mut out = Vec::new();
        let mut targetnames: Vec<(usize, u32, String)> = Vec::new();
        let mut targeted: BTreeSet<String> = BTreeSet::new();
        let mut targets: Vec<(usize, u32, String, String)> = Vec::new();
        for (index, entity) in map.entities.iter().enumerate() {
            self.check_class(index, entity, &mut out);
            for pair in entity.pairs() {
                let key = String::from_utf8_lossy(pair.key()).into_owned();
                let value = String::from_utf8_lossy(pair.value()).into_owned();
                if key == "targetname" {
                    targetnames.push((index, pair.line(), value.clone()));
                } else if self.is_targeting(&key) {
                    targeted.insert(value.clone());
                    targets.push((index, pair.line(), key.clone(), value.clone()));
                }
                let push = |out: &mut Vec<MapDiagnostic>, kind| {
                    out.push(MapDiagnostic {
                        entity: index,
                        line: pair.line(),
                        kind,
                    });
                };
                if let Some(kind) = self.check_pair(entity, &key, &value) {
                    push(&mut out, kind);
                }
            }
        }
        let names: BTreeSet<&str> = targetnames.iter().map(|(_, _, n)| n.as_str()).collect();
        for (entity, line, key, name) in targets {
            if !name.is_empty() && !names.contains(name.as_str()) {
                out.push(MapDiagnostic {
                    entity,
                    line,
                    kind: MapDiagnosticKind::NoTarget { key, name },
                });
            }
        }
        for (entity, line, name) in targetnames {
            if !name.is_empty() && !targeted.contains(&name) {
                out.push(MapDiagnostic {
                    entity,
                    line,
                    kind: MapDiagnosticKind::Untargeted { name },
                });
            }
        }
        out
    }

    /// MAP-8.1 for one entity.
    fn check_class(&self, index: usize, entity: &Entity, out: &mut Vec<MapDiagnostic>) {
        let Some(classname) = entity.classname() else {
            out.push(MapDiagnostic {
                entity: index,
                line: entity.line(),
                kind: MapDiagnosticKind::NoClassname,
            });
            return;
        };
        let classname = String::from_utf8_lossy(classname).into_owned();
        if self.functions.contains(&classname) {
            return;
        }
        let line = entity
            .pairs()
            .find(|pair| pair.key() == b"classname")
            .map_or_else(|| entity.line(), qs_map::Pair::line);
        let suggestion = closest(&classname, self.functions.iter(), |_| false);
        out.push(MapDiagnostic {
            entity: index,
            line,
            kind: MapDiagnosticKind::UnknownClass {
                classname,
                suggestion,
            },
        });
    }

    /// MAP-8.2 to MAP-8.4 for one pair.
    fn check_pair(&self, entity: &Entity, key: &str, value: &str) -> Option<MapDiagnosticKind> {
        // EDICT-8.2: trailing spaces go, `_` keys are ignored, `angle` and `light` renamed.
        let looked_up = key.trim_end_matches(' ');
        if looked_up.starts_with('_') || COMPILER_KEYS.contains(&looked_up) {
            return None;
        }
        let looked_up = match looked_up {
            "angle" => "angles",
            "light" => "light_lev",
            other => other,
        };
        let Some(&field) = self.fields.get(looked_up) else {
            let class = entity
                .classname()
                .map(|c| String::from_utf8_lossy(c).into_owned());
            let reads = class.and_then(|c| self.class_keys.get(&c));
            let suggestion = closest(key, self.fields.keys(), |name| {
                reads.is_some_and(|keys| keys.iter().any(|k| k == name))
            });
            return Some(MapDiagnosticKind::UnknownKey {
                key: key.to_owned(),
                suggestion,
            });
        };
        // `angle` is one number however `angles` reads (EDICT-8.2).
        let field = if key == "angle" {
            FieldValue::Float
        } else {
            field
        };
        let readable = match field {
            FieldValue::String | FieldValue::Other => true,
            FieldValue::Float => is_number(value),
            FieldValue::Vector => {
                let parts: Vec<&str> = value.split(' ').collect();
                parts.len() == qc_types::PROGS_VECTOR_SLOTS
                    && parts.iter().all(|part| is_number(part))
            }
            FieldValue::Entity => !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
            FieldValue::Function => self.functions.contains(value),
            FieldValue::Field => self.fields.contains_key(value),
        };
        (!readable).then(|| MapDiagnosticKind::BadValue {
            key: key.to_owned(),
            value: value.to_owned(),
            expected: field,
        })
    }
}

/// Whether `text` is a whole decimal number as a mapper writes one: digits with an optional
/// sign, point and exponent, so `atof` reads all of it (MAP-8.3).
fn is_number(text: &str) -> bool {
    let starts = text
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.');
    starts && text.parse::<f64>().is_ok_and(f64::is_finite)
}

/// The name of `names` closest to `written`, at most [`MAX_SUGGESTION_EDITS`] away and not
/// `written` itself; at one distance, a name `preferred` holds first, then the first in order.
fn closest<'a>(
    written: &str,
    names: impl Iterator<Item = &'a String>,
    preferred: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut best: Option<(usize, bool, &String)> = None;
    for name in names {
        let edits = distance(&written.to_ascii_lowercase(), &name.to_ascii_lowercase());
        if edits > MAX_SUGGESTION_EDITS || name == written {
            continue;
        }
        let rank = (edits, !preferred(name));
        if best.is_none_or(|(e, p, _)| rank < (e, p)) {
            best = Some((edits, !preferred(name), name));
        }
    }
    best.map(|(_, _, name)| name.clone())
}

/// The Levenshtein distance between `a` and `b`, by bytes.
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut previous = row.first().copied().unwrap_or_default();
        if let Some(first) = row.first_mut() {
            *first = i.saturating_add(1);
        }
        for (j, &cb) in b.iter().enumerate() {
            let above = row.get(j.saturating_add(1)).copied().unwrap_or_default();
            let left = row.get(j).copied().unwrap_or_default();
            let cost = usize::from(ca != cb);
            let value = previous
                .saturating_add(cost)
                .min(above.saturating_add(1))
                .min(left.saturating_add(1));
            previous = above;
            if let Some(cell) = row.get_mut(j.saturating_add(1)) {
                *cell = value;
            }
        }
    }
    row.last().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests;
