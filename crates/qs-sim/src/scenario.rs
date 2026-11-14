// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic scenarios (`docs/spec/scenario.md`, SCN; P3-19): a route of the engine oracle
//! (DIFF-16.1) with a seed, a player start, assertions on globals and fields, and screenshots,
//! played in a [`World`] in the process. [`play`] runs the steps and shows the world to a
//! visitor, which the server simulation differential reads frame by frame; [`run`] checks a
//! scenario's assertions and answers with a [`Report`].

use std::fmt;

use qc_progs::Progs;
use serde::Serialize;

use crate::{CLIENT_EDICT, EditError, NamedValue, QcValue, State, UserCmd, World, WorldError};

/// One server frame's length when a scenario does not say: `Host_FilterTime`'s shortest.
pub const SCN_FRAMETIME: f64 = 1.0 / 72.0;

/// The frame in which the player enters the game (SCN-2.1, DIFF-16.1): the client connects in
/// frame 1 and sends `begin` in frame 4.
const ENTERED: usize = 4;

/// A scenario as SCN-1 reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    /// The engine's command line words its `engine` lines give.
    pub engine: Vec<String>,
    /// The map.
    pub map: String,
    /// A server frame's length in seconds.
    pub frametime: f64,
    /// The state the server's random source starts at, `None` for MOVE-6.1's 1 (SCN-1.3).
    pub seed: Option<u32>,
    /// Where the player is put once it has entered the game (SCN-1.4).
    pub start: Option<Start>,
    /// The steps, in order.
    pub steps: Vec<Step>,
}

/// A player start: an origin and a yaw (SCN-1.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    /// Where the player stands.
    pub origin: [f32; 3],
    /// Which way it faces, in degrees.
    pub yaw: f32,
}

/// One step of a scenario.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// A client string command, read in the next frame.
    Command(String),
    /// A line typed at the server's console, run in the next frame (DIFF-16.6).
    Console(String),
    /// The move sent each frame from now on.
    Move(UserCmd),
    /// Frames to run.
    Frames(u32),
    /// An assertion, checked here (SCN-1.5).
    Expect(Expectation),
    /// A screenshot asked for here, by its name (SCN-1.6).
    Screenshot(String),
}

/// An assertion (SCN-1.5): a target, how it compares and what with.
#[derive(Clone, Debug, PartialEq)]
pub struct Expectation {
    /// The line it is on, from 1.
    pub line: usize,
    /// The line's text after `expect `.
    pub text: String,
    /// What is read.
    pub target: Target,
    /// How it compares.
    pub op: Op,
    /// What it is compared with.
    pub value: Expected,
}

/// What an assertion reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A named global.
    Global(String),
    /// A named field of an edict.
    Field(Edict, String),
}

/// Which edict an assertion reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edict {
    /// This edict number.
    Number(usize),
    /// The player, edict 1.
    Player,
    /// The lowest-numbered edict in use with this classname.
    Classname(String),
}

/// A comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`, floats only.
    Lt,
    /// `<=`, floats only.
    Le,
    /// `>`, floats only.
    Gt,
    /// `>=`, floats only.
    Ge,
}

impl Op {
    /// The operator `word` writes.
    fn of(word: &str) -> Option<Self> {
        Some(match word {
            "==" => Self::Eq,
            "!=" => Self::Ne,
            "<" => Self::Lt,
            "<=" => Self::Le,
            ">" => Self::Gt,
            ">=" => Self::Ge,
            _ => return None,
        })
    }

    /// Whether it orders, which only floats do.
    const fn orders(self) -> bool {
        match self {
            Self::Eq | Self::Ne => false,
            Self::Lt | Self::Le | Self::Gt | Self::Ge => true,
        }
    }

    /// Whether `ordering`, the read value against the expected one, satisfies it.
    fn holds(self, ordering: Option<std::cmp::Ordering>) -> bool {
        use std::cmp::Ordering::{Equal, Greater, Less};
        matches!(
            (self, ordering),
            (Self::Eq, Some(Equal))
                | (Self::Ne, Some(Less | Greater) | None)
                | (Self::Lt, Some(Less))
                | (Self::Gt, Some(Greater))
                | (Self::Le, Some(Less | Equal))
                | (Self::Ge, Some(Greater | Equal))
        )
    }
}

/// The value an assertion compares with.
#[derive(Clone, Debug, PartialEq)]
pub enum Expected {
    /// A float.
    Float(f32),
    /// A vector.
    Vector([f32; 3]),
    /// A string.
    String(String),
    /// An edict, `#n`.
    Entity(usize),
}

/// Why a scenario's text does not read: its line, from 1, and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioError {
    /// The line, 0 for the scenario as a whole.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ScenarioError {}

impl Scenario {
    /// Reads `text` as SCN-1 says.
    ///
    /// # Errors
    ///
    /// [`ScenarioError`] for a word SCN-1 does not list, a line whose rest does not read as its
    /// word says, a second `seed` or `start`, or no `map`.
    pub fn parse(text: &str) -> Result<Self, ScenarioError> {
        let mut scenario = Self {
            engine: Vec::new(),
            map: String::new(),
            frametime: SCN_FRAMETIME,
            seed: None,
            start: None,
            steps: Vec::new(),
        };
        for (number, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            let at = number.saturating_add(1);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let error = |message: &str| ScenarioError {
                line: at,
                message: message.to_owned(),
            };
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            match word {
                "engine" => scenario.engine.push(rest.to_owned()),
                "map" => rest.clone_into(&mut scenario.map),
                "frametime" => {
                    scenario.frametime = rest
                        .parse()
                        .ok()
                        .filter(|t: &f64| *t > 0.0 && *t <= 1.0)
                        .ok_or_else(|| error("frametime is above 0, at most 1"))?;
                }
                "cmd" => scenario.steps.push(Step::Command(rest.to_owned())),
                "console" => scenario.steps.push(Step::Console(rest.to_owned())),
                "move" => scenario
                    .steps
                    .push(Step::Move(parse_move(rest).ok_or_else(|| {
                        error("move is pitch yaw roll forward side up buttons impulse")
                    })?)),
                "frames" => scenario.steps.push(Step::Frames(
                    rest.parse()
                        .ok()
                        .filter(|n| *n >= 1)
                        .ok_or_else(|| error("frames is a count from 1"))?,
                )),
                "seed" => {
                    if scenario.seed.is_some() {
                        return Err(error("a second seed"));
                    }
                    scenario.seed = Some(
                        rest.parse()
                            .map_err(|_| error("seed is a whole number from 0 to 4294967295"))?,
                    );
                }
                "start" => {
                    if scenario.start.is_some() {
                        return Err(error("a second start"));
                    }
                    let numbers = floats(rest).ok_or_else(|| error("start is x y z yaw"))?;
                    let [x, y, z, yaw] = numbers[..] else {
                        return Err(error("start is x y z yaw"));
                    };
                    scenario.start = Some(Start {
                        origin: [x, y, z],
                        yaw,
                    });
                }
                "expect" => scenario.steps.push(Step::Expect(
                    parse_expect(rest, at).map_err(|m| error(&m.to_string()))?,
                )),
                "screenshot" => {
                    let named = !rest.is_empty()
                        && rest
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                    if !named {
                        return Err(error("screenshot is a name of letters, digits, - and _"));
                    }
                    scenario.steps.push(Step::Screenshot(rest.to_owned()));
                }
                _ => return Err(error("unknown line")),
            }
        }
        if scenario.map.is_empty() {
            return Err(ScenarioError {
                line: 0,
                message: "no map line".to_owned(),
            });
        }
        Ok(scenario)
    }
}

/// Whitespace-separated numbers.
fn floats(text: &str) -> Option<Vec<f32>> {
    text.split_whitespace().map(|w| w.parse().ok()).collect()
}

/// `move <pitch> <yaw> <roll> <forward> <side> <up> <buttons> <impulse>` (SCN-1.2).
fn parse_move(text: &str) -> Option<UserCmd> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let [pitch, yaw, roll, forward, side, up, buttons, impulse] = words[..] else {
        return None;
    };
    let angle = |w: &str| w.parse::<f32>().ok();
    let short = |w: &str| w.parse::<i16>().ok().map(f32::from);
    let byte = |w: &str| w.parse::<u8>().ok();
    Some(UserCmd {
        angles: [angle(pitch)?, angle(yaw)?, angle(roll)?],
        forward: short(forward)?,
        side: short(side)?,
        up: short(up)?,
        buttons: byte(buttons)?,
        impulse: byte(impulse)?,
    })
}

/// Why an `expect` line does not parse (SCN-1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpectError {
    /// `expect global` without a name, an op and a value.
    GlobalForm,
    /// `expect field` without an edict, a name, an op and a value.
    FieldForm,
    /// `classname=` with no classname.
    EmptyClassname,
    /// An edict that is not a number, `player` or `classname=<name>`.
    EdictForm,
    /// Neither `expect global` nor `expect field`.
    TargetForm,
    /// No op and value after the target.
    OpValueForm,
    /// An op that is not `==`, `!=`, `<`, `<=`, `>` or `>=`.
    Op,
    /// A value that does not parse.
    Value,
    /// An ordering op on a value that is not a float.
    OrderedNotFloat,
}

impl fmt::Display for ExpectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::GlobalForm => "expect global <name> <op> <value>",
            Self::FieldForm => "expect field <edict> <name> <op> <value>",
            Self::EmptyClassname => "classname= names a classname",
            Self::EdictForm => "an edict is a number, player or classname=<name>",
            Self::TargetForm => "expect global ... or expect field ...",
            Self::OpValueForm => "expect ... <op> <value>",
            Self::Op => "an op is ==, !=, <, <=, > or >=",
            Self::Value => {
                "a value is a number, a vector 'x y z', a string in double quotes or an edict #n"
            }
            Self::OrderedNotFloat => "<, <=, > and >= compare floats",
        })
    }
}

/// `expect <target> <op> <value>` (SCN-1.5).
fn parse_expect(text: &str, line: usize) -> Result<Expectation, ExpectError> {
    let (target, rest) = match text.split_once(' ') {
        Some(("global", rest)) => {
            let (name, rest) = rest.split_once(' ').ok_or(ExpectError::GlobalForm)?;
            (Target::Global(name.to_owned()), rest)
        }
        Some(("field", rest)) => {
            let (edict, rest) = rest.split_once(' ').ok_or(ExpectError::FieldForm)?;
            let (name, rest) = rest.split_once(' ').ok_or(ExpectError::FieldForm)?;
            let edict = match edict {
                "player" => Edict::Player,
                _ => match edict.strip_prefix("classname=") {
                    Some(class) if !class.is_empty() => Edict::Classname(class.to_owned()),
                    Some(_) => return Err(ExpectError::EmptyClassname),
                    None => Edict::Number(edict.parse().map_err(|_| ExpectError::EdictForm)?),
                },
            };
            (Target::Field(edict, name.to_owned()), rest)
        }
        _ => return Err(ExpectError::TargetForm),
    };
    let (op, value) = rest.split_once(' ').ok_or(ExpectError::OpValueForm)?;
    let op = Op::of(op).ok_or(ExpectError::Op)?;
    let value = parse_value(value.trim()).ok_or(ExpectError::Value)?;
    if op.orders() && !matches!(value, Expected::Float(_)) {
        return Err(ExpectError::OrderedNotFloat);
    }
    Ok(Expectation {
        line,
        text: text.to_owned(),
        target,
        op,
        value,
    })
}

/// A value as an assertion writes it (SCN-1.5).
fn parse_value(text: &str) -> Option<Expected> {
    if let Some(inner) = text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        let numbers = floats(inner)?;
        let [x, y, z] = numbers[..] else {
            return None;
        };
        return Some(Expected::Vector([x, y, z]));
    }
    if let Some(inner) = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .filter(|_| text.len() >= 2)
    {
        return Some(Expected::String(inner.to_owned()));
    }
    if let Some(number) = text.strip_prefix('#') {
        return number.parse().ok().map(Expected::Entity);
    }
    text.parse().ok().map(Expected::Float)
}

/// Where a run is when it shows the world to its visitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Moment<'s> {
    /// After the map settled (frame 0) or after a frame.
    Frame,
    /// At an assertion.
    Expect(&'s Expectation),
    /// At a screenshot, by its name.
    Screenshot(&'s str),
}

/// Why a play stopped before its last step.
#[derive(Debug)]
pub enum PlayError<E> {
    /// A frame failed with a run error (SCN-2.3).
    Frame {
        /// The frame, from 1.
        frame: usize,
        /// Why.
        error: WorldError,
    },
    /// The player could not be put at the scenario's start.
    Start(EditError),
    /// The visitor stopped the play.
    Visit(E),
}

impl<E: fmt::Display> fmt::Display for PlayError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame { frame, error } => write!(f, "frame {frame}: {error}"),
            Self::Start(error) => write!(f, "the player's start: {error}"),
            Self::Visit(error) => error.fmt(f),
        }
    }
}

impl Scenario {
    /// Plays the scenario in `world`, settled on its map (SCN-2.1): the client connects in frame 1
    /// and answers each signon stage in the next frame, `start` puts the player after frame 4, and
    /// each step runs in order. `visit` sees the world after the map settled and after each frame
    /// (with the frame's number), and at each assertion and screenshot; it stops the play by
    /// answering `false`. Returns the frames run.
    ///
    /// # Errors
    ///
    /// [`PlayError`]: a frame's run error, a start the player could not be moved to, or the
    /// visitor's error.
    pub fn play<E>(
        &self,
        world: &mut World,
        progs: &Progs<'_>,
        mut visit: impl FnMut(Moment<'_>, usize, &World) -> Result<bool, E>,
    ) -> Result<usize, PlayError<E>> {
        let mut frame = 0_usize;
        if !visit(Moment::Frame, frame, world).map_err(PlayError::Visit)? {
            return Ok(frame);
        }
        let mut cmd = UserCmd::default();
        for step in &self.steps {
            match step {
                Step::Move(next) => cmd = *next,
                Step::Command(text) => world.client_command(text),
                Step::Console(text) => {
                    world.console(text);
                    world.console("\n");
                }
                Step::Expect(expectation) => {
                    if !visit(Moment::Expect(expectation), frame, world)
                        .map_err(PlayError::Visit)?
                    {
                        return Ok(frame);
                    }
                }
                Step::Screenshot(name) => {
                    if !visit(Moment::Screenshot(name), frame, world).map_err(PlayError::Visit)? {
                        return Ok(frame);
                    }
                }
                Step::Frames(count) => {
                    for _ in 0..*count {
                        frame = frame.saturating_add(1);
                        // DIFF-16.1, PHYS-10.7: the client connects in frame 1 and answers each
                        // signon stage in the next frame, as the oracle's client does.
                        match frame {
                            1 => world.connect_next_frame(),
                            2 => world.client_command("prespawn"),
                            3 => {
                                world.client_command("name \"player\"\n");
                                world.client_command("color 0 0\n");
                                world.client_command("spawn ");
                            }
                            ENTERED => world.client_command("begin"),
                            _ => {}
                        }
                        world.set_command(cmd);
                        world
                            .frame(progs, self.frametime)
                            .map_err(|error| PlayError::Frame { frame, error })?;
                        drop(world.take_messages());
                        // An impulse is sent once.
                        cmd.impulse = 0;
                        // SCN-2.1: the start once the player is in the game.
                        if frame == ENTERED
                            && let Some(start) = self.start
                        {
                            world
                                .move_player(progs, start.origin, [0.0, start.yaw, 0.0])
                                .map_err(PlayError::Start)?;
                        }
                        if !visit(Moment::Frame, frame, world).map_err(PlayError::Visit)? {
                            return Ok(frame);
                        }
                    }
                }
            }
        }
        Ok(frame)
    }

    /// Plays the scenario in `world` and checks its assertions (SCN-2).
    #[must_use]
    pub fn run(&self, world: &mut World, progs: &Progs<'_>) -> Report {
        let mut checks = Vec::new();
        let mut screenshots = Vec::new();
        let played = self.play(world, progs, |moment, frame, world| {
            match moment {
                Moment::Frame => {}
                Moment::Expect(expectation) => checks.push(expectation.check(world, progs)),
                Moment::Screenshot(name) => {
                    screenshots.push(Shot::taken(&world.state(progs), name, frame));
                }
            }
            Ok::<bool, std::convert::Infallible>(true)
        });
        let (frames, error) = match played {
            Ok(frames) => (frames, None),
            Err(PlayError::Frame { frame, error }) => {
                (frame, Some(format!("frame {frame}: {error}")))
            }
            Err(PlayError::Start(error)) => (ENTERED, Some(format!("the player's start: {error}"))),
            Err(PlayError::Visit(never)) => match never {},
        };
        Report {
            passed: error.is_none() && checks.iter().all(|check| check.passed),
            frames,
            checks,
            screenshots,
            error,
        }
    }
}

/// The result of a scenario's run (SCN-2.4).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    /// Every assertion passed and no frame failed.
    pub passed: bool,
    /// The frames that ran.
    pub frames: usize,
    /// Each assertion checked, in order.
    pub checks: Vec<Check>,
    /// Each screenshot asked for, in order.
    pub screenshots: Vec<Shot>,
    /// The frame's run error, or the start's, that stopped the run.
    pub error: Option<String>,
}

/// One assertion's outcome (SCN-2.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Check {
    /// Its line, from 1.
    pub line: usize,
    /// Its text after `expect `.
    pub text: String,
    /// Whether it held.
    pub passed: bool,
    /// The value read, as the scenario writes values, or why there is none.
    pub found: String,
}

/// A screenshot asked for (SCN-1.6): its name, its frame and the player's eye.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shot {
    /// Its name.
    pub name: String,
    /// The frame it was asked for after.
    pub frame: usize,
    /// The player's origin.
    pub origin: [f32; 3],
    /// The player's origin plus its `view_ofs`.
    pub eye: [f32; 3],
    /// The player's `v_angle`.
    pub angles: [f32; 3],
}

impl Expectation {
    /// The expectation checked against `world` (SCN-2.2).
    fn check(&self, world: &World, progs: &Progs<'_>) -> Check {
        let state = world.state(progs);
        let found = self.target.read(world, &state);
        let (passed, found) = match found {
            Ok(value) => match self.value.compare(value) {
                Some(ordering) => (self.op.holds(ordering), value.shown()),
                None => (
                    false,
                    format!("{}, not of the type expected", value.shown()),
                ),
            },
            Err(why) => (false, why.to_string()),
        };
        Check {
            line: self.line,
            text: self.text.clone(),
            passed,
            found,
        }
    }
}

/// Why an assertion's target has no value (SCN-2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReadError {
    /// No global has the name.
    NoGlobal(String),
    /// No edict in use has the classname.
    NoClassname(String),
    /// The edict is not in use.
    NotInUse(usize),
    /// No field has the name.
    NoField(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGlobal(name) => write!(f, "no global {name}"),
            Self::NoClassname(class) => write!(f, "no edict in use with classname {class}"),
            Self::NotInUse(number) => write!(f, "edict {number} is not in use"),
            Self::NoField(name) => write!(f, "no field {name}"),
        }
    }
}

impl Target {
    /// The value the target names in `state`, or why there is none.
    fn read<'s>(&self, world: &World, state: &'s State) -> Result<&'s QcValue, ReadError> {
        let named = |values: &'s [NamedValue], name: &str| {
            values
                .iter()
                .find(|value| value.name == name)
                .map(|value| &value.value)
        };
        match self {
            Target::Global(name) => {
                named(&state.globals, name).ok_or_else(|| ReadError::NoGlobal(name.clone()))
            }
            Target::Field(edict, name) => {
                let number = match edict {
                    Edict::Number(n) => *n,
                    Edict::Player => CLIENT_EDICT,
                    Edict::Classname(class) => (0..state.edicts.len())
                        .find(|n| {
                            world.in_use(*n)
                                && state.edicts.get(*n).is_some_and(|fields| {
                                    named(fields, "classname")
                                        == Some(&QcValue::String(class.as_bytes().to_vec()))
                                })
                        })
                        .ok_or_else(|| ReadError::NoClassname(class.clone()))?,
                };
                if !world.in_use(number) {
                    return Err(ReadError::NotInUse(number));
                }
                let fields = state
                    .edicts
                    .get(number)
                    .ok_or(ReadError::NotInUse(number))?;
                named(fields, name).ok_or_else(|| ReadError::NoField(name.clone()))
            }
        }
    }
}

impl Expected {
    /// How `value` compares with the expected value; `None` when they are of different types. A vector, a
    /// string or an edict is equal or not: `Less` stands for not equal.
    fn compare(&self, value: &QcValue) -> Option<Option<std::cmp::Ordering>> {
        use std::cmp::Ordering::{Equal, Less};
        let equal = |same: bool| Some(Some(if same { Equal } else { Less }));
        match (value, self) {
            (QcValue::Float(a), Expected::Float(b)) => Some(a.partial_cmp(b)),
            (QcValue::Vector(a), Expected::Vector(b)) => {
                // SCN-1.5: compared exactly, component by component.
                equal(
                    a.iter()
                        .zip(b)
                        .all(|(x, y)| x.partial_cmp(y) == Some(Equal)),
                )
            }
            (QcValue::String(a), Expected::String(b)) => equal(a == b.as_bytes()),
            (QcValue::Entity(a), Expected::Entity(b)) => equal(a == b),
            _ => None,
        }
    }
}

impl QcValue {
    /// A value as a scenario writes it.
    fn shown(&self) -> String {
        match self {
            QcValue::Float(f) => f.to_string(),
            QcValue::Vector([x, y, z]) => format!("'{x} {y} {z}'"),
            QcValue::String(bytes) => format!("\"{}\"", String::from_utf8_lossy(bytes)),
            QcValue::Entity(n) => format!("#{n}"),
            QcValue::Field(name) | QcValue::Function(name) => name.clone(),
            QcValue::Void => "void".to_owned(),
        }
    }
}

impl Shot {
    /// The screenshot `name` asked for after frame `frame`: the player's eye (SCN-1.6).
    fn taken(state: &State, name: &str, frame: usize) -> Self {
        let vector = |field: &str| {
            state
                .edicts
                .get(CLIENT_EDICT)
                .and_then(|fields| fields.iter().find(|f| f.name == field))
                .and_then(|f| match f.value {
                    QcValue::Vector(v) => Some(v),
                    _ => None,
                })
                .unwrap_or_default()
        };
        let [x, y, z] = vector("origin");
        let [dx, dy, dz] = vector("view_ofs");
        Self {
            name: name.to_owned(),
            frame,
            origin: [x, y, z],
            eye: [x + dx, y + dy, z + dz],
            angles: vector("v_angle"),
        }
    }
}

#[cfg(test)]
mod tests;
