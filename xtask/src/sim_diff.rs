// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask sim-diff <route> [-- <engine word>...]`: the server simulation differential
//! (P3-25, DIFF-16). The route plays through the engine oracle, id's server, and through
//! `qs-sim`; the state after the map loads and after each server frame must be the same in both,
//! every named global and every named field of every edict read by its type (DIFF-16.2).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_progs::Progs;
use qc_trace::{Record, Trace};
use qc_types::Slot;
use qs_bsp::{Bsp, BspLoader};
use qs_sim::{LoadOptions, NamedValue, QcValue, RawState, State, UserCmd, World, read_state};
use qs_vfs::{LoadedFile, StdFs, Vfs};

use crate::oracles;

/// `cargo xtask sim-diff <route> [-- <engine word>...]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// The route, in the engine oracle's script form (`oracles/README.md`).
    route: PathBuf,
    /// Play the route on this map in place of its `map` line; repeated, on each in turn.
    #[arg(long = "map")]
    maps: Vec<String>,
    /// The engine's command line, after `--`: `-basedir <dir>` and the like.
    #[arg(last = true)]
    engine: Vec<String>,
}

/// Plays the route through both, on each map `--map` names or its own, and compares every
/// frame; returns whether they agreed on every map.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    let text =
        fs::read_to_string(&args.route).map_err(|e| format!("{}: {e}", args.route.display()))?;
    let route = Route::parse(&text)?;
    let built = oracles::build(root)?;
    let out = root.join("target/sim-diff");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    if args.maps.is_empty() {
        return compare(&built, &args.route, &route, &out, &args.engine);
    }
    let mut passed = true;
    for map in &args.maps {
        // DIFF-16.5: the route on another map is the same script with that map line.
        let script = out.join(format!("{map}.route"));
        let replaced = on_map(&text, map);
        fs::write(&script, replaced).map_err(|e| format!("{}: {e}", script.display()))?;
        let rerouted = Route {
            map: map.clone(),
            ..route.clone()
        };
        passed &= compare(&built, &script, &rerouted, &out, &args.engine)?;
    }
    Ok(passed)
}

/// The route `text` with its `map` lines naming `map` (DIFF-16.5).
fn on_map(text: &str, map: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("map ") {
            out.push_str("map ");
            out.push_str(map);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Plays `route`, the script at `script`, through both and compares every frame.
fn compare(
    built: &oracles::Oracles<'_>,
    script: &Path,
    route: &Route,
    out: &Path,
    engine_words: &[String],
) -> Result<bool, String> {
    let stem = script
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("route");
    let trace_path = out.join(format!("{stem}.trace"));
    oracles::engine_trace(built, script, &trace_path, engine_words)?;
    let bytes = fs::read(&trace_path).map_err(|e| format!("{}: {e}", trace_path.display()))?;
    let trace = qc_trace::decode(&bytes).map_err(|e| format!("{}: {e:?}", trace_path.display()))?;
    let mut engine = route.engine.clone();
    engine.extend(engine_words.iter().cloned());
    let game = Game::load(&engine, &route.map)?;
    let progs = Progs::read(&game.progs).map_err(|e| format!("progs.dat: {e}"))?;
    let mut oracle = OracleFrames::new(&progs, &trace)?;
    let mut failure = None;
    let frames = game.play(&progs, route, |frame, mine| {
        let Some(theirs) = oracle.next_state()? else {
            failure = Some(format!(
                "the oracle ran {} frames, qs-sim more",
                frame.saturating_sub(1)
            ));
            return Ok(false);
        };
        let found = differences(&theirs, mine);
        if found.is_empty() {
            return Ok(true);
        }
        failure = Some(format!("frame {frame}:\n      {}", found.join("\n      ")));
        Ok(false)
    })?;
    if failure.is_none() && oracle.next_state()?.is_some() {
        failure = Some(format!("qs-sim ran {frames} frames, the oracle more"));
    }
    if let Some(failure) = failure {
        println!("FAIL  sim-diff {}: {failure}", script.display());
        return Ok(false);
    }
    println!(
        "ok    sim-diff {}: {frames} frames the same in id's server and qs-sim",
        script.display()
    );
    Ok(true)
}

/// A route as DIFF-16.1 plays it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Route {
    engine: Vec<String>,
    map: String,
    frametime: f64,
    steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// A client string command, read in the next frame.
    Command(String),
    /// The move sent each frame from now on.
    Move(UserCmd),
    /// Frames to run.
    Frames(u32),
}

impl Route {
    /// Reads a route as the engine oracle's harness does (`oracles/engine/engine.c`, `main`).
    fn parse(text: &str) -> Result<Self, String> {
        let mut route = Self {
            frametime: 1.0 / 72.0,
            ..Self::default()
        };
        for (number, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            let at = number.saturating_add(1);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            match word {
                "engine" => route.engine.push(rest.to_owned()),
                "map" => rest.clone_into(&mut route.map),
                "frametime" => {
                    route.frametime = rest
                        .parse()
                        .ok()
                        .filter(|t: &f64| *t > 0.0 && *t <= 1.0)
                        .ok_or(format!("line {at}: frametime is above 0, at most 1"))?;
                }
                "cmd" => route.steps.push(Step::Command(rest.to_owned())),
                "move" => route.steps.push(Step::Move(parse_move(rest, at)?)),
                "frames" => route.steps.push(Step::Frames(
                    rest.parse()
                        .ok()
                        .filter(|n| *n >= 1)
                        .ok_or(format!("line {at}: frames is a count from 1"))?,
                )),
                _ => return Err(format!("line {at}: unknown line")),
            }
        }
        if route.map.is_empty() {
            return Err("no map line".to_owned());
        }
        Ok(route)
    }
}

/// `move <pitch> <yaw> <roll> <forward> <side> <up> <buttons> <impulse>`.
fn parse_move(text: &str, at: usize) -> Result<UserCmd, String> {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let bad = || format!("line {at}: move is pitch yaw roll forward side up buttons impulse");
    let [pitch, yaw, roll, forward, side, up, buttons, impulse] = words[..] else {
        return Err(bad());
    };
    let angle = |w: &str| w.parse::<f32>().map_err(|_| bad());
    let short = |w: &str| w.parse::<i16>().map(f32::from).map_err(|_| bad());
    let byte = |w: &str| w.parse::<u8>().map_err(|_| bad());
    Ok(UserCmd {
        angles: [angle(pitch)?, angle(yaw)?, angle(roll)?],
        forward: short(forward)?,
        side: short(side)?,
        up: short(up)?,
        buttons: byte(buttons)?,
        impulse: byte(impulse)?,
    })
}

/// The game as `qs-sim` plays it: the progs and the map from the game directory.
struct Game {
    /// The engine's command line, the route's `engine` words and the command line's.
    engine: Vec<String>,
    progs: Vec<u8>,
    vfs: Vfs<StdFs>,
    map: String,
    bsp_bytes: Vec<u8>,
}

impl Game {
    fn load(engine: &[String], map: &str) -> Result<Self, String> {
        let vfs = Vfs::new(StdFs, engine, ".").map_err(|e| format!("{e:?}"))?;
        let read = |name: &str| -> Result<Vec<u8>, String> {
            vfs.load(name.as_bytes())
                .map_err(|e| format!("{name}: {e:?}"))?
                .map(LoadedFile::into_bytes)
                .ok_or_else(|| format!("{name} is not on the search path"))
        };
        let progs = read("progs.dat")?;
        let bsp_bytes = read(&format!("maps/{map}.bsp"))?;
        Ok(Self {
            engine: engine.to_vec(),
            progs,
            vfs,
            map: map.to_owned(),
            bsp_bytes,
        })
    }

    /// Plays `route`, giving `visit` the state after the map loads and after each frame
    /// (DIFF-16.1); stops when `visit` says to, and returns the frames visited.
    fn play(
        &self,
        progs: &Progs<'_>,
        route: &Route,
        mut visit: impl FnMut(usize, &State) -> Result<bool, String>,
    ) -> Result<usize, String> {
        let bsp =
            Bsp::read(&self.bsp_bytes, BspLoader::default()).map_err(|e| e.message(&self.map))?;
        // One search path, as the preview host keeps one.
        let files = Vfs::new(StdFs, &self.engine, ".").map_err(|e| format!("{e:?}"))?;
        let reader = Box::new(move |name: &str| {
            files
                .load(name.as_bytes())
                .ok()
                .flatten()
                .map(LoadedFile::into_bytes)
        });
        // DIFF-16.1: id's server runs the progs as it is, with id's builtin table.
        let options = LoadOptions {
            registration: self.vfs.registration(),
            ..LoadOptions::default()
        };
        let mut world =
            World::load(progs, &self.map, &bsp, reader, options).map_err(|e| e.to_string())?;
        world.settle(progs).map_err(|e| e.to_string())?;
        let mut frame = 0_usize;
        if !visit(frame, &world.state(progs))? {
            return Ok(frame);
        }
        let mut cmd = UserCmd::default();
        for step in &route.steps {
            match step {
                Step::Move(next) => cmd = *next,
                Step::Command(text) => world.client_command(text),
                Step::Frames(count) => {
                    for _ in 0..*count {
                        frame = frame.saturating_add(1);
                        // DIFF-16.1, PHYS-10.7: the client connects in frame 1 and answers
                        // each signon stage in the next frame, as the oracle's client does.
                        match frame {
                            1 => world.connect_next_frame(),
                            2 => world.client_command("prespawn"),
                            3 => {
                                world.client_command("name \"player\"\n");
                                world.client_command("color 0 0\n");
                                world.client_command("spawn ");
                            }
                            4 => world.client_command("begin"),
                            _ => {}
                        }
                        world.set_command(cmd);
                        world
                            .frame(progs, route.frametime)
                            .map_err(|e| format!("frame {frame}: {e}"))?;
                        drop(world.take_messages());
                        // An impulse is sent once.
                        cmd.impulse = 0;
                        if !visit(frame, &world.state(progs))? {
                            return Ok(frame);
                        }
                    }
                }
            }
        }
        Ok(frame)
    }
}

/// The oracle's raw state, replayed from its trace.
struct Replayed<'p> {
    progs: &'p Progs<'p>,
    globals: Vec<Slot>,
    fields: Vec<Vec<Slot>>,
    edict_size: i64,
    strings: BTreeMap<u32, Vec<u8>>,
}

impl RawState for Replayed<'_> {
    fn global(&self, slot: usize) -> Slot {
        self.globals.get(slot).copied().unwrap_or(Slot::ZERO)
    }

    fn field(&self, edict: usize, slot: usize) -> Slot {
        self.fields
            .get(edict)
            .and_then(|row| row.get(slot))
            .copied()
            .unwrap_or(Slot::ZERO)
    }

    fn num_edicts(&self) -> usize {
        self.fields.len()
    }

    fn entity(&self, value: Slot) -> usize {
        let value = i64::from(value.as_i32());
        if value < 0 || value.checked_rem(self.edict_size) != Some(0) {
            return 0;
        }
        value
            .checked_div(self.edict_size)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0)
    }

    fn string(&self, progs: &Progs<'_>, value: Slot) -> Vec<u8> {
        let bits = value.bits();
        if let Some(text) = self.strings.get(&bits) {
            return text.clone();
        }
        progs
            .string(value.as_i32())
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    }
}

/// The oracle's state at frame 0 and after each frame, one at a time: the trace's frame
/// records applied in order to all-zero globals and fields (TRACE-4.12), its strings as its
/// string records say.
struct OracleFrames<'p, 't> {
    replayed: Replayed<'p>,
    records: core::slice::Iter<'t, Record>,
    entityfields: usize,
}

impl<'p, 't> OracleFrames<'p, 't> {
    fn new(progs: &'p Progs<'p>, trace: &'t Trace) -> Result<Self, String> {
        let header = &trace.header;
        Ok(Self {
            replayed: Replayed {
                progs,
                globals: Vec::new(),
                fields: Vec::new(),
                edict_size: i64::from(header.edict_size()).max(1),
                strings: BTreeMap::new(),
            },
            records: trace.records.iter(),
            entityfields: usize::try_from(header.entityfields())
                .map_err(|_| "a field count past usize")?,
        })
    }

    /// The next frame's state, `None` after the last.
    fn next_state(&mut self) -> Result<Option<State>, String> {
        for record in self.records.by_ref() {
            let replayed = &mut self.replayed;
            match record {
                Record::String { value, text } => {
                    replayed.strings.insert(*value, text.as_bytes().to_vec());
                }
                Record::Frame { changes, .. } => {
                    for change in changes.globals() {
                        let slot = usize::try_from(change.slot).map_err(|_| "a slot past usize")?;
                        if replayed.globals.len() <= slot {
                            replayed.globals.resize(slot.saturating_add(1), Slot::ZERO);
                        }
                        if let Some(cell) = replayed.globals.get_mut(slot) {
                            *cell = Slot::from_bits(change.bits);
                        }
                    }
                    for change in changes.fields() {
                        let edict =
                            usize::try_from(change.edict).map_err(|_| "an edict past usize")?;
                        let field =
                            usize::try_from(change.field).map_err(|_| "a field past usize")?;
                        if replayed.fields.len() <= edict {
                            replayed.fields.resize(
                                edict.saturating_add(1),
                                vec![Slot::ZERO; self.entityfields],
                            );
                        }
                        if let Some(cell) = replayed
                            .fields
                            .get_mut(edict)
                            .and_then(|row| row.get_mut(field))
                        {
                            *cell = Slot::from_bits(change.bits);
                        }
                    }
                    return Ok(Some(read_state(replayed.progs, &*replayed)));
                }
                Record::Error { message, .. } => {
                    return Err(format!(
                        "the oracle stopped on an error: {}",
                        String::from_utf8_lossy(message.as_bytes())
                    ));
                }
                Record::Run { .. } | Record::Step(_) | Record::Exit { .. } => {
                    return Err("not an engine trace".to_owned());
                }
            }
        }
        Ok(None)
    }
}

/// The most differences one frame's report names (DIFF-16.4).
const SHOWN: usize = 12;

/// The differences between the oracle's state and ours, globals first, then edict by edict,
/// at most [`SHOWN`] of them; edicts past one side's end read as that side's zero state
/// (DIFF-16.3).
fn differences(theirs: &State, mine: &State) -> Vec<String> {
    let mut out: Vec<String> = differs(&theirs.globals, &mine.globals)
        .map(|at| at.describe("global"))
        .collect();
    let edicts = theirs.edicts.len().max(mine.edicts.len());
    for n in 0..edicts {
        if out.len() >= SHOWN {
            break;
        }
        let (a, b) = (theirs.edicts.get(n), mine.edicts.get(n));
        let zero_a = a.is_none().then(|| zeroed(b));
        let zero_b = b.is_none().then(|| zeroed(a));
        let (Some(a), Some(b)) = (a.or(zero_a.as_ref()), b.or(zero_b.as_ref())) else {
            continue;
        };
        let classname = a
            .iter()
            .chain(b.iter())
            .find(|f| f.name == "classname")
            .and_then(|f| match &f.value {
                QcValue::String(text) => Some(String::from_utf8_lossy(text).into_owned()),
                _ => None,
            })
            .filter(|name| !name.is_empty())
            .map_or_else(String::new, |name| format!(" ({name})"));
        out.extend(differs(a, b).map(|at| at.describe(&format!("edict {n}{classname} field"))));
    }
    out.truncate(SHOWN);
    out
}

/// `fields` with every value zero, for an edict one side has not reached.
fn zeroed(fields: Option<&Vec<NamedValue>>) -> Vec<NamedValue> {
    fields
        .map(|fields| {
            fields
                .iter()
                .map(|f| NamedValue {
                    name: f.name.clone(),
                    value: zero_of(&f.value),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn zero_of(value: &QcValue) -> QcValue {
    match value {
        QcValue::Float(_) => QcValue::Float(0.0),
        QcValue::Vector(_) => QcValue::Vector([0.0; 3]),
        QcValue::String(_) => QcValue::String(Vec::new()),
        QcValue::Entity(_) => QcValue::Entity(0),
        QcValue::Field(_) => QcValue::Field(String::new()),
        QcValue::Function(_) => QcValue::Function(String::new()),
        QcValue::Void => QcValue::Void,
    }
}

/// Where two value lists first differ.
struct Difference<'a> {
    name: &'a str,
    theirs: &'a QcValue,
    mine: &'a QcValue,
}

impl Difference<'_> {
    fn describe(&self, what: &str) -> String {
        format!(
            "{what} {}: id's server {:?}, qs-sim {:?}",
            self.name, self.theirs, self.mine
        )
    }
}

fn differs<'a>(
    theirs: &'a [NamedValue],
    mine: &'a [NamedValue],
) -> impl Iterator<Item = Difference<'a>> {
    theirs
        .iter()
        .zip(mine)
        .filter(|(a, b)| !same(&a.value, &b.value))
        .map(|(a, b)| Difference {
            name: &a.name,
            theirs: &a.value,
            mine: &b.value,
        })
}

/// Whether two values are the same, floats by their bits (DIFF-16.2).
fn same(a: &QcValue, b: &QcValue) -> bool {
    match (a, b) {
        (QcValue::Float(x), QcValue::Float(y)) => x.to_bits() == y.to_bits(),
        (QcValue::Vector(x), QcValue::Vector(y)) => {
            x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests;
