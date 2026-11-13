// SPDX-License-Identifier: GPL-3.0-or-later

//! The map tool oracles (P6-03, MAP-11): id's qbsp, vis and light, built unchanged from
//! `qutils/` of Quake-Tools with the system C compiler, and ericw-tools' qbsp, vis and light,
//! built with CMake from its pinned commit and the submodules that commit records. Every input
//! is pinned by commit and SHA-256 in `oracles/maps.toml`; none is committed or shipped
//! (LIC-7). `cargo xtask oracles maps` builds them and compiles `oracles/maps/room.map` with each
//! set, checking each writes a BSP29 file: the map compiler differential (P6-07) runs them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use super::compile::{Code, Compiler, Instrument, Unit};
use super::{OUT, Sources, Upstream, fetch, sha256, stem};
use crate::corpus::archive::read_tar_gz;
use crate::corpus::fetch::remove_partial;
use crate::corpus::safe_member;
use crate::corpus::verify::is_hex;
use crate::download::Net;
use crate::exec::Exec;

/// The pins, relative to the repository root.
const MAPS: &str = "oracles/maps.toml";
/// The sealed room each set of tools compiles.
const ROOM: &str = "oracles/maps/room.map";
/// What qutils' files are compiled with before anything else.
const COMPAT: &str = "oracles/maps/compat.h";
/// qutils' library files every tool links.
const COMMON: [&str; 3] = ["common/cmdlib.c", "common/mathlib.c", "common/bspfile.c"];
/// The tools, as the directories of `qutils/` and of ericw-tools' build hold them.
const TOOLS: [&str; 3] = ["qbsp", "vis", "light"];
/// The version a BSP29 file starts with (BSP-1.2).
const BSP29: i32 = 29;
/// Where the room is compiled, under `target/oracles`. id's qbsp sets its directories from the
/// map's absolute path (`SetQdirFromPath` in qutils' `COMMON/CMDLIB.C`): the first `quake`
/// anywhere in it, in any case, ends the Quake directory, and the next path component is the
/// game directory. A path with no `quake` stops qbsp, so the room goes under `quake/id1`. A
/// checkout whose own path holds `quake` (`quakecstudio-editor`) still wins the search; that
/// changes only where qbsp looks for a worldspawn's `wad`, which the room has none of.
const WORK: [&str; 3] = ["maps-work", "quake", "id1"];
/// The length of a full commit hash in hexadecimal.
const COMMIT_LEN: usize = 40;
/// The length of a SHA-256 in hexadecimal.
const SHA256_LEN: usize = 64;

/// `oracles/maps.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    qutils: Upstream,
    archive: Vec<Archive>,
}

/// A repository at a commit, fetched as GitHub's gzipped tar and unpacked at `into` in
/// ericw-tools' tree. `tree` pins what is inside rather than the archive's bytes, which GitHub
/// does not promise to keep: the SHA-256 of one line `<path> <SHA-256>` per regular file, its
/// path without the archive's top directory, sorted, joined by newlines.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    id: String,
    repo: String,
    commit: String,
    tree: String,
    into: String,
}

/// An archive's regular files, by their paths without its top directory.
type Members = Vec<(String, Vec<u8>)>;

/// Where the map tool oracles are built.
pub(crate) struct MapTools {
    /// id's qbsp, vis and light, in [`TOOLS`] order.
    pub(crate) id: [PathBuf; 3],
    /// ericw-tools' qbsp, vis and light, in [`TOOLS`] order.
    pub(crate) ericw: [PathBuf; 3],
}

/// Reads and checks the pins: every commit a full hash, every archive's SHA-256 whole, and the
/// first archive ericw-tools itself, unpacked at the top of the tree.
fn read_pins(root: &Path) -> Result<Pins, String> {
    let text = fs::read_to_string(root.join(MAPS)).map_err(|e| format!("{MAPS}: {e}"))?;
    let pins: Pins = toml::from_str(&text).map_err(|e| format!("{MAPS}: {e}"))?;
    if !is_hex(&pins.qutils.commit, COMMIT_LEN) {
        return Err(format!("{MAPS}: qutils is not pinned to a full commit"));
    }
    for archive in &pins.archive {
        if !is_hex(&archive.commit, COMMIT_LEN) {
            return Err(format!(
                "{MAPS}: {} is not pinned to a full commit",
                archive.id
            ));
        }
        if !is_hex(&archive.tree, SHA256_LEN) {
            return Err(format!("{MAPS}: {} has no whole tree SHA-256", archive.id));
        }
    }
    match pins.archive.first() {
        Some(first) if first.into.is_empty() => Ok(pins),
        Some(_) | None => Err(format!(
            "{MAPS}: the first archive must be ericw-tools, unpacked at the top (into = \"\")"
        )),
    }
}

/// Builds the map tool oracles unless the stamp shows they are current, then compiles the room
/// with each set of tools.
pub(crate) fn check(root: &Path, exec: &dyn Exec, net: &dyn Net) -> Result<bool, String> {
    if cfg!(windows) {
        return Err("the map tool oracles build on Linux and macOS only".to_owned());
    }
    let tools = build(root, exec, net)?;
    let work = work_dir(root);
    let mut passed = true;
    for (name, set) in [("id", &tools.id), ("ericw-tools", &tools.ericw)] {
        let dir = work.join(name);
        passed &= compile_room(root, exec, set, &dir)?;
    }
    Ok(passed)
}

/// The directory the room is compiled in, one subdirectory per set of tools: [`WORK`] under
/// `target/oracles`.
fn work_dir(root: &Path) -> PathBuf {
    let mut work = root.join(OUT);
    work.extend(WORK);
    work
}

/// The bin directory's executables for one set of tools.
fn tool_paths(bin: &Path, prefix: &str) -> [PathBuf; 3] {
    TOOLS.map(|tool| bin.join(Compiler::executable(&format!("{prefix}{tool}"))))
}

/// Fetches and builds both sets of tools, unless every binary is there and the stamp of their
/// inputs is unchanged.
fn build(root: &Path, exec: &dyn Exec, net: &dyn Net) -> Result<MapTools, String> {
    let pins = read_pins(root)?;
    let out = root.join(OUT);
    let cache = out.join("src");
    let objects = out.join("build").join("maps");
    let bin = out.join("bin");
    for dir in [&cache, &objects, &bin] {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tools = MapTools {
        id: tool_paths(&bin, ""),
        ericw: tool_paths(&bin, "ericw-"),
    };
    let compiler = Compiler::find(&objects, Instrument::None, exec)?;
    let stamp_now = stamp(root, &compiler, exec)?;
    let stamp_file = bin.join("maps-stamp");
    if tools
        .id
        .iter()
        .chain(&tools.ericw)
        .all(|path| path.is_file())
        && fs::read_to_string(&stamp_file).is_ok_and(|s| s.trim() == stamp_now)
    {
        println!("oracles: map tools up to date in {}", bin.display());
        return Ok(tools);
    }
    build_id(&pins, &cache, &objects, &compiler, root, net, &tools)?;
    build_ericw(&pins, &cache, &objects, exec, net, &tools)?;
    fs::write(&stamp_file, stamp_now).map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    for (tool, (id, ericw)) in TOOLS.iter().zip(tools.id.iter().zip(&tools.ericw)) {
        println!(
            "oracles: {tool} at {} and {}",
            id.display(),
            ericw.display()
        );
    }
    Ok(tools)
}

/// A digest of the inputs of the map tools' build: the pins, the compat header, the C
/// compiler and CMake. The libraries ericw-tools links (TBB, Embree) are not in it: after
/// upgrading them, delete `target/oracles/bin/maps-stamp` to rebuild.
fn stamp(root: &Path, compiler: &Compiler<'_>, exec: &dyn Exec) -> Result<String, String> {
    let cmake = exec
        .output(Command::new("cmake").arg("--version"))
        .map_err(|e| format!("cannot run cmake: {e}"))?;
    let mut inputs = vec![
        compiler.describe(),
        String::from_utf8_lossy(&cmake.stdout).into_owned(),
    ];
    for file in [MAPS, COMPAT] {
        let bytes = fs::read(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        inputs.push(format!("{file} {}", sha256::hex_digest(&bytes)));
    }
    Ok(sha256::hex_digest(inputs.join("\n").as_bytes()))
}

/// id's tools: qutils' files fetched, copied to lower-case names (the files' own `#include`s
/// are lower case, the repository's names upper case), and compiled unchanged.
fn build_id(
    pins: &Pins,
    cache: &Path,
    objects: &Path,
    compiler: &Compiler<'_>,
    root: &Path,
    net: &dyn Net,
    tools: &MapTools,
) -> Result<(), String> {
    let upstream = Upstream {
        id: pins.qutils.id.clone(),
        repo: pins.qutils.repo.clone(),
        commit: pins.qutils.commit.clone(),
        files: pins.qutils.files.clone(),
    };
    let sources = Sources {
        upstream: vec![upstream],
        extract: Vec::new(),
    };
    fetch(&sources, cache, net)?;
    let fetched = cache.join(&pins.qutils.id).join("qutils");
    let lower = objects.join("qutils");
    for file in pins.qutils.files.keys() {
        let relative = file
            .strip_prefix("qutils/")
            .unwrap_or(file)
            .to_ascii_lowercase();
        let to = lower.join(&relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let from = fetched.join(file.strip_prefix("qutils/").unwrap_or(file));
        fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    let compat = root.join(COMPAT);
    let common = lower.join("common");
    for (tool, out) in TOOLS.iter().zip(&tools.id) {
        let dir = lower.join(tool);
        let includes = [common.clone(), dir.clone()];
        let mut sources: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect();
        sources.sort();
        sources.extend(COMMON.iter().map(|file| lower.join(file)));
        let mut linked = Vec::new();
        for source in &sources {
            let object = objects.join(format!("{tool}_{}.o", stem(source)));
            compiler.object(
                &Unit {
                    source,
                    code: Code::Upstream,
                    includes: &includes,
                    defines: &[],
                    forced_include: Some(&compat),
                },
                &object,
            )?;
            linked.push(object);
        }
        compiler.link(&linked, out)?;
    }
    Ok(())
}

/// The regular files of a gzipped tar, each without the archive's top directory.
///
/// # Errors
///
/// When the archive cannot be read, or names a member outside its directory.
fn members(bytes: &[u8]) -> Result<Members, String> {
    let files = read_tar_gz(bytes, &|_| true).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(files.len());
    for (name, data) in files {
        let Some((_, inside)) = name.split_once('/') else {
            continue;
        };
        if !safe_member(inside) {
            return Err(format!("{name}: not a path inside the archive"));
        }
        out.push((inside.to_owned(), data));
    }
    Ok(out)
}

/// The digest [`Archive::tree`] pins.
fn tree_digest(members: &Members) -> String {
    let mut lines: Vec<String> = members
        .iter()
        .map(|(path, data)| format!("{path} {}", sha256::hex_digest(data)))
        .collect();
    lines.sort();
    sha256::hex_digest(lines.join("\n").as_bytes())
}

/// The members of `archive`, from the cache when it holds them with the pinned tree, else
/// downloaded and checked.
fn fetch_archive(archive: &Archive, cache: &Path, net: &dyn Net) -> Result<Members, String> {
    let path = cache
        .join("archives")
        .join(format!("{}.tar.gz", archive.id));
    if let Ok(bytes) = fs::read(&path)
        && let Ok(members) = members(&bytes)
        && tree_digest(&members) == archive.tree
    {
        return Ok(members);
    }
    let url = format!(
        "https://codeload.github.com/{}/tar.gz/{}",
        archive.repo, archive.commit
    );
    let partial = path.with_extension("partial");
    let checked = net.download(&url, &partial).and_then(|()| {
        let bytes = fs::read(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        let members = members(&bytes).map_err(|e| format!("{url}: {e}"))?;
        let got = tree_digest(&members);
        if got == archive.tree {
            Ok(members)
        } else {
            Err(format!(
                "{}@{}: tree {got}, {MAPS} says {}",
                archive.repo, archive.commit, archive.tree
            ))
        }
    });
    match checked {
        Ok(members) => {
            fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("oracles: fetched {}@{}", archive.repo, archive.commit);
            Ok(members)
        }
        Err(error) => {
            remove_partial(&partial);
            Err(error)
        }
    }
}

/// Writes `members` under `into`.
fn unpack(members: Members, into: &Path) -> Result<(), String> {
    for (inside, data) in members {
        let to = into.join(inside);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&to, data).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// ericw-tools: its archive and its submodules' unpacked into one tree, configured with CMake
/// (tests, documentation and the light preview off) and its three tools built.
fn build_ericw(
    pins: &Pins,
    cache: &Path,
    objects: &Path,
    exec: &dyn Exec,
    net: &dyn Net,
    tools: &MapTools,
) -> Result<(), String> {
    let tree = objects.join("ericw-tools");
    let build = objects.join("ericw-tools-build");
    if tree.exists() {
        fs::remove_dir_all(&tree).map_err(|e| format!("{}: {e}", tree.display()))?;
    }
    for archive in &pins.archive {
        let members = fetch_archive(archive, cache, net)?;
        unpack(members, &tree.join(&archive.into))?;
    }
    let mut configure = Command::new("cmake");
    configure.arg("-S").arg(&tree).arg("-B").arg(&build).args([
        "-DCMAKE_BUILD_TYPE=Release",
        "-DDISABLE_TESTS=ON",
        "-DDISABLE_DOCS=ON",
        "-DENABLE_LIGHTPREVIEW=OFF",
    ]);
    run(exec, &mut configure, "configure ericw-tools")?;
    let jobs = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut make = Command::new("cmake");
    make.arg("--build")
        .arg(&build)
        .args(["--parallel", &jobs.to_string(), "--target"])
        .args(TOOLS);
    run(exec, &mut make, "build ericw-tools")?;
    for (tool, out) in TOOLS.iter().zip(&tools.ericw) {
        let built = build.join(tool).join(Compiler::executable(tool));
        fs::copy(&built, out).map_err(|e| format!("{}: {e}", built.display()))?;
    }
    Ok(())
}

fn run(exec: &dyn Exec, command: &mut Command, what: &str) -> Result<(), String> {
    let status = exec
        .status(command)
        .map_err(|e| format!("cannot {what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: {status}"))
    }
}

/// Compiles the room in `dir`, emptied first, with qbsp, then vis, then light of `set`, and
/// checks that each succeeds and leaves a BSP29 file.
fn compile_room(
    root: &Path,
    exec: &dyn Exec,
    set: &[PathBuf; 3],
    dir: &Path,
) -> Result<bool, String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let map = dir.join("room.map");
    fs::copy(root.join(ROOM), &map).map_err(|e| format!("{ROOM}: {e}"))?;
    let bsp = dir.join("room.bsp");
    let [qbsp, vis, light] = set;
    for (tool, input) in [(qbsp, &map), (vis, &bsp), (light, &bsp)] {
        let mut command = Command::new(tool);
        command.arg(input).current_dir(dir);
        let output = exec
            .output(&mut command)
            .map_err(|e| format!("cannot run {}: {e}", tool.display()))?;
        if !output.status.success() {
            println!(
                "FAIL  {} {}: {}\n{}{}",
                tool.display(),
                input.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(false);
        }
    }
    let version = fs::read(&bsp)
        .ok()
        .and_then(|bytes| bytes.first_chunk::<4>().copied())
        .map(i32::from_le_bytes);
    if version == Some(BSP29) {
        println!("ok    {} compiled {ROOM} to BSP29", qbsp.display());
        Ok(true)
    } else {
        println!(
            "FAIL  {} left {} with version {version:?}, not {BSP29}",
            qbsp.display(),
            bsp.display()
        );
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
