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
/// `retries` retries on any error.
pub(crate) struct Curl {
    pub(crate) retries: u32,
}

/// The `curl` every command fetches with: three retries.
pub(crate) const CURL: Curl = Curl { retries: 3 };

impl Curl {
    fn command(&self, url: &str) -> Command {
        let mut command = Command::new("curl");
        command
            .args(["-fsSL", "--retry"])
            .arg(self.retries.to_string())
            .args(["--retry-all-errors", "-A", USER_AGENT])
            .arg(url);
        command
    }
}

impl Net for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let output = self
            .command(url)
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
        let status = self
            .command(url)
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{CURL, Curl, Net};
    use crate::test_dirs::fresh;

    /// A `file:` URL for `path`, which curl reads with no network.
    fn file_url(path: &Path) -> String {
        let path = path.to_string_lossy().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    fn dir(name: &str) -> PathBuf {
        fresh(&format!("download/{name}"))
    }

    #[test]
    fn curl_fetches_and_downloads_a_file() {
        let dir = dir("curl-fetch");
        let source = dir.join("source.txt");
        fs::write(&source, b"body").unwrap();
        assert_eq!(CURL.get(&file_url(&source)).unwrap(), b"body");
        let out = dir.join("nested/out.txt");
        CURL.download(&file_url(&source), &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"body");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let dir = dir("curl-missing");
        let url = file_url(&dir.join("nowhere.txt"));
        // No retries: a missing file is missing however often it is asked for.
        let curl = Curl { retries: 0 };
        assert!(curl.get(&url).unwrap_err().contains("curl"));
        assert!(curl.download(&url, &dir.join("out.txt")).is_err());
    }
}
