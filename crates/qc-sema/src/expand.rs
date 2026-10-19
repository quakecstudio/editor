// SPDX-License-Identifier: GPL-3.0-or-later

//! A module's files as the compiler reads them: after the preprocessor (QC-9).
//!
//! The parser cannot read a file as written when the file uses a macro as a statement
//! (`INHIBIT_COOP` in `rerelease-mg1`) or keeps code under a conditional that the build does
//! not take. So the semantic model preprocesses the module's files in compilation order
//! (`qc_preproc`), renders each file's tokens back to text, and parses and resolves that
//! text. Every token of the rendered text keeps the place it came from in the source: its own
//! span for an ordinary token, the outermost macro use for a token produced by expansion
//! (QC-9.7). [`OriginMap::locate`] maps a span of the rendered text back through those
//! origins, so every definition, reference and diagnostic is reported at a span of a source
//! file, while the lossless tree of the file as written stays the editing view.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use qc_diag::{Code, Diagnostic, Span};
use qc_lexer::TokenKind;
use qc_preproc::{FileSpan, Loader, PpToken, Preprocessor};
use qc_project::{FileGraph, Form, Via};

use crate::resolve::FileDiagnostic;
use crate::symbol::{FileId, Location};

/// Which files of a module the semantic model reads, and how (QC-23.3, PRJ-5).
///
/// Every id names a file whose path and text are in the [`crate::Db`]
/// ([`crate::Db::set_file`]). An `#include`d file is not listed: the preprocessor reads it
/// through its path when a listed file includes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModuleInput {
    /// Files preprocessed only for the macros they define, before the compiled files: the
    /// `progs.src` of FTEQCC's source form, whose own lines are the graph's (PRJ-4.4, PRJ-5).
    pub prelude: Vec<FileId>,
    /// The files compiled, in compilation order (QC-23.3).
    pub compiled: Vec<FileId>,
}

impl ModuleInput {
    /// The input for a module's file graph, each file named by its index in
    /// `graph.files` (as [`Location::file`] is): the root `progs.src` of the source form is
    /// the prelude, and every file the graph lists but did not reach by `#include` is
    /// compiled, in the graph's order.
    #[must_use]
    pub fn from_graph(graph: &FileGraph) -> Self {
        let mut input = Self::default();
        for (index, file) in graph.files.iter().enumerate() {
            let id = FileId(index);
            match file.via {
                Via::Include => {}
                Via::Root if graph.form == Some(Form::Source) => input.prelude.push(id),
                Via::Root | Via::Listed | Via::IncludeList => input.compiled.push(id),
            }
        }
        input
    }
}

/// Where each byte range of a rendered file came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OriginMap {
    /// For each token of the rendered text, in order: the offset it starts at and its origin.
    origins: Vec<(usize, Location)>,
}

impl OriginMap {
    /// The source location of the span `span` of the rendered text (QC-9.7): from the origin
    /// of the token it starts in to the end of the origin of the token it ends in, when the
    /// two are in the same file and in order, and otherwise the first token's origin alone.
    /// `None` when the rendered text has no token at or before `span.start`.
    #[must_use]
    pub fn locate(&self, span: Span) -> Option<Location> {
        let first = self.origin_at(span.start)?;
        let last_byte = span.end.checked_sub(1).filter(|&e| e > span.start);
        let Some(last) = last_byte.and_then(|e| self.origin_at(e)) else {
            return Some(first);
        };
        if last.file == first.file && last.span.end >= first.span.start {
            return Some(Location {
                file: first.file,
                span: Span::new(first.span.start, last.span.end),
            });
        }
        Some(first)
    }

    /// A map from `(offset, origin)` pairs in rendered order.
    #[cfg(test)]
    pub(crate) fn from_origins(origins: Vec<(usize, Location)>) -> Self {
        Self { origins }
    }

    /// The origin of the token the rendered offset `offset` belongs to.
    fn origin_at(&self, offset: usize) -> Option<Location> {
        let at = self.origins.partition_point(|(start, _)| *start <= offset);
        let index = at.checked_sub(1)?;
        self.origins.get(index).map(|(_, location)| *location)
    }
}

/// One compiled file after preprocessing: the text the parser reads and its origins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rendered {
    /// The rendered tokens.
    pub(crate) text: Vec<u8>,
    /// Where each token came from.
    pub(crate) origins: OriginMap,
}

/// The preprocessed module: each compiled file rendered, and the preprocessor's diagnostics
/// at their source spans.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Preprocessed {
    /// The compiled files' renderings, in the order of [`ModuleInput::compiled`].
    pub(crate) files: Vec<Rendered>,
    /// Lexical and preprocessor diagnostics.
    pub(crate) diagnostics: Vec<FileDiagnostic>,
}

/// Reads files out of the database by path, so the preprocessor sees the editor's buffers
/// rather than the disk.
struct DbLoader<'a> {
    texts: &'a HashMap<PathBuf, &'a [u8]>,
}

impl Loader for DbLoader<'_> {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        self.texts
            .get(path)
            .map(|text| text.to_vec())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }
}

/// The files of the database the preprocessor can read: each path's text and id.
pub(crate) struct Files<'a> {
    /// Each file's text by its path.
    pub(crate) texts: HashMap<PathBuf, &'a [u8]>,
    /// Each file's id by its path.
    pub(crate) ids: HashMap<PathBuf, FileId>,
    /// Each file's path by its id.
    pub(crate) paths: HashMap<FileId, PathBuf>,
}

/// Preprocesses the module `input` in `dialect` (QC-9, QC-23.3), and with FTEQCC's
/// autoprototyping twice, as FTEQCC reads the files twice (FTE-22.3, FTE-22.4).
pub(crate) fn preprocess(
    files: &Files<'_>,
    input: &ModuleInput,
    dialect: qc_lexer::Dialect,
    autoproto: bool,
) -> Preprocessed {
    let loader = DbLoader {
        texts: &files.texts,
    };
    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
    if autoproto {
        let _ = run(&mut pp, files, input);
        pp.restart();
    }
    run(&mut pp, files, input)
}

/// One pass over the prelude and the compiled files.
fn run<L: Loader>(
    pp: &mut Preprocessor<L>,
    files: &Files<'_>,
    input: &ModuleInput,
) -> Preprocessed {
    let mut out = Preprocessed::default();
    let listed = input
        .prelude
        .iter()
        .map(|id| (id, false))
        .chain(input.compiled.iter().map(|id| (id, true)));
    for (id, compiled) in listed {
        let read = match files.paths.get(id) {
            Some(path) => pp
                .preprocess_file(path)
                .map_err(|err| format!("{}: {err}", path.display())),
            None => Err(format!("file {} has no path", id.0)),
        };
        let output = match read {
            Ok(output) => output,
            Err(err) => {
                // QCD-5.2: a listed file that cannot be read (QC-23.5).
                out.diagnostics.push(FileDiagnostic {
                    file: *id,
                    diagnostic: Diagnostic {
                        code: Code::E0402,
                        message: format!("Error opening {err}"),
                        span: Span::new(0, 0),
                    },
                });
                if compiled {
                    out.files.push(Rendered::default());
                }
                continue;
            }
        };
        if !compiled {
            continue;
        }
        for d in output.diagnostics {
            let Some(location) = source_location(pp, files, d.span) else {
                continue;
            };
            out.diagnostics.push(FileDiagnostic {
                file: location.file,
                diagnostic: Diagnostic {
                    code: d.code,
                    message: d.message,
                    span: location.span,
                },
            });
        }
        out.files.push(render(pp, files, &output.tokens));
    }
    out
}

/// The database location of a preprocessor span: the database file with the same path.
fn source_location<L: Loader>(
    pp: &Preprocessor<L>,
    files: &Files<'_>,
    span: FileSpan,
) -> Option<Location> {
    let path = &pp.sources().file(span.file)?.path;
    let file = *files.ids.get(path)?;
    Some(Location {
        file,
        span: span.span,
    })
}

/// Renders a preprocessed file as text: each token's own bytes (a resolved frame reference
/// as its number), a line break wherever the token's origin moves to another line, and a
/// space otherwise. A token that starts its line indented stays indented, since the parser's
/// recovery treats a token in column 0 as the start of a top-level declaration (QCD-1.4).
fn render<L: Loader>(pp: &Preprocessor<L>, files: &Files<'_>, tokens: &[PpToken]) -> Rendered {
    let mut out = Rendered::default();
    let mut last_line = None;
    for token in tokens {
        let origin = pp.origin(token);
        let line = pp.sources().line(origin);
        if line != last_line {
            if !out.text.is_empty() {
                out.text.push(b'\n');
            }
            let file = pp.sources().file(origin.file);
            let indented = file
                .and_then(|f| origin.span.start.checked_sub(1).and_then(|i| f.text.get(i)))
                .is_some_and(|b| *b != b'\n');
            if indented {
                out.text.push(b' ');
            }
        } else {
            out.text.push(b' ');
        }
        last_line = line;
        if let Some(location) = source_location(pp, files, origin) {
            out.origins.origins.push((out.text.len(), location));
        }
        let own = pp.sources().text(token.span).unwrap_or_default();
        match token.kind {
            TokenKind::Float(value) if own.first() == Some(&b'$') => {
                out.text.extend_from_slice(value.to_string().as_bytes());
            }
            _ => out.text.extend_from_slice(own),
        }
    }
    out
}
