// SPDX-License-Identifier: GPL-3.0-or-later

//! The Project, Symbols and Problems panels (P2-19): the open project's files as a tree, the
//! open file's or the whole project's symbols, and every problem. Choosing a row asks for its
//! file to be opened at that place; the shell runs `file.open` for it, so the panels open a
//! file the way the palette and the agent do.
//!
//! What the panels list is `qs-workbench`'s, worked out when the project opened; a frame only
//! reads it. Every string comes from [`crate::strings`].

use std::path::{Path, PathBuf};

use eframe::egui::{Button, CollapsingHeader, RichText, ScrollArea, Ui};
use qs_workbench::{EntryKind, OpenFile, Problem, Severity, Symbol, SymbolKind, TreeEntry};

use crate::icons::Icon;
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// A file to open at a byte offset: what choosing a row asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Jump {
    /// The file.
    pub(crate) path: PathBuf,
    /// Where the caret goes.
    pub(crate) offset: usize,
}

/// Which symbols the Symbols section lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SymbolScope {
    /// The open file's.
    #[default]
    File,
    /// The whole project's.
    Project,
}

/// The Project section's tree: each directory a collapsible header, each file a row that opens
/// it. `open` is the open file, drawn selected.
pub(crate) fn tree_ui(ui: &mut Ui, entries: &[TreeEntry], open: Option<&Path>) -> Option<Jump> {
    let mut jump = None;
    level(ui, entries, open, &mut jump);
    jump
}

/// Draws `entries`, whose first is at the shallowest depth among them, recursing into each
/// directory's entries.
fn level(ui: &mut Ui, entries: &[TreeEntry], open: Option<&Path>, jump: &mut Option<Jump>) {
    let mut rest = entries;
    while let Some((entry, after)) = rest.split_first() {
        let children = after
            .iter()
            .take_while(|child| child.depth > entry.depth)
            .count();
        let (inside, next) = after.split_at(children);
        match &entry.kind {
            EntryKind::Directory => {
                CollapsingHeader::new(entry.name.as_str())
                    .id_salt((entry.depth, entry.name.as_str(), inside.len()))
                    .default_open(true)
                    .show(ui, |ui| level(ui, inside, open, jump));
            }
            EntryKind::File(path) => {
                let selected = open == Some(path.as_path());
                if ui
                    .add(
                        Button::selectable(selected, entry.name.as_str())
                            .frame_when_inactive(false),
                    )
                    .clicked()
                {
                    *jump = Some(Jump {
                        path: path.clone(),
                        offset: 0,
                    });
                }
            }
        }
        rest = next;
    }
}

/// The Symbols section: the scope's switch and its symbols, each a row that opens its
/// definition.
pub(crate) fn symbols_ui(
    ui: &mut Ui,
    theme: &Theme,
    scope: &mut SymbolScope,
    open: Option<&OpenFile>,
    project: &[Symbol],
    max_height: f32,
) -> Option<Jump> {
    ui.horizontal(|ui| {
        for (value, text) in [
            (SymbolScope::File, Text::SymbolsFile),
            (SymbolScope::Project, Text::SymbolsAll),
        ] {
            ui.selectable_value(scope, value, text.get());
        }
    });
    let symbols = match scope {
        SymbolScope::File => open.map_or(&[][..], OpenFile::outline),
        SymbolScope::Project => project,
    };
    if symbols.is_empty() {
        ui.label(RichText::new(Text::NoSymbols.get()).color(theme.color(Token::TextMuted)));
        return None;
    }
    let mut jump = None;
    let row_height = ui.spacing().interact_size.y;
    ScrollArea::vertical()
        .id_salt(*scope)
        .max_height(max_height)
        .auto_shrink([false, true])
        .show_rows(ui, row_height, symbols.len(), |ui, rows| {
            for symbol in symbols.get(rows).unwrap_or_default() {
                let label = RichText::new(symbol.name.as_str()).monospace();
                let response = ui
                    .horizontal(|ui| {
                        let row = ui.add(Button::new(label).frame(false));
                        ui.label(
                            RichText::new(kind(symbol.kind).get())
                                .small()
                                .color(theme.color(Token::TextFaint)),
                        );
                        row
                    })
                    .inner;
                if response.clicked() {
                    jump = Some(Jump {
                        path: symbol.range.path.clone(),
                        offset: symbol.range.span.start,
                    });
                }
            }
        });
    jump
}

/// The word for a symbol's kind.
const fn kind(kind: SymbolKind) -> Text {
    match kind {
        SymbolKind::Function => Text::SymbolFunction,
        SymbolKind::Field => Text::SymbolField,
        SymbolKind::Constant => Text::SymbolConstant,
        SymbolKind::Global => Text::SymbolGlobal,
    }
}

/// The Problems tab: every problem, each a row that opens its file at its span. `root` is the
/// project directory, which the rows' paths are shown under.
pub(crate) fn problems_ui(
    ui: &mut Ui,
    theme: &Theme,
    problems: &[Problem],
    root: Option<&Path>,
) -> Option<Jump> {
    if problems.is_empty() {
        ui.label(RichText::new(Text::NoProblems.get()).color(theme.color(Token::TextMuted)));
        return None;
    }
    let mut jump = None;
    let row_height = ui.spacing().interact_size.y;
    ScrollArea::vertical()
        .id_salt(Text::Problems)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, problems.len(), |ui, rows| {
            for problem in problems.get(rows).unwrap_or_default() {
                if ui
                    .add(Button::new(problem_label(theme, problem, root)).frame(false))
                    .clicked()
                {
                    jump = Some(Jump {
                        path: problem.path.clone(),
                        offset: problem.diagnostic.span.start,
                    });
                }
            }
        });
    jump
}

/// A problem's row: its severity's icon, its code and message, then where it is, as
/// `file:line:column` under `root`.
pub fn problem_text(problem: &Problem, root: Option<&Path>) -> String {
    let icon = match problem.diagnostic.severity() {
        Severity::Error => Icon::Error,
        Severity::Warning => Icon::Warning,
    };
    let shown = root
        .and_then(|root| problem.path.strip_prefix(root).ok())
        .unwrap_or(&problem.path);
    format!(
        "{} {} {}  {}:{}:{}",
        icon.char(),
        problem.diagnostic.code.as_str(),
        problem.diagnostic.message,
        shown.display(),
        problem.location.line(),
        problem.location.column()
    )
}

/// [`problem_text`] in the severity's colour.
fn problem_label(theme: &Theme, problem: &Problem, root: Option<&Path>) -> RichText {
    let color = match problem.diagnostic.severity() {
        Severity::Error => theme.color(Token::StatusError),
        Severity::Warning => theme.color(Token::StatusWarn),
    };
    RichText::new(problem_text(problem, root)).color(color)
}

/// How many of `problems` are errors and how many warnings.
pub(crate) fn counts(problems: &[Problem]) -> (usize, usize) {
    let errors = problems
        .iter()
        .filter(|p| p.diagnostic.severity() == Severity::Error)
        .count();
    (errors, problems.len().saturating_sub(errors))
}

/// The project directory's name, the tree's heading.
pub(crate) fn root_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests;
