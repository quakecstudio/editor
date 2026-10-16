// SPDX-License-Identifier: GPL-3.0-or-later

//! Downloads over HTTPS with the system `curl`, which every CI runner and developer machine
//! has, so xtask needs no HTTP or TLS crate.
//!
//! [`Net`] is the seam the corpus and asset tests replace with canned bytes; nothing in a test
//! touches the network.

use std::fs;
use std::path::Path;
use std::process::Command;

/// The `User-Agent` every request sends.
const USER_AGENT: &str = "quakecstudio-xtask";

/// Fetches URLs.
pub(crate) trait Net {
    /// The body of `url`, or why it could not be fetched.
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;

    /// Writes the body of `url` to `out`. On an error `out` may hold part of the body.
    fn download(&self, url: &str, out: &Path) -> Result<(), String>;
}

/// [`Net`] through the system `curl`: redirects followed, an HTTP error status a failure, and
/// three retries on any error.
pub(crate) struct Curl;

impl Curl {
    fn command(url: &str) -> Command {
        let mut command = Command::new("curl");
        command
            .args([
                "-fsSL",
                "--retry",
                "3",
                "--retry-all-errors",
                "-A",
                USER_AGENT,
            ])
            .arg(url);
        command
    }
}

impl Net for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let output = Self::command(url)
            .output()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(format!(
            "curl {url}: {} {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }

    fn download(&self, url: &str, out: &Path) -> Result<(), String> {
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let status = Self::command(url)
            .arg("-o")
            .arg(out)
            .status()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("curl {url}: {status}"))
        }
    }
}
