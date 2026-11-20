// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent's feature (P4-09, P6-41): the agent panel, an inline edit's pending answer, and
//! the glue between the panel and its commands, which the shell used to hold.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;
use qs_agent::history;
use qs_commands::Session;
use qs_settings::{Scope as SettingsScope, Setting, SettingsStore};
use qs_workbench::checkpoints::{
    CREATE_CHECKPOINT, CheckpointMade, CreateCheckpointParams, RESTORE_CHECKPOINT,
    RestoreCheckpointParams,
};
use qs_workbench::engine::{ENGINE_SCREENSHOT, EngineScreenshotParams, Screenshot};
use qs_workbench::staged::HunksParams;
use qs_workbench::{ACCEPT_HUNKS, REJECT_HUNKS};

use super::{AgentAction, AgentPanel};
use crate::features::{Commands, ShellRequest};
use crate::project::Jump;
use crate::shell::Workspace;
use crate::strings::Text;

/// An inline edit's answer while it is coming: the edited text, or why there is none.
type InlineAnswer = mpsc::Receiver<Result<String, String>>;

/// The agent panel and an inline edit waiting for its answer.
pub(crate) struct AgentFeature {
    /// The panel and its threads.
    pub(crate) panel: AgentPanel,
    /// The answer of the inline edit asked for, until it comes (P4-10).
    inline: Option<InlineAnswer>,
}

impl AgentFeature {
    /// The feature over `panel`, with no inline edit asked for.
    pub(crate) const fn new(panel: AgentPanel) -> Self {
        Self {
            panel,
            inline: None,
        }
    }

    /// Starts a thread on `task` over the open project, each `@` mention it names attached
    /// (UI-8), its calls decided by the settings in `settings_file` and the project's; why it
    /// could not.
    ///
    /// # Errors
    ///
    /// No project open, no provider set up, no model or no checkpoint.
    pub(crate) fn start(
        &mut self,
        session: &mut Session<Workspace>,
        task: &str,
        settings_file: Option<&Path>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let root = session.context().agent_root()?;
        let live = session.context().live();
        let task = qs_agent::expand_mentions(session, &root, task, &live);
        self.start_thread(session, &root, &task, settings_file, ctx)
    }

    /// Reads the external agents from the user's settings in `settings_file` (`agent.external`,
    /// P4-14), for the panel to offer.
    pub(crate) fn load_external(&mut self, settings_file: &Path) {
        let agents = SettingsStore::open(settings_file.to_path_buf(), None)
            .map_err(|error| tracing::warn!(%error, "the external agents were not read"))
            .ok()
            .and_then(|store| serde_json::from_value(store.value(Setting::AgentExternal).0).ok())
            .unwrap_or_default();
        self.panel.set_external_agents(agents);
    }

    /// Adds Claude Code to the external agents, kept in the user's settings when there is a
    /// file for them, and has new threads run on it.
    fn use_claude_code(&mut self, settings_file: Option<&Path>) {
        let claude = super::external::claude_code();
        let mut agents = self.panel.external_agents().to_vec();
        if !agents.iter().any(|agent| agent.name == claude.name) {
            agents.push(claude);
        }
        if let Some(file) = settings_file {
            let value = serde_json::to_value(&agents).ok();
            let kept = SettingsStore::open(file.to_path_buf(), None).and_then(|mut store| {
                store.put(SettingsScope::User, Setting::AgentExternal, value)
            });
            if let Err(error) = kept {
                tracing::warn!(%error, "the external agents were not kept");
            }
        }
        self.panel.set_external_agents(agents);
    }

    /// Starts a thread about `entity`, its state attached (Ask about this, UI-9); why it could
    /// not. Its field values are attached as they are, never read as mentions.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub(crate) fn ask_about(
        &mut self,
        session: &mut Session<Workspace>,
        entity: &qs_agent::EntityState,
        settings_file: Option<&Path>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let root = session.context().agent_root()?;
        self.start_thread(session, &root, &entity.ask_about(), settings_file, ctx)
    }

    /// Starts a thread on `task` exactly as given, with nothing in it read as a mention: the
    /// editor's question about selected code (P4-14); why it could not.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub(crate) fn ask(
        &mut self,
        session: &mut Session<Workspace>,
        task: &str,
        settings_file: Option<&Path>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let root = session.context().agent_root()?;
        self.start_thread(session, &root, task, settings_file, ctx)
    }

    /// Starts a thread on `task` as given, over the project in `root`, its calls decided by the
    /// profile and tool rules the settings hold now (P4-30).
    fn start_thread(
        &mut self,
        session: &mut Session<Workspace>,
        root: &Path,
        task: &str,
        settings_file: Option<&Path>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let ctx = ctx.clone();
        // An external agent decides its own calls and asks through the panel; the IDE's tools
        // reach it over MCP.
        if self.panel.external_agent().is_some() {
            let servers = super::external::ide_tools(root);
            // The project as it is before the agent touches it, to restore (AGT-3).
            let label = CreateCheckpointParams {
                label: Some(task.to_owned()),
            };
            let checkpoint = serde_json::to_value(label)
                .ok()
                .and_then(|label| {
                    session
                        .execute(CREATE_CHECKPOINT, label)
                        .map_err(|error| tracing::warn!(%error, "no checkpoint before the agent"))
                        .ok()
                })
                .and_then(|made| serde_json::from_value::<CheckpointMade>(made).ok())
                .map(|made| made.number);
            return self
                .panel
                .start_external(root, task, (servers, checkpoint), move || {
                    ctx.request_repaint();
                });
        }
        if let Some(permissions) = AgentSettings::of(settings_file, root).permissions() {
            self.panel.set_permissions(permissions);
        }
        self.panel
            .start(session, root, task, move || ctx.request_repaint())
    }

    /// Asks the model for an inline edit of `selection` in `file`, the text of the editor's
    /// file at `path` (P4-10); its answer comes to [`Self::take_inline`].
    ///
    /// # Errors
    ///
    /// No provider set up, or no model.
    pub(crate) fn start_inline(
        &mut self,
        (path, file): (&Path, &str),
        selection: &str,
        instruction: &str,
        ctx: Option<egui::Context>,
    ) -> Result<(), String> {
        let prompt = qs_agent::inline_prompt(path, file, selection, instruction);
        let answered = self.panel.ask_once(prompt, move || {
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        })?;
        self.inline = Some(answered);
        Ok(())
    }

    /// The inline edit's answer, once it has come.
    pub(crate) fn take_inline(&mut self) -> Option<Result<String, String>> {
        let answer = self.inline.as_ref()?.try_recv().ok()?;
        self.inline = None;
        Some(answer)
    }

    /// Waits for the inline edit's answer. For tests.
    #[cfg(test)]
    pub(crate) fn wait_inline(&mut self) -> Option<Result<String, String>> {
        let answer = self.inline.as_ref()?.recv().ok()?;
        self.inline = None;
        Some(answer)
    }

    /// Does what the panel asked for, keeping the permissions it changes in the settings.
    pub(crate) fn apply(
        &mut self,
        action: AgentAction,
        commands: &mut Commands<'_>,
        settings_file: Option<&Path>,
        ctx: &egui::Context,
    ) {
        // The project's settings as they are when read: after a call that may open another.
        let settings = |commands: &Commands<'_>| match commands.session().context().agent_root() {
            Ok(root) => AgentSettings::of(settings_file, &root),
            Err(_) => AgentSettings::none(),
        };
        match action {
            AgentAction::Start(task) => {
                let started = self.start(commands.session_mut(), &task, settings_file, ctx);
                self.panel.set_error(started.err());
            }
            AgentAction::SaveSetup => self.panel.save_setup_to_keychain(),
            AgentAction::UseClaudeCode => self.use_claude_code(settings_file),
            AgentAction::Accept(hunks) => {
                let _accepted: Option<serde_json::Value> =
                    commands.run(ACCEPT_HUNKS, &HunksParams { hunks });
            }
            AgentAction::Reject(hunks) => {
                let _rejected: Option<serde_json::Value> =
                    commands.run(REJECT_HUNKS, &HunksParams { hunks });
            }
            AgentAction::Permit(index, permit) => {
                if let Some(kept) = self.panel.permit(commands.session_mut(), index, permit) {
                    settings(commands).keep(&kept);
                }
            }
            AgentAction::Profile(profile) => {
                let mut permissions = self.panel.permissions().clone();
                permissions.profile = profile;
                settings(commands).keep(&permissions);
                self.panel.set_permissions(permissions);
            }
            AgentAction::Restore(number) => {
                let _restored: Option<serde_json::Value> =
                    commands.run(RESTORE_CHECKPOINT, &RestoreCheckpointParams { number });
            }
            AgentAction::OpenHistory => {
                let threads = commands
                    .session()
                    .context()
                    .agent_root()
                    .ok()
                    .and_then(|root| history::load(&history::threads_dir(&root)).ok())
                    .unwrap_or_default();
                self.panel.show_history(threads);
            }
            AgentAction::AttachScreenshot => {
                let shot: Option<Screenshot> = commands.run(
                    ENGINE_SCREENSHOT,
                    &EngineScreenshotParams {
                        path: None,
                        size: None,
                    },
                );
                match shot.map(|shot| std::fs::read(&shot.path)) {
                    Some(Ok(bytes)) => self.panel.attach_png(&bytes),
                    Some(Err(error)) => self.panel.set_error(Some(error.to_string())),
                    None => self
                        .panel
                        .set_error(Some(Text::GameStopped.get().to_owned())),
                }
            }
            AgentAction::RunAgain(index, at, text) => {
                let ctx = ctx.clone();
                let ran =
                    self.panel
                        .run_again(commands.session_mut(), (index, at, text), move || {
                            ctx.request_repaint()
                        });
                self.panel.set_error(ran.err());
            }
        }
    }

    /// After the threads took their models' events (P4-31): asks for the last file a call
    /// named while the editor follows the agent, and a thread that finished asks for the
    /// modder's attention, as the system shows it (the dock or the taskbar).
    pub(crate) fn after(
        &mut self,
        workspace: &Workspace,
        ctx: &egui::Context,
    ) -> Option<ShellRequest> {
        let followed = self.panel.take_followed().pop().and_then(|path| {
            let path = if path.is_absolute() {
                path
            } else {
                workspace
                    .agent_root()
                    .map(|root| root.join(&path))
                    .unwrap_or(path)
            };
            path.is_file()
                .then_some(ShellRequest::Jump(Jump { path, offset: 0 }))
        });
        if self.panel.take_finished() > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        followed
    }
}

impl Workspace {
    /// The open project's folder, which an agent thread works in.
    pub(crate) fn agent_root(&self) -> Result<PathBuf, String> {
        self.workbench
            .root()
            .map(Path::to_path_buf)
            .ok_or_else(|| Text::NoProject.get().to_owned())
    }

    /// What the running game shows now, for `@entity` and `@error` (PRIN-2): the selected
    /// edict with its fields as the Properties panel writes them, and the console's last error.
    pub(crate) fn live(&self) -> qs_agent::Live {
        let entities = self.preview.entities();
        let entity = entities.selected().map(|selected| qs_agent::EntityState {
            edict: selected.edict.into(),
            classname: selected.classname.clone(),
            fields: selected
                .fields
                .iter()
                .map(|field| (field.name.clone(), entities.show(&field.value)))
                .collect(),
        });
        let error = self
            .preview
            .console()
            .filter(|line| line.is_error())
            .last()
            .map(|line| line.text.clone());
        qs_agent::Live { entity, error }
    }
}

/// The user's settings and a project's, where the agent's profile and tool rules are kept
/// (P4-30).
struct AgentSettings(Option<SettingsStore>);

impl AgentSettings {
    /// The settings in `settings_file` and those of the project in `root`; none when the
    /// window keeps no settings file or they do not read.
    fn of(settings_file: Option<&Path>, root: &Path) -> Self {
        Self(settings_file.and_then(|user| {
            SettingsStore::open(
                user.to_path_buf(),
                Some(qs_settings::project_settings_path(root)),
            )
            .map_err(|error| tracing::warn!(%error, "settings not read"))
            .ok()
        }))
    }

    /// No settings: no project is open.
    const fn none() -> Self {
        Self(None)
    }

    /// The agent's profile and tool rules as the settings set them.
    fn permissions(&self) -> Option<qs_agent::Permissions> {
        let store = self.0.as_ref()?;
        let value = |setting| store.value(setting).0;
        Some(qs_agent::Permissions {
            profile: serde_json::from_value(value(Setting::AgentProfile)).unwrap_or_default(),
            rules: serde_json::from_value(value(Setting::ToolPermissions)).unwrap_or_default(),
        })
    }

    /// Keeps `permissions` in the project's settings, the profile and the rules.
    fn keep(self, permissions: &qs_agent::Permissions) {
        let Some(mut store) = self.0 else {
            return;
        };
        for (setting, value) in [
            (
                Setting::AgentProfile,
                serde_json::to_value(permissions.profile),
            ),
            (
                Setting::ToolPermissions,
                serde_json::to_value(&permissions.rules),
            ),
        ] {
            let kept = value.map_err(|error| error.to_string()).and_then(|value| {
                store
                    .put(SettingsScope::Project, setting, Some(value))
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = kept {
                tracing::warn!(%error, "the agent's permissions were not kept");
            }
        }
    }
}
