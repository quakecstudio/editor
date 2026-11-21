// SPDX-License-Identifier: GPL-3.0-or-later

//! The model tool oracle (L-47, MDL-8): id's modelgen, built unchanged from `qutils/` of
//! Quake-Tools with the system C compiler, its files pinned with the map tools' in
//! `oracles/maps.toml`. `cargo xtask oracles models` builds it and gives it and `qs-mdl`'s
//! `Grab::build` the same scripts, each made from a seed: a base of triangles, frames of the
//! same triangles moved, a skin or two, and now and then an origin, a scale, an eye position,
//! a group of frames, a triangle of no area, a group with no frame and a frame shown for no
//! time or less. Each must write the same model, or both refuse the script.
//!
//! Two kinds of byte modelgen writes are whatever its stack held, and are not compared: what
//! follows the NUL of a frame's name, and the `lightnormalindex` of a frame's and a group's
//! bounds (MDL-7.3).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qs_mdl::mdl::grab::{Corners, Grab, GrabEntry, GrabFrame, MDL_GRAB_INTERVAL, Picture};
use qs_mdl::mdl::write::{Model, ModelFrame, ModelFrameEntry};
use qs_mdl::mdl::{MDL_ST_RAND, MDL_ST_SYNC};

use super::compile::{Code, Compiler, Instrument, Unit};
use super::rng::Rng;
use super::{OUT, maps, stem};
use crate::download::Net;
use crate::exec::Exec;

/// modelgen's own directory in `qutils/`, lower case as the build holds it.
const TOOL: &str = "modelgen";
/// qutils' library files modelgen links.
const LIBS: [&str; 5] = [
    "common/cmdlib.c",
    "common/mathlib.c",
    "common/scriplib.c",
    "common/trilib.c",
    "common/lbmlib.c",
];
/// Where the scripts are run, under `target/oracles`: modelgen sets its directories from the
/// script's absolute path as qbsp does (`SetQdirFromPath`).
const WORK: [&str; 3] = ["models-work", "quake", "id1"];
/// The word `SetQdirFromPath` looks for in the script's path.
const QUAKE: &str = "quake";
/// The longest word of a script, `MAXTOKEN` in `SCRIPLIB.H`, with its NUL.
const MAX_TOKEN: usize = 128;
/// The width and height of the picture modelgen cuts a skin from, "LBMs are always loaded as
/// 320x200 bitmaps".
const PICTURE: [u16; 2] = [320, 200];
/// The magic number of Alias's triangle file and the floats that start and end an object,
/// `MAGIC`, `FLOAT_START` and `FLOAT_END` in `TRILIB.C`.
const TRI_MAGIC: i32 = 123_322;
const TRI_START: f32 = 99_999.0;
const TRI_END: f32 = -TRI_START;
/// The floats of a point of a triangle file before and after its position: a normal before,
/// a colour and a texture coordinate after (`aliaspoint_t`).
const POINT_BEFORE: usize = 3;
const POINT_AFTER: usize = 5;
/// How many scripts are compared when the command line gives no number.
pub(crate) const ROUNDS: u32 = 1000;
/// How many differing scripts are shown before the run stops.
const SHOWN: usize = 8;
/// The steps of a unit that most generated positions are on.
const STEPS: f32 = 16.0;
/// What a position off those steps is multiplied by.
const OFF_STEP: f32 = 1.000_123;
/// The widest models made, in units, of which one is drawn.
const SPANS: [i16; 3] = [4, 30, 200];
/// The scales made.
const SCALES: [f32; 4] = [0.5, 1.25, 3.0, 0.7];

/// A coordinate within `span` units of 0: on a sixteenth, or one time in four a little off.
fn coord(rng: &mut Rng, span: i16) -> f32 {
    let reach = usize::try_from(span).unwrap_or(1).saturating_mul(32);
    let steps = i16::try_from(rng.below(reach.saturating_add(1))).unwrap_or(0);
    let whole = f32::from(steps.saturating_sub(span.saturating_mul(16))) / STEPS;
    if rng.chance(25) {
        whole * OFF_STEP
    } else {
        whole
    }
}

/// A position within `span` units of the origin on each axis.
fn point(rng: &mut Rng, span: i16) -> [f32; 3] {
    [coord(rng, span), coord(rng, span), coord(rng, span)]
}

/// The script of `seed`.
fn case(seed: u64) -> Grab {
    let mut rng = Rng(seed.wrapping_mul(2).wrapping_add(1));
    let span = SPANS.get(rng.below(SPANS.len())).copied().unwrap_or(4);
    let pool: Vec<[f32; 3]> = (0..rng.below(12).saturating_add(4))
        .map(|_| point(&mut rng, span))
        .collect();
    let picks: Vec<[usize; 3]> = (0..rng.below(18).saturating_add(2))
        .map(|_| {
            let a = rng.below(pool.len());
            let mut b = rng.below(pool.len());
            let c = rng.below(pool.len());
            // Now and then a triangle of no area is left as it came.
            if b == a && !rng.chance(8) {
                b = a.saturating_add(1).checked_rem(pool.len()).unwrap_or(0);
            }
            [a, b, c]
        })
        .collect();
    let soup = |pool: &[[f32; 3]]| -> Vec<Corners> {
        picks
            .iter()
            .map(|corners| corners.map(|at| pool.get(at).copied().unwrap_or_default()))
            .collect()
    };
    let mut named = 0_u32;
    let mut frame = |rng: &mut Rng| {
        named = named.saturating_add(1);
        let moved: Vec<[f32; 3]> = pool
            .iter()
            .map(|[x, y, z]| {
                let [dx, dy, dz] = point(rng, span.checked_div(4).unwrap_or(1).max(1));
                [x + dx, y + dy, z + dz]
            })
            .collect();
        GrabFrame {
            name: format!("f{named}").into_bytes(),
            triangles: soup(&moved),
        }
    };
    let mut frames = Vec::new();
    for _ in 0..rng.below(3).saturating_add(1) {
        if rng.chance(33) {
            // Now and then a group has no frame, or a frame shown for no time or less.
            let count = if rng.chance(4) {
                0
            } else {
                rng.below(3).saturating_add(1)
            };
            let group = (0..count)
                .map(|_| {
                    let interval = if rng.chance(50) {
                        MDL_GRAB_INTERVAL
                    } else if rng.chance(8) {
                        f32::from(i8::try_from(rng.below(2)).unwrap_or(0).saturating_neg()) / 4.0
                    } else {
                        f32::from(u8::try_from(rng.below(40)).unwrap_or(0).saturating_add(5))
                            / 100.0
                    };
                    (frame(&mut rng), interval)
                })
                .collect();
            frames.push(GrabEntry::Group(group));
        } else {
            frames.push(GrabEntry::Single(frame(&mut rng)));
        }
    }
    let [width, height] = PICTURE.map(usize::from);
    let skins = (0..rng.below(2).saturating_add(1))
        .map(|skin| Picture {
            width,
            pixels: (0..width.saturating_mul(height))
                .map(|at| {
                    let colour = at.wrapping_mul(7).wrapping_add(skin.wrapping_mul(13));
                    colour.to_le_bytes().first().copied().unwrap_or(0)
                })
                .collect(),
        })
        .collect();
    let plain = rng.chance(33);
    let small = span.checked_div(8).unwrap_or(1).max(1);
    Grab {
        base: soup(&pool),
        skins,
        frames,
        origin: if plain {
            [0.0; 3]
        } else {
            point(&mut rng, small)
        },
        scale: if plain {
            1.0
        } else {
            SCALES.get(rng.below(SCALES.len())).copied().unwrap_or(1.0)
        },
        eye_position: if plain {
            [0.0; 3]
        } else {
            point(&mut rng, small)
        },
        flags: i32::try_from(rng.below(9)).unwrap_or(0),
        synctype: if rng.chance(50) {
            MDL_ST_SYNC
        } else {
            MDL_ST_RAND
        },
    }
}

/// A count as a file's big-endian `int`.
fn count(of: usize) -> [u8; 4] {
    i32::try_from(of).unwrap_or(i32::MAX).to_be_bytes()
}

/// `triangles` as one object of an Alias triangle file, as `LoadTriangleList` reads it. An
/// empty group follows the object: at the end of the file the reader reads again whatever
/// count it last read, and a group's is 0.
fn tri_file(triangles: &[Corners]) -> Vec<u8> {
    let mut out = TRI_MAGIC.to_be_bytes().to_vec();
    out.extend(TRI_START.to_be_bytes());
    out.extend(b"object\0");
    out.extend(count(triangles.len()));
    out.extend(b"texture\0");
    for corner in triangles.iter().flatten() {
        out.extend([0_u8; 4].repeat(POINT_BEFORE));
        for part in corner {
            out.extend(part.to_be_bytes());
        }
        out.extend([0_u8; 4].repeat(POINT_AFTER));
    }
    out.extend(TRI_END.to_be_bytes());
    out.extend(b"object\0");
    out.extend(TRI_START.to_be_bytes());
    out.extend(b"group\0");
    out.extend(count(0));
    out.extend(TRI_END.to_be_bytes());
    out.extend(b"group\0");
    out
}

/// `pixels`, a picture of [`PICTURE`]'s size, as a `PBM ` form with no compression, as
/// `LoadLBM` reads it.
fn lbm(pixels: &[u8]) -> Vec<u8> {
    let [width, height] = PICTURE;
    let chunk = |name: &[u8; 4], body: &[u8]| {
        let mut out = name.to_vec();
        out.extend(count(body.len()));
        out.extend(body);
        out
    };
    // w, h, x, y, nPlanes, masking, compression, pad, transparentColor, the aspect, the page.
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 1, 1]);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    let palette: Vec<u8> = (0..=u8::MAX).flat_map(|shade| [shade; 3]).collect();
    let mut form = b"PBM ".to_vec();
    form.extend(chunk(b"BMHD", &header));
    form.extend(chunk(b"CMAP", &palette));
    form.extend(chunk(b"BODY", pixels));
    chunk(b"FORM", &form)
}

/// Every frame of `grab`, a group's in turn.
fn frames(grab: &Grab) -> impl Iterator<Item = &GrabFrame> {
    grab.frames.iter().flat_map(|entry| match entry {
        GrabEntry::Single(frame) => vec![frame],
        GrabEntry::Group(frames) => frames.iter().map(|(frame, _)| frame).collect(),
    })
}

/// modelgen's script for `grab`, whose files are in `dir`. `$cd` is `dir` after what
/// `SetQdirFromPath` takes for the Quake directory, the path through the first `quake` and
/// the byte after it: a `$cd` that is no absolute path is put after that.
fn script(grab: &Grab, dir: &Path) -> Result<String, String> {
    let full = dir.display().to_string();
    let cd = full
        .to_ascii_lowercase()
        .find(QUAKE)
        .and_then(|at| full.get(at.saturating_add(QUAKE.len()).saturating_add(1)..))
        .filter(|cd| cd.len() < MAX_TOKEN)
        .ok_or_else(|| format!("{full}: no short path under a quake directory"))?;
    let name = |frame: &GrabFrame| String::from_utf8_lossy(&frame.name).into_owned();
    let mut out = format!("$modelname thing\n$cd {cd}\n");
    if grab.synctype == MDL_ST_SYNC {
        out.push_str("$sync\n");
    }
    let [ox, oy, oz] = grab.origin;
    let [ex, ey, ez] = grab.eye_position;
    let _infallible = write!(
        out,
        "$flags {}\n$origin {ox} {oy} {oz}\n$eyeposition {ex} {ey} {ez}\n$scale {}\n$base base\n",
        grab.flags, grab.scale
    );
    for skin in 0..grab.skins.len() {
        let _infallible = writeln!(out, "$skin skin{skin}");
    }
    for entry in &grab.frames {
        match entry {
            GrabEntry::Single(frame) => {
                let _infallible = writeln!(out, "$frame {}", name(frame));
            }
            GrabEntry::Group(frames) => {
                out.push_str("$framegroupstart\n");
                for (frame, interval) in frames {
                    let _infallible = writeln!(out, "$frame {} {interval}", name(frame));
                }
                out.push_str("$framegroupend\n");
            }
        }
    }
    Ok(out)
}

/// Writes `grab`'s files to `dir`, emptied first, and runs modelgen on them: the model it
/// wrote, or `None` when it stopped.
fn theirs(
    exec: &dyn Exec,
    tool: &Path,
    grab: &Grab,
    dir: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let failed = |path: &Path| {
        let path = path.to_path_buf();
        move |error| format!("{}: {error}", path.display())
    };
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(failed(dir))?;
    }
    fs::create_dir_all(dir).map_err(failed(dir))?;
    let mut files = vec![("base.tri".to_owned(), tri_file(&grab.base))];
    for (skin, picture) in grab.skins.iter().enumerate() {
        files.push((format!("skin{skin}.lbm"), lbm(&picture.pixels)));
    }
    for frame in frames(grab) {
        let name = String::from_utf8_lossy(&frame.name);
        files.push((format!("{name}.tri"), tri_file(&frame.triangles)));
    }
    files.push(("thing.qc".to_owned(), script(grab, dir)?.into_bytes()));
    for (name, bytes) in files {
        let path = dir.join(name);
        fs::write(&path, bytes).map_err(failed(&path))?;
    }
    let output = exec
        .output(
            Command::new(tool)
                .arg(dir.join("thing.qc"))
                .current_dir(dir),
        )
        .map_err(|e| format!("cannot run {}: {e}", tool.display()))?;
    Ok(fs::read(dir.join("thing.mdl"))
        .ok()
        .filter(|_| output.status.success()))
}

/// `frame` without the bytes modelgen leaves as its stack had them.
fn settle(frame: &mut ModelFrame) -> Result<(), String> {
    let name = frame.name().to_vec();
    frame
        .set_name(&name)
        .map_err(|e| format!("modelgen's frame name: {e}"))?;
    frame.bboxmin.lightnormalindex = 0;
    frame.bboxmax.lightnormalindex = 0;
    Ok(())
}

/// modelgen's file `bytes`, read and written back without the bytes it leaves unset.
fn settled(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut model = Model::read(bytes).map_err(|e| format!("modelgen's model: {e}"))?;
    for entry in &mut model.frames {
        match entry {
            ModelFrameEntry::Single(frame) => settle(frame)?,
            ModelFrameEntry::Group {
                bboxmin,
                bboxmax,
                frames,
                ..
            } => {
                bboxmin.lightnormalindex = 0;
                bboxmax.lightnormalindex = 0;
                for frame in frames {
                    settle(frame)?;
                }
            }
        }
    }
    model
        .to_bytes()
        .map_err(|e| format!("modelgen's model: {e}"))
}

/// Builds id's modelgen unless it is there, from the files `oracles/maps.toml` pins.
fn build(root: &Path, exec: &dyn Exec, net: &dyn Net) -> Result<PathBuf, String> {
    let out = root.join(OUT);
    let objects = out.join("build").join("models");
    let bin = out.join("bin");
    for dir in [&objects, &bin] {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tool = bin.join(Compiler::executable(TOOL));
    let lower = maps::qutils(root, &objects, net)?;
    let compiler = Compiler::find(&objects, Instrument::None, exec)?;
    let compat = root.join(maps::COMPAT);
    let includes = [lower.join("common"), lower.join(TOOL)];
    let sources = std::iter::once(format!("{TOOL}/{TOOL}.c"))
        .chain(LIBS.map(str::to_owned))
        .map(|file| lower.join(file));
    let mut linked = Vec::new();
    for source in sources {
        let object = objects.join(format!("{TOOL}_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Upstream,
                includes: &includes,
                defines: &[],
                forced_include: Some(&compat),
            },
            &object,
        )?;
        linked.push(object);
    }
    compiler.link(&linked, &tool)?;
    Ok(tool)
}

/// Builds id's modelgen and compares it with `qs-mdl` on `rounds` scripts.
pub(crate) fn check(
    root: &Path,
    exec: &dyn Exec,
    net: &dyn Net,
    rounds: u32,
) -> Result<bool, String> {
    if cfg!(windows) {
        return Err("the model tool oracle builds on Linux and macOS only".to_owned());
    }
    let tool = build(root, exec, net)?;
    let dir = WORK
        .iter()
        .fold(root.join(OUT), |path, part| path.join(part));
    let (mut same, mut refused, mut differ) = (0_u32, 0_u32, 0_usize);
    for seed in 0..u64::from(rounds) {
        let grab = case(seed);
        let theirs = theirs(exec, &tool, &grab, &dir)?;
        let ours = grab.build().map(|model| model.to_bytes());
        match (theirs, ours) {
            (None, Err(_)) => refused = refused.saturating_add(1),
            (Some(theirs), Ok(Ok(ours))) if settled(&theirs)? == ours => {
                same = same.saturating_add(1);
            }
            (theirs, ours) => {
                differ = differ.saturating_add(1);
                let ours = match ours {
                    Ok(Ok(bytes)) => format!("a model of {} bytes", bytes.len()),
                    Ok(Err(error)) => format!("refused: {error}"),
                    Err(error) => format!("refused: {error}"),
                };
                let theirs = theirs.map_or_else(
                    || "refused".to_owned(),
                    |bytes| format!("a model of {} bytes", bytes.len()),
                );
                println!("FAIL  seed {seed}: modelgen {theirs}, qs-mdl {ours}");
                if differ >= SHOWN {
                    break;
                }
            }
        }
    }
    let passed = differ == 0 && same > 0;
    println!(
        "{}  modelgen and qs-mdl: the same model for {same} of {rounds} scripts, {refused} \
         refused by both, {differ} differ",
        if passed { "ok  " } else { "FAIL" }
    );
    Ok(passed)
}

#[cfg(test)]
mod tests;
