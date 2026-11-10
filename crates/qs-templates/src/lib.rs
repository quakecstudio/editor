// SPDX-License-Identifier: GPL-3.0-or-later

//! New-project templates (P4-17, QA-12, LIC-5): a starter mod that builds and plays at once,
//! made from id's own QuakeC, fetched under its GPLv2 terms rather than shipped.
//!
//! - [`Template::Id1`]: id's QuakeC 1.01, `qcc/v101qc` of `id-Software/Quake-Tools` at
//!   `c0d1b91c74eb654365ac7755bc837e497caaca73`, in the vanilla dialect for the original game.
//! - [`Template::Rerelease`]: the rerelease's base-campaign QuakeC, `quakec` of
//!   `id-Software/quake-rerelease-qc` at `634eefab09a77eb7b5f5ca7078ba3d8784a91142`, in the
//!   FTEQCC dialect for the rerelease.
//!
//! Each file is fetched by that commit ([`GitHub`]) or read from a local copy ([`Folder`]) and
//! checked against its SHA-256 before it is written. The project then gets what makes it ours
//! to build and test:
//!
//! - `quakec.toml` with the template's dialect and target;
//! - `.gitignore` excluding `build/`, where the build output and the run roots go (ADR-0008);
//! - `COPYING`, the GPLv2 text both upstreams carry, and `NOTICE.md`, saying where the code
//!   came from, under what terms, and what was added;
//! - a sample test: `tests.qc`, added to `progs.src`, with a `test_` function, and
//!   `tests/start.scenario`, which `project.test` runs (P4-26, `docs/spec/qc-tests.md`).

mod files;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The id of the command that makes a new project from a template.
pub const NEW_PROJECT: &str = "project.new";

/// The GPLv2 text both upstreams carry (`qcc/COPYING`, `COPYING.txt`), the same bytes.
const COPYING: &str = include_str!("COPYING-GPL-2.0.txt");

/// The sample unit test, added to `progs.src` (QCT-1.1, QCT-2.2).
const TESTS_QC: &str = "// SPDX-License-Identifier: GPL-3.0-or-later\n\
// A sample unit test (docs/spec/qc-tests.md): a function named test_ that takes no\n\
// parameters passes when it returns and fails when it calls error. `quakec-ide test` and\n\
// the IDE's project.test run it.\n\
\n\
void() test_anglemod_wraps =\n\
{\n\
\tif (anglemod (370) != 10)\n\
\t\terror (\"anglemod (370) is not 10\");\n\
\tif (anglemod (-90) != 270)\n\
\t\terror (\"anglemod (-90) is not 270\");\n\
};\n";

/// The sample scenario test (QCT-3, SCN-1): the player enters the start map with full health.
const START_SCENARIO: &str = "# A sample scenario test (docs/spec/scenario.md), played on the \
game's start map\n\
# when a game is found.\n\
map start\n\
frames 4\n\
expect field player classname == \"player\"\n\
expect field player health == 100\n";

/// A starter project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    /// id's QuakeC 1.01, for the original game.
    Id1,
    /// The rerelease's QuakeC, for Quake's 2021 rerelease.
    Rerelease,
}

/// Where a template's files come from.
#[derive(Clone, Copy, Debug)]
pub struct Upstream {
    /// The GitHub repository, `owner/name`.
    pub repository: &'static str,
    /// The commit.
    pub commit: &'static str,
    /// The directory of the QuakeC in it.
    pub directory: &'static str,
}

impl Template {
    /// Both templates.
    pub const ALL: [Self; 2] = [Self::Id1, Self::Rerelease];

    /// Where its files come from.
    #[must_use]
    pub const fn upstream(self) -> Upstream {
        match self {
            Self::Id1 => Upstream {
                repository: "id-Software/Quake-Tools",
                commit: "c0d1b91c74eb654365ac7755bc837e497caaca73",
                directory: "qcc/v101qc",
            },
            Self::Rerelease => Upstream {
                repository: "id-Software/quake-rerelease-qc",
                commit: "634eefab09a77eb7b5f5ca7078ba3d8784a91142",
                directory: "quakec",
            },
        }
    }

    /// Its files below [`Upstream::directory`], with their SHA-256.
    #[must_use]
    pub const fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Id1 => files::ID1,
            Self::Rerelease => files::RERELEASE,
        }
    }

    /// The `quakec.toml` dialect and target.
    const fn dialect_and_target(self) -> (&'static str, &'static str) {
        match self {
            Self::Id1 => ("vanilla", "quake"),
            Self::Rerelease => ("fteqcc", "rerelease"),
        }
    }

    /// What the notice calls the code.
    const fn title(self) -> &'static str {
        match self {
            Self::Id1 => "id Software's QuakeC 1.01",
            Self::Rerelease => "id Software's QuakeC for the Quake rerelease (base campaign)",
        }
    }
}

/// Reads one file of a template's upstream directory.
pub trait Fetch: Send + Sync {
    /// The bytes of `path`, below the upstream directory of `template`.
    ///
    /// # Errors
    ///
    /// Why the file could not be read.
    fn get(&self, template: Template, path: &str) -> Result<Vec<u8>, String>;
}

/// The files at their pinned commits on GitHub, over HTTPS.
pub struct GitHub {
    agent: ureq::Agent,
}

impl GitHub {
    /// A client whose requests may take a minute.
    #[must_use]
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_mins(1)))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for GitHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for GitHub {
    fn get(&self, template: Template, path: &str) -> Result<Vec<u8>, String> {
        let upstream = template.upstream();
        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{path}",
            upstream.repository, upstream.commit, upstream.directory
        );
        let mut response = self
            .agent
            .get(&url)
            .call()
            .map_err(|error| format!("{url}: {error}"))?;
        response
            .body_mut()
            .read_to_vec()
            .map_err(|error| format!("{url}: {error}"))
    }
}

/// A local copy of each upstream directory, as `cargo xtask corpus fetch` writes it: the
/// template's files under the folder given for it.
pub struct Folder {
    id1: Option<PathBuf>,
    rerelease: Option<PathBuf>,
}

impl Folder {
    /// The copies of id1's and the rerelease's QuakeC, where there are any.
    #[must_use]
    pub const fn new(id1: Option<PathBuf>, rerelease: Option<PathBuf>) -> Self {
        Self { id1, rerelease }
    }
}

impl Fetch for Folder {
    fn get(&self, template: Template, path: &str) -> Result<Vec<u8>, String> {
        let folder = match template {
            Template::Id1 => self.id1.as_ref(),
            Template::Rerelease => self.rerelease.as_ref(),
        }
        .ok_or_else(|| format!("no local copy of the {template:?} QuakeC was given"))?;
        let file = folder.join(path);
        std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))
    }
}

/// Why a project was not made.
#[derive(Debug)]
pub enum TemplateError {
    /// The directory exists and is not empty.
    NotEmpty(PathBuf),
    /// The directory's name cannot be a mod name (PRJ-2.3).
    BadName(PathBuf),
    /// A file could not be fetched.
    Fetch(String),
    /// A fetched file is not the one pinned.
    Checksum {
        /// The file.
        path: String,
        /// Its SHA-256, as fetched.
        found: String,
    },
    /// A file could not be written.
    Write(PathBuf, std::io::Error),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEmpty(dir) => write!(f, "{} is not empty", dir.display()),
            Self::BadName(dir) => write!(f, "{} cannot name a mod", dir.display()),
            Self::Fetch(why) => write!(f, "a template file could not be fetched: {why}"),
            Self::Checksum { path, found } => {
                write!(f, "{path} is not the pinned file: its SHA-256 is {found}")
            }
            Self::Write(path, error) => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for TemplateError {}

/// What [`create`] made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Created {
    /// The project's directory.
    pub path: PathBuf,
    /// The mod name, its directory's name.
    pub name: String,
    /// The files written, relative to the directory.
    pub files: Vec<String>,
}

/// Makes a project of `template` in `dir`, which must be new or empty, with files from
/// `fetch`.
///
/// # Errors
///
/// [`TemplateError`] when the directory is not empty or cannot name a mod, a file cannot be
/// fetched or is not the pinned one, or a file cannot be written.
pub fn create(template: Template, dir: &Path, fetch: &dyn Fetch) -> Result<Created, TemplateError> {
    make(template, template.files(), dir, fetch)
}

/// [`create`] with the files of `table` in place of the template's own.
fn make(
    template: Template,
    table: &[(&str, &str)],
    dir: &Path,
    fetch: &dyn Fetch,
) -> Result<Created, TemplateError> {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.contains([':', '\\', '/']))
        .ok_or_else(|| TemplateError::BadName(dir.to_path_buf()))?
        .to_owned();
    if dir.exists() {
        let mut entries = std::fs::read_dir(dir)
            .map_err(|error| TemplateError::Write(dir.to_path_buf(), error))?;
        if entries.next().is_some() {
            return Err(TemplateError::NotEmpty(dir.to_path_buf()));
        }
    }
    // Everything is fetched and checked before anything is written.
    let mut fetched = Vec::new();
    for (path, sha256) in table {
        let bytes = fetch.get(template, path).map_err(TemplateError::Fetch)?;
        let found = hex(&Sha256::digest(&bytes));
        if found != *sha256 {
            return Err(TemplateError::Checksum {
                path: (*path).to_owned(),
                found,
            });
        }
        fetched.push(((*path).to_owned(), bytes));
    }
    let mut files = Vec::new();
    for (path, mut bytes) in fetched {
        if path == "progs.src" {
            if !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(b"\ntests.qc\n");
        }
        write(dir, &path, &bytes, &mut files)?;
    }
    let (dialect, target) = template.dialect_and_target();
    let toml = format!(
        "[project]\nname = \"{name}\"\ndialect = \"{dialect}\"\ntarget = \"{target}\"\n\n\
         [[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n"
    );
    write(dir, "quakec.toml", toml.as_bytes(), &mut files)?;
    write(
        dir,
        ".gitignore",
        b"# The build output and the run roots (ADR-0008).\nbuild/\n",
        &mut files,
    )?;
    write(dir, "tests.qc", TESTS_QC.as_bytes(), &mut files)?;
    write(
        dir,
        "tests/start.scenario",
        START_SCENARIO.as_bytes(),
        &mut files,
    )?;
    write(dir, "COPYING", COPYING.as_bytes(), &mut files)?;
    write(dir, "NOTICE.md", notice(template).as_bytes(), &mut files)?;
    Ok(Created {
        path: dir.to_path_buf(),
        name,
        files,
    })
}

/// The notice: where the code came from, its terms, and what the template added.
fn notice(template: Template) -> String {
    let upstream = template.upstream();
    format!(
        "# Notice\n\n\
         This mod starts from {title}, copyright (C) 1996-1997 Id Software, Inc., from\n\
         `{directory}` of https://github.com/{repository} at commit `{commit}`.\n\n\
         id Software released that code under the GNU General Public License version 2, whose\n\
         text is in `COPYING`; each file's header grants version 2 or any later version. This\n\
         mod is distributed under the same terms.\n\n\
         QuakeC Studio added `quakec.toml`, `.gitignore`, `tests.qc` (listed last in\n\
         `progs.src`) and `tests/start.scenario`, and this notice.\n",
        title = template.title(),
        directory = upstream.directory,
        repository = upstream.repository,
        commit = upstream.commit,
    )
}

/// Writes `bytes` to `path` below `dir`, making its folders, and records it.
fn write(
    dir: &Path,
    path: &str,
    bytes: &[u8],
    files: &mut Vec<String>,
) -> Result<(), TemplateError> {
    let file = dir.join(path);
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| TemplateError::Write(folder.to_path_buf(), error))?;
    }
    std::fs::write(&file, bytes).map_err(|error| TemplateError::Write(file.clone(), error))?;
    files.push(path.to_owned());
    Ok(())
}

/// `bytes` as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The parameters of `project.new`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewProjectParams {
    /// The starter: `id1` for the original game, `rerelease` for the 2021 rerelease.
    pub template: Template,
    /// The new project's directory, which must be new or empty; its name is the mod's.
    pub path: PathBuf,
}

/// `project.new`: makes a project from a template.
struct NewProject {
    fetch: Arc<dyn Fetch>,
}

impl<C: 'static> Command<C> for NewProject {
    const ID: &'static str = NEW_PROJECT;
    const SUMMARY: &'static str = "Makes a new project from a starter template, id's QuakeC \
        1.01 or the rerelease's, with quakec.toml, a sample test and the GPLv2 notices; it \
        builds and plays at once.";
    type Params = NewProjectParams;
    type Output = Created;
    type Error = TemplateError;

    fn run(
        &self,
        _context: &mut C,
        params: NewProjectParams,
    ) -> Result<Outcome<Created, C>, TemplateError> {
        let created = create(params.template, &params.path, self.fetch.as_ref())?;
        Ok(Outcome::outside_history(created))
    }
}

/// Registers `project.new`, its files read through `fetch`.
///
/// # Errors
///
/// [`RegisterError`] when the id is taken.
pub fn register<C: 'static>(
    registry: &mut Registry<C>,
    fetch: Arc<dyn Fetch>,
) -> Result<(), RegisterError> {
    registry.register(NewProject { fetch })
}
