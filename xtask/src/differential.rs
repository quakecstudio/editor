// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask differential`: the end-to-end differential VM suite (P1-13, FND-8, QA-1).
//!
//! The suite itself is the `differential` binary of the `qs-differential` crate (its
//! `src/bin/differential.rs`): it writes scenarios (`docs/spec/differential.md` DIFF-2) for the
//! corpus progs, runs each in the id VM oracle and in QuakeC Studio's runner, compares the
//! traces (DIFF-9) and measures coverage (DIFF-11). This xtask command builds the oracles
//! (`cargo xtask oracles`) so the VM oracle exists, then runs that binary; it does not link
//! `qs-differential`, which is a `tests`-layer crate nothing may depend on (`layers.toml`), so
//! it runs the binary with `cargo run` instead.
//!
//! - `cargo xtask differential` and `--fast` run the fast subset: id 1.01, enforcing zero
//!   divergences (the subset the CI `Test Assets` job proves on every change).
//! - `cargo xtask differential --full` runs every oracle-built codebase, the nightly corpus,
//!   enforcing zero divergences and the 90% statement-coverage bar (TEST-6).
//!
//! The corpus progs must be built first (`cargo xtask corpus build`); the asset cache is
//! `$QS_TEST_ASSETS` (the same default as `cargo xtask corpus`).

use std::path::Path;
use std::process::Command;

use crate::oracles;

/// Runs `cargo xtask differential [--fast | --full]`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let mode = match args {
        [] => "fast",
        [flag] if flag == "--fast" => "fast",
        [flag] if flag == "--full" => "full",
        _ => return Err("usage: cargo xtask differential [--fast | --full]".to_owned()),
    };
    // Build the oracles so the VM oracle binary exists (P1-11); the suite runs it per scenario.
    let oracles = oracles::build(root)?;
    let assets = asset_root()?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "run",
            "--release",
            "--locked",
            "-p",
            "qs-differential",
            "--bin",
            "differential",
            "--",
            "--mode",
            mode,
            "--oracle",
        ])
        .arg(&oracles.vm)
        .arg("--assets")
        .arg(&assets)
        .status()
        .map_err(|e| format!("cannot run the differential suite: {e}"))?;
    Ok(status.success())
}

/// The asset cache root (`$QS_TEST_ASSETS`, else the default under the cache directory), where
/// `cargo xtask corpus build` wrote the progs.
fn asset_root() -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;
    if let Some(dir) = std::env::var_os("QS_TEST_ASSETS").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".cache"))
        })
        .ok_or("no asset cache: set QS_TEST_ASSETS")?;
    Ok(base.join("quakecstudio").join("test-assets"))
}
