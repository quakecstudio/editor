// SPDX-License-Identifier: GPL-3.0-or-later

//! A module's ordered file graph (PRJ-4, PRJ-5, PRJ-6).
//!
//! The source is read a line at a time. Only the directives that decide which files are read
//! are followed; everything else is the preprocessor's (QC-9) and the dialect profiles' work.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Component, Path, PathBuf};

use qc_lexer::Code;
use qc_preproc::Loader;

use crate::cond;

/// Most entries a module's file graph may hold (PRJ-6.4).
///
/// `PRJ_MAX_FILES` in `docs/spec/project-file.md`: a bound on graphs that bring files in
/// exponentially often, such as each file including the next one twice.
pub const PRJ_MAX_FILES: usize = 65_536;

/// Most files a graph may be reading at once, the module's source included (PRJ-6.3).
///
/// `PRJ_MAX_DEPTH` in `docs/spec/project-file.md`: a bound on files that bring themselves in
/// with no `#pragma once`.
pub const PRJ_MAX_DEPTH: usize = 200;

/// The UTF-8 byte order mark, skipped at the start of a file (PRJ-5.1).
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

/// How a module's source is read (PRJ-4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// id's list: the output, then the source files (QC-23, PRJ-4.3).
    List,
    /// FTEQCC's source form: the file is QuakeC and is compiled first (PRJ-4.4).
    Source,
}

/// How a file came into the graph (PRJ-6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The module's source, read in source form.
    Root,
    /// An entry of a list-form `progs.src`.
    Listed,
    /// An entry of an `#includelist`.
    IncludeList,
    /// An `#include`.
    Include,
}

/// One entry of a file graph: a file compiled at this point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphFile {
    /// The file, as resolved.
    pub path: PathBuf,
    /// How it came in.
    pub via: Via,
    /// The entry of the file that brought it in; `None` for the root and for list entries.
    pub parent: Option<usize>,
}

/// A problem found while building a graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Its code (QCD-5, QCD-8).
    pub code: Code,
    /// The message, with the names filled in.
    pub message: String,
    /// The file holding the directive or list entry that raised it.
    pub file: PathBuf,
    /// The 1-based line of the directive or entry, or 0 for the whole source (PRJ-6.5).
    pub line: u32,
}

/// The files one module compiles, in order (PRJ-6.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileGraph {
    /// How the source was read; `None` when it could not be read.
    pub form: Option<Form>,
    /// The files compiled, in compilation order.
    pub files: Vec<GraphFile>,
    /// The output the source declares (PRJ-4.3, PRJ-5.6).
    pub declared_output: Option<PathBuf>,
    /// The other programs' sources it registers with `#pragma sourcefile` (PRJ-5.7).
    pub sourcefiles: Vec<PathBuf>,
    /// The problems found.
    pub diagnostics: Vec<Diagnostic>,
}

/// A macro defined before a module's first file (PRJ-2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    /// Its name.
    pub name: String,
    /// Its body; empty for `NAME` alone.
    pub value: String,
}

impl Define {
    /// Parses `NAME` or `NAME=VALUE`, where `NAME` is a QuakeC name (QC-3.1).
    #[must_use]
    pub fn parse(entry: &str) -> Option<Self> {
        let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
        let mut bytes = name.bytes();
        let first = bytes.next()?;
        let is_name = (first.is_ascii_alphabetic() || first == b'_')
            && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_');
        is_name.then(|| Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// The path with `.` removed and each `..` folded into the name before it.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(component);
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                out.push(component);
            }
        }
    }
    out
}

fn dir_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Splits `line` into tokens as `COM_Parse` does (QC-23.2), skipping `/* */` comments too
/// (PRJ-4.2). Returns the tokens and whether the line ends inside a `/* */` comment.
fn com_parse_words(line: &[u8], mut in_comment: bool) -> (Vec<String>, bool) {
    let mut words = Vec::new();
    let mut i = 0_usize;
    loop {
        if in_comment {
            match find(line, i, b"*/") {
                Some(end) => {
                    in_comment = false;
                    i = end.saturating_add(2);
                }
                None => return (words, true),
            }
            continue;
        }
        let Some(&byte) = line.get(i) else { break };
        let next = line.get(i.saturating_add(1)).copied();
        match (byte, next) {
            (b'/', Some(b'/')) => break,
            (b'/', Some(b'*')) => {
                in_comment = true;
                i = i.saturating_add(2);
            }
            _ if byte <= b' ' => i = i.saturating_add(1),
            (b'"', _) => {
                let start = i.saturating_add(1);
                let end = line
                    .get(start..)
                    .and_then(|rest| rest.iter().position(|b| *b == b'"'))
                    .map_or(line.len(), |len| start.saturating_add(len));
                words.push(lossy(line.get(start..end)));
                i = end.saturating_add(1);
            }
            (b'{' | b'}' | b'(' | b')' | b'\'' | b':', _) => {
                words.push(char::from(byte).to_string());
                i = i.saturating_add(1);
            }
            _ => {
                let start = i;
                while let Some(&b) = line.get(i) {
                    if b <= b' ' || matches!(b, b'{' | b'}' | b'(' | b')' | b'\'' | b':') {
                        break;
                    }
                    i = i.saturating_add(1);
                }
                words.push(lossy(line.get(start..i)));
            }
        }
    }
    (words, in_comment)
}

/// Splits an `#includelist` line into words (PRJ-5.3). Returns the words and whether the line
/// ends inside a `/* */` comment.
fn list_words(line: &[u8], mut in_comment: bool) -> (Vec<String>, bool) {
    let mut words = Vec::new();
    let mut i = 0_usize;
    loop {
        if in_comment {
            match find(line, i, b"*/") {
                Some(end) => {
                    in_comment = false;
                    i = end.saturating_add(2);
                }
                None => return (words, true),
            }
            continue;
        }
        let Some(&byte) = line.get(i) else { break };
        let next = line.get(i.saturating_add(1)).copied();
        match (byte, next) {
            (b'/', Some(b'/')) => break,
            (b'/', Some(b'*')) => {
                in_comment = true;
                i = i.saturating_add(2);
            }
            _ if byte <= b' ' || matches!(byte, b',' | b';' | b'(' | b')' | b']') => {
                i = i.saturating_add(1);
            }
            _ => {
                let start = i;
                while let Some(&b) = line.get(i) {
                    let comment = b == b'/' && line.get(i.saturating_add(1)) == Some(&b'/');
                    if b <= b' ' || matches!(b, b',' | b';' | b'(' | b')' | b']') || comment {
                        break;
                    }
                    i = i.saturating_add(1);
                }
                let word = lossy(line.get(start..i));
                let word = word
                    .strip_prefix('"')
                    .and_then(|w| w.strip_suffix('"'))
                    .map_or_else(|| word.clone(), str::to_owned);
                if !word.is_empty() {
                    words.push(word);
                }
            }
        }
    }
    (words, in_comment)
}

/// Scans a line of QuakeC for comments, skipping string and character literals (PRJ-5.1).
/// Returns whether the line ends inside a `/* */` comment.
fn code_line(line: &[u8], mut in_comment: bool) -> bool {
    let mut i = 0_usize;
    loop {
        if in_comment {
            match find(line, i, b"*/") {
                Some(end) => {
                    in_comment = false;
                    i = end.saturating_add(2);
                }
                None => return true,
            }
            continue;
        }
        let Some(&byte) = line.get(i) else { break };
        let next = line.get(i.saturating_add(1)).copied();
        match (byte, next) {
            (b'/', Some(b'/')) => return false,
            (b'/', Some(b'*')) => {
                in_comment = true;
                i = i.saturating_add(2);
            }
            (b'"' | b'\'', _) => {
                i = i.saturating_add(1);
                while let Some(&b) = line.get(i) {
                    i = i.saturating_add(1);
                    if b == b'\\' {
                        i = i.saturating_add(1);
                    } else if b == byte {
                        break;
                    }
                }
            }
            _ => i = i.saturating_add(1),
        }
    }
    in_comment
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .and_then(|at| from.checked_add(at))
}

fn lossy(bytes: Option<&[u8]>) -> String {
    String::from_utf8_lossy(bytes.unwrap_or_default()).into_owned()
}

/// The directive on a line, if it is one (PRJ-5.1): its name and the text after the name.
fn directive(line: &[u8]) -> Option<(String, &[u8])> {
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let start = line.iter().position(|b| !blank(b))?;
    let rest = line.get(start..)?.strip_prefix(b"#")?;
    let skip = rest.iter().take_while(|b| blank(b)).count();
    let rest = rest.get(skip..)?;
    let first = rest.first()?;
    if !(first.is_ascii_alphabetic() || *first == b'_') {
        return None;
    }
    let len = rest
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
        .count();
    Some((lossy(rest.get(..len)), rest.get(len..).unwrap_or_default()))
}

/// The operand of `#include`, `#output` and similar: the text of a `"..."` or `<...>`.
fn quoted(operand: &[u8]) -> Option<String> {
    let operand = operand.trim_ascii_start();
    let close = match operand.first()? {
        b'"' => b'"',
        b'<' => b'>',
        _ => return None,
    };
    let body = operand.get(1..)?;
    let len = body.iter().position(|b| *b == close)?;
    Some(lossy(body.get(..len)))
}

/// The first name in `text`, after blanks.
fn first_name(text: &[u8]) -> Option<String> {
    let text = text.trim_ascii_start();
    let len = text
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
        .count();
    (len > 0).then(|| lossy(text.get(..len)))
}

#[derive(Debug)]
struct Cond {
    taking: bool,
    parent: bool,
    done: bool,
}

/// What the words of a line are, in the file being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    /// QuakeC: words are ignored.
    Code,
    /// A list-form `progs.src`: the first word is the output, the rest are files.
    Progs { output_seen: bool },
}

#[derive(Debug)]
struct Cursor {
    path: PathBuf,
    text: Vec<u8>,
    pos: usize,
    line: u32,
    entry: Option<usize>,
    base: Base,
    include_list: Option<u32>,
    conds: Vec<Cond>,
    in_comment: bool,
    pending: VecDeque<(String, u32, Via)>,
}

impl Cursor {
    fn new(path: PathBuf, text: Vec<u8>, entry: Option<usize>, base: Base) -> Self {
        Self {
            path,
            pos: if text.starts_with(UTF8_BOM) {
                UTF8_BOM.len()
            } else {
                0
            },
            text,
            line: 0,
            entry,
            base,
            include_list: None,
            conds: Vec::new(),
            in_comment: false,
            pending: VecDeque::new(),
        }
    }

    fn taking(&self) -> bool {
        self.conds.last().is_none_or(|c| c.taking)
    }

    /// The next line without its line break, with its 1-based number.
    fn next_line(&mut self) -> Option<(Vec<u8>, u32)> {
        let rest = self.text.get(self.pos..).filter(|r| !r.is_empty())?;
        let len = rest.iter().position(|b| *b == b'\n');
        let line = rest.get(..len.unwrap_or(rest.len()))?;
        let line = line.strip_suffix(b"\r").unwrap_or(line).to_vec();
        self.pos = match len {
            Some(len) => self.pos.saturating_add(len).saturating_add(1),
            None => self.text.len(),
        };
        self.line = self.line.saturating_add(1);
        Some((line, self.line))
    }
}

struct Builder<'l, L> {
    loader: &'l mut L,
    root_dir: PathBuf,
    macros: HashMap<String, String>,
    include_dirs: Vec<PathBuf>,
    once: HashSet<PathBuf>,
    stack: Vec<Cursor>,
    graph: FileGraph,
}

impl<L: Loader> Builder<'_, L> {
    fn report(&mut self, code: Code, message: String, file: &Path, line: u32) {
        self.graph.diagnostics.push(Diagnostic {
            code,
            message,
            file: file.to_path_buf(),
            line,
        });
    }

    fn run(&mut self) {
        while let Some(cursor) = self.stack.last_mut() {
            if let Some((word, line, via)) = cursor.pending.pop_front() {
                self.bring_in(&word, line, via);
                continue;
            }
            let Some((mut text, number)) = cursor.next_line() else {
                let opened = cursor.include_list;
                if let (Some(opened), Some(done)) = (opened, self.stack.pop()) {
                    let message = "#includelist without #endlist".to_owned();
                    self.report(Code::E0406, message, &done.path, opened);
                }
                continue;
            };
            let is_directive = !cursor.in_comment && directive(&text).is_some();
            if !cursor.taking() && !is_directive {
                continue;
            }
            if is_directive {
                while text.last() == Some(&b'\\') {
                    text.pop();
                    match cursor.next_line() {
                        Some((more, _)) => {
                            text.push(b'\n');
                            text.extend_from_slice(&more);
                        }
                        None => break,
                    }
                }
                if let Some((name, operand)) = directive(&text) {
                    let operand = operand.to_vec();
                    self.directive(&name, &operand, number);
                }
                if let Some(cursor) = self.stack.last_mut() {
                    cursor.in_comment = cursor.taking() && code_line(&text, false);
                }
                continue;
            }
            let (words, in_comment, via) = match (cursor.include_list, cursor.base) {
                (Some(_), _) => {
                    let (words, c) = list_words(&text, cursor.in_comment);
                    (words, c, Via::IncludeList)
                }
                (None, Base::Progs { .. }) => {
                    let (words, c) = com_parse_words(&text, cursor.in_comment);
                    (words, c, Via::Listed)
                }
                (None, Base::Code) => (Vec::new(), code_line(&text, cursor.in_comment), Via::Root),
            };
            cursor.in_comment = in_comment;
            for word in words {
                if let Base::Progs { output_seen: false } = cursor.base
                    && cursor.include_list.is_none()
                {
                    cursor.base = Base::Progs { output_seen: true };
                    self.graph.declared_output = Some(normalize(&dir_of(&cursor.path).join(word)));
                } else {
                    cursor.pending.push_back((word, number, via));
                }
            }
        }
    }

    fn directive(&mut self, name: &str, operand: &[u8], line: u32) {
        let Some(cursor) = self.stack.last_mut() else {
            return;
        };
        let parent = cursor.taking();
        match name {
            "ifdef" | "ifndef" => {
                let defined = first_name(operand).is_some_and(|n| self.macros.contains_key(&n));
                let taking = parent && defined == (name == "ifdef");
                cursor.conds.push(Cond {
                    taking,
                    parent,
                    done: taking,
                });
            }
            "if" => {
                let taking = parent && cond::eval(&lossy(Some(operand)), &self.macros);
                cursor.conds.push(Cond {
                    taking,
                    parent,
                    done: taking,
                });
            }
            "elif" | "elifdef" | "elifndef" => {
                if let Some(c) = cursor.conds.last_mut() {
                    let value = c.parent
                        && !c.done
                        && match name {
                            "elif" => cond::eval(&lossy(Some(operand)), &self.macros),
                            _ => {
                                let defined = first_name(operand)
                                    .is_some_and(|n| self.macros.contains_key(&n));
                                defined == (name == "elifdef")
                            }
                        };
                    c.taking = value;
                    c.done |= value;
                }
            }
            "else" => {
                if let Some(c) = cursor.conds.last_mut() {
                    c.taking = c.parent && !c.done;
                    c.done = true;
                }
            }
            "endif" => {
                cursor.conds.pop();
            }
            _ if !parent => {}
            "define" => {
                if let Some(macro_name) = first_name(operand) {
                    let body = operand
                        .trim_ascii_start()
                        .get(macro_name.len()..)
                        .unwrap_or_default();
                    self.macros
                        .insert(macro_name, lossy(Some(body.trim_ascii())));
                }
            }
            "undef" => {
                if let Some(macro_name) = first_name(operand) {
                    self.macros.remove(&macro_name);
                }
            }
            "include" => {
                if let Some(path) = quoted(operand) {
                    cursor.pending.push_back((path, line, Via::Include));
                }
            }
            "includelist" => cursor.include_list = Some(line),
            "endlist" => cursor.include_list = None,
            "output" => {
                if let Some(path) = quoted(operand) {
                    self.graph.declared_output = Some(normalize(&dir_of(&cursor.path).join(path)));
                }
            }
            "pragma" => self.pragma(operand),
            _ => {}
        }
    }

    /// The pragmas of PRJ-5.6 to PRJ-5.9.
    fn pragma(&mut self, operand: &[u8]) {
        let Some(cursor) = self.stack.last() else {
            return;
        };
        let operand = operand.trim_ascii_start();
        let len = operand
            .iter()
            .take_while(|b| !matches!(b, b' ' | b'\t' | b'('))
            .count();
        let pragma = lossy(operand.get(..len)).to_ascii_lowercase();
        let (args, _) = com_parse_words(operand.get(len..).unwrap_or_default(), false);
        let here = dir_of(&cursor.path);
        match pragma.as_str() {
            "progs_dat" => {
                if let Some(path) = args.first() {
                    self.graph.declared_output = Some(normalize(&here.join(path)));
                }
            }
            "sourcefile" => {
                for name in args {
                    let path = normalize(&self.root_dir.join(name));
                    if !self.graph.sourcefiles.contains(&path) {
                        self.graph.sourcefiles.push(path);
                    }
                }
            }
            "includedir" => {
                if let Some(dir) = args.first() {
                    self.include_dirs.push(normalize(&here.join(dir)));
                }
            }
            "once" => {
                let path = cursor.path.clone();
                self.once.insert(path);
            }
            _ => {}
        }
    }

    /// The components of `path` that the name `name` spells, as the directories holding them
    /// spell them, joined with `/`, when one differs from the name's (PRJ-7.2). The
    /// directory the name was resolved against is not compared.
    fn stored_spelling(&mut self, name: &Path, path: &Path) -> Option<String> {
        let spelt = normalize(name)
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count();
        let components: Vec<Component<'_>> = path.components().collect();
        let first = components.len().saturating_sub(spelt);
        let mut prefix: PathBuf = components.iter().take(first).collect();
        let mut differs = false;
        let mut stored = Vec::new();
        for component in components.iter().skip(first) {
            prefix.push(component);
            let name = component.as_os_str();
            let on_disk = self
                .loader
                .stored_name(&prefix)
                .unwrap_or_else(|| name.to_os_string());
            differs |= on_disk != name;
            stored.push(on_disk.to_string_lossy().into_owned());
        }
        differs.then(|| stored.join("/"))
    }

    /// Finds `word` (PRJ-5.2) and starts reading it.
    fn bring_in(&mut self, word: &str, line: u32, via: Via) {
        let Some(cursor) = self.stack.last() else {
            return;
        };
        let from = cursor.path.clone();
        let parent = cursor.entry;
        let relative = Path::new(word);
        let mut candidates = vec![normalize(&dir_of(&from).join(relative))];
        if relative.is_relative() {
            for dir in &self.include_dirs {
                candidates.push(normalize(&dir.join(relative)));
            }
        }
        let mut first_error: Option<io::Error> = None;
        let mut found = None;
        for candidate in &candidates {
            match self.loader.load(candidate) {
                Ok(bytes) => {
                    found = Some((candidate.clone(), bytes));
                    break;
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        let Some((path, bytes)) = found else {
            let shown = candidates.first().cloned().unwrap_or_default();
            let reason = first_error.map_or_else(String::new, |e| e.to_string());
            let (code, message) = if via == Via::Include {
                (
                    Code::E0111,
                    format!("cannot include {}: {reason}", shown.display()),
                )
            } else {
                (
                    Code::E0402,
                    format!("Error opening {}: {reason}", shown.display()),
                )
            };
            self.report(code, message, &from, line);
            return;
        };
        if let Some(stored) = self.stored_spelling(relative, &path) {
            let message = format!("{word} differs in case from the file it opens, {stored}");
            self.report(Code::W0402, message, &from, line);
        }
        if self.once.contains(&path) {
            return;
        }
        if self.stack.len() >= PRJ_MAX_DEPTH {
            let message = format!("files nested too deeply to read {}", path.display());
            self.report(Code::E0408, message, &from, line);
            return;
        }
        if self.graph.files.len() >= PRJ_MAX_FILES {
            let message = "too many files in the project graph".to_owned();
            self.report(Code::E0407, message, &from, line);
            self.stack.clear();
            return;
        }
        let entry = self.graph.files.len();
        self.graph.files.push(GraphFile {
            path: path.clone(),
            via,
            parent,
        });
        self.stack
            .push(Cursor::new(path, bytes, Some(entry), Base::Code));
    }
}

/// Whether the first token of `text`, split as `COM_Parse` splits (QC-23.2) with `/* */`
/// comments skipped, starts with `#` (PRJ-4.1). `None` when there is no token.
fn source_form(text: &[u8]) -> Option<bool> {
    let mut in_comment = false;
    let text = text.strip_prefix(UTF8_BOM).unwrap_or(text);
    for line in text.split(|b| *b == b'\n') {
        let (words, still) = com_parse_words(line, in_comment);
        in_comment = still;
        if let Some(first) = words.first() {
            return Some(first.starts_with('#'));
        }
    }
    None
}

/// Builds the file graph of the module whose source is `src` (PRJ-4 to PRJ-6).
///
/// `defines` are defined before the first file (PRJ-5.4) and `include_dirs` are searched
/// after the including file's directory (PRJ-5.2). Implements PRJ-4, PRJ-5 and PRJ-6; the
/// list form follows QC-23.
pub fn build_graph<L: Loader>(
    loader: &mut L,
    src: &Path,
    defines: &[Define],
    include_dirs: &[PathBuf],
) -> FileGraph {
    let src = normalize(src);
    let mut builder = Builder {
        loader,
        root_dir: dir_of(&src),
        macros: defines
            .iter()
            .map(|d| (d.name.clone(), d.value.clone()))
            .collect(),
        include_dirs: include_dirs.iter().map(|d| normalize(d)).collect(),
        once: HashSet::new(),
        stack: Vec::new(),
        graph: FileGraph::default(),
    };
    let text = match builder.loader.load(&src) {
        Ok(text) => text,
        Err(err) => {
            let message = format!("Error opening {}: {err}", src.display());
            builder.report(Code::E0402, message, &src, 0);
            return builder.graph;
        }
    };
    match source_form(&text) {
        Some(true) => {
            builder.graph.form = Some(Form::Source);
            builder.graph.files.push(GraphFile {
                path: src.clone(),
                via: Via::Root,
                parent: None,
            });
            builder
                .stack
                .push(Cursor::new(src, text, Some(0), Base::Code));
        }
        Some(false) => {
            builder.graph.form = Some(Form::List);
            let base = Base::Progs { output_seen: false };
            builder.stack.push(Cursor::new(src, text, None, base));
        }
        None => {
            builder.graph.form = Some(Form::List);
            let message = "No destination filename.  qcc -help for info.".to_owned();
            builder.report(Code::E0401, message, &src, 0);
            return builder.graph;
        }
    }
    builder.run();
    builder.graph
}
