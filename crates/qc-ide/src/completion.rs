// SPDX-License-Identifier: GPL-3.0-or-later

//! Completion: the names visible at the cursor.
//!
//! `qcc` has two scopes, the globals and the function being defined (QC-12.1), so the names
//! visible in a function body are every global, function and field, and that function's
//! parameters and the locals declared before the cursor. After a member access, `.` or `->`,
//! only fields are offered, since a field access names a field (QC-16.6); the members of an
//! FTEQCC vector (`.x`, FTE-9.1) and class are not offered there yet. The client filters the
//! list by what has been typed, as the protocol's completion model lets it.

use std::collections::HashSet;
use std::path::Path;

use qc_project::Loader;
use qc_sema::{DefId, DefKind, Definition};

use crate::host::AnalysisHost;
use crate::syntax::{after_member_access, enclosing_function};

/// One name to offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The name.
    pub label: String,
    /// What it is.
    pub kind: DefKind,
    /// Its type, as FTEQCC writes it, such as `float(entity)`.
    pub detail: String,
}

impl<L: Loader> AnalysisHost<L> {
    /// The names visible at byte `offset` of `path`, each once, in declaration order; read in
    /// the first module that compiles the file.
    pub fn completions(&mut self, path: &Path, offset: usize) -> Vec<Completion> {
        let Some(&(module, file)) = self.modules_of(path).first() else {
            return Vec::new();
        };
        let Some((_, data)) = self.module(module) else {
            return Vec::new();
        };
        let Some(parse) = data.parses.get(file.0) else {
            return Vec::new();
        };
        let index = &data.analysis.index;
        let fields_only = after_member_access(parse, offset);
        let function: Option<DefId> = enclosing_function(parse, offset)
            .and_then(|name| index.definition_at(file, name.span().start))
            .map(|(def, _)| def);
        let visible = |def: &Definition| -> bool {
            if def.generated || def.member_of.is_some() {
                return false;
            }
            if fields_only {
                return def.kind == DefKind::Field;
            }
            match def.kind {
                DefKind::Global | DefKind::Function | DefKind::Field => true,
                DefKind::Param => function.is_some() && def.owner == function,
                DefKind::Local => {
                    function.is_some()
                        && def.owner == function
                        && def.location.file == file
                        && def.location.span.start < offset
                }
            }
        };
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        for def in &index.defs {
            if visible(def) && seen.insert(def.name.as_str()) {
                out.push(Completion {
                    label: def.name.clone(),
                    kind: def.kind,
                    detail: def.ty.fteqcc_name(),
                });
            }
        }
        out
    }
}
