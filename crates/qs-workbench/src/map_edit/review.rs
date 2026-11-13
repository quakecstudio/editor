// SPDX-License-Identifier: GPL-3.0-or-later

//! Map edits staged for review (P6-21, MAP-29): the agent's changes to the open map wait as
//! hunks to accept or reject one by one, as its code edits do (P4-04). The map is edited at
//! once, so a compile and the game see the change; what is staged is the map as it was when
//! review began. A hunk is a run of changed parts: an entity's keys, or one of its brushes, so
//! brushes added to the world are hunks of their own and not one change to the world. A
//! rejected hunk puts back the parts it replaced, as an edit undo takes back; an accepted one
//! is kept. Once no hunk is left, review ends.

use std::ops::Range;

use qs_commands::{Command, NoParams, Outcome};
use qs_diff::{Hunk, hunks, script};
use qs_map::{Brush, Entity, Item, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{MapDocument, edit_map};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.stage`.
pub const MAP_STAGE: &str = "map.stage";
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

/// The entities of `parts`, each entity its keys and the brushes after it. An entity whose keys
/// and brushes are an entity of `known`'s is that entity, so the text of what no hunk touched
/// is written as it was read.
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

/// What a hunk changes, as the person reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapHunk {
    /// Its number in the list, which `map.accept` and `map.reject` take.
    pub index: MapHunkNumber,
    /// The entities it is in, by their place in the map now and their classname.
    pub entities: Vec<String>,
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
        // The entity each part of the map now is in, by place.
        let mut owner = Vec::with_capacity(self.after.len());
        let mut place = 0_usize;
        for (i, part) in self.after.iter().enumerate() {
            if let (Part::Keys(_), true) = (part, i > 0) {
                place = place.saturating_add(1);
            }
            owner.push(place);
        }
        let classname = |place: usize| {
            self.file
                .entities
                .get(place)
                .and_then(Entity::classname)
                .map_or_else(String::new, |name| {
                    String::from_utf8_lossy(name).into_owned()
                })
        };
        self.hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| {
                let count = |parts: &[Part], range: &Range<usize>, keys: bool| {
                    parts
                        .get(range.clone())
                        .unwrap_or_default()
                        .iter()
                        .filter(|part| matches!(part, Part::Keys(_)) == keys)
                        .count()
                };
                let mut places: Vec<usize> =
                    owner.get(hunk.new.clone()).unwrap_or_default().to_vec();
                if places.is_empty() {
                    // Only removals: the entity the removed parts were in, or follow.
                    places.push(owner.get(hunk.new.start).copied().unwrap_or(place));
                }
                places.dedup();
                MapHunk {
                    index: MapHunkNumber::new(index),
                    entities: places
                        .into_iter()
                        .map(|place| format!("{place} {}", classname(place)))
                        .collect(),
                    keys: count(&self.before, &hunk.old, true).max(count(
                        &self.after,
                        &hunk.new,
                        true,
                    )),
                    brushes_added: count(&self.after, &hunk.new, false),
                    brushes_removed: count(&self.before, &hunk.old, false),
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
        let places: Vec<usize> = asked.iter().map(|number| number.get()).collect();
        match places.iter().find(|&&index| index >= self.hunks.len()) {
            Some(&index) => Err(WorkbenchError::NoMapHunk { index }),
            None => Ok(places),
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
    /// Ends review when nothing is left to decide.
    fn settle_review(&mut self) {
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
        }
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
        let entities = rebuild(&review.resolve(&chosen), &[review.staged, review.file]);
        let mut staged = review.staged.clone();
        staged.entities = entities;
        document.staged = Some(staged);
        document.settle_review();
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
            let entities = rebuild(&review.resolve(&kept), &[review.file, review.staged]);
            document.replace_entities(entities);
            document.settle_review();
            Ok(())
        })?;
        let hunks = context.workbench().map_hunks()?;
        Ok(Outcome::undoable(hunks, step))
    }
}

#[cfg(test)]
mod tests;
