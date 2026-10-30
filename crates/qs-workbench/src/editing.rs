// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing the open project: an editor's text, saving it, renaming a name across files, and
//! the language features asked at a place: definition, references, hover and completion.

use std::path::{Path, PathBuf};

use qc_ide::{Caution, Completion, FileRange, Hover, Span};
use qc_project::{Loader, normalize};

use crate::{Workbench, WorkbenchError, problems, tree};

/// Where a new text of a file comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The editor showing the file, which has the text already.
    Editor,
    /// Anything else, such as a rename or its undo: the editor takes the new text.
    Elsewhere,
}

/// One file a rename changed: its text before and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The file.
    pub path: PathBuf,
    /// Its text before.
    pub before: Vec<u8>,
    /// Its text after.
    pub after: Vec<u8>,
}

/// What a rename did: each file it changed, and what it could not check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renamed {
    /// The files changed, in path order.
    pub changes: Vec<Change>,
    /// What a map may still name by the old name, if anything.
    pub caution: Option<Caution>,
}

impl<L: Loader> Workbench<L> {
    /// Takes `text` as the text of `path`, as an editor has it, and works out the tree,
    /// problems and symbols again, and the open file's outline and highlighting.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Project`] when `path` is a project file whose new text does not open.
    pub fn update(&mut self, path: &Path, text: Vec<u8>) -> Result<(), WorkbenchError> {
        self.set_text(path, text, Source::Editor)
    }

    /// Takes `text` as the text of `path`, from `source`.
    pub(crate) fn set_text(
        &mut self,
        path: &Path,
        text: Vec<u8>,
        source: Source,
    ) -> Result<(), WorkbenchError> {
        let path = normalize(path);
        self.host
            .set_overlay(&path, text.clone())
            .map_err(WorkbenchError::Project)?;
        if let Some(root) = &self.root {
            self.tree = tree(&self.host.files(), root);
        }
        self.problems = problems(&mut self.host);
        self.symbols = self.host.workspace_symbols();
        if let Some(open) = self.open.as_mut()
            && open.path == path
        {
            open.offset = open.offset.min(text.len());
            open.text = text;
            open.outline = self.host.document_symbols(&path);
            open.highlights = self.host.highlights(&path);
            if source == Source::Elsewhere {
                self.generation = self.generation.wrapping_add(1);
                open.version = self.generation;
            }
        }
        Ok(())
    }

    /// The current text of `path`: an editor's, else what is on disk.
    pub fn text(&mut self, path: &Path) -> Option<Vec<u8>> {
        self.host.text(path)
    }

    /// Writes the current text of `path` to disk.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when there is no text for it, and
    /// [`WorkbenchError::Save`] when it cannot be written.
    pub fn save(&mut self, path: &Path) -> Result<(), WorkbenchError> {
        let path = normalize(path);
        let text = self
            .host
            .text(&path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        std::fs::write(&path, text).map_err(|error| WorkbenchError::Save { path, error })
    }

    /// Where the name at byte `offset` of `path` is defined.
    pub fn definition(&mut self, path: &Path, offset: usize) -> Option<FileRange> {
        self.host.definition(path, offset)
    }

    /// Every declaration and use of the definition of the name at byte `offset` of `path`, by
    /// path and position.
    pub fn references(&mut self, path: &Path, offset: usize) -> Vec<FileRange> {
        self.host.references(path, offset, true)
    }

    /// What the name at byte `offset` of `path` is.
    pub fn hover(&mut self, path: &Path, offset: usize) -> Option<Hover> {
        self.host.hover(path, offset)
    }

    /// The names that may be written at byte `offset` of `path`.
    pub fn completions(&mut self, path: &Path, offset: usize) -> Vec<Completion> {
        self.host.completions(path, offset)
    }

    /// Renames the definition of the name at byte `offset` of `path` to `name`, in every file
    /// that writes it, as editors' texts; nothing is written to disk.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Rename`] when the rename is refused. A rename edits only QuakeC
    /// files, never a project file, so giving them their new text cannot fail.
    pub fn rename(
        &mut self,
        path: &Path,
        offset: usize,
        name: &str,
    ) -> Result<Renamed, WorkbenchError> {
        let rename = self
            .host
            .rename(path, offset, name)
            .map_err(WorkbenchError::Rename)?;
        let mut changes = Vec::new();
        for file in rename.edits {
            let before = self
                .host
                .text(&file.path)
                .ok_or_else(|| WorkbenchError::NoFile {
                    path: file.path.clone(),
                })?;
            let after = apply(&before, &file.edits);
            self.set_text(&file.path, after.clone(), Source::Elsewhere)?;
            changes.push(Change {
                path: file.path,
                before,
                after,
            });
        }
        Ok(Renamed {
            changes,
            caution: rename.caution,
        })
    }
}

/// `text` with each `(span, replacement)` of `edits` made; the spans do not overlap.
fn apply(text: &[u8], edits: &[(Span, String)]) -> Vec<u8> {
    let mut sorted: Vec<&(Span, String)> = edits.iter().collect();
    sorted.sort_by_key(|(span, _)| span.start);
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for (span, replacement) in sorted {
        let start = span.start.clamp(at, text.len());
        out.extend_from_slice(text.get(at..start).unwrap_or_default());
        out.extend_from_slice(replacement.as_bytes());
        at = span.end.clamp(start, text.len());
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests;
