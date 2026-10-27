// SPDX-License-Identifier: GPL-3.0-or-later

//! A salsa-style incremental query database for the semantic model.
//!
//! The editor re-runs the semantic model on every keystroke, so it must recompute only what
//! an edit changed (GOAL-1, the incremental model; the design doc, "Semantic model"). This
//! database holds each file's source text as an *input*, and memoizes the *derived* queries
//! built on top: [`Db::parse`] for one file, [`Db::lower`] for one file's declarations, and
//! [`Db::module_index`] for a whole module's resolved symbol table.
//!
//! Incrementality follows salsa's model in miniature. Every input carries a *revision*, the
//! value of a monotonic clock when it last changed. A memoized query records the highest
//! input revision it read; when asked again it recomputes only if one of the files it
//! depends on has a newer revision. Editing one file bumps only that file's revision, so a
//! module whose other files are unchanged re-parses and re-lowers only the edited file, and
//! re-resolves the module from the mix of fresh and cached per-file results.
//!
//! [`Db::analyze_module`] reads a module as the compiler does: it preprocesses the module's
//! files in compilation order (QC-9), parses and lowers each file's expansion, and resolves the
//! module, mapping every location back to a span of a source file through the expansion
//! origins (QC-9.7). Its per-file results are memoized on the expanded text, salsa's early
//! cutoff: an edit is preprocessed again, but only a file whose expansion changed is parsed
//! and lowered again, so editing a macro's definition re-parses the files that use it and no
//! others. [`Db::parse`] stays the tree of the file as written, the editing view.
//!
//! There is no crate dependency on `salsa`: the engine is small, and the project builds its
//! foundations itself. Every query the database answers also ticks a counter
//! ([`QueryCounts`]), so a test can assert that an edit re-ran only the dependent queries
//! (the issue's second acceptance criterion).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use qc_diag::{Code, Diagnostic};
use qc_syntax::Parse;

use crate::expand::{
    Files, ModuleInput, OriginMap, Preprocessed, ProtoPlan, Rendered, preprocess, settings_at,
};
use crate::lower::{FileDecls, lower_file};
use crate::resolve::{
    FileDiagnostic, FilePlace, FirstPass, ModuleIndex, ResolveOptions, Sources, resolve_mapped,
    resolve_module,
};
use crate::symbol::{FileId, Location};

/// How many times each memoized query actually ran, as opposed to being served from cache.
///
/// The counts only ever rise. A test records them, makes an edit, and checks the deltas to
/// prove an edit re-ran only the queries that depended on the changed file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryCounts {
    /// Times [`Db::parse`] parsed a file from scratch.
    pub parse: u64,
    /// Times [`Db::lower`] lowered a file from scratch.
    pub lower: u64,
    /// Times [`Db::module_index`] or [`Db::analyze_module`] resolved a module from scratch.
    pub module_index: u64,
    /// Times [`Db::analyze_module`] preprocessed a module.
    pub preprocess: u64,
}

/// A module read as the compiler reads it: resolved from its files' expansions
/// ([`Db::analyze_module`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleAnalysis {
    /// The resolved module, every location a span of a source file.
    pub index: ModuleIndex,
    /// The preprocessor's, lexer's and parser's diagnostics, each at a span of a source file.
    pub syntax: Vec<FileDiagnostic>,
}

impl ModuleAnalysis {
    /// Every diagnostic: the syntax diagnostics, then the semantic ones.
    pub fn diagnostics(&self) -> impl Iterator<Item = &FileDiagnostic> {
        self.syntax.iter().chain(&self.index.diagnostics)
    }
}

/// The parse and lowering of one compiled file's expansion, kept while the expansion's text
/// and the lexer's flags in it are the same.
#[derive(Clone, Debug)]
struct ExpandedFile {
    text: Vec<u8>,
    /// The pragmas' changes to the compiler flags, from each rendered offset (FTEF-21.1).
    flags: Vec<(usize, qc_preproc::FlagEdit)>,
    /// The pragmas' changes to the keywords, from each rendered offset (FTEF-20.1).
    keywords: Vec<(usize, qc_preproc::KeywordEdit)>,
    /// The rendered offsets of the strings FTEQCC's lexer did not join to the one before them
    /// (FTEF-21.17).
    seams: Vec<usize>,
    parse: Parse,
    decls: FileDecls,
}

/// A monotonic revision stamp. A larger value is later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Revision(u64);

/// One input file: its text and the revision at which it last changed.
#[derive(Clone, Debug)]
struct InputFile {
    text: Vec<u8>,
    revision: Revision,
}

/// A memoized derived value, with the input revision it was computed at.
#[derive(Clone, Debug)]
struct Memo<T> {
    value: T,
    /// The highest revision of any input this value read.
    computed_at: Revision,
}

/// The incremental semantic database.
///
/// Set a file's text with [`Db::set_file_text`], then ask for [`Db::parse`], [`Db::lower`]
/// or [`Db::module_index`]. The database is single-threaded: the syntax tree it caches is
/// built on `Rc` (`qc_syntax::SyntaxNode`), matching the editor's analysis thread.
#[derive(Debug, Default)]
pub struct Db {
    /// The current clock. Bumped each time an input changes.
    clock: Revision,
    /// The dialect every file is parsed in (PRJ-2.8). Vanilla by default.
    dialect: qc_syntax::Dialect,
    /// Whether modules resolve with FTEQCC's autoprototyping (FTE-22.3). Off by default.
    autoproto: bool,
    /// The input files, by id.
    files: HashMap<FileId, InputFile>,
    /// Memoized per-file parses.
    parses: HashMap<FileId, Memo<Parse>>,
    /// The files that are a module's source in source form, a `progs.src` whose own text
    /// FTEQCC compiles, parsed past a first `#!` line (PRJ-4.10, FTEF-19.27).
    module_sources: HashSet<FileId>,
    /// Memoized per-file lowerings.
    lowerings: HashMap<FileId, Memo<FileDecls>>,
    /// The memoized module index over an ordered list of files.
    module: Option<Memo<(Vec<FileId>, ModuleIndex)>>,
    /// Each file's path, for the preprocessor (QC-9.2).
    paths: HashMap<FileId, PathBuf>,
    /// The parse and lowering of each compiled file's last expansion, by the pass it is of:
    /// `true` for FTEQCC's first pass when a second follows.
    expanded: HashMap<(FileId, bool), ExpandedFile>,
    /// The memoized analysis of a module read through the preprocessor.
    analysis: Option<Memo<(ModuleInput, ModuleAnalysis)>>,
    /// Query-run counters.
    counts: Cell<QueryCounts>,
}

impl Db {
    /// A new, empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialect files are parsed in.
    #[must_use]
    pub fn dialect(&self) -> qc_syntax::Dialect {
        self.dialect
    }

    /// Sets the dialect every file is parsed in (PRJ-2.8), selecting the vanilla or an FTEQCC
    /// profile. Changing it invalidates every cached parse, lowering and module index, since
    /// they depend on the dialect; set it once before adding files in the common case.
    pub fn set_dialect(&mut self, dialect: qc_syntax::Dialect) {
        if self.dialect == dialect {
            return;
        }
        self.dialect = dialect;
        self.clock.0 = self.clock.0.saturating_add(1);
        self.parses.clear();
        self.lowerings.clear();
        self.module = None;
        self.expanded.clear();
        self.analysis = None;
        // Stamp every file with the new revision so dependent queries recompute.
        for file in self.files.values_mut() {
            file.revision = self.clock;
        }
    }

    /// Sets whether modules resolve with FTEQCC's autoprototyping, `-Fautoproto` (FTE-22.3):
    /// every top-level declaration visible to every function body. It changes only the
    /// module index, so only that is recomputed.
    pub fn set_autoproto(&mut self, autoproto: bool) {
        if self.autoproto != autoproto {
            self.autoproto = autoproto;
            self.module = None;
            self.analysis = None;
        }
    }

    /// The query-run counters so far.
    #[must_use]
    pub fn counts(&self) -> QueryCounts {
        self.counts.get()
    }

    /// Sets, or replaces, the text of file `file`.
    ///
    /// This is an input change: it bumps the clock and stamps the file with the new
    /// revision, so queries that read this file recompute on their next call and queries
    /// that do not are served from cache. Setting the same bytes a file already has is still
    /// a change: the revision advances. Callers that want to avoid needless recomputation
    /// compare the bytes themselves before calling.
    pub fn set_file_text(&mut self, file: FileId, text: Vec<u8>) {
        self.clock.0 = self.clock.0.saturating_add(1);
        self.files.insert(
            file,
            InputFile {
                text,
                revision: self.clock,
            },
        );
    }

    /// Sets, or replaces, the path and text of file `file`: the path is how the preprocessor
    /// finds it ([`Db::analyze_module`]), as a file a module lists or one it `#include`s.
    pub fn set_file(&mut self, file: FileId, path: PathBuf, text: Vec<u8>) {
        self.paths.insert(file, path);
        self.set_file_text(file, text);
    }

    /// The path of file `file`, if it was set with [`Db::set_file`].
    #[must_use]
    pub fn file_path(&self, file: FileId) -> Option<&Path> {
        self.paths.get(&file).map(PathBuf::as_path)
    }

    /// The bytes of file `file`, or an empty slice if it has none.
    #[must_use]
    pub fn file_text(&self, file: FileId) -> &[u8] {
        self.files.get(&file).map_or(&[], |f| f.text.as_slice())
    }

    /// The revision at which file `file` last changed.
    fn file_revision(&self, file: FileId) -> Revision {
        self.files.get(&file).map_or(Revision(0), |f| f.revision)
    }

    fn bump_parse(&self) {
        let mut c = self.counts.get();
        c.parse = c.parse.saturating_add(1);
        self.counts.set(c);
    }

    fn bump_lower(&self) {
        let mut c = self.counts.get();
        c.lower = c.lower.saturating_add(1);
        self.counts.set(c);
    }

    fn bump_preprocess(&self) {
        let mut c = self.counts.get();
        c.preprocess = c.preprocess.saturating_add(1);
        self.counts.set(c);
    }

    fn bump_module_index(&self) {
        let mut c = self.counts.get();
        c.module_index = c.module_index.saturating_add(1);
        self.counts.set(c);
    }

    /// The parse of file `file`, memoized.
    ///
    /// Recomputes only when the file's text has changed since the cached parse.
    pub fn parse(&mut self, file: FileId) -> Parse {
        let revision = self.file_revision(file);
        if let Some(memo) = self.parses.get(&file)
            && memo.computed_at >= revision
        {
            return memo.value.clone();
        }
        self.bump_parse();
        let value = if self.module_sources.contains(&file) {
            qc_syntax::parse_module_source(self.file_text(file), self.dialect)
        } else {
            qc_syntax::parse_with(self.file_text(file), self.dialect)
        };
        self.parses.insert(
            file,
            Memo {
                value: value.clone(),
                computed_at: revision,
            },
        );
        value
    }

    /// The lowered declarations of file `file`, memoized.
    ///
    /// Depends only on the file's parse, so it recomputes only when the file changed.
    pub fn lower(&mut self, file: FileId) -> FileDecls {
        let revision = self.file_revision(file);
        if let Some(memo) = self.lowerings.get(&file)
            && memo.computed_at >= revision
        {
            return memo.value.clone();
        }
        let parse = self.parse(file);
        self.bump_lower();
        let value = lower_file(&parse, self.dialect);
        self.lowerings.insert(
            file,
            Memo {
                value: value.clone(),
                computed_at: revision,
            },
        );
        value
    }

    /// The resolved semantic index of the module made of `files`, in compilation order
    /// (QC-23.3), memoized.
    ///
    /// Recomputes only when the file list changes or one of its files does. The per-file
    /// [`Db::lower`] results it reads are themselves cached, so an edit to one file
    /// re-lowers only that file and then re-resolves the module from the cached rest.
    pub fn module_index(&mut self, files: &[FileId]) -> ModuleIndex {
        let max_revision = files
            .iter()
            .map(|&f| self.file_revision(f))
            .max()
            .unwrap_or(Revision(0));
        if let Some(memo) = &self.module
            && memo.value.0 == files
            && memo.computed_at >= max_revision
        {
            return memo.value.1.clone();
        }
        let lowered: Vec<FileDecls> = files.iter().map(|&f| self.lower(f)).collect();
        self.bump_module_index();
        let options = ResolveOptions {
            dialect: self.dialect,
            autoproto: self.autoproto,
            case_insensitive: false,
            qcfuncs_off: false,
            target: None,
        };
        let index = resolve_module(&lowered, options);
        self.module = Some(Memo {
            value: (files.to_vec(), index.clone()),
            computed_at: max_revision,
        });
        index
    }

    /// FTEF-31.4: each file's name and lines, which "Address-of temp on line" prints.
    fn places(&self) -> HashMap<FileId, FilePlace> {
        self.paths
            .iter()
            .map(|(&id, path)| {
                let text = self.files.get(&id).map_or(&[][..], |f| f.text.as_slice());
                let lines = std::iter::once(0)
                    .chain(
                        text.iter()
                            .enumerate()
                            .filter(|&(_, &b)| b == b'\n')
                            .map(|(at, _)| at.saturating_add(1)),
                    )
                    .collect();
                let name = path.to_string_lossy().into_owned();
                (id, FilePlace { name, lines })
            })
            .collect()
    }

    /// The module `input` read as the compiler reads it, memoized: its files preprocessed in
    /// compilation order (QC-9, QC-23.3; twice with autoprototyping, FTE-22.4), each file's
    /// expansion parsed and lowered, and the module resolved, with every definition,
    /// reference and diagnostic at a span of a source file (QC-9.7).
    ///
    /// Recomputes when any file with a path changes. Preprocessing then runs again, but a
    /// file whose expansion is the same text as before is not parsed or lowered again: an
    /// edit re-parses only the files whose expansion it changed.
    pub fn analyze_module(&mut self, input: &ModuleInput) -> ModuleAnalysis {
        let max_revision = self
            .paths
            .keys()
            .map(|&f| self.file_revision(f))
            .max()
            .unwrap_or(Revision(0));
        if let Some(memo) = &self.analysis
            && memo.value.0 == *input
            && memo.computed_at >= max_revision
        {
            return memo.value.1.clone();
        }
        // FTEF-19.27: the prelude's files are the source form's `progs.src`, parsed as a
        // module's source; a file that is one no longer, or newly, is parsed again.
        let sources: HashSet<FileId> = input.prelude.iter().copied().collect();
        if sources != self.module_sources {
            for file in sources.symmetric_difference(&self.module_sources) {
                self.parses.remove(file);
                self.lowerings.remove(file);
            }
            self.module = None;
            self.module_sources = sources;
        }
        self.bump_preprocess();
        let preprocessed = {
            let mut files = Files {
                texts: HashMap::new(),
                ids: HashMap::new(),
                paths: HashMap::new(),
            };
            // The preprocessor names a file by its normalized path (QC-9.2), so the files are
            // looked up by theirs.
            for (&id, path) in &self.paths {
                let path = qc_project::normalize(path);
                files.texts.insert(path.clone(), self.file_text(id));
                files.ids.insert(path.clone(), id);
                files.paths.insert(id, path);
            }
            preprocess(&files, input, self.dialect, self.autoproto)
        };
        let mut syntax = preprocessed.diagnostics.clone();
        let mut decls = Vec::with_capacity(input.compiled.len());
        let mut keywords = Vec::with_capacity(input.compiled.len());
        let mut relexed = Vec::new();
        let preprocessor_count = syntax.len();
        for (&id, rendered) in input.compiled.iter().zip(&preprocessed.files) {
            let file = self.expanded_file(id, rendered, false);
            keywords.push(file.parse.keywords().to_vec());
            // FTEF-21.17: the tokens an `extern` block lexed with other flags have the parser's
            // lexical errors and warnings, not the preprocessor's.
            relexed.extend(
                file.parse
                    .relexed()
                    .iter()
                    .filter_map(|&span| rendered.origins.locate(span)),
            );
            // FTEF-12.19: a single pass that prototypes part of the module skips those
            // initialisers as the first of two does.
            let skipped = match (&preprocessed.plan, preprocessed.autoproto) {
                (Some(plan), false) => prototyped_initialisers(&file.parse, plan, decls.len()),
                (Some(_) | None, _) => Vec::new(),
            };
            syntax.extend(parse_diagnostics(&file.parse, rendered, id, &skipped));
            decls.push(file.decls);
        }
        let FirstRead {
            decls: first_decls,
            keywords: first_keywords,
            relexed: first_relexed,
            syntax: first_syntax,
        } = self.read_first_pass(input, &preprocessed);
        if !relexed.is_empty() {
            let mut index = 0_usize;
            syntax.retain(|d| {
                let from_preprocessor = index < preprocessor_count;
                index = index.saturating_add(1);
                let lexical = lexer_code(d.diagnostic.code);
                let at = qc_diag::Span::new(d.diagnostic.span.start, d.diagnostic.span.start);
                !(from_preprocessor
                    && lexical
                    && relexed
                        .iter()
                        .any(|l| l.file == d.file && l.span.contains(at)))
            });
        }
        // The first pass found its own, which the second finds again unless its flags differ.
        for d in first_syntax {
            let at = qc_diag::Span::new(d.diagnostic.span.start, d.diagnostic.span.start);
            let relexed_there = lexer_code(d.diagnostic.code)
                && first_relexed
                    .iter()
                    .any(|l| l.file == d.file && l.span.contains(at));
            if !relexed_there && !syntax.contains(&d) {
                syntax.push(d);
            }
        }
        let maps: Vec<&OriginMap> = preprocessed.files.iter().map(|r| &r.origins).collect();
        // FTEF-19.7: FTEQCC keeps a function's first declaration against one in a file named
        // `*extensions.qc`.
        let extensions = self
            .paths
            .iter()
            .filter(|(_, path)| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().ends_with("extensions.qc"))
            })
            .map(|(&id, _)| id)
            .collect();
        let places = self.places();
        let warnings = each_file(&preprocessed.files, |r| r.warnings.as_slice());
        let keywords: Vec<&[(usize, qc_syntax::Keywords)]> =
            keywords.iter().map(Vec::as_slice).collect();
        let targets = each_file(&preprocessed.files, |r| r.targets.as_slice());
        let first_maps: Vec<&OriginMap> = preprocessed.first.iter().map(|r| &r.origins).collect();
        let first_warnings = each_file(&preprocessed.first, |r| r.warnings.as_slice());
        let first_keywords: Vec<&[(usize, qc_syntax::Keywords)]> =
            first_keywords.iter().map(Vec::as_slice).collect();
        let sources = Sources {
            maps: &maps,
            warnings: &warnings,
            keywords: &keywords,
            targets: &targets,
            extensions,
            places,
            plan: preprocessed.plan.as_ref(),
            first: preprocessed.autoproto.then_some(FirstPass {
                files: &first_decls,
                maps: &first_maps,
                warnings: &first_warnings,
                keywords: &first_keywords,
            }),
        };
        self.bump_module_index();
        let command_line_flags = input
            .command_line
            .flags(qc_preproc::FlagSettings::default());
        let options = ResolveOptions {
            dialect: self.dialect,
            autoproto: preprocessed.autoproto,
            // FTEF-19.33, FTEF-19.28: only the command line sets `caseinsens`, and `qcfuncs`
            // before the core types are named.
            case_insensitive: command_line_flags.is_on(qc_preproc::FteFlag::Caseinsensitive),
            qcfuncs_off: !command_line_flags.is_on(qc_preproc::FteFlag::Qcfuncs),
            target: Some(preprocessed.target),
        };
        let analysis = ModuleAnalysis {
            index: resolve_mapped(&decls, &sources, options),
            syntax,
        };
        self.analysis = Some(Memo {
            value: (input.clone(), analysis.clone()),
            computed_at: max_revision,
        });
        analysis
    }

    /// FTEF-12.18: the first pass of a two-pass compile, read with its own flags, warning
    /// settings and keywords: each compiled file's declarations and keywords, the spans its
    /// `extern` blocks lexed otherwise, and the diagnostics the second pass may not find again.
    fn read_first_pass(&mut self, input: &ModuleInput, preprocessed: &Preprocessed) -> FirstRead {
        if preprocessed.first.is_empty() {
            self.expanded.retain(|(_, first), _| !*first);
        }
        let mut read = FirstRead {
            syntax: preprocessed
                .first_diagnostics
                .iter()
                .filter(|d| lexer_code(d.diagnostic.code))
                .cloned()
                .collect(),
            ..FirstRead::default()
        };
        for (&id, rendered) in input.compiled.iter().zip(&preprocessed.first) {
            let file = self.expanded_file(id, rendered, true);
            read.keywords.push(file.parse.keywords().to_vec());
            read.relexed.extend(
                file.parse
                    .relexed()
                    .iter()
                    .filter_map(|&span| rendered.origins.locate(span)),
            );
            // FTEF-12.19: what the first pass skips over, FTEQCC's parser does not read.
            let skipped = preprocessed
                .plan
                .as_ref()
                .map(|plan| prototyped_initialisers(&file.parse, plan, read.decls.len()))
                .unwrap_or_default();
            read.syntax
                .extend(parse_diagnostics(&file.parse, rendered, id, &skipped));
            read.decls.push(file.decls);
        }
        read
    }

    /// The lowered declarations of compiled file `file` as [`Db::analyze_module`] last read
    /// it, from its expansion (QC-9.7, QC-9.16); `None` before a module that compiles it has
    /// been analysed.
    #[must_use]
    pub fn expanded_decls(&self, file: FileId) -> Option<&FileDecls> {
        self.expanded
            .get(&(file, false))
            .map(|expanded| &expanded.decls)
    }

    /// The parse and lowering of compiled file `file` whose expansion is `rendered`: the cached
    /// ones when the expansion and the lexer's flags and keywords in it are unchanged, otherwise
    /// parsed and lowered again.
    fn expanded_file(&mut self, file: FileId, rendered: &Rendered, first: bool) -> ExpandedFile {
        let text = rendered.text.as_slice();
        // The first pass read as the second was, with the same text, flags and keywords, is
        // that parse again.
        if first
            && let Some(second) = self.expanded.get(&(file, false))
            && second.text == text
            && second.flags == rendered.flags
            && second.keywords == rendered.keywords
        {
            return second.clone();
        }
        if let Some(cached) = self.expanded.get(&(file, first))
            && cached.text == text
            && cached.flags == rendered.flags
            && cached.keywords == rendered.keywords
            && cached.seams == rendered.seams
        {
            return cached.clone();
        }
        self.bump_parse();
        let parse = qc_syntax::parse_preprocessed_flagged(
            text,
            self.dialect,
            &rendered.flags,
            &rendered.keywords,
            &rendered.seams,
        );
        self.bump_lower();
        let decls = lower_file(&parse, self.dialect);
        let value = ExpandedFile {
            text: text.to_vec(),
            flags: rendered.flags.clone(),
            keywords: rendered.keywords.clone(),
            seams: rendered.seams.clone(),
            parse,
            decls,
        };
        self.expanded.insert((file, first), value.clone());
        value
    }
}

/// Whether the lexer gives `code` as it lexes a token's text, which lexing the token again with
/// other flags gives again or not (QCD-2, FTEF-12.12, FTEF-21.17): the lexical errors and the
/// lexer's own warnings, not the preprocessor's, such as a stale macro or a macro in a string.
const fn lexer_code(code: Code) -> bool {
    matches!(
        code,
        Code::E0101
            | Code::E0102
            | Code::E0103
            | Code::E0104
            | Code::E0105
            | Code::E0123
            | Code::E0124
            | Code::W0110
            | Code::W0111
            | Code::W0112
            | Code::W0118
            | Code::W0119
            | Code::W0120
            | Code::W0121
            | Code::W0123
    )
}

/// What [`Db::read_first_pass`] reads of the first pass.
#[derive(Default)]
struct FirstRead {
    /// Each compiled file's declarations.
    decls: Vec<FileDecls>,
    /// Each compiled file's keywords at each offset.
    keywords: Vec<Vec<(usize, qc_syntax::Keywords)>>,
    /// The spans its `extern` blocks lexed with other flags.
    relexed: Vec<Location>,
    /// Its diagnostics.
    syntax: Vec<FileDiagnostic>,
}

/// The diagnostics of the parse of compiled file `id`, whose expansion is `rendered`, at their
/// source spans (QC-9.7), each warning as the settings where it stands report it (FTEF-19.22).
fn parse_diagnostics<'a>(
    parse: &'a Parse,
    rendered: &'a Rendered,
    id: FileId,
    skipped: &'a [qc_diag::Span],
) -> impl Iterator<Item = FileDiagnostic> + 'a {
    parse.diagnostics().iter().filter_map(move |d| {
        if skipped.iter().any(|s| s.contains(d.span)) {
            return None;
        }
        let settings = settings_at(&rendered.warnings, d.span.start);
        let code = settings.map_or(Some(d.code), |s| s.reported_code(d.code))?;
        let d = Diagnostic {
            code,
            message: d.message.clone(),
            span: d.span,
        };
        Some(locate_diagnostic(&rendered.origins, id, &d))
    })
}

/// The spans of the initialisers, function bodies among them, of the declarations that FTEQCC's
/// first pass reads with `autoprototype` on, in compiled file number `file` (FTEF-12.19): it
/// skips over them.
fn prototyped_initialisers(parse: &Parse, plan: &ProtoPlan, file: usize) -> Vec<qc_diag::Span> {
    use qc_syntax::ast::AstNode;

    let Some(tree) = parse.tree() else {
        return Vec::new();
    };
    tree.declarations()
        .flat_map(|declaration| declaration.declarators().collect::<Vec<_>>())
        .filter_map(|declarator| {
            let start = declarator.name()?.span().start;
            let initialiser = declarator.initializer()?;
            plan.proto_at(file, start)
                .then(|| initialiser.syntax().span())
        })
        .collect()
}

/// A parser diagnostic of a file's expansion, at the source span it came from (QC-9.7).
fn locate_diagnostic(origins: &OriginMap, file: FileId, d: &Diagnostic) -> FileDiagnostic {
    let location = origins
        .locate(d.span)
        .unwrap_or(Location { file, span: d.span });
    FileDiagnostic {
        file: location.file,
        diagnostic: Diagnostic {
            code: d.code,
            message: d.message.clone(),
            span: location.span,
        },
    }
}

/// What `get` reads of each of `files`, in order.
fn each_file<'r, T>(files: &'r [Rendered], get: impl Fn(&'r Rendered) -> &'r [T]) -> Vec<&'r [T]> {
    files.iter().map(get).collect()
}
