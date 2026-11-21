// SPDX-License-Identifier: GPL-3.0-or-later

//! Next-edit suggestions (P4-11, UI-7): what an edit in the editor implies elsewhere, from
//! the semantic model. A name typed over where it is first declared, a field's, a
//! function's or a global's, implies the same change at every other declaration and use of
//! it; each of those is a suggestion, taken one at a time.
//!
//! The model followed is the refactoring an editor offers when a declaration's name is
//! edited in place (JetBrains' "rename usages" hint, Zed's and VS Code's Tab through the
//! next edit): nothing is changed until it is accepted, and each acceptance is one change.

use std::path::{Path, PathBuf};

use qc_ide::{FileRange, Symbol};
use qc_project::Loader;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};

use crate::editing::{Change, Saved, Source, TextStep};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of the command that answers the next suggestion.
pub const NEXT_EDIT: &str = "edit.next";
/// The id of the command that makes the next suggestion's change.
pub const ACCEPT_NEXT_EDIT: &str = "edit.accept_next";
/// The id of the command that drops the suggestions.
pub const DISMISS_NEXT_EDIT: &str = "edit.dismiss_next";

/// One place a suggestion changes: the old name's bytes in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Site {
    path: PathBuf,
    start: usize,
    end: usize,
}

/// A name being typed over at its first declaration, and the places that still say the old
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    /// The file the name is typed in, and the name's bytes there as they are now.
    path: PathBuf,
    start: usize,
    end: usize,
    /// The name before it was typed over, and as it is now.
    old: String,
    new: String,
    /// The places left, in the order they are suggested.
    sites: Vec<Site>,
}

/// The suggestions an edit left.
#[derive(Debug, Default)]
pub(crate) struct NextEdits {
    pending: Option<Pending>,
}

/// The next suggestion, as `edit.next` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NextEdit {
    /// The name the places still say.
    pub old: String,
    /// The name they are given.
    pub new: String,
    /// The file the name was typed over in, and the byte after the name there.
    pub typed_path: PathBuf,
    /// The byte after the typed name.
    pub typed_end: usize,
    /// The file of the next place.
    pub path: PathBuf,
    /// The first byte of the old name there.
    pub start: usize,
    /// The byte after it.
    pub end: usize,
    /// How many places are left, this one counted.
    pub left: usize,
}

/// What accepting a suggestion did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    /// The file changed, with its text before and after.
    pub change: Change,
    /// The byte after the new name there.
    pub offset: usize,
    /// How many places are left.
    pub left: usize,
}

/// Whether `byte` may be part of a name.
const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `name` is one a declaration may have: letters, digits and underscores, the
/// first no digit.
fn is_name(name: &[u8]) -> bool {
    name.first().is_some_and(|first| !first.is_ascii_digit())
        && name.iter().all(|byte| is_name_byte(*byte))
}

/// The one run of bytes that differs between `before` and `after`: where it starts in
/// both, and where it ends in each.
fn changed(before: &[u8], after: &[u8]) -> (usize, usize, usize) {
    let start = before
        .iter()
        .zip(after)
        .take_while(|(old, new)| old == new)
        .count();
    let most = before.len().min(after.len()).saturating_sub(start);
    let same_end = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(most)
        .take_while(|(old, new)| old == new)
        .count();
    (
        start,
        before.len().saturating_sub(same_end),
        after.len().saturating_sub(same_end),
    )
}

/// The name of `text` that holds the bytes from `start` to `end`, all of them a name's:
/// its first byte and the byte after it. None when a byte between is no name's, or when
/// there is no name there.
fn name_around(text: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    if !text.get(start..end)?.iter().all(|byte| is_name_byte(*byte)) {
        return None;
    }
    let before = text.get(..start)?;
    let after = text.get(end..)?;
    let left = before
        .iter()
        .rev()
        .take_while(|byte| is_name_byte(**byte))
        .count();
    let right = after.iter().take_while(|byte| is_name_byte(**byte)).count();
    let (first, last) = (start.saturating_sub(left), end.saturating_add(right));
    (first < last).then_some((first, last))
}

/// `at`, a byte of a text, after a change that took the bytes up to `old_end` to
/// `new_end`: moved with what follows the change, or where it was before it.
const fn moved(at: usize, old_end: usize, new_end: usize) -> usize {
    if at >= old_end {
        at.saturating_sub(old_end).saturating_add(new_end)
    } else {
        at
    }
}

impl Pending {
    /// Whether the name as typed may be suggested: a name, and another than the old one.
    fn offered(&self) -> bool {
        is_name(self.new.as_bytes()) && self.new != self.old
    }

    /// Moves what follows a change of `path` with it.
    fn follow(&mut self, path: &Path, old_end: usize, new_end: usize) {
        for site in self.sites.iter_mut().filter(|site| site.path == path) {
            site.start = moved(site.start, old_end, new_end);
            site.end = moved(site.end, old_end, new_end);
        }
    }
}

impl NextEdits {
    /// Takes an edit of `path` in the editor, from `before` to `after`: typing on in the
    /// name being typed over keeps its suggestions, with the name as it is now; any other
    /// edit drops them, and one in the name `symbols` says is first declared there starts
    /// them, `references` giving every declaration and use of the name at a byte, or
    /// nothing for a name that cannot be renamed.
    fn typed(
        &mut self,
        path: &Path,
        (before, after): (&[u8], &[u8]),
        symbols: &[Symbol],
        references: &mut dyn FnMut(usize) -> Option<Vec<FileRange>>,
    ) {
        let (start, old_end, new_end) = changed(before, after);
        if start == old_end && start == new_end {
            return;
        }
        if let Some(mut pending) = self.pending.take()
            && pending.path == path
            && start >= pending.start
            && old_end <= pending.end
        {
            pending.end = moved(pending.end, old_end, new_end);
            pending.follow(path, old_end, new_end);
            // A byte that is no name's ends the name: the suggestions go with it.
            let name = after.get(pending.start..pending.end);
            if let Some(name) = name.filter(|name| name.iter().all(|b| is_name_byte(*b))) {
                pending.new = String::from_utf8_lossy(name).into_owned();
                self.pending = Some(pending);
            }
            return;
        }
        self.pending = self.started(path, (before, after), symbols, references);
    }

    /// The suggestions an edit starts, when it is in the name a symbol is first declared by.
    fn started(
        &self,
        path: &Path,
        (before, after): (&[u8], &[u8]),
        symbols: &[Symbol],
        references: &mut dyn FnMut(usize) -> Option<Vec<FileRange>>,
    ) -> Option<Pending> {
        let (start, old_end, new_end) = changed(before, after);
        let (first, last) = name_around(before, start, old_end)?;
        let old = before.get(first..last)?;
        let declared = symbols.iter().any(|symbol| {
            symbol.range.path == path
                && symbol.range.span.start == first
                && symbol.name.as_bytes() == old
        });
        if !declared {
            return None;
        }
        let end = moved(last, old_end, new_end);
        let new = after.get(first..end)?;
        if !new.iter().all(|byte| is_name_byte(*byte)) {
            return None;
        }
        let mut sites: Vec<Site> = references(first)?
            .into_iter()
            .filter(|range| !(range.path == path && range.span.start == first))
            .map(|range| Site {
                path: range.path,
                start: range.span.start,
                end: range.span.end,
            })
            .collect();
        if sites.is_empty() {
            return None;
        }
        // The file being typed in first, top to bottom, then the others by path.
        sites.sort_by_cached_key(|site| (site.path != path, site.path.clone(), site.start));
        let mut pending = Pending {
            path: path.to_path_buf(),
            start: first,
            end,
            old: String::from_utf8_lossy(old).into_owned(),
            new: String::from_utf8_lossy(new).into_owned(),
            sites,
        };
        pending.follow(path, old_end, new_end);
        Some(pending)
    }
}

impl<L: Loader> Workbench<L> {
    /// Takes the editor's edit of `path`, whose text was `before` and is `after`, for the
    /// suggestions it implies (UI-7). The symbols are the last check's: an edit made before
    /// the check of the one before it is back starts none.
    pub(crate) fn note_typed(&mut self, path: &Path, before: &[u8], after: &[u8]) {
        let (next, host) = (&mut self.next_edits, &mut self.host);
        // A name `symbol.rename` refuses, the engine's own, is not suggested either.
        next.typed(path, (before, after), &self.symbols, &mut |offset| {
            host.prepare_rename(path, offset).ok()?;
            Some(host.references(path, offset, true))
        });
    }

    /// The next suggestion, when an edit left one.
    #[must_use]
    pub fn next_edit(&self) -> Option<NextEdit> {
        let pending = self.next_edits.pending.as_ref().filter(|p| p.offered())?;
        let site = pending.sites.first()?;
        Some(NextEdit {
            old: pending.old.clone(),
            new: pending.new.clone(),
            typed_path: pending.path.clone(),
            typed_end: pending.end,
            path: site.path.clone(),
            start: site.start,
            end: site.end,
            left: pending.sites.len(),
        })
    }

    /// Drops the suggestions.
    pub fn dismiss_next_edit(&mut self) {
        self.next_edits.pending = None;
    }

    /// Makes the next suggestion's change in its file's text, in the editor's text of it and
    /// unsaved, and leaves the rest. A place that no longer says the old name, its file
    /// edited since, is passed over. `None` when no suggestion is left.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Project`] when the file is a project file whose new text does not
    /// open.
    pub fn accept_next_edit(&mut self) -> Result<Option<Accepted>, WorkbenchError> {
        loop {
            let Some(pending) = self.next_edits.pending.as_mut().filter(|p| p.offered()) else {
                return Ok(None);
            };
            if pending.sites.is_empty() {
                self.next_edits.pending = None;
                return Ok(None);
            }
            let site = pending.sites.remove(0);
            let (old, new) = (pending.old.clone(), pending.new.clone());
            let Some(before) = self.host.text(&site.path) else {
                continue;
            };
            if before.get(site.start..site.end) != Some(old.as_bytes()) {
                continue;
            }
            let mut after = before.clone();
            after.splice(site.start..site.end, new.bytes());
            let new_end = site.start.saturating_add(new.len());
            self.set_text(&site.path, after.clone(), Source::Elsewhere)?;
            // The name typed over is where it is first declared, so no place is before it in
            // its own file, and it stays where it is.
            let left = self.next_edits.pending.as_mut().map_or(0, |pending| {
                pending.follow(&site.path, site.end, new_end);
                pending.sites.len()
            });
            if left == 0 {
                self.next_edits.pending = None;
            }
            return Ok(Some(Accepted {
                change: Change {
                    path: site.path,
                    before,
                    after,
                },
                offset: new_end,
                left,
            }));
        }
    }
}

/// What `edit.accept_next` answers: where the change was made and how many are left.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NextEditDone {
    /// The file changed.
    pub path: PathBuf,
    /// The byte after the new name there.
    pub offset: usize,
    /// How many suggestions are left.
    pub left: usize,
}

/// `edit.next`: the next suggestion.
struct Next;

impl<C: HasWorkbench + 'static> Command<C> for Next {
    const ID: &'static str = NEXT_EDIT;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Answers the next edit the last one implies: after a field's, \
        a function's or a global's name is typed over where it is first declared, the next \
        place that still says the old name, and how many are left.";
    type Params = NoParams;
    type Output = Option<NextEdit>;
    type Error = !;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<Option<NextEdit>, C>, Self::Error> {
        Ok(Outcome::unchanged(context.workbench().next_edit()))
    }
}

/// `edit.accept_next`: makes the next suggestion's change.
struct AcceptNext;

impl<C: HasWorkbench + 'static> Command<C> for AcceptNext {
    const ID: &'static str = ACCEPT_NEXT_EDIT;
    const SUMMARY: &'static str = "Makes the next suggested edit, giving the next place that \
        says the old name the new one, in the editor's text of its file; nothing is saved. \
        Tab in the code editor runs it.";
    type Params = NoParams;
    type Output = Option<NextEditDone>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<Option<NextEditDone>, C>, WorkbenchError> {
        let Some(accepted) = context.workbench().accept_next_edit()? else {
            return Ok(Outcome::unchanged(None));
        };
        let done = NextEditDone {
            path: accepted.change.path.clone(),
            offset: accepted.offset,
            left: accepted.left,
        };
        Ok(Outcome::undoable(
            Some(done),
            TextStep::new(vec![accepted.change], Saved::Unsaved),
        ))
    }
}

/// `edit.dismiss_next`: drops the suggestions.
struct DismissNext;

impl<C: HasWorkbench + 'static> Command<C> for DismissNext {
    const ID: &'static str = DISMISS_NEXT_EDIT;
    const SUMMARY: &'static str = "Drops the suggested edits. Escape in the code editor runs it.";
    type Params = NoParams;
    type Output = ();
    type Error = !;

    fn run(&self, context: &mut C, NoParams {}: NoParams) -> Result<Outcome<(), C>, Self::Error> {
        context.workbench().dismiss_next_edit();
        Ok(Outcome::unchanged(()))
    }
}

/// Registers the next-edit commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub(crate) fn register_next_edit<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(Next)?;
    registry.register(AcceptNext)?;
    registry.register(DismissNext)
}

#[cfg(test)]
mod tests;
