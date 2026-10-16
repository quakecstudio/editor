// SPDX-License-Identifier: GPL-3.0-or-later

//! Where Steam, GOG and the Heroic launcher record the folders they installed Quake into.

use std::path::{Path, PathBuf};

use qs_paths::Platform;
use qs_vfs::EntryKind;

use crate::vdf::{self, Value};
use crate::{
    Finder, GOG_QUAKE_ENHANCED_ID, GOG_QUAKE_ID, InstallFs, Registry, Source, manifest_name,
};

/// Steam's registry key on Windows, holding `SteamPath`.
const STEAM_KEY: &str = r"HKEY_CURRENT_USER\Software\Valve\Steam";

/// GOG's registry key for its games on 64-bit Windows; each game's subkey holds `path`.
const GOG_KEY: &str = r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games";

/// The install folder Steam uses when an app manifest gives none.
const STEAM_DEFAULT_INSTALLDIR: &str = "Quake";

impl<F: InstallFs + ?Sized, R: Registry + ?Sized> Finder<'_, F, R> {
    /// Every folder a store says it installed Quake into, with how it was found.
    pub(crate) fn store_folders(&self) -> Vec<(PathBuf, Source)> {
        let mut folders = Vec::new();
        for library in self.steam_libraries() {
            if let Some(folder) = self.steam_install(&library) {
                folders.push((folder, Source::Steam));
            }
        }
        if self.platform == Platform::Windows {
            for id in [GOG_QUAKE_ID, GOG_QUAKE_ENHANCED_ID] {
                if let Some(path) = self.registry.string(&format!(r"{GOG_KEY}\{id}"), "path") {
                    let path = path.trim_end_matches(['\\', '/']);
                    if !path.is_empty() {
                        folders.push((PathBuf::from(path), Source::Gog));
                    }
                }
            }
        }
        for library in self.stores.gog_libraries() {
            folders.extend(
                self.gog_library_games(library)
                    .into_iter()
                    .map(|folder| (folder, Source::Gog)),
            );
        }
        for heroic in self.stores.heroic() {
            folders.extend(
                self.heroic_games(heroic)
                    .into_iter()
                    .map(|folder| (folder, Source::Heroic)),
            );
        }
        folders
    }

    /// The Steam client folders, then every library they list, each once.
    fn steam_libraries(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if self.platform == Platform::Windows
            && let Some(path) = self.registry.string(STEAM_KEY, "SteamPath")
        {
            roots.push(PathBuf::from(path));
        }
        roots.extend(self.stores.steam().iter().cloned());
        let mut libraries: Vec<PathBuf> = Vec::new();
        let mut seen = Vec::new();
        for root in roots {
            let mut candidates = vec![root.clone()];
            candidates.extend(self.library_folders(&root));
            for library in candidates {
                let canonical = self.fs.canonical(&library);
                if !seen.contains(&canonical) {
                    seen.push(canonical);
                    libraries.push(library);
                }
            }
        }
        libraries
    }

    /// The library paths in a Steam client folder's `libraryfolders.vdf`: in `config/`
    /// (current clients, as Ironwail reads it) or `steamapps/` (older ones). Each library
    /// is a numbered block with a `path`, or in the oldest form a numbered string.
    fn library_folders(&self, root: &Path) -> Vec<PathBuf> {
        let files = [
            ["config", "libraryfolders.vdf"],
            ["steamapps", "libraryfolders.vdf"],
        ];
        let mut paths = Vec::new();
        for [dir, name] in files {
            let Some(text) = self
                .child(root, dir, EntryKind::Dir)
                .and_then(|dir| self.child(&dir, name, EntryKind::File))
                .and_then(|file| self.read_text(&file))
            else {
                continue;
            };
            let Some(folders) = vdf::parse(&text).and_then(|value| {
                value
                    .pairs()
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("libraryfolders"))
                    .map(|(_, value)| value.clone())
            }) else {
                continue;
            };
            for (key, value) in folders.pairs() {
                if !key.bytes().all(|byte| byte.is_ascii_digit()) {
                    continue;
                }
                let path = match value {
                    Value::Str(path) => Some(path.as_str()),
                    Value::Block(_) => value.get("path").and_then(Value::as_str),
                };
                if let Some(path) = path.filter(|path| !path.is_empty()) {
                    paths.push(PathBuf::from(path));
                }
            }
        }
        paths
    }

    /// Quake's folder in one Steam library: `steamapps/common/<installdir>`, with
    /// `installdir` from `steamapps/appmanifest_2310.acf`, or `Quake` without one.
    fn steam_install(&self, library: &Path) -> Option<PathBuf> {
        let steamapps = self.child(library, "steamapps", EntryKind::Dir)?;
        let installdir = self
            .child(&steamapps, &manifest_name(), EntryKind::File)
            .and_then(|manifest| self.read_text(&manifest))
            .and_then(|text| vdf::parse(&text))
            .and_then(|value| {
                value
                    .get("AppState")
                    .and_then(|state| state.get("installdir"))
                    .and_then(Value::as_str)
                    .filter(|dir| !dir.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| STEAM_DEFAULT_INSTALLDIR.to_owned());
        let common = self.child(&steamapps, "common", EntryKind::Dir)?;
        self.child(&common, &installdir, EntryKind::Dir)
    }

    /// The folders in a GOG library that hold `goggame-<id>.info` for Quake or Quake
    /// Enhanced, in name order.
    fn gog_library_games(&self, library: &Path) -> Vec<PathBuf> {
        let mut entries = self.fs.read_dir(library).unwrap_or_default();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Dir)
            .map(|entry| library.join(entry.name))
            .filter(|folder| {
                [GOG_QUAKE_ID, GOG_QUAKE_ENHANCED_ID].iter().any(|id| {
                    self.child(folder, &format!("goggame-{id}.info"), EntryKind::File)
                        .is_some()
                })
            })
            .collect()
    }

    /// The install paths Heroic records for Quake and Quake Enhanced in
    /// `gog_store/installed.json`: `{"installed": [{"appName": ..., "install_path": ...}]}`.
    fn heroic_games(&self, heroic: &Path) -> Vec<PathBuf> {
        let Some(text) = self
            .child(heroic, "gog_store", EntryKind::Dir)
            .and_then(|dir| self.child(&dir, "installed.json", EntryKind::File))
            .and_then(|file| self.read_text(&file))
        else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Vec::new();
        };
        let Some(installed) = json.get("installed").and_then(serde_json::Value::as_array) else {
            return Vec::new();
        };
        installed
            .iter()
            .filter(|game| {
                game.get("appName")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id == GOG_QUAKE_ID || id == GOG_QUAKE_ENHANCED_ID)
            })
            .filter_map(|game| game.get("install_path").and_then(serde_json::Value::as_str))
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect()
    }
}
