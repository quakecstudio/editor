// SPDX-License-Identifier: GPL-3.0-or-later

//! The host xtask runs on.

/// The target triple of the host xtask runs on: the triple the toolchain that
/// built xtask builds for, which is the one cargo names in `CARGO_TARGET_*`
/// variables and the `cc` crate expects.
pub(crate) fn host_triple() -> Result<&'static str, String> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => return Err(format!("no known target triple for {arch} {os}")),
    };
    Ok(triple)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::host_triple;

    #[test]
    fn the_host_triple_names_this_os_and_architecture() {
        let triple = host_triple().unwrap();
        assert!(triple.starts_with(std::env::consts::ARCH), "{triple}");
        let os = [
            ("macos", "-apple-darwin"),
            ("linux", "-unknown-linux-gnu"),
            ("windows", "-pc-windows-msvc"),
        ]
        .into_iter()
        .find(|(os, _)| *os == std::env::consts::OS)
        .map(|(_, suffix)| suffix);
        assert_eq!(
            os.map(|suffix| triple.ends_with(suffix)),
            Some(true),
            "{triple}"
        );
    }
}
