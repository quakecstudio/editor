// SPDX-License-Identifier: GPL-3.0-or-later

//! Map edits staged for review (P6-21, MAP-29): the agent's changes to the open map wait as
//! hunks to accept or reject one by one, as its code edits do (P4-04). The map is edited at
//! once, so a compile and the game see the change; what is staged is the map as it was when
//! review began. A hunk is a run of changed parts: an entity's keys, or one of its brushes, so
//! brushes added to the world are hunks of their own and not one change to the world. A
//! rejected hunk puts back the parts it replaced, as an edit undo takes back; an accepted one
//! is kept. Review ends once nothing differs from where it began, by decisions, edits or undo.

use std::ops::Range;

use qs_commands::{Command, NoParams, Outcome};
use qs_diff::{Hunk, hunks, script};
use qs_map::{Brush, Entity, Item, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, edit_map};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.stage`.
pub const MAP_STAGE: &str = "map.stage";
/// The id of `map.unstage`.
pub const MAP_UNSTAGE: &str = "map.unstage";
/// The id of `map.staged`.
pub const MAP_STAGED: &str = "map.staged";
/// The id of `map.accept`.
pub const MAP_ACCEPT: &str = "map.accept";
/// The id of `map.reject`.
pub const MAP_REJECT: &str = "map.reject";

/// One part of a map, as review compares them: an entity's keys, then each of its brushes.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    /// An entity with its keys and no brushes; the entity's own value for its text.
    Keys(Entity),
    /// One brush of the entity before it.
    Brush(Brush),
}

/// Which kind of part a count is of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    /// Entities' keys.
    Keys,
    /// Brushes.
    Brushes,
}

impl Part {
    /// Its kind.
    const fn kind(&self) -> PartKind {
        match self {
            Self::Keys(_) => PartKind::Keys,
            Self::Brush(_) => PartKind::Brushes,
        }
    }
}

/// The parts of `file`, in order.
fn parts(file: &MapFile) -> Vec<Part> {
    let mut out = Vec::new();
    for entity in &file.entities {
        let mut keys = entity.clone();
        keys.items.retain(|item| matches!(item, Item::Pair(_)));
        out.push(Part::Keys(keys));
        out.extend(entity.brushes().cloned().map(Part::Brush));
    }
    out
}

/// The entity each of `parts` is in, by its place in their map.
fn owners(parts: &[Part]) -> Vec<usize> {
    let mut out = Vec::with_capacity(parts.len());
    let mut place = 0_usize;
    for (i, part) in parts.iter().enumerate() {
        if part.kind() == PartKind::Keys && i > 0 {
            place = place.saturating_add(1);
        }
        out.push(place);
    }
    out
}

/// The entities of `parts`, each entity its keys and the brushes after it. An entity whose keys
/// and brushes are an entity of `known`'s, the first file first, is that entity, so the text of
/// what no hunk touched is written as it was read.
fn rebuild(parts: &[Part], known: &[&MapFile]) -> Vec<Entity> {
    let mut entities: Vec<Entity> = Vec::new();
    for part in parts {
        match part {
            Part::Keys(keys) => entities.push(keys.clone()),
            Part::Brush(brush) => {
                if let Some(entity) = entities.last_mut() {
                    entity.items.push(Item::Brush(brush.clone()));
                }
            }
        }
    }
    let same = |a: &Entity, b: &Entity| a.pairs().eq(b.pairs()) && a.brushes().eq(b.brushes());
    for entity in &mut entities {
        let original = known
            .iter()
            .flat_map(|file| file.entities.iter())
            .find(|candidate| same(candidate, entity));
        if let Some(original) = original {
            entity.clone_from(original);
        }
    }
    entities
}

/// A hunk's number in the list `map.staged` gives, from 0.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct MapHunkNumber(usize);

impl MapHunkNumber {
    /// The hunk at `index` in the list.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the list.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for MapHunkNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which map an entity a hunk names is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HunkSide {
    /// The map now: the hunk adds or changes parts of it.
    Now,
    /// The map as review began: the hunk only removed its parts.
    Before,
}

/// An entity a hunk is in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HunkEntity {
    /// Its place in its map.
    pub entity: EntityIndex,
    /// Its classname, empty for none.
    pub classname: String,
    /// Which map that is.
    pub side: HunkSide,
}

/// What a hunk changes, as the person reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapHunk {
    /// Its number in the list, which `map.accept` and `map.reject` take.
    pub index: MapHunkNumber,
    /// The entities it is in.
    pub entities: Vec<HunkEntity>,
    /// How many entities' keys it adds, removes or changes.
    pub keys: usize,
    /// How many brushes the map has in its place now.
    pub brushes_added: usize,
    /// How many brushes it replaced.
    pub brushes_removed: usize,
}

/// The review of the open map: the map as review began and the hunks from it to the map now.
struct Review<'a> {
    before: Vec<Part>,
    after: Vec<Part>,
    hunks: Vec<Hunk>,
    staged: &'a MapFile,
    file: &'a MapFile,
}

impl<'a> Review<'a> {
    /// The review of `document`, when one is open.
    fn of(document: &'a MapDocument) -> Option<Self> {
        let staged = document.staged.as_ref()?;
        let before = parts(staged);
        let after = parts(&document.file);
        let hunks = hunks(&script(&before, &after));
        Some(Self {
            before,
            after,
            hunks,
            staged,
            file: &document.file,
        })
    }

    /// The hunks, as the person reads them.
    fn listed(&self) -> Vec<MapHunk> {
        let owners_now = owners(&self.after);
        let owners_before = owners(&self.before);
        let entity = |file: &MapFile, place: usize, side: HunkSide| HunkEntity {
            entity: EntityIndex::new(place),
            classname: file
                .entities
                .get(place)
                .and_then(Entity::classname)
                .map_or_else(String::new, |name| {
                    String::from_utf8_lossy(name).into_owned()
                }),
            side,
        };
        let count = |parts: &[Part], range: &Range<usize>, kind: PartKind| {
            parts
                .get(range.clone())
                .unwrap_or_default()
                .iter()
                .filter(|part| part.kind() == kind)
                .count()
        };
        self.hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| {
                // A hunk that adds or changes parts is in the entities of the map now; one that
                // only removes them, in the entities they were in.
                let (owners, file, side, range) = if hunk.new.is_empty() {
                    (&owners_before, self.staged, HunkSide::Before, &hunk.old)
                } else {
                    (&owners_now, self.file, HunkSide::Now, &hunk.new)
                };
                let mut places = owners.get(range.clone()).unwrap_or_default().to_vec();
                places.dedup();
                MapHunk {
                    index: MapHunkNumber::new(index),
                    entities: places
                        .into_iter()
                        .map(|place| entity(file, place, side))
                        .collect(),
                    keys: count(&self.before, &hunk.old, PartKind::Keys).max(count(
                        &self.after,
                        &hunk.new,
                        PartKind::Keys,
                    )),
                    brushes_added: count(&self.after, &hunk.new, PartKind::Brushes),
                    brushes_removed: count(&self.before, &hunk.old, PartKind::Brushes),
                }
            })
            .collect()
    }

    /// The parts with the hunks numbered in `take_new` from the map now and the rest as review
    /// began.
    fn resolve(&self, take_new: &[usize]) -> Vec<Part> {
        let mut out = Vec::new();
        let mut old_at = 0_usize;
        for (index, hunk) in self.hunks.iter().enumerate() {
            out.extend(
                self.before
                    .get(old_at..hunk.old.start)
                    .unwrap_or_default()
                    .iter()
                    .cloned(),
            );
            let (side, range) = if take_new.contains(&index) {
                (&self.after, &hunk.new)
            } else {
                (&self.before, &hunk.old)
            };
            out.extend(side.get(range.clone()).unwrap_or_default().iter().cloned());
            old_at = hunk.old.end;
        }
        out.extend(
            self.before
                .get(old_at..)
                .unwrap_or_default()
                .iter()
                .cloned(),
        );
        out
    }

    /// Every hunk's number.
    fn all(&self) -> Vec<usize> {
        (0..self.hunks.len()).collect()
    }

    /// The places of `asked`, every hunk when empty, after checking each is a hunk.
    fn chosen(&self, asked: &[MapHunkNumber]) -> Result<Vec<usize>, WorkbenchError> {
        if asked.is_empty() {
            return Ok(self.all());
        }
        match asked.iter().find(|number| number.get() >= self.hunks.len()) {
            Some(&hunk) => Err(WorkbenchError::NoMapHunk { hunk }),
            None => Ok(asked.iter().map(|number| number.get()).collect()),
        }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The open map's hunks waiting for review, none when review has not begun.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoMap`] with no map open.
    pub fn map_hunks(&self) -> Result<Vec<MapHunk>, WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        Ok(Review::of(document)
            .map(|review| review.listed())
            .unwrap_or_default())
    }
}

impl MapDocument {
    /// Ends review when nothing differs from where it began.
    pub(super) fn settle_review(&mut self) {
        if self.staged.as_ref() == Some(&self.file) {
            self.staged = None;
        }
    }
}

/// `map.stage`.
pub(crate) struct MapStage;

impl<C: HasWorkbench + 'static> Command<C> for MapStage {
    const ID: &'static str = MAP_STAGE;
    const SUMMARY: &'static str = "Begins review of the open map's edits: the map as it is now is \
        kept, and each edit after it waits as a hunk to accept or reject. The agent's map edits \
        begin it themselves; while it is open it is left as it is.";
    type Params = NoParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<(), C>, WorkbenchError> {
        let document = context.workbench().map_mut()?;
        if document.staged.is_none() {
            document.staged = Some(document.file.clone());
            document.touch();
        }
        Ok(Outcome::outside_history(()))
    }
}

/// `map.unstage`.
pub(crate) struct MapUnstage;

impl<C: HasWorkbench + 'static> Command<C> for MapUnstage {
    const ID: &'static str = MAP_UNSTAGE;
    const SUMMARY: &'static str = "Ends the open map's review when nothing waits in it, as after \
        an agent's map command that changed nothing; a review with hunks is left open.";
    type Params = NoParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<(), C>, WorkbenchError> {
        context.workbench().map_mut()?.settle_review();
        Ok(Outcome::outside_history(()))
    }
}

/// `map.staged`.
pub(crate) struct MapStaged;

impl<C: HasWorkbench + 'static> Command<C> for MapStaged {
    const ID: &'static str = MAP_STAGED;
    const SUMMARY: &'static str = "Lists the open map's hunks waiting for review: for each, the \
        entities it is in, and the keys and brushes it adds, changes or removes.";
    type Params = NoParams;
    type Output = Vec<MapHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _: NoParams,
    ) -> Result<Outcome<Vec<MapHunk>, C>, WorkbenchError> {
        Ok(Outcome::unchanged(context.workbench().map_hunks()?))
    }
}

/// The parameters of `map.accept` and `map.reject`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapHunksParams {
    /// The hunks' numbers, as `map.staged` lists them now; every hunk when empty.
    #[serde(default)]
    pub hunks: Vec<MapHunkNumber>,
}

/// `map.accept`.
pub(crate) struct MapAccept;

impl<C: HasWorkbench + 'static> Command<C> for MapAccept {
    const ID: &'static str = MAP_ACCEPT;
    const SUMMARY: &'static str = "Accepts hunks of the open map's review, every one when none \
        are named: their change is kept and no longer waits.";
    type Params = MapHunksParams;
    type Output = Vec<MapHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapHunksParams,
    ) -> Result<Outcome<Vec<MapHunk>, C>, WorkbenchError> {
        let workbench = context.workbench();
        let document = workbench.map_mut()?;
        let Some(review) = Review::of(document) else {
            return Ok(Outcome::outside_history(Vec::new()));
        };
        let chosen = review.chosen(&params.hunks)?;
        // Accepted parts are the map now's, so its text is preferred.
        let entities = rebuild(&review.resolve(&chosen), &[review.file, review.staged]);
        let mut staged = review.staged.clone();
        staged.entities = entities;
        document.staged = Some(staged);
        document.settle_review();
        document.touch();
        Ok(Outcome::outside_history(workbench.map_hunks()?))
    }
}

/// `map.reject`.
pub(crate) struct MapReject;

impl<C: HasWorkbench + 'static> Command<C> for MapReject {
    const ID: &'static str = MAP_REJECT;
    const SUMMARY: &'static str = "Rejects hunks of the open map's review, every one when none \
        are named: the parts they replaced are put back, as an edit undo takes back.";
    type Params = MapHunksParams;
    type Output = Vec<MapHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapHunksParams,
    ) -> Result<Outcome<Vec<MapHunk>, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        if document.staged.is_none() {
            return Ok(Outcome::unchanged(Vec::new()));
        }
        let ((), step) = edit_map(context, |document| {
            let Some(review) = Review::of(document) else {
                return Ok(());
            };
            let chosen = review.chosen(&params.hunks)?;
            let kept: Vec<usize> = review
                .all()
                .into_iter()
                .filter(|index| !chosen.contains(index))
                .collect();
            // Rejected parts are the map's as review began, so its text is preferred.
            let entities = rebuild(&review.resolve(&kept), &[review.staged, review.file]);
            document.replace_entities(entities);
            Ok(())
        })?;
        let hunks = context.workbench().map_hunks()?;
        Ok(Outcome::undoable(hunks, step))
    }
}

#[cfg(test)]
mod tests;
