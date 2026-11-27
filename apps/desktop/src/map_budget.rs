// SPDX-License-Identifier: GPL-3.0-or-later

//! The map's engine limits in the Properties panel, below the compile controls (L-79, MAP-38):
//! what `map.budget` last found, a meter for each limit of the target engine, the worst first,
//! the meters past the limit in the error colour and the near ones in the warning colour. The
//! budget is made after each compile and dropped when the map changes.

use eframe::egui::{ProgressBar, RichText, Ui};
use qs_map_edit::MapRevision;
use qs_workbench::map_budget::{BudgetLine, LineBound, MapBudget, Standing, Used};

use crate::strings::{Text, budget_meter, budget_unbounded, budget_unknown, limit_unknown};
use crate::theme::{Theme, Token};

/// The meters shown before the rest are folded away: the worst of the target's.
const SHOWN: usize = 6;
/// The width of a meter's bar, in points.
const METER_WIDTH: f32 = 80.0;

/// What `map.budget` last found, and the map's edits it was found at.
#[derive(Debug, Default)]
pub(crate) struct BudgetView {
    found: Option<(MapBudget, MapRevision)>,
}

impl BudgetView {
    /// Takes what `map.budget` found in the map at `at`.
    pub(crate) fn set(&mut self, found: MapBudget, at: MapRevision) {
        self.found = Some((found, at));
    }

    /// Forgets what was found when the map is no longer at the edits it was found at.
    pub(crate) fn follow(&mut self, edits: MapRevision) {
        if self.found.as_ref().is_some_and(|(_, at)| *at != edits) {
            self.found = None;
        }
    }

    /// Whether what is shown is `found`, found at `at`.
    pub(crate) fn shows(&self, found: &MapBudget, at: MapRevision) -> bool {
        self.found
            .as_ref()
            .is_some_and(|(shown, shown_at)| shown == found && *shown_at == at)
    }

    /// Shows nothing.
    pub(crate) fn clear(&mut self) {
        self.found = None;
    }

    /// What was found, for tests.
    #[cfg(test)]
    pub(crate) fn found(&self) -> Option<&MapBudget> {
        self.found.as_ref().map(|(found, _)| found)
    }

    /// The meters of the target engine, the worst [`SHOWN`] first and the rest folded.
    pub(crate) fn properties(&self, ui: &mut Ui, theme: &Theme) {
        let Some(target) = self
            .found
            .as_ref()
            .and_then(|(found, _)| found.engines.first())
        else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(Text::MapBudgetHeading.get()).strong());
            ui.label(RichText::new(target.name.as_str()).color(theme.color(Token::TextMuted)));
        });
        let (worst, rest) = target.lines.split_at(SHOWN.min(target.lines.len()));
        for line in worst {
            meter(ui, theme, line, &target.name);
        }
        if !rest.is_empty() {
            ui.collapsing(Text::MapBudgetRest.get(), |ui| {
                for line in rest {
                    meter(ui, theme, line, &target.name);
                }
            });
        }
        ui.separator();
    }
}

/// One limit's meter: its words and, with a bound, a bar of the share.
fn meter(ui: &mut Ui, theme: &Theme, line: &BudgetLine, engine: &str) {
    let token = match line.standing {
        Standing::Past => Token::StatusError,
        Standing::Near => Token::StatusWarn,
        Standing::Within => Token::StatusOk,
        Standing::Unknown => Token::TextMuted,
    };
    let colour = theme.color(token);
    match (&line.used, &line.bound) {
        (Used::Count { count, .. } | Used::AtLeast { count, .. }, LineBound::At { most, .. }) => {
            ui.horizontal_wrapped(|ui| {
                let share = line.share.unwrap_or(0.0).clamp(0.0, 1.0);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a share of 0 to 1 drawn as a bar"
                )]
                let bar = ProgressBar::new(share as f32)
                    .desired_width(METER_WIDTH)
                    .corner_radius(0.0)
                    .fill(colour);
                ui.add(bar);
                let text = match line.standing {
                    Standing::Past | Standing::Near => colour,
                    Standing::Within | Standing::Unknown => theme.color(Token::Text),
                };
                ui.label(RichText::new(budget_meter(*count, *most, &line.name)).color(text));
            });
        }
        (Used::Count { count, .. } | Used::AtLeast { count, .. }, LineBound::Unlimited) => {
            ui.label(RichText::new(budget_unbounded(*count, &line.name, engine)).color(colour));
        }
        (Used::Count { .. } | Used::AtLeast { .. }, LineBound::Unknown) => {
            ui.label(
                RichText::new(budget_unknown(&line.name, &limit_unknown(engine))).color(colour),
            );
        }
        (Used::NotMeasured { why }, _) => {
            ui.label(RichText::new(budget_unknown(&line.name, why)).color(colour));
        }
    }
}
