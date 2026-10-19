// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask engines [--check]`: generates the engine-target database tables of
//! `qc-engines` (P2-04, LANG-6) from the fact sources pinned in
//! `crates/qc-engines/sources.toml`.
//!
//! Each source file is fetched at its pinned commit and checked, with its repository's license
//! file, against the recorded SHA-256, then read once for each module kind it has a table
//! for, with that table's macros defined (ENGT-2.3, ENGT-2.8). Only facts are extracted
//! (ENGT-2): each builtin's
//! number or engine name, its QuakeC name and its signature, and extension names. No comment
//! text and no code is copied: comments are read only to find an extension name that stands
//! alone on a line or follows `Part of`, and that name is all that is kept. The tables are
//! written to `crates/qc-engines/src/tables.rs`, formatted with `rustfmt`; `--check` writes
//! nothing and fails when the checked-in file differs, which is the regeneration check CI
//! runs. The extraction rules are `docs/spec/engine-targets.md`, ENGT-2 to ENGT-4.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::corpus::manifest::Manifest;
use crate::download::{CURL, Net};
use crate::oracles::sha256::hex_digest;

/// The pinned sources, relative to the workspace root.
pub(crate) const SOURCES: &str = "crates/qc-engines/sources.toml";
/// The generated tables, relative to the workspace root.
pub(crate) const TABLES: &str = "crates/qc-engines/src/tables.rs";
/// The corpus manifest whose rerelease pin the rerelease source must match.
const MANIFEST: &str = "corpus/manifest.toml";
/// The corpus source the rerelease pin must agree with.
const RERELEASE_CORPUS_SOURCE: &str = "quake-rerelease-qc";

const USAGE: &str = "usage: cargo xtask engines [--check]";

/// The module kinds a table may be for (PRJ-3.1): server, client and menu QuakeC.
const KINDS: [&str; 3] = ["ssqc", "csqc", "menuqc"];

/// Words before a builtin's type that only mark it deprecated (ENGT-2.5); each may take a
/// parenthesised argument, which is dropped with it.
const MODIFIERS: [&str; 4] = ["DEP", "DEP_CSQC", "FTEDEP", "__deprecated"];

/// `sources.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sources {
    pub(crate) source: Vec<Pin>,
}

/// One pinned source file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pin {
    pub(crate) id: String,
    pub(crate) repo: String,
    pub(crate) commit: String,
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) license: String,
    pub(crate) license_file: String,
    pub(crate) license_sha256: String,
    pub(crate) tables: Vec<Table>,
}

/// One reading of a source file: the module kind its builtins are for and the macros defined
/// before it is read (ENGT-2.3, ENGT-2.8).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Table {
    pub(crate) kind: String,
    pub(crate) defines: Vec<String>,
}

impl Pin {
    fn url(&self, path: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{path}",
            self.repo, self.commit
        )
    }
}

/// How a progs names a builtin to the engine (ENGT-2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// `#N`, with `N` at least 1.
    Number(u32),
    /// `#0:NAME`: looked up by name.
    Name(String),
}

/// One builtin a source declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) key: Key,
    pub(crate) name: String,
    pub(crate) signature: String,
    pub(crate) extensions: Vec<String>,
}

/// The facts of one source file.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) builtins: Vec<Item>,
    pub(crate) extensions: Vec<String>,
}

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let check = match args {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err(USAGE.to_owned()),
    };
    run(root, check, &CURL)
}

fn run(root: &Path, check: bool, net: &dyn Net) -> Result<bool, String> {
    let pins = load(root)?;
    agree_with_corpus(root, &pins)?;
    let mut all = Vec::new();
    for pin in &pins {
        let text = fetch(net, pin, &pin.path, &pin.sha256)?;
        fetch(net, pin, &pin.license_file, &pin.license_sha256)?;
        let mut tables = Vec::new();
        for table in &pin.tables {
            let facts = extract(&text, &table.defines)
                .map_err(|e| format!("{} {}: {e}", pin.id, table.kind))?;
            tables.push((table.kind.clone(), facts));
        }
        all.push((pin.clone(), tables));
    }
    let tables = rustfmt(root, &render(&all))?;
    let path = root.join(TABLES);
    if check {
        let current = fs::read_to_string(&path).map_err(|e| format!("{TABLES}: {e}"))?;
        if current == tables {
            eprintln!("engines: {TABLES} matches its sources");
            return Ok(true);
        }
        eprintln!("engines: {TABLES} differs from what its sources give; run cargo xtask engines");
        return Ok(false);
    }
    fs::write(&path, tables).map_err(|e| format!("{TABLES}: {e}"))?;
    eprintln!("engines: wrote {TABLES}");
    Ok(true)
}

/// Reads `sources.toml`. Each source has at least one table, each for a module kind of
/// [`KINDS`] and no kind twice (ENGT-2.8).
pub(crate) fn load(root: &Path) -> Result<Vec<Pin>, String> {
    let text = fs::read_to_string(root.join(SOURCES)).map_err(|e| format!("{SOURCES}: {e}"))?;
    let sources: Sources = toml::from_str(&text).map_err(|e| format!("{SOURCES}: {e}"))?;
    for pin in &sources.source {
        if pin.tables.is_empty() {
            return Err(format!("{SOURCES}: {} has no table", pin.id));
        }
        let mut seen = BTreeSet::new();
        for table in &pin.tables {
            if !KINDS.contains(&table.kind.as_str()) {
                return Err(format!(
                    "{SOURCES}: {} has a table of unknown kind {:?}",
                    pin.id, table.kind
                ));
            }
            if !seen.insert(table.kind.as_str()) {
                return Err(format!(
                    "{SOURCES}: {} has two {} tables",
                    pin.id, table.kind
                ));
            }
        }
    }
    Ok(sources.source)
}

/// The rerelease pin must be the corpus manifest's (ENGT-1.3).
fn agree_with_corpus(root: &Path, pins: &[Pin]) -> Result<(), String> {
    let manifest = Manifest::load(&root.join(MANIFEST))?;
    let corpus = manifest
        .source(RERELEASE_CORPUS_SOURCE)
        .ok_or_else(|| format!("{MANIFEST} has no source {RERELEASE_CORPUS_SOURCE}"))?;
    let pin = pins
        .iter()
        .find(|p| corpus.url.ends_with(&format!("/{}", p.repo)))
        .ok_or_else(|| format!("{SOURCES} has no source from {}", corpus.url))?;
    let same = pin.commit == corpus.commit
        && corpus.checksums.get(&pin.path) == Some(&pin.sha256)
        && corpus.checksums.get(&pin.license_file) == Some(&pin.license_sha256);
    if same {
        Ok(())
    } else {
        Err(format!(
            "{SOURCES}: {} does not match {RERELEASE_CORPUS_SOURCE} in {MANIFEST}",
            pin.id
        ))
    }
}

fn fetch(net: &dyn Net, pin: &Pin, path: &str, sha256: &str) -> Result<String, String> {
    let url = pin.url(path);
    let bytes = net.get(&url)?;
    let got = hex_digest(&bytes);
    if got != sha256 {
        return Err(format!("{url}: SHA-256 is {got}, expected {sha256}"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Splits one physical line into its code and its comment text, carrying whether a `/*`
/// comment is open across lines. String literals stay in the code.
fn split_line(line: &str, in_block: &mut bool) -> (String, String) {
    let mut code = String::new();
    let mut comment = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if *in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
                code.push(' ');
            } else {
                comment.push(c);
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                chars.next();
                comment.extend(chars.by_ref());
                break;
            }
            ('/', Some('*')) => {
                chars.next();
                *in_block = true;
            }
            ('"', _) => {
                code.push(c);
                while let Some(s) = chars.next() {
                    code.push(s);
                    if s == '\\' {
                        if let Some(escaped) = chars.next() {
                            code.push(escaped);
                        }
                    } else if s == '"' {
                        break;
                    }
                }
            }
            _ => code.push(c),
        }
    }
    (code, comment)
}

/// Whether `word` has the shape of an extension name: upper-case letters, digits and
/// underscores, with an underscore after a leading letter (ENGT-2.6).
fn is_extension_name(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && word.contains('_')
        && !word.ends_with('_')
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// The names after `Part of` in a comment (ENGT-2.7).
fn part_of(comment: &str) -> Vec<String> {
    let Some((_, rest)) = comment.split_once("Part of ") else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for word in rest.split(',') {
        let word = word.trim();
        let (word, last) = match word.split_once(|c: char| c.is_whitespace() || c == '.') {
            Some((first, _)) => (first, true),
            None => (word, false),
        };
        let name_like = !word.is_empty()
            && word.chars().any(|c| c.is_ascii_uppercase())
            && word
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '?');
        if !name_like {
            break;
        }
        names.push(word.to_owned());
        if last {
            break;
        }
    }
    names
}

/// Evaluates a `#if` expression over the defined macros (ENGT-2.3).
fn eval(expr: &str, defined: &BTreeSet<String>) -> Result<bool, String> {
    let tokens = tokenize(expr)?;
    let mut p = Cond {
        tokens: &tokens,
        at: 0,
        defined,
    };
    let value = p.or()?;
    if p.at == tokens.len() {
        Ok(value)
    } else {
        Err(format!("cannot read #if {expr}"))
    }
}

fn tokenize(expr: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_ascii_alphanumeric() || c == '_' {
            let mut word = String::new();
            while let Some(&w) = chars
                .peek()
                .filter(|w| w.is_ascii_alphanumeric() || **w == '_')
            {
                word.push(w);
                chars.next();
            }
            tokens.push(word);
        } else if c == '&' || c == '|' {
            chars.next();
            if chars.next() != Some(c) {
                return Err(format!("cannot read #if {expr}"));
            }
            tokens.push(format!("{c}{c}"));
        } else if matches!(c, '!' | '(' | ')') {
            chars.next();
            tokens.push(c.to_string());
        } else {
            return Err(format!("cannot read #if {expr}"));
        }
    }
    Ok(tokens)
}

struct Cond<'a> {
    tokens: &'a [String],
    at: usize,
    defined: &'a BTreeSet<String>,
}

impl Cond<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }

    fn take(&mut self) -> Result<&str, String> {
        let token = self
            .tokens
            .get(self.at)
            .map(String::as_str)
            .ok_or("#if ends early")?;
        self.at = self.at.saturating_add(1);
        Ok(token)
    }

    fn expect(&mut self, want: &str) -> Result<(), String> {
        let got = self.take()?;
        if got == want {
            Ok(())
        } else {
            Err(format!("#if: expected {want}, found {got}"))
        }
    }

    fn or(&mut self) -> Result<bool, String> {
        let mut value = self.and()?;
        while self.peek() == Some("||") {
            self.at = self.at.saturating_add(1);
            value |= self.and()?;
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<bool, String> {
        let mut value = self.unary()?;
        while self.peek() == Some("&&") {
            self.at = self.at.saturating_add(1);
            value &= self.unary()?;
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<bool, String> {
        match self.take()? {
            "!" => Ok(!self.unary()?),
            "(" => {
                let value = self.or()?;
                self.expect(")")?;
                Ok(value)
            }
            "defined" => {
                let paren = self.peek() == Some("(");
                if paren {
                    self.at = self.at.saturating_add(1);
                }
                let name = self.take()?.to_owned();
                if paren {
                    self.expect(")")?;
                }
                Ok(self.defined.contains(&name))
            }
            word if word.chars().all(|c| c.is_ascii_digit()) => Ok(word != "0"),
            word => Err(format!("#if: cannot read {word}")),
        }
    }
}

/// One open conditional block.
struct Block {
    /// Whether the lines around the block are read.
    outer: bool,
    /// Whether the current branch is read.
    active: bool,
}

/// Extracts the facts of one source file, read with `defines` defined (ENGT-2).
pub(crate) fn extract(text: &str, defines: &[String]) -> Result<Facts, String> {
    let mut defined: BTreeSet<String> = defines.iter().cloned().collect();
    let mut facts = Facts::default();
    let mut blocks: Vec<Block> = Vec::new();
    let mut in_block = false;
    let mut pending = String::new();
    let mut heading: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        let (code, comment) = split_line(line, &mut in_block);
        let active = blocks.last().is_none_or(|b| b.active);
        let trimmed = code.trim();
        if let Some(directive) = trimmed.strip_prefix('#') {
            directive_line(directive, &mut blocks, active, &mut defined, &mut facts)
                .map_err(|e| format!("line {number}: {e}"))?;
            continue;
        }
        if !active {
            continue;
        }
        let note = comment.trim();
        if trimmed.is_empty() && pending.trim().is_empty() && is_extension_name(note) {
            if !facts.extensions.iter().any(|e| e == note) {
                facts.extensions.push(note.to_owned());
            }
            heading = Some(note.to_owned());
            continue;
        }
        pending.push(' ');
        pending.push_str(&code);
        while let Some((statement, rest)) = pending.split_once(';') {
            let statement = statement.to_owned();
            pending = rest.to_owned();
            if let Some(item) = builtin(&statement, &comment, heading.as_ref())
                .map_err(|e| format!("line {number}: {e}"))?
                && !facts.builtins.iter().any(|b| b.key == item.key)
            {
                facts.builtins.push(item);
            }
        }
    }
    if !blocks.is_empty() {
        return Err("a conditional block has no #endif".to_owned());
    }
    Ok(facts)
}

fn directive_line(
    directive: &str,
    blocks: &mut Vec<Block>,
    active: bool,
    defined: &mut BTreeSet<String>,
    facts: &mut Facts,
) -> Result<(), String> {
    let directive = directive.trim();
    let (word, rest) = directive
        .split_once(char::is_whitespace)
        .unwrap_or((directive, ""));
    let rest = rest.trim();
    match word {
        "ifdef" | "ifndef" | "if" => {
            let value = match word {
                "ifdef" => defined.contains(rest),
                "ifndef" => !defined.contains(rest),
                _ => active && eval(rest, defined)?,
            };
            blocks.push(Block {
                outer: active,
                active: active && value,
            });
        }
        "else" => {
            let block = blocks.last_mut().ok_or("#else without #if")?;
            block.active = block.outer && !block.active;
        }
        "endif" => {
            blocks.pop().ok_or("#endif without #if")?;
        }
        "define" if active => {
            let (name, body) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let (name, params) = name.split_once('(').unwrap_or((name, ""));
            if body.trim().is_empty()
                && params.is_empty()
                && is_extension_name(name)
                && !facts.extensions.iter().any(|e| e == name)
            {
                facts.extensions.push(name.to_owned());
            }
            defined.insert(name.to_owned());
        }
        "undef" if active => {
            defined.remove(rest);
        }
        "define" | "undef" | "pragma" => {}
        _ => return Err(format!("unknown directive #{word}")),
    }
    Ok(())
}

/// Reads one statement as a builtin declaration, `TYPE NAME = #N` or `TYPE NAME = #0:NAME`
/// (ENGT-2.4); any other statement is `None`.
fn builtin(
    statement: &str,
    comment: &str,
    heading: Option<&String>,
) -> Result<Option<Item>, String> {
    let statement = statement.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((lhs, rhs)) = statement.rsplit_once('=') else {
        return Ok(None);
    };
    let Some(target) = rhs.trim().strip_prefix('#') else {
        return Ok(None);
    };
    let (digits, alias) = match target.split_once(':') {
        Some((digits, alias)) => (digits.trim(), Some(alias.trim())),
        None => (target.trim(), None),
    };
    let number: u32 = digits
        .parse()
        .map_err(|_| format!("bad builtin number in {statement}"))?;
    let key = match (number, alias) {
        (0, Some(alias)) if is_word(alias) => Key::Name(alias.to_owned()),
        (n, None) if n > 0 => Key::Number(n),
        _ => return Err(format!("bad builtin in {statement}")),
    };
    let (name, signature) = name_and_signature(strip_modifiers(lhs.trim()))
        .ok_or_else(|| format!("cannot read the builtin {statement}"))?;
    let mut extensions = part_of(comment);
    if extensions.is_empty() {
        extensions.extend(heading.cloned());
    }
    Ok(Some(Item {
        key,
        name: name.to_owned(),
        signature: signature.to_owned(),
        extensions,
    }))
}

/// Splits a declaration into its name and its function type, in either form QuakeC compilers
/// accept: `float(string s) strlen` or `float strlen(string s)`, both giving the signature
/// `float(string s)` (ENGT-2.4).
fn name_and_signature(lhs: &str) -> Option<(String, String)> {
    let (head, name) = split_trailing_word(lhs);
    if is_word(name) {
        let signature = head.trim();
        return signature
            .ends_with(')')
            .then(|| (name.to_owned(), signature.to_owned()))
            .filter(|(_, s)| !s.contains('\u{fffd}'));
    }
    let params = lhs.strip_suffix(')')?;
    let mut depth = 1_usize;
    let mut open = None;
    for (i, c) in params.char_indices().rev() {
        match c {
            ')' => depth = depth.saturating_add(1),
            '(' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let (before, group) = lhs.split_at(open?);
    let (ret, name) = split_trailing_word(before.trim_end());
    let ret = ret.trim();
    if !is_word(name) || ret.is_empty() || group.contains('\u{fffd}') || ret.contains('\u{fffd}') {
        return None;
    }
    Some((name.to_owned(), format!("{ret}{group}")))
}

/// `text` split before its trailing identifier characters.
fn split_trailing_word(text: &str) -> (&str, &str) {
    let split = text
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(0, |(i, c)| i.saturating_add(c.len_utf8()));
    text.split_at(split)
}

fn is_word(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Drops the deprecation markers before a builtin's type (ENGT-2.5).
fn strip_modifiers(mut lhs: &str) -> &str {
    loop {
        let word_end = lhs
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(lhs.len());
        let (word, rest) = lhs.split_at(word_end);
        if !MODIFIERS.contains(&word) {
            return lhs;
        }
        let rest = rest.trim_start();
        lhs = match rest.strip_prefix('(') {
            Some(inner) => skip_group(inner).trim_start(),
            None => rest,
        };
    }
}

/// The text after the `)` that closes a group whose `(` has been read, skipping strings.
fn skip_group(text: &str) -> &str {
    let mut depth = 1_usize;
    let mut in_string = false;
    for (i, c) in text.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '(' if !in_string => depth = depth.saturating_add(1),
            ')' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text.get(i.saturating_add(1)..).unwrap_or("");
                }
            }
            _ => {}
        }
    }
    ""
}

/// The Rust constant each source's tables are written to.
fn const_name(id: &str) -> String {
    id.to_ascii_uppercase().replace('-', "_")
}

/// Writes the tables as Rust source (before `rustfmt`): for each source, one builtin table
/// and one extension list per module kind it is read for.
pub(crate) fn render(all: &[(Pin, Vec<(String, Facts)>)]) -> String {
    let mut out = String::new();
    out.push_str(
        "// SPDX-License-Identifier: GPL-3.0-or-later\n\n\
         //! The engine-target database tables, generated by `cargo xtask engines` from the fact\n\
         //! sources pinned in `crates/qc-engines/sources.toml`; do not edit. Only facts are\n\
         //! extracted: builtin numbers, engine names, QuakeC names and signatures, and extension\n\
         //! names (ENGT-2). ADR-0014 records the legal basis.\n\n\
         use crate::{Builtin, BuiltinKey, Source};\n\n",
    );
    out.push_str("/// The pinned sources, with the SHA-256 of each file (ENGT-1).\n");
    out.push_str("pub const SOURCES: &[Source] = &[\n");
    for (pin, _) in all {
        out.push_str(&format!(
            "Source {{ id: {:?}, repo: {:?}, commit: {:?}, path: {:?}, sha256: {:?}, \
             license: {:?}, license_file: {:?}, license_sha256: {:?} }},\n",
            pin.id,
            pin.repo,
            pin.commit,
            pin.path,
            pin.sha256,
            pin.license,
            pin.license_file,
            pin.license_sha256
        ));
    }
    out.push_str("];\n\n");
    for (pin, tables) in all {
        for (kind, facts) in tables {
            render_table(&mut out, pin, kind, facts);
        }
    }
    out
}

/// Writes one table of [`render`].
fn render_table(out: &mut String, pin: &Pin, kind: &str, facts: &Facts) {
    let name = format!("{}_{}", const_name(&pin.id), const_name(kind));
    out.push_str(&format!(
        "/// The {kind} builtins of `{}`, `{}` (ENGT-2), in file order.\n\
         pub(crate) const {name}_BUILTINS: &[Builtin] = &[\n",
        pin.id, pin.path
    ));
    for item in &facts.builtins {
        let key = match &item.key {
            Key::Number(n) => format!("BuiltinKey::Number({n})"),
            Key::Name(alias) => format!("BuiltinKey::Name({alias:?})"),
        };
        out.push_str(&format!(
            "Builtin {{ key: {key}, name: {:?}, signature: {:?}, extensions: &{:?} }},\n",
            item.name, item.signature, item.extensions
        ));
    }
    out.push_str(&format!(
        "];\n\n/// The {kind} extension names of `{}` (ENGT-2.6), in file order.\n\
         pub(crate) const {name}_EXTENSIONS: &[&str] = &{:?};\n\n",
        pin.id, facts.extensions
    ));
}

/// Formats `source` with the workspace's `rustfmt` settings.
fn rustfmt(root: &Path, source: &str) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt has no stdin")?
        .write_all(source.as_bytes())
        .map_err(|e| format!("rustfmt: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("rustfmt: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("rustfmt: {e}"))
}

#[cfg(test)]
mod tests;
