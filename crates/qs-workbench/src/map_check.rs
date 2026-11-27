// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.check` and `map.rename_key` (P5-04, MAP-2, MAP-7): a `.map`'s diagnostics, its format's
//! (`qs_map`, MAP-1 to MAP-7) and its entities' against the project's server QuakeC
//! (`qc_entities::map_check`, MAP-8); and, once a rename of a field is confirmed, the key renamed in
//! the open maps (MAP-8.7).

use std::path::{Path, PathBuf};

use qc_entities::{EntitySchema, MapDiagnosticKind};
use qc_project::ModuleKind;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use qs_map::MapFile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::{Change, Source};
use crate::editing::{Saved, TextStep};
use crate::entity_classes::class_info;
use crate::map_budget::BudgetKey;
use crate::{EntityCheck, EntityProblem, HasWorkbench, MapProject, Workbench, WorkbenchError};
use qs_map_edit::{ClassInfo, EntityIndex, QuickFix};

/// The id of `map.check`.
pub const MAP_CHECK: &str = "map.check";

/// The id of `map.rename_key`.
pub const MAP_RENAME_KEY: &str = "map.rename_key";

/// The parameters of `map.check`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapCheckParams {
    /// The `.map` file.
    pub path: PathBuf,
}

/// What `map.check` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapChecked {
    /// Every diagnostic: the format's in file order, then the entities'.
    pub diagnostics: Vec<MapProblem>,
}

/// One diagnostic of a map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapProblem {
    /// The 1-based line, or 0 for the whole map.
    pub line: u32,
    /// What is wrong.
    pub message: String,
    /// The key or classname to write instead, when one is close (MAP-8.1, MAP-8.2).
    pub suggestion: Option<String>,
}

/// The parameters of `map.rename_key`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameKeyParams {
    /// The key as the maps write it, the field's old name.
    pub from: String,
    /// The new key.
    pub to: String,
}

/// What `map.rename_key` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KeysRenamed {
    /// Each open map that had the key, with how many pairs were renamed; unsaved.
    pub maps: Vec<RenamedIn>,
}

/// One map a key was renamed in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RenamedIn {
    /// The map.
    pub path: PathBuf,
    /// The pairs renamed.
    pub pairs: usize,
}

/// What the map editor asks of the project (`HasMap`), answered from its server QuakeC.
impl<L: crate::Loader> MapProject for Workbench<L> {
    fn project_root(&self) -> Option<PathBuf> {
        self.root().map(Path::to_path_buf)
    }

    fn entity_class(&mut self, classname: &str) -> Option<ClassInfo> {
        let classes = self.entity_classes().ok()?;
        classes
            .iter()
            .find(|class| class.name == classname)
            .map(class_info)
    }

    fn targeting_keys(&mut self) -> Vec<String> {
        Self::targeting_keys(self)
    }

    fn entity_check(&mut self) -> Option<EntityCheck> {
        // L-77: what `map.playability` last found, while the map is the same; it needs no
        // project.
        let playable = self.playable.clone();
        // BUDGET-3.3: what `map.budget` last found, while the map, the target and the sources
        // are the same.
        let budget = self.budget.clone();
        let inputs = self.budget_inputs();
        let budget_problems =
            move |text: &[u8]| budget.problems(&BudgetKey::of(text.to_vec(), inputs));
        let Ok(schema) = self.entity_schema() else {
            return Some(Box::new(move |map| {
                let text = map.to_bytes();
                let mut problems = playable.problems(&text);
                problems.extend(budget_problems(&text));
                problems
            }));
        };
        // FIRE-5.3: what `map.logic_issues` last found, while the map and the sources are the same.
        let logic = self.logic.clone();
        let sources = self.source_key();
        Some(Box::new(move |map| {
            let text = map.to_bytes();
            let mut problems = entity_problems(&schema, map);
            problems.extend(logic.problems(&text, sources));
            problems.extend(playable.problems(&text));
            problems.extend(budget_problems(&text));
            problems
        }))
    }
}

/// The problems `schema` finds with the entities of `map` (MAP-8), each with the edit that
/// puts it right where the schema suggests one.
fn entity_problems(schema: &EntitySchema, map: &MapFile) -> Vec<EntityProblem> {
    schema
        .check(map)
        .into_iter()
        .map(|d| {
            let entity = EntityIndex::new(d.entity.get());
            let fix = match &d.kind {
                MapDiagnosticKind::UnknownKey {
                    key,
                    suggestion: Some(to),
                } => Some(QuickFix::RenameKey {
                    entity,
                    from: key.clone(),
                    to: to.clone(),
                }),
                MapDiagnosticKind::UnknownClass {
                    suggestion: Some(classname),
                    ..
                } => Some(QuickFix::SetClassname {
                    entity,
                    classname: classname.clone(),
                }),
                MapDiagnosticKind::UnknownKey {
                    suggestion: None, ..
                }
                | MapDiagnosticKind::UnknownClass {
                    suggestion: None, ..
                }
                | MapDiagnosticKind::NoClassname
                | MapDiagnosticKind::EmptyClassname
                | MapDiagnosticKind::BadValue { .. }
                | MapDiagnosticKind::NoTarget { .. }
                | MapDiagnosticKind::Untargeted { .. } => None,
            };
            EntityProblem {
                entity: Some(entity),
                line: d.line,
                message: d.kind.to_string(),
                fix,
                point: None,
            }
        })
        .collect()
}

impl<L: crate::Loader> Workbench<L> {
    /// The fields, functions and classes of the project's server modules (MAP-8); the first
    /// module's on a clash.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`] with no project open.
    pub fn entity_schema(&mut self) -> Result<EntitySchema, WorkbenchError> {
        if self.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let mut schema = EntitySchema::default();
        for module in self.host.project_modules() {
            if module.kind == ModuleKind::Ssqc {
                schema.extend(EntitySchema::of_module(module.db, module.input));
            }
        }
        Ok(schema)
    }

    /// The map at `path` as its editor holds it, else as on disk.
    fn map_file(&mut self, path: &Path) -> Result<MapFile, WorkbenchError> {
        let text = self.text(path).ok_or_else(|| WorkbenchError::NoFile {
            path: path.to_path_buf(),
        })?;
        MapFile::parse(&text).map_err(|error| WorkbenchError::Map {
            path: path.to_path_buf(),
            error,
        })
    }

    /// Every diagnostic of the map at `path` (MAP-1 to MAP-8).
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`] with no project open, [`WorkbenchError::NoFile`] for a
    /// file that cannot be read and [`WorkbenchError::Map`] for one that does not parse.
    pub fn check_map(&mut self, path: &Path) -> Result<Vec<MapProblem>, WorkbenchError> {
        let schema = self.entity_schema()?;
        let map = self.map_file(path)?;
        let mut problems: Vec<MapProblem> = map
            .check()
            .into_iter()
            .map(|d| MapProblem {
                line: d.line,
                message: d.kind.to_string(),
                suggestion: None,
            })
            .collect();
        for d in schema.check(&map) {
            let suggestion = match &d.kind {
                MapDiagnosticKind::UnknownClass { suggestion, .. }
                | MapDiagnosticKind::UnknownKey { suggestion, .. } => suggestion.clone(),
                MapDiagnosticKind::NoClassname
                | MapDiagnosticKind::EmptyClassname
                | MapDiagnosticKind::BadValue { .. }
                | MapDiagnosticKind::NoTarget { .. }
                | MapDiagnosticKind::Untargeted { .. } => None,
            };
            problems.push(MapProblem {
                line: d.line,
                message: d.kind.to_string(),
                suggestion,
            });
        }
        Ok(problems)
    }

    /// Renames key `from` to `to` in every open `.map` (MAP-8.7), as editors' texts; nothing
    /// is saved. Returns each map changed, its text before and after.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Map`] when an open map does not parse or `to` cannot be written as a
    /// key; the maps before it keep their new text.
    pub fn rename_map_key(
        &mut self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(Change, usize)>, WorkbenchError> {
        let maps: Vec<PathBuf> = self
            .tabs()
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("map"))
            })
            .cloned()
            .collect();
        let mut changed = Vec::new();
        for path in maps {
            let mut map = self.map_file(&path)?;
            let before = map.to_bytes();
            let pairs = map
                .rename_key(from.as_bytes(), to.as_bytes())
                .map_err(|error| WorkbenchError::Map {
                    path: path.clone(),
                    error,
                })?;
            if pairs == 0 {
                continue;
            }
            let after = map.to_bytes();
            self.set_text(&path, after.clone(), Source::Elsewhere)?;
            changed.push((
                Change {
                    path,
                    before,
                    after,
                },
                pairs,
            ));
        }
        Ok(changed)
    }
}

/// `map.check`.
struct MapCheck;

impl<C: HasWorkbench + 'static> Command<C> for MapCheck {
    const ID: &'static str = MAP_CHECK;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Checks a .map: what id's qbsp and ericw-tools read \
        otherwise, and its entities against the project's server QuakeC as the engine reads \
        them: unknown classnames and keys with the closest name, values their field cannot \
        read, and targets and targetnames nothing matches. It reads the file alone, so it \
        says a world that names no WAD is one as id's qbsp does; map.issues and map.find_leaks say \
        whether a texture of the open map is missing.";
    type Params = MapCheckParams;
    type Output = MapChecked;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapCheckParams,
    ) -> Result<Outcome<MapChecked, C>, WorkbenchError> {
        let diagnostics = context.workbench().check_map(&params.path)?;
        Ok(Outcome::unchanged(MapChecked { diagnostics }))
    }
}

/// `map.rename_key`.
struct RenameKey;

impl<C: HasWorkbench + 'static> Command<C> for RenameKey {
    const ID: &'static str = MAP_RENAME_KEY;
    const SUMMARY: &'static str = "Renames a key in every open .map, keeping each value and \
        its formatting: what to run once the person confirms, after symbol.rename renames a \
        field a map may name. Nothing is saved.";
    type Params = RenameKeyParams;
    type Output = KeysRenamed;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RenameKeyParams,
    ) -> Result<Outcome<KeysRenamed, C>, WorkbenchError> {
        let changed = context
            .workbench()
            .rename_map_key(&params.from, &params.to)?;
        let maps = changed
            .iter()
            .map(|(change, pairs)| RenamedIn {
                path: change.path.clone(),
                pairs: *pairs,
            })
            .collect();
        Ok(Outcome::undoable(
            KeysRenamed { maps },
            TextStep::new(
                changed.into_iter().map(|(change, _)| change).collect(),
                Saved::Unsaved,
            ),
        ))
    }
}

/// Registers `map.check` and `map.rename_key`.
pub(crate) fn register_map<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapCheck)?;
    registry.register(RenameKey)
}

#[cfg(test)]
mod tests;
