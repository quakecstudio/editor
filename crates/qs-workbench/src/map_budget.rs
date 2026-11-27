// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.budget` (L-79, MAP-38): how much of the target engine's limits the open map uses, and
//! of id's. The lumps and the face extent come from the last compile's report, or from the BSP
//! the map was opened from; the edicts, precaches and light styles from the map spawned in
//! `qs-sim` on each skill by the project's own program, with room past the bounds so a map
//! past them is counted, and a spawn that runs out of that room is past every bound. Limits
//! passed and nearly reached join `map.issues` while the map, the target and the sources are
//! the same. The spawns may run on their own thread, as `map.playability`'s search does.
//!
//! Implements `docs/spec/budget.md` BUDGET-1 to BUDGET-3; the limits are ENGT-5's.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use qc_builtins::{FatalError, Precache, PrecacheRoom, RuntimeError};
use qc_edict::EdictCapacity;
use qc_engines::{Bound, LimitFact, MapFormat, Measure};
use qc_vm::ErrorKind;
use qs_bsp::{Bsp, BspError, BspLoader, BspVersion};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use qs_map_edit::{EntityProblem, LumpCounts, MapSource, Written};
use qs_sim::spawn::{LoadError, SpawnError};
use qs_sim::{Room, WorldError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use qc_engines::{Bsp2, Engine, MapLimit, Past};

use crate::map_balance::SKILLS;
use crate::map_filter::{GameMode, MapSpawn, Skill};
use crate::map_logic::SourceKey;
use crate::map_playable::Answer;
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.budget`.
pub const MAP_BUDGET: &str = "map.budget";
/// The id of `map.budget_found`.
pub const MAP_BUDGET_FOUND: &str = "map.budget_found";

/// The share of a bound at which a limit is near (BUDGET-1.3).
pub const BUDGET_NEAR_SHARE: f64 = 0.9;

/// Percent in one.
const PERCENT: f64 = 100.0;

/// What the map uses of one limit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Used {
    /// This many; for a limit measured in a spawn, on the first skill that uses the most.
    Count {
        /// How many.
        count: usize,
        /// The skill, for a limit measured in a spawn.
        skill: Option<Skill>,
    },
    /// At least this many: the spawn on `skill` ran out of the room it was given (BUDGET-2.4).
    AtLeast {
        /// One more than the room held.
        count: usize,
        /// The skill.
        skill: Skill,
    },
    /// Not measured.
    NotMeasured {
        /// Why.
        why: String,
    },
}

impl Used {
    /// The count, at least, when measured.
    const fn count(&self) -> Option<usize> {
        match self {
            Self::Count { count, .. } | Self::AtLeast { count, .. } => Some(*count),
            Self::NotMeasured { .. } => None,
        }
    }
}

/// An engine's bound on a limit (ENGT-5.7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineBound {
    /// At most this.
    At {
        /// The most.
        most: usize,
        /// What the engine does past it (ENGT-5.5).
        past: Past,
        /// Where it is stated: a source of `sources.toml`, or `id`.
        source: String,
        /// The file.
        path: String,
    },
    /// The engine's source sets none.
    Unlimited,
    /// The engine has no source.
    Unknown,
}

impl From<Bound> for LineBound {
    fn from(bound: Bound) -> Self {
        match bound {
            Bound::At(LimitFact {
                most,
                past,
                source,
                path,
                limit: _,
                applies: _,
            }) => Self::At {
                most,
                past,
                source: source.to_owned(),
                path: path.to_owned(),
            },
            Bound::Unlimited => Self::Unlimited,
            Bound::Unknown => Self::Unknown,
        }
    }
}

/// Where a limit stands (BUDGET-1.3), worst first.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// The map uses more than the bound.
    Past,
    /// The map uses at least [`BUDGET_NEAR_SHARE`] of the bound.
    Near,
    /// Below that, or the engine sets no bound.
    Within,
    /// The bound is unknown or the use not measured.
    Unknown,
}

/// One limit of one engine (BUDGET-1.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BudgetLine {
    /// The limit.
    pub limit: MapLimit,
    /// Its name in words, such as `clipnodes`.
    pub name: String,
    /// What the map uses.
    pub used: Used,
    /// The engine's bound for the map's format.
    pub bound: LineBound,
    /// What the map uses over the bound; `None` without both.
    pub share: Option<f64>,
    /// Where it stands.
    pub standing: Standing,
}

/// One engine's budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineBudget {
    /// The engine (ENGT-3.4).
    pub engine: Engine,
    /// Its display name, such as `QuakeSpasm`.
    pub name: String,
    /// Whether it reads BSP2 (ENGT-5.6).
    pub bsp2: Bsp2,
    /// A line for each limit, worst first (BUDGET-1.4).
    pub lines: Vec<BudgetLine>,
}

/// What `map.budget` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapBudget {
    /// The target engine's budget, then id's when the target is another (BUDGET-1.2).
    pub engines: Vec<EngineBudget>,
    /// The format the map was compiled to, or the BSP it was opened from; `None` before a
    /// compile (BUDGET-2.2).
    pub format: Option<Written>,
}

/// The parameters of `map.budget`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BudgetParams {
    /// When the answer is given: at once, the default, or later, the spawns run on their own
    /// thread and the answer taken with `map.budget_found`.
    #[serde(default)]
    pub answer: Answer,
}

/// What a budget is of besides the map: the target engine and the project's sources
/// (BUDGET-3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BudgetInputs {
    target: Option<Engine>,
    sources: SourceKey,
}

/// What a budget is of: the map's text and its [`BudgetInputs`] (BUDGET-3.3). A budget of
/// another key is stale.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BudgetKey {
    map: Vec<u8>,
    inputs: BudgetInputs,
}

/// The budget `map.budget` last made, for `map.issues` and `map.budget_found`.
#[derive(Clone, Debug, Default)]
pub(crate) struct BudgetFound {
    key: BudgetKey,
    found: Option<MapBudget>,
    problems: Vec<EntityProblem>,
}

impl BudgetKey {
    /// The key of the map whose text is `map`, with `inputs`.
    pub(crate) const fn of(map: Vec<u8>, inputs: BudgetInputs) -> Self {
        Self { map, inputs }
    }
}

impl BudgetFound {
    /// The problems, while the budget's key is still `key`.
    pub(crate) fn problems(&self, key: &BudgetKey) -> Vec<EntityProblem> {
        if &self.key == key {
            self.problems.clone()
        } else {
            Vec::new()
        }
    }
}

/// A budget being made on its own thread, of the key it was started at.
#[derive(Debug)]
pub(crate) struct BudgetRun {
    key: BudgetKey,
    done: Receiver<MapBudget>,
}

/// Why a limit is not measured (BUDGET-2.1, BUDGET-2.5).
#[derive(Clone, Debug)]
enum Unmeasured {
    /// The map has not been compiled since it changed.
    NotCompiled,
    /// A compile neither lights nor vises the map.
    NotLit,
    /// The BSP the map was opened from does not read.
    Unreadable(BspError),
    /// The spawn could not be made or failed.
    Spawn(String),
    /// Every skill's spawn ran out of another limit's room.
    RanOut(MapLimit),
}

impl core::fmt::Display for Unmeasured {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompiled => f.write_str("the map has not been compiled since it changed"),
            Self::NotLit => f.write_str("a compile neither lights nor vises the map"),
            Self::Unreadable(error) => write!(f, "the BSP does not read: {error}"),
            Self::Spawn(error) => f.write_str(error),
            Self::RanOut(limit) => write!(f, "every skill's spawn ran out of {}", limit.name()),
        }
    }
}

/// One spawn-measured limit on one skill (BUDGET-2.3, BUDGET-2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpawnUse {
    /// This many.
    Counted(usize, Skill),
    /// More than the room held: at least this many.
    AtLeast(usize, Skill),
}

impl SpawnUse {
    const fn count(self) -> usize {
        match self {
            Self::Counted(count, _) | Self::AtLeast(count, _) => count,
        }
    }

    /// The more of `slot` and `self`; the first of equal counts.
    fn most(self, slot: &mut Option<Self>) {
        if slot.is_none_or(|kept| self.count() > kept.count()) {
            *slot = Some(self);
        }
    }

    const fn used(self) -> Used {
        match self {
            Self::Counted(count, skill) => Used::Count {
                count,
                skill: Some(skill),
            },
            Self::AtLeast(count, skill) => Used::AtLeast { count, skill },
        }
    }
}

/// The light styles a spawn set: how many, and one more than the highest (BUDGET-2.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Styles {
    set: Option<SpawnUse>,
    highest: Option<SpawnUse>,
}

/// What the spawns use (BUDGET-2.3), each the most of the skills with its skill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Spawned {
    edicts: Option<SpawnUse>,
    models: Option<SpawnUse>,
    sounds: Option<SpawnUse>,
    styles: Styles,
    /// The limit a skill's spawn ran out of, when one did.
    ran_out: Option<MapLimit>,
}

impl Spawned {
    /// Its use of a spawn-measured `limit` for an engine of `bound` (BUDGET-2.3): the light
    /// styles set, or one more than the highest when that is past the bound.
    fn of(&self, limit: MapLimit, bound: Bound) -> Option<SpawnUse> {
        match limit {
            MapLimit::Edicts => self.edicts,
            MapLimit::ModelPrecaches => self.models,
            MapLimit::SoundPrecaches => self.sounds,
            MapLimit::LightStyles => {
                let past = |highest: SpawnUse| match bound {
                    Bound::At(fact) => highest.count() > fact.most,
                    Bound::Unlimited | Bound::Unknown => false,
                };
                match self.styles.highest {
                    Some(highest) if past(highest) => Some(highest),
                    Some(_) | None => self.styles.set,
                }
            }
            MapLimit::Models
            | MapLimit::Entities
            | MapLimit::EntityText
            | MapLimit::Planes
            | MapLimit::Nodes
            | MapLimit::Clipnodes
            | MapLimit::Leafs
            | MapLimit::Visleafs
            | MapLimit::Vertexes
            | MapLimit::Faces
            | MapLimit::Marksurfaces
            | MapLimit::Texinfo
            | MapLimit::Edges
            | MapLimit::Surfedges
            | MapLimit::Textures
            | MapLimit::Lighting
            | MapLimit::Visibility
            | MapLimit::FaceExtent => None,
        }
    }
}

/// Where the lumps come from (BUDGET-2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LumpSource {
    /// A compile's report, which has no light or vis.
    Compile,
    /// The BSP the map was opened from.
    Bsp,
}

/// What the map uses, measured once and read for each engine.
struct Measured {
    lumps: Result<(LumpCounts, LumpSource), Unmeasured>,
    format: MapFormat,
    spawned: Result<Spawned, Unmeasured>,
}

impl Measured {
    /// What the map uses of `limit`, for `engine` (BUDGET-2.1, BUDGET-2.3).
    fn used(&self, limit: MapLimit, engine: Engine) -> Used {
        let not = |why: &Unmeasured| Used::NotMeasured {
            why: why.to_string(),
        };
        let count = |count| Used::Count { count, skill: None };
        match limit.measured_by() {
            Measure::Spawn => match &self.spawned {
                Ok(spawned) => match spawned.of(limit, engine.bound(limit, self.format)) {
                    Some(found) => found.used(),
                    None => not(&Unmeasured::RanOut(spawned.ran_out.unwrap_or(limit))),
                },
                Err(why) => not(why),
            },
            Measure::Compile => {
                let (lumps, source) = match &self.lumps {
                    Ok(found) => *found,
                    Err(why) => return not(why),
                };
                match limit {
                    MapLimit::Lighting | MapLimit::Visibility if source == LumpSource::Compile => {
                        not(&Unmeasured::NotLit)
                    }
                    MapLimit::FaceExtent if engine == Engine::Vanilla => count(lumps.face_extent),
                    _ => count(lump_count(&lumps, limit)),
                }
            }
        }
    }
}

/// The count of a compile-measured `limit` in `lumps`; the FitzQuake lineage's face extent.
const fn lump_count(lumps: &LumpCounts, limit: MapLimit) -> usize {
    match limit {
        MapLimit::Models => lumps.models,
        MapLimit::Entities => lumps.entities,
        MapLimit::EntityText => lumps.entity_text,
        MapLimit::Planes => lumps.planes,
        MapLimit::Nodes => lumps.nodes,
        MapLimit::Clipnodes => lumps.clipnodes,
        MapLimit::Leafs => lumps.leafs,
        MapLimit::Visleafs => lumps.visleafs,
        MapLimit::Vertexes => lumps.vertexes,
        MapLimit::Faces => lumps.faces,
        MapLimit::Marksurfaces => lumps.marksurfaces,
        MapLimit::Texinfo => lumps.texinfo,
        MapLimit::Edges => lumps.edges,
        MapLimit::Surfedges => lumps.surfedges,
        MapLimit::Textures => lumps.textures,
        MapLimit::Lighting => lumps.lighting,
        MapLimit::Visibility => lumps.visibility,
        MapLimit::FaceExtent => lumps.fitz_face_extent,
        MapLimit::Edicts
        | MapLimit::ModelPrecaches
        | MapLimit::SoundPrecaches
        | MapLimit::LightStyles => 0,
    }
}

/// The line of `limit` for `engine`, the map using `used` (BUDGET-1.2, BUDGET-1.3).
fn line(engine: Engine, limit: MapLimit, used: Used, format: MapFormat) -> BudgetLine {
    let bound = engine.bound(limit, format);
    let (share, standing) = match (bound, used.count()) {
        (Bound::At(fact), Some(count)) => {
            let share = ratio(count, fact.most);
            let standing = if count > fact.most {
                Standing::Past
            } else if share >= BUDGET_NEAR_SHARE {
                Standing::Near
            } else {
                Standing::Within
            };
            (Some(share), standing)
        }
        (Bound::Unlimited, Some(_)) => (None, Standing::Within),
        (Bound::Unknown, _) | (_, None) => (None, Standing::Unknown),
    };
    BudgetLine {
        limit,
        name: limit.name().to_owned(),
        used,
        bound: bound.into(),
        share,
        standing,
    }
}

/// `count` over `most`, as a fraction; infinite for a bound of 0.
fn ratio(count: usize, most: usize) -> f64 {
    let as_float = |n: usize| u32::try_from(n).map_or_else(|_| f64::from(u32::MAX), f64::from);
    as_float(count) / as_float(most)
}

/// `lines` worst first (BUDGET-1.4): by standing, then by share, largest first, then in the
/// order of ENGT-5.4.
fn worst_first(lines: &mut [BudgetLine]) {
    lines.sort_by(|a, b| {
        a.standing
            .cmp(&b.standing)
            .then_with(|| {
                let share = |line: &BudgetLine| line.share.unwrap_or(f64::NEG_INFINITY);
                share(b).total_cmp(&share(a))
            })
            .then_with(|| a.limit.cmp(&b.limit))
    });
}

/// The budget of `engine` for what `measured` holds.
fn engine_budget(engine: Engine, measured: &Measured) -> EngineBudget {
    let mut lines: Vec<BudgetLine> = MapLimit::ALL
        .into_iter()
        .map(|limit| line(engine, limit, measured.used(limit, engine), measured.format))
        .collect();
    worst_first(&mut lines);
    EngineBudget {
        engine,
        name: engine.name().to_owned(),
        bsp2: engine.bsp2(),
        lines,
    }
}

/// What a past line says the engine does (BUDGET-3.1).
fn consequence(engine: Engine, past: Past) -> String {
    let name = engine.name();
    match past {
        Past::Refused => format!("{name} refuses it"),
        Past::Warned => format!("{name} loads it with a warning"),
        Past::Tools => "id's compiler cannot write it".to_owned(),
        Past::Undefined => format!("{name} writes past its array, which is undefined"),
    }
}

impl EngineBudget {
    /// Its issues, the map's format being `format` (BUDGET-3.1, BUDGET-3.2).
    fn issues(&self, format: Option<&Written>) -> Vec<String> {
        let engine = self.engine;
        let mut out = Vec::new();
        for line in &self.lines {
            let LineBound::At { most, past, .. } = &line.bound else {
                continue;
            };
            let used = match &line.used {
                Used::Count { count, .. } => format!("{count}"),
                Used::AtLeast { count, .. } => format!("at least {count}"),
                Used::NotMeasured { .. } => continue,
            };
            match line.standing {
                Standing::Past => out.push(format!(
                    "{used} {} is past {}'s limit of {most}: {}",
                    line.name,
                    engine.name(),
                    consequence(engine, *past)
                )),
                Standing::Near => {
                    let percent = (line.share.unwrap_or(0.0) * PERCENT).floor();
                    out.push(format!(
                        "{used} {} is {percent}% of {}'s limit of {most}",
                        line.name,
                        engine.name()
                    ));
                }
                Standing::Within | Standing::Unknown => {}
            }
        }
        if let Some(Written::Bsp2 { lump }) = format {
            let name = engine.name();
            let reads = match engine.bsp2() {
                Bsp2::Reads => format!("which {name} reads"),
                Bsp2::DoesNotRead => format!("which {name} does not read: it will not load"),
                Bsp2::Unknown => format!("and whether {name} reads BSP2 is not known"),
            };
            out.push(match lump {
                Some(lump) => format!(
                    "the {} are too many for BSP29, so the map is compiled as BSP2, {reads}",
                    lump.name()
                ),
                None => format!("the map is BSP2, {reads}"),
            });
        }
        out
    }
}

impl MapBudget {
    /// Its issues, the target's (BUDGET-3.1, BUDGET-3.2), each for the whole map.
    fn problems(&self) -> Vec<EntityProblem> {
        self.engines
            .first()
            .map_or_default(|target| target.issues(self.format.as_ref()))
            .into_iter()
            .map(|message| EntityProblem {
                entity: None,
                line: 0,
                message,
                fix: None,
                point: None,
            })
            .collect()
    }
}

/// The largest bound of `engines` on `limit`, id's among them (BUDGET-2.4).
fn largest(engines: &[Engine], limit: MapLimit) -> usize {
    engines
        .iter()
        .chain(&[Engine::Vanilla])
        .filter_map(|engine| match engine.bound(limit, MapFormat::Bsp2) {
            Bound::At(fact) => Some(fact.most),
            Bound::Unlimited | Bound::Unknown => None,
        })
        .max()
        .unwrap_or(0)
}

/// The room a spawn needs past every bound of `engines` (BUDGET-2.4): one more edict and light
/// style, and one more name than each precache list's bound besides its empty entry 0.
fn room(engines: &[Engine]) -> Room {
    let past = |limit| largest(engines, limit).saturating_add(1);
    Room {
        edicts: EdictCapacity::at_least_ids(past(MapLimit::Edicts)),
        precaches: Some(PrecacheRoom {
            models: past(MapLimit::ModelPrecaches).saturating_add(1),
            sounds: past(MapLimit::SoundPrecaches).saturating_add(1),
        }),
        light_styles: Some(past(MapLimit::LightStyles)),
    }
}

impl WorkbenchError {
    /// The limit a spawn that failed with this error ran out of (BUDGET-2.4): ED_Alloc's `no free
    /// edicts`, a full precache list, or a light style past the array; `None` for any other
    /// failure.
    fn ran_out(&self) -> Option<MapLimit> {
        let Self::Spawn { error, .. } = self else {
            return None;
        };
        let run = match error {
            WorldError::Spawn(LoadError::Spawn(SpawnError::NoFreeEdicts)) => {
                return Some(MapLimit::Edicts);
            }
            WorldError::Spawn(LoadError::Run(run)) | WorldError::Frame(run) => run,
            WorldError::Spawn(LoadError::Spawn(_)) | WorldError::Progs(_) => return None,
        };
        let message = match &run.kind {
            ErrorKind::Builtin { message } | ErrorKind::Fatal { message } => message,
            _ => return None,
        };
        // The builtins' own messages, as the VM carries them byte for byte (DIFF-8.2).
        [
            (FatalError::NoFreeEdicts.to_string(), MapLimit::Edicts),
            (
                RuntimeError::ModelPrecacheOverflow.to_string(),
                MapLimit::ModelPrecaches,
            ),
            (
                RuntimeError::SoundPrecacheOverflow.to_string(),
                MapLimit::SoundPrecaches,
            ),
            (
                RuntimeError::InvalidLightstyle.to_string(),
                MapLimit::LightStyles,
            ),
        ]
        .into_iter()
        .find(|(known, _)| known == message)
        .map(|(_, limit)| limit)
    }
}

/// The count a spawn that ran out of `limit`'s room had at least (BUDGET-2.4).
fn at_least(engines: &[Engine], limit: MapLimit) -> usize {
    largest(engines, limit).saturating_add(2)
}

/// Everything a budget needs, all of it its own, to be made on any thread.
struct BudgetJob {
    lumps: Result<(LumpCounts, LumpSource), Unmeasured>,
    format: Option<Written>,
    engines: Vec<Engine>,
    spawn: Result<MapSpawn, String>,
}

impl BudgetJob {
    /// What the map spawns on each skill (BUDGET-2.3 to BUDGET-2.5).
    fn spawned(&self) -> Result<Spawned, Unmeasured> {
        let spawn = self
            .spawn
            .as_ref()
            .map_err(|error| Unmeasured::Spawn(error.clone()))?;
        let mut spawned = Spawned::default();
        for skill in SKILLS {
            let counted = spawn.world(skill, GameMode::Single, |progs, mut world| {
                world.settle(progs).map_err(|error| spawn.failed(error))?;
                SpawnUse::Counted(world.num_edicts(), skill).most(&mut spawned.edicts);
                SpawnUse::Counted(world.precached(Precache::Model), skill)
                    .most(&mut spawned.models);
                SpawnUse::Counted(world.precached(Precache::Sound), skill)
                    .most(&mut spawned.sounds);
                SpawnUse::Counted(world.light_styles_set(), skill).most(&mut spawned.styles.set);
                SpawnUse::Counted(world.light_styles_highest(), skill)
                    .most(&mut spawned.styles.highest);
                Ok(())
            });
            if let Err(error) = counted {
                let limit = error
                    .ran_out()
                    .ok_or_else(|| Unmeasured::Spawn(error.to_string()))?;
                let beyond = SpawnUse::AtLeast(at_least(&self.engines, limit), skill);
                let slot = match limit {
                    MapLimit::Edicts => &mut spawned.edicts,
                    MapLimit::ModelPrecaches => &mut spawned.models,
                    MapLimit::SoundPrecaches => &mut spawned.sounds,
                    _ => &mut spawned.styles.highest,
                };
                beyond.most(slot);
                spawned.ran_out = Some(limit);
            }
        }
        Ok(spawned)
    }

    /// The budget (BUDGET-1).
    fn run(self) -> MapBudget {
        let format = match self.format {
            Some(Written::Bsp2 { .. }) => MapFormat::Bsp2,
            Some(Written::Bsp29) | None => MapFormat::Bsp29,
        };
        let spawned = self.spawned();
        let measured = Measured {
            lumps: self.lumps,
            format,
            spawned,
        };
        MapBudget {
            engines: self
                .engines
                .iter()
                .map(|&engine| engine_budget(engine, &measured))
                .collect(),
            format: self.format,
        }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The target engine and the sources a budget made now would be of (BUDGET-3.3): a budget
    /// made with others, of the same map, is stale.
    pub(crate) fn budget_inputs(&mut self) -> BudgetInputs {
        BudgetInputs {
            target: self.engine(),
            sources: self.source_key(),
        }
    }

    /// What the open map's budget is of now (BUDGET-3.3).
    fn budget_key(&mut self) -> BudgetKey {
        let map = self
            .map()
            .map_or_default(|document| document.file().to_bytes());
        BudgetKey::of(map, self.budget_inputs())
    }

    /// What a budget of the open map needs (BUDGET-2.1, BUDGET-2.2): the lumps of its last
    /// compile or its BSP, the format, the engines and the spawn.
    fn budget_job(&mut self) -> Result<BudgetJob, WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        let (lumps, format) = match document.source() {
            MapSource::Bsp(bytes) => match Bsp::read(bytes, BspLoader::default()) {
                Ok(bsp) => {
                    let format = match bsp.version {
                        BspVersion::Bsp2Psb | BspVersion::Bsp2 => Written::Bsp2 { lump: None },
                        BspVersion::Bsp29 | BspVersion::Valve => Written::Bsp29,
                    };
                    let mut counts: LumpCounts = bsp.counts().into();
                    // The entities as the editor holds them, which the spawn reads too.
                    counts.entities = document.file().entities.len();
                    counts.entity_text = document.file().to_bytes().len().saturating_add(1);
                    (Ok((counts, LumpSource::Bsp)), Some(format))
                }
                Err(error) => (Err(Unmeasured::Unreadable(error)), None),
            },
            MapSource::Map => match document.report() {
                Some(report) => (Ok((report.lumps, LumpSource::Compile)), Some(report.format)),
                None => (Err(Unmeasured::NotCompiled), None),
            },
        };
        let target = self.engine().unwrap_or(Engine::Vanilla);
        let engines = if target == Engine::Vanilla {
            vec![target]
        } else {
            vec![target, Engine::Vanilla]
        };
        let spawn = self
            .map_spawn()
            .map(|spawn| spawn.with_room(room(&engines)))
            .map_err(|error| error.to_string());
        Ok(BudgetJob {
            lumps,
            format,
            engines,
            spawn,
        })
    }

    /// Keeps `found`, made of the map, target and sources at `key`, while they are still
    /// those (BUDGET-3.3).
    fn keep_budget(&mut self, found: &MapBudget, key: BudgetKey) {
        if self.budget_key() == key {
            self.budget = BudgetFound {
                key,
                found: Some(found.clone()),
                problems: found.problems(),
            };
        }
    }

    /// The open map's budget (BUDGET-1): made at once, or with [`Answer::Later`] its spawns
    /// started on their own thread and `None` answered.
    fn map_budget(&mut self, answer: Answer) -> Result<Option<MapBudget>, WorkbenchError> {
        let job = self.budget_job()?;
        let key = self.budget_key();
        match answer {
            Answer::Now => {
                let found = job.run();
                self.keep_budget(&found, key);
                Ok(Some(found))
            }
            Answer::Later => {
                let (send, done) = mpsc::channel();
                std::thread::spawn(move || {
                    // A run the workbench dropped, for a later one, is waited on by no one.
                    drop(send.send(job.run()));
                });
                self.budget_run = Some(BudgetRun { key, done });
                Ok(None)
            }
        }
    }

    /// The budget started last, once it is made and the map, target and sources are still
    /// those it was made of, kept as [`Answer::Now`] keeps one; else the one kept while they
    /// are the same; else `None`.
    fn budget_found(&mut self) -> Option<MapBudget> {
        if let Some(run) = self.budget_run.take() {
            match run.done.try_recv() {
                Ok(found) => self.keep_budget(&found, run.key),
                Err(TryRecvError::Empty) => {
                    self.budget_run = Some(run);
                    return None;
                }
                Err(TryRecvError::Disconnected) => {}
            }
        }
        let key = self.budget_key();
        self.budget.found.clone().filter(|_| self.budget.key == key)
    }
}

/// `map.budget`.
struct MapBudgetCommand;

impl<C: HasWorkbench + 'static> Command<C> for MapBudgetCommand {
    const ID: &'static str = MAP_BUDGET;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "How much of the target engine's limits the open map uses, \
        and of id's: for each BSP lump, the face extent, edicts, precached models and sounds \
        and light styles, what the map uses, the engine's limit with what the engine does past \
        it, the share and whether it is past, near or within, worst first. The lumps come from \
        the last compile while the map is unchanged; the edicts, precaches and light styles \
        from the map spawned by the project's program on each skill. Limits past and near the \
        target's join map.issues while the map, the target and the sources are the same. With \
        answer \"later\" it answers null at once and spawns on its own thread; \
        map.budget_found gives the answer.";
    type Params = BudgetParams;
    type Output = Option<MapBudget>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BudgetParams,
    ) -> Result<Outcome<Option<MapBudget>, C>, WorkbenchError> {
        let budget = context.workbench().map_budget(params.answer)?;
        Ok(Outcome::outside_history(budget))
    }
}

/// `map.budget_found`.
struct MapBudgetFound;

impl<C: HasWorkbench + 'static> Command<C> for MapBudgetFound {
    const ID: &'static str = MAP_BUDGET_FOUND;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Answers what map.budget found of the open map as it is now: \
        the answer of a budget started with answer \"later\" once it is made, or the last \
        answer while the map, the target and the sources have not changed since; null while \
        one is being made, and when there is none.";
    type Params = NoParams;
    type Output = Option<MapBudget>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _: NoParams,
    ) -> Result<Outcome<Option<MapBudget>, C>, WorkbenchError> {
        Ok(Outcome::outside_history(context.workbench().budget_found()))
    }
}

/// Registers `map.budget` and `map.budget_found`.
///
/// # Errors
///
/// [`RegisterError`] when one is already registered.
pub(crate) fn register_map_budget<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapBudgetCommand)?;
    registry.register(MapBudgetFound)
}

#[cfg(test)]
mod tests;
