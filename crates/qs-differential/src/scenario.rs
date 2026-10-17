// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a differential scenario file (`differential.md` DIFF-2).
//!
//! A scenario names a progs, a map's entities and the settings of one run. Both the id VM
//! oracle (`oracles/vm/scenario.c`) and this runner read it, and the whole contract between
//! them is DIFF-2: the two must accept and refuse exactly the same files and read each value
//! the same way, so that a divergence in a trace is a defect in a VM, not in a reader.
//!
//! The reader works in bytes, not text, because DIFF-2.1 makes a scenario lines of bytes and
//! a NUL byte a byte of its line. It checks the integer and number syntax of DIFF-2.7 and
//! DIFF-2.8 itself before converting, because Rust's `i64`, `f64` and `f32` parsing accept
//! texts the scenario refuses (`+1`, `inf`, `nan`, hexadecimal). `crates/qs-tests`'
//! `scenario_numbers` pins the per-text verdicts and values against
//! `docs/spec/vectors/scenario/numbers.txt`, which `cargo xtask oracles` also runs through the
//! oracle.
//!
//! Implements `docs/spec/differential.md` DIFF-2 (the scenario file). The defaults and limits
//! are the constants of DIFF-2.2 and the oracle's `scenario.c`.

use std::path::{Path, PathBuf};

/// `DIFF_DEFAULT_MAP` (DIFF-2.2): the map name when a scenario gives none.
pub const DIFF_DEFAULT_MAP: &str = "test";
/// `DIFF_DEFAULT_FRAMES` (DIFF-2.2): server frames after the clients enter.
pub const DIFF_DEFAULT_FRAMES: u32 = 100;
/// `DIFF_DEFAULT_FRAMETIME` (DIFF-2.2): the length of one server frame, in seconds.
pub const DIFF_DEFAULT_FRAMETIME: f64 = 0.1;
/// `DIFF_DEFAULT_SEED` (DIFF-2.2): the random state a run starts with.
pub const DIFF_DEFAULT_SEED: u32 = 1;

/// Most client slots a server has (`QS_MAX_CLIENTS`, EDICT-5.1); `maxclients` is 1 to this.
pub const DIFF_MAX_CLIENTS: u32 = 16;
/// Most brush submodels a map may have (`submodels`, DIFF-2.2): `*1` to `*254`.
pub const DIFF_MAX_SUBMODELS: u32 = 254;
/// Most server frames a scenario may run (`frames`, DIFF-2.2).
pub const DIFF_MAX_FRAMES: u32 = 1_000_000;
/// The largest `seed` a scenario may give, `u32::MAX` (DIFF-2.2).
pub const DIFF_MAX_SEED: u64 = 4_294_967_295;
/// Most words the oracle reads from one line (`scenario.c`): a 17th is "too many arguments".
const MAX_WORDS: usize = 16;

/// Why a scenario was refused (DIFF-2.9), with the line it was refused at, or 0 for a
/// whole-file check (no progs, clients over maxclients, an input past the last frame).
///
/// The reason text is not part of the contract; DIFF-2.9 fixes only that the file is refused
/// and no trace is written. The line matches the oracle's, which names it in the refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The line the refusal is reported at, from 1, or 0 for a whole-file check.
    pub line: usize,
    /// A short reason, for a human reading a failed run.
    pub reason: String,
}

impl core::fmt::Display for Refused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.reason)
        } else {
            write!(f, "{}", self.reason)
        }
    }
}

impl std::error::Error for Refused {}

/// One client input of an `input` line (DIFF-2.6, DIFF-4.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// `button0 <0 or 1>`.
    Button0 {
        /// The frame the input is applied at (DIFF-5.1), 1 to `frames`.
        frame: u32,
        /// The client edict, 1 to `clients`.
        edict: u32,
        /// 0 or 1.
        value: f32,
    },
    /// `button2 <0 or 1>`.
    Button2 {
        /// The frame the input is applied at.
        frame: u32,
        /// The client edict.
        edict: u32,
        /// 0 or 1.
        value: f32,
    },
    /// `impulse <1 to 255>`.
    Impulse {
        /// The frame the input is applied at.
        frame: u32,
        /// The client edict.
        edict: u32,
        /// 1 to 255.
        value: f32,
    },
    /// `v_angle <x> <y> <z>`.
    VAngle {
        /// The frame the input is applied at.
        frame: u32,
        /// The client edict.
        edict: u32,
        /// The three angle components, each an `f32` of DIFF-2.8.
        values: [f32; 3],
    },
}

impl Input {
    /// The frame this input is applied at.
    #[must_use]
    pub const fn frame(&self) -> u32 {
        match self {
            Self::Button0 { frame, .. }
            | Self::Button2 { frame, .. }
            | Self::Impulse { frame, .. }
            | Self::VAngle { frame, .. } => *frame,
        }
    }

    /// The client edict this input is applied to.
    #[must_use]
    pub const fn edict(&self) -> u32 {
        match self {
            Self::Button0 { edict, .. }
            | Self::Button2 { edict, .. }
            | Self::Impulse { edict, .. }
            | Self::VAngle { edict, .. } => *edict,
        }
    }
}

/// A `cvar` setting and its text (DIFF-2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cvar {
    /// The setting name, as raw bytes: it may hold a NUL byte (DIFF-2.1).
    pub name: Vec<u8>,
    /// The text, as raw bytes; its value is `Q_atof`'s, which stops at a NUL (DIFF-2.4).
    pub text: Vec<u8>,
}

/// A `model` directive: a name and its bounds (DIFF-2.2, DIFF-6.3).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelBounds {
    /// The model name, as raw bytes: it may hold a NUL byte (DIFF-2.1).
    pub name: Vec<u8>,
    /// The mins, three `f32` of DIFF-2.8.
    pub mins: [f32; 3],
    /// The maxs, three `f32` of DIFF-2.8.
    pub maxs: [f32; 3],
}

/// A read scenario: every directive of DIFF-2.2, with the defaults filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    /// The progs.dat to run, as a path relative to the scenario's directory (required).
    pub progs: PathBuf,
    /// The map's entities file, or `None` for the one `worldspawn` default (DIFF-2.2).
    pub entities: Option<PathBuf>,
    /// The map name (`DIFF_DEFAULT_MAP` by default).
    pub map: String,
    /// Client slots (1 to 16; default 1).
    pub maxclients: u32,
    /// How many players enter (0 to `maxclients`; default 0).
    pub clients: u32,
    /// Brush submodels the map has (0 to 254; default 0).
    pub submodels: u32,
    /// Server frames after the clients enter (default `DIFF_DEFAULT_FRAMES`).
    pub frames: u32,
    /// The length of each server frame, in seconds (default `DIFF_DEFAULT_FRAMETIME`).
    pub frametime: f64,
    /// The random state (default `DIFF_DEFAULT_SEED`).
    pub seed: u32,
    /// The `model` directives, in file order (DIFF-6.3).
    pub models: Vec<ModelBounds>,
    /// The `cvar` directives, in file order; a later one for a name wins (DIFF-2.4).
    pub cvars: Vec<Cvar>,
    /// The `input` directives, in file order (DIFF-4.3).
    pub inputs: Vec<Input>,
}

/// One directive key that may appear at most once (`once` in `scenario.c`).
const ONCE_KEYS: [&[u8]; 9] = [
    b"progs",
    b"entities",
    b"map",
    b"maxclients",
    b"clients",
    b"submodels",
    b"frames",
    b"frametime",
    b"seed",
];

/// Reads the scenario at `path`, relative paths resolved against its directory.
///
/// # Errors
///
/// [`Refused`] when the file cannot be read, or when it breaks DIFF-2.9 (an unknown key, a
/// missing or extra argument, a value that is not an integer or a number or is out of range, a
/// repeated single key, a repeated model name, or no `progs`).
pub fn read_file(path: &Path) -> Result<Scenario, Refused> {
    let bytes = std::fs::read(path).map_err(|e| Refused {
        line: 0,
        reason: format!("cannot read the scenario: {e}"),
    })?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    read(&bytes, dir)
}

/// Reads a scenario from its `bytes`, with `dir` the directory its relative paths resolve
/// against (DIFF-2.1).
///
/// # Errors
///
/// [`Refused`] on any of the DIFF-2.9 refusals.
pub fn read(bytes: &[u8], dir: &Path) -> Result<Scenario, Refused> {
    let mut builder = Builder::new(dir);
    for (index, raw) in split_lines(bytes).enumerate() {
        let line = index.saturating_add(1);
        let words = split_words(raw, line)?;
        if words.is_empty() {
            continue;
        }
        builder.directive(&words, line)?;
    }
    builder.finish()
}

/// Splits `bytes` into lines ended by `\n`, each with a trailing `\r` removed (DIFF-2.1). The
/// final line need not be terminated. A line keeps any NUL byte it holds.
fn split_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    // `split` on `\n` yields a trailing empty slice when the input ends in `\n`; that line is
    // empty after trimming and is skipped, matching the oracle, which stops at the last byte.
    bytes.split(|&b| b == b'\n').map(|line| match line.last() {
        Some(&b'\r') => line.get(..line.len().saturating_sub(1)).unwrap_or(line),
        _ => line,
    })
}

/// One word of a directive line, the raw bytes between the separators (DIFF-2.1).
#[derive(Clone, Copy)]
struct Word<'a> {
    bytes: &'a [u8],
}

impl Word<'_> {
    /// Whether the word holds a NUL byte. The oracle tells this by a C string's length
    /// differing from the word's; here it is a direct test (DIFF-2.1).
    fn has_nul(&self) -> bool {
        self.bytes.contains(&0)
    }

    /// The word as a `&str` when it is all ASCII and holds no NUL, for a key or a number.
    fn as_ascii(&self) -> Option<&str> {
        self.bytes
            .iter()
            .all(|&b| b.is_ascii() && b != 0)
            .then(|| core::str::from_utf8(self.bytes).ok())
            .flatten()
    }
}

/// Splits a line into words on spaces and tabs, after removing the `#` comment (DIFF-2.1).
///
/// # Errors
///
/// [`Refused`] when the line has more than [`MAX_WORDS`] words, as the oracle refuses a 17th.
fn split_words(line: &[u8], at: usize) -> Result<Vec<Word<'_>>, Refused> {
    let body = match line.iter().position(|&b| b == b'#') {
        Some(hash) => line.get(..hash).unwrap_or(line),
        None => line,
    };
    let mut words = Vec::new();
    let mut rest = body;
    loop {
        // Skip leading separators.
        let start = rest
            .iter()
            .position(|&b| b != b' ' && b != b'\t')
            .unwrap_or(rest.len());
        rest = rest.get(start..).unwrap_or(&[]);
        if rest.is_empty() {
            break;
        }
        let end = rest
            .iter()
            .position(|&b| b == b' ' || b == b'\t')
            .unwrap_or(rest.len());
        if words.len() == MAX_WORDS {
            return Err(refuse(at, "too many arguments"));
        }
        words.push(Word {
            bytes: rest.get(..end).unwrap_or(rest),
        });
        rest = rest.get(end..).unwrap_or(&[]);
    }
    Ok(words)
}

/// A refusal at `line` with `reason`.
fn refuse(line: usize, reason: &str) -> Refused {
    Refused {
        line,
        reason: reason.to_owned(),
    }
}

/// Builds a [`Scenario`] directive by directive, holding the defaults and the "seen once" set.
struct Builder {
    dir: PathBuf,
    progs: Option<PathBuf>,
    entities: Option<PathBuf>,
    map: String,
    maxclients: u32,
    clients: u32,
    submodels: u32,
    frames: u32,
    frametime: f64,
    seed: u32,
    models: Vec<ModelBounds>,
    cvars: Vec<Cvar>,
    inputs: Vec<Input>,
    seen: [bool; ONCE_KEYS.len()],
}

impl Builder {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            progs: None,
            entities: None,
            map: DIFF_DEFAULT_MAP.to_owned(),
            maxclients: 1,
            clients: 0,
            submodels: 0,
            frames: DIFF_DEFAULT_FRAMES,
            frametime: DIFF_DEFAULT_FRAMETIME,
            seed: DIFF_DEFAULT_SEED,
            models: Vec::new(),
            cvars: Vec::new(),
            inputs: Vec::new(),
            seen: [false; ONCE_KEYS.len()],
        }
    }

    /// A path argument, joined to the scenario's directory. A path may not hold a NUL byte, a
    /// space or a tab; the word split already rules the last two out, and a NUL means no file.
    fn path(&self, word: Word<'_>, at: usize) -> Result<PathBuf, Refused> {
        if word.has_nul() {
            return Err(refuse(at, "a path holds a NUL byte"));
        }
        let name = core::str::from_utf8(word.bytes)
            .map_err(|_| refuse(at, "a path is not valid UTF-8"))?;
        Ok(self.dir.join(name))
    }

    /// Marks a single key seen, refusing a repeat (DIFF-2.9).
    fn mark_once(&mut self, key: &[u8], at: usize) -> Result<(), Refused> {
        if let Some(index) = ONCE_KEYS.iter().position(|k| *k == key) {
            if self.seen.get(index).copied().unwrap_or(false) {
                return Err(refuse(at, "a key that may appear once is repeated"));
            }
            if let Some(slot) = self.seen.get_mut(index) {
                *slot = true;
            }
        }
        Ok(())
    }

    /// Reads one directive line (`words` is non-empty).
    fn directive(&mut self, words: &[Word<'_>], at: usize) -> Result<(), Refused> {
        let key = words.first().copied().unwrap_or(Word { bytes: &[] });
        // A key holding a NUL byte is a key no directive name has, so it is unknown.
        let name = key.as_ascii().ok_or_else(|| refuse(at, "unknown key"))?;
        // Reject a NUL byte in any argument that is not a model or cvar name, or a cvar text
        // (DIFF-2.1): those may hold one, a name no QuakeC string gives.
        let named = name == "model" || name == "cvar";
        for (index, word) in words.iter().enumerate().skip(1) {
            let name_slot = named && index == 1;
            let cvar_text = name == "cvar" && index == 2;
            if word.has_nul() && !name_slot && !cvar_text {
                return Err(refuse(at, "an argument holds a NUL byte"));
            }
        }
        self.mark_once(name.as_bytes(), at)?;
        let args = words.get(1..).unwrap_or(&[]);
        match name {
            "progs" => {
                let [word] = args else {
                    return Err(refuse(at, "progs takes a path"));
                };
                self.progs = Some(self.path(*word, at)?);
            }
            "entities" => {
                let [word] = args else {
                    return Err(refuse(at, "entities takes a path"));
                };
                self.entities = Some(self.path(*word, at)?);
            }
            "map" => {
                let [word] = args else {
                    return Err(refuse(at, "map takes a name"));
                };
                let name = word
                    .as_ascii()
                    .filter(|n| valid_map_name(n))
                    .ok_or_else(|| {
                        refuse(at, "map takes a name of 1 to 63 bytes from a-z, 0-9, _")
                    })?;
                self.map = name.to_owned();
            }
            "maxclients" => {
                self.maxclients = one_integer(args, 1, DIFF_MAX_CLIENTS.into(), at, "maxclients")?;
            }
            "clients" => {
                self.clients = one_integer(args, 0, DIFF_MAX_CLIENTS.into(), at, "clients")?;
            }
            "submodels" => {
                self.submodels = one_integer(args, 0, DIFF_MAX_SUBMODELS.into(), at, "submodels")?;
            }
            "frames" => {
                self.frames = one_integer(args, 0, DIFF_MAX_FRAMES.into(), at, "frames")?;
            }
            "frametime" => {
                let [word] = args else {
                    return Err(refuse(at, "frametime takes a number"));
                };
                self.frametime = read_frametime(*word, at)?;
            }
            "seed" => {
                let value = one_integer_u64(args, 0, DIFF_MAX_SEED, at, "seed")?;
                self.seed = u32::try_from(value).map_err(|_| refuse(at, "seed is out of range"))?;
            }
            "model" => self.model(args, at)?,
            "cvar" => self.cvar(args, at)?,
            "input" => self.input(args, at)?,
            _ => return Err(refuse(at, "unknown key")),
        }
        Ok(())
    }

    /// A `model` line: a name and six numbers (DIFF-2.2, DIFF-6.3).
    fn model(&mut self, args: &[Word<'_>], at: usize) -> Result<(), Refused> {
        let [name, numbers @ ..] = args else {
            return Err(refuse(at, "model takes a name and six numbers"));
        };
        let [a, b, c, d, e, f] = numbers else {
            return Err(refuse(at, "model takes a name and six numbers"));
        };
        let bounds = [
            read_model_number(*a, at)?,
            read_model_number(*b, at)?,
            read_model_number(*c, at)?,
            read_model_number(*d, at)?,
            read_model_number(*e, at)?,
            read_model_number(*f, at)?,
        ];
        if self.models.iter().any(|m| m.name == name.bytes) {
            return Err(refuse(at, "a model name is repeated"));
        }
        let [min_x, min_y, min_z, max_x, max_y, max_z] = bounds;
        self.models.push(ModelBounds {
            name: name.bytes.to_vec(),
            mins: [min_x, min_y, min_z],
            maxs: [max_x, max_y, max_z],
        });
        Ok(())
    }

    /// A `cvar` line: a name and a text (DIFF-2.4).
    fn cvar(&mut self, args: &[Word<'_>], at: usize) -> Result<(), Refused> {
        let [name, text] = args else {
            return Err(refuse(at, "cvar takes a name and a text"));
        };
        self.cvars.push(Cvar {
            name: name.bytes.to_vec(),
            text: text.bytes.to_vec(),
        });
        Ok(())
    }

    /// An `input` line: a frame, an edict, a field and its values (DIFF-2.6).
    fn input(&mut self, args: &[Word<'_>], at: usize) -> Result<(), Refused> {
        let [frame, edict, field, values @ ..] = args else {
            return Err(refuse(
                at,
                "input takes a frame, an edict, a field and values",
            ));
        };
        let frame = integer(*frame, 1, DIFF_MAX_FRAMES.into(), at, "input frame")?;
        let edict = integer(*edict, 1, DIFF_MAX_CLIENTS.into(), at, "input edict")?;
        let field = field
            .as_ascii()
            .ok_or_else(|| refuse(at, "input field is button0, button2, impulse or v_angle"))?;
        let input = match field {
            "button0" | "button2" => {
                let [value] = values else {
                    return Err(refuse(at, "a button takes 0 or 1"));
                };
                let value = integer(*value, 0, 1, at, "a button")?;
                let value = int_to_f32(value);
                if field == "button0" {
                    Input::Button0 {
                        frame,
                        edict,
                        value,
                    }
                } else {
                    Input::Button2 {
                        frame,
                        edict,
                        value,
                    }
                }
            }
            "impulse" => {
                let [value] = values else {
                    return Err(refuse(at, "impulse takes an integer from 1 to 255"));
                };
                let value = integer(*value, 1, 255, at, "impulse")?;
                Input::Impulse {
                    frame,
                    edict,
                    value: int_to_f32(value),
                }
            }
            "v_angle" => {
                let [a, b, c] = values else {
                    return Err(refuse(at, "v_angle takes three numbers"));
                };
                Input::VAngle {
                    frame,
                    edict,
                    values: [
                        read_model_number(*a, at)?,
                        read_model_number(*b, at)?,
                        read_model_number(*c, at)?,
                    ],
                }
            }
            _ => {
                return Err(refuse(
                    at,
                    "input field is button0, button2, impulse or v_angle",
                ));
            }
        };
        self.inputs.push(input);
        Ok(())
    }

    /// The whole-file checks of DIFF-2.9, reported at line 0 as the oracle reports them.
    fn finish(self) -> Result<Scenario, Refused> {
        let progs = self.progs.ok_or_else(|| refuse(0, "no progs"))?;
        if self.clients > self.maxclients {
            return Err(refuse(0, "clients is more than maxclients"));
        }
        for input in &self.inputs {
            if input.frame() > self.frames {
                return Err(refuse(0, "an input's frame is after the last frame"));
            }
            if input.edict() > self.clients {
                return Err(refuse(0, "an input's edict is not a client that enters"));
            }
        }
        Ok(Scenario {
            progs,
            entities: self.entities,
            map: self.map,
            maxclients: self.maxclients,
            clients: self.clients,
            submodels: self.submodels,
            frames: self.frames,
            frametime: self.frametime,
            seed: self.seed,
            models: self.models,
            cvars: self.cvars,
            inputs: self.inputs,
        })
    }
}

/// A map name is 1 to 63 bytes of `a`-`z`, `0`-`9` and `_` (DIFF-2.2).
fn valid_map_name(name: &str) -> bool {
    let len = name.len();
    (1..=63).contains(&len)
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Reads the one integer argument of a directive that takes one, into a `u32`.
fn one_integer(
    args: &[Word<'_>],
    min: i64,
    max: i64,
    at: usize,
    what: &str,
) -> Result<u32, Refused> {
    let [word] = args else {
        return Err(refuse(at, &format!("{what} takes an integer")));
    };
    let value = integer_i64(*word, min, max, at, what)?;
    u32::try_from(value).map_err(|_| refuse(at, &format!("{what} is out of range")))
}

/// Reads the one integer argument of a directive, into a `u64` (for `seed`).
fn one_integer_u64(
    args: &[Word<'_>],
    min: i64,
    max: u64,
    at: usize,
    what: &str,
) -> Result<u64, Refused> {
    let [word] = args else {
        return Err(refuse(at, &format!("{what} takes an integer")));
    };
    let max_i64 = i64::try_from(max).unwrap_or(i64::MAX);
    let value = integer_i64(*word, min, max_i64, at, what)?;
    u64::try_from(value).map_err(|_| refuse(at, &format!("{what} is out of range")))
}

/// Reads an integer word into a `u32`, refusing an out-of-range value (DIFF-2.7).
fn integer(word: Word<'_>, min: i64, max: i64, at: usize, what: &str) -> Result<u32, Refused> {
    let value = integer_i64(word, min, max, at, what)?;
    u32::try_from(value).map_err(|_| refuse(at, &format!("{what} is out of range")))
}

/// Reads an integer word into an `i64` with the syntax and range of DIFF-2.7.
///
/// The syntax is checked first; then Rust's `i64` parsing, which refuses a value that does not
/// fit, so a text with more significant digits than an `i64` holds is refused (out of range),
/// as the oracle refuses one over 18 digits.
fn integer_i64(word: Word<'_>, min: i64, max: i64, at: usize, what: &str) -> Result<i64, Refused> {
    let text = word
        .as_ascii()
        .filter(|t| is_integer(t))
        .ok_or_else(|| refuse(at, &format!("{what} takes an integer")))?;
    let value: i64 = text
        .parse()
        .map_err(|_| refuse(at, &format!("{what} is out of range")))?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(refuse(at, &format!("{what} is out of range")))
    }
}

/// DIFF-2.7: an optional `-`, then one or more decimal digits and nothing else.
fn is_integer(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Reads `frametime`: the `double` nearest the text, greater than 0 and at most 1 (DIFF-2.8).
fn read_frametime(word: Word<'_>, at: usize) -> Result<f64, Refused> {
    let text = word
        .as_ascii()
        .filter(|t| is_number(t))
        .ok_or_else(|| refuse(at, "frametime takes a number"))?;
    let value: f64 = text
        .parse()
        .map_err(|_| refuse(at, "frametime is not a number"))?;
    // spec DIFF-2.8: refused unless the double is greater than 0 and at most 1.
    if value > 0.0 && value <= 1.0 {
        Ok(value)
    } else {
        Err(refuse(at, "frametime is greater than 0 and at most 1"))
    }
}

/// Reads a `model` or `v_angle` number: the `f32` nearest the text, which must be finite
/// (DIFF-2.8).
fn read_model_number(word: Word<'_>, at: usize) -> Result<f32, Refused> {
    let text = word
        .as_ascii()
        .filter(|t| is_number(t))
        .ok_or_else(|| refuse(at, "a model or v_angle number"))?;
    let value: f32 = text
        .parse()
        .map_err(|_| refuse(at, "a model or v_angle number"))?;
    // spec DIFF-2.8: a number whose nearest f32 is infinite is refused.
    if value.is_finite() {
        Ok(value)
    } else {
        Err(refuse(at, "a model or v_angle number rounds to infinity"))
    }
}

/// DIFF-2.8: an optional `-`; digits with at most one `.` and at least one digit; and an
/// optional exponent `e`/`E`, an optional sign and one or more digits. Nothing else.
fn is_number(text: &str) -> bool {
    let text = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (
            text.get(..at).unwrap_or(""),
            Some(text.get(at.saturating_add(1)..).unwrap_or("")),
        ),
        None => (text, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits_only = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok =
        digits_only(whole) && digits_only(fraction) && !(whole.is_empty() && fraction.is_empty());
    let exponent_ok = match exponent {
        None => true,
        Some(e) => {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && digits_only(e)
        }
    };
    mantissa_ok && exponent_ok
}

/// Converts a small integer (a button, impulse or similar, 0 to 255) to the `f32` id's
/// `SV_ReadClientMove` stores (DIFF-4.3): the exact value, with no rounding in this range.
fn int_to_f32(value: u32) -> f32 {
    // 0 to 255 is exactly representable, so a lossless conversion suffices; written out rather
    // than `as` to keep the no-cast rule (the range is checked by the caller).
    f32::from(u8::try_from(value).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests;
