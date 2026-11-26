// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.entities`: the entity classes the open project's server QuakeC defines, read from
//! its spawn functions (`qc_entities::entities`, P5-03), and written as an FGD file for other map
//! editors when asked.

use std::path::PathBuf;

use qc_entities::{EntityClass, Fgd, Found, Key, KeyType, Shape};
use qc_project::ModuleKind;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasWorkbench, Workbench, WorkbenchError};
use qs_map_edit::{ClassInfo, FlagInfo, KeyInfo};
use qs_sim::spawn::{
    EDICT_SPAWNFLAG_NOT_DEATHMATCH, EDICT_SPAWNFLAG_NOT_EASY, EDICT_SPAWNFLAG_NOT_HARD,
    EDICT_SPAWNFLAG_NOT_MEDIUM,
};

/// The id of `project.entities`.
pub const PROJECT_ENTITIES: &str = "project.entities";

/// The parameters of `project.entities`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntitiesParams {
    /// Write the classes to this FGD file too.
    #[serde(default)]
    pub fgd: Option<PathBuf>,
}

/// What `project.entities` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EntitiesFound {
    /// Every class, by classname.
    pub classes: Vec<ClassInfo>,
    /// The FGD file written, when one was asked for.
    pub fgd: Option<PathBuf>,
    /// The keys the code fires entities by, sorted (MAP-8.8).
    pub targeting: Vec<String>,
}

/// `class` as the map editor and `project.entities` give it.
pub(crate) fn class_info(class: &EntityClass) -> ClassInfo {
    ClassInfo {
        name: class.name.clone(),
        found: match class.found {
            Found::Documented => "documented",
            Found::Unreferenced => "unreferenced",
        }
        .to_owned(),
        shape: match class.shape {
            Shape::Point => "point",
            Shape::Brush => "brush",
        }
        .to_owned(),
        color: class.color,
        size: class.size,
        model: class.model.clone(),
        doc: class.doc.clone(),
        keys: class
            .keys
            .iter()
            .map(|key| KeyInfo {
                name: key.name.clone(),
                ty: match key.ty {
                    KeyType::Float => "float",
                    KeyType::Vector => "vector",
                    KeyType::String => "string",
                    KeyType::Entity => "entity",
                    KeyType::Function => "function",
                    KeyType::Other => "other",
                }
                .to_owned(),
                default: key.default.clone(),
                comment: key.comment.clone(),
            })
            .collect(),
        spawnflags: spawnflags(class),
    }
}

/// The flags the engine itself tests on every entity (EDICT-8.4), each named as the modder
/// reads it: freed on a skill, or in deathmatch. `SPAWNFLAG_NOT_HARD` frees it on skill "2 or
/// more", so on nightmare too.
const ENGINE_FLAGS: [(i32, &str); 4] = [
    (EDICT_SPAWNFLAG_NOT_EASY, "Not in easy"),
    (EDICT_SPAWNFLAG_NOT_MEDIUM, "Not in normal"),
    (EDICT_SPAWNFLAG_NOT_HARD, "Not in hard or nightmare"),
    (EDICT_SPAWNFLAG_NOT_DEATHMATCH, "Not in deathmatch"),
];

/// The world's class, which is the level itself: no skill or game mode leaves it out.
const WORLDSPAWN: &str = "worldspawn";

/// `class`'s spawnflags by value, then the engine's (EDICT-8.4) where the class uses none of
/// their bits, so a monster is kept off a skill by a tick rather than a number typed; the
/// world's class has its own alone.
fn spawnflags(class: &EntityClass) -> Vec<FlagInfo> {
    let mut flags: Vec<FlagInfo> = class
        .spawnflags
        .iter()
        .map(|flag| FlagInfo {
            value: flag.value,
            name: flag.name.clone(),
        })
        .collect();
    if class.name == WORLDSPAWN {
        return flags;
    }
    for (value, name) in ENGINE_FLAGS {
        let value = value.unsigned_abs();
        if !flags.iter().any(|flag| flag.value == value) {
            flags.push(FlagInfo {
                value,
                name: Some(name.to_owned()),
            });
        }
    }
    flags
}

impl<L: crate::Loader> Workbench<L> {
    /// The entity classes of the project's server modules, by classname; the first module's
    /// class of a name when two define it.
    pub fn entity_classes(&mut self) -> Result<Vec<EntityClass>, WorkbenchError> {
        if self.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let mut classes: Vec<EntityClass> = Vec::new();
        for module in self.host.project_modules() {
            if module.kind != ModuleKind::Ssqc {
                continue;
            }
            for class in EntityClass::of_module(module.db, module.input) {
                if !classes.iter().any(|known| known.name == class.name) {
                    classes.push(class);
                }
            }
        }
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(classes)
    }

    /// The keys the project's server QuakeC fires entities by, sorted (MAP-8.8).
    pub fn targeting_keys(&mut self) -> Vec<String> {
        let mut keys = std::collections::BTreeSet::new();
        for module in self.host.project_modules() {
            if module.kind == ModuleKind::Ssqc {
                keys.extend(Key::targeting(module.db, module.input));
            }
        }
        keys.into_iter().collect()
    }
}

/// `project.entities`.
struct ProjectEntities;

impl<C: HasWorkbench + 'static> Command<C> for ProjectEntities {
    const ID: &'static str = PROJECT_ENTITIES;
    const SUMMARY: &'static str = "Lists the entity classes the open project's server QuakeC \
        defines, read from its spawn functions: each class's keys, defaults, spawnflags, \
        comment, model and size; with fgd, also writes them as an FGD file for other map \
        editors.";
    type Params = EntitiesParams;
    type Output = EntitiesFound;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: EntitiesParams,
    ) -> Result<Outcome<EntitiesFound, C>, WorkbenchError> {
        let classes = context.workbench().entity_classes()?;
        if let Some(path) = &params.fgd {
            std::fs::write(path, Fgd(&classes).to_string()).map_err(|error| {
                WorkbenchError::Save {
                    path: path.clone(),
                    error,
                }
            })?;
        }
        let targeting = context.workbench().targeting_keys();
        Ok(Outcome::outside_history(EntitiesFound {
            classes: classes.iter().map(class_info).collect(),
            fgd: params.fgd,
            targeting,
        }))
    }
}

/// Registers `project.entities`.
pub(crate) fn register_entities<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ProjectEntities)
}

#[cfg(test)]
mod tests;
