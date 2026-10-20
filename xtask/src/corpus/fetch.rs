// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask corpus fetch`: downloads codebases and maps by checksum.
//!
//! A codebase is its source repository's archive at the pinned commit, kept as
//! `<cache>/downloads/<source>-<commit>.tar.gz`; from GitLab only the codebase's directory is
//! downloaded, kept as `<source>-<commit>-<path>.tar.gz` with `_` for `/`. Only the files under
//! the codebase's `path` are read from it, and every pinned checksum under that path must match
//! before anything is written to `<cache>/codebase/<id>/`; so from Quake-Tools only
//! `qcc/v101qc/` becomes corpus, and id's compiler and engine sources stay with the oracles
//! build. A codebase with a `project` gets that file of this repository as its `quakec.toml`.
//! A tree writes each of its codebases to its directory under `<cache>/tree/<id>/`. A map is one
//! pinned file, kept as `<cache>/map/<id>.map` and checked against its SHA-256. Anything already
//! in the cache with the right content is not downloaded again, so a second run works offline.
//! An entry marked `never` is refused.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use super::archive::read_tar_gz;
use super::manifest::{Entry, Manifest, Source};
use super::safe_member;
use super::verify::{archive_scope, archive_url, raw_url};
use crate::download::Net;
use crate::oracles::sha256::hex_digest;

/// Fetches the codebases, maps and trees named in `ids`, or every usable one when `ids` is
/// empty.
pub(crate) fn cmd_fetch(m: &Manifest, dest: &Path, ids: &[String], net: &dyn Net) -> Vec<String> {
    let ids: Vec<String> = if ids.is_empty() {
        let entries = m.codebase.iter().chain(&m.map).filter(|e| e.usable());
        let entries = entries.map(|e| e.id.clone());
        entries.chain(m.tree.iter().map(|t| t.id.clone())).collect()
    } else {
        ids.to_vec()
    };
    let mut errs = Vec::new();
    for id in &ids {
        let table = m.entry(id).map(|(table, _)| table);
        let fetched = match table {
            Some("map") => fetch_map(m, id, dest, net).map(|path| {
                println!("map {id}: {}", path.display());
            }),
            None if m.tree(id).is_some() => write_tree(m, id, dest, net).map(|(dir, count)| {
                println!("tree {id}: {count} files in {}", dir.display());
            }),
            _ => write_codebase(m, id, dest, net).map(|(dir, count)| {
                println!("codebase {id}: {count} files in {}", dir.display());
            }),
        };
        if let Err(e) = fetched {
            errs.push(e);
        }
    }
    errs
}

/// The entry `id` of `table`, refused when it is unknown or marked `never`.
fn usable<'m>(m: &'m Manifest, table: &str, id: &str) -> Result<(&'m Entry, &'m Source), String> {
    let Some((found, e)) = m.entry(id) else {
        return Err(format!("{table} {id}: no such entry in the manifest"));
    };
    if !e.usable() {
        return Err(format!(
            "{found} {id}: use = never, so it is never fetched ({})",
            e.reason
        ));
    }
    if found != table {
        return Err(format!("{table} {id}: is a {found}, not a {table}"));
    }
    let sid = e.source.as_deref().unwrap_or_default();
    let src = m
        .source(sid)
        .ok_or_else(|| format!("{table} {id}: unknown source {sid:?}"))?;
    Ok((e, src))
}

/// Every file of codebase `id` at its source's pinned commit, by path relative to the
/// codebase's directory. Every pinned checksum under that directory is checked.
pub(crate) fn codebase_files(
    m: &Manifest,
    id: &str,
    dest: &Path,
    net: &dyn Net,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let (e, src) = usable(m, "codebase", id)?;
    let base = base_dir(e);
    let scope = archive_scope(&src.url, &base);
    let name = if scope.is_empty() {
        format!("{}-{}.tar.gz", src.id, src.commit)
    } else {
        format!(
            "{}-{}-{}.tar.gz",
            src.id,
            src.commit,
            scope.replace('/', "_")
        )
    };
    let archive = dest.join("downloads").join(name);
    if archive.is_file()
        && let Ok(files) = read_codebase(&archive, src, &base)
    {
        return Ok(files);
    }
    let url =
        archive_url(&src.url, &src.commit, scope).map_err(|e| format!("codebase {id}: {e}"))?;
    let part = archive.with_extension("part");
    let downloaded = net.download(&url, &part);
    if let Err(e) = downloaded {
        remove_partial(&part);
        return Err(format!("codebase {id}: download of {url} failed: {e}"));
    }
    match read_codebase(&part, src, &base) {
        Ok(files) => {
            fs::rename(&part, &archive).map_err(|e| format!("{}: {e}", archive.display()))?;
            Ok(files)
        }
        Err(e) => {
            remove_partial(&part);
            Err(format!("codebase {id}: {e}"))
        }
    }
}

/// The codebase's directory in its source, with a trailing `/`, or empty for the whole
/// repository.
pub(crate) fn base_dir(e: &Entry) -> String {
    match e.path.as_deref().unwrap_or(".") {
        "." => String::new(),
        path => format!("{}/", path.trim_end_matches('/')),
    }
}

/// Reads the files under `base` from a repository archive and checks the pinned ones.
fn read_codebase(
    archive: &Path,
    src: &Source,
    base: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    // The archive's top directory is `<repo>-<commit>`; paths below it are the repository's.
    let in_repo = |name: &str| name.split_once('/').map_or("", |(_, rel)| rel).to_owned();
    let keep = |name: &str| in_repo(name).starts_with(base);
    let entries = read_tar_gz(BufReader::new(file), &keep)
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut files = BTreeMap::new();
    for (name, data) in entries {
        files.insert(in_repo(&name), data);
    }
    for (p, want) in &src.checksums {
        if !p.starts_with(base) {
            continue;
        }
        let got = files.get(p).map(|d| hex_digest(d));
        if got.as_deref() != Some(want.as_str()) {
            return Err(format!(
                "{}:{p}: sha256 {}, manifest says {want}",
                src.id,
                got.as_deref().unwrap_or("(missing from the archive)")
            ));
        }
    }
    Ok(files
        .into_iter()
        .filter_map(|(p, d)| Some((p.get(base.len()..)?.to_owned(), d)))
        .collect())
}

/// Writes every file of codebase `id` to `<dest>/codebase/<id>/`; returns the directory and
/// the number of files.
pub(crate) fn write_codebase(
    m: &Manifest,
    id: &str,
    dest: &Path,
    net: &dyn Net,
) -> Result<(PathBuf, usize), String> {
    let dir = dest.join("codebase").join(id);
    let files = checked_files(m, id, dest, net)?;
    let count = write_files(&dir, &files)?;
    Ok((dir, count))
}

/// Every file of codebase `id`, its `project` file as `quakec.toml` included, by path relative
/// to its directory. Every member is checked before any is written, so a refused archive
/// writes nothing.
fn checked_files(
    m: &Manifest,
    id: &str,
    dest: &Path,
    net: &dyn Net,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = codebase_files(m, id, dest, net)?;
    if let Some(rel) = files.keys().find(|rel| !safe_member(rel)) {
        return Err(format!(
            "codebase {id}: {rel:?} is not a safe relative path"
        ));
    }
    if let Some(project) = m.entry(id).and_then(|(_, e)| e.project.as_deref()) {
        let path = m.dir.join(project);
        let data =
            fs::read(&path).map_err(|e| format!("codebase {id}: {}: {e}", path.display()))?;
        files.insert("quakec.toml".to_owned(), data);
    }
    Ok(files)
}

/// Writes `files` under `dir`; returns how many.
fn write_files(dir: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<usize, String> {
    for (rel, data) in files {
        write_file(&dir.join(rel), data)?;
    }
    Ok(files.len())
}

/// Writes each codebase of tree `id` to its directory under `<dest>/tree/<id>/`; returns the
/// tree's directory and the number of files.
pub(crate) fn write_tree(
    m: &Manifest,
    id: &str,
    dest: &Path,
    net: &dyn Net,
) -> Result<(PathBuf, usize), String> {
    let tree = m
        .tree(id)
        .ok_or_else(|| format!("tree {id}: no such tree in the manifest"))?;
    let root = dest.join("tree").join(id);
    if let Some(sub) = tree.dirs.keys().find(|sub| !safe_member(sub)) {
        return Err(format!("tree {id}: {sub:?} is not a safe relative path"));
    }
    // Every codebase is fetched and checked before any is written, so a refused one leaves
    // no part of the tree behind.
    let mut checked = Vec::new();
    for (sub, codebase) in &tree.dirs {
        let files = checked_files(m, codebase, dest, net).map_err(|e| format!("tree {id}: {e}"))?;
        checked.push((root.join(sub), files));
    }
    let mut count = 0_usize;
    for (dir, files) in &checked {
        count = count.saturating_add(write_files(dir, files)?);
    }
    Ok((root, count))
}

/// Downloads map `id` to `<dest>/map/<id>.map` unless it is already there with its pinned
/// SHA-256.
fn fetch_map(m: &Manifest, id: &str, dest: &Path, net: &dyn Net) -> Result<PathBuf, String> {
    let (e, src) = usable(m, "map", id)?;
    let path = e.path.as_deref().unwrap_or_default();
    let want = src
        .checksums
        .get(path)
        .ok_or_else(|| format!("map {id}: {path} has no pinned checksum"))?;
    let out = dest.join("map").join(format!("{id}.map"));
    if fs::read(&out).is_ok_and(|bytes| hex_digest(&bytes) == *want) {
        return Ok(out);
    }
    let url = raw_url(&src.url, &src.commit, path).map_err(|e| format!("map {id}: {e}"))?;
    let bytes = net
        .get(&url)
        .map_err(|e| format!("map {id}: download of {url} failed: {e}"))?;
    let got = hex_digest(&bytes);
    if got != *want {
        return Err(format!(
            "map {id}: {url}: sha256 {got}, manifest says {want}"
        ));
    }
    write_file(&out, &bytes)?;
    Ok(out)
}

/// Removes a partial download after a failure. One that was never created is already gone;
/// any other failure to remove it is reported, and the next run writes over it.
pub(crate) fn remove_partial(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("warning: {}: {e}", path.display()),
    }
}

/// Writes `data` to `path` through a temporary file, so an interrupted run leaves no partial
/// file under the final name.
pub(crate) fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    fs::write(&part, data).map_err(|e| format!("{}: {e}", part.display()))?;
    fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub(crate) mod tests;
