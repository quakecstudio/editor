// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask assets fetch | precache`: the open test assets in `corpus/manifest.toml`
//! (QA-4, LIC-4). `docs/test-assets.md` has the license review and the policy.
//!
//! `fetch` downloads each usable `[[asset]]` archive into `<cache>/downloads/<sha256>.zip`,
//! checks its SHA-256, extracts only the pinned members (each checked against its own SHA-256)
//! to `<cache>/<id>/`, checks the result has what a headless engine scenario needs, and writes
//! each stand-in as a loose file. A cached archive or member with the right SHA-256 is not
//! fetched or written again, so a second run works offline.
//!
//! `precache` lists every `precache_model` and `precache_sound` call in each codebase an asset
//! `covers` and fails when a precached file is neither in the asset's pak files nor given a
//! stand-in.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use qs_pak::Pak;

use super::archive::Zip;
use super::fetch::{base_dir, codebase_files, remove_partial, write_file};
use super::manifest::{Entry, Manifest};
use super::safe_member;
use crate::download::Net;
use crate::oracles::sha256::hex_digest;

/// Files an engine needs to start a map with no id data: the palette and colormap, the HUD
/// wad, and the map a scenario loads first.
const SCENARIO_FILES: [&str; 4] = [
    "gfx/palette.lmp",
    "gfx/colormap.lmp",
    "gfx.wad",
    "maps/start.bsp",
];
/// At least one of each kind of content a scenario exercises: the kind, a prefix and a suffix.
const SCENARIO_KINDS: [(&str, &str, &str); 3] = [
    ("map", "maps/", ".bsp"),
    ("model", "progs/", ".mdl"),
    ("sound", "sound/", ".wav"),
];

/// Where one file in a game directory's paks is.
pub(crate) struct PakFile {
    pak: PathBuf,
    index: usize,
}

/// Downloads, checks and extracts every usable asset.
pub(crate) fn cmd_fetch(m: &Manifest, dest: &Path, net: &dyn Net) -> Vec<String> {
    let mut errs = Vec::new();
    for e in m.asset.iter().filter(|e| e.usable()) {
        let fetched = fetch_archive(e, dest, net)
            .and_then(|archive| extract(e, &archive, dest))
            .and_then(|out| {
                let game_dir = out.join(&e.game_dir);
                let files = pak_files(&game_dir)?;
                let names: BTreeSet<&str> = files.keys().map(String::as_str).collect();
                errs.extend(
                    check_scenario(&names)
                        .into_iter()
                        .map(|x| format!("asset {}: {x}", e.id)),
                );
                install_stand_ins(e, &game_dir, &files)?;
                Ok(out)
            });
        match fetched {
            Ok(out) => println!(
                "asset {}: {} files in {}",
                e.id,
                e.members.len(),
                out.display()
            ),
            Err(x) => errs.push(format!("asset {}: {x}", e.id)),
        }
    }
    errs
}

/// The asset's archive in the download cache, downloaded only if missing or wrong.
fn fetch_archive(e: &Entry, dest: &Path, net: &dyn Net) -> Result<PathBuf, String> {
    let archive = dest.join("downloads").join(format!("{}.zip", e.sha256));
    if fs::read(&archive).is_ok_and(|bytes| hex_digest(&bytes) == e.sha256) {
        return Ok(archive);
    }
    let part = archive.with_extension("part");
    let downloaded = net.download(&e.url, &part);
    if let Err(x) = downloaded {
        remove_partial(&part);
        return Err(format!("download of {} failed: {x}", e.url));
    }
    let got = fs::read(&part)
        .map(|bytes| hex_digest(&bytes))
        .map_err(|x| format!("{}: {x}", part.display()))?;
    if got != e.sha256 {
        remove_partial(&part);
        return Err(format!(
            "{}: sha256 {got}, manifest says {}",
            e.url, e.sha256
        ));
    }
    fs::rename(&part, &archive).map_err(|x| format!("{}: {x}", archive.display()))?;
    Ok(archive)
}

/// Extracts the pinned members, checking each SHA-256; returns the asset's directory.
fn extract(e: &Entry, archive: &Path, dest: &Path) -> Result<PathBuf, String> {
    let out = dest.join(&e.id);
    // Opened only when a member is not already extracted.
    let mut zip: Option<Zip> = None;
    for (name, want) in &e.members {
        let rel = name
            .strip_prefix(e.strip.as_str())
            .filter(|rel| safe_member(name) && safe_member(rel))
            .ok_or_else(|| format!("member {name:?} is not a safe path under {:?}", e.strip))?;
        let target = out.join(rel);
        if fs::read(&target).is_ok_and(|bytes| hex_digest(&bytes) == *want) {
            continue;
        }
        let zip = match &mut zip {
            Some(zip) => zip,
            None => zip.insert(
                File::open(archive)
                    .and_then(Zip::open)
                    .map_err(|x| format!("{}: {x}", archive.display()))?,
            ),
        };
        let data = zip
            .read(name)
            .map_err(|x| format!("{}: {name}: {x}", archive.display()))?
            .ok_or_else(|| format!("{}: no member {name}", archive.display()))?;
        let got = hex_digest(&data);
        if got != *want {
            return Err(format!("{name}: sha256 {got}, manifest says {want}"));
        }
        write_file(&target, &data)?;
    }
    Ok(out)
}

/// Every file in the game directory's `pak0.pak`, `pak1.pak`, ... in the order the engine
/// reads them, so a later pak's copy of a name wins.
pub(crate) fn pak_files(game_dir: &Path) -> Result<BTreeMap<String, PakFile>, String> {
    let mut files = BTreeMap::new();
    for i in 0_u32.. {
        let path = game_dir.join(format!("pak{i}.pak"));
        if !path.is_file() {
            break;
        }
        let pak = open_pak(&path)?;
        for (index, entry) in pak.entries().iter().enumerate() {
            entry
                .range(pak.file_len())
                .map_err(|x| format!("{}: entry {index} {x}", path.display()))?;
            let name: String = entry.name().iter().map(|&b| char::from(b)).collect();
            files.insert(
                name,
                PakFile {
                    pak: path.clone(),
                    index,
                },
            );
        }
    }
    Ok(files)
}

fn open_pak(path: &Path) -> Result<Pak, String> {
    let mut file = File::open(path).map_err(|x| format!("{}: {x}", path.display()))?;
    Pak::read_from(&mut file).map_err(|x| format!("{} {x}", path.display()))
}

/// Writes each stand-in as a loose file under the name the QuakeC precaches, copied from the
/// pak file that has the stand-in, so the engine finds it.
fn install_stand_ins(
    e: &Entry,
    game_dir: &Path,
    files: &BTreeMap<String, PakFile>,
) -> Result<(), String> {
    for (name, stand_in) in &e.stand_ins {
        let found = files
            .get(stand_in)
            .ok_or_else(|| format!("stand-in {stand_in} for {name} is not in the paks"))?;
        let pak = open_pak(&found.pak)?;
        let mut file =
            File::open(&found.pak).map_err(|x| format!("{}: {x}", found.pak.display()))?;
        let data = pak
            .entries()
            .get(found.index)
            .ok_or_else(|| format!("{}: no entry {}", found.pak.display(), found.index))
            .and_then(|entry| {
                pak.read_entry(&mut file, entry)
                    .map_err(|x| format!("{} {x}", found.pak.display()))
            })?;
        write_file(&game_dir.join(name), &data)?;
    }
    Ok(())
}

/// What a headless scenario needs is present: a map, models, textures and sounds.
fn check_scenario(names: &BTreeSet<&str>) -> Vec<String> {
    let mut errs: Vec<String> = SCENARIO_FILES
        .iter()
        .filter(|f| !names.contains(*f))
        .map(|f| format!("missing {f}"))
        .collect();
    for (kind, prefix, suffix) in SCENARIO_KINDS {
        if !names
            .iter()
            .any(|n| n.starts_with(prefix) && n.ends_with(suffix))
        {
            errs.push(format!("no {kind} ({prefix}*{suffix})"));
        }
    }
    errs
}

/// Whether a precache call loads a model or a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Model,
    Sound,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Sound => "sound",
        }
    }
}

/// One `precache_model` or `precache_sound` call; `name` is `None` when the argument is not a
/// string literal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Precache {
    kind: Kind,
    name: Option<String>,
    file: String,
    line: usize,
    arg: String,
}

impl Precache {
    /// The file the engine loads: a model by its own path, a sound under `sound/`.
    fn path(&self) -> Option<String> {
        let name = self.name.as_deref()?;
        Some(match self.kind {
            Kind::Model => name.to_owned(),
            Kind::Sound => format!("sound/{name}"),
        })
    }
}

/// The precache calls in QuakeC source, left to right. Strings and comments are skipped
/// whole, so a call inside one is not counted. Names match in any case, since some compilers
/// accept any case, and the `2` variants (`precache_model2`) count too.
fn precache_calls(file: &str, text: &str) -> Vec<Precache> {
    let bytes = text.as_bytes();
    let mut calls = Vec::new();
    let mut at = 0_usize;
    let mut line = 1_usize;
    while let Some(&b) = bytes.get(at) {
        let rest = bytes.get(at..).unwrap_or_default();
        let skip = if b == b'"' {
            string_len(rest)
        } else if rest.starts_with(b"//") {
            rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len())
        } else if rest.starts_with(b"/*") {
            find(rest.get(2..).unwrap_or_default(), b"*/")
                .map_or(rest.len(), |end| end.saturating_add(4))
        } else if let Some((call, len)) = precache_at(bytes, at) {
            calls.push(Precache {
                kind: call.0,
                name: call.1,
                file: file.to_owned(),
                line,
                arg: call.2,
            });
            len
        } else {
            1
        };
        let skip = skip.max(1);
        let end = at.saturating_add(skip).min(bytes.len());
        let newlines = bytes
            .get(at..end)
            .unwrap_or_default()
            .iter()
            .filter(|&&c| c == b'\n')
            .count();
        line = line.saturating_add(newlines);
        at = end;
    }
    calls
}

/// The length of the string literal at the start of `text`, with its quotes: a backslash
/// escapes the next character, and a newline or the end of input ends an unterminated one.
fn string_len(text: &[u8]) -> usize {
    let mut i = 1_usize;
    while let Some(&c) = text.get(i) {
        match c {
            b'"' => return i.saturating_add(1),
            b'\n' => return i,
            b'\\' => i = i.saturating_add(2),
            _ => i = i.saturating_add(1),
        }
    }
    text.len()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A precache call's kind, its literal name, and its argument as written.
type Call = (Kind, Option<String>, String);

/// The precache call starting at `at`, if one does, and its length up to and including the
/// `)`.
fn precache_at(bytes: &[u8], at: usize) -> Option<(Call, usize)> {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if at > 0 && bytes.get(at.checked_sub(1)?).is_some_and(|&b| word(b)) {
        return None;
    }
    let rest = bytes.get(at..)?;
    let prefix = rest.get(..9)?;
    if !prefix.eq_ignore_ascii_case(b"precache_") {
        return None;
    }
    let after = rest.get(9..)?;
    let (kind, mut i) = if after.get(..5)?.eq_ignore_ascii_case(b"model") {
        (Kind::Model, 14)
    } else if after.get(..5)?.eq_ignore_ascii_case(b"sound") {
        (Kind::Sound, 14)
    } else {
        return None;
    };
    if rest.get(i) == Some(&b'2') {
        i = i.checked_add(1)?;
    }
    let skip_space = |mut i: usize| {
        while rest.get(i).is_some_and(u8::is_ascii_whitespace) {
            i = i.saturating_add(1);
        }
        i
    };
    i = skip_space(i);
    if rest.get(i) != Some(&b'(') {
        return None;
    }
    i = skip_space(i.checked_add(1)?);
    let open = i;
    let close = open.checked_add(rest.get(open..)?.iter().position(|&b| b == b')')?)?;
    let arg = String::from_utf8_lossy(rest.get(open..close)?)
        .trim()
        .to_owned();
    let literal = arg
        .strip_prefix('"')
        .and_then(|a| a.strip_suffix('"'))
        .filter(|a| !a.contains('"'))
        .map(str::to_owned);
    if literal.is_none() && arg.contains('"') {
        return None;
    }
    Some(((kind, literal, arg), close.checked_add(1)?))
}

/// The coverage of `calls` by the files in `names`: precached paths missing, paths served by
/// a stand-in, and calls whose argument is not a literal.
fn coverage<'c>(
    calls: &'c [Precache],
    names: &BTreeSet<&str>,
    stand_ins: &BTreeMap<String, String>,
) -> (BTreeSet<String>, BTreeSet<String>, Vec<&'c Precache>) {
    let mut missing = BTreeSet::new();
    let mut standing_in = BTreeSet::new();
    let mut dynamic = Vec::new();
    for c in calls {
        let Some(p) = c.path() else {
            dynamic.push(c);
            continue;
        };
        if names.contains(p.as_str()) {
            continue;
        }
        if stand_ins
            .get(&p)
            .is_some_and(|s| names.contains(s.as_str()))
        {
            standing_in.insert(p);
        } else {
            missing.insert(p);
        }
    }
    (missing, standing_in, dynamic)
}

/// Checks every usable asset against the precaches of each codebase it covers.
pub(crate) fn cmd_precache(m: &Manifest, dest: &Path, net: &dyn Net) -> Vec<String> {
    let mut errs = Vec::new();
    for e in m.asset.iter().filter(|e| e.usable()) {
        let game_dir = dest.join(&e.id).join(&e.game_dir);
        if !game_dir.is_dir() {
            errs.push(format!(
                "asset {}: {} does not exist; run cargo xtask assets fetch first",
                e.id,
                game_dir.display()
            ));
            continue;
        }
        let files = match pak_files(&game_dir) {
            Ok(files) => files,
            Err(x) => {
                errs.push(format!("asset {}: {x}", e.id));
                continue;
            }
        };
        let names: BTreeSet<&str> = files.keys().map(String::as_str).collect();
        for cid in &e.covers {
            match covers(m, e, cid, &names, dest, net) {
                Ok(missing) => errs.extend(
                    missing
                        .into_iter()
                        .map(|p| format!("asset {}: {cid} precaches {p}, which is missing", e.id)),
                ),
                Err(x) => errs.push(format!("asset {}: {x}", e.id)),
            }
        }
    }
    errs
}

/// Prints how the asset covers codebase `cid`'s precaches; returns the paths it lacks.
fn covers(
    m: &Manifest,
    e: &Entry,
    cid: &str,
    names: &BTreeSet<&str>,
    dest: &Path,
    net: &dyn Net,
) -> Result<BTreeSet<String>, String> {
    let base = m
        .codebase
        .iter()
        .find(|c| c.id == cid)
        .map(base_dir)
        .unwrap_or_default();
    let files = codebase_files(m, cid, dest, net)?;
    let mut sources = 0_usize;
    let mut calls = Vec::new();
    for (rel, data) in &files {
        if rel.to_lowercase().ends_with(".qc") {
            sources = sources.saturating_add(1);
            let text: String = data.iter().map(|&b| char::from(b)).collect();
            calls.extend(precache_calls(&format!("{base}{rel}"), &text));
        }
    }
    let (missing, standing_in, dynamic) = coverage(&calls, names, &e.stand_ins);
    let literal: BTreeSet<String> = calls.iter().filter_map(Precache::path).collect();
    let present = literal
        .len()
        .saturating_sub(missing.len())
        .saturating_sub(standing_in.len());
    println!(
        "asset {} covers {cid}: {sources} .qc files, {} precache calls, {} files, {present} \
         present, {} by stand-in, {} missing",
        e.id,
        calls.len(),
        literal.len(),
        standing_in.len(),
        missing.len()
    );
    for p in &standing_in {
        let stand_in = e.stand_ins.get(p).map_or("", String::as_str);
        println!("  stand-in: {p} -> {stand_in}");
    }
    for c in dynamic {
        println!(
            "  not a literal: {}:{} precache_{}({})",
            c.file,
            c.line,
            c.kind.name(),
            c.arg
        );
    }
    Ok(missing)
}

#[cfg(test)]
mod tests;
