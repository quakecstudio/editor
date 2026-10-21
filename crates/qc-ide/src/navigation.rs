// SPDX-License-Identifier: GPL-3.0-or-later

//! Go-to-definition and find-references across a project's modules.
//!
//! Within a module these are the semantic model's (`qc_sema::ModuleIndex::definition_at` and
//! `references`), which follow id's `qcc` resolution (QC-12). Across modules, a definition in a
//! file several modules compile, such as a shared `defs.qc`, is the same definition in each, so
//! its references are those of every module that compiles its file.

use std::path::Path;

use qc_project::Loader;
use qc_sema::{DefId, Location};

use crate::FileRange;
use crate::host::AnalysisHost;

/// A definition in one module: the module's index in the host and the definition's id there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModuleDef {
    pub(crate) module: usize,
    pub(crate) def: DefId,
}

impl<L: Loader> AnalysisHost<L> {
    /// The definition the name at byte `offset` of `path` names, or the name ending at it, in
    /// the first module that compiles the file and resolves it.
    pub(crate) fn def_at(&mut self, path: &Path, offset: usize) -> Option<ModuleDef> {
        for (module, file) in self.modules_of(path) {
            let Some((_, data)) = self.module(module) else {
                continue;
            };
            let index = &data.analysis.index;
            let found = index.definition_at(file, offset).or_else(|| {
                offset
                    .checked_sub(1)
                    .and_then(|before| index.definition_at(file, before))
            });
            if let Some((def, _)) = found {
                return Some(ModuleDef { module, def });
            }
        }
        None
    }

    /// Where a location of module `module` is, as a path and span.
    pub(crate) fn range(&self, module: usize, location: Location) -> Option<FileRange> {
        let (state, _) = self.module(module)?;
        Some(FileRange {
            path: state.path(location.file)?.to_path_buf(),
            span: location.span,
        })
    }

    /// The definition `target` is in every module that compiles its file, `target` first.
    pub(crate) fn same_defs(&mut self, target: ModuleDef) -> Vec<ModuleDef> {
        let mut out = vec![target];
        let Some(at) = self.def_range(target) else {
            return out;
        };
        let Some((name, kind)) = self
            .module(target.module)
            .and_then(|(_, data)| data.analysis.index.def(target.def))
            .map(|d| (d.name.clone(), d.kind))
        else {
            return out;
        };
        for (module, file) in self.modules_of(&at.path) {
            if module == target.module {
                continue;
            }
            let Some((_, data)) = self.module(module) else {
                continue;
            };
            let found = data.analysis.index.defs.iter().position(|d| {
                d.location.file == file
                    && d.location.span == at.span
                    && d.name == name
                    && d.kind == kind
            });
            if let Some(index) = found {
                out.push(ModuleDef {
                    module,
                    def: DefId(index),
                });
            }
        }
        out
    }

    /// Where definition `target` is declared.
    pub(crate) fn def_range(&self, target: ModuleDef) -> Option<FileRange> {
        let (_, data) = self.module(target.module)?;
        let def = data.analysis.index.def(target.def)?;
        self.range(target.module, def.location)
    }

    /// Go-to-definition: where the name at byte `offset` of `path` is defined (QC-12.5). A
    /// definition's own name, or a later declaration of it (QC-11.5), goes to the first
    /// declaration. `None` when no name there resolves.
    pub fn definition(&mut self, path: &Path, offset: usize) -> Option<FileRange> {
        let target = self.def_at(path, offset)?;
        self.def_range(target)
    }

    /// Find-references: every place the definition of the name at byte `offset` of `path` is
    /// used, in every module that compiles its file, and with `include_declaration` every
    /// declaration of it too; sorted by path and position, each once.
    pub fn references(
        &mut self,
        path: &Path,
        offset: usize,
        include_declaration: bool,
    ) -> Vec<FileRange> {
        let Some(target) = self.def_at(path, offset) else {
            return Vec::new();
        };
        let mut out: Vec<FileRange> = Vec::new();
        for same in self.same_defs(target) {
            let Some((_, data)) = self.module(same.module) else {
                continue;
            };
            let locations = data
                .analysis
                .index
                .references(same.def, include_declaration);
            for location in locations {
                if let Some(range) = self.range(same.module, location) {
                    out.push(range);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}
