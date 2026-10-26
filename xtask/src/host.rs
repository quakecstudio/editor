// SPDX-License-Identifier: GPL-3.0-or-later

//! The host xtask runs on: the target triple its toolchain reports.

use std::path::Path;
use std::process::Command;

use crate::exec::Exec;

/// The host triple of the cargo running xtask, from `cargo -vV`: the triple
/// cargo builds for without `--target`, and the one its `CARGO_TARGET_*`
/// variables name.
pub(crate) fn cargo_host(root: &Path, exec: &dyn Exec) -> Result<String, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = exec
        .output(Command::new(cargo).arg("-vV").current_dir(root))
        .map_err(|error| format!("could not run cargo -vV: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo -vV failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_host(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| "cargo -vV printed no host line".to_owned())
}

/// The triple on the `host:` line of `rustc -vV` or `cargo -vV` output.
pub(crate) fn parse_host(version: &str) -> Option<&str> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::path::Path;
    use std::process::Command;

    use super::{cargo_host, parse_host};
    use crate::exec::{Fake, Reply, System};

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn host_line() {
        let version = "rustc 1.99.0-nightly (abc 2026-10-01)\nbinary: rustc\n\
                       host: x86_64-unknown-linux-gnu\nrelease: 1.99.0-nightly\n";
        assert_eq!(parse_host(version), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(parse_host("rustc 1.99.0\nrelease: 1.99.0\n"), None);
        assert_eq!(parse_host("host:   \n"), None);
    }

    #[test]
    fn the_cargo_host_is_the_host_line_of_its_version_output() {
        let fake = Fake::new(|_: &Command| Reply {
            stdout: "cargo 1.99.0 (abc 2026-09-01)\nhost: aarch64-unknown-linux-gnu\n".to_owned(),
            ..Reply::default()
        });
        assert_eq!(
            cargo_host(root(), &fake).unwrap(),
            "aarch64-unknown-linux-gnu"
        );
        assert!(fake.seen.lock().unwrap()[0].ends_with(" -vV"));
        let failed = Fake::new(|_: &Command| Reply {
            code: 1,
            stderr: "bad".to_owned(),
            ..Reply::default()
        });
        let error = cargo_host(root(), &failed).unwrap_err();
        assert!(
            error.contains("cargo -vV failed") && error.contains("bad"),
            "{error}"
        );
        let silent = Fake::new(|_: &Command| Reply::exit(0));
        let error = cargo_host(root(), &silent).unwrap_err();
        assert!(error.contains("no host line"), "{error}");
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        let error = cargo_host(root(), &cannot).unwrap_err();
        assert!(error.contains("could not run cargo -vV"), "{error}");
    }

    #[test]
    fn the_real_cargo_names_this_host() {
        let host = cargo_host(root(), &System).unwrap();
        assert!(host.starts_with(std::env::consts::ARCH), "{host}");
    }
}
