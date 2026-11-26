// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.fire` and `map.logic_issues` (L-76, MAP-35): an entity of the open map fired in `qs-sim`
//! with the project's server program, with the trace of what the program did about it, and the
//! issues only running shows. The map is spawned as `map.entities_removed_by_skill` spawns it
//! (`crate::map_filter`), the player brought into the game, and the entity's `use` or `touch`
//! run; nothing here reads a classname, so a mod's triggers are traced as id's are.
//!
//! Implements `docs/spec/fire.md` FIRE-4 and FIRE-5.

use std::collections::{BTreeMap, BTreeSet};

use qc_progs::Progs;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use qs_sim::scenario::{SCN_FRAMETIME, Seconds};
use qs_sim::{
    Callback, FIRE_SECONDS, FireBy, FireError, FireOptions, FireTrace, MapPlace, TraceEnd,
    TraceEvent, World,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_filter::{GameMode, MapSpawn, Skill};
use crate::{EntityProblem, HasWorkbench, Workbench, WorkbenchError};
use qs_map_edit::EntityIndex;

/// The id of `map.fire`.
pub const MAP_FIRE: &str = "map.fire";
/// The id of `map.logic_issues`.
pub const MAP_LOGIC_ISSUES: &str = "map.logic_issues";

/// The uses of one entity in one trace above which the chain comes back to it (FIRE-4.4).
pub const FIRE_LOOP_USES: usize = 8;
/// The most firings an entity is given to fire its targets (FIRE-4.5).
pub const FIRE_COUNT_USES: usize = 16;
/// The length of each trace `map.logic_issues` takes, in seconds (FIRE-4.1).
pub const FIRE_ISSUE_SECONDS: Seconds = Seconds::new(5.0);
/// The key an entity is fired by (MAP-8.8).
const TARGETNAME: &[u8] = b"targetname";

/// Which function fires an entity (FIRE-1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FiredBy {
    /// Its `use` when it has one, else its `touch`.
    #[default]
    Either,
    /// Its `use`, as another entity's targets are fired.
    Use,
    /// Its `touch`, as a player walking into it.
    Touch,
}

impl From<FiredBy> for FireBy {
    fn from(by: FiredBy) -> Self {
        match by {
            FiredBy::Either => Self::Either,
            FiredBy::Use => Self::Use,
            FiredBy::Touch => Self::Touch,
        }
    }
}

/// The parameters of `map.fire`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FireParams {
    /// The entity, by its place in the map.
    pub entity: EntityIndex,
    /// The skill the map is spawned at.
    #[serde(default)]
    pub skill: Skill,
    /// The kind of game.
    #[serde(default)]
    pub mode: GameMode,
    /// Which function fires it.
    #[serde(default)]
    pub by: FiredBy,
    /// The longest the trace runs, in simulated seconds: 10 when not given, 300 at most.
    #[serde(default)]
    pub seconds: Option<f64>,
}

/// One of the four functions an entity is called by (FIRE-2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackKind {
    /// `use`.
    Use,
    /// `touch`.
    Touch,
    /// `think`.
    Think,
    /// `blocked`.
    Blocked,
}

impl From<Callback> for CallbackKind {
    fn from(callback: Callback) -> Self {
        match callback {
            Callback::Use => Self::Use,
            Callback::Touch => Self::Touch,
            Callback::Think => Self::Think,
            Callback::Blocked => Self::Blocked,
        }
    }
}

/// An edict's number in the spawned game (EDICT-1.4): 0 the world, 1 the player.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Edict(usize);

impl Edict {
    /// Edict `number`.
    #[must_use]
    pub const fn new(number: usize) -> Self {
        Self(number)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// An entity a trace names: its edict in the game, and its place in the map when it is one of
/// the map's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Fired {
    /// The edict's number.
    pub edict: Edict,
    /// Its place in the map; `None` for an entity the game spawned itself, and the player.
    pub entity: Option<EntityIndex>,
}

/// A field written, with its value as a scenario writes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FieldWritten {
    /// The field.
    pub name: String,
    /// Its value.
    pub value: String,
}

/// One thing a firing did (FIRE-2), each with the game's time then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FiredEvent {
    /// A `use`, `touch`, `think` or `blocked` of an entity ran.
    Ran {
        /// The game's time.
        time: f32,
        /// Which function.
        callback: CallbackKind,
        /// The entity.
        entity: Fired,
        /// Its classname.
        classname: String,
        /// The function's name.
        function: String,
    },
    /// The program looked an entity up by its `targetname`.
    Looked {
        /// The game's time.
        time: f32,
        /// The entity whose targets these are.
        by: Fired,
        /// The name looked for.
        name: String,
        /// Whether the search began at the world, so that finding none means no entity has
        /// the name.
        from_start: bool,
        /// The entity found, when there was one.
        found: Option<Fired>,
    },
    /// An entity was looked up and has no `use`: firing it does nothing.
    Unanswered {
        /// The game's time.
        time: f32,
        /// The entity whose target it is.
        by: Fired,
        /// The entity.
        entity: Fired,
        /// Its classname.
        classname: String,
    },
    /// An entity was removed.
    Removed {
        /// The game's time.
        time: f32,
        /// The entity.
        entity: Fired,
        /// The classname it had.
        classname: String,
    },
    /// The program spawned an entity.
    Spawned {
        /// The game's time.
        time: f32,
        /// Its edict.
        edict: Edict,
        /// Its classname.
        classname: String,
    },
    /// `centerprint` printed to the player.
    Print {
        /// The game's time.
        time: f32,
        /// The text.
        text: String,
    },
    /// A sound started.
    Sound {
        /// The game's time.
        time: f32,
        /// The entity it comes from.
        entity: Fired,
        /// The sample.
        sample: String,
    },
    /// Fields of an entity the firing reached were written.
    Changed {
        /// The game's time.
        time: f32,
        /// The entity.
        entity: Fired,
        /// The fields, with their values then.
        fields: Vec<FieldWritten>,
    },
}

/// Why a trace ended (FIRE-3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FireEnd {
    /// Nothing the firing reached has a think pending.
    Quiet,
    /// The trace ran its whole length.
    Length,
    /// The program asked the host for a command, which was not run.
    Host {
        /// The command, as the console reads it.
        command: String,
    },
    /// A function failed.
    Error {
        /// The error.
        message: String,
    },
}

/// What `map.fire` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapFired {
    /// The function the entity was fired by.
    pub fired_by: CallbackKind,
    /// What happened, in order.
    pub events: Vec<FiredEvent>,
    /// Why the trace ended.
    pub end: FireEnd,
    /// The simulated seconds the trace ran.
    pub seconds: f64,
}

/// The parameters of `map.logic_issues`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogicParams {
    /// The skill the map is spawned at.
    #[serde(default)]
    pub skill: Skill,
    /// The kind of game.
    #[serde(default)]
    pub mode: GameMode,
}

/// What is wrong with a map's logic (FIRE-4).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LogicKind {
    /// An entity fired has no `use`: firing it does nothing (FIRE-4.2).
    NothingToUse,
    /// A chain removes an entity and then fires it (FIRE-4.3).
    RemovedThenFired,
    /// A chain comes back to an entity again and again (FIRE-4.4).
    Loop,
    /// Firing an entity stops the game with an error (FIRE-4.4).
    Error,
    /// An entity fires its targets only at a firing the map cannot reach (FIRE-4.5).
    NeverReached,
}

/// One issue of a map's logic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct LogicIssue {
    /// The entity the issue is about.
    pub entity: EntityIndex,
    /// What is wrong.
    pub kind: LogicKind,
    /// The entity that was fired when it showed.
    pub fired: EntityIndex,
    /// What is wrong, in words.
    pub message: String,
}

/// What `map.logic_issues` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LogicIssues {
    /// The issues, by entity.
    pub issues: Vec<LogicIssue>,
    /// The entities fired to find them.
    pub fired: usize,
}

/// The issues `map.logic_issues` last found, for `map.issues` (FIRE-5.3).
#[derive(Clone, Debug, Default)]
pub(crate) struct LogicFound {
    /// The map's text when they were found.
    map: Vec<u8>,
    /// The workbench's generation then.
    generation: u64,
    problems: Vec<EntityProblem>,
}

impl LogicFound {
    /// The problems, while the map is still `map` and the project's text at `generation`.
    pub(crate) fn problems(&self, map: &[u8], generation: u64) -> Vec<EntityProblem> {
        if self.map == map && self.generation == generation {
            self.problems.clone()
        } else {
            Vec::new()
        }
    }
}

/// A spawned map with the player in it, ready to fire, and which edict is which map entity.
struct Stage<'p, 'a> {
    progs: &'p Progs<'a>,
    world: World,
    /// The place in the map of each edict that is one of its entities.
    entities: BTreeMap<usize, EntityIndex>,
}

impl<'p, 'a> Stage<'p, 'a> {
    /// `world`, just spawned from `spawn`, settled and with the player in the game (FIRE-4.1).
    fn new(
        spawn: &MapSpawn,
        progs: &'p Progs<'a>,
        mut world: World,
    ) -> Result<Self, WorkbenchError> {
        world.settle(progs).map_err(|error| spawn.failed(error))?;
        world
            .enter(progs, SCN_FRAMETIME)
            .map_err(|error| spawn.failed(error))?;
        let entities = spawn
            .places()
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(place, &entity)| {
                let edict = world.map_entity(progs, MapPlace::new(place))?;
                Some((edict, EntityIndex::new(entity)))
            })
            .collect();
        Ok(Self {
            progs,
            world,
            entities,
        })
    }

    /// The edict of the map's entity `entity`, when the game kept it.
    fn edict(&self, entity: EntityIndex) -> Option<usize> {
        self.entities
            .iter()
            .find(|&(_, &place)| place == entity)
            .map(|(&edict, _)| edict)
    }

    /// Fires `edict` in a copy of the world.
    fn fire(&self, edict: usize, options: FireOptions) -> Result<FireTrace, FireError> {
        self.world.clone().fire(self.progs, edict, options)
    }

    /// `trace` with each entity's place in the map (FIRE-5.1).
    fn answer(&self, trace: FireTrace) -> MapFired {
        // An edict freed and taken again is no longer the map's entity.
        let mut entities = self.entities.clone();
        let mut events = Vec::with_capacity(trace.events.len());
        for event in trace.events {
            let fired = |edict: usize| Fired {
                edict: Edict::new(edict),
                entity: entities.get(&edict).copied(),
            };
            let answered = match event {
                TraceEvent::Ran {
                    time,
                    callback,
                    entity,
                    classname,
                    function,
                } => FiredEvent::Ran {
                    time,
                    callback: callback.into(),
                    entity: fired(entity),
                    classname,
                    function,
                },
                TraceEvent::Looked {
                    time,
                    by,
                    name,
                    from_start,
                    found,
                } => FiredEvent::Looked {
                    time,
                    by: fired(by),
                    name,
                    from_start,
                    found: found.map(fired),
                },
                TraceEvent::Unanswered {
                    time,
                    by,
                    entity,
                    classname,
                } => FiredEvent::Unanswered {
                    time,
                    by: fired(by),
                    entity: fired(entity),
                    classname,
                },
                TraceEvent::Removed {
                    time,
                    entity,
                    classname,
                } => {
                    let removed = fired(entity);
                    entities.remove(&entity);
                    FiredEvent::Removed {
                        time,
                        entity: removed,
                        classname,
                    }
                }
                TraceEvent::Spawned {
                    time,
                    entity,
                    classname,
                } => {
                    entities.remove(&entity);
                    FiredEvent::Spawned {
                        time,
                        edict: Edict::new(entity),
                        classname,
                    }
                }
                TraceEvent::Print { time, text } => FiredEvent::Print { time, text },
                TraceEvent::Sound {
                    time,
                    entity,
                    sample,
                } => FiredEvent::Sound {
                    time,
                    entity: fired(entity),
                    sample,
                },
                TraceEvent::Changed {
                    time,
                    entity,
                    fields,
                } => FiredEvent::Changed {
                    time,
                    entity: fired(entity),
                    fields: fields
                        .into_iter()
                        .map(|field| FieldWritten {
                            value: field.value.shown(),
                            name: field.name,
                        })
                        .collect(),
                },
            };
            events.push(answered);
        }
        MapFired {
            fired_by: trace.fired_by.into(),
            events,
            end: match trace.end {
                TraceEnd::Quiet => FireEnd::Quiet,
                TraceEnd::Length => FireEnd::Length,
                TraceEnd::Host(command) => FireEnd::Host {
                    command: command.to_string(),
                },
                TraceEnd::Error(message) => FireEnd::Error { message },
            },
            seconds: trace.seconds,
        }
    }
}

/// An entity of the map that has keys naming entities to fire (MAP-8.8).
struct Firer {
    entity: EntityIndex,
    /// The names its keys hold.
    names: BTreeSet<String>,
    /// Its own `targetname`, when it has one.
    targetname: Option<String>,
}

/// The issues of one map, gathered trace by trace (FIRE-4).
struct Finder<'s, 'p, 'a> {
    stage: &'s Stage<'p, 'a>,
    /// The `targetname` of each entity of the map that has one.
    targetnames: BTreeMap<EntityIndex, String>,
    issues: BTreeSet<LogicIssue>,
}

impl Finder<'_, '_, '_> {
    /// How the issues name an entity: its place and its classname.
    fn named(entity: EntityIndex, classname: &str) -> String {
        format!("entity {} ({classname})", entity.get())
    }

    /// The issues `trace` shows, `fired` the entity fired (FIRE-4.2 to FIRE-4.4).
    fn read(&mut self, fired: EntityIndex, trace: &MapFired) {
        // The names of the entities this trace removed, with the entity each was.
        let mut removed: BTreeMap<&str, EntityIndex> = BTreeMap::new();
        let mut uses: BTreeMap<EntityIndex, (usize, &str)> = BTreeMap::new();
        for event in &trace.events {
            match event {
                // FIRE-4.2, FIRE-4.6: a target of the map's that has no use.
                FiredEvent::Unanswered {
                    entity:
                        Fired {
                            entity: Some(entity),
                            ..
                        },
                    classname,
                    ..
                } => {
                    self.issues.insert(LogicIssue {
                        entity: *entity,
                        kind: LogicKind::NothingToUse,
                        fired,
                        message: format!(
                            "{} is fired and has no use function: firing it does nothing",
                            Self::named(*entity, classname)
                        ),
                    });
                }
                FiredEvent::Removed {
                    entity:
                        Fired {
                            entity: Some(entity),
                            ..
                        },
                    ..
                } => {
                    if let Some(name) = self.targetnames.get(entity) {
                        removed.insert(name, *entity);
                    }
                }
                // FIRE-4.3: a search from the world for a name this trace removed, finding none.
                FiredEvent::Looked {
                    name,
                    from_start: true,
                    found: None,
                    ..
                } => {
                    if let Some(&entity) = removed.get(name.as_str()) {
                        self.issues.insert(LogicIssue {
                            entity,
                            kind: LogicKind::RemovedThenFired,
                            fired,
                            message: format!(
                                "entity {} ({name:?}) is removed and then fired by the same \
                                 chain: nothing is left to fire",
                                entity.get()
                            ),
                        });
                    }
                }
                FiredEvent::Ran {
                    callback: CallbackKind::Use,
                    entity:
                        Fired {
                            entity: Some(entity),
                            ..
                        },
                    classname,
                    ..
                } => {
                    let (count, _) = uses.entry(*entity).or_insert((0, classname));
                    *count = count.saturating_add(1);
                }
                _ => {}
            }
        }
        // FIRE-4.4: a chain that comes back to an entity, and one that stops the game.
        for (entity, (count, classname)) in uses {
            if count > FIRE_LOOP_USES {
                self.issues.insert(LogicIssue {
                    entity,
                    kind: LogicKind::Loop,
                    fired,
                    message: format!(
                        "{} is used {count} times in {} seconds once entity {} is fired: the \
                         chain comes back to it",
                        Self::named(entity, classname),
                        trace.seconds.ceil(),
                        fired.get()
                    ),
                });
            }
        }
        if let FireEnd::Error { message } = &trace.end {
            self.issues.insert(LogicIssue {
                entity: fired,
                kind: LogicKind::Error,
                fired,
                message: format!("firing entity {} stops the game: {message}", fired.get()),
            });
        }
    }

    /// FIRE-4.5: `firer`, which `firers` entities fire, fires its own targets only at a firing
    /// after those.
    fn count(&mut self, firer: &Firer, edict: usize, firers: usize) {
        let mut world = self.stage.world.clone();
        let options = FireOptions {
            by: FireBy::Use,
            length: Seconds::new(0.0),
            frametime: SCN_FRAMETIME,
        };
        for firing in 1..=firers.saturating_add(FIRE_COUNT_USES) {
            let Ok(trace) = world.fire(self.stage.progs, edict, options) else {
                return;
            };
            let fires = trace.events.iter().any(|event| {
                matches!(event, TraceEvent::Looked { by, name, .. }
                    if *by == edict && firer.names.contains(name))
            });
            if fires {
                if firing > firers {
                    self.issues.insert(LogicIssue {
                        entity: firer.entity,
                        kind: LogicKind::NeverReached,
                        fired: firer.entity,
                        message: format!(
                            "entity {} fires its targets at its {firing}th firing, and {firers} \
                             {} it: its targets are never fired",
                            firer.entity.get(),
                            if firers == 1 {
                                "entity fires"
                            } else {
                                "entities fire"
                            }
                        ),
                    });
                }
                return;
            }
            if !world.in_use(edict) || matches!(trace.end, TraceEnd::Error(_)) {
                return;
            }
        }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The entities of the open map with a key naming entities to fire, and every entity's
    /// `targetname` (MAP-8.8).
    fn firers(&mut self) -> (Vec<Firer>, BTreeMap<EntityIndex, String>) {
        let keys = self.targeting_keys();
        let Some(document) = self.map() else {
            return (Vec::new(), BTreeMap::new());
        };
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let mut firers = Vec::new();
        let mut targetnames = BTreeMap::new();
        for (place, entity) in document.file().entities.iter().enumerate() {
            let index = EntityIndex::new(place);
            let targetname = entity
                .value(TARGETNAME)
                .filter(|name| !name.is_empty())
                .map(text);
            if let Some(name) = &targetname {
                targetnames.insert(index, name.clone());
            }
            let names: BTreeSet<String> = keys
                .iter()
                .filter_map(|key| entity.value(key.as_bytes()))
                .filter(|name| !name.is_empty())
                .map(text)
                .collect();
            if !names.is_empty() {
                firers.push(Firer {
                    entity: index,
                    names,
                    targetname,
                });
            }
        }
        (firers, targetnames)
    }

    /// Fires `params.entity` of the open map and answers the trace (FIRE-5.1).
    fn map_fire(&mut self, params: &FireParams) -> Result<MapFired, WorkbenchError> {
        let spawn = self.map_spawn()?;
        let entity = params.entity;
        spawn.world(params.skill, params.mode, |progs, world| {
            let mut stage = Stage::new(&spawn, progs, world)?;
            let edict = stage
                .edict(entity)
                .ok_or(WorkbenchError::NotSpawned { entity })?;
            let options = FireOptions {
                by: params.by.into(),
                length: params.seconds.map_or(FIRE_SECONDS, Seconds::new),
                frametime: SCN_FRAMETIME,
            };
            let trace = stage
                .world
                .fire(progs, edict, options)
                .map_err(|error| WorkbenchError::NotFired { entity, error })?;
            Ok(stage.answer(trace))
        })
    }

    /// The issues of the open map only running shows (FIRE-4), kept for `map.issues`
    /// (FIRE-5.3).
    fn map_logic_issues(&mut self, params: &LogicParams) -> Result<LogicIssues, WorkbenchError> {
        let spawn = self.map_spawn()?;
        let (firers, targetnames) = self.firers();
        let found = spawn.world(params.skill, params.mode, |progs, world| {
            let stage = Stage::new(&spawn, progs, world)?;
            let mut finder = Finder {
                stage: &stage,
                targetnames,
                issues: BTreeSet::new(),
            };
            let options = FireOptions {
                by: FireBy::Either,
                length: FIRE_ISSUE_SECONDS,
                frametime: SCN_FRAMETIME,
            };
            let mut fired = 0_usize;
            for firer in &firers {
                // FIRE-4.1: an entity the game did not keep, or with neither function, is not
                // fired.
                let Some(edict) = stage.edict(firer.entity) else {
                    continue;
                };
                let Ok(trace) = stage.fire(edict, options) else {
                    continue;
                };
                fired = fired.saturating_add(1);
                finder.read(firer.entity, &stage.answer(trace));
                let firing = firer.targetname.as_ref().map_or(0, |name| {
                    firers
                        .iter()
                        .filter(|other| other.names.contains(name))
                        .count()
                });
                if firing > 0 {
                    finder.count(firer, edict, firing);
                }
            }
            Ok(LogicIssues {
                issues: finder.issues.into_iter().collect(),
                fired,
            })
        })?;
        let generation = self.generation;
        if let Some(document) = self.map() {
            let file = document.file();
            let problems = found
                .issues
                .iter()
                .map(|issue| EntityProblem {
                    entity: issue.entity,
                    line: file
                        .entities
                        .get(issue.entity.get())
                        .map_or(0, qs_map::Entity::line),
                    message: issue.message.clone(),
                    fix: None,
                })
                .collect();
            self.logic = LogicFound {
                map: file.to_bytes(),
                generation,
                problems,
            };
        }
        Ok(found)
    }
}

/// `map.fire`.
struct MapFire;

impl<C: HasWorkbench + 'static> Command<C> for MapFire {
    const ID: &'static str = MAP_FIRE;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Fires an entity of the open map as the game would, its use or \
        its touch run in the simulation with the project's program, and answers what the \
        QuakeC did about it in order: each use, touch and think the firing reached, each entity \
        looked up by its targetname, removed or spawned, each centerprint and sound, and the \
        fields written. Changes nothing: not the map, not a game that is playing it.";
    type Params = FireParams;
    type Output = MapFired;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FireParams,
    ) -> Result<Outcome<MapFired, C>, WorkbenchError> {
        let fired = context.workbench().map_fire(&params)?;
        Ok(Outcome::outside_history(fired))
    }
}

/// `map.logic_issues`.
struct MapLogicIssues;

impl<C: HasWorkbench + 'static> Command<C> for MapLogicIssues {
    const ID: &'static str = MAP_LOGIC_ISSUES;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Fires every entity of the open map that has targets, each in \
        a copy of the spawned map, and lists what only running shows: a target with no use \
        function, a chain that removes what it then fires, a chain that comes back to the same \
        entity or stops the game, and an entity that fires its targets only at a firing the map \
        cannot reach, such as a counter with too high a count. The issues join map.issues until \
        the map or the code changes.";
    type Params = LogicParams;
    type Output = LogicIssues;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: LogicParams,
    ) -> Result<Outcome<LogicIssues, C>, WorkbenchError> {
        let issues = context.workbench().map_logic_issues(&params)?;
        Ok(Outcome::outside_history(issues))
    }
}

/// Registers `map.fire` and `map.logic_issues`.
///
/// # Errors
///
/// [`RegisterError`] when one is already registered.
pub(crate) fn register_map_logic<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapFire)?;
    registry.register(MapLogicIssues)
}

#[cfg(test)]
mod tests;
