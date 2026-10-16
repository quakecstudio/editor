// SPDX-License-Identifier: GPL-3.0-or-later

//! Where QuakeC Studio keeps its files on each operating system.
//!
//! This is the single paths module (CONV-16): no other crate joins a path onto
//! the home directory. The layout is native per OS, as decided in
//! `docs/adr/0011-app-directories.md`:
//!
//! | Kind   | macOS                                   | Windows                          | Linux and other Unix                          |
//! | ------ | --------------------------------------- | -------------------------------- | --------------------------------------------- |
//! | config | `~/Library/Application Support/<App>`   | `%APPDATA%\<App>`                | `$XDG_CONFIG_HOME/<app>` (`~/.config`)        |
//! | data   | `~/Library/Application Support/<App>`   | `%LOCALAPPDATA%\<App>\data`      | `$XDG_DATA_HOME/<app>` (`~/.local/share`)     |
//! | cache  | `~/Library/Caches/<App>`                | `%LOCALAPPDATA%\<App>\cache`     | `$XDG_CACHE_HOME/<app>` (`~/.cache`)          |
//! | logs   | `~/Library/Logs/<App>`                  | `%LOCALAPPDATA%\<App>\logs`      | `$XDG_STATE_HOME/<app>/logs` (`~/.local/state`) |
//!
//! Resolution is a pure function of the platform and an environment lookup, so
//! every platform's rules are tested on every host.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// The directory name on macOS and Windows, where names are displayed to users.
///
/// Follows the product name chosen in ADR-0006.
pub const APP_DIR_NAME: &str = "QuakeC Studio";

/// The directory name on Linux and other Unix systems, where names are lowercase.
pub const APP_DIR_NAME_UNIX: &str = "quakec-studio";

/// An operating-system family with its own directory conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS: `~/Library` folders.
    MacOs,
    /// Windows: the roaming and local `AppData` folders.
    Windows,
    /// Linux and every other Unix: the XDG Base Directory specification.
    Linux,
}

impl Platform {
    /// The platform this binary was compiled for.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Why the application directories could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathsError {
    /// A required environment variable is unset or empty.
    MissingVar {
        /// The variable's name.
        name: &'static str,
    },
    /// A required environment variable holds a relative path.
    RelativeVar {
        /// The variable's name.
        name: &'static str,
        /// The value it holds.
        value: OsString,
    },
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVar { name } => {
                write!(f, "environment variable {name} is not set")
            }
            Self::RelativeVar { name, value } => write!(
                f,
                "environment variable {name} must be an absolute path, not {}",
                value.to_string_lossy()
            ),
        }
    }
}

impl std::error::Error for PathsError {}

/// The application's directories. None of them is created by resolving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
}

impl AppDirs {
    /// Settings the user edits or that should follow them between machines.
    #[must_use]
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Data the application keeps, such as indexes and agent checkpoints.
    #[must_use]
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Files that can be deleted at any time and rebuilt.
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Log files.
    #[must_use]
    pub fn logs(&self) -> &Path {
        &self.logs
    }
}

/// Resolves the directories for the running platform from the process environment.
///
/// # Errors
///
/// Returns [`PathsError`] when a variable the platform needs is unset, empty
/// or relative.
pub fn current() -> Result<AppDirs, PathsError> {
    resolve(Platform::current(), std::env::var_os)
}

/// Resolves the directories for `platform`, reading variables through `env`.
///
/// An empty variable counts as unset. On Linux an unset or relative `XDG_*`
/// variable falls back to its default under `HOME`, as the XDG Base Directory
/// specification requires.
///
/// # Errors
///
/// Returns [`PathsError`] when `HOME` (macOS, Linux) or `APPDATA` and
/// `LOCALAPPDATA` (Windows) are unset, empty or relative.
pub fn resolve<F>(platform: Platform, env: F) -> Result<AppDirs, PathsError>
where
    F: Fn(&'static str) -> Option<OsString>,
{
    match platform {
        Platform::MacOs => {
            let library = required(platform, &env, "HOME")?.join("Library");
            let support = library.join("Application Support").join(APP_DIR_NAME);
            Ok(AppDirs {
                config: support.clone(),
                data: support,
                cache: library.join("Caches").join(APP_DIR_NAME),
                logs: library.join("Logs").join(APP_DIR_NAME),
            })
        }
        Platform::Windows => {
            let roaming = required(platform, &env, "APPDATA")?.join(APP_DIR_NAME);
            let local = required(platform, &env, "LOCALAPPDATA")?.join(APP_DIR_NAME);
            Ok(AppDirs {
                config: roaming,
                data: local.join("data"),
                cache: local.join("cache"),
                logs: local.join("logs"),
            })
        }
        Platform::Linux => {
            let home = required(platform, &env, "HOME")?;
            let xdg = |name: &'static str, default: &[&str]| -> PathBuf {
                let base = env(name)
                    .filter(|value| is_absolute(platform, value))
                    .map_or_else(
                        || {
                            default
                                .iter()
                                .fold(home.clone(), |path, part| path.join(part))
                        },
                        PathBuf::from,
                    );
                base.join(APP_DIR_NAME_UNIX)
            };
            Ok(AppDirs {
                config: xdg("XDG_CONFIG_HOME", &[".config"]),
                data: xdg("XDG_DATA_HOME", &[".local", "share"]),
                cache: xdg("XDG_CACHE_HOME", &[".cache"]),
                logs: xdg("XDG_STATE_HOME", &[".local", "state"]).join("logs"),
            })
        }
    }
}

fn required<F>(platform: Platform, env: &F, name: &'static str) -> Result<PathBuf, PathsError>
where
    F: Fn(&'static str) -> Option<OsString>,
{
    let Some(value) = env(name).filter(|value| !value.is_empty()) else {
        return Err(PathsError::MissingVar { name });
    };
    if !is_absolute(platform, &value) {
        return Err(PathsError::RelativeVar { name, value });
    }
    Ok(PathBuf::from(value))
}

/// Whether `value` is an absolute path on `platform`, whatever the host is.
fn is_absolute(platform: Platform, value: &OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    match platform {
        Platform::MacOs | Platform::Linux => bytes.starts_with(b"/"),
        Platform::Windows => match bytes {
            [drive, b':', b'\\' | b'/', ..] => drive.is_ascii_alphabetic(),
            [b'\\', b'\\', ..] => true,
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

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
    fn macos_uses_library_folders() {
        let dirs = resolve(Platform::MacOs, env_of(&[("HOME", "/Users/q")])).unwrap();
        let support = path(&["/Users/q", "Library", "Application Support", APP_DIR_NAME]);
        assert_eq!(dirs.config(), support);
        assert_eq!(dirs.data(), support);
        assert_eq!(
            dirs.cache(),
            path(&["/Users/q", "Library", "Caches", APP_DIR_NAME])
        );
        assert_eq!(
            dirs.logs(),
            path(&["/Users/q", "Library", "Logs", APP_DIR_NAME])
        );
    }

    #[test]
    fn macos_ignores_xdg_variables() {
        let env = env_of(&[("HOME", "/Users/q"), ("XDG_CONFIG_HOME", "/x")]);
        let dirs = resolve(Platform::MacOs, env).unwrap();
        assert_eq!(
            dirs.config(),
            path(&["/Users/q", "Library", "Application Support", APP_DIR_NAME])
        );
    }

    #[test]
    fn windows_splits_roaming_config_from_local_data() {
        let env = env_of(&[
            ("APPDATA", r"C:\Users\q\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\q\AppData\Local"),
        ]);
        let dirs = resolve(Platform::Windows, env).unwrap();
        let local = path(&[r"C:\Users\q\AppData\Local", APP_DIR_NAME]);
        assert_eq!(
            dirs.config(),
            path(&[r"C:\Users\q\AppData\Roaming", APP_DIR_NAME])
        );
        assert_eq!(dirs.data(), local.join("data"));
        assert_eq!(dirs.cache(), local.join("cache"));
        assert_eq!(dirs.logs(), local.join("logs"));
    }

    #[test]
    fn windows_accepts_unc_paths() {
        let env = env_of(&[
            ("APPDATA", r"\\server\share\Roaming"),
            ("LOCALAPPDATA", "D:/Local"),
        ]);
        let dirs = resolve(Platform::Windows, env).unwrap();
        assert_eq!(
            dirs.config(),
            path(&[r"\\server\share\Roaming", APP_DIR_NAME])
        );
    }

    #[test]
    fn windows_requires_both_appdata_variables() {
        let env = env_of(&[("APPDATA", r"C:\Roaming")]);
        assert_eq!(
            resolve(Platform::Windows, env),
            Err(PathsError::MissingVar {
                name: "LOCALAPPDATA"
            })
        );
    }

    #[test]
    fn windows_rejects_relative_and_unix_style_paths() {
        for value in ["Roaming", "/home/q", "C:Roaming", "1:\\x"] {
            let env = env_of(&[("APPDATA", value), ("LOCALAPPDATA", r"C:\Local")]);
            assert_eq!(
                resolve(Platform::Windows, env),
                Err(PathsError::RelativeVar {
                    name: "APPDATA",
                    value: OsString::from(value)
                }),
                "{value}"
            );
        }
    }

    #[test]
    fn linux_defaults_under_home() {
        let dirs = resolve(Platform::Linux, env_of(&[("HOME", "/home/q")])).unwrap();
        assert_eq!(
            dirs.config(),
            path(&["/home/q", ".config", APP_DIR_NAME_UNIX])
        );
        assert_eq!(
            dirs.data(),
            path(&["/home/q", ".local", "share", APP_DIR_NAME_UNIX])
        );
        assert_eq!(
            dirs.cache(),
            path(&["/home/q", ".cache", APP_DIR_NAME_UNIX])
        );
        assert_eq!(
            dirs.logs(),
            path(&["/home/q", ".local", "state", APP_DIR_NAME_UNIX, "logs"])
        );
    }

    #[test]
    fn linux_honours_absolute_xdg_variables() {
        let env = env_of(&[
            ("HOME", "/home/q"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "/cache"),
            ("XDG_STATE_HOME", "/state"),
        ]);
        let dirs = resolve(Platform::Linux, env).unwrap();
        assert_eq!(dirs.config(), path(&["/cfg", APP_DIR_NAME_UNIX]));
        assert_eq!(dirs.data(), path(&["/data", APP_DIR_NAME_UNIX]));
        assert_eq!(dirs.cache(), path(&["/cache", APP_DIR_NAME_UNIX]));
        assert_eq!(dirs.logs(), path(&["/state", APP_DIR_NAME_UNIX, "logs"]));
    }

    #[test]
    fn linux_ignores_empty_and_relative_xdg_variables() {
        let env = env_of(&[
            ("HOME", "/home/q"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_CACHE_HOME", "relative/cache"),
        ]);
        let dirs = resolve(Platform::Linux, env).unwrap();
        assert_eq!(
            dirs.config(),
            path(&["/home/q", ".config", APP_DIR_NAME_UNIX])
        );
        assert_eq!(
            dirs.cache(),
            path(&["/home/q", ".cache", APP_DIR_NAME_UNIX])
        );
    }

    #[test]
    fn home_must_be_set_absolute_and_non_empty() {
        for platform in [Platform::MacOs, Platform::Linux] {
            assert_eq!(
                resolve(platform, env_of(&[])),
                Err(PathsError::MissingVar { name: "HOME" })
            );
            assert_eq!(
                resolve(platform, env_of(&[("HOME", "")])),
                Err(PathsError::MissingVar { name: "HOME" })
            );
            assert_eq!(
                resolve(platform, env_of(&[("HOME", "q")])),
                Err(PathsError::RelativeVar {
                    name: "HOME",
                    value: OsString::from("q")
                })
            );
        }
    }

    #[test]
    fn errors_name_the_variable() {
        assert_eq!(
            PathsError::MissingVar { name: "HOME" }.to_string(),
            "environment variable HOME is not set"
        );
        assert_eq!(
            PathsError::RelativeVar {
                name: "APPDATA",
                value: OsString::from("x")
            }
            .to_string(),
            "environment variable APPDATA must be an absolute path, not x"
        );
    }

    #[test]
    fn current_platform_matches_the_build_target() {
        let expected = if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        };
        assert_eq!(Platform::current(), expected);
    }

    #[test]
    fn deliberately_failing_test_for_issue_3() {
        assert_eq!(std::env::consts::FAMILY, "deliberately-failing");
    }
}
