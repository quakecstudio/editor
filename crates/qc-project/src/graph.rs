// SPDX-License-Identifier: GPL-3.0-or-later

//! A module's ordered file graph (PRJ-4, PRJ-5, PRJ-6).
//!
//! The source is read a line at a time, and under the FTEQCC and GMQCC dialects each QuakeC
//! file with the preprocessor's directive reader (PRJ-5.13, PRJ-5.15). Only the directives
//! that decide which files are read are followed; everything else is the preprocessor's (QC-9)
//! and the dialect profiles' work.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::{Component, Path, PathBuf};

use qc_lexer::{Code, Dialect, is_whitespace};
use qc_preproc::{
    FileStart, FteEvent, FteFileReader, Loader, ModuleState, com_parse, cond, qcc_load_as,
};

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
    /// id's list: the output, then the source files (QC-23, PRJ-4.12).
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
    /// The output the source declares (PRJ-4.12, PRJ-5.6).
    pub declared_output: Option<PathBuf>,
    /// The other programs' sources it registers with `#pragma sourcefile` (PRJ-5.7).
    pub sourcefiles: Vec<PathBuf>,
    /// What a `#pragma autoproto [N]` in a list-form `progs.src` asks for, which comes
    /// before every definition of the module (FTEF-12.10); `None` when there is none.
    pub autoproto: Option<bool>,
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

/// Whether `byte` is one of the bytes `COM_Parse` makes a token of its own, which ends a word
/// (QC-23.2).
const fn single_token(byte: u8) -> bool {
    matches!(byte, b'{' | b'}' | b'(' | b')' | b'\'' | b':')
}

/// The name a quoted word of `quoted` bytes opens: its text up to a NUL in it, as the token is
/// a C string (QC-23.7).
fn quoted_name(quoted: &[u8]) -> String {
    lossy(quoted.split(|b| *b == 0).next())
}

/// Splits `line` into tokens as `COM_Parse` does (QC-23.2), skipping `/* */` comments too
/// (PRJ-4.2), `quote` the bytes of a quoted word the lines before left open, which runs on to
/// its closing quote whatever lines it takes (QC-23.9). Returns the tokens, whether the line ends
/// inside a `/* */` comment, and the bytes of a quoted word it leaves open, its line break among
/// them.
fn com_parse_words(
    line: &[u8],
    mut in_comment: bool,
    quote: Option<Vec<u8>>,
) -> (Vec<String>, bool, Option<Vec<u8>>) {
    let mut words = Vec::new();
    let mut i = 0_usize;
    let open = |mut bytes: Vec<u8>, from: &[u8]| {
        bytes.extend_from_slice(from);
        bytes.push(b'\n');
        bytes
    };
    if let Some(mut bytes) = quote {
        let Some(close) = line.iter().position(|b| *b == b'"') else {
            return (words, in_comment, Some(open(bytes, line)));
        };
        bytes.extend_from_slice(line.get(..close).unwrap_or_default());
        words.push(quoted_name(&bytes));
        i = close.saturating_add(1);
    }
    loop {
        if in_comment {
            match find(line, i, b"*/") {
                Some(end) => {
                    in_comment = false;
                    i = end.saturating_add(2);
                }
                None => return (words, true, None),
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
            // QC-23.8: `while ( (c = *data) <= ' ')` with a signed `char`.
            _ if is_whitespace(byte) => i = i.saturating_add(1),
            (b'"', _) => {
                let start = i.saturating_add(1);
                let rest = line.get(start..).unwrap_or_default();
                let Some(len) = rest.iter().position(|b| *b == b'"') else {
                    return (words, in_comment, Some(open(Vec::new(), rest)));
                };
                let end = start.saturating_add(len);
                words.push(quoted_name(line.get(start..end).unwrap_or_default()));
                i = end.saturating_add(1);
            }
            _ if single_token(byte) => {
                words.push(char::from(byte).to_string());
                i = i.saturating_add(1);
            }
            _ => {
                let start = i;
                while let Some(&b) = line.get(i) {
                    // QC-23.8: `} while (c>32);` ends the word at a byte of 0x80 or more too.
                    if is_whitespace(b) || single_token(b) {
                        break;
                    }
                    i = i.saturating_add(1);
                }
                words.push(lossy(line.get(start..i)));
            }
        }
    }
    (words, in_comment, None)
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
    /// The file being compiled while this one is read, FTEQCC's `compilingfile`: the file
    /// itself, or for an `#include`d one the file that included it is read for.
    unit: PathBuf,
    text: Vec<u8>,
    pos: usize,
    line: u32,
    entry: Option<usize>,
    base: Base,
    include_list: Option<u32>,
    conds: Vec<Cond>,
    in_comment: bool,
    /// In a list, the bytes of a quoted word open at the end of the line read last, and the line
    /// it opened on (QC-23.9).
    quote: Option<(Vec<u8>, u32)>,
    pending: VecDeque<(String, u32, Via)>,
    /// Under the FTEQCC and GMQCC dialects, the reader of a QuakeC file, which reads it in place
    /// of the lines above (PRJ-5.13, PRJ-5.15).
    fte: Option<FteFileReader>,
}

impl Cursor {
    fn new(path: PathBuf, text: Vec<u8>, entry: Option<usize>, base: Base) -> Self {
        Self {
            unit: path.clone(),
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
            quote: None,
            pending: VecDeque::new(),
            fte: None,
        }
    }

    /// A list-form `progs.src` whose text is `text`, as [`progs_text`] gives it.
    fn list(path: PathBuf, text: Vec<u8>) -> Self {
        let base = Base::Progs { output_seen: false };
        Self {
            text,
            pos: 0,
            ..Self::new(path, Vec::new(), None, base)
        }
    }

    /// A QuakeC file of a vanilla module, of `bytes`: its text ends at its first NUL byte, as
    /// id's `LoadFile` and the lexer end it (QC-1.1, PRJ-5.1).
    fn vanilla(path: PathBuf, mut bytes: Vec<u8>, entry: Option<usize>) -> Self {
        if let Some(nul) = bytes.iter().position(|&b| b == 0) {
            bytes.truncate(nul);
        }
        Self::new(path, bytes, entry, Base::Code)
    }

    /// A list-form `progs.src` of an FTEQCC or GMQCC module, of `bytes`, read as FTEQCC reads it
    /// (PRJ-4.9).
    fn fteqcc_list(path: PathBuf, bytes: Vec<u8>) -> Self {
        let mut cursor = Self::new(path, Vec::new(), None, Base::Progs { output_seen: true });
        cursor.fte = Some(FteFileReader::progs(bytes));
        cursor
    }

    /// A QuakeC file of an FTEQCC or GMQCC module, read as FTEQCC reads it from `start`
    /// (PRJ-5.13, PRJ-5.15).
    fn fteqcc(path: PathBuf, text: Vec<u8>, entry: Option<usize>, start: FileStart) -> Self {
        let mut cursor = Self::new(path, Vec::new(), entry, Base::Code);
        cursor.fte = Some(FteFileReader::new(text, start));
        cursor
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
    /// The macros, and under FTEQCC and GMQCC its count of open sides (PRJ-5.4, PRJ-5.13,
    /// PRJ-5.15).
    state: ModuleState,
    include_dirs: Vec<PathBuf>,
    once: HashSet<PathBuf>,
    stack: Vec<Cursor>,
    graph: FileGraph,
    /// The module's dialect, which decides how an `#if` reads (PRJ-5.5, FTEF-14.33).
    dialect: Dialect,
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
            if let Some(reader) = &mut cursor.fte {
                let event = reader.next(&mut self.state);
                self.fte_event(event);
                continue;
            }
            let Some((mut text, number)) = cursor.next_line() else {
                let Some(done) = self.stack.pop() else {
                    continue;
                };
                // QC-23.9: id's `COM_Parse` reads on past the end of the text for the closing
                // quote; this profile ends the list with an error.
                if let Some((_, opened)) = done.quote {
                    let message = "EOF inside quote".to_owned();
                    self.report(Code::E0101, message, &done.path, opened);
                }
                if let Some(opened) = done.include_list {
                    let message = "#includelist without #endlist".to_owned();
                    self.report(Code::E0406, message, &done.path, opened);
                }
                continue;
            };
            let in_quote = cursor.quote.is_some();
            let is_directive = !cursor.in_comment && !in_quote && directive(&text).is_some();
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
                    let (bytes, opened) = cursor.quote.take().unzip();
                    let (words, c, still) = com_parse_words(&text, cursor.in_comment, bytes);
                    cursor.quote = still.map(|bytes| (bytes, opened.unwrap_or(number)));
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

    /// Acts on what the FTEQCC reader of the innermost file met, or ends the file (PRJ-5.13).
    fn fte_event(&mut self, event: Option<FteEvent>) {
        let Some(event) = event else {
            self.stack.pop();
            return;
        };
        match event {
            FteEvent::Include { name, line } => {
                self.bring_in(&lossy(Some(&name)), line, Via::Include)
            }
            FteEvent::ListEntry { name, line } => {
                self.bring_in(&lossy(Some(&name)), line, Via::IncludeList);
            }
            FteEvent::Output { name, .. } => {
                if let Some(cursor) = self.stack.last() {
                    let path = dir_of(&cursor.path).join(lossy(Some(&name)));
                    self.graph.declared_output = Some(normalize(&path));
                }
            }
            FteEvent::Listed { name, line } => {
                self.bring_in(&lossy(Some(&name)), line, Via::Listed)
            }
            FteEvent::Error {
                code,
                message,
                line,
            } => {
                if let Some(file) = self.stack.last().map(|cursor| cursor.path.clone()) {
                    self.report(code, message.to_owned(), &file, line);
                }
            }
            FteEvent::Pragma { name, text, .. } => self.pragma(&lossy(Some(&name)), &text),
            FteEvent::UnclosedList { line } => {
                if let Some(cursor) = self.stack.last() {
                    let file = cursor.path.clone();
                    let message = "#includelist without #endlist".to_owned();
                    self.report(Code::E0406, message, &file, line);
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
                let defined = first_name(operand).is_some_and(|n| self.state.is_defined(&n));
                let taking = parent && defined == (name == "ifdef");
                cursor.conds.push(Cond {
                    taking,
                    parent,
                    done: taking,
                });
            }
            "if" => {
                let taking = parent
                    && cond::eval(
                        &lossy(Some(operand)),
                        &|n| self.state.value(n).map(str::to_owned),
                        self.dialect,
                    );
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
                            "elif" => cond::eval(
                                &lossy(Some(operand)),
                                &|n| self.state.value(n).map(str::to_owned),
                                self.dialect,
                            ),
                            _ => {
                                let defined =
                                    first_name(operand).is_some_and(|n| self.state.is_defined(&n));
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
                    self.state
                        .define(macro_name, lossy(Some(body.trim_ascii())));
                }
            }
            "undef" => {
                if let Some(macro_name) = first_name(operand) {
                    self.state.undefine(&macro_name);
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
            "pragma" => {
                let operand = operand.trim_ascii_start();
                let len = operand
                    .iter()
                    .take_while(|b| !matches!(b, b' ' | b'\t' | b'('))
                    .count();
                let name = lossy(operand.get(..len));
                self.pragma(&name, operand.get(len..).unwrap_or_default());
            }
            _ => {}
        }
    }

    /// The pragmas of PRJ-5.6 to PRJ-5.9: the pragma `name`, with the text `args` after it.
    fn pragma(&mut self, name: &str, args: &[u8]) {
        let Some(cursor) = self.stack.last() else {
            return;
        };
        let pragma = name.to_ascii_lowercase();
        let (args, _, _) = com_parse_words(args, false, None);
        // FTEF-14.65: FTEQCC joins an include directory to the file being compiled and marks
        // that file with `#pragma once`; GMQCC's reading uses the file holding it (PRJ-5.12).
        let marked = if self.dialect == Dialect::Fteqcc {
            &cursor.unit
        } else {
            &cursor.path
        };
        let here = dir_of(marked);
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
                let path = marked.clone();
                self.once.insert(path);
            }
            // FTEF-12.10: `autoprototype = *msg?!!atoi(msg):true;`. FTEQCC reads a list-form
            // `progs.src` with its preprocessor, so the pragma there holds for the module.
            "autoproto" if matches!(cursor.base, Base::Progs { .. }) => {
                let on = args
                    .first()
                    .is_none_or(|n| cond::fte_atoi(n.as_bytes()) != 0);
                self.graph.autoproto = Some(on);
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

    /// Finds `word` (PRJ-5.2) and starts reading it. Under FTEQCC an `#include` or an
    /// `#includelist` entry is found beside the file being compiled, not the file it is in
    /// (PRJ-5.11, FTEF-14.40); under GMQCC an `#include` is found beside the file it is in and
    /// then beside the file being compiled (PRJ-5.12).
    fn bring_in(&mut self, word: &str, line: u32, via: Via) {
        let Some(cursor) = self.stack.last() else {
            return;
        };
        let from = cursor.path.clone();
        let unit = cursor.unit.clone();
        let beside: Vec<&Path> = match (self.dialect, via) {
            (Dialect::Gmqcc, Via::Include) => vec![&from, &unit],
            (Dialect::Fteqcc, Via::Include | Via::IncludeList) => vec![&unit],
            (Dialect::Vanilla, _)
            | (Dialect::Fteqcc, Via::Root | Via::Listed)
            | (Dialect::Gmqcc, Via::Root | Via::Listed | Via::IncludeList) => vec![&from],
        };
        let parent = cursor.entry;
        let relative = Path::new(word);
        let mut candidates: Vec<PathBuf> = Vec::new();
        for base in beside {
            let candidate = normalize(&dir_of(base).join(relative));
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
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
        let mut cursor = if self.dialect.has_fteqcc_extensions() {
            let start = if via == Via::Include {
                FileStart::Included
            } else {
                FileStart::Compiled
            };
            Cursor::fteqcc(path, bytes, Some(entry), start)
        } else {
            Cursor::vanilla(path, bytes, Some(entry))
        };
        // `QCC_Include(fullname, newunit)`: only a file read as a new unit is compiled.
        if via == Via::Include {
            cursor.unit = unit;
        }
        self.stack.push(cursor);
    }
}

/// The text of a module's source that its form is told from and a list is read from: under
/// the FTEQCC and GMQCC dialects the text FTEQCC's `QCC_LoadFile` gives (PRJ-4.6, FTEF-14.64)
/// past a `#!` first line (PRJ-4.10), under the vanilla dialect its bytes past a UTF-8 byte order
/// mark (PRJ-5.1) up to the NUL byte id's `COM_Parse` ends it at (PRJ-4.11).
fn progs_text(bytes: &[u8], dialect: Dialect) -> Vec<u8> {
    if dialect.has_fteqcc_extensions() {
        qcc_load_as(bytes.to_vec(), FileStart::Source).0
    } else {
        let text = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        text.get(..com_parse_end(text)).unwrap_or(text).to_vec()
    }
}

/// Where id's `COM_Parse` ends the text of a list, read token by token as it reads them, with no
/// `/* */` comments: at the first NUL byte it meets between tokens or that ends a word or a `//`
/// comment, as at the NUL `LoadFile` writes after the file's bytes (QC-23.7). A quoted token runs
/// on past a NUL in it to its closing quote, or, as [`com_parse_words`] reads a quote, to its
/// line's end. The end of `text` when it has no such NUL.
fn com_parse_end(text: &[u8]) -> usize {
    let until = |from: usize, stop: fn(u8) -> bool| {
        let rest = text.get(from..).unwrap_or_default();
        rest.iter()
            .position(|&b| stop(b))
            .map_or(text.len(), |len| from.saturating_add(len))
    };
    let mut i = 0_usize;
    while let Some(&byte) = text.get(i) {
        let next = i.saturating_add(1);
        i = match (byte, text.get(next)) {
            (0, _) => return i,
            (b'/', Some(&b'/')) => until(next, |b| b == 0 || b == b'\n'),
            (b'"', _) => until(next, |b| b == b'"').saturating_add(1),
            _ if is_whitespace(byte) || single_token(byte) => next,
            _ => until(next, |b| is_whitespace(b) || single_token(b)),
        };
    }
    text.len()
}

/// Whether the first token of `text`, split as `COM_Parse` splits (QC-23.2) with `/* */`
/// comments skipped, starts with `#` (PRJ-4.1); under the FTEQCC and GMQCC dialects the first
/// word `QCC_COM_Parse` reads (PRJ-4.9). `None` when there is no token.
fn source_form(text: &[u8], dialect: Dialect) -> Option<bool> {
    if dialect.has_fteqcc_extensions() {
        return com_parse(text, 0).map(|(word, _)| word.first() == Some(&b'#'));
    }
    let mut in_comment = false;
    let mut quote = None;
    for line in text.split(|b| *b == b'\n') {
        let (words, still, open) = com_parse_words(line, in_comment, quote);
        in_comment = still;
        quote = open;
        if let Some(first) = words.first() {
            return Some(first.starts_with('#'));
        }
    }
    None
}

/// Builds the file graph of the module whose source is `src` (PRJ-4 to PRJ-6).
///
/// `defines` are defined before the first file (PRJ-5.4) and `include_dirs` are searched
/// after the including file's directory (PRJ-5.2); an `#if` is read as `dialect` reads it
/// (PRJ-5.5, FTEF-14.33). Implements PRJ-4, PRJ-5 and PRJ-6; the list form follows QC-23.
pub fn build_graph<L: Loader>(
    loader: &mut L,
    src: &Path,
    defines: &[Define],
    include_dirs: &[PathBuf],
    dialect: Dialect,
) -> FileGraph {
    let src = normalize(src);
    let mut builder = Builder {
        loader,
        root_dir: dir_of(&src),
        state: {
            let defines = defines.iter().map(|d| (d.name.clone(), d.value.clone()));
            if dialect == Dialect::Fteqcc {
                ModuleState::fteqcc(defines)
            } else {
                ModuleState::new(defines)
            }
        },
        include_dirs: include_dirs.iter().map(|d| normalize(d)).collect(),
        once: HashSet::new(),
        stack: Vec::new(),
        graph: FileGraph::default(),
        dialect,
    };
    let text = match builder.loader.load(&src) {
        Ok(text) => text,
        Err(err) => {
            let message = format!("Error opening {}: {err}", src.display());
            builder.report(Code::E0402, message, &src, 0);
            return builder.graph;
        }
    };
    let listed = progs_text(&text, dialect);
    let fteqcc = dialect.has_fteqcc_extensions();
    match source_form(&listed, dialect) {
        Some(true) => {
            builder.graph.form = Some(Form::Source);
            builder.graph.files.push(GraphFile {
                path: src.clone(),
                via: Via::Root,
                parent: None,
            });
            let cursor = if fteqcc {
                Cursor::fteqcc(src, text, Some(0), FileStart::Source)
            } else {
                Cursor::vanilla(src, text, Some(0))
            };
            builder.stack.push(cursor);
        }
        // PRJ-4.9: FTEQCC reads a list with no word, an empty output name and no file, as it
        // reads any other list.
        Some(false) | None if fteqcc => {
            builder.graph.form = Some(Form::List);
            builder.stack.push(Cursor::fteqcc_list(src, text));
        }
        Some(false) => {
            builder.graph.form = Some(Form::List);
            builder.stack.push(Cursor::list(src, listed));
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
