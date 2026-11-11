// SPDX-License-Identifier: GPL-3.0-or-later

//! Project search and replace (P4-33, UI-17), as Zed's project search and VS Code's Search view
//! offer it: the text found in every file of the project, matching case or not, as a whole word
//! or not, and literally or as a regular expression; and every match of a file replaced at once,
//! saved, as one undoable change.
//!
//! The text is bytes, as the compiler reads it. A search runs over each stretch of valid UTF-8
//! between bytes that are not, so a match never spans such a byte and its offsets are the file's
//! own. Ignoring case folds ASCII letters only, which is all QuakeC's names and keywords use.
//!
//! `file.replace_all` replaces every match in one file and saves it; replacing across a project is
//! that command once per file, so each file is its own step in the undo history.
//! `file.replace_range` changes one range of a file's text, unsaved, as an edit in an excerpt of
//! the multibuffer does.

use std::path::{Path, PathBuf};

use qc_project::{Loader, normalize};
use qs_commands::{BoxError, Command, Outcome, RegisterError, Registry, UndoStep};
use regex_lite::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::Source;
use crate::tools::SearchMatch;
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of the command that replaces every match in a file and saves it.
pub const REPLACE_IN_FILE: &str = "file.replace_all";
/// The id of the command that changes one range of a file's text.
pub const REPLACE_RANGE: &str = "file.replace_range";

/// Whether a search tells upper and lower case apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchCase {
    /// `A` finds `a`.
    #[default]
    Ignore,
    /// `A` finds only `A`.
    Match,
}

/// Whether a search finds its text only as a whole word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WholeWord {
    /// Anywhere: `self` finds `myself`.
    #[default]
    Anywhere,
    /// Only between word boundaries.
    Word,
}

/// How a search reads its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Syntax {
    /// As the characters themselves.
    #[default]
    Literal,
    /// As a regular expression, whose replacement may name its groups (`$1`, `${name}`).
    Regex,
}

/// What a search looks for, ready to run.
#[derive(Clone, Debug)]
pub struct Query {
    regex: Regex,
    syntax: Syntax,
}

impl Query {
    /// A search for `text`.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Pattern`] when `syntax` is [`Syntax::Regex`] and `text` is not one.
    pub fn new(
        text: &str,
        case: MatchCase,
        word: WholeWord,
        syntax: Syntax,
    ) -> Result<Self, WorkbenchError> {
        let body = match syntax {
            Syntax::Literal => regex_lite::escape(text),
            Syntax::Regex => text.to_owned(),
        };
        let pattern = match word {
            WholeWord::Anywhere => body,
            WholeWord::Word => format!(r"\b(?:{body})\b"),
        };
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(case == MatchCase::Ignore)
            .build()
            .map_err(|error| WorkbenchError::Pattern {
                pattern: text.to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Self { regex, syntax })
    }

    /// The query of a command's parameters.
    ///
    /// # Errors
    ///
    /// As [`Query::new`].
    pub fn of(options: &SearchOptions) -> Result<Self, WorkbenchError> {
        Self::new(
            &options.text,
            if options.case_sensitive {
                MatchCase::Match
            } else {
                MatchCase::Ignore
            },
            if options.whole_word {
                WholeWord::Word
            } else {
                WholeWord::Anywhere
            },
            if options.regex {
                Syntax::Regex
            } else {
                Syntax::Literal
            },
        )
    }

    /// Every match in `text`, as byte ranges; an empty match is never one.
    #[must_use]
    pub fn matches(&self, text: &[u8]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (base, valid) in valid_stretches(text) {
            for found in self.regex.find_iter(valid) {
                if found.start() < found.end() {
                    out.push((
                        base.saturating_add(found.start()),
                        base.saturating_add(found.end()),
                    ));
                }
            }
        }
        out
    }

    /// `text` with every match replaced by `replacement`, and how many there were. A regular
    /// expression's replacement expands its groups; a literal one is put as it is.
    #[must_use]
    pub fn replace(&self, text: &[u8], replacement: &str) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(text.len());
        let mut count = 0_usize;
        let mut at = 0_usize;
        for (base, valid) in valid_stretches(text) {
            for captures in self.regex.captures_iter(valid) {
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                if whole.start() == whole.end() {
                    continue;
                }
                let start = base.saturating_add(whole.start());
                out.extend_from_slice(text.get(at..start).unwrap_or_default());
                match self.syntax {
                    Syntax::Literal => out.extend_from_slice(replacement.as_bytes()),
                    Syntax::Regex => {
                        let mut expanded = String::new();
                        captures.expand(replacement, &mut expanded);
                        out.extend_from_slice(expanded.as_bytes());
                    }
                }
                at = base.saturating_add(whole.end());
                count = count.saturating_add(1);
            }
        }
        out.extend_from_slice(text.get(at..).unwrap_or_default());
        (out, count)
    }
}

/// Each stretch of valid UTF-8 in `text`, with the byte it starts at.
fn valid_stretches(text: &[u8]) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    for chunk in text.utf8_chunks() {
        let valid = chunk.valid();
        if !valid.is_empty() {
            out.push((at, valid));
        }
        at = at
            .saturating_add(valid.len())
            .saturating_add(chunk.invalid().len());
    }
    out
}

/// What a search looks for, as a command's parameters spell it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchOptions {
    /// The text to find.
    pub text: String,
    /// Whether case matters; it does not by default.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Whether only whole words are found.
    #[serde(default)]
    pub whole_word: bool,
    /// Whether the text is a regular expression.
    #[serde(default)]
    pub regex: bool,
}

/// The parameters of `file.replace_all`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceInFileParams {
    /// The file.
    pub path: PathBuf,
    /// The text to find.
    pub text: String,
    /// What each match becomes; a regular expression's may name its groups, `$1` or `${name}`.
    pub replacement: String,
    /// Whether case matters; it does not by default.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Whether only whole words are found.
    #[serde(default)]
    pub whole_word: bool,
    /// Whether the text is a regular expression.
    #[serde(default)]
    pub regex: bool,
}

/// What `file.replace_all` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplacedInFile {
    /// The file.
    pub path: PathBuf,
    /// How many matches were replaced; none changes nothing and saves nothing.
    pub replaced: usize,
}

/// The parameters of `file.replace_range`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceRangeParams {
    /// The file.
    pub path: PathBuf,
    /// The first byte replaced.
    pub start: usize,
    /// The byte after the last one replaced.
    pub end: usize,
    /// What the range becomes.
    pub text: String,
}

impl<L: Loader> Workbench<L> {
    /// Every match of `query` in the project's files, their current text, in path order: each
    /// with its line, from 1, and its byte range. Lines are counted by line feeds; at most
    /// `limit` matches, and whether there were more.
    pub fn search_with(&mut self, query: &Query, limit: usize) -> (Vec<SearchMatch>, bool) {
        let mut found = Vec::new();
        for path in self.project_files() {
            let Some(text) = self.host.text(&path) else {
                continue;
            };
            let mut line = 1_usize;
            let mut line_start = 0_usize;
            let mut scanned = 0_usize;
            for (start, end) in query.matches(&text) {
                if found.len() == limit {
                    return (found, true);
                }
                let before = text.get(scanned..start).unwrap_or_default();
                for (index, byte) in before.iter().enumerate() {
                    if *byte == b'\n' {
                        line = line.saturating_add(1);
                        line_start = scanned.saturating_add(index).saturating_add(1);
                    }
                }
                scanned = start;
                let line_end = text
                    .get(line_start..)
                    .and_then(|rest| rest.iter().position(|b| *b == b'\n'))
                    .map_or(text.len(), |i| line_start.saturating_add(i));
                let shown = text.get(line_start..line_end).unwrap_or_default();
                found.push(SearchMatch {
                    path: path.clone(),
                    line,
                    start,
                    end,
                    text: String::from_utf8_lossy(shown.strip_suffix(b"\r").unwrap_or(shown))
                        .into_owned(),
                });
            }
        }
        (found, false)
    }

    /// Replaces every match of `query` in `path`'s text with `replacement` and saves the file;
    /// the text before and after, and how many were replaced. Nothing changes when none was.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when the file has no text; [`WorkbenchError::Save`] when it
    /// cannot be written.
    pub fn replace_in(
        &mut self,
        path: &Path,
        query: &Query,
        replacement: &str,
    ) -> Result<(Vec<u8>, Vec<u8>, usize), WorkbenchError> {
        let path = normalize(path);
        let before = self
            .host
            .text(&path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        let (after, count) = query.replace(&before, replacement);
        if count > 0 {
            self.set_text(&path, after.clone(), Source::Elsewhere)?;
            self.save(&path)?;
        }
        Ok((before, after, count))
    }

    /// `path`'s text with bytes `start..end` replaced by `text`, unsaved; the text before and
    /// after.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when the file has no text; [`WorkbenchError::Offset`] when the
    /// range is not within it.
    pub fn replace_range(
        &mut self,
        path: &Path,
        (start, end): (usize, usize),
        text: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), WorkbenchError> {
        let path = normalize(path);
        let before = self
            .host
            .text(&path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        let (Some(head), Some(tail)) = (before.get(..start), before.get(end..)) else {
            return Err(WorkbenchError::Offset {
                path,
                offset: end.max(start),
                len: before.len(),
            });
        };
        if start > end {
            return Err(WorkbenchError::Offset {
                path,
                offset: start,
                len: before.len(),
            });
        }
        let mut after = Vec::with_capacity(before.len());
        after.extend_from_slice(head);
        after.extend_from_slice(text.as_bytes());
        after.extend_from_slice(tail);
        self.set_text(&path, after.clone(), Source::Elsewhere)?;
        Ok((before, after))
    }
}

/// A file's text before and after a change, and whether undoing it saves the file.
struct FileStep {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
    saved: Saved,
}

/// Whether a step's change was saved, so its undo and redo save too.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Saved {
    /// Kept in the editor's text only.
    Unsaved,
    /// Written to the file.
    Written,
}

impl FileStep {
    fn set<C: HasWorkbench>(&self, context: &mut C, before: bool) -> Result<(), BoxError> {
        let workbench = context.workbench();
        let text = if before { &self.before } else { &self.after };
        workbench.set_text(&self.path, text.clone(), Source::Elsewhere)?;
        if self.saved == Saved::Written {
            workbench.save(&self.path)?;
        }
        Ok(())
    }
}

impl<C: HasWorkbench + 'static> UndoStep<C> for FileStep {
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.set(context, true)
    }

    fn redo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.set(context, false)
    }
}

/// `file.replace_all`: every match in a file replaced, and the file saved.
struct ReplaceInFile;

impl<C: HasWorkbench + 'static> Command<C> for ReplaceInFile {
    const ID: &'static str = REPLACE_IN_FILE;
    const SUMMARY: &'static str = "Replaces every match of a search in a file and saves it, \
        as one undoable change; matching case, whole words and regular expressions as \
        project.search.";
    type Params = ReplaceInFileParams;
    type Output = ReplacedInFile;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ReplaceInFileParams,
    ) -> Result<Outcome<ReplacedInFile, C>, WorkbenchError> {
        let workbench = context.workbench();
        if workbench.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let query = Query::of(&SearchOptions {
            text: params.text,
            case_sensitive: params.case_sensitive,
            whole_word: params.whole_word,
            regex: params.regex,
        })?;
        let path = normalize(&params.path);
        let (before, after, replaced) = workbench.replace_in(&path, &query, &params.replacement)?;
        let done = ReplacedInFile {
            path: path.clone(),
            replaced,
        };
        if replaced == 0 {
            return Ok(Outcome::unchanged(done));
        }
        Ok(Outcome::undoable(
            done,
            FileStep {
                path,
                before,
                after,
                saved: Saved::Written,
            },
        ))
    }
}

/// `file.replace_range`: one range of a file's text changed, unsaved.
struct ReplaceRange;

impl<C: HasWorkbench + 'static> Command<C> for ReplaceRange {
    const ID: &'static str = REPLACE_RANGE;
    const SUMMARY: &'static str = "Replaces a byte range of a file's text, in the editor's \
        text; nothing is saved. An edit in an excerpt of the multibuffer.";
    type Params = ReplaceRangeParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ReplaceRangeParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        let workbench = context.workbench();
        let path = normalize(&params.path);
        let (before, after) =
            workbench.replace_range(&path, (params.start, params.end), &params.text)?;
        Ok(Outcome::undoable(
            (),
            FileStep {
                path,
                before,
                after,
                saved: Saved::Unsaved,
            },
        ))
    }
}

/// Registers `file.replace_all` and `file.replace_range`.
///
/// # Errors
///
/// [`RegisterError`] when an id is taken.
pub fn register_search<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ReplaceInFile)?;
    registry.register(ReplaceRange)
}

#[cfg(test)]
mod tests;
