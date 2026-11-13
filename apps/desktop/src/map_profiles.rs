// SPDX-License-Identifier: GPL-3.0-or-later

//! Compile profiles, portals and backups in the map view (P6-20, MAP-28): the profile chosen
//! from `map.profiles` and run by a button or Cmd+B, the notice of how the last run ended with
//! its time, the last compile's portals drawn in every pane, and the newest backup restored.

use eframe::egui::{self, Painter, RichText, Stroke, Ui};
use qs_workbench::map_profile::{ProfileRun, TaskResult};

use crate::keymap::RUN_PROFILE;
use crate::map_editor::MapAction;
use crate::map_panes::Projection;
use crate::strings::{Text, profile_done, profile_failed};
use crate::theme::{Theme, Token};

/// What the compile controls ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileAction {
    /// Choose the compile profile Compile runs.
    Choose(String),
    /// Run the chosen compile profile.
    Run,
    /// Draw the last compile's portals.
    ShowPortals,
    /// Stop drawing them.
    HidePortals,
    /// Put the map's newest backup in the editor.
    RestoreBackup,
}

/// The width of a portal's outline, in points.
const PORTAL_WIDTH: f32 = 1.0;

/// The profiles to choose from, the last run's notice and the portals shown.
#[derive(Debug, Default)]
pub(crate) struct ProfileView {
    /// The profiles' names, as `map.profiles` lists them.
    names: Vec<String>,
    /// The one chosen.
    chosen: String,
    /// How the last run ended.
    last: Option<ProfileRun>,
    /// The last compile's portals, while they are shown.
    portals: Option<Vec<Vec<[f32; 3]>>>,
}

impl ProfileView {
    /// Takes the profiles' names and the one chosen.
    pub(crate) fn set_profiles(&mut self, names: Vec<String>, chosen: String) {
        self.names = names;
        self.chosen = chosen;
    }

    /// Takes the profile chosen.
    pub(crate) fn set_chosen(&mut self, chosen: String) {
        self.chosen = chosen;
    }

    /// Forgets the last run, as another map opening does.
    pub(crate) fn clear_run(&mut self) {
        self.last = None;
    }

    /// Takes how a run ended, for the notice.
    pub(crate) fn set_run(&mut self, run: ProfileRun) {
        self.last = Some(run);
    }

    /// Takes the portals to draw, or none to stop drawing them.
    pub(crate) fn set_portals(&mut self, portals: Option<Vec<Vec<[f32; 3]>>>) {
        self.portals = portals;
    }

    /// Whether the portals are drawn.
    pub(crate) const fn shows_portals(&self) -> bool {
        self.portals.is_some()
    }

    /// The profile shown as chosen, for tests.
    #[cfg(test)]
    pub(crate) fn chosen(&self) -> &str {
        &self.chosen
    }

    /// What the notice says of the last run, for tests.
    #[cfg(test)]
    pub(crate) fn notice(&self) -> Option<String> {
        self.last.as_ref().map(|run| notice(run).0)
    }

    /// The controls in the Properties panel, and the notice of the last run; what they asked
    /// for.
    pub(crate) fn controls(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        ui.horizontal_wrapped(|ui| {
            let mut chosen = self.chosen.clone();
            egui::ComboBox::from_id_salt(Text::MapProfilePicker.get())
                .selected_text(chosen.as_str())
                .show_ui(ui, |ui| {
                    for name in &self.names {
                        ui.selectable_value(&mut chosen, name.clone(), name.as_str());
                    }
                })
                .response
                .on_hover_text(Text::MapProfilePicker.get());
            if chosen != self.chosen {
                actions.push(MapAction::Profile(ProfileAction::Choose(chosen)));
            }
            let shortcut = ui.ctx().format_shortcut(&RUN_PROFILE);
            if ui
                .button(Text::MapCompileProfile.get())
                .on_hover_text(shortcut)
                .clicked()
            {
                actions.push(MapAction::Profile(ProfileAction::Run));
            }
            let mut shown = self.portals.is_some();
            if ui
                .checkbox(&mut shown, Text::MapShowPortals.get())
                .changed()
            {
                actions.push(MapAction::Profile(if shown {
                    ProfileAction::ShowPortals
                } else {
                    ProfileAction::HidePortals
                }));
            }
            if ui.button(Text::MapRestoreBackup.get()).clicked() {
                actions.push(MapAction::Profile(ProfileAction::RestoreBackup));
            }
        });
        if let Some(run) = &self.last {
            let (text, done) = notice(run);
            let colour = if done {
                theme.color(Token::StatusOk)
            } else {
                theme.color(Token::StatusError)
            };
            ui.label(RichText::new(text).color(colour));
        }
        actions
    }

    /// Draws the portals, when shown, through `projection`.
    pub(crate) fn draw(&self, painter: &Painter, projection: &Projection, theme: &Theme) {
        let Some(portals) = &self.portals else {
            return;
        };
        let stroke = Stroke::new(PORTAL_WIDTH, theme.color(Token::StatusWarn));
        for portal in portals {
            let next = portal.iter().cycle().skip(1);
            for (&from, &to) in portal.iter().zip(next) {
                projection.line(painter, [from, to], stroke);
            }
        }
    }
}

/// What the notice says of `run`, and whether every task was done.
fn notice(run: &ProfileRun) -> (String, bool) {
    let failed = run.tasks.iter().find_map(|task| match &task.result {
        TaskResult::Failed { message } => Some((task.task.name(), message)),
        TaskResult::Done => None,
    });
    match failed {
        None => (profile_done(&run.profile, run.seconds), true),
        Some((task, why)) => (profile_failed(&run.profile, task, run.seconds, why), false),
    }
}
