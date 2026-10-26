// SPDX-License-Identifier: GPL-3.0-or-later

//! Statements and the errors in them.

use super::{Program, body, errors, first_expr, sexpr, statements};
use crate::ast::{self, AstNode as _};
use crate::{Code, QC_MAX_NESTING, SyntaxKind, parse};

/// The kind of each statement of a body.
fn stmt_kinds(stmts: &str) -> Vec<SyntaxKind> {
    statements(&body(stmts))
        .iter()
        .map(|s| s.syntax().kind())
        .collect()
}

// spec QC-14.1: the first token chooses the statement: a block, return, while, do, local, if
// or an expression
#[test]
fn statement_kinds() {
    assert_eq!(
        stmt_kinds(
            "{ a = 1; }\nreturn;\nwhile (a) a = 0;\ndo a = 1; while (a);\nlocal float x;\n\
             if (a) b = 1;\na = 2;"
        ),
        vec![
            SyntaxKind::Block,
            SyntaxKind::ReturnStmt,
            SyntaxKind::WhileStmt,
            SyntaxKind::DoStmt,
            SyntaxKind::LocalDecl,
            SyntaxKind::IfStmt,
            SyntaxKind::ExprStmt,
        ]
    );
    assert_eq!(errors(&body("return a + 1;\nreturn;")), vec![]);
}

// spec QC-3.2: keywords are keywords only where the parser compares a token with them
#[test]
fn keywords_are_contextual() {
    let parsed = Program {
        globals: "float do;\nfloat a;\nfloat if, local, while, entity;\n",
        body: "a = do + if + local + while + entity;",
    }
    .parse();
    assert_eq!(errors(&parsed), vec![]);
    assert_eq!(
        first_expr("a = do + if + local;"),
        "(= a (+ (+ do if) local))"
    );
    // A statement starting with `do` is a `do` loop, whose body then starts at `=`.
    let parsed = body("do = 1;");
    assert_eq!(errors(&parsed)[0], (Code::E0201, "not a name".to_owned()));
    assert!(matches!(statements(&parsed)[0], ast::Stmt::Do(_)));
    // `for` and `switch` start a statement only before a `(`, `break` and `continue` only
    // before a `;`; elsewhere they are names.
    let parsed = Program {
        globals: "float for, switch, break, continue;\n",
        body: "for = 1; switch = 2; break = 3; continue = 4;",
    }
    .parse();
    assert_eq!(errors(&parsed), vec![]);
    // Keywords taken as keywords get keyword kinds; the same words elsewhere stay names.
    let tokens: Vec<SyntaxKind> = body("if (a) b = 1; else b = 2;")
        .syntax()
        .tokens()
        .filter(|t| t.text() == b"if" || t.text() == b"else")
        .map(|t| t.kind())
        .collect();
    assert_eq!(tokens, vec![SyntaxKind::IfKw, SyntaxKind::ElseKw]);
}

// spec QC-14.2: a nested block needs a statement, and there is no empty statement
#[test]
fn no_empty_statements() {
    assert_eq!(
        errors(&body(";")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    assert_eq!(
        errors(&body("if (a) {}")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    assert_eq!(errors(&parse(b"void() f = {};")), vec![]);
}

// spec QC-14.7: the parentheses around a condition are required
#[test]
fn conditions_need_parentheses() {
    assert_eq!(
        errors(&body("while a a = 1;"))[0],
        (Code::E0204, "expected (, found a".to_owned())
    );
    assert_eq!(
        errors(&body("if a) b = 1;")),
        vec![(Code::E0204, "expected (, found a".to_owned())]
    );
    assert_eq!(
        errors(&body("if (a b = 1;")),
        vec![(Code::E0204, "expected ), found b".to_owned())]
    );
}

// spec QCD-3.4: `PR_Expect` reports "expected X, found Y"
#[test]
fn do_while_needs_semicolon() {
    assert_eq!(
        errors(&body("do a = 1; while (a) }")),
        vec![(Code::E0204, "expected ;, found }".to_owned())]
    );
    assert_eq!(
        errors(&body("do a = 1; until (a);"))[0],
        (Code::E0204, "expected while, found until".to_owned())
    );
}

// spec QC-14.8: there is no `for`, `switch`, `break`, `continue` or `goto`; under the vanilla
// profile each FTEQCC keyword construct is rejected with E0510 (QCD-6.10), and `goto`, which
// is not an FTEQCC construct here, is read as an ordinary name.
#[test]
fn no_other_statements() {
    for word in [
        "for (a = 0; a < 3; a = a + 1) b = 1;",
        "switch (a) { }",
        "break;",
        "continue;",
    ] {
        let parsed = body(word);
        assert_eq!(
            errors(&parsed)[0].0,
            Code::E0510,
            "{word} should be rejected in the vanilla dialect"
        );
    }
    // `goto` is not in the FTEQCC subset, so it stays an ordinary name (an expression).
    let parsed = body("goto;");
    let stmts = statements(&parsed);
    let ast::Stmt::Expr(stmt) = &stmts[0] else {
        panic!("goto");
    };
    assert_eq!(sexpr(&stmt.expr().unwrap()), "goto");
}

#[test]
fn if_else_binds_to_nearest_if() {
    let parsed = body("if (a) if (b) c = 1; else c = 2;");
    let ast::Stmt::If(outer) = &statements(&parsed)[0] else {
        panic!("not an if");
    };
    assert!(outer.else_branch().is_none());
    let Some(ast::Stmt::If(inner)) = outer.then_branch() else {
        panic!("not an inner if");
    };
    assert!(inner.else_branch().is_some());
}

// spec QC-11.9, QCD-3.8: fields and functions cannot be local, E0208
#[test]
fn local_fields_and_functions() {
    for decl in [
        "local .float g;",
        "local void() g;",
        "local float(float x) g;",
    ] {
        assert_eq!(
            errors(&body(decl)),
            vec![(
                Code::E0208,
                "Fields and functions must be global".to_owned()
            )],
            "{decl}"
        );
    }
    assert_eq!(
        errors(&body("local float x, y = 2;\nlocal vector v;")),
        vec![]
    );
}

// spec QCD-3.4: a declaration in a body without `local` is an error, parsed as a local
#[test]
fn declaration_without_local() {
    let parsed = body("\tfloat x;\n\tx = 1;");
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0204, "expected ;, found x".to_owned())]
    );
    let stmts = statements(&parsed);
    let ast::Stmt::Local(local) = &stmts[0] else {
        panic!("not a local");
    };
    assert!(local.local_token().is_none());
    assert_eq!(stmts.len(), 2);
}

// spec QC-27.15, QCD-6.9: nesting past the limit is an error, E0509
#[test]
fn nesting_limit() {
    let parens = |n: usize| format!("a = {}a{};", "(".repeat(n), ")".repeat(n));
    // The statement, its expression, `=` and its right side take four levels.
    assert_eq!(errors(&body(&parens(QC_MAX_NESTING - 4))), vec![]);
    assert_eq!(
        errors(&body(&parens(QC_MAX_NESTING - 3))),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
    let ifs = format!("{}a = 1;", "if (a) ".repeat(QC_MAX_NESTING));
    assert_eq!(
        errors(&body(&ifs))[0],
        (Code::E0509, "nesting too deep".to_owned())
    );
    let ifs = format!("{}a = 1;", "if (a) ".repeat(QC_MAX_NESTING - 4));
    assert_eq!(errors(&body(&ifs)), vec![]);
    let chain = format!("a = a{};", " + a".repeat(QC_MAX_NESTING));
    assert_eq!(
        errors(&body(&chain)),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
    let fields = format!("a = e{};", ".en".repeat(QC_MAX_NESTING));
    assert_eq!(
        errors(&body(&fields)),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
    let dots = format!("{}float f;", ". ".repeat(QC_MAX_NESTING));
    assert_eq!(
        errors(&parse(dots.as_bytes())),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
}

// spec QC-27.15: no input exhausts the parser's stack
#[test]
fn deep_input_does_not_overflow() {
    for source in [
        format!("void() f = {{ a = {}; }};", "(".repeat(100_000)),
        format!("void() f = {{ a = {}a; }};", "!".repeat(100_000)),
        format!("void() f = {{ {} }};", "{".repeat(100_000)),
        format!("void() f = {{ {}a = 1; }};", "if (a) ".repeat(100_000)),
        format!("void() f = {{ a = a{}; }};", " = a".repeat(100_000)),
        format!("{}float f;", ". ".repeat(100_000)),
        format!("void({}) f;", "void(".repeat(100_000)),
    ] {
        let parsed = parse(source.as_bytes());
        assert_eq!(parsed.syntax().text(), source.as_bytes());
        let codes: Vec<Code> = parsed.errors().map(|d| d.code).take(3).collect();
        assert!(codes.contains(&Code::E0509), "{codes:?} {}", &source[..40]);
    }
}

// spec QCD-1.4: a field declaration in a body is an error, parsed as a local
#[test]
fn field_declaration_in_a_body() {
    let parsed = body("\t.float x;\n\ta = 1;");
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    let stmts = statements(&parsed);
    assert!(matches!(&stmts[0], ast::Stmt::Local(l) if l.local_token().is_none()));
    assert_eq!(stmts.len(), 2);
}
