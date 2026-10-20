// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask corpus` and `cargo xtask assets`: the conformance corpus and the open test
//! assets in `corpus/manifest.toml` (P1-14; FND-9, LIC-4, LIC-5, LIC-7, QA-4).
//!
//! - `corpus verify [--online]` checks the manifest and the repository against the corpus
//!   policy ([`verify`]).
//! - `corpus fetch [<id>...]` downloads codebases and maps by checksum into the cache and
//!   writes each codebase to `<cache>/codebase/<id>/`, each map to `<cache>/map/<id>.map` and
//!   each tree of codebases to `<cache>/tree/<id>/` ([`fetch`]). With no ids it fetches every
//!   usable codebase and map, and every tree.
//! - `corpus build [<id>...]` fetches the codebases an oracle compiler builds, compiles them
//!   with the oracles into `<cache>/progs/<id>/` and checks that each progs.dat round-trips
//!   through `qc-progs` byte for byte ([`build`]).
//! - `assets fetch` downloads, checks and extracts the test assets, and `assets precache`
//!   checks they cover the precaches of the codebases they name ([`assets`]).
//!
//! The cache is `--dest <dir>`, else `$QS_TEST_ASSETS`, else `quakecstudio/test-assets` under
//! `$XDG_CACHE_HOME` or the home directory's `.cache`, and never inside the source tree.
//! Everything downloaded is kept in `<cache>/downloads/` and checked again before use, so a
//! second run works offline. Every command first runs the offline manifest checks.

mod archive;
mod assets;
mod build;
mod fetch;
pub(crate) mod manifest;
mod verify;

use std::env;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::download::{CURL, Net};
use manifest::Manifest;

const MANIFEST: &str = "corpus/manifest.toml";

/// Runs `cargo xtask corpus <verify | fetch | build> ...`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    run(root, args, &CURL)
}

/// [`main`] with the files `net` fetches.
fn run(root: &Path, args: &[String], net: &dyn Net) -> Result<bool, String> {
    let usage = "usage: cargo xtask corpus verify [--online] \
                 | fetch [--dest <dir>] [<id>...] | build [--dest <dir>] [<id>...]";
    let Some((command, rest)) = args.split_first() else {
        return Err(usage.to_owned());
    };
    let m = Manifest::load(&root.join(MANIFEST))?;
    match command.as_str() {
        "verify" => {
            let online = match rest {
                [] => false,
                [flag] if flag == "--online" => true,
                _ => return Err(usage.to_owned()),
            };
            Ok(report(cmd_verify(root, &m, online.then_some(net))))
        }
        "fetch" | "build" => {
            let (dest, ids) = dest_and_ids(root, rest)?;
            let mut errs = verify::check(&m);
            if errs.is_empty() {
                errs = if command == "fetch" {
                    fetch::cmd_fetch(&m, &dest, &ids, net)
                } else {
                    build::cmd_build(root, &m, &dest, &ids, net)
                };
            }
            Ok(report(errs))
        }
        _ => Err(usage.to_owned()),
    }
}

/// Runs `cargo xtask assets <fetch | precache> [--dest <dir>]`.
pub(crate) fn assets_main(root: &Path, args: &[String]) -> Result<bool, String> {
    assets_run(root, args, &CURL)
}

/// [`assets_main`] with the files `net` fetches.
fn assets_run(root: &Path, args: &[String], net: &dyn Net) -> Result<bool, String> {
    let usage = "usage: cargo xtask assets <fetch | precache> [--dest <dir>]";
    let Some((command, rest)) = args.split_first() else {
        return Err(usage.to_owned());
    };
    let (dest, ids) = dest_and_ids(root, rest)?;
    if !ids.is_empty() {
        return Err(usage.to_owned());
    }
    let m = Manifest::load(&root.join(MANIFEST))?;
    let mut errs = verify::check(&m);
    if errs.is_empty() {
        errs = match command.as_str() {
            "fetch" => assets::cmd_fetch(&m, &dest, net),
            "precache" => assets::cmd_precache(&m, &dest, net),
            _ => return Err(usage.to_owned()),
        };
    }
    Ok(report(errs))
}

/// Prints every error and returns whether there were none.
fn report(errs: Vec<String>) -> bool {
    for e in &errs {
        eprintln!("error: {e}");
    }
    errs.is_empty()
}

fn cmd_verify(root: &Path, m: &Manifest, online: Option<&dyn Net>) -> Vec<String> {
    let mut errs = verify::check(m);
    match tracked_files(root) {
        Ok(files) => {
            let read = |f: &str| std::fs::read(root.join(f)).map_err(|e| e.to_string());
            errs.extend(verify::check_repo(m, &files, &read));
        }
        Err(e) => errs.push(e),
    }
    if let Some(net) = online {
        errs.extend(verify::check_online(m, net));
    }
    let pinned: usize = m.source.iter().map(|s| s.checksums.len()).sum();
    println!(
        "corpus {}: {} codebases, {} maps, {} assets, {} sources, {pinned} pinned files, {} errors",
        if online.is_some() {
            "online"
        } else {
            "offline"
        },
        m.codebase.len(),
        m.map.len(),
        m.asset.len(),
        m.source.len(),
        errs.len()
    );
    errs
}

fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("cannot run git ls-files: {e}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed ({})", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Splits `[--dest <dir>] [<id>...]`, and checks the cache is outside the source tree.
fn dest_and_ids(root: &Path, args: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    let mut dest = None;
    let mut ids = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--dest" {
            let dir = args.next().ok_or("--dest needs a directory")?;
            dest = Some(PathBuf::from(dir));
        } else if arg.starts_with('-') {
            return Err(format!("unknown option {arg}"));
        } else {
            ids.push(arg.clone());
        }
    }
    let dest = dest
        .or_else(|| default_dest(&|key| env::var_os(key).map(PathBuf::from)))
        .ok_or("no cache directory: set QS_TEST_ASSETS or pass --dest")?;
    let dest = env::current_dir()
        .map(|cwd| cwd.join(&dest))
        .unwrap_or(dest);
    if resolve(&dest).starts_with(resolve(root)) {
        return Err(format!(
            "the cache {} is inside the source tree; corpus files are never kept there (LIC-7)",
            dest.display()
        ));
    }
    Ok((dest, ids))
}

/// `path` with its symlinks and `..` resolved, one part at a time: each prefix that exists is
/// canonicalized, so a symlink anywhere along it is followed, and a part that does not exist
/// yet, which cannot be a symlink, is kept as written, a `..` after it removing it. A cache
/// that does not exist yet resolves through its nearest existing ancestor.
fn resolve(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(part);
                if let Ok(canonical) = resolved.canonicalize() {
                    resolved = canonical;
                }
            }
        }
    }
    resolved
}

/// `$QS_TEST_ASSETS`, else `quakecstudio/test-assets` under `$XDG_CACHE_HOME`, else under the
/// home directory's `.cache` (`$HOME`, or `%USERPROFILE%` on Windows).
fn default_dest(var: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let nonempty = |key: &str| var(key).filter(|p| !p.as_os_str().is_empty());
    if let Some(dest) = nonempty("QS_TEST_ASSETS") {
        return Some(dest);
    }
    let base = nonempty("XDG_CACHE_HOME").or_else(|| {
        nonempty("HOME")
            .or_else(|| nonempty("USERPROFILE"))
            .map(|home| home.join(".cache"))
    })?;
    Some(base.join("quakecstudio").join("test-assets"))
}

/// A relative archive path with no empty, `.`, `..`, drive or backslash parts.
pub(crate) fn safe_member(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|p| !matches!(p, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::{Path, PathBuf};

    use super::{assets_run, default_dest, dest_and_ids, run, safe_member};
    use crate::download::Net;
    use crate::test_dirs::claim;

    /// A network where every file is missing.
    struct Offline;

    impl Net for Offline {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            Err(format!("{url}: offline"))
        }

        fn download(&self, url: &str, _out: &Path) -> Result<(), String> {
            Err(format!("{url}: offline"))
        }
    }

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// A cache directory outside the repository, as the source tree refuses one,
    /// this run's and this test's own.
    fn cache(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!(
                "quakecstudio-xtask-tests-cache-{}",
                std::process::id()
            ))
            .join(name);
        claim(&dir);
        dir.display().to_string()
    }

    #[test]
    fn verify_checks_the_manifest_and_the_repository() {
        assert!(run(&root(), &args("verify"), &Offline).unwrap());
        assert!(!run(&root(), &args("verify --online"), &Offline).unwrap());
        assert!(run(&root(), &args("verify --fast"), &Offline).is_err());
        assert!(run(&root(), &args("unknown"), &Offline).is_err());
        assert!(run(&root(), &[], &Offline).is_err());
    }

    #[test]
    fn fetch_and_build_report_what_they_cannot_do() {
        let dest = cache("fetch-and-build");
        assert!(
            !run(
                &root(),
                &args(&format!("fetch --dest {dest} id-1.01")),
                &Offline
            )
            .unwrap()
        );
        assert!(
            !run(
                &root(),
                &args(&format!("build --dest {dest} copper")),
                &Offline
            )
            .unwrap()
        );
        assert!(run(&root(), &args("fetch --dest"), &Offline).is_err());
        assert!(run(&root(), &args("fetch --quick"), &Offline).is_err());
    }

    #[test]
    fn assets_run_their_commands() {
        let dest = cache("assets");
        assert!(!assets_run(&root(), &args(&format!("fetch --dest {dest}")), &Offline).unwrap());
        assert!(!assets_run(&root(), &args(&format!("precache --dest {dest}")), &Offline).unwrap());
        assert!(assets_run(&root(), &args(&format!("check --dest {dest}")), &Offline).is_err());
        assert!(
            assets_run(
                &root(),
                &args(&format!("fetch --dest {dest} extra")),
                &Offline
            )
            .is_err()
        );
        assert!(assets_run(&root(), &[], &Offline).is_err());
    }

    /// An offline network that remembers what was asked of it.
    #[derive(Default)]
    struct Recording(std::cell::RefCell<Vec<String>>);

    impl Net for Recording {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.0.borrow_mut().push(url.to_owned());
            Offline.get(url)
        }

        fn download(&self, url: &str, out: &Path) -> Result<(), String> {
            self.0.borrow_mut().push(url.to_owned());
            Offline.download(url, out)
        }
    }

    #[test]
    fn fetch_downloads_the_codebase() {
        let net = Recording::default();
        let fetch = args(&format!("fetch --dest {} id-1.01", cache("fetch")));
        assert!(!run(&root(), &fetch, &net).unwrap());
        assert_eq!(net.0.borrow().len(), 1, "{:?}", net.0.borrow());
    }

    #[test]
    fn the_entry_points_need_a_command() {
        assert!(super::main(&root(), &[]).is_err());
        assert!(super::assets_main(&root(), &[]).is_err());
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| PathBuf::from(v))
        }
    }

    #[test]
    fn the_cache_comes_from_the_environment() {
        let dest = default_dest(&env(&[("QS_TEST_ASSETS", "/x/assets"), ("HOME", "/h")]));
        assert_eq!(dest, Some(PathBuf::from("/x/assets")));
        let dest = default_dest(&env(&[("XDG_CACHE_HOME", "/c"), ("HOME", "/h")]));
        assert_eq!(dest, Some(PathBuf::from("/c/quakecstudio/test-assets")));
        let dest = default_dest(&env(&[("QS_TEST_ASSETS", ""), ("HOME", "/h")]));
        assert_eq!(
            dest,
            Some(PathBuf::from("/h/.cache/quakecstudio/test-assets"))
        );
        let dest = default_dest(&env(&[("USERPROFILE", "/u")]));
        assert_eq!(
            dest,
            Some(PathBuf::from("/u/.cache/quakecstudio/test-assets"))
        );
        assert_eq!(default_dest(&env(&[])), None);
    }

    #[test]
    fn a_cache_inside_the_source_tree_is_refused() {
        let root = std::env::current_dir().unwrap();
        let inside = root.join("target").display().to_string();
        let err = dest_and_ids(&root, &["--dest".to_owned(), inside]).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let outside = Path::new("/elsewhere/cache").display().to_string();
        let args = ["--dest".to_owned(), outside, "id-1.01".to_owned()];
        let (dest, ids) = dest_and_ids(&root.join("sub"), &args).unwrap();
        assert!(dest.ends_with("cache"));
        assert_eq!(ids, ["id-1.01"]);
        assert!(dest_and_ids(&root, &["--dest".to_owned()]).is_err());
        assert!(dest_and_ids(&root, &["--online".to_owned()]).is_err());
    }

    #[test]
    fn a_cache_reaching_the_source_tree_through_dot_dot_is_refused() {
        let root = root();
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let through = root
            .parent()
            .unwrap()
            .join("not-yet")
            .join("..")
            .join(&name)
            .join("cache");
        let err =
            dest_and_ids(&root, &["--dest".to_owned(), through.display().to_string()]).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let back_out = root.join("not-yet").join("..").join("target");
        assert!(
            dest_and_ids(
                &root,
                &["--dest".to_owned(), back_out.display().to_string()]
            )
            .is_err()
        );
        let beside = root
            .join("not-yet")
            .join("..")
            .join("..")
            .join("elsewhere-cache");
        assert!(dest_and_ids(&root, &["--dest".to_owned(), beside.display().to_string()]).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_cache_reaching_the_source_tree_through_a_symlink_is_refused() {
        let dir = std::path::PathBuf::from(cache("symlink"));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("tree");
        if link.exists() {
            std::fs::remove_file(&link).unwrap();
        }
        std::os::unix::fs::symlink(root(), &link).unwrap();
        let dest = link.join("not-yet").join("cache").display().to_string();
        let err = dest_and_ids(&root(), &["--dest".to_owned(), dest]).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let beside = dir.join("cache").display().to_string();
        assert!(dest_and_ids(&root(), &["--dest".to_owned(), beside]).is_ok());
    }

    #[test]
    fn safe_members_are_relative_paths() {
        assert!(safe_member("lite/id1/pak0.pak"));
        for bad in [
            "",
            "lite/../x",
            "/etc/passwd",
            "lite/a\\b",
            "C:/x",
            "lite/",
            "a//b",
            "./a",
        ] {
            assert!(!safe_member(bad), "{bad}");
        }
    }
}
