// SPDX-License-Identifier: GPL-3.0-or-later

//! What the oracle module's tests share: oracles that a fake runs, and fresh directories.

#![expect(
    clippy::panic,
    reason = "test code: a directory that cannot be made fails the test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Oracles;
use crate::exec::Exec;

/// The repository.
pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// A new, empty `target/xtask-tests/<name>`.
pub(crate) fn fresh_dir(name: &str) -> PathBuf {
    let dir = root().join("target").join("xtask-tests").join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    dir
}

/// Oracles whose programs `exec` runs, named `qcc`, `vm-oracle` and `fteqcc`, working in
/// `work`.
pub(crate) fn fake_oracles<'a>(work: &Path, exec: &'a dyn Exec) -> Oracles<'a> {
    Oracles {
        qcc: PathBuf::from("qcc"),
        vm: PathBuf::from("vm-oracle"),
        fteqcc: PathBuf::from("fteqcc"),
        work: work.to_path_buf(),
        rerelease: work.join("artifacts"),
        exec,
    }
}

/// The arguments of `command`, as paths.
pub(crate) fn args(command: &Command) -> Vec<PathBuf> {
    command.get_args().map(PathBuf::from).collect()
}

/// The last argument of `command`, where the oracle writes its trace.
pub(crate) fn last_arg(command: &Command) -> PathBuf {
    args(command).pop().unwrap_or_default()
}
