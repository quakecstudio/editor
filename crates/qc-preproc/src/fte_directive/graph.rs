// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_Precompiler, QCC_PR_NewLine, QCC_PR_LexWhitespace

//! The project graph's reading of a file of an FTEQCC module (`docs/spec/project-file.md`
//! PRJ-5.13, PRJ-5.14): the directives that decide which files are read, run by the reader the
//! preprocessor runs them with (FTEF-14.33 to FTEF-14.47, FTEF-14.52), and the code between
//! them read as the preprocessor reads it, a token of the lexer at a time, so that a directive is
//! met where the preprocessor meets one. The graph opens the files; [`FteFileReader::next`] says
//! what it met.

use std::collections::HashMap;

use qc_lexer::{Dialect, Punct, TokenKind};

use super::{
    Definer, Directive, Earlier, ElifTest, Gap, IfTest, Junk, Skipped, TextStart, byte_at,
    condition, elif_defined, lossy, read, simple_get_token, skip_side, skip_to_end_of_line,
    starts_line, whitespace,
};
use crate::fte_macro::{Reading, read_define};
use crate::{FileId, TokenStream, Variadic};

/// The UTF-8 byte order mark, which FTEQCC's `QCC_LoadFile` skips (PRJ-5.1).
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

/// The most `#includelist`s and `#pragma DONT_COMPILE_THIS_FILE`s one inside another's reading
/// the graph reads, as the preprocessor's bound on nested directives (FTEF-14.47, PRJ-5.14).
const GRAPH_MAX_NESTED: usize = 64;

/// A macro as the graph keeps it: its body, which an `#if` reads, and its parameters, which
/// decide whether an `#append` adds to it (FTEF-14.39).
#[derive(Clone, Debug, PartialEq, Eq)]
struct GraphMacro {
    value: String,
    params: Option<Vec<Vec<u8>>>,
    variadic: Variadic,
}

/// What a module keeps from file to file while the project graph reads it (PRJ-5.4,
/// PRJ-5.13): its macros, and FTEQCC's count of open sides that are taken (FTEF-14.37).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleState {
    macros: HashMap<String, GraphMacro>,
    ifs: usize,
}

impl ModuleState {
    /// A module whose macros are `defines`, each a name and a body (PRJ-5.4).
    pub fn new(defines: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut state = Self::default();
        for (name, value) in defines {
            state.define(name, value);
        }
        state
    }

    /// An FTEQCC module: `FTEQCC` and `__FTEQCC__` defined with empty bodies, as
    /// `QCC_SetDefaultProperties` defines them (FTEF-14.48), then `defines`.
    pub fn fteqcc(defines: impl IntoIterator<Item = (String, String)>) -> Self {
        let own = ["FTEQCC", "__FTEQCC__"].map(|name| (name.to_owned(), String::new()));
        Self::new(own.into_iter().chain(defines))
    }

    /// Whether `name` is a macro.
    #[must_use]
    pub fn is_defined(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    /// The body of the macro `name`, as an `#if` reads it.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.macros.get(name).map(|m| m.value.as_str())
    }

    /// Defines the object-like macro `name` as `value`, replacing any macro of that name.
    pub fn define(&mut self, name: String, value: String) {
        let mac = GraphMacro {
            value,
            params: None,
            variadic: Variadic::No,
        };
        self.macros.insert(name, mac);
    }

    /// Removes the macro `name`, if there is one.
    pub fn undefine(&mut self, name: &str) {
        self.macros.remove(name);
    }
}

/// How a file the graph reads starts, which decides whether a `#` on its first line is a
/// directive (FTEF-14.52).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStart {
    /// A compiled file, the module's source or an entry of a list: `QCC_PR_CompileFile` calls
    /// `QCC_PR_NewLine` at its start, which skips spaces and tabs.
    Compiled,
    /// An `#include`d file: `QCC_PR_Precompiler` is called at its first byte.
    Included,
}

/// What the reader met that the graph acts on (PRJ-5.13), each with the 1-based line it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FteEvent {
    /// An `#include` of a file named by these bytes, which is read before the rest of this one
    /// (FTEF-14.40).
    Include { name: Vec<u8>, line: u32 },
    /// A word of an `#includelist`, a file compiled before the rest of the list is read
    /// (FTEF-14.41, PRJ-5.3).
    ListEntry { name: Vec<u8>, line: u32 },
    /// `#output` and the bytes of the string after it (PRJ-5.6).
    Output { name: Vec<u8>, line: u32 },
    /// A `#pragma`, its name and its text (FTEF-14.43), but `DONT_COMPILE_THIS_FILE`, which the
    /// reader runs itself (FTEF-14.44).
    Pragma {
        name: Vec<u8>,
        text: Vec<u8>,
        line: u32,
    },
    /// The text ended inside the `#includelist` on this line: "eof in includelist" (FTEF-14.41).
    UnclosedList { line: u32 },
}

/// What the words of the text are, inside the reading a directive started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The words of the `#includelist` on this line.
    List { line: u32 },
    /// The text `#pragma DONT_COMPILE_THIS_FILE` passes over, a byte at a time.
    DontCompile,
}

/// The 1-based line of an offset of a text, counted on from the last one asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Lines {
    at: usize,
    line: u32,
}

impl Lines {
    /// The 1-based line of `offset` in `text`: one more than the line feeds before it. The
    /// reader asks for the lines of its directives and words in the order it meets them, so
    /// `offset` is never before the one asked for last.
    fn line(&mut self, text: &[u8], offset: usize) -> u32 {
        let between = text.get(self.at..offset).unwrap_or_default();
        let feeds = between.iter().filter(|&&b| b == b'\n').count();
        self.line = self
            .line
            .saturating_add(u32::try_from(feeds).unwrap_or(u32::MAX));
        self.at = offset;
        self.line.saturating_add(1)
    }
}

/// One file of an FTEQCC module, read as FTEQCC reads it for the project graph (PRJ-5.13).
#[derive(Debug)]
pub struct FteFileReader {
    text: Vec<u8>,
    /// The file's tokens, as the preprocessor lexes them, which the code is read by.
    stream: TokenStream,
    /// Where reading goes on.
    at: usize,
    /// Where the code is read from again, as the preprocessor reads on after a directive, when
    /// reading moved by other than a token: the tokens are lexed again from there.
    resume: Option<usize>,
    /// How the file starts, which decides whether a `#` before its first line break is a
    /// directive's (FTEF-14.52).
    start: TextStart,
    /// The readings directives started that are open, innermost last.
    modes: Vec<Mode>,
    /// Whether the text has ended: at its end, or at a directive that ends it.
    ended: bool,
    lines: Lines,
}

impl FteFileReader {
    /// A reader of `text`, a file that starts as `start` says; a UTF-8 byte order mark at its
    /// start is skipped (PRJ-5.1).
    #[must_use]
    pub fn new(text: Vec<u8>, start: FileStart) -> Self {
        let text = match text.strip_prefix(UTF8_BOM) {
            Some(rest) => rest.to_vec(),
            None => text,
        };
        Self {
            stream: TokenStream::lex(&text, Dialect::Fteqcc),
            text,
            at: 0,
            resume: None,
            start: match start {
                FileStart::Compiled => TextStart::Compiled,
                FileStart::Included => TextStart::Included,
            },
            modes: Vec::new(),
            ended: false,
            lines: Lines::default(),
        }
    }

    /// Reads on to the next thing the graph acts on, with the module's `state`, which the
    /// directives on the way change; `None` at the end of the file. After an `Include` or a
    /// `ListEntry` the graph reads that file, with the same state, before it calls this again.
    /// Implements PRJ-5.13.
    pub fn next(&mut self, state: &mut ModuleState) -> Option<FteEvent> {
        loop {
            if self.ended {
                return self.close_innermost_list();
            }
            if self.modes.is_empty() {
                if let Some(event) = self.code(state) {
                    return Some(event);
                }
                continue;
            }
            let word = match whitespace(&self.text, self.at) {
                Gap::Text(at) => at,
                Gap::LineStart(hash) => match read(&self.text, hash) {
                    Some(directive) => {
                        if let Some(event) = self.run(state, hash, directive) {
                            return Some(event);
                        }
                        continue;
                    }
                    // FTE-23.10: no directive, so the `#` is read as code.
                    None => hash,
                },
            };
            if byte_at(&self.text, word).is_none() {
                self.ended = true;
                continue;
            }
            if let Some(event) = self.word(word) {
                return Some(event);
            }
        }
    }

    /// Reads the next token of the code as the preprocessor reads it (`Preprocessor::next_item`):
    /// a `#` that starts a line, as FTEF-14.52 and FTEF-14.53 say, starts a directive, which is
    /// run; any other token is passed over.
    fn code(&mut self, state: &mut ModuleState) -> Option<FteEvent> {
        if let Some(at) = self.resume.take() {
            self.stream.resume_at(&self.text, at, Dialect::Fteqcc);
        }
        let Some(token) = self.stream.tokens.get(self.stream.pos) else {
            self.ended = true;
            return None;
        };
        let (hash, end) = (token.span.start, token.span.end);
        let directive = (token.kind == TokenKind::Punct(Punct::Hash)
            && starts_line(&self.text, self.stream.code_start(hash), hash, self.start))
        .then(|| read(&self.text, hash))
        .flatten();
        let Some(directive) = directive else {
            self.stream.pos = self.stream.pos.saturating_add(1);
            self.at = end;
            return None;
        };
        let event = self.run(state, hash, directive);
        self.resume = Some(self.at);
        event
    }

    /// At the end of the text, drops the innermost open `#includelist` and the readings inside
    /// it: "eof in includelist" (FTEF-14.41); `None` when no list is open.
    fn close_innermost_list(&mut self) -> Option<FteEvent> {
        while let Some(mode) = self.modes.pop() {
            match mode {
                Mode::List { line } => return Some(FteEvent::UnclosedList { line }),
                Mode::DontCompile => {}
            }
        }
        None
    }

    /// Reads the word at `at`, which is in the text, as the innermost reading reads it; the
    /// code outside any reading is [`Self::code`]'s.
    fn word(&mut self, at: usize) -> Option<FteEvent> {
        match self.modes.last() {
            // FTEF-14.44: `if (!qcc_iswhite(*pr_file_p)) pr_file_p++;`.
            None | Some(Mode::DontCompile) => self.at = at.saturating_add(1),
            // FTEF-14.41: a word `QCC_PR_SimpleGetToken` reads, `#endlist` ending the list.
            Some(Mode::List { .. }) => {
                let (word, stop) = simple_get_token(&self.text, at);
                let Some(word) = word else {
                    self.at = stop.saturating_add(1);
                    return None;
                };
                let name = self.text.get(word.clone()).unwrap_or_default().to_vec();
                if name == b"#endlist" {
                    self.modes.pop();
                    self.at = skip_to_end_of_line(&self.text, stop, Junk::Checked).at;
                    self.resume = Some(self.at);
                    return None;
                }
                self.at = stop;
                if self.text.get(stop) == Some(&b'\r') {
                    self.at = stop.saturating_add(1);
                }
                let line = self.lines.line(&self.text, word.start);
                return Some(FteEvent::ListEntry { name, line });
            }
        }
        None
    }

    /// Opens the reading `mode` starts, at `at`; past the bound on nested readings the text
    /// ends instead (PRJ-5.14).
    fn open(&mut self, mode: Mode, at: usize) {
        if self.modes.len() >= GRAPH_MAX_NESTED {
            self.ended = true;
            return;
        }
        self.modes.push(mode);
        self.at = at;
    }

    /// Runs `directive`, read at the `#` at `hash`, as `QCC_PR_Precompiler` does for the graph
    /// (FTEF-14.35 to FTEF-14.44, FTEF-14.52): sets where reading goes on and gives what the graph acts on.
    fn run(
        &mut self,
        state: &mut ModuleState,
        hash: usize,
        directive: Directive,
    ) -> Option<FteEvent> {
        let line = self.lines.line(&self.text, hash);
        let text = self.text.as_slice();
        match directive {
            Directive::Hanging(end)
            | Directive::Error { end, .. }
            | Directive::Warning { end, .. }
            | Directive::Build { end }
            | Directive::Pack { end, .. } => self.at = end.at,
            Directive::Define { definer, word_end } => {
                let reading = Reading::File(FileId(0), text);
                let (read, stop) = read_define(&reading, word_end);
                if let Ok(definition) = read {
                    let name = lossy(text.get(definition.name.clone()).unwrap_or_default());
                    let value = lossy(definition.value.bytes());
                    let new = GraphMacro {
                        value,
                        params: definition.params,
                        variadic: definition.variadic,
                    };
                    let old = state.macros.get(&name);
                    let same = old.is_some_and(|old| {
                        old.params == new.params && old.variadic == new.variadic
                    });
                    // FTEF-14.39: "append with no previous define is treated as just a regular
                    // define", and one of other parameters replaces it.
                    let value = match (definer, old) {
                        (Definer::Append, Some(old)) if same => {
                            format!("{} {}", old.value, new.value).trim_end().to_owned()
                        }
                        _ => new.value.clone(),
                    };
                    state.macros.insert(name, GraphMacro { value, ..new });
                }
                self.at = skip_to_end_of_line(text, stop, Junk::Ignored).at;
            }
            Directive::Undef { name, end } => {
                state.undefine(&lossy(text.get(name).unwrap_or_default()));
                self.at = end.at;
            }
            Directive::If { test, name, end } => {
                let (value, end) = match test {
                    IfTest::Expression(from) => {
                        let macros = |name: &str| state.value(name).map(str::to_owned);
                        let (value, stop) = condition(text, from, &macros);
                        let end = skip_to_end_of_line(text, stop, Junk::Ignored).at;
                        (value.unwrap_or(false), end)
                    }
                    IfTest::Defined | IfTest::NotDefined => {
                        let name = lossy(text.get(name).unwrap_or_default());
                        (state.is_defined(&name) == (test == IfTest::Defined), end.at)
                    }
                };
                if value {
                    state.ifs = state.ifs.saturating_add(1);
                    self.at = end;
                } else {
                    self.skip(state, end, Earlier::NoSideTaken);
                }
            }
            Directive::NotIf { word_end } => self.at = word_end,
            Directive::Else { end } => {
                if state.ifs == 0 {
                    self.at = end.at;
                } else {
                    state.ifs = state.ifs.saturating_sub(1);
                    self.skip(state, end.at, Earlier::SideTaken);
                }
            }
            Directive::Endif { end } => {
                state.ifs = state.ifs.saturating_sub(1);
                self.at = end.at;
            }
            Directive::Eof | Directive::Package { .. } => self.ended = true,
            Directive::Output { name, end } => {
                self.at = end.at;
                return Some(FteEvent::Output { name: name?, line });
            }
            Directive::IncludeList { end } => self.open(Mode::List { line }, end.at),
            Directive::Include { name, end } => {
                self.at = end.at;
                // `if (!*newfile) return;`: an empty name includes nothing; in the text
                // `DONT_COMPILE_THIS_FILE` passes over, the file compiles nothing (PRJ-5.14).
                if self.modes.contains(&Mode::DontCompile) {
                    return None;
                }
                let name = name.ok().filter(|name| !name.is_empty())?;
                return Some(FteEvent::Include { name, line });
            }
            Directive::Pragma { pragma, end } => {
                if pragma.is("DONT_COMPILE_THIS_FILE") {
                    self.open(Mode::DontCompile, end);
                    return None;
                }
                self.at = end;
                return Some(FteEvent::Pragma {
                    name: pragma.name,
                    text: pragma.message,
                    line,
                });
            }
        }
        None
    }

    /// Skips a side from `from`, the end of its conditional's line, as
    /// `QCC_PR_FalsePreProcessorIf` does (FTEF-14.37), to where reading goes on.
    fn skip(&mut self, state: &mut ModuleState, from: usize, earlier: Earlier) {
        let text = self.text.as_slice();
        let skipped = skip_side(text, from, earlier, |elif| match elif.test {
            ElifTest::Expression => {
                let macros = |name: &str| state.value(name).map(str::to_owned);
                let (value, stop) = condition(text, elif.at, &macros);
                (value.unwrap_or(false), stop, Junk::Ignored)
            }
            ElifTest::Defined | ElifTest::NotDefined => {
                let (value, stop) =
                    elif_defined(text, elif.at, elif.test, |name| state.is_defined(name));
                (value, stop, Junk::Checked)
            }
        });
        match skipped {
            Skipped::Taken { end, .. } => {
                state.ifs = state.ifs.saturating_add(1);
                self.at = end.at;
            }
            Skipped::Endif(end) => self.at = end,
            Skipped::Eof => self.ended = true,
        }
    }
}
