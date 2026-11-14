// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask map-diff [--maps <dir>] [--map <name>]... [--traces <n>]`: the map compiler
//! differential (P6-07, MAP-11, MAP-19). Each corpus map is compiled by qs-map-bsp and by an
//! oracle, id's qbsp for a map it reads and ericw-tools' qbsp for the rest (a Valve 220 map, or
//! one with `misc_external_map` instances, compiled by qs-map-bsp in the extended dialect), and
//! the two BSPs compared: whether the map leaks, the entity lump's entities, the faces' count
//! within a tolerance, and traces through each clipping hull of the world, which must agree.
//!
//! The maps are the corpus's (MAP-19), fetched into the cache as `cargo xtask corpus fetch`
//! does, or those under `--maps`, and the fixtures in `oracles/maps/`. The oracles are built as
//! `cargo xtask oracles maps` builds them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use qs_bsp::{Bsp, BspLoader};
use qs_map::qbsp;
use qs_map_bsp::{Compiler, Dialect, Warning};
use qs_sim::collision::{Collision, Hull, Trace, recursive_hull_check};

use crate::corpus::{self, manifest::Manifest};
use crate::download::Net;
use crate::exec::Exec;
use crate::oracles::OUT;
use crate::oracles::maps::{MapTools, WORK, build};

/// `cargo xtask map-diff`.
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The folder of maps; when not given, the corpus's maps (MAP-19), fetched into the cache's
    /// `map` folder.
    #[arg(long)]
    maps: Option<PathBuf>,
    /// Only the maps of these names, without `.map`; every one when none is given.
    #[arg(long = "map")]
    only: Vec<String>,
    /// How many traces each clipping hull of each map takes.
    #[arg(long, default_value_t = DEFAULT_TRACES)]
    traces: usize,
}

/// The corpus manifest, whose maps are compared (MAP-19).
const MANIFEST: &str = "corpus/manifest.toml";
/// The traces each hull takes unless `--traces` says otherwise.
const DEFAULT_TRACES: usize = 2000;
/// The fixtures compiled with the corpus: the oracles' room and the instance map (P6-23).
const FIXTURES: [&str; 2] = [
    "oracles/maps/room.map",
    "oracles/maps/instances/instances.map",
];
/// The longest an oracle may take on one map.
const ORACLE_LIMIT: Duration = Duration::from_secs(600);
/// The world's clipping hulls traced: point, player and shambler (BSP-7).
const HULLS: usize = 3;
/// How far apart two traces' ends may be and still agree, in units. ericw-tools reads a face's
/// plane in double precision where id's qbsp, and so qs-map-bsp, reads it in single (QBSP-2),
/// so a plane of an angled brush lies up to a fraction of a unit from the other's, and at a
/// corner the two may stop on different planes a few hundredths apart.
const END_TOLERANCE: f32 = 0.25;
/// How far a disagreeing trace is moved, along each axis both ways, to tell a difference in
/// what is solid from one at an edge: a trace that only grazes an edge agrees once moved, one
/// through a brush only one map has disagrees however it is moved.
const NUDGE: f32 = 0.25;
/// The keys naming a world's WADs, which neither collision nor leaks read; id's qbsp stops on a
/// WAD it cannot open, so both compilers are given the map without them.
const WAD_KEYS: [&[u8]; 2] = [b"wad", b"_wad"];
/// How far from the oracle's the faces' count may be, as a share of it: the two merge and
/// split faces each their own way (QBSP-10), which neither collision nor leaks see.
const FACE_TOLERANCE: f64 = 0.25;
/// How many faces either way are within tolerance however small the map, where one brush cut
/// another way is a large share.
const FACE_SLACK: f64 = 8.0;
/// What the extended dialect makes of a `func_detail_illusionary`, an entity ericw-tools leaves
/// in the world (QBSP-14.8).
const ILLUSIONARY: &str = "func_illusionary";
/// The classname QBSP-14.8 turns into [`ILLUSIONARY`].
const DETAIL_ILLUSIONARY: &str = "func_detail_illusionary";
/// The classname of an instance of an external map (MAP-14).
const EXTERNAL_MAP: &str = "misc_external_map";

/// Which oracle compiles a map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Oracle {
    /// id's qbsp, for a map it reads.
    Id,
    /// ericw-tools' qbsp, for a Valve 220 map or one with instances.
    EricwTools,
}

impl Oracle {
    /// The oracles of the map `text`, each with the dialect qs-map-bsp compiles it in: id's
    /// qbsp in id's dialect for a map it reads with no instances, and ericw-tools' qbsp in the
    /// extended dialect for every map.
    fn of(text: &[u8]) -> Vec<(Self, Dialect)> {
        let instances = String::from_utf8_lossy(text).contains(EXTERNAL_MAP);
        let mut out = Vec::new();
        if !instances && qbsp::load_map_file(text).is_ok() {
            out.push((Self::Id, Dialect::Id));
        }
        out.push((Self::EricwTools, Dialect::Extended));
        out
    }

    /// Its qbsp among `tools`.
    fn qbsp(self, tools: &MapTools) -> &Path {
        let [qbsp, _, _] = match self {
            Self::Id => &tools.id,
            Self::EricwTools => &tools.ericw,
        };
        qbsp
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Id => "qbsp",
            Self::EricwTools => "ericw-qbsp",
        }
    }
}

/// One compile of a map: its BSP and whether it leaked.
struct Compiled {
    bsp: Vec<u8>,
    leaks: bool,
}

/// What comparing two compiles of a map found.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Comparison {
    /// Whether each leaked: ours, the oracle's.
    pub(crate) leaks: (bool, bool),
    /// Each entity lump's classnames, in order, agree.
    pub(crate) entities_agree: bool,
    /// Each one's faces: ours, the oracle's.
    pub(crate) faces: (usize, usize),
    /// The traces each hull took and those that disagreed.
    pub(crate) traces: [(usize, usize); HULLS],
}

impl std::fmt::Display for Comparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            leaks,
            entities_agree,
            faces,
            traces,
        } = self;
        let entities = if *entities_agree { "agree" } else { "differ" };
        write!(
            f,
            "leaks {} / {}, entities {entities}, faces {} / {}, traces differing by hull",
            leaks.0, leaks.1, faces.0, faces.1
        )?;
        for (hull, (taken, differ)) in traces.iter().enumerate() {
            write!(f, " {hull}: {differ} of {taken}")?;
        }
        Ok(())
    }
}

impl Comparison {
    /// Whether the two agree: the same leak, the same entities, faces within
    /// [`FACE_TOLERANCE`] or [`FACE_SLACK`] and every trace the same.
    pub(crate) fn agrees(&self) -> bool {
        let (ours, theirs) = self.faces;
        let faces = f64::from(u32::try_from(ours).unwrap_or(u32::MAX));
        let oracle = f64::from(u32::try_from(theirs).unwrap_or(u32::MAX));
        self.leaks.0 == self.leaks.1
            && self.entities_agree
            && (faces - oracle).abs() <= (oracle * FACE_TOLERANCE).max(FACE_SLACK)
            && self.traces.iter().all(|&(_, differ)| differ == 0)
    }
}

/// `cargo xtask map-diff`; whether every map agrees.
pub(crate) fn main(
    root: &Path,
    exec: &dyn Exec,
    net: &dyn Net,
    args: &Args,
) -> Result<bool, String> {
    let corpus = match &args.maps {
        Some(dir) => dir.clone(),
        None => {
            let manifest = Manifest::load(&root.join(MANIFEST))?;
            let ids = manifest.map.into_iter().map(|entry| entry.id).collect();
            if !corpus::main(root, &corpus::Args::fetch(ids))? {
                return Err("the corpus's maps could not be fetched".to_owned());
            }
            corpus::cache_dest(root, None)?.join("map")
        }
    };
    let mut maps: Vec<PathBuf> = FIXTURES.iter().map(|map| root.join(map)).collect();
    let mut found: Vec<PathBuf> = fs::read_dir(&corpus)
        .map_err(|e| format!("{}: {e}", corpus.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "map"))
        .collect();
    found.sort();
    maps.extend(found);
    let name = |path: &Path| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    if !args.only.is_empty() {
        maps.retain(|path| args.only.contains(&name(path)));
    }
    let tools = build(root, exec, net)?;
    let work = WORK
        .iter()
        .fold(root.join(OUT), |path, part| path.join(part));
    let mut passed = true;
    for path in &maps {
        let text = without_wads(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for (oracle, dialect) in Oracle::of(&text) {
            let dir = work.join("map-diff").join(oracle.name()).join(name(path));
            let theirs = oracle_compile(exec, oracle.qbsp(&tools), path, &text, &dir)?;
            let ours = our_compile(&text, dialect, path);
            // Ours beside the oracle's, to look at when they differ.
            if let Ok(compiled) = &ours {
                let file = dir.join("ours.bsp");
                fs::write(&file, &compiled.bsp).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            let (agrees, said) = match (ours, theirs) {
                (Ok(ours), Ok(theirs)) => {
                    let illusionaries = match dialect {
                        Dialect::Id => 0,
                        Dialect::Extended => count_classname(&text, DETAIL_ILLUSIONARY),
                    };
                    let comparison = compare(&ours, &theirs, args.traces, illusionaries)?;
                    (comparison.agrees(), comparison.to_string())
                }
                (Err(ours), Err(theirs)) => (true, format!("both stop: {ours}; {theirs}")),
                (Err(ours), Ok(_)) => (false, format!("only ours stops: {ours}")),
                (Ok(_), Err(theirs)) => (false, format!("only the oracle stops: {theirs}")),
            };
            passed &= agrees;
            let verdict = if agrees { "ok   " } else { "FAIL " };
            println!("{verdict} {} ({}): {said}", name(path), oracle.name());
        }
    }
    Ok(passed)
}

/// The map `text` of `path` compiled by `qbsp` in `dir`, emptied first, with the files beside
/// the map (its external maps) copied in; the oracle's last line when it stops.
fn oracle_compile(
    exec: &dyn Exec,
    qbsp: &Path,
    path: &Path,
    text: &[u8],
    dir: &Path,
) -> Result<Result<Compiled, String>, String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let from = path.parent().unwrap_or(Path::new("."));
    let beside = fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in beside.filter_map(Result::ok) {
        let file = entry.path();
        if file.extension().is_some_and(|ext| ext == "map") && file.is_file() {
            let to = dir.join(entry.file_name());
            fs::copy(&file, &to).map_err(|e| format!("{}: {e}", file.display()))?;
        }
    }
    let map = dir.join(path.file_name().unwrap_or_default());
    fs::write(&map, text).map_err(|e| format!("{}: {e}", map.display()))?;
    let mut command = std::process::Command::new(qbsp);
    command.arg(&map).current_dir(dir);
    let output = exec
        .output_within(&mut command, ORACLE_LIMIT)
        .map_err(|e| format!("cannot run {}: {e}", qbsp.display()))?
        .ok_or_else(|| {
            format!(
                "{} took over {ORACLE_LIMIT:?} on {}",
                qbsp.display(),
                map.display()
            )
        })?;
    let bsp_path = map.with_extension("bsp");
    // An oracle that stops writes no BSP, or one it began before it stopped.
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stdout);
        let why = said
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no message");
        return Ok(Err(format!("{}: {why}", output.status)));
    }
    let bsp = fs::read(&bsp_path).map_err(|e| format!("{}: {e}", bsp_path.display()))?;
    // id's qbsp opens the leak file for hull 0 whenever it fills the outside and writes points
    // only when the fill reaches an occupant (qutils `outside.c`, `FillOutside`): a leak is a
    // leak file with something in it.
    let leaks = fs::metadata(map.with_extension("pts")).is_ok_and(|file| file.len() > 0);
    Ok(Ok(Compiled { bsp, leaks }))
}

/// The map `text` with its world's WAD keys ([`WAD_KEYS`]) taken out.
fn without_wads(text: &[u8]) -> Result<Vec<u8>, qs_map::MapError> {
    let mut file = qs_map::MapFile::parse(text)?;
    if let Some(world) = file.entities.first_mut() {
        for key in WAD_KEYS {
            world.remove(key);
        }
    }
    Ok(file.to_bytes())
}

/// The map `text` at `path` compiled by qs-map-bsp in `dialect`, its external maps read from
/// beside it.
fn our_compile(text: &[u8], dialect: Dialect, path: &Path) -> Result<Compiled, String> {
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut load = |name: &str| fs::read(dir.join(name)).ok();
    let compiled = Compiler::default()
        .with_dialect(dialect)
        .compile_with_externals(text, None, &mut load)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let leaks = compiled.warnings.iter().any(|w| matches!(w, Warning::Leak));
    Ok(Compiled {
        bsp: compiled.bsp,
        leaks,
    })
}

/// A small generator of the traces' points, the same on every run and machine.
struct Points(u64);

impl Points {
    /// The next number from 0 to 1, from a 64-bit linear congruential generator (Knuth's
    /// MMIX constants).
    fn next(&mut self) -> f32 {
        const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
        const INCREMENT: u64 = 1_442_695_040_888_963_407;
        const TOP_BITS: u32 = 40;
        const SCALE: f32 = 16_777_216.0;
        self.0 = self.0.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        let high = u32::try_from(self.0 >> TOP_BITS).unwrap_or(0);
        #[expect(clippy::cast_precision_loss, reason = "24 bits fit an f32 exactly")]
        let unit = high as f32 / SCALE;
        unit
    }

    /// A point within `mins` to `maxs`.
    fn within(&mut self, mins: [f32; 3], maxs: [f32; 3]) -> [f32; 3] {
        let mut p = [0.0; 3];
        for ((p, lo), hi) in p.iter_mut().zip(mins).zip(maxs) {
            *p = lo + (hi - lo) * self.next();
        }
        p
    }
}

/// Whether the trace `line` through hulls `a` and `b` agrees: both start in solid, or neither
/// does and both are all solid or not and end within [`END_TOLERANCE`]. A trace that starts in
/// solid is `allsolid` by which leaves the tree's splits lead it through, not by the solids
/// themselves (WORLD-2, id's `SV_RecursiveHullCheck`), so for it only `startsolid` is compared.
fn agree(a: &Hull<'_>, b: &Hull<'_>, line: ([f32; 3], [f32; 3])) -> bool {
    let run = |hull: &Hull<'_>| {
        let mut trace = Trace::new(line.1);
        recursive_hull_check(hull, hull.first, line, &mut trace).map(|_| trace)
    };
    match (run(a), run(b)) {
        (Ok(x), Ok(y)) => {
            let apart = x
                .endpos
                .iter()
                .zip(y.endpos)
                .map(|(p, q)| (p - q) * (p - q))
                .sum::<f32>()
                .sqrt();
            x.startsolid == y.startsolid
                && (x.startsolid || (x.allsolid == y.allsolid && apart <= END_TOLERANCE))
        }
        (Err(_), Err(_)) => true,
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => false,
    }
}

/// `line` moved by [`NUDGE`] along each axis, both ways.
fn nudged((from, to): ([f32; 3], [f32; 3])) -> Vec<([f32; 3], [f32; 3])> {
    let mut out = Vec::with_capacity(6);
    for axis in 0..3 {
        for way in [NUDGE, -NUDGE] {
            let moved = |mut p: [f32; 3]| {
                if let Some(c) = p.get_mut(axis) {
                    *c += way;
                }
                p
            };
            out.push((moved(from), moved(to)));
        }
    }
    out
}

/// The seed of every map's traces.
const SEED: u64 = 0x5155_414b;

/// How many entities of the map `text` have the classname `name`, in any case.
fn count_classname(text: &[u8], name: &str) -> usize {
    qs_map::MapFile::parse(text).map_or(0, |file| {
        file.entities
            .iter()
            .filter(|entity| {
                entity
                    .classname()
                    .is_some_and(|classname| classname.eq_ignore_ascii_case(name.as_bytes()))
            })
            .count()
    })
}

/// The two compiles compared; ours has `illusionaries` more [`ILLUSIONARY`] entities, which the
/// extended dialect makes where ericw-tools merges (QBSP-14.8).
fn compare(
    ours: &Compiled,
    theirs: &Compiled,
    traces: usize,
    illusionaries: usize,
) -> Result<Comparison, String> {
    let a = Bsp::read(&ours.bsp, BspLoader::FitzQuake).map_err(|e| format!("our BSP: {e}"))?;
    let b = Bsp::read(&theirs.bsp, BspLoader::FitzQuake)
        .map_err(|e| format!("the oracle's BSP: {e}"))?;
    let classnames = |bsp: &Bsp<'_>| -> Vec<String> {
        String::from_utf8_lossy(bsp.entities())
            .split('{')
            .filter_map(|block| {
                let mut words = block.split('"').skip(1).step_by(2);
                std::iter::from_fn(|| Some((words.next()?, words.next()?)))
                    .find(|(key, _)| *key == "classname")
                    .map(|(_, value)| value.to_owned())
            })
            .collect()
    };
    let (world_a, world_b) = (
        a.models.first().ok_or("our BSP has no world")?,
        b.models.first().ok_or("the oracle's BSP has no world")?,
    );
    let mut mins = world_a.mins;
    let mut maxs = world_a.maxs;
    for (m, other) in mins.iter_mut().zip(world_b.mins) {
        *m = m.min(other);
    }
    for (m, other) in maxs.iter_mut().zip(world_b.maxs) {
        *m = m.max(other);
    }
    let (ca, cb) = (Collision::new(&a), Collision::new(&b));
    let heads = |c: &Collision| c.model(1.0).map(|model| model.heads);
    let (ha, hb) = (
        heads(&ca).ok_or("our BSP has no world model")?,
        heads(&cb).ok_or("the oracle's BSP has no world model")?,
    );
    let mut points = Points(SEED);
    let mut counts = [(0_usize, 0_usize); HULLS];
    for (hull, count) in counts.iter_mut().enumerate() {
        let (Some(set_a), Some(set_b)) = (ca.sets().first(), cb.sets().first()) else {
            return Err("a BSP has no hulls".to_owned());
        };
        let (hull_a, hull_b) = (set_a.hull(hull, ha), set_b.hull(hull, hb));
        for _ in 0..traces {
            let line = (points.within(mins, maxs), points.within(mins, maxs));
            count.0 = count.0.saturating_add(1);
            let differs = |line: ([f32; 3], [f32; 3])| !agree(&hull_a, &hull_b, line);
            if differs(line) && nudged(line).into_iter().all(differs) {
                count.1 = count.1.saturating_add(1);
            }
        }
    }
    Ok(Comparison {
        leaks: (ours.leaks, theirs.leaks),
        entities_agree: {
            let (mine, oracle) = (classnames(&a), classnames(&b));
            let count = |names: &[String]| names.iter().filter(|n| *n == ILLUSIONARY).count();
            let others = |names: Vec<String>| -> Vec<String> {
                names.into_iter().filter(|n| n != ILLUSIONARY).collect()
            };
            count(&mine) == count(&oracle).saturating_add(illusionaries)
                && others(mine) == others(oracle)
        },
        faces: (a.faces.len(), b.faces.len()),
        traces: counts,
    })
}
