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
/// `retries` retries on any error, curl's own and curl ending with no exit status of its own,
/// as Windows' `curl.exe` sometimes ends with an access violation.
pub(crate) struct Curl {
    pub(crate) retries: u32,
    /// The program run, `curl` but in tests.
    pub(crate) program: &'static str,
}

/// The `curl` every command fetches with: three retries.
pub(crate) const CURL: Curl = Curl {
    retries: 3,
    program: "curl",
};

/// The exit statuses curl itself ends with: 0 and its error codes, all below 256.
const CURL_STATUSES: core::ops::Range<i32> = 0..256;

/// Whether a run ended other than with one of curl's own statuses: killed by a signal, or a
/// Windows exception such as `0xC0000005`.
fn crashed(status: std::process::ExitStatus) -> bool {
    status
        .code()
        .is_none_or(|code| !CURL_STATUSES.contains(&code))
}

impl Curl {
    /// Runs `make`'s command until it ends with one of curl's own statuses, at most `retries`
    /// more times; its last output.
    fn run(&self, make: impl Fn() -> Command) -> Result<std::process::Output, String> {
        let mut tries = 0;
        loop {
            let output = make()
                .output()
                .map_err(|e| format!("cannot run {}: {e}", self.program))?;
            if !crashed(output.status) || tries >= self.retries {
                return Ok(output);
            }
            tries = tries.saturating_add(1);
        }
    }

    fn command(&self, url: &str) -> Command {
        let mut command = Command::new(self.program);
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
        let output = self.run(|| self.command(url))?;
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
        let output = self.run(|| {
            let mut command = self.command(url);
            command.arg("-o").arg(out);
            command
        })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!("curl {url}: {}", output.status))
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

    // A curl that crashes is run again: a script that kills itself the first time it runs in a
    // folder, then copies the file, stands in for curl.exe's access violation.
    #[cfg(unix)]
    #[test]
    fn a_crashed_curl_is_run_again() {
        use std::os::unix::fs::PermissionsExt;

        let dir = dir("curl-crash");
        let source = dir.join("source.txt");
        fs::write(&source, b"body").unwrap();
        let script = dir.join("crashy");
        let marker = dir.join("ran");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ ! -f '{marker}' ]; then touch '{marker}'; kill -SEGV $$; fi\n\
                 for last; do :; done\ncat \"${{last#file://}}\"\n",
                marker = marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let program: &'static str =
            Box::leak(script.to_string_lossy().into_owned().into_boxed_str());
        let once = Curl {
            retries: 1,
            program,
        };
        assert_eq!(once.get(&file_url(&source)).unwrap(), b"body");
        fs::remove_file(&marker).unwrap();
        let never = Curl {
            retries: 0,
            program,
        };
        assert!(
            never.get(&file_url(&source)).is_err(),
            "no retries leaves the crash"
        );
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let dir = dir("curl-missing");
        let url = file_url(&dir.join("nowhere.txt"));
        // No retries: a missing file is missing however often it is asked for.
        let curl = Curl {
            retries: 0,
            program: "curl",
        };
        assert!(curl.get(&url).unwrap_err().contains("curl"));
        assert!(curl.download(&url, &dir.join("out.txt")).is_err());
    }
}
