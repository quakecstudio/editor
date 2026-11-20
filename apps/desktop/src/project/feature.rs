// SPDX-License-Identifier: GPL-3.0-or-later

//! The project's feature (P6-41): the recent projects, why a project did not open, a new
//! project and an export, the user's settings file, and the guide to the game's files, which the
//! shell used to hold. The user's settings file stays with the shell, which reads it.

use std::path::{Path, PathBuf};

use eframe::egui;
use qs_settings::{Scope as SettingsScope, Setting, SettingsStore};
use qs_templates::{NEW_PROJECT, NewProjectParams, Template};
use qs_workbench::export::{ExportParams, Exported, PROJECT_EXPORT};
use qs_workbench::game_files::GameFiles;
use qs_workbench::{
    CHOOSE_GAME, ChooseGameParams, GAME, OPEN_PROJECT, OpenProjectParams, RecentProjects,
};

use crate::features::Commands;
use crate::game_guide::{self, Guide, GuideAction};
use crate::new_project::NewProject;
use crate::shell::{ShellError, Workspace};
use crate::strings::Text;
use crate::theme::Theme;

/// The project's state: the recent projects and their file, the last project noted, why a
/// project did not open, the settings file, the last export, and the guide to the game's files.
#[derive(Default)]
pub(crate) struct ProjectFeature {
    /// The projects opened most recently.
    recent: RecentProjects,
    /// The file they are kept in.
    recent_file: Option<PathBuf>,
    /// The project last put first in the recent projects.
    seen_root: Option<PathBuf>,
    /// Why the project asked for did not open, shown in the Project panel.
    pub(crate) error: Option<String>,
    /// Where the last export wrote its zip, or why it did not (P4-28).
    pub(crate) export_note: Option<Result<PathBuf, String>>,
    /// Whether the guide to the game's files is shown (P3-21).
    guide: Guide,
    /// What the game folder holds and lacks, for the guide.
    guide_files: Option<GameFiles>,
    /// The New Project dialog (L-28).
    new_project: NewProject,
    /// What Play would read the game from, for the Welcome page, with the game folder chosen
    /// when it was looked for; `None` before the first look.
    found: Option<(Option<PathBuf>, GameFiles)>,
}

impl ProjectFeature {
    /// Reads the recent projects from `file`, and keeps them there.
    pub(crate) fn set_recent_file(&mut self, file: PathBuf) {
        self.recent = RecentProjects::load(&file);
        self.recent_file = Some(file);
    }

    /// The projects opened most recently, the latest first.
    pub(crate) fn recent_projects(&self) -> &[PathBuf] {
        self.recent.projects()
    }

    /// Opens the project in `path`, through its command.
    ///
    /// # Errors
    ///
    /// [`ShellError`] when the command fails.
    pub(crate) fn open_project(commands: &mut Commands<'_>, path: &Path) -> Result<(), ShellError> {
        let params = serde_json::to_value(OpenProjectParams {
            path: path.to_path_buf(),
        })?;
        commands.execute(OPEN_PROJECT, params)?;
        Ok(())
    }

    /// Opens the project in `folder`, saying why when it does not open; a recent project that
    /// no longer opens comes off the list.
    pub(crate) fn open(&mut self, commands: &mut Commands<'_>, folder: &Path) {
        // A folder with no project file opens empty, as Zed opens any folder; one that is not
        // there any more, as a recent project may be, does not.
        let opened = if folder.is_dir() {
            Self::open_project(commands, folder).map_err(|error| error.to_string())
        } else {
            Err(format!("{} {}", Text::FolderGone.get(), folder.display()))
        };
        match opened {
            Ok(()) => self.error = None,
            Err(error) => {
                self.error = Some(error);
                self.recent.forget(folder);
                self.save_recent();
            }
        }
    }

    /// Shows the New Project dialog, the games the game folder (or the first install found)
    /// holds deciding which is chosen to begin with.
    pub(crate) fn ask_new_project(&mut self, commands: &Commands<'_>) {
        let games = commands.session().context().preview.games();
        self.new_project.open(games);
    }

    /// The New Project dialog while it is shown: Choose Folder makes the project.
    pub(crate) fn new_project_dialog(
        &mut self,
        commands: &mut Commands<'_>,
        (ctx, theme): (&egui::Context, &Theme),
    ) {
        if let Some(template) = self.new_project.show(ctx, theme) {
            self.new_project(commands, template);
        }
    }

    /// Asks for a new folder with the system's dialog, makes a project of `template` there
    /// (`project.new`, P4-17) and opens it, saying why when it could not.
    fn new_project(&mut self, commands: &mut Commands<'_>, template: Template) {
        let picked = rfd::FileDialog::new()
            .set_title(Text::NewProjectTitle.get())
            .save_file();
        let Some(folder) = picked else {
            return;
        };
        let params = NewProjectParams {
            template,
            path: folder.clone(),
        };
        let made = serde_json::to_value(params)
            .map_err(|error| error.to_string())
            .and_then(|params| {
                commands
                    .execute(NEW_PROJECT, params)
                    .map_err(|error| error.to_string())
            });
        match made {
            Ok(_) => self.open(commands, &folder),
            Err(error) => self.error = Some(error),
        }
    }

    /// Exports the open project for players (`project.export`, P4-28), checked against the game
    /// folder Play reads (the first install found when none was chosen); the status bar says
    /// where the zip is or why there is none.
    pub(crate) fn export(&mut self, commands: &mut Commands<'_>) {
        let game = commands
            .session()
            .context()
            .preview
            .game_folder()
            .map(Path::to_path_buf);
        let params = ExportParams {
            game,
            ..ExportParams::default()
        };
        let exported = serde_json::to_value(params)
            .map_err(|error| error.to_string())
            .and_then(|params| {
                commands
                    .execute(PROJECT_EXPORT, params)
                    .map_err(|error| error.to_string())
            })
            .and_then(|answer| {
                serde_json::from_value::<Exported>(answer).map_err(|error| error.to_string())
            });
        self.export_note = Some(exported.map(|exported| exported.path));
    }

    /// Puts the project just opened in `workspace` first in the recent projects.
    pub(crate) fn note_project(&mut self, workspace: &Workspace) {
        let root = workspace.workbench.root().map(Path::to_path_buf);
        if root == self.seen_root {
            return;
        }
        if let Some(root) = &root
            && self.recent.opened(root)
        {
            self.save_recent();
        }
        self.seen_root = root;
    }

    /// Writes the recent projects to their file, if they have one.
    fn save_recent(&self) {
        if let Some(file) = &self.recent_file
            && let Err(error) = self.recent.save(file)
        {
            tracing::warn!(%error, "recent projects not written");
        }
    }

    /// Chooses `path` as the game folder, read in place (`preview.choose_game`), and keeps it
    /// in the settings file when it plays, so the next start finds it (P3-44).
    pub(crate) fn choose_game_folder(
        commands: &mut Commands<'_>,
        settings_file: Option<&Path>,
        path: &Path,
    ) -> Option<GameFiles> {
        let chosen: Option<GameFiles> = commands.run(
            CHOOSE_GAME,
            &ChooseGameParams {
                path: path.to_path_buf(),
            },
        );
        if chosen.as_ref().is_some_and(|files| files.state.plays()) {
            commands.set_notice(None);
            if let Some(file) = settings_file {
                let value = serde_json::to_value(path).ok();
                let saved = SettingsStore::open(file.to_path_buf(), None).and_then(|mut store| {
                    store.put(SettingsScope::User, Setting::GameFolder, value)
                });
                if let Err(error) = saved {
                    tracing::warn!(%error, "the game folder was not saved");
                }
            }
        }
        chosen
    }

    /// P3-21: Play that failed for want of the game opens the guide to its files; the guide
    /// stays hidden when the files are all there.
    pub(crate) fn open_guide_to_missing_files(&mut self, commands: &mut Commands<'_>) {
        let files: Option<GameFiles> = commands.run(GAME, &serde_json::json!({}));
        if let Some(files) = files {
            self.guide = Guide::for_files(&files);
            self.guide_files = Some(files);
        }
    }

    /// Looks for the game, so the Welcome page says what Play would read: the chosen folder,
    /// or the first install on this machine.
    pub(crate) fn look_for_game(&mut self, commands: &mut Commands<'_>) {
        let chosen = Self::chosen_game(commands);
        self.found = commands
            .run(GAME, &serde_json::json!({}))
            .map(|files| (chosen, files));
    }

    /// The game folder chosen for Play, if one was.
    fn chosen_game(commands: &Commands<'_>) -> Option<PathBuf> {
        let preview = &commands.session().context().preview;
        preview.game_folder().map(Path::to_path_buf)
    }

    /// What Play would read the game from, for the Welcome page. Once the game was looked for
    /// it is looked for again whenever the folder chosen changed, however it was chosen: the
    /// guide, the palette, the agent or MCP.
    pub(crate) fn game_found(&mut self, commands: &mut Commands<'_>) -> Option<&GameFiles> {
        let looked = self.found.as_ref().map(|(chosen, _)| chosen);
        if looked.is_some_and(|chosen| *chosen != Self::chosen_game(commands)) {
            self.look_for_game(commands);
        }
        self.found.as_ref().map(|(_, files)| files)
    }

    /// Opens the guide to the game's files (P3-21), from the File menu: what the game folder,
    /// or the first install found, holds and lacks, and the folder picker.
    pub(crate) fn show_game_guide(&mut self, commands: &mut Commands<'_>) {
        self.guide_files = commands.run(GAME, &serde_json::json!({}));
        self.guide = Guide::Shown;
    }

    /// The guide to the game's files while it is shown: a chosen folder is read in place and
    /// kept for Play when it plays (`preview.choose_game`), so Play works without a restart.
    pub(crate) fn game_guide(
        &mut self,
        commands: &mut Commands<'_>,
        settings_file: Option<&Path>,
        (ctx, theme): (&egui::Context, &Theme),
    ) {
        let Some(files) = &self.guide_files else {
            return;
        };
        match game_guide::window(ctx, theme, self.guide, files) {
            Some(GuideAction::Close) => self.guide = Guide::Hidden,
            Some(GuideAction::ChooseFolder) => {
                let picked = rfd::FileDialog::new()
                    .set_title(Text::ChooseGameTitle.get())
                    .pick_folder();
                if let Some(path) = picked {
                    let chosen = Self::choose_game_folder(commands, settings_file, &path);
                    if chosen.is_some() {
                        self.guide_files = chosen;
                    }
                }
            }
            None => {}
        }
    }
}
