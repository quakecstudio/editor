// SPDX-License-Identifier: GPL-3.0-or-later

//! The typed view of the FTEQCC nodes, and the edges of the FTEQCC parse where a construct
//! must not be taken for another (`docs/spec/dialect-fteqcc-rerelease.md`).

use super::sexpr;
use crate::ast::{self, AstNode as _, BinaryOp};
use crate::{Code, Dialect, Parse, SyntaxKind, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The error codes of `source` under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<Code> {
    fte(source).errors().map(|d| d.code).collect()
}

/// `body`, indented, as the statements of a function, with globals for it to use.
fn in_body(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
    format!("float a, b, c, i, n;\nvoid() f =\n{{\n{indented}}};\n")
}

/// The statements of the function `f` of `parsed`.
fn statements(parsed: &Parse) -> Vec<ast::Stmt> {
    let file = parsed.tree().unwrap();
    let body = file
        .declarations()
        .filter_map(|d| d.declarators().next())
        .find(|d| d.name().is_some_and(|n| n.text() == b"f"))
        .and_then(|d| d.initializer());
    let Some(ast::Initializer::Body(body)) = body else {
        panic!("f has no body");
    };
    body.block().unwrap().statements().collect()
}

/// The statements of a body parsed with no error under the FTEQCC dialect.
fn fte_statements(body: &str) -> Vec<ast::Stmt> {
    let source = in_body(body);
    let parsed = fte(&source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new(),
        "{source}"
    );
    statements(&parsed)
}

/// The expression of the only statement of a body, an expression statement.
fn fte_expr(body: &str) -> ast::Expr {
    match fte_statements(body).into_iter().next() {
        Some(ast::Stmt::Expr(stmt)) => stmt.expr().unwrap(),
        other => panic!("not an expression statement: {other:?}"),
    }
}

/// The first top-level node of kind `N` in `source`, parsed under the FTEQCC dialect.
fn first<N: ast::AstNode>(source: &str) -> N {
    fte(source)
        .syntax()
        .descendants()
        .find_map(N::cast)
        .unwrap()
}

// spec FTE-2.1: a `for` exposes its three clauses and its body.
#[test]
fn for_statement_view() {
    let Some(ast::Stmt::For(for_stmt)) = fte_statements("for (i = 0; i < n; i++) a = i;")
        .into_iter()
        .next()
    else {
        panic!("not a for loop");
    };
    assert_eq!(for_stmt.clauses().count(), 3);
    assert!(
        for_stmt
            .clauses()
            .all(|c| c.kind() == SyntaxKind::ForClause)
    );
    assert!(matches!(for_stmt.body(), Some(ast::Stmt::Expr(_))));
    let Some(ast::Stmt::For(empty)) = fte_statements("for (;;) return;").into_iter().next() else {
        panic!("not a for loop");
    };
    assert_eq!(empty.clauses().count(), 0);
    assert!(matches!(empty.body(), Some(ast::Stmt::Return(_))));
}

// spec FTE-3.1: a `switch` exposes its scrutinee, block and labels; `default` has no value.
#[test]
fn switch_statement_view() {
    let Some(ast::Stmt::Switch(switch)) =
        fte_statements("switch (i) { case 1: a = 1; default: a = 2; }")
            .into_iter()
            .next()
    else {
        panic!("not a switch");
    };
    assert_eq!(sexpr(&switch.scrutinee().unwrap()), "i");
    let block = switch.block().unwrap();
    let labels: Vec<ast::CaseLabel> = block
        .syntax()
        .children()
        .filter_map(ast::CaseLabel::cast)
        .collect();
    assert_eq!(labels.len(), 2);
    assert!(!labels[0].is_default());
    assert_eq!(sexpr(&labels[0].value().unwrap()), "1");
    assert!(labels[1].is_default());
    assert!(labels[1].value().is_none());
}

// spec FTE-5.1: a postfix expression exposes its operand.
// spec FTE-8.1: a ternary exposes its condition and both arms.
// spec FTE-13.2: a subscript exposes its base and index.
#[test]
fn expression_views() {
    let ast::Expr::Postfix(post) = fte_expr("i++;") else {
        panic!("not a postfix expression");
    };
    assert_eq!(sexpr(&post.operand().unwrap()), "i");
    let ast::Expr::Ternary(ternary) = fte_expr("b ? c : i;") else {
        panic!("not a ternary");
    };
    assert_eq!(sexpr(&ternary.condition().unwrap()), "b");
    let (yes, no) = ternary.arms();
    assert_eq!(sexpr(&yes.unwrap()), "c");
    assert_eq!(sexpr(&no.unwrap()), "i");
    let ast::Expr::Binary(assign) = fte_expr("a = b[i + 1];") else {
        panic!("not an assignment");
    };
    let Some(ast::Expr::Index(index)) = assign.rhs() else {
        panic!("not a subscript");
    };
    assert_eq!(sexpr(&index.base().unwrap()), "b");
    assert_eq!(sexpr(&index.index().unwrap()), "(+ i 1)");
}

// spec FTE-19.6: a cast exposes the type it casts to.
#[test]
fn cast_view() {
    let ast::Expr::Binary(assign) = fte_expr("a = (vector) b;") else {
        panic!("not an assignment");
    };
    let Some(ast::Expr::Cast(cast)) = assign.rhs() else {
        panic!("not a cast");
    };
    let Some(ast::Type::Simple(simple)) = cast.ty() else {
        panic!("not a simple type");
    };
    assert_eq!(simple.kind(), Some(ast::SimpleTypeKind::Vector));
}

// spec FTE-6.1: `-=`, `*=` and `/=` are their own operators.
#[test]
fn compound_assignment_operators() {
    for (stmt, op) in [
        ("a -= b;", BinaryOp::SubAssign),
        ("a *= b;", BinaryOp::MulAssign),
        ("a /= b;", BinaryOp::DivAssign),
    ] {
        let ast::Expr::Binary(bin) = fte_expr(stmt) else {
            panic!("not a binary expression");
        };
        assert_eq!(bin.op(), Some(op), "{stmt}");
    }
}

// spec FTE-13.1: an aggregate initialiser exposes its expression elements, and a trailing
// comma before `}` is accepted.
#[test]
fn aggregate_initialiser_view() {
    let source = "const string names[] = { \"a\", \"b\", };\n";
    assert_eq!(fte_errors(source), Vec::<Code>::new());
    let aggregate: ast::AggregateInit = first(source);
    assert_eq!(aggregate.elements().count(), 2);
}

// spec FTE-14.1: a typedef exposes its type name, and the declaration after it on the same
// line is a declaration of its own.
#[test]
fn typedef_view() {
    let source = "typedef struct { string mdl; float frame; } poseInfo; float x;\n";
    let parsed = fte(source);
    assert_eq!(parsed.errors().count(), 0);
    let typedef: ast::TypedefDecl = first(source);
    assert_eq!(typedef.name().unwrap().text(), b"poseInfo");
    assert_eq!(parsed.tree().unwrap().declarations().count(), 1);
}

// spec FTE-15.4: an enum's name is the name before its `{` or `:`, not a constant; the
// declaration after it on the same line is a declaration of its own.
#[test]
fn enum_view() {
    let source = "enum Kind : float { K_A, K_B }; float x;\n";
    let parsed = fte(source);
    assert_eq!(parsed.errors().count(), 0);
    let en: ast::EnumDecl = first(source);
    assert_eq!(en.name().unwrap().text(), b"Kind");
    assert_eq!(parsed.tree().unwrap().declarations().count(), 1);
    let anonymous: ast::EnumDecl = first("enum { K_A };\n");
    assert!(anonymous.name().is_none());
}

// spec FTE-2.1, FTE-3.1, FTE-4.1: `for`, `switch`, `break` and `continue` are statements only
// in their own shape, so a value named like one is still an expression.
#[test]
fn statement_words_are_names_elsewhere() {
    let source = "float for, switch, continue;\nvoid() break = #6;\n\
                  void() f =\n{\n\tfor = 1;\n\tswitch = for;\n\tbreak();\n\tcontinue = 2;\n};\n";
    let parsed = fte(source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new()
    );
    assert!(
        statements(&parsed)
            .iter()
            .all(|s| matches!(s, ast::Stmt::Expr(_)))
    );
}

// spec FTE-21.1: a reserved word followed by a name is not a declaration of a user type, and
// neither is a name alone at file level.
#[test]
fn reserved_words_do_not_start_declarations() {
    assert!(!fte_errors(&in_body("for x;")).is_empty());
    assert!(!fte_errors(&in_body("break x;")).is_empty());
    assert_eq!(fte_errors("foo = 1;\n"), [Code::E0203]);
    assert_eq!(fte_errors("poseInfo pose;\n"), Vec::<Code>::new());
}

// spec FTE-10.1: a `const` local needs no `local`, and `local` may precede the qualifier.
#[test]
fn qualified_locals_parse() {
    for body in [
        "const float x = 1;",
        "local const float y = 2;",
        "var float z;",
    ] {
        let stmts = fte_statements(body);
        assert!(matches!(stmts.first(), Some(ast::Stmt::Local(_))), "{body}");
    }
}

// spec FTE-17.4: under FTEQCC a builtin number may be 0 with or without a name.
#[test]
fn builtin_zero_needs_no_name() {
    assert_eq!(fte_errors("void() g = #0;\n"), Vec::<Code>::new());
    assert_eq!(fte_errors("void() g = #1.5;\n"), [Code::E0209]);
}

// spec FTE-19.5: only a negative number stands for an operator; a positive one after an
// operand is an error, as in vanilla.
#[test]
fn a_positive_number_is_not_an_operator() {
    assert_eq!(fte_errors(&in_body("a = b 1;")), [Code::E0204]);
}

// spec FTE-8.4: `?:` is read at the assignment priority, tighter than `&&` and `||`: the
// right operand of `=`, `&&` and `||` is the conditional, and a comparison is its condition.
#[test]
fn ternary_binds_at_the_assignment_priority() {
    for (body, tree) in [
        ("a = b ? c : i;", "(= a (?: b c i))"),
        ("a = b = c ? i : n;", "(= a (= b (?: c i n)))"),
        ("a && b ? c : i;", "(&& a (?: b c i))"),
        ("a || b ? c : i;", "(|| a (?: b c i))"),
        ("a == b ? c : i;", "(?: (== a b) c i)"),
        ("a + b ? c : i;", "(?: (+ a b) c i)"),
        ("!a ? b : c;", "(?: (! a) b c)"),
        ("(a = b) ? c : i;", "(?: ((= a b)) c i)"),
    ] {
        assert_eq!(sexpr(&fte_expr(body)), tree, "{body}");
    }
}

// spec FTE-8.4: the arms of `?:` are read at the loosest priority, so a nested conditional
// groups to the right in its last arm and inside its middle one, and the last arm takes `&&`
// and `=`.
#[test]
fn ternary_arms_take_every_operator() {
    for (body, tree) in [
        ("a ? b : c ? i : n;", "(?: a b (?: c i n))"),
        ("a ? b ? c : i : n;", "(?: a (?: b c i) n)"),
        ("a ? b : c && i;", "(?: a b (&& c i))"),
        ("a ? b : c = i;", "(?: a b (= c i))"),
        ("a = b ? c : i ? n : a;", "(= a (?: b c (?: i n a)))"),
        ("a && b ? c : i || n;", "(&& a (?: b c (|| i n)))"),
    ] {
        assert_eq!(sexpr(&fte_expr(body)), tree, "{body}");
    }
}
