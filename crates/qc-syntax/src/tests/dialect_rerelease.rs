// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC constructs the rerelease uses beyond FTE-2 to FTE-16
//! (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-15.4 and FTE-17 to FTE-21): each parses under
//! the FTEQCC profile and is a diagnostic under the vanilla profile.

use super::sexpr;
use crate::ast::{self, AstNode as _, BinaryOp, UnaryOp};
use crate::{Code, Dialect, Parse, SyntaxKind, parse, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The error codes of `source` under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<Code> {
    fte(source).errors().map(|d| d.code).collect()
}

/// Every diagnostic code, warnings included, of `source` under the FTEQCC dialect.
fn fte_codes(source: &str) -> Vec<Code> {
    fte(source).diagnostics().iter().map(|d| d.code).collect()
}

/// The error codes of `source` under the vanilla dialect.
fn vanilla_errors(source: &str) -> Vec<Code> {
    parse(source.as_bytes()).errors().map(|d| d.code).collect()
}

/// Asserts `source` parses with no error under the FTEQCC dialect and keeps its bytes.
fn fte_ok(source: &str) -> Parse {
    let parsed = fte(source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new(),
        "{source}\n{}",
        parsed.syntax().debug_tree()
    );
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    parsed
}

/// `body` as the statements of a function, indented, with globals for it to use. A line in
/// column 0 that looks like a declaration would end the body (the parser's recovery rule).
fn in_body(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
    format!("float a, b, c;\nentity e;\n.float fl;\nvoid() f =\n{{\n{indented}}};\n")
}

/// The first statement of the function `f` of `parsed`.
fn first_statement(parsed: &Parse) -> ast::Stmt {
    let file = parsed.tree().unwrap();
    let body = file
        .declarations()
        .filter_map(|d| d.declarators().next())
        .find(|d| d.name().is_some_and(|n| n.text() == b"f"))
        .and_then(|d| d.initializer());
    let Some(ast::Initializer::Body(body)) = body else {
        panic!("f has no body");
    };
    body.block().unwrap().statements().next().unwrap()
}

/// The s-expression of the first statement of a body, an expression statement, under FTEQCC.
fn fte_expr(stmt: &str) -> String {
    let parsed = fte_ok(&in_body(stmt));
    match first_statement(&parsed) {
        ast::Stmt::Expr(expr) => sexpr(&expr.expr().unwrap()),
        other => panic!("not an expression statement: {other:?}"),
    }
}

/// The binary operator of the first statement of a body, an expression statement.
fn fte_op(stmt: &str) -> Option<BinaryOp> {
    let parsed = fte_ok(&in_body(stmt));
    let ast::Stmt::Expr(expr) = first_statement(&parsed) else {
        panic!("not an expression statement");
    };
    let Some(ast::Expr::Binary(bin)) = expr.expr() else {
        panic!("not a binary expression");
    };
    bin.op()
}

/// The first declarator of the first declaration of `parsed`.
fn first_declarator(parsed: &Parse) -> ast::Declarator {
    let file = parsed.tree().unwrap();
    file.declarations()
        .next()
        .unwrap()
        .declarators()
        .next()
        .unwrap()
}

// spec FTE-17.1: `rettype name(params)` declares a function: the declarator carries its own
// parameter list, for a prototype, a builtin or a definition.
#[test]
fn c_style_function_declarators_parse() {
    let parsed = fte_ok("void makevectorsfixed(vector ang);\n");
    let declarator = first_declarator(&parsed);
    assert_eq!(declarator.name().unwrap().text(), b"makevectorsfixed");
    let params: Vec<_> = declarator.params().unwrap().params().collect();
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name().unwrap().text(), b"ang");
    fte_ok("float checkextension( string s ) = #99;\n");
    fte_ok("void InitLightStyles() = { };\n");
    // A vanilla-order declarator has no parameter list of its own.
    assert!(first_declarator(&fte_ok("void() g;\n")).params().is_none());
    // Vanilla wants `;` after the name.
    assert_eq!(
        vanilla_errors("void makevectorsfixed(vector ang);"),
        [Code::E0204]
    );
}

// spec FTE-17.2: a function's body may follow its declarator without `=`, in either order.
#[test]
fn function_bodies_need_no_equals() {
    for source in [
        "void InitLightStyles() { }\n",
        "void(entity e) remove { remove_real(e); }\nvoid(entity e) remove_real = #15;\n",
        "void() think [1, think] { }\n",
    ] {
        let parsed = fte_ok(source);
        let init = first_declarator(&parsed).initializer();
        assert!(matches!(init, Some(ast::Initializer::Body(_))), "{source}");
    }
    assert_eq!(vanilla_errors("void() g { };"), [Code::E0204]);
}

// spec FTE-17.3: the `;` after a function declaration is optional, prototype or body, and a
// `;` between definitions is skipped.
#[test]
fn function_semicolons_are_optional() {
    fte_ok("void f()\nvoid g()\nfloat x;\n");
    fte_ok("void f() { }\nvoid() g = { };\n;;\nfloat x;\n");
    assert_eq!(fte_codes("void() f = { }\nfloat x;\n"), Vec::<Code>::new());
    assert_eq!(vanilla_errors("void() f = { }\nfloat x;"), [Code::E0204]);
    assert_eq!(vanilla_errors(";"), [Code::E0203]);
}

// spec FTE-17.4: a builtin is `#n` or `#n:name`, and `n` may be 0.
#[test]
fn named_builtins_parse() {
    let parsed = fte_ok("void(string s) bprint = #0:ex_bprint;\n");
    let Some(ast::Initializer::Builtin(builtin)) = first_declarator(&parsed).initializer() else {
        panic!("no builtin");
    };
    assert_eq!(builtin.number().unwrap().text(), b"0");
    assert!(
        builtin
            .syntax()
            .child_tokens()
            .any(|t| t.kind() == SyntaxKind::Name && t.text() == b"ex_bprint")
    );
    fte_ok("float() finaleFinished = #0:ex_finaleFinished;\n");
    fte_ok("void() crash = #6;\n");
    assert_eq!(vanilla_errors("float() f = #0;"), [Code::E0507]);
}

/// The number's text and the engine name of the builtin initialiser of the first declarator
/// of `source`, parsed under FTEQCC with no error.
fn fte_builtin(source: &str) -> (Option<String>, Option<String>) {
    let parsed = fte_ok(source);
    let Some(ast::Initializer::Builtin(builtin)) = first_declarator(&parsed).initializer() else {
        panic!("no builtin in {source}");
    };
    (
        builtin.number().map(|t| t.text_lossless()),
        builtin.engine_name(qc_lexer::LexFlags::default()),
    )
}

// spec FTE-17.5: a string after `#` or `:`, or a name after `:`, is the builtin's engine name,
// and `:` takes a number as `#` does
#[test]
fn string_and_colon_builtins_parse() {
    let name = |n: &str| (None, Some(n.to_owned()));
    for (source, expected) in [
        (
            r#"void(string s, ...) bp = #"ex_bprint";"#,
            name("ex_bprint"),
        ),
        ("void(string s, ...) br = :ex_bprint3;", name("ex_bprint3")),
        (
            r#"void(string s, ...) bs = :"ex_bprint4";"#,
            name("ex_bprint4"),
        ),
        (r#"void() e = #"a\tb";"#, name("a\tb")),
        ("void(string s, ...) bq = :5;", (Some("5".to_owned()), None)),
        (
            "void(string s, ...) bt = :0:ex_bprint5;",
            (Some("0".to_owned()), Some("ex_bprint5".to_owned())),
        ),
        ("void(string s, ...) bx :ex_bprint6;", name("ex_bprint6")),
    ] {
        assert_eq!(fte_builtin(&format!("{source}\n")), expected, "{source}");
    }
}

// spec FTE-17.5: a name is read after a string or a name only by FTE-17.4's `#number:`, a name
// directly after `#` is no builtin, and only a function takes `:`
#[test]
fn string_and_colon_builtins_take_no_second_name() {
    for source in [
        r#"void(string s, ...) bu = #"ex_a":ex_b;"#,
        "void(string s, ...) bv = :ex_a:ex_b;",
        "float fx = :ex_bprint7;",
    ] {
        assert_ne!(fte_errors(source), Vec::<Code>::new(), "{source}");
    }
    assert_eq!(fte_errors("void() bw = #ex_bprint2;"), [Code::E0209]);
}

// spec FTE-17.6: FTEQCC binds an `i` literal of any size and a float literal inside int, and
// takes no `u` or `l` literal or float outside int
#[test]
fn builtin_numbers_by_literal_type() {
    for number in [
        "16i",
        "16I",
        "16777217i",
        "0x1000001i",
        "4294967313i",
        "123456789",
        "2147483520",
        "-2147483648",
        "0x7FFFFF80",
    ] {
        let source = format!("void() f = #{number};\n");
        assert_eq!(fte_errors(&source), Vec::<Code>::new(), "{number}");
    }
    for number in [
        "16u",
        "16U",
        "16l",
        "16ll",
        "16ul",
        "2147483647",
        "2147483648",
        "0x80000000",
        "-2147483904",
    ] {
        let source = format!("void() f = #{number};\n");
        assert_eq!(fte_errors(&source), [Code::E0209], "{number}");
    }
}

// spec FTE-17.5: under the vanilla profile the string and colon forms stay errors
#[test]
fn vanilla_has_no_string_or_colon_builtins() {
    assert_eq!(
        vanilla_errors(r#"void() bp = #"ex_bprint";"#),
        [Code::E0209]
    );
    assert_ne!(
        vanilla_errors("void() br = :ex_bprint;"),
        Vec::<Code>::new()
    );
    assert_ne!(vanilla_errors("void() bx :ex_bprint;"), Vec::<Code>::new());
}

// spec FTE-18.1: `...` may end a parameter list after named parameters.
#[test]
fn varargs_after_named_parameters_parse() {
    let parsed = fte_ok("void(string s, ...) bprint = #0:ex_bprint;\n");
    let Some(ast::Type::Function(func)) =
        parsed.tree().unwrap().declarations().next().unwrap().ty()
    else {
        panic!("not a function type");
    };
    let list = func.params().unwrap();
    assert!(list.is_variadic());
    assert_eq!(list.params().count(), 1);
    assert_eq!(vanilla_errors("void(string s, ...) bprint;"), [Code::E0203]);
}

// spec FTE-18.2: a parameter may have a default value, a cast included.
#[test]
fn default_parameter_values_parse() {
    let parsed = fte_ok(
        "float(entity e) SUB_True = { return 1; };\n\
         float SUB_Count(entity e, float(entity e) predicate = (float(entity e))SUB_True);\n",
    );
    let file = parsed.tree().unwrap();
    let declarator = file
        .declarations()
        .nth(1)
        .unwrap()
        .declarators()
        .next()
        .unwrap();
    let params: Vec<_> = declarator.params().unwrap().params().collect();
    assert!(params[0].default_value().is_none());
    assert!(matches!(
        params[1].default_value(),
        Some(ast::Expr::Cast(_))
    ));
    fte_ok("void(float NoKeys = 0) CheatCommand = { };\n");
    assert_eq!(vanilla_errors("void(float k = 0) g;"), [Code::E0204]);
}

// spec FTE-18.3: a parameter may have a type the program names.
#[test]
fn user_typed_parameters_parse() {
    fte_ok("enum UpgradeType { UT_A, UT_B };\nfloat GetUpgradeAmount(UpgradeType Type) { };\n");
    assert!(!vanilla_errors("float(UpgradeType t) g;").is_empty());
}

// spec FTE-19.1: `^`, `<<`, `>>` and the assignments `&=`, `|=`, `^=`, `&~=`, `<<=` and `>>=`.
#[test]
fn further_binary_operators_parse() {
    assert_eq!(fte_op("a = b ^ c;"), Some(BinaryOp::Assign));
    assert_eq!(fte_expr("a ^ b;"), "(^ a b)");
    assert_eq!(fte_expr("a << 2;"), "(<< a 2)");
    assert_eq!(fte_expr("a >> 2;"), "(>> a 2)");
    assert_eq!(fte_op("a &= b;"), Some(BinaryOp::BitAndAssign));
    assert_eq!(fte_op("a |= b;"), Some(BinaryOp::BitOrAssign));
    assert_eq!(fte_op("a ^= b;"), Some(BinaryOp::BitXorAssign));
    assert_eq!(fte_op("a &~= b;"), Some(BinaryOp::BitClearAssign));
    assert_eq!(fte_op("a <<= b;"), Some(BinaryOp::ShlAssign));
    assert_eq!(fte_op("a >>= b;"), Some(BinaryOp::ShrAssign));
    assert_eq!(fte_op("a += b;"), Some(BinaryOp::AddAssign));
    assert_eq!(fte_op("a % b;"), Some(BinaryOp::Rem));
    assert!(vanilla_errors(&in_body("a = b ^ c;")).contains(&Code::E0105));
}

// spec FTE-19.2: `(+)` is `|=`, `(-)` is `&~=` and `e->f` is `e.f`.
#[test]
fn remapped_operators_parse() {
    assert_eq!(fte_op("e.fl (+) b;"), Some(BinaryOp::BitOrAssign));
    assert_eq!(fte_op("e.fl (-) b;"), Some(BinaryOp::BitClearAssign));
    assert_eq!(fte_expr("e->fl = 0;"), "(= (. e fl) 0)");
    assert!(!vanilla_errors(&in_body("e->fl = 0;")).is_empty());
}

// spec FTE-19.3: shifts, `^`, `&` and `|` bind as tightly as `*`, one tighter than `+`, and
// the compound assignments are right-associative like `=`.
#[test]
fn further_operator_priorities() {
    assert_eq!(fte_expr("a + b ^ c;"), "(+ a (^ b c))");
    assert_eq!(fte_expr("a << 1 + b;"), "(+ (<< a 1) b)");
    assert_eq!(fte_expr("a * b >> c;"), "(>> (* a b) c)");
    assert_eq!(fte_expr("a |= b = c;"), "(|= a (= b c))");
    assert_eq!(fte_expr("a = b &~= c;"), "(= a (&~= b c))");
}

// spec FTE-19.4: `-`, `+` and `~` start a term; `-` and `+` take a term, `~` an operand of
// `!`'s priority.
#[test]
fn prefix_operators_parse() {
    assert_eq!(
        fte_expr("a = -fabs(b);"),
        "(= a (unary Some(Neg) (call fabs b)))"
    );
    assert_eq!(fte_expr("a = -b * c;"), "(= a (* (unary Some(Neg) b) c))");
    assert_eq!(fte_expr("a = +b;"), "(= a (unary Some(Plus) b))");
    assert_eq!(
        fte_expr("a = ~b & c;"),
        "(= a (unary Some(BitNot) (& b c)))"
    );
    assert_eq!(fte_expr("a = -e.fl;"), "(= a (unary Some(Neg) (. e fl)))");
    let parsed = fte_ok(&in_body("a = ~b;"));
    let ast::Stmt::Expr(stmt) = first_statement(&parsed) else {
        panic!("not an expression statement");
    };
    let Some(ast::Expr::Binary(assign)) = stmt.expr() else {
        panic!("not an assignment");
    };
    let Some(ast::Expr::Unary(unary)) = assign.rhs() else {
        panic!("not a prefix operator");
    };
    assert_eq!(unary.op(), Some(UnaryOp::BitNot));
    assert_eq!(vanilla_errors(&in_body("a = -b;")), [Code::E0201]);
}

// spec FTE-19.5: a negative number where an operator belongs is a subtraction, at the
// priority of `+`.
#[test]
fn negative_number_after_an_operand_subtracts() {
    assert_eq!(fte_expr("a = b -1;"), "(= a ( b -1))");
    assert_eq!(fte_op("b -1;"), Some(BinaryOp::Add));
    assert_eq!(fte_expr("a = b * c -1;"), "(= a ( (* b c) -1))");
    assert_eq!(fte_expr("a = b -1 * c;"), "(= a ( b (* -1 c)))");
    assert_eq!(fte_expr("a = -1;"), "(= a -1)");
    assert_eq!(vanilla_errors(&in_body("a = b -1;")), [Code::E0204]);
}

// spec FTE-19.6: `(type) e` is a cast; a parenthesis around an expression is not.
#[test]
fn casts_parse() {
    assert_eq!(fte_expr("a = (float) b;"), "(= a (cast b))");
    assert_eq!(fte_expr("a = (float(entity x)) b;"), "(= a (cast b))");
    assert_eq!(fte_expr("a = (b) * c;"), "(= a (* (b) c))");
    assert_eq!(fte_expr("a = (float) b * c;"), "(= a (* (cast b) c))");
    assert!(!vanilla_errors(&in_body("a = (float) b;")).is_empty());
}

// spec FTE-21.1: a statement that starts with a type, a user type or `.` declares locals with
// no `local`, anywhere in the body.
#[test]
fn locals_need_no_local_keyword() {
    for body in [
        "a = 1;\nfloat x = a;\nb = x;",
        "poseInfo pose = poses[1];",
        "a = 1; vector v = '0 0 0', w;",
        ".float lf;",
    ] {
        let parsed = fte_ok(&in_body(body));
        let file = parsed.tree().unwrap();
        let locals = file
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LocalDecl)
            .count();
        assert_eq!(locals, 1, "{body}");
    }
    assert_eq!(vanilla_errors(&in_body("float x = a;")), [Code::E0204]);
}

// spec FTE-21.2: a local may be a field or a function variable.
#[test]
fn field_and_function_locals_parse() {
    fte_ok(&in_body("void() tutsh;\ntutsh = f;"));
    fte_ok(&in_body("local void() pain_event;"));
    fte_ok(&in_body("local .float lf;"));
    assert_eq!(vanilla_errors(&in_body("local void() g;")), [Code::E0208]);
}

// spec FTE-21.3: a definition that is not a function may lack its `;`, with a warning.
// spec QCD-7.18: the warning is W0203 "Missing semicolon at end of definition".
#[test]
fn a_missing_semicolon_after_a_definition_warns() {
    let parsed = fte(&in_body("local float color = a + b * 16\nc = color;"));
    assert_eq!(parsed.errors().count(), 0);
    let warnings: Vec<_> = parsed
        .diagnostics()
        .iter()
        .map(|d| (d.code, d.message.as_str()))
        .collect();
    assert_eq!(
        warnings,
        [(Code::W0203, "Missing semicolon at end of definition")]
    );
    assert!(!Code::W0203.is_error());
    assert_eq!(fte_codes("float x\nfloat y;\n"), [Code::W0203]);
    assert_eq!(
        vanilla_errors(&in_body("local float x = 1\nc = 1;")),
        [Code::E0204]
    );
}

// spec FTE-21.4: a nested block may be empty.
#[test]
fn empty_blocks_parse() {
    fte_ok(&in_body("if (a) { } else { b = 1; }"));
    fte_ok(&in_body("{ }"));
    assert_eq!(vanilla_errors(&in_body("if (a) { }")), [Code::E0201]);
}

// spec FTE-21.5: a `;` alone is an empty statement, with a warning.
// spec QCD-7.19: the warning is W0204 "Hanging ';'".
#[test]
fn a_hanging_semicolon_is_an_empty_statement() {
    let parsed = fte(&in_body("if (a) return;;\nb = 1;"));
    assert_eq!(parsed.errors().count(), 0);
    let diagnostics: Vec<_> = parsed
        .diagnostics()
        .iter()
        .map(|d| (d.code, d.message.as_str()))
        .collect();
    assert_eq!(diagnostics, [(Code::W0204, "Hanging ';'")]);
    assert!(!Code::W0204.is_error());
    let empties = parsed
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::EmptyStmt)
        .count();
    assert_eq!(empties, 1);
    assert_eq!(vanilla_errors(&in_body("b = 1;;")), [Code::E0201]);
}

// spec FTE-15.4: `enum Name { ... }` names a type as well as its constants.
#[test]
fn named_enums_parse() {
    let parsed = fte_ok("enum UpgradeType\n{\n\tUT_Health,\n\tUT_Shells\n};\n");
    let file = parsed.tree().unwrap();
    let en = file
        .syntax()
        .children()
        .find_map(ast::EnumDecl::cast)
        .unwrap();
    assert_eq!(en.name().unwrap().text(), b"UpgradeType");
    assert_eq!(en.enumerators().count(), 2);
    let anonymous = fte_ok("enum { A, B };\n");
    let en = anonymous
        .tree()
        .unwrap()
        .syntax()
        .children()
        .find_map(ast::EnumDecl::cast)
        .unwrap();
    assert!(en.name().is_none());
    let class = fte_ok("enum class Kind : int { K_A };\n");
    let en = class
        .tree()
        .unwrap()
        .syntax()
        .children()
        .find_map(ast::EnumDecl::cast)
        .unwrap();
    assert_eq!(en.name().unwrap().text(), b"Kind");
    assert!(!vanilla_errors("enum UpgradeType { A };").is_empty());
}

// spec FTE-17.1, FTE-21.1: the parser keeps every byte of the FTEQCC constructs.
#[test]
fn fteqcc_constructs_are_lossless() {
    let source = "void f(float a, ...) { float x = -a; x (+) 1; x = x -1; ; { } }\n";
    assert_eq!(fte(source).syntax().text(), source.as_bytes());
    assert_eq!(fte_errors(source), Vec::<Code>::new());
}

// spec FTEF-13.24: `*^` and `><` have the priority of `*`, and `<=>` that of `==`
#[test]
fn fteqcc_power_cross_and_spaceship_operators() {
    assert_eq!(fte_op("a *^ b;"), Some(BinaryOp::Pow));
    assert_eq!(fte_op("a >< b;"), Some(BinaryOp::Cross));
    assert_eq!(fte_op("a <=> b;"), Some(BinaryOp::Spaceship));
    assert_eq!(fte_expr("a + b *^ c;"), "(+ a (*^ b c))");
    assert_eq!(fte_expr("a >< b - c;"), "(- (>< a b) c)");
    assert_eq!(fte_expr("a <=> b + c;"), "(<=> a (+ b c))");
    assert!(!fte_errors(&in_body("a ^^ b;")).is_empty());
    assert!(!fte_errors(&in_body("@a;")).is_empty());
}

// spec FTEF-12.12: preprocessed text is lexed again with the flags FTEQCC's lexer read in each
// part of it, `macroinstrings` aside
#[test]
fn preprocessed_text_is_lexed_with_its_flags() {
    use qc_preproc::{FlagEdit, FteFlag};
    let source = b"string s = \"a\":\"b\";\nstring t = \"a\":\"b\";\n";
    let errors = |flags: &[(usize, FlagEdit)]| -> Vec<Code> {
        crate::parse_preprocessed_flagged(source, Dialect::Fteqcc, flags, &[], &[])
            .errors()
            .map(|d| d.code)
            .collect()
    };
    assert!(!errors(&[]).is_empty());
    let qccx = super::whole(&[(FteFlag::Qccx, true)]);
    let defaults = super::whole(&[]);
    assert!(errors(&[(0, qccx)]).is_empty());
    // The flags hold from their byte: the `:` before it is the punctuation.
    let second_line = source.iter().position(|&b| b == b'\n').unwrap() + 1;
    assert!(!errors(&[(0, defaults), (second_line, qccx)]).is_empty());
    assert!(!errors(&[(0, qccx), (second_line, defaults)]).is_empty());
    assert!(errors(&[(0, qccx), (second_line, qccx)]).is_empty());
    let parsed = crate::parse_preprocessed_flagged(source, Dialect::Fteqcc, &[(0, qccx)], &[], &[]);
    assert_eq!(parsed.syntax().text(), source);
    let ssp = super::whole(&[(FteFlag::Dblstarexp, true)]);
    let powers = |flags: &[(usize, FlagEdit)]| {
        crate::parse_preprocessed_flagged(b"float x = 2 ** 3;\n", Dialect::Fteqcc, flags, &[], &[])
            .syntax()
            .tokens()
            .filter(|t| t.kind() == SyntaxKind::StarCaret)
            .count()
    };
    assert_eq!(powers(&[(0, ssp)]), 1);
    assert_eq!(powers(&[]), 0);
    // `macroinstrings` is not read: the text has no macros to look up.
    let strings = super::whole(&[(FteFlag::Macroinstrings, true)]);
    let parsed = crate::parse_preprocessed_flagged(
        b"string s = \"#X\";\n",
        Dialect::Fteqcc,
        &[(0, strings)],
        &[],
        &[],
    );
    assert!(parsed.diagnostics().is_empty());
}
