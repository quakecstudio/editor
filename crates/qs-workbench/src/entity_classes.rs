// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.entities`: the entity classes the open project's server QuakeC defines, read from
//! its spawn functions (`qc_sema::entities`, P5-03), and written as an FGD file for other map
//! editors when asked.

use std::path::PathBuf;

use qc_project::ModuleKind;
use qc_sema::{EntityClass, Fgd, Found, KeyType, Shape};
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasWorkbench, Workbench, WorkbenchError};

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
}

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

impl From<&EntityClass> for ClassInfo {
    fn from(class: &EntityClass) -> Self {
        Self {
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
                })
                .collect(),
            spawnflags: class
                .spawnflags
                .iter()
                .map(|flag| FlagInfo {
                    value: flag.value,
                    name: flag.name.clone(),
                })
                .collect(),
        }
    }
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
            for class in module.db.entity_classes(module.input) {
                if !classes.iter().any(|known| known.name == class.name) {
                    classes.push(class);
                }
            }
        }
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(classes)
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
        Ok(Outcome::outside_history(EntitiesFound {
            classes: classes.iter().map(ClassInfo::from).collect(),
            fgd: params.fgd,
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
