// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC statement forms, state headers and terms of `docs/spec/dialect-fteqcc.md`,
//! FTEF-18: each parses under the FTEQCC profile, keeps its bytes, and is a diagnostic under the
//! vanilla profile.

use super::sexpr;
use crate::ast::{self, AstNode as _};
use crate::{Code, Dialect, Parse, SyntaxKind, parse, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The diagnostic codes of `source` under the FTEQCC dialect, warnings included.
fn fte_codes(source: &str) -> Vec<Code> {
    fte(source).diagnostics().iter().map(|d| d.code).collect()
}

/// Asserts `source` parses with no diagnostic under the FTEQCC dialect and keeps its bytes.
fn fte_clean(source: &str) -> Parse {
    let parsed = fte(source);
    assert_eq!(
        parsed
            .diagnostics()
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        Vec::<Code>::new(),
        "{source}\n{}",
        parsed.syntax().debug_tree()
    );
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    parsed
}

/// [`fte_clean`], and the vanilla dialect rejects `source`.
fn fte_ok(source: &str) -> Parse {
    let parsed = fte_clean(source);
    assert!(
        parse(source.as_bytes()).errors().next().is_some(),
        "vanilla accepts {source}"
    );
    parsed
}

/// `body` as the statements of a function `f`, indented.
fn in_body(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
    format!("float a, b, c;\nentity e;\nvoid() f =\n{{\n{indented}}};\n")
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

/// The s-expression of the expression of `stmt`, the body's only statement.
fn expr_of(stmt: &str) -> String {
    let parsed = fte_clean(&in_body(stmt));
    match statements(&parsed).first() {
        Some(ast::Stmt::Expr(expr)) => sexpr(&expr.expr().unwrap()),
        Some(ast::Stmt::Return(ret)) => sexpr(&ret.value().unwrap()),
        other => panic!("no expression: {other:?}"),
    }
}

/// The state header of the first function of `parsed`.
fn state(parsed: &Parse) -> ast::State {
    parsed
        .syntax()
        .descendants()
        .find_map(ast::State::cast)
        .unwrap()
}

// spec FTEF-18.3: a state's frame and next function are expressions, and a missing `,` is a warning
// spec QCD-7.21: W0205 "missing comma in state definition"
#[test]
fn state_parts_are_expressions() {
    let parsed = fte_ok("float F = 3;\nvoid() n;\nvoid() f = [F + 1, n] {};\n");
    let header = state(&parsed);
    assert_eq!(
        header.exprs().map(|e| sexpr(&e)).collect::<Vec<_>>(),
        ["(+ F 1)"]
    );
    assert_eq!(header.next().unwrap().text(), b"n");
    let parsed = fte_ok("void() f = [1, self.think] {};\n");
    let header = state(&parsed);
    assert!(header.next().is_none());
    assert_eq!(
        header.exprs().map(|e| sexpr(&e)).collect::<Vec<_>>(),
        ["1", "(. self think)"]
    );
    // The plain form keeps its vanilla shape: a literal frame and the next think's name.
    let parsed = fte("void() f = [$walk1, f] {};\n");
    assert!(state(&parsed).frame().is_some());
    assert_eq!(state(&parsed).next().unwrap().text(), b"f");
    let parsed = fte("void() f = [1 f] {};\n");
    let warning = &parsed.diagnostics()[0];
    assert_eq!(warning.code, Code::W0205);
    assert_eq!(warning.message, "missing comma in state definition");
    assert!(!warning.code.is_error());
    assert_eq!(state(&parsed).next().unwrap().text(), b"f");
    // Vanilla takes only a number and a name (QC-13.6).
    assert!(
        parse(b"void() f = [1 + 1, f] {};\n")
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-18.19: `[++ first .. last]` and `[-- first .. last]` cycle frames, `(w)` cycling weaponframe
// spec QCD-3.12: E0212 "Forwards State Cycle with backwards range" for two constant frames against the direction
#[test]
fn frame_cycles_parse() {
    for source in [
        "void() f = [++ 1 .. 5] {};\n",
        "void() f = [-- 5 .. 1] {};\n",
        "void() f = [++(w) 1..5] {};\n",
        "void() f = [--(W) $b .. $a] {};\n",
        "float A, B;\nvoid() f = [++ A .. B] {};\n",
        "void() f = [++ 2 .. 2] {};\n",
    ] {
        let parsed = fte(source);
        assert_eq!(parsed.errors().count(), 0, "{source}");
        assert!(state(&parsed).is_cycle(), "{source}");
        assert!(state(&parsed).next().is_none(), "{source}");
    }
    for source in [
        "void() f = [++ 5 .. 1] {};\n",
        "void() f = [-- 1 .. 5] {};\n",
        "void() f = [++(w) 3 .. -1] {};\n",
    ] {
        let parsed = fte(source);
        let codes: Vec<Code> = parsed.errors().map(|d| d.code).collect();
        assert_eq!(codes, [Code::E0212], "{source}");
        assert_eq!(
            parsed.errors().next().unwrap().message,
            "Forwards State Cycle with backwards range"
        );
    }
    assert_eq!(fte_codes("void() f = [++(x) 1 .. 2] {};\n"), [Code::E0204]);
    assert_eq!(fte_codes("void() f = [++ 1, 2] {};\n"), [Code::E0204]);
    assert!(
        parse(b"void() f = [++ 1 .. 5] {};\n")
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-18.5: an expression statement holds expressions separated by `,`, the last kept
#[test]
fn sequence_statements_parse() {
    assert_eq!(expr_of("a = 1, b = 2;"), "(, (= a 1) (= b 2))");
    assert_eq!(expr_of("a = 1, b, c = 3;"), "(, (= a 1) (, b (= c 3)))");
    // Arguments, initialisers and case values separate: no sequence there.
    assert_eq!(expr_of("f(a, b);"), "(call f a b)");
    let parsed = fte_clean(&in_body("local float x = 1, y = 2;"));
    assert!(matches!(
        statements(&parsed).first(),
        Some(ast::Stmt::Local(_))
    ));
    // Vanilla stops at the `,`.
    assert!(
        parse(in_body("a = 1, b = 2;").as_bytes())
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-18.20: returns, conditions, parentheses, subscripts, the middle of `?:` and sizeof take sequences
#[test]
fn sequences_where_fteqcc_allows_them() {
    assert_eq!(expr_of("return a, b;"), "(, a b)");
    assert_eq!(expr_of("x = (a, b);"), "(= x ((, a b)))");
    assert_eq!(expr_of("x = v[a, 1];"), "(= x (index v (, a 1)))");
    assert_eq!(expr_of("return a ? b, c : e;"), "(?: a (, b c) e)");
    for stmt in [
        "if (a, b) c = 1;",
        "while (a = 1, b) c = 1;",
        "do c = 1; while (a, b);",
        "switch (a, b) { default: break; }",
    ] {
        fte_ok(&in_body(stmt));
    }
    // The last arm of `?:` ends at a `,`.
    assert_eq!(expr_of("return a ? b : c, e;"), "(, (?: a b c) e)");
}

// spec FTEF-18.6: `name:` and `:name` are labels and `goto name;` jumps to one
// spec QCD-7.22: W0312 "Duplicate label NAME" for a label the function already has
#[test]
fn labels_and_goto_parse() {
    let parsed = fte_ok(&in_body("top:\na = 1;\n:frik\ngoto top;\ngoto frik;"));
    let stmts = statements(&parsed);
    let labels: Vec<String> = stmts
        .iter()
        .filter_map(|s| match s {
            ast::Stmt::Label(label) => Some(label.name().unwrap().text_lossless()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, ["top", "frik"]);
    let gotos: Vec<String> = stmts
        .iter()
        .filter_map(|s| match s {
            ast::Stmt::Goto(goto) => Some(goto.label().unwrap().text_lossless()),
            _ => None,
        })
        .collect();
    assert_eq!(gotos, ["top", "frik"]);
    // A name and a `:` with a space between is no label.
    assert!(fte(&in_body("top :\na = 1;")).errors().next().is_some());
    // A label twice in a function is a warning; another function may have its own.
    let parsed = fte(&in_body("l:\nl:\n:l\na = 1;"));
    let codes: Vec<Code> = parsed.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, [Code::W0312, Code::W0312]);
    assert_eq!(parsed.diagnostics()[0].message, "Duplicate label l");
    let two = "void() g = { l: a = 1; };\nfloat a;\nvoid() f = { l: a = 2; void() h = { l: a = 3; }; };\n";
    assert!(fte_codes(two).is_empty(), "{:?}", fte_codes(two));
    // A label or goto without a name is an error.
    assert_eq!(fte_codes(&in_body(":1\na = 1;")), [Code::E0201]);
    assert_eq!(fte_codes(&in_body("goto 1;")), [Code::E0201]);
}

// spec FTEF-18.7: `return = e;` sets the value returned
#[test]
fn return_assign_parses() {
    let parsed = fte_ok("float() f =\n{\n\treturn = 1 + 2;\n\treturn;\n};\n");
    let stmts = statements(&parsed);
    let Some(ast::Stmt::Return(ret)) = stmts.first() else {
        panic!("not a return");
    };
    assert_eq!(sexpr(&ret.value().unwrap()), "(+ 1 2)");
}

// spec FTEF-18.8: `if !(e)` negates the condition
#[test]
fn if_not_bang_parses() {
    let parsed = fte_ok(&in_body("if !(a) b = 1;"));
    let Some(ast::Stmt::If(if_stmt)) = statements(&parsed).into_iter().next() else {
        panic!("not an if");
    };
    assert_eq!(sexpr(&if_stmt.condition().unwrap()), "a");
    assert!(if_stmt.syntax().token_of(SyntaxKind::Not).is_some());
}

// spec FTEF-18.9: `__until (e) s` and `do s __until (e);`, and `until (e)` is a call when not a keyword
#[test]
fn until_loops_parse() {
    let parsed = fte_ok(&in_body("__until (a) b = 1;\ndo b = 2; __UNTIL (a);"));
    let stmts = statements(&parsed);
    assert!(matches!(stmts[0], ast::Stmt::While(_)));
    assert!(stmts[0].syntax().token_of(SyntaxKind::UntilKw).is_some());
    assert!(matches!(stmts[1], ast::Stmt::Do(_)));
    assert!(stmts[1].syntax().token_of(SyntaxKind::UntilKw).is_some());
    assert_eq!(expr_of("until (a);"), "(call until a)");
    assert!(
        !fte(&in_body("do b = 2; until (a);"))
            .errors()
            .next()
            .is_none()
    );
}

// spec FTEF-18.10: `case a .. b:` is a range
#[test]
fn case_ranges_parse() {
    let parsed = fte_ok(&in_body(
        "switch (a) {\ncase 1 .. 5:\ncase b..c:\n\tbreak;\n}",
    ));
    let labels: Vec<ast::CaseLabel> = parsed
        .syntax()
        .descendants()
        .filter_map(ast::CaseLabel::cast)
        .collect();
    assert_eq!(sexpr(&labels[0].value().unwrap()), "1");
    assert_eq!(sexpr(&labels[0].range_end().unwrap()), "5");
    assert_eq!(sexpr(&labels[1].range_end().unwrap()), "c");
}

// spec FTEF-18.11: `a ?: b` has no middle operand
#[test]
fn elvis_parses() {
    assert_eq!(expr_of("return a ?: b;"), "(?: a ? b)");
    assert_eq!(expr_of("return a ?: b ?: c;"), "(?: a ? (?: b ? c))");
    assert!(
        parse(in_body("return a ?: b;").as_bytes())
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-18.12: `sizeof` takes a type or an expression, with or without parentheses
#[test]
fn sizeof_forms_parse() {
    assert_eq!(expr_of("x = sizeof(float);"), "(= x (sizeof type float))");
    assert_eq!(expr_of("x = sizeof(int *);"), "(= x (sizeof type int *))");
    assert_eq!(
        expr_of("x = sizeof(node_t*);"),
        "(= x (sizeof type node_t*))"
    );
    assert_eq!(expr_of("x = sizeof(.float);"), "(= x (sizeof type .float))");
    assert_eq!(expr_of("x = sizeof(a) + 1;"), "(= x (+ (sizeof a) 1))");
    assert!(
        parse(in_body("x = sizeof a;").as_bytes())
            .errors()
            .next()
            .is_some()
    );
    assert_eq!(expr_of("x = sizeof a + 1;"), "(= x (sizeof (+ a 1)))");
    assert_eq!(expr_of("x = sizeof \"hi\";"), "(= x (sizeof \"hi\"))");
    assert_eq!(expr_of("x = __sizeof(a);"), "(= x (sizeof a))");
}

// spec FTEF-18.15: a function type may return a function
#[test]
fn functions_returning_functions_parse() {
    let parsed = fte_ok("float(float)(string) f;\n");
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let Some(ast::Type::Function(outer)) = decl.ty() else {
        panic!("not a function type");
    };
    let inner = outer.syntax().children().find_map(ast::Type::cast).unwrap();
    assert!(matches!(inner, ast::Type::Function(_)));
    let params: Vec<String> = outer
        .params()
        .unwrap()
        .params()
        .map(|p| String::from_utf8_lossy(&p.syntax().text()).into_owned())
        .collect();
    assert_eq!(params, ["string"]);
}

// spec FTEF-18.18: `..T` is a field of a field
#[test]
fn field_of_field_parses() {
    // Vanilla reads the two `.` tokens as two field levels too.
    let parsed = fte("..float ff;\n");
    assert!(parsed.diagnostics().is_empty());
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let Some(ast::Type::Field(outer)) = decl.ty() else {
        panic!("not a field type");
    };
    assert!(matches!(
        outer.syntax().children().find_map(ast::Type::cast),
        Some(ast::Type::Field(_))
    ));
    let parsed = fte(&in_body("..vector fv;"));
    assert!(matches!(
        statements(&parsed).first(),
        Some(ast::Stmt::Local(_))
    ));
}
