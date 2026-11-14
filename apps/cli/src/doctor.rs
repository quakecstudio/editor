// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-ide doctor`: what an installed QuakeC Studio can reach where it runs (P4-18): the
//! games it finds, the preview host beside it started headless on a game and stopped over its
//! bridge, and the system keychain, an entry written, read back and deleted. The release's
//! install tests run it from each package, the Flatpak's from inside its sandbox, so a package
//! that cannot find Steam, start the host or reach the keychain fails before it ships.
//!
//! Each check prints one `ok` or `FAIL` line, as the repository's other checks do.

use std::io::Write;
use std::path::{Path, PathBuf};

use qs_engine::{Flavour, LaunchSpec, launch, locate_host, prepare_run_root};
use qs_install::{Install, examine_folder, find_installs};
use qs_models::{KeyStore, Keychain, Secret};

/// The keychain entry the check writes and deletes; the agent's keys are under other names.
const KEYCHAIN_PROBE: &str = "quakec-ide-doctor";

/// The map the host is started on, which every edition and LibreQuake has.
const START_MAP: &str = "start";

/// The checks `doctor` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, clap::ValueEnum)]
pub(crate) enum Check {
    /// The game installs found in the stores' usual places.
    Games,
    /// The preview host beside this program, started headless and stopped.
    Host,
    /// The system keychain.
    Keychain,
}

impl Check {
    /// Every check, in the order they run.
    pub(crate) const ALL: [Self; 3] = [Self::Games, Self::Host, Self::Keychain];
}

/// What `doctor` was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Doctor {
    /// The checks to run.
    pub(crate) checks: Vec<Check>,
    /// The game folder the host plays; the first install found otherwise.
    pub(crate) game: Option<PathBuf>,
    /// Whether finding no game fails the games check.
    pub(crate) require_game: bool,
}

/// One check's outcome: what it found, or why it failed.
type Outcome = Result<String, String>;

impl Doctor {
    /// Runs `doctor`, writing a line per check to `out`; whether every check passed.
    pub(crate) fn run(&self, out: &mut impl Write) -> std::io::Result<bool> {
        let installs = find_installs();
        let mut passed = true;
        for check in Check::ALL {
            if !self.checks.contains(&check) {
                continue;
            }
            let outcome = match check {
                Check::Games => {
                    let requirement = if self.require_game {
                        Requirement::Required
                    } else {
                        Requirement::Optional
                    };
                    games(&installs, requirement)
                }
                Check::Host => host(self.game.as_deref(), &installs),
                Check::Keychain => keychain(&Keychain),
            };
            match outcome {
                Ok(found) => writeln!(out, "ok    {}: {found}", check.name())?,
                Err(why) => {
                    passed = false;
                    writeln!(out, "FAIL  {}: {why}", check.name())?;
                }
            }
        }
        Ok(passed)
    }
}

impl Check {
    /// The word a check's line starts with.
    const fn name(self) -> &'static str {
        match self {
            Check::Games => "games",
            Check::Host => "preview host",
            Check::Keychain => "keychain",
        }
    }
}

/// The installs found, by store and folder.
/// Whether `doctor` fails when no game is found (`--require-game`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Requirement {
    /// It fails.
    Required,
    /// It only reports it.
    Optional,
}

fn games(installs: &[Install], requirement: Requirement) -> Outcome {
    let require = requirement == Requirement::Required;
    if installs.is_empty() {
        return if require {
            Err("none found in the stores' usual places".to_owned())
        } else {
            Ok("none found".to_owned())
        };
    }
    let found: Vec<String> = installs
        .iter()
        .map(|install| format!("{:?} at {}", install.source, install.root.display()))
        .collect();
    Ok(found.join("; "))
}

/// The host beside this program, started headless on `game` (or the first install with the
/// original game) and stopped over its bridge.
fn host(game: Option<&Path>, installs: &[Install]) -> Outcome {
    let me = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
    let path = locate_host(None, &me).map_err(|e| e.to_string())?;
    let install = match game {
        Some(folder) => {
            examine_folder(folder).ok_or_else(|| format!("{}: no game here", folder.display()))?
        }
        None => installs
            .iter()
            .find(|install| install.original.is_some())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "found at {}, but no game to start it on: name one with --game",
                    path.display()
                )
            })?,
    };
    let base = install
        .original
        .ok_or_else(|| format!("{}: no original game here", install.root.display()))?;
    let scratch = std::env::temp_dir().join(format!("quakec-ide-doctor-{}", std::process::id()));
    let mut run_root = prepare_run_root(&scratch, Flavour::Original, &base, "doctor")
        .map_err(|e| e.to_string())?;
    // A game that is not the registered one refuses a mod (FS-4.5); `id1` alone always plays.
    run_root.game = None;
    let spec = LaunchSpec {
        host: path.clone(),
        run_root,
        map: Some(START_MAP.to_owned()),
        socket: socket(),
        extra: vec!["--headless".to_owned()],
    };
    let mut running = launch(&spec, "quakec-ide doctor").map_err(|e| e.to_string())?;
    let stopped = running
        .client
        .shutdown()
        .map_err(|e| format!("shutdown: {e}"));
    if stopped.is_err() {
        // The host is of no use once its bridge failed.
        running.child.kill().ok();
    }
    let status = running.child.wait().map_err(|e| e.to_string());
    // The run root is scratch; a file left in it does no harm.
    std::fs::remove_dir_all(&scratch).ok();
    stopped?;
    let status = status?;
    if !status.success() {
        return Err(format!("{} stopped with {status}", path.display()));
    }
    Ok(format!(
        "{} played {START_MAP} on {} and stopped",
        path.display(),
        base.path.display()
    ))
}

/// The socket or named pipe the host listens at, unique to this run.
fn socket() -> PathBuf {
    let name = format!("quakec-ide-doctor-{}", std::process::id());
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

/// An entry written to `store`, read back the same and deleted.
fn keychain(store: &impl KeyStore) -> Outcome {
    let probe = Secret::new(format!("doctor-{}", std::process::id()));
    store
        .set(KEYCHAIN_PROBE, &probe)
        .map_err(|e| format!("cannot write an entry: {e}"))?;
    let read = store.get(KEYCHAIN_PROBE);
    let deleted = store.delete(KEYCHAIN_PROBE);
    match read {
        Ok(Some(back)) if back == probe => {}
        Ok(Some(_)) => return Err("an entry read back differs from the one written".to_owned()),
        Ok(None) => return Err("an entry written is not there to read".to_owned()),
        Err(e) => return Err(format!("cannot read an entry: {e}")),
    }
    deleted.map_err(|e| format!("cannot delete an entry: {e}"))?;
    Ok("an entry written, read back and deleted".to_owned())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_models::{KeyStore as _, MemoryKeys};

    use super::{Check, Doctor, Requirement, games, keychain};

    // No spec statement: P4-18: the keychain check writes, reads back and deletes its entry,
    // leaving the store as it found it
    #[test]
    fn the_keychain_check_leaves_nothing_behind() {
        let store = MemoryKeys::new();
        assert!(keychain(&store).is_ok());
        assert_eq!(store.get(super::KEYCHAIN_PROBE).unwrap(), None);
    }

    // No spec statement: P4-18: finding no game passes unless one is required
    #[test]
    fn no_game_fails_only_when_one_is_required() {
        assert!(games(&[], Requirement::Optional).is_ok());
        assert!(games(&[], Requirement::Required).is_err());
    }

    // No spec statement: P4-18: only the checks asked for run, each a line
    #[test]
    fn only_the_checks_asked_for_run() {
        let mut out = Vec::new();
        let doctor = Doctor {
            checks: vec![Check::Games],
            game: None,
            require_game: false,
        };
        assert!(doctor.run(&mut out).unwrap());
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("ok    games: "), "{text}");
    }
}
