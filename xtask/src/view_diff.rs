// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask view-diff [--map <name>]... [-- <engine word>...]`: the view differential
//! (P3-28, VIEWDIFF). Each map's start plays through the view oracle, id's whole engine
//! headless, and through the preview host's `--view-script`; their frames and sound are
//! compared within the map's tolerances in `docs/spec/engine/view-diff.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qs_vfs::{LoadedFile, StdFs, Vfs};

use crate::oracles;

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
/// (VIEWDIFF-5.1).
const SETTLE_FRAMES: u32 = 216;
/// The frames after the shots, for the sound: three seconds (VIEWDIFF-5.1).
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
    let doc = fs::read_to_string(root.join(TOLERANCES_DOC))
        .map_err(|e| format!("{TOLERANCES_DOC}: {e}"))?;
    let tolerances = parse_tolerances(&doc)?;
    let built = oracles::build(root)?;
    let host = build_host(root)?;
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
        let start = player_start(&args.engine, map)?;
        let dir = out.join(map);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let run = |side: &str| -> Result<(), String> {
            let script = dir.join(format!("{side}.view"));
            fs::write(&script, route(map, start, side))
                .map_err(|e| format!("{}: {e}", script.display()))?;
            let audio = dir.join(format!("{side}.raw"));
            let mut command = if side == "id" {
                let mut command = Command::new(&built.view);
                command.arg(&script).arg(&audio).args(&args.engine);
                command
            } else {
                let mut command = Command::new(&host);
                // The host reads `-basedir` and `-game` as the engines write them (ADR-0008).
                command
                    .args(&args.engine)
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
        run("id")?;
        run("qs")?;
        let mut lines = Vec::new();
        let mut ok = true;
        for shot in SHOTS {
            let read = |side: &str| {
                let path = dir.join(format!("{side}-{shot}.ppm"));
                fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
            };
            let frames = compare_frames(&read("id")?, &read("qs")?);
            let (verdict, text) = match frames {
                Ok(frame) => {
                    let fits = frame.fits(tolerance);
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
        let sound = |side: &str| {
            let path = dir.join(format!("{side}.raw"));
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let heard = compare_sound(&sound("id")?, &sound("qs")?);
        let (verdict, text) = match heard {
            Sound::Silent => (true, "both silent".to_owned()),
            Sound::OneSilent => (false, "one is silent".to_owned()),
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

/// Each route's shots: the start's view and the view behind it.
const SHOTS: [&str; 2] = ["ahead", "behind"];

/// The script of `map`'s route for `side`, `id` or `qs` (VIEWDIFF-5.1): settle, then a shot
/// at the start's angle and one the other way, then the frames for the sound.
fn route(map: &str, (origin, yaw): ([f32; 3], f32), side: &str) -> String {
    let [x, y, z] = origin;
    let behind = (yaw + 180.0).rem_euclid(360.0);
    format!(
        "map {map}\nframes {SETTLE_FRAMES}\n\
         place {x} {y} {z} 0 {yaw}\nframes {PLACE_FRAMES}\nshot {side}-ahead.ppm\n\
         place {x} {y} {z} 0 {behind}\nframes {PLACE_FRAMES}\nshot {side}-behind.ppm\n\
         frames {SOUND_FRAMES}\n"
    )
}

/// The release preview host, built as the app ships it.
fn build_host(root: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "--locked",
            "--release",
            "--package",
            "preview-host",
        ])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot build the preview host: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build of the preview host failed: {status}"));
    }
    Ok(root
        .join("target/release")
        .join(format!("preview-host{}", std::env::consts::EXE_SUFFIX)))
}

/// The origin and yaw of `map`'s first `info_player_start`, read through the game the engine
/// words name (VIEWDIFF-5.1).
fn player_start(engine: &[String], map: &str) -> Result<([f32; 3], f32), String> {
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
    start_of(&text).ok_or_else(|| format!("{name} has no info_player_start"))
}

/// The first `info_player_start` of the entity text `text`: its `origin`, and its `angle`, 0
/// without one.
fn start_of(text: &str) -> Option<([f32; 3], f32)> {
    for entity in text.split('{').skip(1) {
        let body = entity.split('}').next()?;
        let pairs: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
        let value = |key: &str| {
            pairs
                .chunks(2)
                .find(|pair| pair.first() == Some(&key))
                .and_then(|pair| pair.get(1).copied())
        };
        if value("classname") != Some("info_player_start") {
            continue;
        }
        let numbers: Vec<f32> = value("origin")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let origin = <[f32; 3]>::try_from(numbers).ok()?;
        let yaw = value("angle").and_then(|a| a.parse().ok()).unwrap_or(0.0);
        return Some((origin, yaw));
    }
    None
}

/// The tolerances table of `doc` (VIEWDIFF-5.2): map name or `*`, then the three numbers.
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
        let [map, frame, brightness, sound] = <[&str; 4]>::try_from(cells)
            .map_err(|_| format!("{TOLERANCES_DOC}: a row has not four cells: {line}"))?;
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

/// The header both engines write before a frame's pixels (VIEWDIFF-1.2, VIEWDIFF-2.1).
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
    /// The mean of the windows' absolute differences over the oracle's mean (VIEWDIFF-4.3).
    Difference(f64),
}

/// The oracle's recording `id` against ours, `qs`, 16-bit little-endian stereo (VIEWDIFF-4).
fn compare_sound(id: &[u8], qs: &[u8]) -> Sound {
    let id = envelope(id);
    let qs = envelope(qs);
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
