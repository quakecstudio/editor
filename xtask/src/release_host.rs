// SPDX-License-Identifier: GPL-3.0-or-later

//! The release preview host, built as the app ships it, for `cargo xtask bench` and
//! `cargo xtask view-diff`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Builds the release preview host and returns its path, under `CARGO_TARGET_DIR` when it is
/// set, else under the workspace's `target`.
pub(crate) fn build(root: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "--locked",
            "--release",
            "--package",
            "quakec-engine",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot build the preview host: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build of the preview host failed: {status}"));
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), |dir| root.join(PathBuf::from(dir)));
    Ok(target
        .join("release")
        .join(format!("quakec-engine{}", std::env::consts::EXE_SUFFIX)))
}
