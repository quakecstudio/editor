// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads facts from an engine's builtin tables in C: the QuakeSpasm lineage's sources
//! (ADR-0016), whose builtins are initialisers of C arrays rather than QuakeC declarations.
//!
//! Implements ENGT-2.11 to ENGT-2.17. The file is split into tokens with its comments
//! dropped unread, the named array's initialiser is found and its macros expanded, and each
//! element is read by the columns `sources.toml` gives. Only facts are kept: a builtin's
//! number or name, the QuakeC name or C function that stands for it, the type string of a
//! table that has one, and extension names. No comment and no other code is read.

use std::collections::{BTreeMap, BTreeSet};

use super::{Facts, Item, Key, Table, is_extension_name, is_word};

/// How deeply macros may expand inside one another before the reading is an error.
const MAX_EXPANSION_DEPTH: usize = 32;

/// One C token. Comments are dropped, so they are never tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    /// A number literal, as written.
    Number(String),
    /// A string literal's text between its quotes, escapes as written.
    Str(String),
    /// A character literal, as written; never a fact.
    Char,
    Punct(char),
    /// A preprocessor line: its directive word and the rest of the line, continuation lines
    /// joined.
    Directive(String, String),
}

/// A `#define`.
#[derive(Clone, Debug)]
struct Macro {
    params: Option<Vec<String>>,
    body: Vec<Tok>,
    /// Whether it is defined inside an `#if` block, which this reading does not evaluate.
    conditional: bool,
    /// Why its definition cannot be read, if it cannot; an error only when it is used.
    unreadable: Option<String>,
}

/// Splits C source into tokens (ENGT-2.11): comments are dropped, string and character
/// literals are single tokens, and a line whose first non-blank character is `#` is one
/// directive.
fn lex(text: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0_usize;
    let mut line_start = true;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at.saturating_add(1)).copied();
        if c == '\n' {
            line_start = true;
            at = at.saturating_add(1);
        } else if c.is_whitespace() {
            at = at.saturating_add(1);
        } else if c == '/' && next == Some('/') {
            while chars.get(at).is_some_and(|&c| c != '\n') {
                at = at.saturating_add(1);
            }
        } else if c == '/' && next == Some('*') {
            at = skip_block_comment(&chars, at.saturating_add(2))?;
        } else if c == '#' && line_start {
            let (directive, end) = directive(&chars, at.saturating_add(1))?;
            tokens.push(directive);
            at = end;
        } else {
            line_start = false;
            let (token, end) = token(&chars, at)?;
            tokens.push(token);
            at = end;
        }
    }
    Ok(tokens)
}

/// The index after the `*/` that closes a block comment whose body starts at `at`.
fn skip_block_comment(chars: &[char], mut at: usize) -> Result<usize, String> {
    loop {
        match (chars.get(at), chars.get(at.saturating_add(1))) {
            (Some('*'), Some('/')) => return Ok(at.saturating_add(2)),
            (Some(_), _) => at = at.saturating_add(1),
            (None, _) => return Err("a block comment has no end".to_owned()),
        }
    }
}

/// Reads a directive whose word starts at `at`, to the end of its line, joining lines that
/// end in `\` and dropping comments.
fn directive(chars: &[char], mut at: usize) -> Result<(Tok, usize), String> {
    let mut line = String::new();
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at.saturating_add(1)).copied();
        match c {
            '\n' => break,
            '\\' if next == Some('\n') => {
                line.push(' ');
                at = at.saturating_add(2);
            }
            '\\' if next == Some('\r') && chars.get(at.saturating_add(2)) == Some(&'\n') => {
                line.push(' ');
                at = at.saturating_add(3);
            }
            '/' if next == Some('/') => {
                while chars.get(at).is_some_and(|&c| c != '\n') {
                    at = at.saturating_add(1);
                }
            }
            '/' if next == Some('*') => {
                at = skip_block_comment(chars, at.saturating_add(2))?;
                line.push(' ');
            }
            '"' | '\'' => {
                let end = literal_end(chars, at, c).unwrap_or(at.saturating_add(1));
                line.extend(chars.get(at..end).unwrap_or_default());
                at = end;
            }
            _ => {
                line.push(c);
                at = at.saturating_add(1);
            }
        }
    }
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    Ok((Tok::Directive(word.to_owned(), rest.to_owned()), at))
}

/// The index after the literal opened by `quote` at `at`.
fn literal_end(chars: &[char], at: usize, quote: char) -> Result<usize, String> {
    let mut i = at.saturating_add(1);
    loop {
        match chars.get(i) {
            Some('\\') => i = i.saturating_add(2),
            Some(&c) if c == quote => return Ok(i.saturating_add(1)),
            Some('\n') | None => return Err("a literal has no closing quote".to_owned()),
            Some(_) => i = i.saturating_add(1),
        }
    }
}

/// Reads one token starting at `at`.
fn token(chars: &[char], at: usize) -> Result<(Tok, usize), String> {
    let Some(&c) = chars.get(at) else {
        return Err("a token runs past the end".to_owned());
    };
    let word = |start: usize| {
        let mut end = start;
        while chars
            .get(end)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        {
            end = end.saturating_add(1);
        }
        end
    };
    if c == '"' {
        let end = literal_end(chars, at, '"')?;
        let inner: String = chars
            .get(at.saturating_add(1)..end.saturating_sub(1))
            .unwrap_or_default()
            .iter()
            .collect();
        Ok((Tok::Str(inner), end))
    } else if c == '\'' {
        // An apostrophe with no closing quote, as in the text of an `#if 0` block, is
        // punctuation; it is never inside an array this reads.
        Ok(
            literal_end(chars, at, '\'').map_or((Tok::Punct('\''), at.saturating_add(1)), |end| {
                (Tok::Char, end)
            }),
        )
    } else if c.is_ascii_digit() {
        let end = word(at);
        let text: String = chars.get(at..end).unwrap_or_default().iter().collect();
        Ok((Tok::Number(text), end))
    } else if c.is_ascii_alphabetic() || c == '_' {
        let mut end = at;
        while chars
            .get(end)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            end = end.saturating_add(1);
        }
        let text: String = chars.get(at..end).unwrap_or_default().iter().collect();
        Ok((Tok::Ident(text), end))
    } else {
        Ok((Tok::Punct(c), at.saturating_add(1)))
    }
}

/// Reads a `#define` line's rest into its name and macro (ENGT-2.12): a `(` straight after
/// the name makes it function-like. A line with no readable name defines nothing; a macro
/// whose parameters or body cannot be read is kept as unreadable, an error only if used.
fn define(rest: &str, conditional: bool) -> Option<(String, Macro)> {
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_end);
    if !is_word(name) {
        return None;
    }
    let mut unreadable = None;
    let (params, body) = match after.strip_prefix('(') {
        Some(inner) => {
            let (list, body) = inner.split_once(')').unwrap_or((inner, ""));
            let params: Vec<String> = list
                .split(',')
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect();
            if !inner.contains(')') || params.iter().any(|p| !is_word(p)) {
                unreadable = Some(format!("cannot read #define {rest}"));
            }
            (Some(params), body)
        }
        None => (None, after),
    };
    let body = lex(body).unwrap_or_else(|e| {
        unreadable = Some(e);
        Vec::new()
    });
    if body
        .iter()
        .any(|t| matches!(t, Tok::Punct('#') | Tok::Directive(..)))
    {
        unreadable = Some(format!("#define {name} uses # or ##, which is not read"));
    }
    Some((
        name.to_owned(),
        Macro {
            params,
            body,
            conditional,
            unreadable,
        },
    ))
}

/// The macros in force, and how deeply the conditional blocks around the reader are nested.
#[derive(Default)]
struct Preprocessor {
    macros: BTreeMap<String, Macro>,
    depth: usize,
}

impl Preprocessor {
    /// Takes one directive (ENGT-2.12): `#define` and `#undef` change the macros, the
    /// conditional directives change the nesting, and outside `array` every other directive
    /// is passed over. Inside the array a conditional or any other directive is an error.
    fn directive(&mut self, word: &str, rest: &str, inside: bool) -> Result<(), String> {
        match word {
            "define" => {
                if let Some((name, mac)) = define(rest, self.depth > 0) {
                    self.macros.insert(name, mac);
                }
            }
            "undef" => {
                self.macros.remove(rest.trim());
            }
            _ if inside => return Err(format!("#{word} inside the array is not read")),
            "if" | "ifdef" | "ifndef" => self.depth = self.depth.saturating_add(1),
            "endif" => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        Ok(())
    }
}

/// Expands the macros of `tokens` (ENGT-2.12): an object-like macro is replaced by its body,
/// a function-like one followed by `(` by its body with each parameter replaced by its
/// argument, and the result is expanded again with the macro itself left alone. A macro
/// defined inside an `#if` block is an error, since the reading does not know whether that
/// block is compiled.
fn expand(
    tokens: &[Tok],
    macros: &BTreeMap<String, Macro>,
    active: &BTreeSet<String>,
    depth: usize,
) -> Result<Vec<Tok>, String> {
    if depth > MAX_EXPANSION_DEPTH {
        return Err("macros expand too deeply".to_owned());
    }
    let mut out = Vec::new();
    let mut at = 0_usize;
    while let Some(token) = tokens.get(at) {
        at = at.saturating_add(1);
        let Tok::Ident(name) = token else {
            out.push(token.clone());
            continue;
        };
        let Some(mac) = macros.get(name).filter(|_| !active.contains(name)) else {
            out.push(token.clone());
            continue;
        };
        if let Some(why) = &mac.unreadable {
            return Err(format!("{name}: {why}"));
        }
        if mac.conditional {
            return Err(format!(
                "{name} is defined inside an #if block, which is not read"
            ));
        }
        let body = match &mac.params {
            None => mac.body.clone(),
            Some(params) => {
                if tokens.get(at) != Some(&Tok::Punct('(')) {
                    out.push(token.clone());
                    continue;
                }
                let (args, end) = arguments(tokens, at.saturating_add(1))?;
                at = end;
                if args.len() != params.len() && !(params.is_empty() && args == [vec![]]) {
                    return Err(format!("{name} takes {} arguments", params.len()));
                }
                let mut body = Vec::new();
                for t in &mac.body {
                    let arg = match t {
                        Tok::Ident(p) => params.iter().position(|q| q == p),
                        _ => None,
                    };
                    match arg.and_then(|i| args.get(i)) {
                        Some(arg) => {
                            body.extend(expand(arg, macros, active, depth.saturating_add(1))?)
                        }
                        None => body.push(t.clone()),
                    }
                }
                body
            }
        };
        let mut inner = active.clone();
        inner.insert(name.clone());
        out.extend(expand(&body, macros, &inner, depth.saturating_add(1))?);
    }
    Ok(out)
}

/// The comma-separated arguments of a macro call whose `(` is just before `at`, and the index
/// after its `)`.
fn arguments(tokens: &[Tok], mut at: usize) -> Result<(Vec<Vec<Tok>>, usize), String> {
    let mut args = vec![Vec::new()];
    let mut depth = 0_usize;
    loop {
        let token = tokens
            .get(at)
            .ok_or("a macro call has no closing parenthesis")?;
        at = at.saturating_add(1);
        match token {
            Tok::Punct(')') if depth == 0 => return Ok((args, at)),
            Tok::Punct(',') if depth == 0 => args.push(Vec::new()),
            _ => {
                match token {
                    Tok::Punct('(' | '{' | '[') => depth = depth.saturating_add(1),
                    Tok::Punct(')' | '}' | ']') => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if let Some(last) = args.last_mut() {
                    last.push(token.clone());
                }
            }
        }
    }
}

/// The tokens of the initialiser of `array` (`NAME [ ] =` then a braced list), without its
/// outer braces and with its macros expanded (ENGT-2.12, ENGT-2.13).
fn initialiser(tokens: &[Tok], array: &str) -> Result<Vec<Tok>, String> {
    let mut pre = Preprocessor::default();
    let mut at = 0_usize;
    let mut start = None;
    while let Some(token) = tokens.get(at) {
        if let Tok::Directive(word, rest) = token {
            pre.directive(word, rest, false)?;
        } else if *token == Tok::Ident(array.to_owned())
            && tokens.get(at.saturating_add(1)) == Some(&Tok::Punct('['))
            && tokens.get(at.saturating_add(2)) == Some(&Tok::Punct(']'))
            && tokens.get(at.saturating_add(3)) == Some(&Tok::Punct('='))
        {
            start = Some(at.saturating_add(4));
            break;
        }
        at = at.saturating_add(1);
    }
    let mut at = start.ok_or_else(|| format!("no array {array}[] = {{...}}"))?;
    loop {
        match tokens.get(at) {
            Some(Tok::Directive(word, rest)) => pre.directive(word, rest, true)?,
            Some(Tok::Punct('{')) => break,
            _ => return Err(format!("{array}[] = is not followed by {{")),
        }
        at = at.saturating_add(1);
    }
    at = at.saturating_add(1);
    // Each run of tokens between directives is expanded with the macros in force where it
    // stands, as the C preprocessor would.
    let mut out = Vec::new();
    let mut part = Vec::new();
    let mut depth = 0_usize;
    loop {
        let token = tokens
            .get(at)
            .ok_or_else(|| format!("the initialiser of {array} has no end"))?;
        at = at.saturating_add(1);
        match token {
            Tok::Directive(word, rest) => {
                out.extend(expand(&part, &pre.macros, &BTreeSet::new(), 0)?);
                part.clear();
                pre.directive(word, rest, true)?;
                continue;
            }
            Tok::Punct('{') => depth = depth.saturating_add(1),
            Tok::Punct('}') if depth == 0 => break,
            Tok::Punct('}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        part.push(token.clone());
    }
    out.extend(expand(&part, &pre.macros, &BTreeSet::new(), 0)?);
    Ok(out)
}

/// Splits a token list at its top-level commas, dropping an empty last part (a trailing
/// comma).
fn split_commas(tokens: &[Tok]) -> Vec<Vec<Tok>> {
    let mut parts = vec![Vec::new()];
    let mut depth = 0_usize;
    for token in tokens {
        match token {
            Tok::Punct(',') if depth == 0 => {
                parts.push(Vec::new());
                continue;
            }
            Tok::Punct('(' | '{' | '[') => depth = depth.saturating_add(1),
            Tok::Punct(')' | '}' | ']') => depth = depth.saturating_sub(1),
            _ => {}
        }
        if let Some(last) = parts.last_mut() {
            last.push(token.clone());
        }
    }
    if parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

/// The fields of a braced element `{ a, b, ... }` (ENGT-2.13).
fn row(element: &[Tok]) -> Result<Vec<Vec<Tok>>, String> {
    match (element.first(), element.last(), element.len()) {
        (Some(Tok::Punct('{')), Some(Tok::Punct('}')), 2..) => Ok(split_commas(
            element
                .get(1..element.len().saturating_sub(1))
                .unwrap_or_default(),
        )),
        _ => Err(format!("an element is not a braced row: {}", show(element))),
    }
}

/// A field that is string literals, joined as C joins adjacent literals.
fn string(field: &[Tok]) -> Option<String> {
    let mut text = String::new();
    for token in field {
        let Tok::Str(s) = token else { return None };
        text.push_str(s);
    }
    (!field.is_empty()).then_some(text)
}

/// A field that is one identifier.
fn ident(field: &[Tok]) -> Option<&str> {
    match field {
        [Tok::Ident(name)] => Some(name),
        _ => None,
    }
}

/// A field that is a decimal integer, with an optional `-`.
fn integer(field: &[Tok]) -> Option<i64> {
    let (negative, digits) = match field {
        [Tok::Number(digits)] => (false, digits),
        [Tok::Punct('-'), Tok::Number(digits)] => (true, digits),
        _ => return None,
    };
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

/// Tokens as text, for error messages.
fn show(tokens: &[Tok]) -> String {
    let mut out = Vec::new();
    for token in tokens {
        out.push(match token {
            Tok::Ident(s) | Tok::Number(s) => s.clone(),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Char => "'?'".to_owned(),
            Tok::Punct(c) => c.to_string(),
            Tok::Directive(w, r) => format!("#{w} {r}"),
        });
    }
    out.join(" ")
}

/// The field at `column` of a row; `None` when the row is shorter, which C fills with zero.
fn field(fields: &[Vec<Tok>], column: Option<usize>) -> Option<&[Tok]> {
    column.and_then(|c| fields.get(c)).map(Vec::as_slice)
}

/// The QuakeC signature in a type string (ENGT-2.15): its last line, whitespace collapsed,
/// which must end in `)`.
fn signature(type_string: &str) -> Result<String, String> {
    let last = type_string.rsplit("\\n").next().unwrap_or(type_string);
    let collapsed = last.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.ends_with(')') {
        Ok(collapsed)
    } else {
        Err(format!("cannot read the type string \"{type_string}\""))
    }
}

/// The facts of a positional array (ENGT-2.14): element `i` is builtin `#i`, unless it is one
/// of the `absent` functions; element 0 is never a builtin.
fn positional(elements: &[Vec<Tok>], absent: &[String]) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let function = ident(element)
            .ok_or_else(|| format!("element {index} is not a function: {}", show(element)))?;
        if index == 0 || absent.iter().any(|a| a == function) {
            continue;
        }
        let number = u32::try_from(index).map_err(|_| format!("element {index} is too far"))?;
        items.push(Item {
            key: Key::Number(number),
            name: function.to_owned(),
            signature: String::new(),
            extensions: Vec::new(),
        });
    }
    Ok(items)
}

/// The name of a row: a string literal that is a QuakeC identifier, or `None` for a row whose
/// name is `NULL`, which ends the list (ENGT-2.15).
fn row_name(fields: &[Vec<Tok>], table: &Table, element: &[Tok]) -> Result<Option<String>, String> {
    let name = field(fields, table.name);
    if name.and_then(ident) == Some("NULL") {
        return Ok(None);
    }
    name.and_then(string)
        .filter(|n| is_word(n))
        .map(Some)
        .ok_or_else(|| format!("a row has no name: {}", show(element)))
}

/// The facts of an array of rows read by `table`'s columns (ENGT-2.15).
fn rows(elements: &[Vec<Tok>], table: &Table) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for element in elements {
        let fields = row(element)?;
        let Some(name) = row_name(&fields, table, element)? else {
            break;
        };
        if table.function.is_some() {
            let function = match field(&fields, table.function) {
                None => None,
                Some(f) => Some(
                    ident(f)
                        .ok_or_else(|| format!("{name}: cannot read its function {}", show(f)))?,
                ),
            };
            if function.is_none_or(|f| table.absent.iter().any(|a| a == f)) {
                continue;
            }
        }
        let number = match field(&fields, table.number) {
            None => 0,
            Some(f) => {
                integer(f).ok_or_else(|| format!("{name}: cannot read its number {}", show(f)))?
            }
        };
        let key = match u32::try_from(number) {
            Ok(0) => Key::Name(name.clone()),
            Ok(n) => Key::Number(n),
            Err(_) => return Err(format!("{name}: builtin number {number} is negative")),
        };
        let signature = match field(&fields, table.signature) {
            None => String::new(),
            Some(f) => signature(
                &string(f)
                    .ok_or_else(|| format!("{name}: cannot read its type string {}", show(f)))?,
            )?,
        };
        let extensions = match field(&fields, table.extension) {
            None => Vec::new(),
            Some(f) => match ident(f) {
                Some(ext) if is_extension_name(ext) => vec![ext.to_owned()],
                _ => return Err(format!("{name}: cannot read its extension {}", show(f))),
            },
        };
        items.push(Item {
            key,
            name,
            signature,
            extensions,
        });
    }
    Ok(items)
}

/// The facts of an array of name patches (ENGT-2.16): a row whose `number` column is 0
/// names a `#0` function the engine gives the builtin `#N` of its `target` column, `-N`; it is
/// that builtin of `targets` keyed by the name, and nothing when `targets` lacks `#N`.
fn aliases(elements: &[Vec<Tok>], table: &Table, targets: &Facts) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for element in elements {
        let fields = row(element)?;
        let Some(name) = row_name(&fields, table, element)? else {
            break;
        };
        let number = field(&fields, table.number)
            .and_then(integer)
            .ok_or_else(|| format!("{name}: cannot read its number"))?;
        let target = field(&fields, table.target)
            .and_then(integer)
            .and_then(|t| u32::try_from(t.saturating_neg()).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("{name}: its target is not a builtin -N"))?;
        if number != 0 {
            continue;
        }
        let Some(builtin) = targets
            .builtins
            .iter()
            .find(|b| b.key == Key::Number(target))
        else {
            continue;
        };
        items.push(Item {
            key: Key::Name(name.clone()),
            name,
            signature: builtin.signature.clone(),
            extensions: builtin.extensions.clone(),
        });
    }
    Ok(items)
}

/// The extension names of `array`, the string in the first column of each row (ENGT-2.17).
fn extension_names(tokens: &[Tok], array: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for element in split_commas(&initialiser(tokens, array)?) {
        let fields = row(&element)?;
        let name = fields
            .first()
            .and_then(|f| string(f))
            .filter(|n| is_extension_name(n))
            .ok_or_else(|| format!("{array}: cannot read an extension name: {}", show(&element)))?;
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Extracts the facts of one reading of a C source (ENGT-2.11 to ENGT-2.17); `targets` is
/// the facts of the reading `table.target_of` names, for a table of name patches.
pub(crate) fn extract(text: &str, table: &Table, targets: Option<&Facts>) -> Result<Facts, String> {
    let array = table.array.as_deref().ok_or("a C table needs an array")?;
    let tokens = lex(text)?;
    let elements = split_commas(&initialiser(&tokens, array)?);
    let items = match (table.name, table.target, targets) {
        (None, _, _) => positional(&elements, &table.absent)?,
        (Some(_), None, _) => rows(&elements, table)?,
        (Some(_), Some(_), Some(targets)) => aliases(&elements, table, targets)?,
        (Some(_), Some(_), None) => return Err("a table with a target needs target_of".to_owned()),
    };
    let mut facts = Facts::default();
    for item in items {
        if !facts.builtins.iter().any(|b| b.key == item.key) {
            facts.builtins.push(item);
        }
    }
    if let Some(list) = &table.extension_array {
        facts.extensions = extension_names(&tokens, list)?;
    }
    Ok(facts)
}

#[cfg(test)]
mod tests;
