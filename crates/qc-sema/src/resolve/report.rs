// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GetDef

//! Reporting a diagnostic under the warning settings in force where it is found
//! (FTEF-19.22), at a location mapped back to its source (QC-9.7).

use qc_diag::{Code, Span};
use qc_syntax::Keywords;

use crate::expand::settings_at;
use crate::resolve::{Resolver, diag};
use crate::symbol::{FileId, Location};

impl Resolver<'_> {
    /// The source location of the span `span` of file `file`'s parsed text: through the
    /// file's origin map when it was preprocessed (QC-9.7), else the span itself.
    pub(in crate::resolve) fn at(&self, file: FileId, span: Span) -> Location {
        self.maps
            .get(file.0)
            .and_then(|map| map.locate(span))
            .unwrap_or(Location { file, span })
    }

    /// Reports `code` at `location`, as the warning settings in force have it (FTEF-19.22): a
    /// warning they ignore is not reported, and one they make an error is `E0125`.
    pub(in crate::resolve) fn report(&mut self, code: Code, location: Location, message: String) {
        let code = match self.settings {
            Some(settings) if self.profile.fteqcc_diagnostics() => match settings.reported_code(code) {
                Some(code) => code,
                None => return,
            },
            Some(_) | None => code,
        };
        self.index.diagnostics.push(diag(code, location, message));
    }

    /// FTEF-19.22: takes the warning settings in force at `at` in the compiled file
    /// `self.unit`, as FTEQCC's `qccwarningaction` stands when it reads there.
    pub(in crate::resolve) fn settle(&mut self, at: usize) {
        if let Some(events) = self.warnings.get(self.unit.0) {
            self.settings = settings_at(events, at).or(self.settings);
        }
        // FTEF-20.1: and the keywords the parser read there.
        if let Some(marks) = self.keyword_marks.get(self.unit.0) {
            self.keywords = Keywords::at(marks, at);
        }
    }
}
