// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice,
    clippy::integer_division,
    reason = "test code: a failed index, unwrap or panic is a failed test"
)]

mod ast_view;
mod declarations;
mod defs;
mod dialect;
mod dialect_ast;
mod dialect_builtins;
mod dialect_edges;
mod dialect_errors;
mod dialect_full;
mod dialect_rerelease;
mod dialect_statements;
mod expressions;
mod flags;
mod fte_directives;
mod keywords;
mod lossless;
mod nesting;
mod recovery;
mod reviewer;
mod statements;
mod tree_api;

use std::fmt::Write as _;

use qc_preproc::{CompilerFlags, FlagEdit, FteFlag};

/// The edit a file's first mark makes: FTEQCC's default flags with `set` set (FTEF-21.1).
fn whole(set: &[(FteFlag, bool)]) -> FlagEdit {
    let flags = set
        .iter()
        .fold(CompilerFlags::default(), |flags, &(flag, on)| {
            flags.with(flag, on)
        });
    FlagEdit::whole(flags)
}

use crate::ast;
use crate::{Code, Parse, SyntaxKind, parse};

/// The hand-written sample program with every vanilla construct.
const SAMPLE: &str = include_str!("sample.qc");

/// A hand-written program of FTEQCC constructs, which parses with no error under the FTEQCC
/// profile.
const SAMPLE_FTEQCC: &str = include_str!("sample_fteqcc.qc");

/// A program around one function body, the factory for statement and expression tests.
#[derive(Clone, Debug)]
struct Program<'a> {
    /// Declarations before the function.
    globals: &'a str,
    /// The statements of the function `f`.
    body: &'a str,
}

impl Default for Program<'_> {
    fn default() -> Self {
        Self {
            globals: "float a, b, c;\nentity e;\n.float fl;\n.entity en;\nvector v;\n",
            body: "",
        }
    }
}

impl Program<'_> {
    fn source(&self) -> String {
        format!("{}void() f =\n{{\n{}\n}};\n", self.globals, self.body)
    }

    fn parse(&self) -> Parse {
        parse(self.source().as_bytes())
    }
}

/// Parses `source` under the FTEQCC profile with the keywords `keywords` in force from its first
/// byte, as the preprocessor marks them (FTEF-20.1).
fn fte_with(source: &str, keywords: crate::Keywords) -> Parse {
    crate::parse_preprocessed_flagged(
        source.as_bytes(),
        crate::Dialect::Fteqcc,
        &[],
        &[(0, crate::KeywordEdit::whole(keywords))],
        &[],
    )
}

/// Parses a function body made of `body`.
fn body(body: &str) -> Parse {
    Program {
        body,
        ..Program::default()
    }
    .parse()
}

/// The codes and messages of every error.
fn errors(parsed: &Parse) -> Vec<(Code, String)> {
    parsed
        .errors()
        .map(|d| (d.code, d.message.clone()))
        .collect()
}

fn error_codes(source: &str) -> Vec<Code> {
    parse(source.as_bytes()).errors().map(|d| d.code).collect()
}

/// The statements of the function `f`.
fn statements(parsed: &Parse) -> Vec<ast::Stmt> {
    let file = parsed.tree().unwrap();
    let f = file
        .declarations()
        .find_map(|d| {
            let declarator = d.declarators().next()?;
            (declarator.name()?.text() == b"f").then_some(declarator)
        })
        .expect("function f");
    let Some(ast::Initializer::Body(body)) = f.initializer() else {
        panic!("f has no body");
    };
    body.block().unwrap().statements().collect()
}

/// An expression as an s-expression, with operators and names as written.
fn sexpr(expr: &ast::Expr) -> String {
    let token_text =
        |t: Option<crate::SyntaxToken>| t.map(|t| t.text_lossless()).unwrap_or_default();
    let sub = |e: Option<ast::Expr>| e.map_or_else(|| "?".to_owned(), |e| sexpr(&e));
    match expr {
        ast::Expr::Literal(lit) => token_text(lit.token()),
        ast::Expr::Name(name) => token_text(name.name()),
        ast::Expr::Paren(paren) => format!("({})", sub(paren.expr())),
        ast::Expr::Not(not) => format!("(! {})", sub(not.operand())),
        ast::Expr::Binary(bin) => format!(
            "({} {} {})",
            token_text(bin.op_token()),
            sub(bin.lhs()),
            sub(bin.rhs())
        ),
        ast::Expr::Field(field) => format!("(. {} {})", sub(field.base()), sub(field.field())),
        ast::Expr::Call(call) => {
            let mut out = format!("(call {}", sub(call.callee()));
            for arg in call
                .args()
                .into_iter()
                .flat_map(|a| a.args().collect::<Vec<_>>())
            {
                write!(out, " {}", sexpr(&arg)).unwrap();
            }
            out.push(')');
            out
        }
        ast::Expr::Postfix(post) => format!("(postfix {})", sub(post.operand())),
        ast::Expr::Ternary(tern) => {
            let (a, b) = tern.arms();
            format!("(?: {} {} {})", sub(tern.condition()), sub(a), sub(b))
        }
        ast::Expr::Index(index) => {
            format!("(index {} {})", sub(index.base()), sub(index.index()))
        }
        ast::Expr::Unary(unary) => format!("(unary {:?} {})", unary.op(), sub(unary.operand())),
        ast::Expr::Cast(cast) => format!("(cast {})", sub(cast.operand())),
        ast::Expr::Vector(vector) => {
            let parts: Vec<String> = vector.elements().map(|e| sexpr(&e)).collect();
            format!("[{}]", parts.join(" "))
        }
        ast::Expr::Sizeof(sizeof) => match sizeof.ty() {
            Some(ty) => format!(
                "(sizeof type {})",
                String::from_utf8_lossy(&ast::AstNode::syntax(&ty).text())
                    .trim()
                    .to_owned()
            ),
            None => format!("(sizeof {})", sub(sizeof.operand())),
        },
    }
}

/// The s-expression of the first statement of `stmts`, an expression statement.
fn first_expr(stmts: &str) -> String {
    let parsed = body(stmts);
    match statements(&parsed).first() {
        Some(ast::Stmt::Expr(stmt)) => stmt.expr().map(|e| sexpr(&e)).unwrap_or_default(),
        other => panic!("not an expression statement: {other:?}"),
    }
}

/// The kinds of the nodes on the path from the root to the first node of `kind`.
fn kinds(parsed: &Parse) -> Vec<SyntaxKind> {
    parsed.syntax().descendants().map(|n| n.kind()).collect()
}

// No spec statement: a lossless tree with no errors for the sample is LANG-2's requirement
// (plan/requirements.yaml), not an engine behaviour; the grammar is pinned by the tests
// citing QC-10 to QC-15.
#[test]
fn sample_parses_without_errors() {
    let parsed = parse(SAMPLE.as_bytes());
    assert_eq!(errors(&parsed), vec![], "{}", parsed.syntax().debug_tree());
    assert_eq!(parsed.syntax().text(), SAMPLE.as_bytes());
    assert!(parsed.tree().is_some());
}

// No spec statement: as above, LANG-2's lossless tree, for the FTEQCC sample; the FTEQCC
// grammar is pinned by the tests citing FTE and FTEF statements.
#[test]
fn fteqcc_sample_parses_without_errors() {
    let parsed = crate::parse_with(SAMPLE_FTEQCC.as_bytes(), crate::Dialect::Fteqcc);
    assert_eq!(errors(&parsed), vec![], "{}", parsed.syntax().debug_tree());
    assert_eq!(parsed.syntax().text(), SAMPLE_FTEQCC.as_bytes());
}
