// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask corpus verify`: checks `corpus/manifest.toml` and the repository against the
//! corpus policy in `corpus/README.md`.
//!
//! Offline (the default): the manifest's shape, that every entry records a license with pinned
//! evidence, that only GPL-3.0-or-later compatible items are marked `vendor`, that QuakeC 1.06
//! and id's game data are listed as never fetched (LIC-4, LIC-5), that every test asset archive
//! is pinned and carries its source's license files, and that no corpus file or game data is
//! tracked in git (LIC-7). `--online` also downloads every pinned file at its commit and
//! compares SHA-256, which checks the URLs, the commits and the checksums.

use std::collections::{BTreeMap, BTreeSet};

use super::manifest::{Entry, Manifest, Source, Tree};
use super::{MANIFEST, safe_member};
use crate::download::Net;
use crate::oracles::sha256::hex_digest;

const USES: [&str; 3] = ["vendor", "fetch-at-test", "never"];
const KINDS: [&str; 2] = ["id", "mod"];
/// Licenses whose code may be combined with GPL-3.0-or-later code, and so may be copied into
/// this repository as test fixtures.
const GPL3_COMPATIBLE: [&str; 10] = [
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MIT",
    "Apache-2.0",
    "Zlib",
    "CC0-1.0",
];
/// Quake content: a tracked file with one of these extensions is corpus or game data.
const CONTENT_EXTS: [&str; 12] = [
    ".qc", ".qh", ".src", ".map", ".bsp", ".pak", ".pk3", ".dat", ".wad", ".lmp", ".mdl", ".spr",
];
const REQUIRED_IDS: [&str; 2] = ["id-1.01", "rerelease"];
/// The never entry for id's pak0/pak1, which no tool fetches and CI never uses.
const ID_GAME_DATA: &str = "id1-paks";
const MIN_MODS: usize = 10;
const MIN_MAPS: usize = 20;

/// Offline checks of the manifest itself; returns every error.
pub(crate) fn check(m: &Manifest) -> Vec<String> {
    let mut errs = Vec::new();
    if m.schema != Some(1) {
        errs.push("schema must be 1".to_owned());
    }
    let sources = check_sources(m, &mut errs);
    let mut seen = BTreeSet::new();
    for (table, e) in m.entries() {
        let at = format!("{table} {}", e.id);
        if !seen.insert(e.id.as_str()) {
            errs.push(format!("{at}: duplicate id"));
        }
        check_entry(table, e, &at, &mut errs);
        if table == "asset" {
            if e.usable() {
                check_asset(e, &at, &sources, &mut errs);
            }
        } else if e.use_ == "vendor" || e.use_ == "fetch-at-test" {
            check_pins(table, e, &at, &sources, &mut errs);
        }
        if let Some(project) = &e.project
            && (table != "codebase" || !safe_member(project))
        {
            errs.push(format!(
                "{at}: project {project:?} must be a relative path, on a codebase"
            ));
        }
    }
    for t in &m.tree {
        if !seen.insert(t.id.as_str()) {
            errs.push(format!("tree {}: duplicate id", t.id));
        }
        check_tree(m, t, &mut errs);
    }
    check_policy(m, &mut errs);
    errs
}

/// A tree lays usable codebases out in separate directories, none inside another.
fn check_tree(m: &Manifest, t: &Tree, errs: &mut Vec<String>) {
    let at = format!("tree {}", t.id);
    if !is_id(&t.id) {
        errs.push(format!("{at}: bad id"));
    }
    if t.dirs.is_empty() {
        errs.push(format!("{at}: no dirs"));
    }
    for (dir, codebase) in &t.dirs {
        if !safe_member(dir) {
            errs.push(format!("{at}: {dir:?} must be a relative directory"));
        }
        let inside = |other: &String| other != dir && dir.starts_with(&format!("{other}/"));
        if let Some(outer) = t.dirs.keys().find(|other| inside(other)) {
            errs.push(format!("{at}: {dir} is inside {outer}"));
        }
        if !m.codebase.iter().any(|e| e.id == *codebase && e.usable()) {
            errs.push(format!("{at}: {dir}: no usable codebase {codebase}"));
        }
    }
}

fn check_sources<'m>(m: &'m Manifest, errs: &mut Vec<String>) -> BTreeMap<&'m str, &'m Source> {
    let mut sources = BTreeMap::new();
    let blocked: Vec<String> = m
        .blocked_urls
        .iter()
        .map(|u| u.trim_end_matches('/').to_lowercase())
        .collect();
    for s in &m.source {
        let sid = &s.id;
        if !is_id(sid) {
            errs.push(format!("source {sid:?}: bad id"));
        }
        if sources.insert(sid.as_str(), s).is_some() {
            errs.push(format!("source {sid}: duplicate id"));
        }
        if host_project(&s.url).is_none() {
            errs.push(format!(
                "source {sid}: url must be https://github.com/o/r or https://gitlab.com/o/r"
            ));
        }
        let url = s.url.trim_end_matches('/').to_lowercase();
        if blocked.iter().any(|b| url.starts_with(b.as_str())) {
            errs.push(format!("source {sid}: {} is blocked", s.url));
        }
        if !is_hex(&s.commit, 40) {
            errs.push(format!(
                "source {sid}: commit must be a full 40-character SHA"
            ));
        }
        if s.checksums.is_empty() {
            errs.push(format!("source {sid}: no pinned checksums"));
        }
        for (p, h) in &s.checksums {
            if !is_hex(h, 64) {
                errs.push(format!("source {sid}: {p}: bad sha256"));
            }
        }
    }
    sources
}

/// Fields every entry needs, and what each `use` requires.
fn check_entry(table: &str, e: &Entry, at: &str, errs: &mut Vec<String>) {
    if !is_id(&e.id) {
        errs.push(format!("{at}: bad id"));
    }
    if !is_license(&e.license) {
        errs.push(format!(
            "{at}: license must be an SPDX id, LicenseRef-* or NONE"
        ));
    }
    if !USES.contains(&e.use_.as_str()) {
        errs.push(format!("{at}: use must be one of {}", USES.join(", ")));
    }
    if table == "codebase" && !KINDS.contains(&e.kind.as_str()) {
        errs.push(format!("{at}: kind must be one of {}", KINDS.join(", ")));
    }
    if e.use_ == "never" {
        if e.source.is_some() || e.path.is_some() {
            errs.push(format!(
                "{at}: a never entry must not name a source or path"
            ));
        }
        if e.reason.is_empty() {
            errs.push(format!("{at}: a never entry needs a reason"));
        }
    } else if e.use_ == "vendor" && !GPL3_COMPATIBLE.contains(&e.license.as_str()) {
        errs.push(format!(
            "{at}: {} is not GPL-3.0-or-later compatible, so use cannot be vendor",
            e.license
        ));
    }
    if table == "asset" && e.use_ == "vendor" {
        errs.push(format!(
            "{at}: assets are binary game data, so use must be fetch-at-test or never"
        ));
    }
}

/// A fetched codebase or map names a pinned source, and its evidence (and a map's file) is
/// checksummed.
fn check_pins(
    table: &str,
    e: &Entry,
    at: &str,
    sources: &BTreeMap<&str, &Source>,
    errs: &mut Vec<String>,
) {
    let source = e.source.as_deref().unwrap_or_default();
    let Some(src) = sources.get(source) else {
        errs.push(format!("{at}: unknown source {source:?}"));
        return;
    };
    let path = e.path.as_deref().unwrap_or_default();
    if path.is_empty() {
        errs.push(format!("{at}: missing path"));
    }
    if table == "map" {
        if !path.ends_with(".map") {
            errs.push(format!("{at}: path must be a .map file"));
        } else if !src.checksums.contains_key(path) {
            errs.push(format!(
                "{at}: {path} has no pinned checksum in source {}",
                src.id
            ));
        }
    }
    if e.evidence.is_empty() {
        errs.push(format!("{at}: no license evidence"));
    }
    for ev in &e.evidence {
        let (other, p) = ev.rsplit_once(':').unwrap_or(("", ev));
        let esrc = if other.is_empty() {
            Some(*src)
        } else {
            sources.get(other).copied()
        };
        match esrc {
            None => errs.push(format!("{at}: evidence {ev}: unknown source")),
            Some(esrc) if !esrc.checksums.contains_key(p) => {
                errs.push(format!("{at}: evidence {ev} has no pinned checksum"));
            }
            Some(_) => {}
        }
    }
}

/// A test asset is a pinned release archive of a pinned source. Every member that is extracted
/// is pinned too, and the source's license evidence is among them, which ties the archive to
/// the license files read at the pinned commit.
fn check_asset(e: &Entry, at: &str, sources: &BTreeMap<&str, &Source>, errs: &mut Vec<String>) {
    let source = e.source.as_deref().unwrap_or_default();
    let Some(src) = sources.get(source) else {
        errs.push(format!("{at}: unknown source {source:?}"));
        return;
    };
    if release_repo(&e.url) != Some(src.url.as_str()) {
        errs.push(format!(
            "{at}: url must be a .zip release download of {}",
            src.url
        ));
    }
    if !is_hex(&e.sha256, 64) {
        errs.push(format!("{at}: bad sha256"));
    }
    if !safe_member(&e.game_dir) {
        errs.push(format!(
            "{at}: game_dir must be a relative directory such as id1"
        ));
    }
    if e.members.is_empty() {
        errs.push(format!("{at}: no pinned members"));
    }
    for (p, h) in &e.members {
        if !safe_member(p) || !p.starts_with(e.strip.as_str()) || *p == e.strip {
            errs.push(format!(
                "{at}: member {p:?} must be a safe path under {:?}",
                e.strip
            ));
        }
        if !is_hex(h, 64) {
            errs.push(format!("{at}: member {p}: bad sha256"));
        }
    }
    if e.evidence.is_empty() {
        errs.push(format!("{at}: no license evidence"));
    }
    for ev in &e.evidence {
        match src.checksums.get(ev) {
            None => errs.push(format!("{at}: evidence {ev} has no pinned checksum")),
            Some(want) if !e.members.values().any(|h| h == want) => errs.push(format!(
                "{at}: evidence {ev} is not among the archive's pinned members"
            )),
            Some(_) => {}
        }
    }
    for (name, stand_in) in &e.stand_ins {
        if !(safe_member(name) && safe_member(stand_in)) {
            errs.push(format!(
                "{at}: stand-in {name:?} = {stand_in:?} must be relative paths"
            ));
        }
    }
}

/// The never list matches the never entries, and the corpus meets its minimums.
fn check_policy(m: &Manifest, errs: &mut Vec<String>) {
    let never: BTreeSet<&str> = m.never.iter().map(String::as_str).collect();
    let never_entries: BTreeSet<&str> = m
        .entries()
        .filter(|(_, e)| !e.usable())
        .map(|(_, e)| e.id.as_str())
        .collect();
    for id in never.difference(&never_entries) {
        errs.push(format!("never list: {id} has no entry with use = never"));
    }
    for id in never_entries.difference(&never) {
        errs.push(format!("{id}: use = never but missing from the never list"));
    }
    if !never.contains("quakec-1.06") {
        errs.push("never list must include quakec-1.06 (LIC-5)".to_owned());
    }
    if !never.contains(ID_GAME_DATA) {
        errs.push(format!("never list must include {ID_GAME_DATA} (LIC-4)"));
    }
    let usable: BTreeSet<&str> = m
        .codebase
        .iter()
        .filter(|e| e.usable())
        .map(|e| e.id.as_str())
        .collect();
    for id in REQUIRED_IDS {
        if !usable.contains(id) {
            errs.push(format!("missing required codebase {id}"));
        }
    }
    let assets: Vec<&Entry> = m.asset.iter().filter(|e| e.usable()).collect();
    for e in &assets {
        for c in &e.covers {
            if !usable.contains(c.as_str()) {
                errs.push(format!("asset {}: covers unknown codebase {c}", e.id));
            }
        }
    }
    if !assets
        .iter()
        .any(|e| e.covers.iter().any(|c| c == "rerelease"))
    {
        errs.push("no test asset covers the rerelease codebase's precaches (QA-4)".to_owned());
    }
    let mods = m
        .codebase
        .iter()
        .filter(|e| e.kind == "mod" && e.usable())
        .count();
    if mods < MIN_MODS {
        errs.push(format!("{mods} usable mods; need at least {MIN_MODS}"));
    }
    let maps = m.map.iter().filter(|e| e.usable()).count();
    if maps < MIN_MAPS {
        errs.push(format!("{maps} usable maps; need at least {MIN_MAPS}"));
    }
}

/// Nothing from the corpus, and no game data, is tracked in the repository. `read` gives a
/// tracked file's bytes.
pub(crate) fn check_repo(
    m: &Manifest,
    files: &[String],
    read: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Vec<String> {
    let mut errs = Vec::new();
    let fixture_dirs: Vec<String> = m
        .fixture_dirs
        .iter()
        .map(|d| format!("{}/", d.trim_end_matches('/')))
        .collect();
    let mut pinned_content = BTreeMap::new();
    for s in &m.source {
        for (p, h) in &s.checksums {
            if is_content(p) {
                pinned_content.insert(h.as_str(), format!("{}:{p}", s.id));
            }
        }
    }
    for f in files {
        if extension(f).as_deref() == Some(".pak") {
            errs.push(format!("{f}: pak files (game data) are never committed"));
            continue;
        }
        if !is_content(f) {
            continue;
        }
        if !fixture_dirs.iter().any(|d| f.starts_with(d.as_str())) {
            errs.push(format!(
                "{f}: Quake content outside the manifest's fixture_dirs"
            ));
        }
        match read(f) {
            Ok(bytes) => {
                if let Some(corpus) = pinned_content.get(hex_digest(&bytes).as_str()) {
                    errs.push(format!("{f}: is a copy of corpus file {corpus}"));
                }
            }
            Err(e) => errs.push(format!("{f}: {e}")),
        }
    }
    let manifest_dir = MANIFEST.rsplit_once('/').map_or("", |(dir, _)| dir);
    for e in &m.codebase {
        if let Some(project) = &e.project {
            let path = format!("{manifest_dir}/{project}");
            if !files.contains(&path) {
                errs.push(format!(
                    "codebase {}: project {path} is not tracked in git",
                    e.id
                ));
            }
        }
    }
    errs
}

/// Downloads every pinned file at its commit and compares SHA-256.
pub(crate) fn check_online(m: &Manifest, net: &dyn Net) -> Vec<String> {
    let mut errs = Vec::new();
    for s in &m.source {
        for (p, want) in &s.checksums {
            let fetched = raw_url(&s.url, &s.commit, p).and_then(|url| net.get(&url));
            match fetched {
                Ok(bytes) => {
                    let got = hex_digest(&bytes);
                    if got != *want {
                        errs.push(format!(
                            "source {}: {p}: sha256 {got}, manifest says {want}",
                            s.id
                        ));
                    }
                }
                Err(e) => errs.push(format!("source {}: {p}: fetch failed: {e}", s.id)),
            }
        }
    }
    errs
}

/// Where a source's host serves one file at a commit.
pub(crate) fn raw_url(url: &str, commit: &str, path: &str) -> Result<String, String> {
    match host_project(url) {
        Some(("github.com", project)) => Ok(format!(
            "https://raw.githubusercontent.com/{project}/{commit}/{}",
            quote(path, true)
        )),
        Some(("gitlab.com", project)) => Ok(format!(
            "https://gitlab.com/api/v4/projects/{}/repository/files/{}/raw?ref={commit}",
            quote(project, false),
            quote(path, false)
        )),
        _ => Err(format!("unsupported host in {url}")),
    }
}

/// The directory of `base` (a repository path ending in `/`, or empty for the whole
/// repository) that the host of `url` can limit an archive to, without the trailing `/`.
/// GitLab's archive API takes `path`, "Subpath of the repository to download", so a codebase in
/// one directory of a large repository (Xonotic's `qcsrc/`, beside 800 MB of game data) is a
/// small download. GitHub's archives are always of the whole repository, so it is empty there.
pub(crate) fn archive_scope<'b>(url: &str, base: &'b str) -> &'b str {
    match host_project(url) {
        Some(("gitlab.com", _)) => base.trim_end_matches('/'),
        _ => "",
    }
}

/// Where a source's host serves the repository at a commit as a `.tar.gz`: the whole of it, or
/// only `scope` when that is not empty ([`archive_scope`]). GitLab's web archive path asks for a
/// sign-in, so it is fetched through the API.
pub(crate) fn archive_url(url: &str, commit: &str, scope: &str) -> Result<String, String> {
    match host_project(url) {
        Some(("github.com", project)) if scope.is_empty() => Ok(format!(
            "https://github.com/{project}/archive/{commit}.tar.gz"
        )),
        Some(("gitlab.com", project)) => {
            let mut out = format!(
                "https://gitlab.com/api/v4/projects/{}/repository/archive.tar.gz?sha={commit}",
                quote(project, false)
            );
            if !scope.is_empty() {
                out.push_str("&path=");
                out.push_str(&quote(scope, false));
            }
            Ok(out)
        }
        Some((host, _)) => Err(format!("{host} archives cannot be limited to {scope}")),
        None => Err(format!("unsupported host in {url}")),
    }
}

/// `("github.com" | "gitlab.com", "owner/repo")` for `https://<host>/<owner>/<repo>`.
fn host_project(url: &str) -> Option<(&'static str, &str)> {
    for host in ["github.com", "gitlab.com"] {
        let Some(project) = url
            .strip_prefix("https://")
            .and_then(|rest| rest.strip_prefix(host))
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        let mut parts = project.split('/');
        let owner = parts.next().unwrap_or_default();
        let repo = parts.next().unwrap_or_default();
        if !owner.is_empty() && !repo.is_empty() && parts.next().is_none() {
            return Some((host, project));
        }
    }
    None
}

/// The repository URL of `https://github.com/<o>/<r>/releases/download/<tag>/<file>.zip`.
fn release_repo(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://github.com/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let [owner, repo, "releases", "download", tag, file] = parts.as_slice() else {
        return None;
    };
    let stem = file.strip_suffix(".zip")?;
    if [owner, repo, tag, &stem].iter().any(|p| p.is_empty()) {
        return None;
    }
    let len = "https://github.com/"
        .len()
        .checked_add(owner.len())?
        .checked_add(1)?
        .checked_add(repo.len())?;
    url.get(..len)
}

/// Percent-encodes everything but unreserved characters, and `/` when `keep_slash`.
fn quote(text: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'.' | b'-' | b'~')
            || (keep_slash && byte == b'/');
        if keep {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `^[a-z0-9][a-z0-9.-]*$`
fn is_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// An SPDX identifier, `LicenseRef-*` or `NONE`: letters, digits, `.`, `+` and `-`.
fn is_license(license: &str) -> bool {
    !license.is_empty()
        && license
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// Exactly `len` lowercase hexadecimal digits.
fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The lowercase extension of a path's last component, with its dot, as Python's
/// `PurePath.suffix` gives it: none for a name that starts or ends with its only dot.
fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dot = name.rfind('.')?;
    let ext = name.get(dot..)?;
    (dot > 0 && ext.len() > 1).then(|| ext.to_lowercase())
}

fn is_content(path: &str) -> bool {
    extension(path).is_some_and(|ext| CONTENT_EXTS.contains(&ext.as_str()))
}

#[cfg(test)]
mod tests;
