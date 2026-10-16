// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the Windows registry, where Steam and GOG record their install folders.

use std::process::Command;

/// The registry values the finder reads: Steam's `SteamPath` and GOG's per-game `path`.
///
/// [`SystemRegistry`] is the real one; tests give the finder a fixed table.
pub trait Registry {
    /// The string value `value` of the key `key`, a full path such as
    /// `HKEY_CURRENT_USER\Software\Valve\Steam`, or `None` if it is missing or not a string.
    fn string(&self, key: &str, value: &str) -> Option<String>;
}

/// The host's registry, read with Windows' own `reg query` command so that no `unsafe`
/// code is needed. On other systems it has no values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SystemRegistry;

impl Registry for SystemRegistry {
    fn string(&self, key: &str, value: &str) -> Option<String> {
        if !cfg!(windows) {
            return None;
        }
        let output = Command::new("reg")
            .args(["query", key, "/v", value])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_reg_query(&String::from_utf8_lossy(&output.stdout), value)
    }
}

/// The data of `value` in the output of `reg query <key> /v <value>`, whose value lines
/// are the name, the type and the data separated by runs of spaces:
///
/// ```text
/// HKEY_CURRENT_USER\Software\Valve\Steam
///     SteamPath    REG_SZ    c:/program files (x86)/steam
/// ```
///
/// Only `REG_SZ` and `REG_EXPAND_SZ` values are strings; an empty string counts as missing.
pub(crate) fn parse_reg_query(output: &str, value: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim_start();
        let Some(rest) = strip_prefix_ignore_case(line, value) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        for kind in ["REG_EXPAND_SZ", "REG_SZ"] {
            if let Some(data) = rest.strip_prefix(kind)
                && data.starts_with(char::is_whitespace)
            {
                let data = data.trim();
                return (!data.is_empty()).then(|| data.to_owned());
            }
        }
    }
    None
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| text.get(prefix.len()..))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_string_values() {
        let output = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
        assert_eq!(
            parse_reg_query(output, "SteamPath").as_deref(),
            Some("c:/program files (x86)/steam")
        );
        assert_eq!(
            parse_reg_query(
                "    path    REG_EXPAND_SZ    D:\\GOG Games\\Quake\n",
                "PATH"
            )
            .as_deref(),
            Some(r"D:\GOG Games\Quake")
        );
    }

    #[test]
    fn ignores_other_values_types_and_empty_data() {
        for output in [
            "",
            "    SteamPathX    REG_SZ    c:/x\n",
            "    SteamPath    REG_DWORD    0x1\n",
            "    SteamPath    REG_SZX    c:/x\n",
            "    SteamPath    REG_SZ    \n",
            "    Steam\n",
        ] {
            assert_eq!(parse_reg_query(output, "SteamPath"), None, "{output:?}");
        }
    }

    #[test]
    fn the_system_registry_is_empty_off_windows() {
        if !cfg!(windows) {
            assert_eq!(
                SystemRegistry.string(r"HKEY_CURRENT_USER\Software\Valve\Steam", "SteamPath"),
                None
            );
        }
        assert_eq!(
            SystemRegistry.string(r"HKEY_CURRENT_USER\Software\QuakeC Studio\Missing", "x"),
            None
        );
    }
}
