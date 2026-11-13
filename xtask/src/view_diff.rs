// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask view-diff [--map <name>]... [-- <engine word>...]`: the view differential
//! (P3-28, VIEWDIFF). Each map's start plays through the view oracle, id's whole engine
//! headless, and through the preview host's `--view-script`; their frames and sound are
//! compared within the map's tolerances in `docs/spec/engine/view-diff.md`.

use std::fs;
use std::path::Path;
use std::process::Command;

use qs_vfs::{LoadedFile, StdFs, Vfs};

use crate::oracles;
use crate::release_host;

/// The spec whose table holds the tolerances (VIEWDIFF-5).
const TOLERANCES_DOC: &str = "docs/spec/engine/view-diff.md";
/// The markers around the table.
const BEGIN: &str = "<!-- view-diff -->";
const END: &str = "<!-- /view-diff -->";

/// id1's maps, the episodes', the start, the end and the deathmatch maps: the routes run with
/// no `--map`.
const ID1_MAPS: [&str; 38] = [
    "start", "e1m1", "e1m2", "e1m3", "e1m4", "e1m5", "e1m6", "e1m7", "e1m8", "e2m1", "e2m2",
    "e2m3", "e2m4", "e2m5", "e2m6", "e2m7", "e3m1", "e3m2", "e3m3", "e3m4", "e3m5", "e3m6", "e3m7",
    "e4m1", "e4m2", "e4m3", "e4m4", "e4m5", "e4m6", "e4m7", "e4m8", "end", "dm1", "dm2", "dm3",
    "dm4", "dm5", "dm6",
];

/// The frames before the first shot: three seconds for the map to load and the client to
/// settle, so its first sounds, heard before the player's view is the listener's, are over
/// (VIEWDIFF-5.6).
const SETTLE_FRAMES: u32 = 216;
/// The frames after the shots, for the sound: three seconds (VIEWDIFF-5.6).
const SOUND_FRAMES: u32 = 216;
/// The frames after each place: a quarter of a second, so the eye's step smoothing, 12 units
/// behind at most and catching up at 80 a second, has settled (VIEWDIFF-2.4).
const PLACE_FRAMES: u32 = 18;

/// The width and height of a frame (VIEWDIFF-2.1).
const SIZE: (usize, usize) = (320, 240);
/// The side of a block of VIEWDIFF-3.1.
const BLOCK: usize = 4;

/// `cargo xtask view-diff`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Play this map's start; repeated, each in turn. Without it, every id1 map's.
    #[arg(long = "map")]
    maps: Vec<String>,
    /// Another option for the preview host, repeated: `--host-arg=--look --host-arg=glquake`
    /// draws GLQuake's look, which the comparison should fail.
    #[arg(long = "host-arg", allow_hyphen_values = true)]
    host_args: Vec<String>,
    /// The engine's command line, after `--`: `-basedir <dir>`, `-game <dir>`.
    #[arg(last = true)]
    engine: Vec<String>,
}

/// A map's tolerances (VIEWDIFF-5.2).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tolerance {
    /// The most a frame's difference may be (VIEWDIFF-3.3).
    frame: f64,
    /// How far from 1 a frame's brightness may be (VIEWDIFF-3.3).
    brightness: f64,
    /// The most the sound's difference may be (VIEWDIFF-4.3).
    sound: f64,
}

/// Plays each map's start through both engines and compares them; returns whether every map
/// passed.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if cfg!(windows) {
        // VIEWDIFF-2.6: id's Windows client needs its DirectX and MGL drivers.
        return Err("the view oracle is built on Linux and macOS only (VIEWDIFF-2.6)".into());
    }
    let engine = absolute(&args.engine)?;
    let doc = fs::read_to_string(root.join(TOLERANCES_DOC))
        .map_err(|e| format!("{TOLERANCES_DOC}: {e}"))?;
    let tolerances = parse_tolerances(&doc)?;
    let built = oracles::build(root)?;
    let host = release_host::build(root)?;
    let out = root.join("target/view-diff");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let maps: Vec<String> = if args.maps.is_empty() {
        ID1_MAPS.iter().map(|&map| map.to_owned()).collect()
    } else {
        args.maps.clone()
    };
    let mut passed = true;
    for map in &maps {
        let tolerance = tolerance_of(&tolerances, map)?;
        let (start, monsters) = player_start(&engine, map)?;
        let dir = out.join(map);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let run = |side: Side| -> Result<(), String> {
            let script = dir.join(format!("{side}.view"));
            fs::write(&script, route(map, start, side))
                .map_err(|e| format!("{}: {e}", script.display()))?;
            let audio = dir.join(format!("{side}.raw"));
            let mut command = if side == Side::Id {
                let mut command = Command::new(&built.view);
                command.arg(&script).arg(&audio).args(&engine);
                command
            } else {
                let mut command = Command::new(&host);
                // The host reads `-basedir` and `-game` as the engines write them (ADR-0008).
                command
                    .args(&engine)
                    .args(&args.host_args)
                    .arg("--view-script")
                    .arg(&script)
                    .arg(&audio);
                command
            };
            let output = command
                .current_dir(&dir)
                .output()
                .map_err(|e| format!("{side}: {e}"))?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "{side} on {map} stopped with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        };
        run(Side::Id)?;
        run(Side::Host)?;
        let mut lines = Vec::new();
        let mut ok = true;
        for shot in SHOTS {
            let read = |side: Side| {
                let path = dir.join(format!("{side}-{shot}.ppm"));
                fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
            };
            let frames = compare_frames(&read(Side::Id)?, &read(Side::Host)?);
            let (verdict, text) = match frames {
                Ok(frame) => {
                    let fits = frame.fits(shot_tolerance(shot, tolerance));
                    (
                        fits,
                        format!(
                            "{shot} difference {:.2}, brightness {:.3}",
                            frame.difference, frame.brightness
                        ),
                    )
                }
                Err(why) => (false, format!("{shot} {why}")),
            };
            ok &= verdict;
            lines.push(text);
        }
        let sound = |side: Side| {
            let path = dir.join(format!("{side}.raw"));
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let heard = compare_sound(&sound(Side::Id)?, &sound(Side::Host)?);
        let (verdict, text) = match heard {
            // VIEWDIFF-5.3: a monster in earshot idles at random, at other times in each.
            _ if monsters => (true, "sound not compared: a monster in earshot".to_owned()),
            Sound::Silent => (true, "both silent".to_owned()),
            Sound::OneSilent => (false, "one is silent".to_owned()),
            Sound::Short => (
                false,
                "a recording is shorter than three seconds".to_owned(),
            ),
            Sound::Difference(difference) => (
                difference <= tolerance.sound,
                format!("sound difference {difference:.3}"),
            ),
        };
        ok &= verdict;
        lines.push(text);
        println!(
            "{}  view-diff {map}: {}",
            if ok { "ok  " } else { "FAIL" },
            lines.join(", ")
        );
        passed &= ok;
    }
    Ok(passed)
}

/// The engine words with `-basedir`'s directory made absolute: both engines run in the map's
/// own directory under `target/view-diff`.
fn absolute(engine: &[String]) -> Result<Vec<String>, String> {
    let mut words = engine.to_vec();
    let mut iter = words.iter_mut();
    while let Some(word) = iter.next() {
        if word == "-basedir"
            && let Some(dir) = iter.next()
        {
            *dir = std::path::absolute(dir.as_str())
                .map_err(|e| format!("{dir}: {e}"))?
                .to_string_lossy()
                .into_owned();
        }
    }
    Ok(words)
}

/// Each route's shots: the start's view, the view behind it, the main menu over the start's
/// view and the console down over it (VIEWDIFF-5.6).
const SHOTS: [&str; 4] = ["ahead", "behind", "menu", "console"];

/// The tolerances a shot is held to: the menu's and the console's own, the map's for the view
/// (VIEWDIFF-5.2, VIEWDIFF-5.5).
fn shot_tolerance(shot: &str, map: Tolerance) -> Tolerance {
    match shot {
        "menu" => MENU_TOLERANCE,
        "console" => CONSOLE_TOLERANCE,
        _ => map,
    }
}

/// The frames the console takes to come down to half the 240-line frame: 120 lines at
/// `scr_conspeed`'s 300 a second is 0.4 s, so half a second (CON-3.1).
const CONSOLE_FRAMES: u32 = 36;
/// The menu's shot's tolerances (VIEWDIFF-5.5): the host draws the menu in QuakeSpasm's look,
/// the view behind it faded by black at 0.5 (REND-13.8), where id's software engine fades it
/// darker; on LibreQuake's maps the shot measured differences of 20 to 28.5 and brightnesses of
/// 1.18 to 1.56 on Linux and macOS.
const MENU_TOLERANCE: Tolerance = Tolerance {
    frame: 32.0,
    brightness: 0.6,
    sound: 0.0,
};
/// The console's shot's tolerances (VIEWDIFF-5.5): the host draws the console in QuakeSpasm's
/// look, its background at `scr_conalpha` 0.5 and its text at the frame's size (CON-5,
/// REND-13.8), where id's engine draws it opaque; differences of 7.8 to 19.3 and brightnesses of
/// 0.75 to 1.05 were measured.
const CONSOLE_TOLERANCE: Tolerance = Tolerance {
    frame: 22.0,
    brightness: 0.3,
    sound: 0.0,
};

/// The frames after the menu and the console close and before the sound's frames, so that the
/// menu's sounds are over: a second (VIEWDIFF-5.6).
const QUIET_FRAMES: u32 = 72;

/// The engine a script is for: id's, through the view oracle, or the preview host. Its name
/// starts each of its files' names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// id's engine, the view oracle.
    Id,
    /// The preview host.
    Host,
}

impl Side {
    /// The name its files start with.
    const fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Host => "qs",
        }
    }

    /// The line its script starts with after the map, to draw without the map's fog: the
    /// host's `fog 0`, and nothing for id's engine, which has no fog (VIEWDIFF-5.6).
    const fn fog_off(self) -> &'static str {
        match self {
            Self::Id => "",
            Self::Host => NO_FOG,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The host's line that turns off the fog a map's worldspawn sets, which id's engine does not
/// draw (VIEWDIFF-5.6, REND-10.5).
const NO_FOG: &str = "fog 0\n";

/// The script of `map`'s route for `side` (VIEWDIFF-5.6): the host's fog off,
/// settle, then a shot at the start's angle and one the other way, the main menu opened over
/// the start's view and shot, the console brought down over it and shot, both closed, a second
/// of quiet, then the frames for the sound. Each shot follows a `place`, after which alone the
/// oracle draws (VIEWDIFF-2.4).
fn route(map: &str, (origin, yaw): ([f32; 3], f32), side: Side) -> String {
    let fog = side.fog_off();
    let [x, y, z] = origin;
    let behind = (yaw + 180.0).rem_euclid(360.0);
    let ahead = format!("place {x} {y} {z} 0 {yaw}\nframes {PLACE_FRAMES}\n");
    format!(
        "map {map}\n{fog}frames {SETTLE_FRAMES}\n\
         {ahead}shot {side}-ahead.ppm\n\
         place {x} {y} {z} 0 {behind}\nframes {PLACE_FRAMES}\nshot {side}-behind.ppm\n\
         {ahead}togglemenu\nframes {PLACE_FRAMES}\nshot {side}-menu.ppm\ntogglemenu\n\
         {ahead}toggleconsole\nframes {CONSOLE_FRAMES}\nshot {side}-console.ppm\n\
         toggleconsole\nframes {QUIET_FRAMES}\n\
         frames {SOUND_FRAMES}\n"
    )
}

/// The origin and yaw of `map`'s first `info_player_start`, read through the game the engine
/// words name (VIEWDIFF-5.6), and whether a monster stands in earshot of it (VIEWDIFF-5.3).
fn player_start(engine: &[String], map: &str) -> Result<(([f32; 3], f32), bool), String> {
    let vfs = Vfs::new(StdFs, engine, ".").map_err(|e| format!("{e:?}"))?;
    let name = format!("maps/{map}.bsp");
    let bytes = vfs
        .load(name.as_bytes())
        .map_err(|e| format!("{name}: {e:?}"))?
        .map(LoadedFile::into_bytes)
        .ok_or_else(|| format!("{name} is not on the game's search path"))?;
    let bsp =
        qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::default()).map_err(|e| e.message(&name))?;
    let text = String::from_utf8_lossy(bsp.entities()).into_owned();
    let start = start_of(&text).ok_or_else(|| format!("{name} has no info_player_start"))?;
    Ok((start, monster_near(&text, start.0)))
}

/// How far a monster's idle sound carries: `SND_NOMINAL_CLIP_DIST` over an attenuation of 1,
/// the least an idle sound has (SND-3.2, VIEWDIFF-5.3).
const EARSHOT: f32 = 1000.0;

/// Whether an entity of the text `text` whose class starts `monster_` stands within
/// [`EARSHOT`] of `origin` (VIEWDIFF-5.3).
fn monster_near(text: &str, origin: [f32; 3]) -> bool {
    entities(text).any(|pairs| {
        let value = |key: &str| {
            pairs
                .chunks(2)
                .find(|pair| pair.first() == Some(&key))
                .and_then(|pair| pair.get(1).copied())
        };
        value("classname").is_some_and(|class| class.starts_with("monster_"))
            && value("origin").and_then(vector).is_some_and(|at| {
                let [dx, dy, dz] = [at[0] - origin[0], at[1] - origin[1], at[2] - origin[2]];
                (dx * dx + dy * dy + dz * dz).sqrt() < EARSHOT
            })
    })
}

/// Each entity of the text `text` as its quoted words, keys and values in turn.
fn entities(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.split('{').skip(1).filter_map(|entity| {
        let body = entity.split('}').next()?;
        Some(body.split('"').skip(1).step_by(2).collect())
    })
}

/// The three numbers of `text`, if it is three numbers.
fn vector(text: &str) -> Option<[f32; 3]> {
    let numbers: Vec<f32> = text
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    <[f32; 3]>::try_from(numbers).ok()
}

/// The first `info_player_start` of the entity text `text`: its `origin`, and its `angle`, 0
/// without one.
fn start_of(text: &str) -> Option<([f32; 3], f32)> {
    for pairs in entities(text) {
        let value = |key: &str| {
            pairs
                .chunks(2)
                .find(|pair| pair.first() == Some(&key))
                .and_then(|pair| pair.get(1).copied())
        };
        if value("classname") != Some("info_player_start") {
            continue;
        }
        let origin = vector(value("origin")?)?;
        let yaw = value("angle").and_then(|a| a.parse().ok()).unwrap_or(0.0);
        return Some((origin, yaw));
    }
    None
}

/// The tolerances table of `doc` (VIEWDIFF-5.2): map name or `*`, the three numbers, and why.
fn parse_tolerances(doc: &str) -> Result<Vec<(String, Tolerance)>, String> {
    let start = doc
        .find(BEGIN)
        .ok_or_else(|| format!("{TOLERANCES_DOC} has no {BEGIN}"))?;
    let end = doc
        .find(END)
        .ok_or_else(|| format!("{TOLERANCES_DOC} has no {END}"))?;
    let table = doc.get(start..end).unwrap_or_default();
    let mut rows = Vec::new();
    for line in table.lines().filter(|line| line.starts_with('|')).skip(2) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let [map, frame, brightness, sound, _why] = <[&str; 5]>::try_from(cells)
            .map_err(|_| format!("{TOLERANCES_DOC}: a row has not five cells: {line}"))?;
        let number = |cell: &str| {
            cell.parse::<f64>()
                .map_err(|_| format!("{TOLERANCES_DOC}: not a number: {cell}"))
        };
        rows.push((
            map.trim_matches('`').to_owned(),
            Tolerance {
                frame: number(frame)?,
                brightness: number(brightness)?,
                sound: number(sound)?,
            },
        ));
    }
    Ok(rows)
}

/// The tolerance of `map`: its row, else the `*` row's (VIEWDIFF-5.2).
fn tolerance_of(rows: &[(String, Tolerance)], map: &str) -> Result<Tolerance, String> {
    rows.iter()
        .find(|(name, _)| name == map)
        .or_else(|| rows.iter().find(|(name, _)| name == "*"))
        .map(|(_, tolerance)| *tolerance)
        .ok_or_else(|| format!("{TOLERANCES_DOC} has no row for {map} and no `*` row"))
}

/// How two frames compare (VIEWDIFF-3.2).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frames {
    /// The mean absolute difference of the blocks' luma, 0 to 255.
    difference: f64,
    /// Our mean luma over the oracle's.
    brightness: f64,
}

impl Frames {
    /// Whether they are within `tolerance` (VIEWDIFF-3.3).
    fn fits(self, tolerance: Tolerance) -> bool {
        self.difference <= tolerance.frame && (self.brightness - 1.0).abs() <= tolerance.brightness
    }
}

/// The oracle's frame `id` against ours, `qs`, both binary PPMs (VIEWDIFF-3).
fn compare_frames(id: &[u8], qs: &[u8]) -> Result<Frames, String> {
    let id = blocks(id).ok_or("the oracle's frame is not a 320 by 240 binary PPM")?;
    let qs = blocks(qs).ok_or("our frame is not a 320 by 240 binary PPM")?;
    let count = count_f64(id.len());
    let difference = id.iter().zip(&qs).map(|(a, b)| (a - b).abs()).sum::<f64>() / count;
    let id_mean = id.iter().sum::<f64>() / count;
    let qs_mean = qs.iter().sum::<f64>() / count;
    let brightness = if id_mean > 0.0 {
        qs_mean / id_mean
    } else if qs_mean > 0.0 {
        f64::INFINITY
    } else {
        1.0
    };
    Ok(Frames {
        difference,
        brightness,
    })
}

/// The header both engines write before a frame's pixels (VIEWDIFF-1.4, VIEWDIFF-2.1).
const PPM_HEADER: &[u8] = b"P6\n320 240\n255\n";
/// The blocks across and down a frame: 320 and 240 over [`BLOCK`] (VIEWDIFF-3.1).
const COLUMNS: usize = 80;
const ROWS: usize = 60;
/// The bytes of one row of pixels.
const ROW_BYTES: usize = 320 * 3;

/// The 4 by 4 blocks' mean luma of a 320 by 240 binary PPM, rows top first (VIEWDIFF-3.1);
/// `None` for any other file.
fn blocks(ppm: &[u8]) -> Option<Vec<f64>> {
    let pixels = ppm.strip_prefix(PPM_HEADER)?;
    let (width, height) = SIZE;
    if pixels.len() != ROW_BYTES.checked_mul(height)? || width.checked_mul(3)? != ROW_BYTES {
        return None;
    }
    let mut sums = vec![0.0; COLUMNS.checked_mul(ROWS)?];
    for (y, row) in pixels.as_chunks::<ROW_BYTES>().0.iter().enumerate() {
        let (rgb, _) = row.as_chunks::<3>();
        for (x, &[r, g, b]) in rgb.iter().enumerate() {
            let luma = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
            let at = y
                .checked_div(BLOCK)?
                .checked_mul(COLUMNS)?
                .checked_add(x.checked_div(BLOCK)?)?;
            *sums.get_mut(at)? += luma;
        }
    }
    let per = count_f64(BLOCK.checked_mul(BLOCK)?);
    Some(sums.into_iter().map(|sum| sum / per).collect())
}

/// How two recordings compare (VIEWDIFF-4).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sound {
    /// Neither has a sound (VIEWDIFF-4.2).
    Silent,
    /// One has none and the other has (VIEWDIFF-4.2).
    OneSilent,
    /// One recording is shorter than the windows compared (VIEWDIFF-4.1).
    Short,
    /// The mean of the windows' absolute differences over the oracle's mean (VIEWDIFF-4.3).
    Difference(f64),
}

/// The oracle's recording `id` against ours, `qs`, 16-bit little-endian stereo (VIEWDIFF-4).
fn compare_sound(id: &[u8], qs: &[u8]) -> Sound {
    let id = envelope(id);
    let qs = envelope(qs);
    if id.len() < LISTENED || qs.len() < LISTENED {
        return Sound::Short;
    }
    let silent = |levels: &[f64]| levels.iter().all(|&level| level == 0.0);
    let (id, qs) = (id.as_slice(), qs.as_slice());
    let (id, qs) = (
        if silent(id) { &[][..] } else { id },
        if silent(qs) { &[][..] } else { qs },
    );
    match (id.is_empty(), qs.is_empty()) {
        (true, true) => Sound::Silent,
        (true, false) | (false, true) => Sound::OneSilent,
        (false, false) => {
            // VIEWDIFF-4.3: the least difference over the shifts of up to two windows either
            // way, ours against theirs.
            let mut least = f64::INFINITY;
            let ours = (0..=MOST_SHIFT).map(|n| (id, qs.get(n..).unwrap_or_default()));
            let theirs = (1..=MOST_SHIFT).map(|n| (id.get(n..).unwrap_or_default(), qs));
            for (id, qs) in ours.chain(theirs) {
                let both = id.len().min(qs.len());
                let difference: f64 = id.iter().zip(qs).map(|(a, b)| (a - b).abs()).sum();
                let level: f64 = id.iter().take(both).sum();
                let relative = if level > 0.0 { difference / level } else { 0.0 };
                least = least.min(relative);
            }
            Sound::Difference(least)
        }
    }
}

/// The samples of a quarter of a second, both channels: 2756 frames of 11025 a second
/// (VIEWDIFF-4.1).
const WINDOW: usize = 2756 * 2;

/// The windows compared: the route's last three seconds, its sound frames (VIEWDIFF-4.1).
const LISTENED: usize = 12;
/// The most windows one recording is shifted against the other (VIEWDIFF-4.3).
const MOST_SHIFT: usize = 2;

/// The root mean square of each quarter of a second of `raw`, both channels, over its last
/// [`LISTENED`] windows (VIEWDIFF-4.1).
fn envelope(raw: &[u8]) -> Vec<f64> {
    let (pairs, _) = raw.as_chunks::<2>();
    let samples: Vec<f64> = pairs
        .iter()
        .map(|&pair| f64::from(i16::from_le_bytes(pair)))
        .collect();
    let levels: Vec<f64> = samples
        .as_chunks::<WINDOW>()
        .0
        .iter()
        .map(|w| (w.iter().map(|s| s * s).sum::<f64>() / count_f64(WINDOW)).sqrt())
        .collect();
    let skip = levels.len().saturating_sub(LISTENED);
    levels.into_iter().skip(skip).collect()
}

/// A count as a float: the counts here are far below 2^53.
#[expect(
    clippy::cast_precision_loss,
    reason = "a pixel or sample count, far below f64's exact range"
)]
fn count_f64(n: usize) -> f64 {
    n as f64
}

#[cfg(test)]
mod tests;
