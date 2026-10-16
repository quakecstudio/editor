// SPDX-License-Identifier: GPL-3.0-or-later

//! `corpus/manifest.toml` as data. `corpus/README.md` explains every field.
//!
//! Every field has a default so that a missing one is reported by [`super::verify::check`]
//! with the entry it belongs to, not as a parse error; an unknown field is a parse error, so a
//! misspelt field cannot pass unchecked.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The whole manifest.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    #[serde(default)]
    pub(crate) schema: Option<i64>,
    /// Ids no tool may fetch.
    #[serde(default)]
    pub(crate) never: Vec<String>,
    /// Directories whose tracked Quake content is this project's own fixtures.
    #[serde(default)]
    pub(crate) fixture_dirs: Vec<String>,
    /// URL prefixes no source may point at.
    #[serde(default)]
    pub(crate) blocked_urls: Vec<String>,
    #[serde(default)]
    pub(crate) source: Vec<Source>,
    #[serde(default)]
    pub(crate) codebase: Vec<Entry>,
    #[serde(default)]
    pub(crate) map: Vec<Entry>,
    #[serde(default)]
    pub(crate) asset: Vec<Entry>,
    #[serde(default)]
    pub(crate) tree: Vec<Tree>,
    /// The directory the manifest was read from, which a codebase's `project` is relative to.
    #[serde(skip)]
    pub(crate) dir: PathBuf,
}

/// Codebases laid out together as the directory tree their sources expect.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tree {
    #[serde(default)]
    pub(crate) id: String,
    // Read only by people.
    #[serde(default, rename = "name")]
    pub(crate) _name: String,
    #[serde(default, rename = "notes")]
    pub(crate) _notes: String,
    /// The codebase written to each directory of the tree, by path in the tree.
    #[serde(default)]
    pub(crate) dirs: BTreeMap<String, String>,
}

/// A repository at a pinned commit.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) commit: String,
    /// SHA-256 of pinned files, by path in the repository.
    #[serde(default)]
    pub(crate) checksums: BTreeMap<String, String>,
}

/// A codebase, map or test asset. Fields that only one table uses stay empty in the others.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    #[serde(default)]
    pub(crate) id: String,
    // Read only by people.
    #[serde(default, rename = "name")]
    pub(crate) _name: String,
    #[serde(default)]
    pub(crate) kind: String,
    pub(crate) source: Option<String>,
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) license: String,
    #[serde(default, rename = "use")]
    pub(crate) use_: String,
    #[serde(default)]
    pub(crate) evidence: Vec<String>,
    #[serde(default)]
    pub(crate) reason: String,
    #[serde(default, rename = "notes")]
    pub(crate) _notes: String,
    #[serde(default)]
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) sha256: String,
    #[serde(default)]
    pub(crate) strip: String,
    #[serde(default)]
    pub(crate) game_dir: String,
    /// Archive members to extract, each with its SHA-256.
    #[serde(default)]
    pub(crate) members: BTreeMap<String, String>,
    /// Codebases whose precaches the asset must satisfy.
    #[serde(default)]
    pub(crate) covers: Vec<String>,
    /// Precached file, and the file in the paks that stands in for it.
    #[serde(default)]
    pub(crate) stand_ins: BTreeMap<String, String>,
    /// A codebase's `quakec.toml` in this repository, relative to the manifest's directory.
    pub(crate) project: Option<String>,
}

impl Entry {
    /// Whether the entry may be fetched at all.
    pub(crate) fn usable(&self) -> bool {
        self.use_ != "never"
    }
}

impl Manifest {
    /// Reads and parses the manifest at `path`.
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut m: Self = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        m.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(m)
    }

    /// Every codebase, map and asset, with the name of its table.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&'static str, &Entry)> {
        let codebases = self.codebase.iter().map(|e| ("codebase", e));
        let maps = self.map.iter().map(|e| ("map", e));
        let assets = self.asset.iter().map(|e| ("asset", e));
        codebases.chain(maps).chain(assets)
    }

    /// The source called `id`.
    pub(crate) fn source(&self, id: &str) -> Option<&Source> {
        self.source.iter().find(|s| s.id == id)
    }

    /// The tree called `id`.
    pub(crate) fn tree(&self, id: &str) -> Option<&Tree> {
        self.tree.iter().find(|t| t.id == id)
    }

    /// The codebase, map or asset called `id`, with its table.
    pub(crate) fn entry(&self, id: &str) -> Option<(&'static str, &Entry)> {
        self.entries().find(|(_, e)| e.id == id)
    }
}
