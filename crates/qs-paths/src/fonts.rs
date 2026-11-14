// SPDX-License-Identifier: GPL-3.0-or-later

//! Where each operating system keeps its fonts, so the desktop app can draw a character its
//! bundled fonts lack in a font of the system's (ADR-0003) without joining paths itself
//! (CONV-16).
//!
//! The folders are the ones each system documents for installed fonts, as font databases such
//! as fontdb and fontconfig's defaults search them: macOS's system, local and user font
//! folders; Windows' `Fonts` folder and its per-user one; and on Linux the `fonts` folder of
//! each XDG data directory and the older `~/.fonts`.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{Platform, absolute_var, join_all};

/// The font folders of the running platform, from the process environment.
#[must_use]
pub fn current_font_dirs() -> Vec<PathBuf> {
    Platform::current().font_dirs(std::env::var_os)
}

impl Platform {
    /// The font folders of the platform, reading variables through `env`, in the order to search:
    /// the user's own fonts last. A folder whose variable is unset, empty or relative is left out.
    #[must_use]
    pub(crate) fn font_dirs<F>(self, env: F) -> Vec<PathBuf>
    where
        F: Fn(&'static str) -> Option<OsString>,
    {
        let var = |name: &'static str| absolute_var(self, &env, name);
        let under = |base: Option<PathBuf>, parts: &[&str]| base.map(|base| join_all(&base, parts));
        match self {
            Platform::MacOs => [
                Some(PathBuf::from("/System/Library/Fonts")),
                Some(PathBuf::from("/Library/Fonts")),
                under(var("HOME"), &["Library", "Fonts"]),
            ]
            .into_iter()
            .flatten()
            .collect(),
            Platform::Windows => [
                under(var("SystemRoot"), &["Fonts"]),
                under(var("LOCALAPPDATA"), &["Microsoft", "Windows", "Fonts"]),
            ]
            .into_iter()
            .flatten()
            .collect(),
            Platform::Linux => {
                let home = var("HOME");
                // XDG_DATA_DIRS is a list; its default is /usr/local/share and /usr/share.
                let system: Vec<PathBuf> = env("XDG_DATA_DIRS")
                    .filter(|dirs| !dirs.is_empty())
                    .map_or_else(
                        || {
                            vec![
                                PathBuf::from("/usr/local/share"),
                                PathBuf::from("/usr/share"),
                            ]
                        },
                        |dirs| {
                            dirs.to_string_lossy()
                                .split(':')
                                .filter(|dir| self.is_absolute(OsStr::new(dir)))
                                .map(PathBuf::from)
                                .collect()
                        },
                    );
                let data_home =
                    var("XDG_DATA_HOME").or_else(|| under(home.clone(), &[".local", "share"]));
                system
                    .into_iter()
                    .map(|dir| join_all(&dir, &["fonts"]))
                    .chain(under(data_home, &["fonts"]))
                    .chain(under(home, &[".fonts"]))
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use crate::Platform;

    fn env(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&'static str) -> Option<OsString> {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    /// Each path from its parts, joined as the host joins them.
    fn paths(list: &[&[&str]]) -> Vec<PathBuf> {
        list.iter().map(|parts| parts.iter().collect()).collect()
    }

    #[test]
    fn macos_searches_the_system_local_and_user_folders() {
        assert_eq!(
            Platform::MacOs.font_dirs(env(&[("HOME", "/Users/q")])),
            paths(&[
                &["/System/Library/Fonts"],
                &["/Library/Fonts"],
                &["/Users/q", "Library", "Fonts"],
            ])
        );
        assert_eq!(
            Platform::MacOs.font_dirs(env(&[("HOME", "relative")])),
            paths(&[&["/System/Library/Fonts"], &["/Library/Fonts"]]),
            "a relative home is left out"
        );
    }

    #[test]
    fn windows_searches_the_fonts_folder_and_the_users() {
        assert_eq!(
            Platform::Windows.font_dirs(env(&[
                ("SystemRoot", "C:\\Windows"),
                ("LOCALAPPDATA", "C:\\Users\\q\\AppData\\Local"),
            ]),),
            paths(&[
                &["C:\\Windows", "Fonts"],
                &[
                    "C:\\Users\\q\\AppData\\Local",
                    "Microsoft",
                    "Windows",
                    "Fonts"
                ],
            ])
        );
        assert!(Platform::Windows.font_dirs(env(&[])).is_empty());
    }

    #[test]
    fn linux_searches_each_xdg_data_folder_then_the_users() {
        assert_eq!(
            Platform::Linux.font_dirs(env(&[("HOME", "/home/q")])),
            paths(&[
                &["/usr/local/share", "fonts"],
                &["/usr/share", "fonts"],
                &["/home/q", ".local", "share", "fonts"],
                &["/home/q", ".fonts"],
            ])
        );
        assert_eq!(
            Platform::Linux.font_dirs(env(&[
                ("HOME", "/home/q"),
                ("XDG_DATA_DIRS", "/opt/share:relative"),
                ("XDG_DATA_HOME", "/data"),
            ])),
            paths(&[
                &["/opt/share", "fonts"],
                &["/data", "fonts"],
                &["/home/q", ".fonts"],
            ])
        );
        assert_eq!(
            Platform::Linux.font_dirs(env(&[("XDG_DATA_DIRS", "")])),
            paths(&[&["/usr/local/share", "fonts"], &["/usr/share", "fonts"]]),
            "an empty list is the default"
        );
    }
}
