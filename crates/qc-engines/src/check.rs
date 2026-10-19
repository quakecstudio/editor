// SPDX-License-Identifier: GPL-3.0-or-later

//! The missing-builtin diagnostic: a call of a builtin the target engine lacks (ENGT-4).
//!
//! Implements ENGT-4 and reports QCD-7.17 (`W0311`). A module's calls are found in its syntax
//! trees and resolved through the semantic model's references (QC-12.5), so a call reaches the
//! diagnostic only when its callee name means a global builtin function. A builtin is keyed by
//! its number (ENGT-4.3) or, under an FTEQCC dialect, by the engine name of `#0:NAME`
//! (ENGT-4.5).

use std::collections::{HashMap, HashSet};

use qc_diag::{Code, Diagnostic, Span};
use qc_lexer::{Dialect, TokenKind};
use qc_project::ModuleKind;
use qc_sema::{DefKind, FileId, ModuleIndex};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::{Parse, SyntaxKind};

use crate::{Builtin, BuiltinKey, Engine};

/// How a module's source names a builtin: its number, or the engine name of `#0:NAME`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Number(u32),
    Name(String),
}

impl Key {
    /// The engine's entry for this key in its table for `kind`, if it has one.
    fn lookup(&self, engine: Engine, kind: ModuleKind) -> Option<Builtin> {
        match self {
            Self::Number(number) => engine.builtin(kind, BuiltinKey::Number(*number)),
            Self::Name(name) => engine
                .builtins(kind)
                .into_iter()
                .find(|b| matches!(b.key, BuiltinKey::Name(n) if n == name)),
        }
    }
}

/// A diagnostic and the file of the module it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file's index in the module's file graph, as in [`qc_sema::Location`].
    pub file: FileId,
    /// The diagnostic; its span is in that file.
    pub diagnostic: Diagnostic,
}

/// The `W0311` warnings for a module of `kind` checked against `engine` (ENGT-4.1, ENGT-4.8).
///
/// `parses` are the module's files in compilation order, `parses[i]` being [`FileId`]`(i)`,
/// and `index` is the module resolved from them ([`qc_sema::resolve_module`]). Returns one
/// warning per call of a builtin the engine's table for `kind` lacks, in file and then source
/// order; none when the engine has no table for `kind` ([`Engine::has_table`]).
#[must_use]
pub fn missing_builtins(
    engine: Engine,
    kind: ModuleKind,
    parses: &[Parse],
    index: &ModuleIndex,
) -> Vec<FileDiagnostic> {
    if !engine.has_table(kind) {
        return Vec::new();
    }
    let keys = builtin_keys(parses);
    let callees = callee_names(parses);
    let mut out = Vec::new();
    for reference in &index.refs {
        let location = reference.location;
        if !callees.contains(&(location.file, location.span)) {
            continue;
        }
        let Some(def) = index.def(reference.def) else {
            continue;
        };
        if def.kind != DefKind::Function {
            continue;
        }
        let Some(key) = keys.get(def.name.as_str()) else {
            continue;
        };
        if key.lookup(engine, kind).is_some() {
            continue;
        }
        out.push(FileDiagnostic {
            file: location.file,
            diagnostic: Diagnostic {
                code: Code::W0311,
                message: message(&def.name, key, engine, kind),
                span: location.span,
            },
        });
    }
    out.sort_by_key(|d| (d.file, d.diagnostic.span));
    out
}

/// `NAME is builtin #N, which ENGINE does not have` (ENGT-4.1), or `#0:ENGINE_NAME` for a
/// builtin looked up by name (ENGT-4.5), with the extensions of the first engine whose table
/// for `kind` lists the key with some (ENGT-4.8).
fn message(name: &str, key: &Key, engine: Engine, kind: ModuleKind) -> String {
    let shown = match key {
        Key::Number(number) => number.to_string(),
        Key::Name(engine_name) => format!("0:{engine_name}"),
    };
    let mut message = format!(
        "{name} is builtin #{shown}, which {} does not have",
        engine.name()
    );
    let extensions = Engine::ALL.into_iter().find_map(|other| {
        key.lookup(other, kind)
            .map(|b| b.extensions)
            .filter(|e| !e.is_empty())
    });
    if let Some(extensions) = extensions {
        message.push_str(" (");
        message.push_str(&extensions.join(", "));
        message.push(')');
    }
    message
}

/// Each global name's builtin key: the number of its first `#N` initialiser in compilation
/// order (ENGT-4.3), or for a name with none, the engine name of its first `#0:NAME`
/// initialiser (ENGT-4.5).
fn builtin_keys(parses: &[Parse]) -> HashMap<String, Key> {
    let mut numbers: HashMap<String, u32> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    for parse in parses {
        let Some(tree) = parse.tree() else {
            continue;
        };
        for decl in tree.declarations() {
            for declarator in decl.declarators() {
                let Some(name) = declarator.name() else {
                    continue;
                };
                let Some(ast::Initializer::Builtin(builtin)) = declarator.initializer() else {
                    continue;
                };
                let Some(literal) = builtin.number().map(|t| t.text_lossless()) else {
                    continue;
                };
                if let Some(number) = builtin_number(&literal) {
                    numbers.entry(name.text_lossless()).or_insert(number);
                } else if let Some(engine_name) = engine_name(&builtin, &literal) {
                    names.entry(name.text_lossless()).or_insert(engine_name);
                }
            }
        }
    }
    let mut keys: HashMap<String, Key> = names
        .into_iter()
        .map(|(name, engine_name)| (name, Key::Name(engine_name)))
        .collect();
    for (name, number) in numbers {
        keys.insert(name, Key::Number(number));
    }
    keys
}

/// The engine name of a `#0:NAME` initialiser whose number literal is `literal` (FTE-17.4,
/// ENGT-4.5): the name after the `:` when the literal is zero as FTEQCC reads it, any number
/// form whose value is 0 (`0`, `00`, `.0`, `0x0`, `0i`).
fn engine_name(builtin: &ast::Builtin, literal: &str) -> Option<String> {
    // ENGT-4.5: `QCC_PR_LexNumber` reads the literal and `#` takes it as `binum` 0 whenever
    // its value is 0; a name is in the initialiser only under FTEQCC, so it is lexed so.
    let lexed = qc_lexer::lex_with(literal.as_bytes(), Dialect::Fteqcc);
    let zero = matches!(lexed.tokens.as_slice(), [token] if token.kind == TokenKind::Float(0.0));
    if !zero {
        return None;
    }
    // FTE-17.4: the parser reads a name in a builtin initialiser only after its `:`.
    builtin
        .syntax()
        .child_tokens()
        .find(|t| t.kind() == SyntaxKind::Name)
        .map(|t| t.text_lossless())
}

/// The value of a builtin number literal: decimal digits, optionally followed by a fraction
/// of zeros, of at least 1 (ENGT-4.3).
pub(crate) fn builtin_number(text: &str) -> Option<u32> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.bytes().any(|b| b != b'0')
    {
        return None;
    }
    whole.parse().ok().filter(|&n| n >= 1)
}

/// The file and span of every name that is the callee of a call (ENGT-4.2).
fn callee_names(parses: &[Parse]) -> HashSet<(FileId, Span)> {
    let mut callees = HashSet::new();
    for (index, parse) in parses.iter().enumerate() {
        let Some(tree) = parse.tree() else {
            continue;
        };
        for node in tree.syntax().descendants() {
            let Some(call) = ast::CallExpr::cast(node) else {
                continue;
            };
            if let Some(ast::Expr::Name(name)) = call.callee()
                && let Some(token) = name.name()
            {
                callees.insert((FileId(index), token.span()));
            }
        }
    }
    callees
}
