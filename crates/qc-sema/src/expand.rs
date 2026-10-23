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
use qc_preproc::{CommandLine, FileSpan, Loader, PpToken, Preprocessor, WarningSettings};
use qc_project::{FileGraph, Form, Module, Via};

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
    /// What a `#pragma autoproto` of a list-form `progs.src` asks for (FTEF-12.10), which the
    /// graph reads, since that file is not preprocessed here.
    pub autoproto: Option<bool>,
    /// The module's include directories (PRJ-5.2), FTEQCC's `-I` options (FTEF-14.49).
    pub include_dirs: Vec<PathBuf>,
    /// FTEQCC's command line, whose `-W`, `-F` and `-K` options set the warnings and compiler
    /// flags the module is read with (FTEF-14.72); none by default.
    pub command_line: CommandLine,
}

impl ModuleInput {
    /// The input for a module's file graph, each file named by its index in
    /// `graph.files` (as [`Location::file`] is): the root `progs.src` of the source form is
    /// the prelude, and every file the graph lists but did not reach by `#include` is
    /// compiled, in the graph's order.
    #[must_use]
    pub fn from_graph(graph: &FileGraph) -> Self {
        let mut input = Self {
            autoproto: graph.autoproto,
            ..Self::default()
        };
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

    /// The input for `module`: its file graph's ([`Self::from_graph`]) with its include
    /// directories.
    #[must_use]
    pub fn from_module(module: &Module) -> Self {
        Self {
            include_dirs: module.include_dirs.clone(),
            ..Self::from_graph(&module.graph)
        }
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
    /// The rendered offset each token starts at, every token's, in order: the preprocessor
    /// marks pragmas by token index, and a token with no source location has no origin.
    pub(crate) starts: Vec<usize>,
    /// Under FTEQCC, the changes to the compiler flags the pragmas make: the rendered offset
    /// from which each holds, and the change (FTEF-12.12, FTEF-21.1); empty under the vanilla
    /// dialect.
    pub(crate) flags: Vec<(usize, qc_preproc::FlagEdit)>,
    /// Under FTEQCC, the warning settings: the rendered offset from which each holds, and the
    /// settings (FTEF-14.63, FTEF-19.22); empty under the vanilla dialect.
    pub(crate) warnings: Vec<(usize, WarningSettings)>,
    /// Under FTEQCC, the changes to the keywords the pragmas make: the rendered offset from
    /// which each holds, and the change (FTEF-20.1); empty under the vanilla dialect.
    pub(crate) keywords: Vec<(usize, qc_preproc::KeywordEdit)>,
    /// The rendered offset of each string token that follows a string token it does not make
    /// one string with: FTEQCC's lexer joins the parts of a string written in one text, not a
    /// macro's string and the one beside it (FTEF-21.17).
    pub(crate) seams: Vec<usize>,
}

/// The settings among `warnings`, each the offset it holds from, in force at `at`: the last
/// that starts at or before it; `None` when there are none, as under the vanilla dialect.
pub(crate) fn settings_at(
    warnings: &[(usize, WarningSettings)],
    at: usize,
) -> Option<WarningSettings> {
    warnings
        .iter()
        .take_while(|(from, _)| *from <= at)
        .last()
        .or_else(|| warnings.first())
        .map(|(_, settings)| *settings)
}

/// The preprocessed module: each compiled file rendered, and the preprocessor's diagnostics
/// at their source spans.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Preprocessed {
    /// The compiled files' renderings, in the order of [`ModuleInput::compiled`].
    pub(crate) files: Vec<Rendered>,
    /// Lexical and preprocessor diagnostics.
    pub(crate) diagnostics: Vec<FileDiagnostic>,
    /// Whether the module was read with FTEQCC's autoprototyping (FTE-22.3): asked for by the
    /// caller, or by a `#pragma autoproto` before any definition (FTEF-12.10).
    pub(crate) autoproto: bool,
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
/// Autoprototyping is `autoproto`, FTEQCC's `-Fautoproto`, unless a `#pragma autoproto`
/// before any definition of the module says otherwise (FTEF-12.10). A later one is the
/// warning of FTEF-12.10, given once: FTEQCC's second pass skips the pragma
/// (`if (!autoprototyped)`).
pub(crate) fn preprocess(
    files: &Files<'_>,
    input: &ModuleInput,
    dialect: qc_lexer::Dialect,
    autoproto: bool,
) -> Preprocessed {
    let loader = DbLoader {
        texts: &files.texts,
    };
    let mut pp = Preprocessor::new(loader)
        .with_dialect(dialect)
        .with_command_line(input.command_line.clone())
        .with_include_dirs(&input.include_dirs);
    let (mut first, pragma, late) = run(&mut pp, files, input);
    if !pragma.or(input.autoproto).unwrap_or(autoproto) {
        first.diagnostics.extend(late);
        return first;
    }
    pp.restart();
    let (mut second, _, _) = run(&mut pp, files, input);
    second.autoproto = true;
    second.diagnostics.extend(late);
    second
}

/// One pass over the prelude and the compiled files, the setting of the last
/// `#pragma autoproto` that comes before any definition (FTEF-12.10): one in the prelude, or
/// one in a compiled file before the first token of any compiled file; and the warning for
/// each later one.
fn run<L: Loader>(
    pp: &mut Preprocessor<L>,
    files: &Files<'_>,
    input: &ModuleInput,
) -> (Preprocessed, Option<bool>, Vec<FileDiagnostic>) {
    let mut out = Preprocessed::default();
    let mut late = Vec::new();
    let mut pragma = None;
    let mut defined = false;
    let listed = input
        .prelude
        .iter()
        .map(|id| (id, false))
        .chain(input.compiled.iter().map(|id| (id, true)));
    for (id, compiled) in listed {
        let read = match files.paths.get(id) {
            Some(path) => {
                // PRJ-4.10: the prelude is the module's source, read as `QCC_main` reads it.
                let output = if compiled {
                    pp.preprocess_file(path)
                } else {
                    pp.preprocess_source(path)
                };
                output.map_err(|err| format!("{}: {err}", path.display()))
            }
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
        for &(at, on, span) in &output.autoproto {
            if !compiled || (!defined && at == 0) {
                pragma = Some(on);
            } else if let Some(location) = source_location(pp, files, span) {
                // `if (numpr_globals != RESERVED_OFS) QCC_PR_ParseWarning(WARN_BADPRAGMA,
                // "#pragma autoproto must appear before any definitions");`
                late.push(FileDiagnostic {
                    file: location.file,
                    diagnostic: Diagnostic {
                        code: Code::W0108,
                        message: "#pragma autoproto must appear before any definitions".to_owned(),
                        span: location.span,
                    },
                });
            }
        }
        if !compiled {
            continue;
        }
        defined |= !output.tokens.is_empty();
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
        let mut rendered = render(pp, files, &output.tokens);
        let offset_of = |rendered: &Rendered, token: usize| {
            rendered
                .starts
                .get(token)
                .copied()
                .unwrap_or(rendered.text.len())
        };
        rendered.flags = output
            .flags
            .iter()
            .map(|&(token, flags)| (offset_of(&rendered, token), flags))
            .collect();
        rendered.warnings = output
            .warnings
            .iter()
            .map(|&(token, settings)| (offset_of(&rendered, token), settings))
            .collect();
        rendered.keywords = output
            .keywords
            .iter()
            .map(|&(token, edit)| (offset_of(&rendered, token), edit))
            .collect();
        out.files.push(rendered);
    }
    (out, pragma, late)
}

/// The database location of a preprocessor span: the database file with the same path, and the
/// bytes of it the span's text came from (FTEF-14.64).
fn source_location<L: Loader>(
    pp: &Preprocessor<L>,
    files: &Files<'_>,
    span: FileSpan,
) -> Option<Location> {
    let source = pp.sources().file(span.file)?;
    let file = *files.ids.get(&source.path)?;
    Some(Location {
        file,
        span: source.file_span(span.span),
    })
}

/// Renders a preprocessed file as text: each token's own bytes (a resolved frame reference
/// as its number), a line break wherever the token's origin moves to another line, and a
/// space otherwise. A token that starts its line indented stays indented, since the parser's
/// recovery treats a token in column 0 as the start of a top-level declaration (QCD-1.4).
fn render<L: Loader>(pp: &Preprocessor<L>, files: &Files<'_>, tokens: &[PpToken]) -> Rendered {
    let mut out = Rendered::default();
    let mut last_line = None;
    let mut last: Option<&PpToken> = None;
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
        } else if !last.is_some_and(|last| adjacent(pp, last, token)) {
            out.text.push(b' ');
        }
        last_line = line;
        if last.is_some_and(|last| unjoined_strings(last, token)) {
            out.seams.push(out.text.len());
        }
        last = Some(token);
        out.starts.push(out.text.len());
        if let Some(location) = source_location(pp, files, origin) {
            out.origins.origins.push((out.text.len(), location));
        }
        let own = token
            .spelling
            .as_deref()
            .map(str::as_bytes)
            .or_else(|| pp.sources().text(token.span))
            .unwrap_or_default();
        match token.kind {
            TokenKind::Float(value) if own.first() == Some(&b'$') => {
                out.text.extend_from_slice(value.to_string().as_bytes());
            }
            _ => out.text.extend_from_slice(own),
        }
    }
    out
}

/// Whether `last` and `next` are strings that `QCC_PR_LexString` does not read into one: it
/// joins the parts written in the text it is reading, a macro's text or a file's, so strings of
/// two expansions, or of an expansion and the source, or of two files, are two tokens.
fn unjoined_strings(last: &PpToken, next: &PpToken) -> bool {
    matches!(last.kind, TokenKind::String(_))
        && matches!(next.kind, TokenKind::String(_))
        && match (last.expansion, next.expansion) {
            (None, None) => last.span.file != next.span.file,
            (from, to) => from != to,
        }
}

/// Whether `next` comes right after `last` in the text the compiler reads, both rendered as
/// those bytes (QC-9.16): they touch ([`PpToken::touches`]: in the source, in a macro's body,
/// in one argument, or under FTEQCC an argument and the body token it is written against), and
/// neither is a built-in constant's value in the source nor a frame reference rendered as its
/// number. The two are rendered with no space between, since a rule may depend on it: a C
/// label is a name with its `:` right after it (FTEF-18.23), and an FTEQCC `#` opens a
/// builtin only right before its number or string (FTE-23.1).
fn adjacent<L: Loader>(pp: &Preprocessor<L>, last: &PpToken, next: &PpToken) -> bool {
    // QC-9.16: a token an expansion made, a `##` paste or a `#` string, is its text there.
    let as_written = |token: &PpToken| match &token.spelling {
        Some(_) => token.expansion.is_some(),
        None => pp
            .sources()
            .text(token.span)
            .is_some_and(|text| text.first() != Some(&b'$')),
    };
    next.touches(last) && as_written(last) && as_written(next)
}
