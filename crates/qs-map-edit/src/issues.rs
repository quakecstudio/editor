// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's issue browser and search (P6-19, MAP-7, MAP-28). `map.issues` lists every
//! diagnostic of the open map: its format's (`qs_map`, MAP-1 to MAP-7), a brush with no volume,
//! a missing player start, textures its WADs lack, and, with a project open, its entities'
//! against the server QuakeC (MAP-8). Each issue gives the entity or brush it is about and a
//! quick fix where one exists; `map.apply_fix` applies one as a step with undo. `map.search` finds
//! what a text or a query in TrenchBroom's query language matches (`crate::query`).

use std::collections::BTreeSet;

use qs_commands::{Command, NoParams, Outcome};
use qs_map::{Brush, DiagnosticKind, Entity, Face, Item, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{BrushIndex, EntityIndex, MapDocument, MapEdited, edit_map, origin_text};
use crate::query::{PointSizes, Query};
use crate::textures::{TextureOrigin, TextureSources};
use crate::uv::FaceRef;
use crate::{EntityProblem, HasMap, MapEditError, MapProject};

/// The id of `map.issues`.
pub const MAP_ISSUES: &str = "map.issues";
/// The id of `map.apply_fix`.
pub const MAP_FIX: &str = "map.apply_fix";
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
    /// The point it is at, when it is about a place rather than an entity or a brush, such as
    /// a place a player cannot leave (`map.playability`).
    pub point: Option<[f64; 3]>,
}

/// What `map.issues` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapIssues {
    /// Every issue: the format's in file order, then the brushes', the entities' and the
    /// map's.
    pub issues: Vec<MapIssue>,
}

/// The parameters of `map.apply_fix`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapFixParams {
    /// The fix, as `map.issues` gave it.
    pub fix: QuickFix,
}

/// The parameters of `map.search`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapSearchParams {
    /// Text to find in names, classnames, keys, values, textures, smart tags and layer and
    /// group names, ignoring case, or a query in TrenchBroom's query language, such as
    /// `classname is "light" && properties.light > 300` or `material like "sky*"`.
    pub query: String,
}

/// What `map.search` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapFound {
    /// The entities matching, in file order.
    pub entities: Vec<EntityIndex>,
    /// The brushes matching, in file order, and those of a layer matching.
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
    /// The faces matching, in file order: a query finds faces or other objects, never both.
    pub faces: Vec<FaceRef>,
}

/// Where a line of the map is: the entity and brush holding it.
#[derive(Clone, Copy)]
struct LinePlace {
    line: u32,
    entity: EntityIndex,
    brush: Option<BrushIndex>,
}

/// Every entity's, pair's, brush's and face's first line in `file`, in file order, which is
/// the order of their lines.
fn line_places(file: &MapFile) -> Vec<LinePlace> {
    let mut places = Vec::new();
    for (e, entity) in file.entities.iter().enumerate() {
        let entity_index = EntityIndex::new(e);
        let mut place = |line: u32, brush: Option<BrushIndex>| {
            places.push(LinePlace {
                line,
                entity: entity_index,
                brush,
            });
        };
        place(entity.line(), None);
        let mut brushes = 0;
        for item in &entity.items {
            match item {
                Item::Pair(pair) => place(pair.line(), None),
                Item::Brush(brush) => {
                    let index = Some(BrushIndex::new(brushes));
                    place(brush.line(), index);
                    for face in &brush.faces {
                        place(face.line(), index);
                    }
                    brushes = brushes.saturating_add(1);
                }
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
    let after = places.partition_point(|place| place.line <= line);
    after
        .checked_sub(1)
        .and_then(|last| places.get(last))
        .map_or((None, None), |place| (Some(place.entity), place.brush))
}

/// Whether `brush` has no volume: fewer than [`SOLID_FACES`] faces with a polygon.
fn is_empty(brush: &Brush) -> bool {
    brush.polygons().len() < SOLID_FACES
}

/// The least and greatest of `corners`, if there are any.
pub(crate) fn bounds(corners: impl IntoIterator<Item = [f64; 3]>) -> Option<[[f64; 3]; 2]> {
    let mut found: Option<[[f64; 3]; 2]> = None;
    for corner in corners {
        let [lo, hi] = found.unwrap_or([corner, corner]);
        found = Some([
            [0, 1, 2].map(|i| {
                lo.get(i)
                    .copied()
                    .unwrap_or_default()
                    .min(corner.get(i).copied().unwrap_or_default())
            }),
            [0, 1, 2].map(|i| {
                hi.get(i)
                    .copied()
                    .unwrap_or_default()
                    .max(corner.get(i).copied().unwrap_or_default())
            }),
        ]);
    }
    found
}

/// The least and greatest corner of `brush`, if it has corners.
pub(crate) fn bounds_of(brush: &Brush) -> Option<[[f64; 3]; 2]> {
    bounds(brush.vertices())
}

/// The box around `brush` when it is a bounded solid (MAP-3.4); none for one that is not
/// closed, whose corners are where its open faces were cut off, far outside the map.
pub(crate) fn solid_bounds(brush: &Brush) -> Option<[[f64; 3]; 2]> {
    brush.is_valid().then(|| bounds_of(brush)).flatten()
}

/// Whether `point` is inside or on `brush`'s solid; a brush with no volume holds nothing.
fn holds(brush: &Brush, point: [f64; 3]) -> bool {
    !is_empty(brush)
        && brush.faces.iter().filter_map(Face::plane).all(|plane| {
            let [x, y, z] = plane.normal;
            point[0] * x + point[1] * y + point[2] * z <= plane.dist
        })
}

/// Where a player start can go in `file`: above the floor at the middle of the world's
/// brushes, when that point is in no brush; none otherwise.
fn start_point(file: &MapFile) -> Option<[f64; 3]> {
    let world = file.world()?;
    let boxes: Vec<[[f64; 3]; 2]> = world.brushes().filter_map(bounds_of).collect();
    let [lo, hi] = bounds(boxes.iter().flatten().copied())?;
    let (x, y) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    // The highest top of a brush under the middle that is below the middle's height.
    let middle = (lo[2] + hi[2]) / 2.0;
    let floor = boxes
        .iter()
        .filter(|[b, t]| b[0] <= x && x <= t[0] && b[1] <= y && y <= t[1] && t[2] <= middle)
        .map(|[_, top]| top[2])
        .fold(None, |best: Option<f64>, z| {
            Some(best.map_or(z, |b| b.max(z)))
        })?;
    let point = [x, y, floor + START_HEIGHT];
    (!world.brushes().any(|brush| holds(brush, point))).then_some(point)
}

/// Whether every texture the map's faces name was found somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Textures {
    /// Each is in a WAD, the BSP the map came from or a level of the game.
    AllFound,
    /// One or more is nowhere.
    SomeMissing,
}

impl MapDocument {
    /// The issues of the map found without the project: its format's, empty brushes and a
    /// missing player start. That the world names no WAD is an issue only when `textures` is
    /// [`Textures::SomeMissing`]: a map textured from the game's own levels needs none
    /// (P6-43), and the issue has nothing to fix.
    fn own_issues(&self, places: &[LinePlace], textures: Textures) -> Vec<MapIssue> {
        let file = self.file();
        let mut issues: Vec<MapIssue> = file
            .check()
            .into_iter()
            .filter(|d| d.kind != DiagnosticKind::NoWad || textures == Textures::SomeMissing)
            .map(|d| {
                let (entity, brush) = place_of(places, d.line);
                MapIssue {
                    line: d.line,
                    message: d.kind.to_string(),
                    entity,
                    brush,
                    fix: None,
                    point: None,
                }
            })
            .collect();
        for (e, entity) in file.entities.iter().enumerate() {
            for (b, brush) in entity.brushes().enumerate() {
                if is_empty(brush) {
                    let (entity, brush_index) = (EntityIndex::new(e), BrushIndex::new(b));
                    issues.push(MapIssue {
                        line: brush.line(),
                        message: NO_VOLUME.to_owned(),
                        entity: Some(entity),
                        brush: Some(brush_index),
                        fix: Some(QuickFix::RemoveBrush {
                            entity,
                            brush: brush_index,
                        }),
                        point: None,
                    });
                }
            }
        }
        if let Some((e, line)) = past_reach(file) {
            issues.push(MapIssue {
                line,
                message: PAST_REACH.to_owned(),
                entity: Some(EntityIndex::new(e)),
                brush: None,
                fix: None,
                point: None,
            });
        }
        let has_start = file
            .entities
            .iter()
            .any(|entity| entity.classname() == Some(PLAYER_START.as_bytes()));
        if !has_start && file.world().is_some_and(Entity::has_brushes) {
            issues.push(MapIssue {
                line: 0,
                message: NO_START.to_owned(),
                entity: None,
                brush: None,
                fix: start_point(file).map(|origin| QuickFix::AddPlayerStart { origin }),
                point: None,
            });
        }
        issues
    }

    /// Applies `fix`.
    fn apply(&mut self, fix: &QuickFix) -> Result<Option<EntityIndex>, MapEditError> {
        match fix {
            QuickFix::RemoveBrush { entity, brush } => {
                if !is_empty(&self.brush(*entity, *brush)?) {
                    return Err(MapEditError::NotEmpty {
                        entity: *entity,
                        brush: *brush,
                    });
                }
                self.replace_brush(*entity, *brush, Vec::new())?;
                Ok(Some(*entity))
            }
            QuickFix::RenameKey { entity, from, to } => {
                let renamed = self
                    .entity_mut(*entity)?
                    .rename_key(from.as_bytes(), to.as_bytes());
                match renamed {
                    Ok(0) => Err(MapEditError::NoKey {
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
}

/// The message of a brush with no volume.
const NO_VOLUME: &str = "a brush with no volume, which both compilers drop";

/// How far from the map's middle a position is sent whole in 16 bits: a coordinate is the
/// short `(int) (f * 8)` (NET-1.1), which holds -4096 and less than 4096.
const COORD_REACH: f64 = 4096.0;

/// The message of a map with an entity whose origin 16 bits do not hold.
const PAST_REACH: &str = "an entity is 4096 units or more from the map's middle: the game's \
    own protocol and FitzQuake's send a position in 16 bits (NET-1.1), so what is there, and a \
    player or a monster that goes there, is drawn somewhere else; it plays only in an engine \
    whose protocol sends wider coordinates (NET-6.2)";

/// The first point entity of `file` whose origin a 16-bit coordinate does not hold, by its
/// place and its line. An origin is what the game sends; the world's brushes are not sent,
/// and a brush entity is sent by its origin, which is the map's middle until it moves.
fn past_reach(file: &MapFile) -> Option<(usize, u32)> {
    let held = |at: &f64| (-COORD_REACH..COORD_REACH).contains(at);
    file.entities
        .iter()
        .enumerate()
        .find_map(|(place, entity)| {
            let far = !entity.has_brushes() && !entity.origin().iter().all(held);
            far.then(|| (place, entity.line()))
        })
}

/// The message of a map with no player start.
const NO_START: &str = "no info_player_start: the game cannot put a player in the map";

/// The issues that are the project's `problems` with a map's entities (MAP-8), each with its
/// fix.
fn entity_issues(places: &[LinePlace], problems: Vec<EntityProblem>) -> Vec<MapIssue> {
    problems
        .into_iter()
        .map(|problem| {
            let (_, brush) = place_of(places, problem.line);
            MapIssue {
                line: problem.line,
                message: problem.message,
                entity: problem.entity,
                brush,
                fix: problem.fix,
                point: problem.point,
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
                point: None,
            })
            .collect()
    }
}

/// `map.issues`.
pub(crate) struct MapIssuesCommand;

impl<C: HasMap + 'static> Command<C> for MapIssuesCommand {
    const ID: &'static str = MAP_ISSUES;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Lists every issue of the open map, each with the entity or \
        brush it is about and a quick fix where one exists: its format's, brushes with no \
        volume, a missing player start, missing textures and, with a project open, its \
        entities' against the server QuakeC.";
    type Params = NoParams;
    type Output = MapIssues;
    type Error = MapEditError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapIssues, C>, MapEditError> {
        let check = context.map_project().entity_check();
        let game = context.game_folders();
        let root = context.map_project().project_root();
        let document = context.map_store().map().ok_or(MapEditError::NoMap)?;
        let places = line_places(document.file());
        let sources = TextureSources::load(
            document,
            root.as_deref(),
            game.base.as_deref(),
            game.chosen.as_deref(),
        );
        let missing = document.texture_issues(&sources);
        let textures = if missing.is_empty() {
            Textures::AllFound
        } else {
            Textures::SomeMissing
        };
        let mut issues = document.own_issues(&places, textures);
        if let Some(check) = &check {
            issues.extend(entity_issues(&places, check(document.file())));
        }
        issues.extend(missing);
        Ok(Outcome::outside_history(MapIssues { issues }))
    }
}

/// `map.apply_fix`.
pub(crate) struct MapFix;

impl<C: HasMap + 'static> Command<C> for MapFix {
    const ID: &'static str = MAP_FIX;
    const SUMMARY: &'static str = "Applies a quick fix `map.issues` gave, as one step with undo.";
    type Params = MapFixParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: MapFixParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| document.apply(&params.fix))?;
        Ok(Outcome::undoable(MapEdited { entity }, step))
    }
}

/// `map.search`.
pub(crate) struct MapSearch;

impl<C: HasMap + 'static> Command<C> for MapSearch {
    const ID: &'static str = MAP_SEARCH;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Finds the open map's objects as TrenchBroom's search does: \
        text found in names, classnames, keys, values, textures, smart tags and layer and group \
        names, ignoring case, or a query in its query language, which finds entities, groups, \
        brushes, the contents of layers, or faces.";
    type Params = MapSearchParams;
    type Output = MapFound;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: MapSearchParams,
    ) -> Result<Outcome<MapFound, C>, MapEditError> {
        let classnames: BTreeSet<String> = context
            .map_store()
            .map_mut()?
            .file()
            .entities
            .iter()
            .filter_map(|entity| entity.classname())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect();
        let project = context.map_project();
        let sizes = classnames
            .into_iter()
            .filter_map(|name| {
                let [min, max] = project.entity_class(&name)?.size?;
                Some((name, [min.map(f64::from), max.map(f64::from)]))
            })
            .collect();
        let document = context.map_store().map_mut()?;
        let query = Query::parse(&params.query);
        Ok(Outcome::outside_history(
            document.query(&query, &PointSizes::new(sizes)),
        ))
    }
}

#[cfg(test)]
mod tests;
