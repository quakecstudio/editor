// SPDX-License-Identifier: GPL-3.0-or-later

//! `paths.open`: what the system's file manager or the command line hands the app, each in its
//! project (PRJ-1.6). A second launch of the desktop app sends it to the one running, over the
//! app's socket, as Zed's command line does.

use std::path::{Path, PathBuf};

use qc_project::{FsLoader, enclosing};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commands::HasWorkbench;
use crate::map_edit::{BSP_EXTENSION, MAP_EXTENSION};
use crate::{Workbench, WorkbenchError};

/// The id of the command that opens folders and files as the system's file manager does.
pub const OPEN_PATHS: &str = "paths.open";

/// The id of the command that takes what the last `paths.open` opened, once.
pub const TAKE_OPENED_PATHS: &str = "paths.take_opened";

/// Registers `paths.open` and `paths.take_opened`.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub(crate) fn register_open_paths<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(OpenPaths)?;
    registry.register(TakeOpenedPaths)
}

/// The parameters of `paths.open`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenPathsParams {
    /// Folders to open as projects, and files to open in the nearest project above each, in
    /// order; a path relative to the app's working directory is made absolute.
    pub paths: Vec<PathBuf>,
}

/// What `paths.open` opened: the last project, file and map of the paths it was given, and
/// those it could not open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathsOpened {
    /// The project open afterwards, when one opened.
    pub project: Option<PathBuf>,
    /// The last file opened in the editor.
    pub file: Option<PathBuf>,
    /// The last map opened in the map editor.
    pub map: Option<PathBuf>,
    /// The paths that did not open, each with why.
    pub not_opened: Vec<NotOpened>,
}

/// A path `paths.open` could not open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotOpened {
    /// The path.
    pub path: PathBuf,
    /// Why.
    pub reason: String,
}

/// `paths.open`.
struct OpenPaths;

impl<C: HasWorkbench + 'static> Command<C> for OpenPaths {
    const ID: &'static str = OPEN_PATHS;
    const SUMMARY: &'static str = "Opens folders as projects, and files in the nearest project \
        above each (a folder holding quakec.toml or progs.src): a .map or .bsp in the map \
        editor, any other file in the editor. A file outside every project opens in the open \
        project, or opens its folder when none is open.";
    type Params = OpenPathsParams;
    type Output = PathsOpened;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: OpenPathsParams,
    ) -> Result<Outcome<PathsOpened, C>, WorkbenchError> {
        let workbench = context.workbench();
        let opened = workbench.open_paths(params.paths);
        Ok(Outcome::outside_history(opened))
    }
}

/// `paths.take_opened`.
struct TakeOpenedPaths;

impl<C: HasWorkbench + 'static> Command<C> for TakeOpenedPaths {
    const ID: &'static str = TAKE_OPENED_PATHS;
    const SUMMARY: &'static str = "Takes what the last paths.open opened, once, or null: the \
        desktop app shows it and comes to the front when another launch sent the paths.";
    type Params = NoParams;
    type Output = Option<PathsOpened>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: NoParams,
    ) -> Result<Outcome<Option<PathsOpened>, C>, WorkbenchError> {
        Ok(Outcome::outside_history(
            context.workbench().opened_paths.take(),
        ))
    }
}

/// Which editor a file opens in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opens {
    /// A `.map` or `.bsp`: the map editor, which needs no project.
    MapEditor,
    /// Any other file: the editor.
    Editor,
}

impl Opens {
    /// The editor `path` opens in, by its extension.
    fn of(path: &Path) -> Self {
        let is_map = path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case(MAP_EXTENSION)
                || extension.eq_ignore_ascii_case(BSP_EXTENSION)
        });
        if is_map {
            Self::MapEditor
        } else {
            Self::Editor
        }
    }
}

impl<L: qc_project::Loader> Workbench<L> {
    /// Opens each of `paths` as `paths.open` says, and keeps what opened for
    /// `paths.take_opened`.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) -> PathsOpened {
        let mut opened = PathsOpened::default();
        for path in paths {
            let path = std::path::absolute(&path).unwrap_or(path);
            if let Err(error) = self.open_path(&path, &mut opened) {
                opened.not_opened.push(NotOpened {
                    path,
                    reason: error.to_string(),
                });
            }
        }
        opened.project = self.root().map(Path::to_path_buf);
        self.opened_paths = Some(opened.clone());
        opened
    }

    /// Opens one path, noting the file or map in `opened`.
    fn open_path(&mut self, path: &Path, opened: &mut PathsOpened) -> Result<(), WorkbenchError> {
        if path.is_dir() {
            return self.open_project(path);
        }
        let opens = Opens::of(path);
        let open = self.root().map(Path::to_path_buf);
        let project = match enclosing(&mut FsLoader, path) {
            Some(dir) if open.as_ref() != Some(&dir) => Some(dir),
            Some(_) => None,
            None if open.is_none() && opens == Opens::Editor => {
                path.parent().map(Path::to_path_buf)
            }
            None => None,
        };
        if let Some(project) = project {
            self.open_project(&project)?;
        }
        match opens {
            Opens::MapEditor => {
                self.open_map(path)?;
                opened.map = Some(path.to_path_buf());
            }
            Opens::Editor => {
                self.open_file(path, 0)?;
                opened.file = Some(path.to_path_buf());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: unwraps state the expected value"
    )]

    use std::fs;

    use qs_commands::Session;
    use serde_json::json;

    use super::*;
    use crate::{Config, register};

    /// A folder under the system's temporary folder, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("qs-workbench-{}-{name}", std::process::id()));
            let _ignored = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    fn session() -> Session<Workbench<FsLoader>> {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        Session::new(Workbench::new(FsLoader, Config::default()), registry)
    }

    fn open(session: &mut Session<Workbench<FsLoader>>, paths: &[PathBuf]) -> PathsOpened {
        let output = session
            .execute(OPEN_PATHS, json!({ "paths": paths }))
            .unwrap();
        serde_json::from_value(output).unwrap()
    }

    // spec PRJ-1.6: a file opens in the nearest project above it, which opens first unless it
    // is open; a map outside every project opens in the open one; a file with no project above
    // it opens its folder when none is open; a folder is a project; a path that does not open
    // is reported and the rest still open
    #[test]
    fn paths_open_in_their_projects() {
        let dir = TempDir::new("open-paths");
        let root = &dir.0;
        let project = root.join("mod/qc");
        fs::create_dir_all(project.join("client")).unwrap();
        fs::write(project.join("progs.src"), "progs.dat\nclient/player.qc\n").unwrap();
        fs::write(project.join("client/player.qc"), "void() player = {};\n").unwrap();
        fs::create_dir_all(root.join("maps")).unwrap();
        fs::write(
            root.join("maps/start.map"),
            "{\n\"classname\" \"worldspawn\"\n}\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("loose")).unwrap();
        fs::write(root.join("loose/a.qc"), "void() a = {};\n").unwrap();

        let mut session = session();
        let player = project.join("client/player.qc");
        let start = root.join("maps/start.map");
        let missing = root.join("maps/missing.map");
        let opened = open(
            &mut session,
            &[player.clone(), missing.clone(), start.clone()],
        );
        assert_eq!(opened.project, Some(project.clone()));
        assert_eq!(opened.file, Some(player.clone()));
        assert_eq!(opened.map, Some(start));
        assert_eq!(
            opened
                .not_opened
                .iter()
                .map(|n| &n.path)
                .collect::<Vec<_>>(),
            [&missing]
        );
        let take = |session: &mut Session<Workbench<FsLoader>>| {
            session.execute(TAKE_OPENED_PATHS, json!({})).unwrap()
        };
        assert_eq!(
            take(&mut session),
            serde_json::to_value(Some(&opened)).unwrap()
        );
        assert_eq!(take(&mut session), serde_json::Value::Null);

        let loose = root.join("loose/a.qc");
        let opened = open(&mut session, std::slice::from_ref(&loose));
        assert_eq!(opened.project, Some(project.clone()));
        assert_eq!(opened.file, Some(loose.clone()));

        let mut session = super::tests::session();
        assert_eq!(
            open(&mut session, &[loose]).project,
            Some(root.join("loose"))
        );
        assert_eq!(
            open(&mut session, std::slice::from_ref(&project)).project,
            Some(project)
        );
        assert_eq!(
            open(&mut session, &[]),
            PathsOpened {
                project: Some(root.join("mod/qc")),
                ..PathsOpened::default()
            }
        );
    }
}
