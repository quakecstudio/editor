// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's issue browser and search (P6-19, MAP-7, MAP-28). `map.issues` lists every
//! diagnostic of the open map: its format's (`qs_map`, MAP-1 to MAP-7), a brush with no volume,
//! a missing player start, textures its WADs lack, and, with a project open, its entities'
//! against the server QuakeC (MAP-8). Each issue gives the entity or brush it is about and a
//! quick fix where one exists; `map.fix` applies one as a step with undo. `map.search` finds
//! entities and brushes by class, key, value, texture and bounds.

use std::path::{Path, PathBuf};

use qc_sema::MapDiagnosticKind;
use qs_commands::{Command, NoParams, Outcome};
use qs_map::{Brush, Entity, Face, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{BrushIndex, EntityIndex, MapDocument, MapEdited, edit_map, origin_text};
use crate::map_textures::{TextureOrigin, TextureSources};
use crate::preview::HasPreview;
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.issues`.
pub const MAP_ISSUES: &str = "map.issues";
/// The id of `map.fix`.
pub const MAP_FIX: &str = "map.fix";
/// The id of `map.search`.
pub const MAP_SEARCH: &str = "map.search";

/// The classname of the player's start, which id's `PutClientInServer` looks for.
const PLAYER_START: &str = "info_player_start";
/// The fewest faces with a polygon a brush with volume has: a tetrahedron's.
const SOLID_FACES: usize = 4;
/// How far above the floor a player start is placed, in map units: id's player box reaches
/// 24 below its origin (`VEC_HULL_MIN`), and 8 more keeps it off the floor.
const START_HEIGHT: f64 = 32.0;

/// A quick fix of an issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuickFix {
    /// Delete a brush with no volume, which the compilers drop.
    RemoveBrush {
        /// The entity holding it.
        entity: EntityIndex,
        /// The brush.
        brush: BrushIndex,
    },
    /// Rename a misspelled key to the field it is closest to.
    RenameKey {
        /// The entity.
        entity: EntityIndex,
        /// The key as written.
        from: String,
        /// The key to write instead.
        to: String,
    },
    /// Set a classname to the class it is closest to.
    SetClassname {
        /// The entity.
        entity: EntityIndex,
        /// The classname to write.
        classname: String,
    },
    /// Place an `info_player_start` at a point inside the map.
    AddPlayerStart {
        /// Where.
        origin: [f64; 3],
    },
}

/// One issue of the open map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapIssue {
    /// The 1-based line, or 0 for the whole map or a map not read from a file.
    pub line: u32,
    /// What is wrong.
    pub message: String,
    /// The entity it is about, if one.
    pub entity: Option<EntityIndex>,
    /// The brush it is about, if one.
    pub brush: Option<BrushIndex>,
    /// What fixes it, where something does.
    pub fix: Option<QuickFix>,
}

/// What `map.issues` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapIssues {
    /// Every issue: the format's in file order, then the brushes', the entities' and the
    /// map's.
    pub issues: Vec<MapIssue>,
}

/// The parameters of `map.fix`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapFixParams {
    /// The fix, as `map.issues` gave it.
    pub fix: QuickFix,
}

/// The parameters of `map.search`: what an entity or brush must match, every part given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapSearchParams {
    /// Text the classname holds, in any case.
    #[serde(default)]
    pub classname: Option<String>,
    /// Text a key holds, in any case.
    #[serde(default)]
    pub key: Option<String>,
    /// Text a value holds, in any case: of a key matching `key` when that is given.
    #[serde(default)]
    pub value: Option<String>,
    /// Text a brush's face's texture name holds, in any case.
    #[serde(default)]
    pub texture: Option<String>,
    /// A box, least corner then greatest: a point entity's origin lies in it, a brush's box
    /// meets it.
    #[serde(default)]
    pub within: Option<[[f64; 3]; 2]>,
}

/// What `map.search` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapFound {
    /// The entities matching, in file order.
    pub entities: Vec<EntityIndex>,
    /// The brushes matching, in file order: those of a matching entity, when a texture or a
    /// box is given.
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
}

/// Where a line of the map is: the entity and brush holding it.
#[derive(Clone, Copy)]
struct LinePlace {
    line: u32,
    entity: EntityIndex,
    brush: Option<BrushIndex>,
}

/// Every entity's, brush's and face's first line in `file`, in file order.
fn line_places(file: &MapFile) -> Vec<LinePlace> {
    let mut places = Vec::new();
    for (e, entity) in file.entities.iter().enumerate() {
        let entity_index = EntityIndex::new(e);
        places.push(LinePlace {
            line: entity.line(),
            entity: entity_index,
            brush: None,
        });
        for (b, brush) in entity.brushes().enumerate() {
            let brush_index = Some(BrushIndex::new(b));
            for line in core::iter::once(brush.line()).chain(brush.faces.iter().map(|f| f.line())) {
                places.push(LinePlace {
                    line,
                    entity: entity_index,
                    brush: brush_index,
                });
            }
        }
    }
    places.retain(|place| place.line > 0);
    places
}

/// The entity and brush holding `line`: the last place starting at or before it.
fn place_of(places: &[LinePlace], line: u32) -> (Option<EntityIndex>, Option<BrushIndex>) {
    if line == 0 {
        return (None, None);
    }
    places
        .iter()
        .take_while(|place| place.line <= line)
        .last()
        .map_or((None, None), |place| (Some(place.entity), place.brush))
}

/// Whether `brush` has no volume: fewer than [`SOLID_FACES`] faces with a polygon.
fn is_empty(brush: &Brush) -> bool {
    brush.polygons().len() < SOLID_FACES
}

/// The least and greatest corner of `brush`, if it has corners.
fn bounds_of(brush: &Brush) -> Option<[[f64; 3]; 2]> {
    let mut found: Option<[[f64; 3]; 2]> = None;
    for corner in brush.vertices() {
        let [lo, hi] = found.unwrap_or([corner, corner]);
        found = Some([
            [
                lo[0].min(corner[0]),
                lo[1].min(corner[1]),
                lo[2].min(corner[2]),
            ],
            [
                hi[0].max(corner[0]),
                hi[1].max(corner[1]),
                hi[2].max(corner[2]),
            ],
        ]);
    }
    found
}

/// Whether `point` is inside or on `brush`'s solid.
fn holds(brush: &Brush, point: [f64; 3]) -> bool {
    brush.faces.iter().filter_map(Face::plane).all(|plane| {
        let [x, y, z] = plane.normal;
        point[0] * x + point[1] * y + point[2] * z <= plane.dist
    })
}

/// Where a player start can go in `file`: above the floor at the middle of the world's
/// brushes, when that point is in no brush; none otherwise.
fn start_point(file: &MapFile) -> Option<[f64; 3]> {
    let world = file.world()?;
    let mut all: Option<[[f64; 3]; 2]> = None;
    for [lo, hi] in world.brushes().filter_map(bounds_of) {
        let [a, b] = all.unwrap_or([lo, hi]);
        all = Some([
            [a[0].min(lo[0]), a[1].min(lo[1]), a[2].min(lo[2])],
            [b[0].max(hi[0]), b[1].max(hi[1]), b[2].max(hi[2])],
        ]);
    }
    let [lo, hi] = all?;
    let (x, y) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    // The highest top of a brush under the middle that is below the middle's height.
    let middle = (lo[2] + hi[2]) / 2.0;
    let floor = world
        .brushes()
        .filter_map(bounds_of)
        .filter(|[b, t]| b[0] <= x && x <= t[0] && b[1] <= y && y <= t[1] && t[2] <= middle)
        .map(|[_, top]| top[2])
        .fold(None, |best: Option<f64>, z| {
            Some(best.map_or(z, |b| b.max(z)))
        })?;
    let point = [x, y, floor + START_HEIGHT];
    (!world.brushes().any(|brush| holds(brush, point))).then_some(point)
}

impl MapDocument {
    /// The issues of the map found without the project: its format's, empty brushes and a
    /// missing player start.
    fn own_issues(&self) -> Vec<MapIssue> {
        let file = self.file();
        let places = line_places(file);
        let mut issues: Vec<MapIssue> = file
            .check()
            .into_iter()
            .map(|d| {
                let (entity, brush) = place_of(&places, d.line);
                MapIssue {
                    line: d.line,
                    message: d.kind.to_string(),
                    entity,
                    brush,
                    fix: None,
                }
            })
            .collect();
        for (e, entity) in file.entities.iter().enumerate() {
            for (b, brush) in entity.brushes().enumerate() {
                if is_empty(brush) {
                    let (entity, brush_index) = (EntityIndex::new(e), BrushIndex::new(b));
                    issues.push(MapIssue {
                        line: brush.line(),
                        message: no_volume().to_owned(),
                        entity: Some(entity),
                        brush: Some(brush_index),
                        fix: Some(QuickFix::RemoveBrush {
                            entity,
                            brush: brush_index,
                        }),
                    });
                }
            }
        }
        let has_start = file
            .entities
            .iter()
            .any(|entity| entity.classname() == Some(PLAYER_START.as_bytes()));
        if !has_start && file.world().is_some_and(Entity::has_brushes) {
            issues.push(MapIssue {
                line: 0,
                message: no_start().to_owned(),
                entity: None,
                brush: None,
                fix: start_point(file).map(|origin| QuickFix::AddPlayerStart { origin }),
            });
        }
        issues
    }

    /// Applies `fix`.
    fn apply(&mut self, fix: &QuickFix) -> Result<Option<EntityIndex>, WorkbenchError> {
        match fix {
            QuickFix::RemoveBrush { entity, brush } => {
                self.brush(*entity, *brush)?;
                self.replace_brush(*entity, *brush, Vec::new())?;
                Ok(Some(*entity))
            }
            QuickFix::RenameKey { entity, from, to } => {
                let renamed = self
                    .entity_mut(*entity)?
                    .rename_key(from.as_bytes(), to.as_bytes());
                match renamed {
                    Ok(0) => Err(WorkbenchError::NoKey {
                        entity: *entity,
                        key: from.clone(),
                    }),
                    Ok(_) => Ok(Some(*entity)),
                    Err(error) => Err(self.map_error(error)),
                }
            }
            QuickFix::SetClassname { entity, classname } => {
                let set = self
                    .entity_mut(*entity)?
                    .set(b"classname", classname.as_bytes());
                set.map_err(|error| self.map_error(error))?;
                Ok(Some(*entity))
            }
            QuickFix::AddPlayerStart { origin } => {
                let mut entity = Entity::new();
                for (key, value) in [
                    ("classname", PLAYER_START.to_owned()),
                    ("origin", origin_text(*origin)),
                ] {
                    entity
                        .set(key.as_bytes(), value.as_bytes())
                        .map_err(|error| self.map_error(error))?;
                }
                self.file.push_entity(entity);
                Ok(Some(EntityIndex::new(
                    self.file.entities.len().saturating_sub(1),
                )))
            }
        }
    }

    /// The entities and brushes `search` matches.
    fn search(&self, search: &MapSearchParams) -> MapFound {
        let holds_text = |text: &[u8], part: Option<&String>| {
            part.is_none_or(|part| {
                String::from_utf8_lossy(text)
                    .to_lowercase()
                    .contains(&part.to_lowercase())
            })
        };
        let mut found = MapFound::default();
        for (e, entity) in self.file().entities.iter().enumerate() {
            let class = holds_text(
                entity.classname().unwrap_or_default(),
                search.classname.as_ref(),
            );
            let keyed = (search.key.is_none() && search.value.is_none())
                || entity.pairs().any(|pair| {
                    holds_text(pair.key(), search.key.as_ref())
                        && holds_text(pair.value(), search.value.as_ref())
                });
            if !class || !keyed {
                continue;
            }
            let brushes: Vec<BrushIndex> = entity
                .brushes()
                .enumerate()
                .filter(|(_, brush)| {
                    let textured = search.texture.is_none()
                        || brush
                            .faces
                            .iter()
                            .any(|f| holds_text(f.texture(), search.texture.as_ref()));
                    let inside = search.within.is_none_or(|within| {
                        bounds_of(brush).is_some_and(|bounds| meets(bounds, within))
                    });
                    textured && inside
                })
                .map(|(b, _)| BrushIndex::new(b))
                .collect();
            let index = EntityIndex::new(e);
            let matches = if entity.has_brushes() {
                (search.texture.is_none() && search.within.is_none()) || !brushes.is_empty()
            } else {
                search.texture.is_none()
                    && search
                        .within
                        .is_none_or(|within| meets([entity.origin(), entity.origin()], within))
            };
            if matches {
                found.entities.push(index);
            }
            if search.texture.is_some() || search.within.is_some() {
                found
                    .brushes
                    .extend(brushes.into_iter().map(|b| (index, b)));
            }
        }
        found
    }
}

/// Whether two boxes, each least corner then greatest, meet.
fn meets([a_lo, a_hi]: [[f64; 3]; 2], [b_lo, b_hi]: [[f64; 3]; 2]) -> bool {
    let below = a_lo.iter().zip(b_hi).all(|(a, b)| *a <= b);
    let above = b_lo.iter().zip(a_hi).all(|(b, a)| *b <= a);
    below && above
}

/// The message of a brush with no volume.
const fn no_volume() -> &'static str {
    "a brush with no volume, which both compilers drop"
}

/// The message of a map with no player start.
const fn no_start() -> &'static str {
    "no info_player_start: the game cannot put a player in the map"
}

/// The issues of the project's entity check of `file` (MAP-8), each with its fix.
fn entity_issues(file: &MapFile, schema: &qc_sema::EntitySchema) -> Vec<MapIssue> {
    let places = line_places(file);
    schema
        .check(file)
        .into_iter()
        .map(|d| {
            let entity = EntityIndex::new(d.entity);
            let (_, brush) = place_of(&places, d.line);
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
                | MapDiagnosticKind::BadValue { .. }
                | MapDiagnosticKind::NoTarget { .. }
                | MapDiagnosticKind::Untargeted { .. } => None,
            };
            MapIssue {
                line: d.line,
                message: d.kind.to_string(),
                entity: Some(entity),
                brush,
                fix,
            }
        })
        .collect()
}

impl MapDocument {
    /// The issues of textures the map's faces use that no WAD of `sources` holds.
    fn texture_issues(&self, sources: &TextureSources) -> Vec<MapIssue> {
        sources
            .listing(self)
            .textures
            .into_iter()
            .filter(|texture| texture.origin == TextureOrigin::Missing)
            .map(|texture| MapIssue {
                line: 0,
                message: format!("no WAD of the map holds the texture {}", texture.name),
                entity: None,
                brush: None,
                fix: None,
            })
            .collect()
    }
}

/// `map.issues`.
pub(crate) struct MapIssuesCommand;

impl<C: HasPreview + 'static> Command<C> for MapIssuesCommand {
    const ID: &'static str = MAP_ISSUES;
    const SUMMARY: &'static str = "Lists every issue of the open map, each with the entity or \
        brush it is about and a quick fix where one exists: its format's, brushes with no \
        volume, a missing player start, missing textures and, with a project open, its \
        entities' against the server QuakeC.";
    type Params = NoParams;
    type Output = MapIssues;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapIssues, C>, WorkbenchError> {
        let schema = context.workbench().entity_schema().ok();
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let chosen = preview.game_folder().map(Path::to_path_buf);
        let root: Option<PathBuf> = workbench.root().map(Path::to_path_buf);
        let document = workbench.map().ok_or(WorkbenchError::NoMap)?;
        let mut issues = document.own_issues();
        if let Some(schema) = &schema {
            issues.extend(entity_issues(document.file(), schema));
        }
        let sources = TextureSources::load(
            document,
            root.as_deref(),
            game.as_deref(),
            chosen.as_deref(),
        );
        issues.extend(document.texture_issues(&sources));
        Ok(Outcome::outside_history(MapIssues { issues }))
    }
}

/// `map.fix`.
pub(crate) struct MapFix;

impl<C: HasWorkbench + 'static> Command<C> for MapFix {
    const ID: &'static str = MAP_FIX;
    const SUMMARY: &'static str = "Applies a quick fix `map.issues` gave, as one step with undo.";
    type Params = MapFixParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapFixParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let (entity, step) = edit_map(context, |document| document.apply(&params.fix))?;
        Ok(Outcome::undoable(MapEdited { entity }, step))
    }
}

/// `map.search`.
pub(crate) struct MapSearch;

impl<C: HasWorkbench + 'static> Command<C> for MapSearch {
    const ID: &'static str = MAP_SEARCH;
    const SUMMARY: &'static str = "Finds the open map's entities and brushes by classname, key, \
        value, texture and a box, every part given; text matches in any case.";
    type Params = MapSearchParams;
    type Output = MapFound;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapSearchParams,
    ) -> Result<Outcome<MapFound, C>, WorkbenchError> {
        let document = context.workbench().map_mut()?;
        Ok(Outcome::outside_history(document.search(&params)))
    }
}

#[cfg(test)]
mod tests;
