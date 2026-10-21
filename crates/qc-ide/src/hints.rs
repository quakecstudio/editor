// SPDX-License-Identifier: GPL-3.0-or-later

//! Inlay hints: the parameter name before each argument of a call.
//!
//! QuakeC passes arguments by position into the parameter globals (QC-13.5), so a call such as
//! `T_Damage (other, self, self, 10)` says nothing of which argument is which. As in
//! rust-analyzer, each argument gets the name of its parameter, taken from the declaration of
//! the function the callee resolves to; an argument that is already a name equal to the
//! parameter's gets none.

use std::path::Path;

use qc_diag::Span;
use qc_project::Loader;
use qc_sema::{DefId, DefKind, ModuleIndex};
use qc_syntax::SyntaxKind;
use qc_syntax::ast::{self, AstNode as _};

use crate::host::{AnalysisHost, ModuleData, ModuleState};
use crate::syntax::{declaration_of, name_at, parameter_names, token_at};

/// One parameter name shown before an argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterHint {
    /// The byte offset of the argument's first token.
    pub offset: usize,
    /// The parameter's name.
    pub name: String,
}

impl<L: Loader> AnalysisHost<L> {
    /// The parameter names of the arguments of every call of `path` whose span meets `range`,
    /// in source order; read in the first module that compiles the file.
    pub fn parameter_hints(&mut self, path: &Path, range: Span) -> Vec<ParameterHint> {
        let Some(&(module, file)) = self.modules_of(path).first() else {
            return Vec::new();
        };
        let Some((state, data)) = self.module(module) else {
            return Vec::new();
        };
        let Some(parse) = data.parses.get(file.0) else {
            return Vec::new();
        };
        let index = &data.analysis.index;
        let mut out = Vec::new();
        for node in parse.syntax().descendants() {
            let Some(call) = ast::CallExpr::cast(node) else {
                continue;
            };
            let span = call.syntax().span();
            if span.end < range.start || span.start > range.end {
                continue;
            }
            let Some(ast::Expr::Name(callee)) = call.callee() else {
                continue;
            };
            let Some(name) = callee.name() else {
                continue;
            };
            let Some((def, _)) = index.definition_at(file, name.span().start) else {
                continue;
            };
            let names = function_parameters(state, data, index, def);
            let Some(args) = call.args() else {
                continue;
            };
            for (arg, param) in args.args().zip(names) {
                let Some(param) = param else {
                    continue;
                };
                if let ast::Expr::Name(n) = &arg
                    && n.name().is_some_and(|t| t.text_lossless() == param)
                {
                    continue;
                }
                let first = arg.syntax().tokens().find(|t| {
                    !matches!(
                        t.kind(),
                        SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment
                    )
                });
                if let Some(first) = first {
                    out.push(ParameterHint {
                        offset: first.span().start,
                        name: param,
                    });
                }
            }
        }
        out
    }
}

/// The parameter names of function `def`, from the first of its declarations that names
/// them; empty when `def` is not a function.
fn function_parameters(
    state: &ModuleState,
    data: &ModuleData,
    index: &ModuleIndex,
    def: DefId,
) -> Vec<Option<String>> {
    let Some(definition) = index.def(def) else {
        return Vec::new();
    };
    if definition.kind != DefKind::Function {
        return Vec::new();
    }
    let declarations = std::iter::once(definition.location).chain(
        index
            .redeclarations
            .iter()
            .filter(|r| r.def == def)
            .map(|r| r.location),
    );
    for location in declarations {
        let Some(parse) = data.parses.get(location.file.0) else {
            continue;
        };
        let text = state.db.file_text(location.file);
        if !name_at(text, location.span, &definition.name) {
            continue;
        }
        let Some(token) = token_at(parse, location.span) else {
            continue;
        };
        let Some(decl) = declaration_of(&token) else {
            continue;
        };
        let names = parameter_names(&decl, &token);
        if names.iter().any(Option::is_some) {
            return names;
        }
    }
    Vec::new()
}
