// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the repository: the files git tracks and small file helpers.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Every file git tracks, plus untracked files that are not ignored, so a new
/// file is checked before it is committed. Paths use `/` and are relative to
/// `root`; files deleted from the working tree are left out.
pub(crate) fn files(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|error| format!("cannot run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|error| format!("git ls-files printed a non-UTF-8 path: {error}"))?;
    let mut files: Vec<String> = listing
        .split('\0')
        .filter(|path| !path.is_empty() && root.join(path).is_file())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

pub(crate) fn read(root: &Path, path: &str) -> Result<String, String> {
    fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"))
}
