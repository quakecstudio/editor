// SPDX-License-Identifier: GPL-3.0-or-later

//! The missing-builtin diagnostic: a call of a builtin the target engine lacks (ENGT-4).
//!
//! Implements ENGT-4 and reports QCD-7.17 (`W0311`). A module's calls are found in its syntax
//! trees and resolved through the semantic model's references (QC-12.5), so a call reaches the
//! diagnostic only when its callee name means a global builtin function. A builtin is keyed by
//! its number, the one the dialect's compiler binds (ENGT-4.12, ENGT-4.13), or, under an
//! FTEQCC dialect, by the engine name of `#0:NAME` (ENGT-4.17), `#"NAME"` or `:NAME`
//! (ENGT-4.14), read as FTEQCC reads it (ENGT-4.15, ENGT-4.16).

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::Dialect;
use qc_project::ModuleKind;
use qc_sema::{DefKind, FileDiagnostic, FileId, ModuleIndex};
use qc_syntax::Parse;
use qc_syntax::ast::{self, AstNode as _};

use crate::{Builtin, BuiltinKey, Engine};

/// How a module's source names a builtin (ENGT-4.12, ENGT-4.17): its number, or the engine
/// name of `#0:NAME`, `#"NAME"` or `:NAME`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltinRef {
    /// `= #N`, `N` at least 1, as the dialect's compiler binds it.
    Number(u32),
    /// The engine name the declaration gives.
    Name(String),
}

impl BuiltinRef {
    /// The engine's entry for this builtin in its table for `kind`, if it has one (ENGT-4.1).
    #[must_use]
    pub fn lookup(&self, engine: Engine, kind: ModuleKind) -> Option<Builtin> {
        match self {
            Self::Number(number) => engine.builtin(kind, BuiltinKey::Number(*number)),
            Self::Name(name) => engine.builtin_named(kind, name),
        }
    }
}

impl Engine {
    /// The `W0311` warnings for a module of `kind` checked against the engine (ENGT-4.1,
    /// ENGT-4.8).
    ///
    /// `files` are the module's files in compilation order, each with its parse in `dialect`, and
    /// `index` is the module resolved from them ([`qc_sema::resolve_module`]). Returns one
    /// warning per call of a builtin the engine's table for `kind` lacks, in file and then source
    /// order; none when the engine has no table for `kind` ([`Engine::has_table`]).
    ///
    /// Only the references to a builtin the engine lacks are looked up in their file's tree, so a
    /// module that declares none costs one pass over its declarations, not over every call.
    #[must_use]
    pub fn missing_builtins(
        self,
        kind: ModuleKind,
        files: &[(FileId, &Parse)],
        index: &ModuleIndex,
        dialect: Dialect,
    ) -> Vec<FileDiagnostic> {
        if !self.has_table(kind) {
            return Vec::new();
        }
        let keys = declared_builtins(files.iter().map(|&(_, parse)| parse), dialect);
        let missing: HashMap<&str, &BuiltinRef> = keys
            .iter()
            .filter(|(_, key)| key.lookup(self, kind).is_none())
            .map(|(name, key)| (name.as_str(), key))
            .collect();
        if missing.is_empty() {
            return Vec::new();
        }
        let parses: HashMap<FileId, &Parse> = files.iter().copied().collect();
        let mut out = Vec::new();
        for reference in index.refs() {
            let location = reference.location;
            let Some(def) = index.def(reference.def) else {
                continue;
            };
            if def.kind != DefKind::Function {
                continue;
            }
            let Some(&key) = missing.get(def.name.as_str()) else {
                continue;
            };
            let called = parses
                .get(&location.file)
                .is_some_and(|parse| names_a_callee(parse, location.span));
            if !called {
                continue;
            }
            out.push(FileDiagnostic::new(
                Code::W0311,
                location,
                message(&def.name, key, self, kind),
            ));
        }
        out.sort_by_key(|d| (d.file, d.diagnostic.span));
        out
    }
}

/// `NAME is builtin #N, which ENGINE does not have` (ENGT-4.1), or `#0:ENGINE_NAME` for a
/// builtin looked up by name (ENGT-4.17), with the extensions of the first engine whose table
/// for `kind` lists the key with some (ENGT-4.8).
fn message(name: &str, key: &BuiltinRef, engine: Engine, kind: ModuleKind) -> String {
    let shown = match key {
        BuiltinRef::Number(number) => number.to_string(),
        BuiltinRef::Name(engine_name) => format!("0:{engine_name}"),
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

/// Each global name's builtin, from a module's files in compilation order parsed in
/// `dialect`: the number of its first `#N` initialiser (ENGT-4.12), or for a name with none,
/// the engine name of its first `#0:NAME`, `#"NAME"` or `:NAME` initialiser (ENGT-4.17,
/// ENGT-4.14).
#[must_use]
pub fn declared_builtins<'p>(
    parses: impl IntoIterator<Item = &'p Parse>,
    dialect: Dialect,
) -> HashMap<String, BuiltinRef> {
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
                let literal = builtin.number().map(|t| t.text_lossless());
                if let Some(number) = literal.as_deref().and_then(|l| builtin_number(l, dialect)) {
                    numbers.entry(name.text_lossless()).or_insert(number);
                } else if builtin.binding(Dialect::Fteqcc) == Some(ast::Binding::Name)
                    && let Some(engine_name) =
                        builtin.engine_name(parse.lex_flags_at(builtin.syntax().span().start))
                {
                    // ENGT-4.9, ENGT-4.14: `binum` 0, from a literal FTEQCC reads as 0 or a name
                    // in place of the number; a name is in the initialiser only under FTEQCC,
                    // so it is read so.
                    names.entry(name.text_lossless()).or_insert(engine_name);
                }
            }
        }
    }
    let mut keys: HashMap<String, BuiltinRef> = names
        .into_iter()
        .map(|(name, engine_name)| (name, BuiltinRef::Name(engine_name)))
        .collect();
    for (name, number) in numbers {
        keys.insert(name, BuiltinRef::Number(number));
    }
    keys
}

/// The number of a builtin number literal in `dialect` (ENGT-4.12): the `binum` the dialect's
/// compiler binds (`qc_syntax::builtin_binum`), when it is at least 1: `qcc`'s `(int)` of the
/// literal's `float` value under the vanilla dialect and FTEQCC's of ENGT-4.13 under an FTEQCC
/// dialect, which binds none for a `d` literal (ENGT-4.15).
pub(crate) fn builtin_number(text: &str, dialect: Dialect) -> Option<u32> {
    let binum = qc_syntax::builtin_binum(text, dialect)?;
    u32::try_from(binum).ok().filter(|&number| number >= 1)
}

/// Whether the token at `span` in `parse` is the name a call expression calls (ENGT-4.2), as in
/// `f(x)`: the token found by descending to the elements whose span holds it, then the nearest
/// call above it, whose callee must be that name.
fn names_a_callee(parse: &Parse, span: Span) -> bool {
    let Some(tree) = parse.tree() else {
        return false;
    };
    let mut node = tree.syntax().clone();
    loop {
        let holding = node
            .children_with_tokens()
            .find(|child| child.span().start <= span.start && span.end <= child.span().end);
        match holding {
            Some(qc_syntax::SyntaxElement::Node(child)) => node = child,
            Some(qc_syntax::SyntaxElement::Token(token)) => {
                if token.span() != span {
                    return false;
                }
                let call = std::iter::once(token.parent())
                    .chain(token.parent().ancestors())
                    .find_map(ast::CallExpr::cast);
                return call.is_some_and(|call| {
                    matches!(call.callee(), Some(ast::Expr::Name(name))
                        if name.name().is_some_and(|t| t.span() == span))
                });
            }
            None => return false,
        }
    }
}
