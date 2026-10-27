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

// spec FTEF-18.28: `[++ first .. last]` and `[-- first .. last]` cycle frames, `(w)` cycling weaponframe
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
    assert_eq!(
        expr_of("a = 1, b = 2, c = 3;"),
        "(, (= a 1) (, (= b 2) (= c 3)))"
    );
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
    // Each expression before a `,` assigns, so none is "Statement does not do anything"
    // (FTEF-18.25).
    assert_eq!(expr_of("return a = 1, b;"), "(, (= a 1) b)");
    assert_eq!(expr_of("x = (a = 1, b);"), "(= x ((, (= a 1) b)))");
    assert_eq!(expr_of("x = v[a = 1, 1];"), "(= x (index v (, (= a 1) 1)))");
    assert_eq!(
        expr_of("return a ? b = 1, c : e;"),
        "(?: a (, (= b 1) c) e)"
    );
    for stmt in [
        "if (a = 1, b) c = 1;",
        "while (a = 1, b) c = 1;",
        "do c = 1; while (a = 1, b);",
        "switch (a = 1, b) { default: break; }",
    ] {
        fte_ok(&in_body(stmt));
    }
    // The last arm of `?:` ends at a `,`.
    assert_eq!(
        expr_of("return a ? b : c = 1, e;"),
        "(, (?: a b (= c 1)) e)"
    );
}

// spec FTEF-18.35: a duplicate `name:` leaves its `:`, which starts a `:name` label, and a `:`
// before no name is "invalid label name"
#[test]
fn a_duplicate_c_label_leaves_its_colon() {
    let parsed = fte(&in_body("l: ;\nl: x: ;"));
    let found: Vec<(Code, String)> = parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code != Code::W0204)
        .map(|d| (d.code, d.message.clone()))
        .collect();
    assert_eq!(
        found,
        [
            (Code::W0312, "Duplicate label l".to_owned()),
            (Code::E0201, "invalid label name \";\"".to_owned()),
        ]
    );
    let labels: Vec<String> = statements(&fte(&in_body("l: a = 1;\nl: x; a = 2;")))
        .iter()
        .filter_map(|s| match s {
            ast::Stmt::Label(label) => Some(label.name().unwrap().text_lossless()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, ["l", "l", "x"]);
}

// spec FTEF-18.23: `name:` and `:name` are labels and `goto name;` jumps to one; a label read since the end of the last function is a duplicate
// spec QCD-7.30: W0312 "Duplicate label NAME" for a label read since the last function's end
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
    let parsed = fte(&in_body("l:\n:l\n:l\na = 1;"));
    let codes: Vec<Code> = parsed.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, [Code::W0312, Code::W0312]);
    assert_eq!(parsed.diagnostics()[0].message, "Duplicate label l");
    let two = "void() g = { l: a = 1; };\nfloat a;\nvoid() f = { l: a = 2; };\n";
    assert!(fte_codes(two).is_empty(), "{:?}", fte_codes(two));
    // A function defined inside another shares the labels read before it, and its end
    // forgets them all.
    let nested = "float a;\nvoid() f = { l: a = 2; void() h = { :l a = 3; }; l: a = 4; };\n";
    assert_eq!(fte_codes(nested), [Code::W0312]);
    // A label or goto without a name is an error.
    assert_eq!(fte_codes(&in_body(":1\na = 1;")), [Code::E0201]);
    assert_eq!(fte_codes(&in_body("goto 1;")), [Code::E0201]);
}

// spec FTEF-18.24: at the end of each function, an inner one included, every goto read since the last end must name a label read since then
// spec QCD-4.8: E0308 "Goto statement with no matching label \"NAME\""
#[test]
fn a_goto_needs_a_label_by_the_end_of_its_function() {
    // A label later in the function is found.
    assert!(fte_codes(&in_body("goto top;\na = 1;\ntop:\na = 2;")).is_empty());
    let parsed = fte(&in_body("goto nowhere;\ngoto top;\ntop:\na = 1;"));
    let codes: Vec<Code> = parsed.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, [Code::E0308]);
    assert_eq!(
        parsed.diagnostics()[0].message,
        "Goto statement with no matching label \"nowhere\""
    );
    assert!(Code::E0308.is_error());
    // Another function's label is forgotten at its end.
    let other = "float a;\nvoid() g = { top: a = 1; };\nvoid() f = { goto top; };\n";
    assert_eq!(fte_codes(other), [Code::E0308]);
    // An inner function's end checks the gotos read before it against the labels read
    // since the last end, its own included, and then forgets both.
    let inner_label = "float a;\nvoid() f = { goto out; void() h = { out: a = 1; }; };\n";
    assert!(
        fte_codes(inner_label).is_empty(),
        "{:?}",
        fte_codes(inner_label)
    );
    let label_after = "float a;\nvoid() f = { goto out; void() h = { a = 1; }; out: a = 2; };\n";
    assert_eq!(fte_codes(label_after), [Code::E0308]);
    let label_before = "float a;\nvoid() f = { top: a = 1; void() h = { a = 2; }; goto top; };\n";
    assert_eq!(fte_codes(label_before), [Code::E0308]);
}

// spec FTEF-18.25: an expression statement, a `for` clause FTEQCC discards, or an expression
// before a `,`, with no call, assignment or `++`/`--` of its own, is "Statement does not do
// anything"; a `void` cast is let through only at the end of a statement.
// spec QCD-7.29: W0313 "Statement does not do anything"
#[test]
fn statements_that_do_nothing_are_warned() {
    let warned = |body: &str| -> Vec<String> {
        let source = in_body(body);
        let parsed = fte(&source);
        assert_eq!(parsed.errors().count(), 0, "{source}");
        parsed
            .diagnostics()
            .iter()
            .filter(|d| d.code == Code::W0313)
            .map(|d| {
                assert_eq!(d.message, "Statement does not do anything");
                String::from_utf8_lossy(&source.as_bytes()[d.span.start..d.span.end]).into_owned()
            })
            .collect()
    };
    for (body, spans) in [
        ("a;", &["a"][..]),
        ("a + 1;", &["a + 1"]),
        ("b ? c : a;", &["b ? c : a"]),
        ("a, b = 1;", &["a"]),
        ("a = 1, b;", &["a = 1, b"]),
        ("b++ + 1;", &["b++ + 1"]),
        ("(a = 1, b);", &["(a = 1, b)"]),
        ("(void) a, b = 1;", &["(void) a"]),
        ("for (a; a < 1; b) c = 1;", &["a", "b"]),
        ("for (a = 0, b; a < 1; c = 1, b) c = 1;", &["b", "b"]),
        ("if (a, b) c = 1;", &["a"]),
    ] {
        assert_eq!(warned(body), spans, "{body}");
    }
    for body in [
        "f();",
        "a = 1;",
        "a += 1;",
        "++a;",
        "a++;",
        "(a++);",
        "a++, b--;",
        "(void) a;",
        "f(), a = 1;",
        "a = b ? c : a;",
        "for (a = 0; a < 1; a++) c = 1;",
        "for (;;) break;",
        // The first condition's call sets the flag, and no expression statement clears it
        // for good, so the `a` of the second is not warned.
        "if (f()) c = 1;\nif (a, b) c = 1;",
        // The call sets the flag before its arguments are read.
        "x = f((a, b));",
    ] {
        assert_eq!(warned(body), Vec::<String>::new(), "{body}");
    }
    // Vanilla has no such warning.
    assert!(
        parse(in_body("a;").as_bytes())
            .diagnostics()
            .iter()
            .all(|d| d.code != Code::W0313)
    );
    assert!(!Code::W0313.is_error());
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

// spec FTEF-18.31: `__until (e) s` and `do s __until (e);`, and `until (e)` is a call when not a keyword
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

// spec FTEF-15.9: `(name)` after a function type is the declarator's parenthesised name
#[test]
fn a_parenthesised_name_after_a_function_type() {
    assert!(fte_codes("typedef void() (fn);\nfn g;\n").is_empty());
    assert!(fte_codes("typedef float() (fn); fn g = { return 1; };\n").is_empty());
    assert!(fte_codes("void() (g) = {};\n").is_empty());
    assert!(fte_codes("void() (g), (h);\n").is_empty());
    let parsed = fte_ok("void() (g) = {};\n");
    let names: Vec<String> = parsed
        .tree()
        .unwrap()
        .declarations()
        .filter_map(|d| d.declarators().next())
        .filter_map(|d| d.name().map(|t| t.text_lossless()))
        .collect();
    assert_eq!(names, ["g"]);
}

// spec FTEF-15.9: before a type's name `(name)` is a parameter list
// spec FTEF-18.36: a further parameter list after a type's is the declarator's, which needs a
// name
#[test]
fn a_further_parameter_list_needs_a_name() {
    assert!(fte_codes("void();\n").is_empty());
    for source in [
        "void()();\n",
        "void() (float);\n",
        "typedef float t; void() (t);\n",
    ] {
        assert!(fte_codes(source).contains(&Code::E0201), "{source}");
    }
}
