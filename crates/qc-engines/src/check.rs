// SPDX-License-Identifier: GPL-3.0-or-later

//! The missing-builtin diagnostic: a call of a builtin the target engine lacks (ENGT-4).
//!
//! Implements ENGT-4 and reports QCD-7.17 (`W0311`). A module's calls are found in its syntax
//! trees and resolved through the semantic model's references (QC-12.5), so a call reaches the
//! diagnostic only when its callee name means a global builtin function.

use std::collections::{HashMap, HashSet};

use qc_diag::{Code, Diagnostic, Span};
use qc_sema::{DefKind, FileId, ModuleIndex};
use qc_syntax::Parse;
use qc_syntax::ast::{self, AstNode as _};

use crate::{BuiltinKey, Engine};

/// A diagnostic and the file of the module it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file's index in the module's file graph, as in [`qc_sema::Location`].
    pub file: FileId,
    /// The diagnostic; its span is in that file.
    pub diagnostic: Diagnostic,
}

/// The `W0311` warnings for a module checked against `engine` (ENGT-4.1).
///
/// `parses` are the module's files in compilation order, `parses[i]` being [`FileId`]`(i)`,
/// and `index` is the module resolved from them ([`qc_sema::resolve_module`]). Returns one
/// warning per call of a builtin the engine's table lacks, in file and then source order.
#[must_use]
pub fn missing_builtins(
    engine: Engine,
    parses: &[Parse],
    index: &ModuleIndex,
) -> Vec<FileDiagnostic> {
    let numbers = builtin_numbers(parses);
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
        let Some(&number) = numbers.get(def.name.as_str()) else {
            continue;
        };
        let key = BuiltinKey::Number(number);
        if engine.builtin(key).is_some() {
            continue;
        }
        out.push(FileDiagnostic {
            file: location.file,
            diagnostic: Diagnostic {
                code: Code::W0311,
                message: message(&def.name, number, engine),
                span: location.span,
            },
        });
    }
    out.sort_by_key(|d| (d.file, d.diagnostic.span));
    out
}

/// `NAME is builtin #N, which ENGINE does not have`, with the extensions of the first engine
/// whose table lists `#N` with some (ENGT-4.1).
fn message(name: &str, number: u32, engine: Engine) -> String {
    let mut message = format!(
        "{name} is builtin #{number}, which {} does not have",
        engine.name()
    );
    let extensions = Engine::ALL.into_iter().find_map(|other| {
        other
            .builtin(BuiltinKey::Number(number))
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

/// Each global name's builtin number: its first `#N` initialiser in compilation order
/// (ENGT-4.3).
fn builtin_numbers(parses: &[Parse]) -> HashMap<String, u32> {
    let mut numbers = HashMap::new();
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
                let Some(number) = builtin
                    .number()
                    .and_then(|t| builtin_number(&t.text_lossless()))
                else {
                    continue;
                };
                numbers.entry(name.text_lossless()).or_insert(number);
            }
        }
    }
    numbers
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
