// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's features (P6-41): each owns its state and the glue between its panel and its
//! commands, so the shell names none of them. A feature's panel draws from the state the frame
//! began with and returns what was asked; the feature acts after, through [`Commands`], and asks
//! the window or another feature for anything else with a [`ShellRequest`], which the shell
//! carries out at once and in order.

use std::path::PathBuf;

use qs_commands::{CommandError, Session};

use crate::agent::{AgentFeature, AgentPanel};
use crate::debugger::DebuggerFeature;
use crate::editors::EditorsFeature;
use crate::git_panel::GitFeature;
use crate::map_feature::MapFeature;
use crate::project::{Jump, ProjectFeature};
use crate::shell::{BottomTab, ShellError, Workspace};
use crate::strings::Text;
use crate::terminal::TerminalFeature;

/// The features, each with its state.
pub(crate) struct Features {
    /// The agent panel and an inline edit's answer (P4-09, P4-10).
    pub(crate) agent: AgentFeature,
    /// The project: recent projects, a new project, export, settings and the game guide.
    pub(crate) project: ProjectFeature,
    /// The editors, the picker and the multibuffer.
    pub(crate) editors: EditorsFeature,
    /// The map view and editor, the outliner and the Properties panel.
    pub(crate) map: MapFeature,
    /// The Git tab (P4-35).
    pub(crate) git: GitFeature,
    /// The Terminal tab (P4-36).
    pub(crate) terminal: TerminalFeature,
    /// The Debugger tab and the console's box (P3-16).
    pub(crate) debugger: DebuggerFeature,
}

impl Features {
    /// The features, the agent's over `agent`.
    pub(crate) fn new(agent: AgentPanel) -> Self {
        Self {
            agent: AgentFeature::new(agent),
            project: ProjectFeature::default(),
            editors: EditorsFeature::default(),
            map: MapFeature::default(),
            git: GitFeature::default(),
            terminal: TerminalFeature::default(),
            debugger: DebuggerFeature::default(),
        }
    }
}

/// What a feature asks of the window or of another feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShellRequest {
    /// Open a file at a place, as a panel's row does.
    Jump(Jump),
    /// Open `path` in the editor with its caret at `line`, from 1.
    ShowLine {
        /// The file.
        path: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Open `file`, relative to the project's directory, at `line`, from 1.
    ShowProjectLine {
        /// The file, relative to the project's directory.
        file: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Bring the editor up to date with the open file, its change marks included, after an
    /// action that may have changed it on disk.
    RefreshEditor,
    /// Show the bottom panel's `tab`, opening the panel.
    ShowBottom(BottomTab),
}

/// The commands a feature runs, and the toolbar's notice it may set.
pub(crate) struct Commands<'a> {
    session: &'a mut Session<Workspace>,
    notice: &'a mut Option<String>,
}

impl<'a> Commands<'a> {
    /// The commands of `session`, and the toolbar's `notice`.
    pub(crate) const fn new(
        session: &'a mut Session<Workspace>,
        notice: &'a mut Option<String>,
    ) -> Self {
        Self { session, notice }
    }

    /// The session, to read the workspace.
    pub(crate) const fn session(&self) -> &Session<Workspace> {
        self.session
    }

    /// The session, for a feature that runs commands through its own model, as the agent does.
    pub(crate) const fn session_mut(&mut self) -> &mut Session<Workspace> {
        self.session
    }

    /// Runs the command `id` with `params`, logging a failure; its output, `None` when it
    /// failed.
    pub(crate) fn run<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Option<O> {
        match self.attempt(id, params) {
            Ok(output) => Some(output),
            Err(error) => {
                tracing::warn!(%error, command = id, "command failed");
                None
            }
        }
    }

    /// Runs the command `id` with `params`: its output, or why it failed.
    pub(crate) fn attempt<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Result<O, ShellError> {
        serde_json::to_value(params)
            .map_err(ShellError::from)
            .and_then(|params| Ok(self.session.execute(id, params)?))
            .and_then(|output| Ok(serde_json::from_value(output)?))
    }

    /// Runs the command `id` with `params` as JSON.
    pub(crate) fn execute(
        &mut self,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CommandError> {
        self.session.execute(id, params)
    }

    /// Clears the toolbar's notice.
    pub(crate) fn clear_notice(&mut self) {
        *self.notice = None;
    }

    /// Says on the toolbar that an edit failed.
    pub(crate) fn edit_failed(&mut self) {
        *self.notice = Some(Text::EditFailed.get().to_owned());
    }
}
