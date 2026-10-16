// SPDX-License-Identifier: GPL-3.0-or-later

//! Expressions: precedence, terms and the errors in them.

use super::{body, errors, first_expr, statements};
use crate::ast;
use crate::{Code, SyntaxKind};

// spec QC-15.1: `*` `/` bind tighter than `+` `-`, which bind tighter than comparisons, then
// `=`, then `&&` `||`; all but `=` associate left
#[test]
fn priorities() {
    assert_eq!(
        first_expr("a = b + c * a - b / c;"),
        "(= a (- (+ b (* c a)) (/ b c)))"
    );
    assert_eq!(first_expr("a = a < b + c;"), "(= a (< a (+ b c)))");
    assert_eq!(first_expr("a = b - c - a;"), "(= a (- (- b c) a))");
    assert_eq!(first_expr("a = b / c / a;"), "(= a (/ (/ b c) a))");
}

// spec QC-15.2: `&` and `|` bind as tightly as `*` and `/`
#[test]
fn bitwise_bind_like_multiplication() {
    assert_eq!(first_expr("a = b & c | 4;"), "(= a (| (& b c) 4))");
    assert_eq!(first_expr("a = a + b & c;"), "(= a (+ a (& b c)))");
    assert_eq!(first_expr("a = b * c & a;"), "(= a (& (* b c) a))");
}

// spec QC-15.3: `=` binds tighter than `&&` and `||`
#[test]
fn assignment_binds_tighter_than_logic() {
    assert_eq!(first_expr("a = b && c;"), "(&& (= a b) c)");
    assert_eq!(first_expr("a && b = c;"), "(&& a (= b c))");
    assert_eq!(first_expr("a = b = c;"), "(= a (= b c))");
}

// spec QC-15.4: `&&` and `||` have one priority and associate left
#[test]
fn logic_is_left_associative() {
    assert_eq!(first_expr("a || b && c;"), "(&& (|| a b) c)");
    assert_eq!(first_expr("a && b || c;"), "(|| (&& a b) c)");
}

// spec QC-15.5: comparisons associate left
#[test]
fn comparisons_associate_left() {
    assert_eq!(first_expr("a == b == c;"), "(== (== a b) c)");
    assert_eq!(first_expr("a < b >= c != a;"), "(!= (>= (< a b) c) a)");
}

// spec QC-15.6: the operand of `!` is an expression at priority 4
#[test]
fn not_takes_a_comparison() {
    assert_eq!(first_expr("a = !b == c;"), "(= a (! (== b c)))");
    assert_eq!(first_expr("a = !b + c;"), "(= a (! (+ b c)))");
    assert_eq!(first_expr("a = !b && c;"), "(&& (= a (! b)) c)");
    assert_eq!(first_expr("a = !!b;"), "(= a (! (! b)))");
}

// spec QC-15.7: a term is `!` expression, a parenthesised expression, a literal, a frame macro
// reference or a name, and `v_x` is a name
#[test]
fn terms() {
    assert_eq!(first_expr("a = (b + c) * a;"), "(= a (* ((+ b c)) a))");
    assert_eq!(first_expr("a = v_x;"), "(= a v_x)");
    assert_eq!(first_expr("v = '1 -2 3.5';"), "(= v '1 -2 3.5')");
    assert_eq!(first_expr("a = $walk1;"), "(= a $walk1)");
    assert_eq!(first_expr("s = \"x\";"), "(= s \"x\")");
    let parsed = body("a = 1;");
    let ast::Stmt::Expr(stmt) = &statements(&parsed)[0] else {
        panic!("not an expression");
    };
    let Some(ast::Expr::Binary(assign)) = stmt.expr() else {
        panic!("not an assignment");
    };
    let Some(ast::Expr::Literal(one)) = assign.rhs() else {
        panic!("not a literal");
    };
    assert_eq!(one.kind(), Some(ast::LiteralKind::Float));
}

// spec QC-15.8: a call ends the field-access chain, so `g().fl` and `g()()` do not parse;
// field access chains to the left
#[test]
fn calls_and_field_access() {
    assert_eq!(first_expr("a = e.en.fl;"), "(= a (. (. e en) fl))");
    assert_eq!(first_expr("e.th();"), "(call (. e th))");
    assert_eq!(
        first_expr("g(a, b + c, h(1));"),
        "(call g a (+ b c) (call h 1))"
    );
    assert_eq!(first_expr("a = g() * 2;"), "(= a (* (call g) 2))");
    assert_eq!(
        errors(&body("a = g().fl;")),
        vec![(Code::E0204, "expected ;, found .".to_owned())]
    );
    assert_eq!(
        errors(&body("g()();")),
        vec![(Code::E0204, "expected ;, found (".to_owned())]
    );
}

// spec QC-4.5: there is no unary minus: `-` before anything but a digit is "not a name"
#[test]
fn no_unary_minus() {
    assert_eq!(
        errors(&body("a = -b;")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    assert_eq!(first_expr("a = b - -5;"), "(= a (- b -5))");
}

// spec QCD-3.4: a negative literal after a name is "expected ;, found -5"
#[test]
fn negative_literal_after_a_name() {
    assert_eq!(
        errors(&body("a = b-5;")),
        vec![(Code::E0204, "expected ;, found -5".to_owned())]
    );
}

// spec QC-17.5: there are no compound assignments
#[test]
fn no_compound_assignment() {
    for op in ["+=", "-=", "*=", "/=", "&=", "|="] {
        assert_eq!(
            errors(&body(&format!("a {op} 1;"))),
            vec![(Code::E0201, "not a name".to_owned())],
            "{op}"
        );
    }
}

// spec QC-27.10, QCD-6.8: the left side of `=` must be a name or a field access, E0508
#[test]
fn cannot_assign() {
    for stmt in [
        "a = 1;",
        "e.fl = 1;",
        "e.en.fl = 1;",
        "(a) = 1;",
        "a = b = c;",
    ] {
        assert_eq!(errors(&body(stmt)), vec![], "{stmt}");
    }
    for (stmt, lhs) in [
        ("5 = a;", "5"),
        ("g() = 3;", "g()"),
        ("(a + b) = 3;", "(a + b)"),
        ("!a = 1;", "!a"),
        ("a + b = c;", "a + b"),
        ("\"s\" = a;", "\"s\""),
    ] {
        let parsed = body(stmt);
        assert_eq!(
            errors(&parsed),
            vec![(Code::E0508, "cannot assign to this expression".to_owned())],
            "{stmt}"
        );
        let span = parsed.errors().next().unwrap().span;
        let text = parsed.syntax().text();
        assert_eq!(&text[span.range()], lhs.as_bytes(), "{stmt}");
    }
}

// spec QC-27.3: a string literal is always a string, never a keyword or punctuation
#[test]
fn string_literals_are_not_keywords() {
    let parsed = body("\"if\" (a) b = 1;");
    let stmts = statements(&parsed);
    assert!(matches!(stmts[0], ast::Stmt::Expr(_)));
    assert!(
        !parsed
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::IfStmt)
    );
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0204, "expected ;, found b".to_owned())]
    );
    assert_eq!(
        errors(&body("a = b \";\"")),
        vec![(Code::E0204, "expected ;, found ;".to_owned())]
    );
}

// spec QCD-1.3: a diagnostic's span is the token the rule names
#[test]
fn diagnostic_spans() {
    let parsed = body("a = b c;");
    let error = parsed.errors().next().unwrap();
    assert_eq!(error.message, "expected ;, found c");
    assert_eq!(&parsed.syntax().text()[error.span.range()], b"c");
    let parsed = body("a = -b;");
    let error = parsed.errors().next().unwrap();
    assert_eq!(&parsed.syntax().text()[error.span.range()], b"-");
    let parsed = crate::parse(b"float a");
    let error = parsed.errors().next().unwrap();
    assert_eq!(error.message, "expected ;, found ");
    assert_eq!((error.span.start, error.span.end), (7, 7));
}

// spec QCD-1.4: the parser keeps going after an error, one error per statement
#[test]
fn one_error_per_statement() {
    let parsed = body("a = -b;\nb = (c;\nc = 1;\nd e;");
    assert_eq!(
        errors(&parsed),
        vec![
            (Code::E0201, "not a name".to_owned()),
            (Code::E0204, "expected ), found ;".to_owned()),
            (Code::E0204, "expected ;, found e".to_owned()),
        ]
    );
    assert_eq!(statements(&parsed).len(), 4);
}

// spec QC-4.5: the `-` that starts no term is kept in an error node inside the expression
#[test]
fn stray_minus_stays_in_the_expression() {
    let parsed = body("a = -b;");
    let error = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::Error)
        .unwrap();
    assert_eq!(error.text(), b"-");
    assert_eq!(error.parent().unwrap().kind(), SyntaxKind::BinaryExpr);
}
