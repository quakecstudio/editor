// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the game stores and launchers keep their files on each operating system, so
//! `qs-install` can look for the user's Quake without joining paths onto the home
//! directory itself (CONV-16).
//!
//! Only folders found from the environment are listed here. Windows' registry entries
//! (Steam's `SteamPath`, GOG's per-game `path`) are read by `qs-install`, which is where
//! `docs/game-data.md` describes every location.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{Platform, absolute_var, join_all};

/// The folders where game stores and launchers may keep Quake, for one platform.
///
/// Every list is in the order to search, and a folder whose environment variable is unset,
/// empty or relative is left out rather than reported as an error: a missing store is the
/// ordinary case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreDirs {
    steam: Vec<PathBuf>,
    gog_libraries: Vec<PathBuf>,
    heroic: Vec<PathBuf>,
}

impl StoreDirs {
    /// Steam client folders: each may hold `config/libraryfolders.vdf` and `steamapps/`.
    ///
    /// macOS: `~/Library/Application Support/Steam`, used by the Steam client and by
    /// steamcmd. Linux: `~/.steam/steam`, `~/.local/share/Steam` and the two Flatpak
    /// equivalents under `~/.var/app/com.valvesoftware.Steam`, as Ironwail searches them.
    /// Windows: none; Steam's folder comes from the registry.
    #[must_use]
    pub fn steam(&self) -> &[PathBuf] {
        &self.steam
    }

    /// Folders that hold one subfolder per installed GOG game.
    ///
    /// Windows: GOG Galaxy's default `%ProgramFiles(x86)%\GOG Galaxy\Games`, and the
    /// offline installers' default `%SystemDrive%\GOG Games`. Elsewhere: none.
    #[must_use]
    pub fn gog_libraries(&self) -> &[PathBuf] {
        &self.gog_libraries
    }

    /// Heroic Games Launcher configuration folders, each of which may hold
    /// `gog_store/installed.json`.
    ///
    /// Linux: `$XDG_CONFIG_HOME/heroic` (`~/.config/heroic`) and the Flatpak's
    /// `~/.var/app/com.heroicgameslauncher.hgl/config/heroic`. Windows: `%APPDATA%\heroic`.
    /// macOS: none.
    #[must_use]
    pub fn heroic(&self) -> &[PathBuf] {
        &self.heroic
    }
}

/// The store folders for the running platform, from the process environment.
#[must_use]
pub fn current_store_dirs() -> StoreDirs {
    store_dirs(Platform::current(), std::env::var_os)
}

/// The store folders for `platform`, reading variables through `env`.
#[must_use]
pub fn store_dirs<F>(platform: Platform, env: F) -> StoreDirs
where
    F: Fn(&'static str) -> Option<OsString>,
{
    let var = |name: &'static str| absolute_var(platform, &env, name);
    let under = |base: &Option<PathBuf>, parts: &[&str]| -> Option<PathBuf> {
        base.as_ref().map(|base| join_all(base, parts))
    };
    match platform {
        Platform::MacOs => {
            let home = var("HOME");
            StoreDirs {
                steam: under(&home, &["Library", "Application Support", "Steam"])
                    .into_iter()
                    .collect(),
                gog_libraries: Vec::new(),
                heroic: Vec::new(),
            }
        }
        Platform::Windows => {
            let program_files = var("ProgramFiles(x86)");
            let system_root = env("SystemDrive").and_then(|drive| {
                let bytes = drive.as_encoded_bytes();
                let is_drive = matches!(bytes, [letter, b':'] if letter.is_ascii_alphabetic());
                is_drive.then(|| {
                    let mut root = drive.clone();
                    root.push("\\");
                    PathBuf::from(root)
                })
            });
            StoreDirs {
                steam: Vec::new(),
                gog_libraries: [
                    under(&program_files, &["GOG Galaxy", "Games"]),
                    under(&system_root, &["GOG Games"]),
                ]
                .into_iter()
                .flatten()
                .collect(),
                heroic: under(&var("APPDATA"), &["heroic"]).into_iter().collect(),
            }
        }
        Platform::Linux => {
            let home = var("HOME");
            let config = var("XDG_CONFIG_HOME").or_else(|| under(&home, &[".config"]));
            let steam_flatpak = under(&home, &[".var", "app", "com.valvesoftware.Steam"]);
            StoreDirs {
                steam: [
                    under(&home, &[".steam", "steam"]),
                    under(&home, &[".local", "share", "Steam"]),
                    under(&steam_flatpak, &[".steam", "steam"]),
                    under(&steam_flatpak, &[".local", "share", "Steam"]),
                ]
                .into_iter()
                .flatten()
                .collect(),
                gog_libraries: Vec::new(),
                heroic: [
                    under(&config, &["heroic"]),
                    under(
                        &home,
                        &[
                            ".var",
                            "app",
                            "com.heroicgameslauncher.hgl",
                            "config",
                            "heroic",
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(vars: &[(&'static str, &str)]) -> impl Fn(&'static str) -> Option<OsString> + use<> {
        let vars: Vec<(&'static str, OsString)> = vars
            .iter()
            .map(|(name, value)| (*name, OsString::from(value)))
            .collect();
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn path(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn macos_looks_in_application_support() {
        let dirs = store_dirs(Platform::MacOs, env_of(&[("HOME", "/Users/q")]));
        assert_eq!(
            dirs.steam(),
            [path(&[
                "/Users/q",
                "Library",
                "Application Support",
                "Steam"
            ])]
        );
        assert!(dirs.gog_libraries().is_empty());
        assert!(dirs.heroic().is_empty());
    }

    #[test]
    fn linux_lists_native_and_flatpak_folders() {
        let dirs = store_dirs(Platform::Linux, env_of(&[("HOME", "/home/q")]));
        let flatpak = path(&["/home/q", ".var", "app", "com.valvesoftware.Steam"]);
        assert_eq!(
            dirs.steam(),
            [
                path(&["/home/q", ".steam", "steam"]),
                path(&["/home/q", ".local", "share", "Steam"]),
                flatpak.join(".steam").join("steam"),
                flatpak.join(".local").join("share").join("Steam"),
            ]
        );
        assert_eq!(
            dirs.heroic(),
            [
                path(&["/home/q", ".config", "heroic"]),
                path(&[
                    "/home/q",
                    ".var",
                    "app",
                    "com.heroicgameslauncher.hgl",
                    "config",
                    "heroic"
                ]),
            ]
        );
        assert!(dirs.gog_libraries().is_empty());
    }

    #[test]
    fn linux_honours_xdg_config_home_for_heroic() {
        let env = env_of(&[("HOME", "/home/q"), ("XDG_CONFIG_HOME", "/cfg")]);
        let dirs = store_dirs(Platform::Linux, env);
        assert_eq!(dirs.heroic().first(), Some(&path(&["/cfg", "heroic"])));
    }

    #[test]
    fn windows_lists_gog_libraries_and_heroic() {
        let env = env_of(&[
            ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
            ("SystemDrive", "C:"),
            ("APPDATA", r"C:\Users\q\AppData\Roaming"),
        ]);
        let dirs = store_dirs(Platform::Windows, env);
        assert!(dirs.steam().is_empty());
        assert_eq!(
            dirs.gog_libraries(),
            [
                path(&[r"C:\Program Files (x86)", "GOG Galaxy", "Games"]),
                path(&["C:\\", "GOG Games"]),
            ]
        );
        assert_eq!(
            dirs.heroic(),
            [path(&[r"C:\Users\q\AppData\Roaming", "heroic"])]
        );
    }

    #[test]
    fn missing_empty_and_relative_variables_drop_their_folders() {
        assert_eq!(
            store_dirs(Platform::Linux, env_of(&[])),
            StoreDirs::default()
        );
        assert_eq!(
            store_dirs(Platform::MacOs, env_of(&[("HOME", "")])),
            StoreDirs::default()
        );
        assert_eq!(
            store_dirs(Platform::MacOs, env_of(&[("HOME", "q")])),
            StoreDirs::default()
        );
        let env = env_of(&[
            ("ProgramFiles(x86)", "Program Files"),
            ("SystemDrive", "C:\\x"),
            ("APPDATA", "/home/q"),
        ]);
        assert_eq!(store_dirs(Platform::Windows, env), StoreDirs::default());
    }

    #[test]
    fn current_store_dirs_does_not_fail() {
        let dirs = current_store_dirs();
        assert_eq!(dirs, store_dirs(Platform::current(), std::env::var_os));
    }
}
