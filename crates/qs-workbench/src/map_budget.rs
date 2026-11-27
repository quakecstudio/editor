// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.budget` (L-79, MAP-38): how much of the target engine's limits the open map uses, and
//! of id's. The lumps and the face extent come from the last compile's report, or from the BSP
//! the map was opened from; the edicts, precaches and light styles from the map spawned in
//! `qs-sim` on each skill by the project's own program, with room past id's limits so a map
//! past them is counted. Limits passed and nearly reached join `map.issues` until the map
//! changes.
//!
//! Implements `docs/spec/budget.md` BUDGET-1 to BUDGET-3; the limits are ENGT-5's.

use qc_builtins::{Precache, PrecacheRoom};
use qc_edict::EdictCapacity;
use qc_engines::{Bound, Engine, LimitFact, MapFormat, Measure};

pub use qc_engines::{Bsp2, MapLimit, Past};
use qs_bsp::{Bsp, BspLoader, BspVersion};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use qs_map_edit::{EntityProblem, LumpCounts, MapSource, Written};
use qs_sim::Room;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_balance::SKILLS;
use crate::map_filter::{GameMode, Skill};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.budget`.
pub const MAP_BUDGET: &str = "map.budget";

/// The share of a bound at which a limit is near (BUDGET-1.3).
pub const BUDGET_NEAR_SHARE: f64 = 0.9;

/// Percent in one.
const PERCENT: f64 = 100.0;

/// Why the lumps are not measured before a compile (BUDGET-2.1).
const NOT_COMPILED: &str = "the map has not been compiled since it changed";
/// Why lighting and visibility are not measured from a compile (BUDGET-2.1).
const NOT_LIT: &str = "a compile neither lights nor vises the map";

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
    /// Not measured.
    NotMeasured {
        /// Why.
        why: String,
    },
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
    /// The engine's id, such as `quakespasm` (ENGT-3.4).
    pub engine: String,
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

/// The issues `map.budget` last found, for `map.issues` (BUDGET-3.3).
#[derive(Clone, Debug, Default)]
pub(crate) struct BudgetFound {
    map: Vec<u8>,
    problems: Vec<EntityProblem>,
}

impl BudgetFound {
    /// The problems, while the map's text is still `map`.
    pub(crate) fn problems(&self, map: &[u8]) -> Vec<EntityProblem> {
        if self.map == map {
            self.problems.clone()
        } else {
            Vec::new()
        }
    }
}

/// What a spawn of the map uses (BUDGET-2.3), each the most of the skills with its skill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Spawned {
    edicts: (usize, Option<Skill>),
    models: (usize, Option<Skill>),
    sounds: (usize, Option<Skill>),
    styles: (usize, Option<Skill>),
}

impl Spawned {
    /// Takes `count` at `skill` into `slot` when it is more.
    fn most(slot: &mut (usize, Option<Skill>), count: usize, skill: Skill) {
        if slot.1.is_none() || count > slot.0 {
            *slot = (count, Some(skill));
        }
    }

    /// Its use of a spawn-measured `limit`.
    const fn of(&self, limit: MapLimit) -> Option<(usize, Option<Skill>)> {
        match limit {
            MapLimit::Edicts => Some(self.edicts),
            MapLimit::ModelPrecaches => Some(self.models),
            MapLimit::SoundPrecaches => Some(self.sounds),
            MapLimit::LightStyles => Some(self.styles),
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

/// The lumps and where they came from, or why they are not measured.
type Lumps = Result<(LumpCounts, LumpSource), String>;

/// What the map uses, measured once and read for each engine.
struct Measured {
    lumps: Lumps,
    format: MapFormat,
    spawned: Result<Spawned, String>,
}

impl Measured {
    /// What the map uses of `limit`, for `engine` (BUDGET-2.1, BUDGET-2.3).
    fn used(&self, limit: MapLimit, engine: Engine) -> Used {
        let count = |count| Used::Count { count, skill: None };
        match limit.measured_by() {
            Measure::Spawn => match &self.spawned {
                Ok(spawned) => spawned
                    .of(limit)
                    .map_or_else(|| count(0), |(count, skill)| Used::Count { count, skill }),
                Err(why) => Used::NotMeasured { why: why.clone() },
            },
            Measure::Compile => {
                let (lumps, source) = match &self.lumps {
                    Ok(found) => *found,
                    Err(why) => return Used::NotMeasured { why: why.clone() },
                };
                let unlit = || Used::NotMeasured {
                    why: NOT_LIT.to_owned(),
                };
                match limit {
                    MapLimit::Lighting if source == LumpSource::Compile => unlit(),
                    MapLimit::Visibility if source == LumpSource::Compile => unlit(),
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
    let count = match &used {
        Used::Count { count, .. } => Some(*count),
        Used::NotMeasured { .. } => None,
    };
    let (share, standing) = match (bound, count) {
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
        engine: engine.id().to_owned(),
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

/// The issues of the target engine's budget (BUDGET-3.1, BUDGET-3.2).
fn issues(target: &EngineBudget, engine: Engine, format: Option<&Written>) -> Vec<String> {
    let mut out = Vec::new();
    for line in &target.lines {
        let (Used::Count { count, .. }, LineBound::At { most, past, .. }) =
            (&line.used, &line.bound)
        else {
            continue;
        };
        match line.standing {
            Standing::Past => out.push(format!(
                "{count} {} is past {}'s limit of {most}: {}",
                line.name,
                engine.name(),
                consequence(engine, *past)
            )),
            Standing::Near => {
                let percent = (line.share.unwrap_or(0.0) * PERCENT).floor();
                out.push(format!(
                    "{count} {} is {percent}% of {}'s limit of {most}",
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
        // A BSP opened as BSP2 names no lump.
        out.push(if lump.is_empty() {
            format!("the map is BSP2, {reads}")
        } else {
            format!("the {lump} are too many for BSP29, so the map is compiled as BSP2, {reads}")
        });
    }
    out
}

/// The room a spawn needs past every bound of `engines` (BUDGET-2.4).
fn room(engines: &[Engine]) -> Room {
    let largest = |limit: MapLimit| {
        engines
            .iter()
            .chain(&[Engine::Vanilla])
            .filter_map(|engine| match engine.bound(limit, MapFormat::Bsp2) {
                Bound::At(fact) => Some(fact.most),
                Bound::Unlimited | Bound::Unknown => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    };
    Room {
        edicts: EdictCapacity::at_least_ids(largest(MapLimit::Edicts)),
        // The lists' entries are the names and the empty entry 0.
        precaches: Some(PrecacheRoom {
            models: largest(MapLimit::ModelPrecaches).saturating_add(1),
            sounds: largest(MapLimit::SoundPrecaches).saturating_add(1),
        }),
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The lumps of the open map (BUDGET-2.1) and its format (BUDGET-2.2).
    fn budget_lumps(&self) -> Result<(Lumps, Option<Written>), WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        Ok(match document.source() {
            MapSource::Bsp(bytes) => match Bsp::read(bytes, BspLoader::default()) {
                Ok(bsp) => {
                    let format = match bsp.version {
                        BspVersion::Bsp2Psb | BspVersion::Bsp2 => Written::Bsp2 {
                            lump: String::new(),
                        },
                        BspVersion::Bsp29 | BspVersion::Valve => Written::Bsp29,
                    };
                    (Ok((bsp.counts().into(), LumpSource::Bsp)), Some(format))
                }
                Err(error) => (Err(error.to_string()), None),
            },
            MapSource::Map => match document.report() {
                Some(report) => (
                    Ok((report.lumps, LumpSource::Compile)),
                    Some(report.format.clone()),
                ),
                None => (Err(NOT_COMPILED.to_owned()), None),
            },
        })
    }

    /// What the open map spawns on each skill, with room past `engines`' bounds (BUDGET-2.3 to
    /// BUDGET-2.5).
    fn budget_spawn(&mut self, engines: &[Engine]) -> Result<Spawned, String> {
        let spawn = self
            .map_spawn()
            .map_err(|error| error.to_string())?
            .with_room(room(engines));
        let mut spawned = Spawned::default();
        for skill in SKILLS {
            spawn
                .world(skill, GameMode::Single, |progs, mut world| {
                    world.settle(progs).map_err(|error| spawn.failed(error))?;
                    Spawned::most(&mut spawned.edicts, world.num_edicts(), skill);
                    Spawned::most(&mut spawned.models, world.precached(Precache::Model), skill);
                    Spawned::most(&mut spawned.sounds, world.precached(Precache::Sound), skill);
                    Spawned::most(&mut spawned.styles, world.light_styles_set(), skill);
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
        }
        Ok(spawned)
    }

    /// The open map's budget (BUDGET-1), keeping its issues for `map.issues` (BUDGET-3.3).
    fn map_budget(&mut self) -> Result<MapBudget, WorkbenchError> {
        let (lumps, format) = self.budget_lumps()?;
        let target = self.engine().unwrap_or(Engine::Vanilla);
        let engines: Vec<Engine> = if target == Engine::Vanilla {
            vec![target]
        } else {
            vec![target, Engine::Vanilla]
        };
        let map_format = match format {
            Some(Written::Bsp2 { .. }) => MapFormat::Bsp2,
            Some(Written::Bsp29) | None => MapFormat::Bsp29,
        };
        let measured = Measured {
            lumps,
            format: map_format,
            spawned: self.budget_spawn(&engines),
        };
        let budgets: Vec<EngineBudget> = engines
            .iter()
            .map(|&engine| engine_budget(engine, &measured))
            .collect();
        if let (Some(document), Some(first)) = (self.map(), budgets.first()) {
            let problems = issues(first, target, format.as_ref())
                .into_iter()
                .map(|message| EntityProblem {
                    entity: None,
                    line: 0,
                    message,
                    fix: None,
                    point: None,
                })
                .collect();
            self.budget = BudgetFound {
                map: document.file().to_bytes(),
                problems,
            };
        }
        Ok(MapBudget {
            engines: budgets,
            format,
        })
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
        target's join map.issues until the map changes.";
    type Params = NoParams;
    type Output = MapBudget;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapBudget, C>, WorkbenchError> {
        let budget = context.workbench().map_budget()?;
        Ok(Outcome::outside_history(budget))
    }
}

/// Registers `map.budget`.
///
/// # Errors
///
/// [`RegisterError`] when it is already registered.
pub(crate) fn register_map_budget<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapBudgetCommand)
}

#[cfg(test)]
mod tests;
