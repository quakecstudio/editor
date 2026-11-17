// SPDX-License-Identifier: GPL-3.0-or-later

//! The host xtask runs on: the target triple its toolchain reports.

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
    use super::parse_host;

    #[test]
    fn host_line() {
        let version = "rustc 1.99.0-nightly (abc 2026-10-01)\nbinary: rustc\n\
                       host: x86_64-unknown-linux-gnu\nrelease: 1.99.0-nightly\n";
        assert_eq!(parse_host(version), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(parse_host("rustc 1.99.0\nrelease: 1.99.0\n"), None);
        assert_eq!(parse_host("host:   \n"), None);
    }
}
