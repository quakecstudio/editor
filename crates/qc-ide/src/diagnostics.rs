// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnostics: every problem the build would report, file by file.
//!
//! A module's diagnostics are the project graph's (PRJ-6.5), the preprocessor's, lexer's and
//! parser's, the semantic model's, and the `W0311` warnings for the engine it targets (ENGT-4),
//! all the one `qc_diag::Diagnostic` the compiler reports too (COMP-2). A file several modules
//! compile has the diagnostics of each, once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qc_diag::{Diagnostic, Span};
use qc_project::{Loader, normalize};

use crate::host::AnalysisHost;

impl<L: Loader> AnalysisHost<L> {
    /// The diagnostics of `path`, from every module that compiles it, in source order, each
    /// once.
    pub fn diagnostics(&mut self, path: &Path) -> Vec<Diagnostic> {
        let path = normalize(path);
        self.all_diagnostics().remove(&path).unwrap_or_default()
    }

    /// The diagnostics of every file a module reads, and of every file the project graph
    /// reports on, by path; a file with none maps to an empty list, so a front end can clear
    /// what it showed before.
    pub fn all_diagnostics(&mut self) -> BTreeMap<PathBuf, Vec<Diagnostic>> {
        self.ensure_all();
        let mut out: BTreeMap<PathBuf, Vec<Diagnostic>> = BTreeMap::new();
        let mut graph: Vec<qc_project::Diagnostic> = Vec::new();
        for index in 0..self.modules.len() {
            let Some((module, data)) = self.module(index) else {
                continue;
            };
            for path in &module.files {
                out.entry(path.clone()).or_default();
            }
            let semantic = data.analysis.diagnostics().map(|d| (d.file, &d.diagnostic));
            let missing = data.missing.iter().map(|d| (d.file, &d.diagnostic));
            for (file, diagnostic) in semantic.chain(missing) {
                if let Some(path) = module.path(file) {
                    out.entry(path.to_path_buf())
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
            graph.extend(module.graph_diagnostics.iter().cloned());
        }
        for d in graph {
            let path = normalize(&d.file);
            let span = self
                .text(&path)
                .map_or(Span::new(0, 0), |text| line_span(&text, d.line));
            out.entry(path).or_default().push(Diagnostic {
                code: d.code,
                message: d.message,
                span,
            });
        }
        for list in out.values_mut() {
            list.sort_by(|a, b| {
                (a.span, a.code.as_str(), &a.message).cmp(&(b.span, b.code.as_str(), &b.message))
            });
            list.dedup();
        }
        out
    }
}

/// The span of 1-based line `line` of `text`, without its line break; line 0, the whole source
/// (PRJ-6.5), is the empty span at the start.
fn line_span(text: &[u8], line: u32) -> Span {
    let Some(skip) = line.checked_sub(1) else {
        return Span::new(0, 0);
    };
    let mut start = 0_usize;
    for _ in 0..skip {
        match text
            .get(start..)
            .and_then(|rest| rest.iter().position(|&b| b == b'\n'))
        {
            Some(at) => start = start.saturating_add(at).saturating_add(1),
            None => return Span::new(text.len(), text.len()),
        }
    }
    let end = text
        .get(start..)
        .and_then(|rest| rest.iter().position(|&b| b == b'\n' || b == b'\r'))
        .map_or(text.len(), |at| start.saturating_add(at));
    Span::new(start, end)
}
