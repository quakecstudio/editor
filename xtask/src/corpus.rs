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

pub(crate) mod archive;
mod assets;
mod build;
pub(crate) mod fetch;
pub(crate) mod manifest;
pub(crate) mod verify;

use std::env;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::download::{CURL, Net};
use manifest::Manifest;

const MANIFEST: &str = "corpus/manifest.toml";

/// `cargo xtask corpus verify [--online] | fetch [--dest <dir>] [<id>...] | build [--dest <dir>]
/// [<id>...]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    #[command(subcommand)]
    command: CorpusCommand,
}

#[derive(clap::Subcommand, Debug)]
enum CorpusCommand {
    /// Check the manifest and the repository.
    Verify {
        /// Also check every source online.
        #[arg(long)]
        online: bool,
    },
    /// Download codebases into the cache.
    Fetch(Selection),
    /// Build codebases in the cache.
    Build(Selection),
}

/// The cache and the entries a command takes.
#[derive(clap::Args, Debug)]
struct Selection {
    /// The cache directory, `$QS_TEST_ASSETS` or the user's cache by default.
    #[arg(long)]
    dest: Option<PathBuf>,
    /// The manifest ids, every one when none is named.
    ids: Vec<String>,
}

impl Args {
    /// `cargo xtask corpus fetch <id>...` into the default cache.
    pub(crate) fn fetch(ids: Vec<String>) -> Self {
        Self {
            command: CorpusCommand::Fetch(Selection { dest: None, ids }),
        }
    }
}

/// Runs `cargo xtask corpus`.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    run(root, args, &CURL)
}

/// [`main`] with the files `net` fetches.
fn run(root: &Path, args: &Args, net: &dyn Net) -> Result<bool, String> {
    let m = Manifest::load(&root.join(MANIFEST))?;
    match &args.command {
        CorpusCommand::Verify { online } => Ok(report(cmd_verify(root, &m, online.then_some(net)))),
        CorpusCommand::Fetch(selection) | CorpusCommand::Build(selection) => {
            let dest = cache_dest(root, selection.dest.as_deref())?;
            let mut errs = verify::check(&m);
            if errs.is_empty() {
                errs = if matches!(args.command, CorpusCommand::Fetch(_)) {
                    fetch::cmd_fetch(&m, &dest, &selection.ids, net)
                } else {
                    build::cmd_build(root, &m, &dest, &selection.ids, net)
                };
            }
            Ok(report(errs))
        }
    }
}

/// `cargo xtask assets <fetch | precache> [--dest <dir>]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct AssetsArgs {
    #[command(subcommand)]
    command: AssetsCommand,
}

#[derive(clap::Subcommand, Debug)]
enum AssetsCommand {
    /// Download the test assets.
    Fetch {
        /// The cache directory.
        #[arg(long)]
        dest: Option<PathBuf>,
    },
    /// Download and unpack them into the cache.
    Precache {
        /// The cache directory.
        #[arg(long)]
        dest: Option<PathBuf>,
    },
}

/// Runs `cargo xtask assets`.
pub(crate) fn assets_main(root: &Path, args: &AssetsArgs) -> Result<bool, String> {
    assets_run(root, args, &CURL)
}

/// [`assets_main`] with the files `net` fetches.
fn assets_run(root: &Path, args: &AssetsArgs, net: &dyn Net) -> Result<bool, String> {
    let (AssetsCommand::Fetch { dest } | AssetsCommand::Precache { dest }) = &args.command;
    let dest = cache_dest(root, dest.as_deref())?;
    let m = Manifest::load(&root.join(MANIFEST))?;
    let mut errs = verify::check(&m);
    if errs.is_empty() {
        errs = match args.command {
            AssetsCommand::Fetch { .. } => assets::cmd_fetch(&m, &dest, net),
            AssetsCommand::Precache { .. } => assets::cmd_precache(&m, &dest, net),
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

/// The cache `dest` names, or the default one, checked to be outside the source tree.
pub(crate) fn cache_dest(root: &Path, dest: Option<&Path>) -> Result<PathBuf, String> {
    let dest = dest
        .map(Path::to_path_buf)
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
    Ok(dest)
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
/// The asset cache `cargo xtask corpus fetch` writes to by default: `$QS_TEST_ASSETS`, else
/// `quakecstudio/test-assets` under the user's cache directory.
pub(crate) fn asset_cache() -> Option<PathBuf> {
    default_dest(&|key| env::var_os(key).map(PathBuf::from))
}

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

    use super::{Args, AssetsArgs, assets_run, cache_dest, default_dest, run, safe_member};
    use crate::download::Net;
    use crate::parse_cli;
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

    /// `cargo xtask corpus` with `text`.
    fn cli(text: &str) -> Result<Args, clap::Error> {
        parse_cli(&text.split_whitespace().collect::<Vec<_>>())
    }

    /// `cargo xtask assets` with `text`.
    fn assets(text: &str) -> Result<AssetsArgs, clap::Error> {
        parse_cli(&text.split_whitespace().collect::<Vec<_>>())
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
        assert!(run(&root(), &cli("verify").unwrap(), &Offline).unwrap());
        assert!(!run(&root(), &cli("verify --online").unwrap(), &Offline).unwrap());
        assert!(cli("verify --fast").is_err());
        assert!(cli("unknown").is_err());
        assert!(cli("").is_err());
    }

    #[test]
    fn fetch_and_build_report_what_they_cannot_do() {
        let dest = cache("fetch-and-build");
        assert!(
            !run(
                &root(),
                &cli(&format!("fetch --dest {dest} id-1.01")).unwrap(),
                &Offline
            )
            .unwrap()
        );
        assert!(
            !run(
                &root(),
                &cli(&format!("build --dest {dest} copper")).unwrap(),
                &Offline
            )
            .unwrap()
        );
        assert!(cli("fetch --dest").is_err());
        assert!(cli("fetch --quick").is_err());
    }

    #[test]
    fn assets_run_their_commands() {
        let dest = cache("assets");
        let fetch = assets(&format!("fetch --dest {dest}")).unwrap();
        assert!(!assets_run(&root(), &fetch, &Offline).unwrap());
        let precache = assets(&format!("precache --dest {dest}")).unwrap();
        assert!(!assets_run(&root(), &precache, &Offline).unwrap());
        assert!(assets(&format!("check --dest {dest}")).is_err());
        assert!(assets(&format!("fetch --dest {dest} extra")).is_err());
        assert!(assets("").is_err());
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
        let fetch = cli(&format!("fetch --dest {} id-1.01", cache("fetch"))).unwrap();
        assert!(!run(&root(), &fetch, &net).unwrap());
        assert_eq!(net.0.borrow().len(), 1, "{:?}", net.0.borrow());
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
        let err = cache_dest(&root, Some(Path::new(&inside))).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let dest = cache_dest(&root.join("sub"), Some(Path::new("/elsewhere/cache"))).unwrap();
        assert!(dest.ends_with("cache"));
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
        let err = cache_dest(&root, Some(Path::new(&through.display().to_string()))).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let back_out = root.join("not-yet").join("..").join("target");
        assert!(cache_dest(&root, Some(Path::new(&back_out.display().to_string()))).is_err());
        let beside = root
            .join("not-yet")
            .join("..")
            .join("..")
            .join("elsewhere-cache");
        assert!(cache_dest(&root, Some(Path::new(&beside.display().to_string()))).is_ok());
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
        let err = cache_dest(&root(), Some(Path::new(&dest))).unwrap_err();
        assert!(err.contains("inside the source tree"), "{err}");
        let beside = dir.join("cache").display().to_string();
        assert!(cache_dest(&root(), Some(Path::new(&beside))).is_ok());
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
