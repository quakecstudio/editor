// SPDX-License-Identifier: GPL-3.0-or-later

//! Next-edit suggestions (P4-11, UI-7): what an edit in the editor implies elsewhere, from
//! the semantic model. A name typed over where it is first declared, a field's, a
//! function's or a global's, implies the same change at every other declaration and use of
//! it; each of those is a suggestion, taken one at a time.
//!
//! The places are the rename's own (`qc-ide`'s `AnalysisHost::rename`), asked of the text
//! as it was before the name was typed over: a vector's components go with it, and a name
//! the rename refuses, the engine's, one that would change what the program means, or one
//! a macro writes, is suggested nowhere. Suggestions last only as long as the text they
//! were made from: any change that is not the editor's typing in the name, or the taking
//! of a suggestion, drops them.
//!
//! The model followed is the refactoring an editor offers when a declaration's name is
//! edited in place (JetBrains' "rename usages" hint, Zed's and VS Code's Tab through the
//! next edit): nothing is changed until it is accepted, and each acceptance is one change.

use std::path::{Path, PathBuf};

use qc_ide::{Rename, Symbol};
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

/// One place a suggestion changes: the bytes of a file that say the old name, or a
/// component of it, and what they are given.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Site {
    path: PathBuf,
    start: usize,
    end: usize,
    old: String,
    new: String,
}

/// What the rename says of the name as it is typed now.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Sites {
    /// It has not been asked since the name last changed.
    Unsettled,
    /// It refuses the name, or the name is the old one, or no name.
    Refused,
    /// The places left, in the order they are suggested.
    Offered(Vec<Site>),
}

/// A name being typed over at its first declaration, and the places that still say the old
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    /// The file the name is typed in, and the name's bytes there as they are now.
    path: PathBuf,
    start: usize,
    end: usize,
    /// The file's text before the name was typed over, which the rename is asked of.
    origin: Vec<u8>,
    /// The name before it was typed over, and as it is now.
    old: String,
    new: String,
    sites: Sites,
    /// How many suggestions have been taken: the name they were given is then the name.
    taken: usize,
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

    /// The places left.
    fn left(&self) -> &[Site] {
        match &self.sites {
            Sites::Offered(sites) => sites,
            Sites::Unsettled | Sites::Refused => &[],
        }
    }

    /// The places `rename`, made of the text before the name was typed over, changes
    /// beside the typed name itself, where they are now: the file being typed in first,
    /// top to bottom, then the others by path.
    fn sites_of(&self, rename: Rename) -> Vec<Site> {
        let origin_end = self.start.saturating_add(self.old.len());
        let (mut own, mut others) = (Vec::new(), Vec::new());
        for file in rename.edits {
            let typed_in = file.path == self.path;
            for (span, new) in file.edits {
                if typed_in && span.start == self.start {
                    continue;
                }
                // A component's place says the old name and its suffix (QC-11.7).
                let suffix = new.strip_prefix(self.new.as_str()).unwrap_or_default();
                let old = format!("{}{suffix}", self.old);
                let (start, end) = if typed_in {
                    (
                        moved(span.start, origin_end, self.end),
                        moved(span.end, origin_end, self.end),
                    )
                } else {
                    (span.start, span.end)
                };
                let site = Site {
                    path: file.path.clone(),
                    start,
                    end,
                    old,
                    new,
                };
                if typed_in {
                    own.push(site);
                } else {
                    others.push(site);
                }
            }
        }
        own.extend(others);
        own
    }

    /// Moves what follows a change of `path` with it.
    fn follow(&mut self, path: &Path, old_end: usize, new_end: usize) {
        let Sites::Offered(sites) = &mut self.sites else {
            return;
        };
        for site in sites.iter_mut().filter(|site| site.path == path) {
            site.start = moved(site.start, old_end, new_end);
            site.end = moved(site.end, old_end, new_end);
        }
    }
}

impl NextEdits {
    /// Takes an edit of `path` in the editor, from `before` to `after`: typing on in the
    /// name being typed over keeps it, with the name as it is now and its places to be
    /// asked again, unless a suggestion has been taken, which the name then belongs to;
    /// any other edit drops the suggestions, and one in the name `symbols` says is first
    /// declared there starts them.
    fn typed(&mut self, path: &Path, (before, after): (&[u8], &[u8]), symbols: &[Symbol]) {
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
            // A byte that is no name's ends the name: the suggestions go with it.
            let name = after.get(pending.start..pending.end);
            if let Some(name) = name.filter(|name| name.iter().all(|b| is_name_byte(*b)))
                && pending.taken == 0
            {
                pending.new = String::from_utf8_lossy(name).into_owned();
                pending.sites = Sites::Unsettled;
                self.pending = Some(pending);
            }
            return;
        }
        self.pending = Self::started(path, (before, after), symbols);
    }

    /// The suggestions an edit starts, when it is in the name a symbol is first declared by.
    fn started(
        path: &Path,
        (before, after): (&[u8], &[u8]),
        symbols: &[Symbol],
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
        Some(Pending {
            path: path.to_path_buf(),
            start: first,
            end,
            origin: before.to_vec(),
            old: String::from_utf8_lossy(old).into_owned(),
            new: String::from_utf8_lossy(new).into_owned(),
            sites: Sites::Unsettled,
            taken: 0,
        })
    }

    /// Drops the suggestions: the text they were made from has changed under them.
    pub(crate) fn forget(&mut self) {
        self.pending = None;
    }
}

impl<L: Loader> Workbench<L> {
    /// Takes the editor's edit of `path`, whose text was `before` and is `after`, for the
    /// suggestions it implies (UI-7). The symbols are the last check's: an edit made before
    /// the check of the one before it is back starts none.
    pub(crate) fn note_typed(&mut self, path: &Path, before: &[u8], after: &[u8]) {
        self.next_edits.typed(path, (before, after), &self.symbols);
    }

    /// Asks the rename for the places of the name as it is typed now, once for each name:
    /// when the editor's edit has been checked, and before a suggestion is answered or
    /// taken. The rename is asked of the file as it was before the name was typed over, the
    /// one text it can read the name's uses from; the file's text now is put back after.
    pub(crate) fn settle_next_edit(&mut self) {
        let Some(pending) = self.next_edits.pending.as_mut() else {
            return;
        };
        if pending.sites != Sites::Unsettled {
            return;
        }
        if !pending.offered() {
            pending.sites = Sites::Refused;
            return;
        }
        let Some(now) = self.host.text(&pending.path) else {
            self.next_edits.forget();
            return;
        };
        if let Err(error) = self.host.set_overlay(&pending.path, &pending.origin) {
            tracing::debug!(%error, "the text before a name was typed over");
            self.next_edits.forget();
            return;
        }
        let renamed = self.host.rename(&pending.path, pending.start, &pending.new);
        if let Err(error) = self.host.set_overlay(&pending.path, &now) {
            tracing::warn!(%error, "the text after a name was typed over was not put back");
            self.next_edits.forget();
            return;
        }
        match renamed.map(|rename| pending.sites_of(rename)) {
            Ok(sites) => pending.sites = Sites::Offered(sites),
            Err(error) => {
                tracing::debug!(%error, "no suggestions for the name as typed");
                pending.sites = Sites::Refused;
            }
        }
    }

    /// The next suggestion, when an edit left one the rename allows.
    #[must_use]
    pub fn next_edit(&self) -> Option<NextEdit> {
        let pending = self.next_edits.pending.as_ref()?;
        let left = pending.left();
        let site = left.first()?;
        Some(NextEdit {
            old: site.old.clone(),
            new: site.new.clone(),
            typed_path: pending.path.clone(),
            typed_end: pending.end,
            path: site.path.clone(),
            start: site.start,
            end: site.end,
            left: left.len(),
        })
    }

    /// Drops the suggestions.
    pub fn dismiss_next_edit(&mut self) {
        self.next_edits.forget();
    }

    /// Makes the next suggestion's change in its file's text, in the editor's text of it and
    /// unsaved, and leaves the rest. `None` when no suggestion is left.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Project`] when the file is a project file whose new text does not
    /// open.
    pub fn accept_next_edit(&mut self) -> Result<Option<Accepted>, WorkbenchError> {
        self.settle_next_edit();
        // The text set here is the suggestion's own, which drops no suggestion.
        let Some(mut pending) = self.next_edits.pending.take() else {
            return Ok(None);
        };
        let Sites::Offered(sites) = &mut pending.sites else {
            self.next_edits.pending = Some(pending);
            return Ok(None);
        };
        if sites.is_empty() {
            self.next_edits.pending = Some(pending);
            return Ok(None);
        }
        let site = sites.remove(0);
        let left = sites.len();
        // Every change of a text drops the suggestions, so the place says the old name; one
        // that does not is no place to change.
        let Some(before) = self
            .host
            .text(&site.path)
            .filter(|text| text.get(site.start..site.end) == Some(site.old.as_bytes()))
        else {
            return Ok(None);
        };
        let mut after = before.clone();
        after.splice(site.start..site.end, site.new.bytes());
        let new_end = site.start.saturating_add(site.new.len());
        self.set_text(&site.path, after.clone(), Source::Elsewhere)?;
        // The name typed over is where it is first declared, so no place is before it in
        // its own file, and it stays where it is.
        pending.follow(&site.path, site.end, new_end);
        pending.taken = pending.taken.saturating_add(1);
        self.next_edits.pending = Some(pending);
        Ok(Some(Accepted {
            change: Change {
                path: site.path,
                before,
                after,
            },
            offset: new_end,
            left,
        }))
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
        let workbench = context.workbench();
        workbench.settle_next_edit();
        Ok(Outcome::unchanged(workbench.next_edit()))
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
