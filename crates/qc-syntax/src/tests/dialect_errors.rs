// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC parse on malformed and cut-off input: each error is reported, the tree keeps
//! every byte, and the parse goes on (QCD-1.4); and the valid forms beside them.

use crate::ast::{AstNode as _, SimpleType};
use crate::{Code, Dialect, Parse, SyntaxKind, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The error codes of `source` under the FTEQCC dialect, after checking that the tree gives
/// back every byte and every diagnostic is inside the source.
fn fte_errors(source: &str) -> Vec<Code> {
    let parsed = fte(source);
    assert_eq!(parsed.syntax().text(), source.as_bytes(), "{source}");
    for d in parsed.diagnostics() {
        assert!(d.span.end <= source.len(), "{source}: {d:?}");
    }
    parsed.errors().map(|d| d.code).collect()
}

/// `body` as the statements of a function, with globals for it to use.
fn in_body(body: &str) -> String {
    format!("float a, b, c;\nentity e;\nvoid() f =\n{{\n\t{body}\n}};\n")
}

/// The kinds of the file-level nodes of `source` under the FTEQCC dialect.
fn top_kinds(source: &str) -> Vec<SyntaxKind> {
    fte(source)
        .syntax()
        .children()
        .map(|n| n.kind())
        .filter(|k| *k != SyntaxKind::Directive)
        .collect()
}

// spec QCD-1.4: a declaration, a body or a construct cut off by the end of the file is an error,
// and the tree keeps every byte
#[test]
fn input_cut_off_at_the_end() {
    for source in [
        "typedef",
        "typedef struct",
        "struct s {",
        "class C {",
        "class C : D { float x;",
        "accessor A : float {",
        "accessor A : float { get",
        "enum E : float",
        "__deprecated(\"why\"",
        "void(float a",
        "void() f = { a = (",
        "void() f = { a = (Thing(float",
        "void() f = { a = (Thing(",
        "void() f = { a = sizeof",
        "void() f = { Thing(float x",
        "void() f = { a",
        "void() f = { a =",
        "void() f = { float(",
        "void() f = { for (",
        "void() f = { switch (a) {",
        "void() f = [1, g",
        "void() f = [++(1",
    ] {
        assert!(!fte_errors(source).is_empty(), "{source}");
    }
    // A class or an accessor with its `}` and no `;` before the end is whole.
    assert_eq!(fte_errors("class C { }"), []);
    assert_eq!(fte_errors("accessor A : float { get float v = f; }"), []);
}

// spec QCD-1.4: a word that starts no type where a type belongs is "not a type"
#[test]
fn not_a_type() {
    for source in [
        "typedef 1 x;\n",
        "typedef static x;\n",
        "typedef for x;\n",
        "goto x;\n",
        "void(1) f;\n",
        "class C { 1; };\n",
        "accessor A : float { get 1; };\n",
    ] {
        assert!(fte_errors(source).contains(&Code::E0203), "{source}");
    }
}

// spec QCD-1.4: a missing name, `;`, `)`, `]` or `}` is reported and the next declaration is
// read
#[test]
fn missing_tokens_are_reported() {
    for source in [
        "enum;\nfloat z;\n",
        "typedef float T x;\nfloat z;\n",
        "void(float a f;\nfloat z;\n",
        "void(;) f;\nfloat z;\n",
        "enum E : float x;\nfloat z;\n",
        "enum { A B };\nfloat z;\n",
        "__deprecated(\"why\"; float x;\nfloat z;\n",
        "accessor A : float { foo float x; };\nfloat z;\n",
    ] {
        assert!(!fte_errors(source).is_empty(), "{source}");
        assert_eq!(
            top_kinds(source).last(),
            Some(&SyntaxKind::Declaration),
            "{source}"
        );
    }
}

// spec FTEF-15.6: `T a, *b` and a struct's `*p` declare pointers, an unnamed struct among them
// spec FTEF-4.1: `T * const p` is a constant pointer
#[test]
fn pointer_declarators() {
    for source in [
        "struct *p;\n",
        "struct { float a; } *p;\n",
        "float * const p;\n",
        "struct s { float a, b; };\n",
    ] {
        assert_eq!(fte_errors(source), [], "{source}");
    }
}

// spec FTEF-15.3: a function-pointer parameter may be an array and may leave out its list
#[test]
fn function_pointer_parameters() {
    for source in [
        "void(float (*cb[2])(float)) f;\n",
        "void(float (*cb)) f;\n",
        "void(.float f) g;\n",
    ] {
        assert_eq!(fte_errors(source), [], "{source}");
    }
    assert!(!fte_errors("void(float &(x)) f;\n").is_empty());
}

// spec FTEF-9.3: a class body may hold empty members and field members
#[test]
fn class_body_members() {
    assert_eq!(fte_errors("class C { ; .float f; };\n"), []);
}

// spec FTEF-10.2: an accessor member may leave out its function, and its index its name
#[test]
fn accessor_member_forms() {
    assert_eq!(
        fte_errors("accessor A : float { get float v; set float[float] = g; };\n"),
        []
    );
    assert!(!fte_errors("accessor A : float { get float v[1] = g; };\n").is_empty());
}

// spec FTEF-16.2: `(.T) e` and `(..T) e` are casts to field types, and a parenthesised number
// or statement word is no cast
#[test]
fn cast_edges() {
    assert_eq!(fte_errors(&in_body("a = (.float) b;")), []);
    assert_eq!(fte_errors(&in_body("a = (..float) b;")), []);
    assert_eq!(fte_errors(&in_body("a = (1);")), []);
    assert!(!fte_errors(&in_body("a = (goto) b;")).is_empty());
}

// spec FTEF-18.3: a state's next function may be any expression, and a header that goes wrong
// is skipped to its `]` or to the body's `{`
#[test]
fn state_header_edges() {
    assert_eq!(fte_errors("void() f = [1, g()] {};\n"), []);
    assert!(!fte_errors("void() f = [1, g x] {};\nfloat z;\n").is_empty());
    assert!(!fte_errors("void() f = [1, g x {};\nfloat z;\n").is_empty());
    assert_eq!(
        top_kinds("void() f = [1, g x] {};\nfloat z;\n"),
        [SyntaxKind::Declaration, SyntaxKind::Declaration]
    );
}

// spec FTEF-18.19: the `(w)` after `++` must hold `w` or `W`
#[test]
fn frame_cycle_weapon_letter() {
    assert_eq!(fte_errors("void() f = [++(W) 1 .. 2] {};\n"), []);
    assert_eq!(fte_errors("void() f = [++(1) 1 .. 2] {};\n"), [Code::E0204]);
}

// spec FTEF-18.23: a C label's `:` follows its name at once; `a : b` is no label
#[test]
fn label_colon_must_touch() {
    assert!(!fte_errors(&in_body("a : b;")).is_empty());
    assert_eq!(fte_errors(&in_body("a: b = 1;")), []);
}

// spec FTEF-18.9: only `__until` is the loop keyword; another `__` word is a name
#[test]
fn only_until_is_a_keyword() {
    assert_eq!(fte_errors(&in_body("__foo(a);")), []);
}

// spec FTEF-15.7: a scoped member name past FTEQCC's length limit is "name too long"
#[test]
fn long_scoped_member_name() {
    let long = "m".repeat(crate::FTEQCC_MAX_NAME - 1);
    assert_eq!(
        fte_errors(&in_body(&format!("a = A::{long};"))),
        [Code::E0202]
    );
}

// spec FTEF-16.5: `if not` and `if !` need their parentheses
#[test]
fn if_not_needs_parentheses() {
    assert!(!fte_errors(&in_body("if not a b = 1;")).is_empty());
    assert!(!fte_errors(&in_body("if !a b = 1;")).is_empty());
}

// spec FTE-2.2: a `for` declaration may declare several locals
#[test]
fn for_declares_several_locals() {
    assert_eq!(
        fte_errors(&in_body("for (float i = 0, j = 1; i < j; i++) a = 1;")),
        []
    );
    assert_eq!(
        fte_errors(&in_body("for (a = 0, .float g = fl; a < 1; a++) b = 1;")),
        []
    );
    assert_eq!(
        fte_errors(&in_body("for (a = 0, 1; a < 1; a++) b = 1;")),
        []
    );
}

// spec FTE-3.1: a switch body's labels and statements end at a declaration line when its `}`
// is missing
#[test]
fn switch_body_ends_at_a_declaration_line() {
    let source = "void() f =\n{\n\tswitch (a) {\n\tcase 1:\nfloat z;\n";
    assert!(!fte_errors(source).is_empty());
    assert_eq!(top_kinds(source).last(), Some(&SyntaxKind::Declaration));
}

// spec FTEF-16.11: aggregates nest, and an element that is no expression ends the list
#[test]
fn aggregate_edges() {
    assert_eq!(fte_errors("float t[2][1] = { { 1 }, { 2 } };\n"), []);
    assert!(!fte_errors("float t[2] = { ; };\n").is_empty());
    let deep = format!("float t = {}1{};\n", "{".repeat(300), "}".repeat(300));
    assert!(fte_errors(&deep).contains(&Code::E0509));
}

// spec QC-27.15: a field type nested past the limit is "nesting too deep" under the FTEQCC
// profile too
#[test]
fn deep_field_types() {
    let source = format!("{}float f;\n", ". ".repeat(300));
    assert!(fte_errors(&source).contains(&Code::E0509));
}

// spec FTE-21.1: a body's statement that starts with a type word followed by no name, or by a
// parameter list longer than the parser looks ahead, is no declaration line
#[test]
fn declaration_shapes_in_a_body() {
    use crate::SyntaxKind as K;
    let kinds = |body: &str| -> Vec<K> {
        let parsed = fte(&in_body(body));
        let block = parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == K::Block)
            .unwrap();
        block.children().map(|n| n.kind()).collect()
    };
    assert_eq!(kinds("float = 3;"), [K::ExprStmt]);
    assert_eq!(fte_errors(&in_body("float = 3;")), []);
    // At column 0, a list longer than the lookahead does not end the body: the statement, an
    // error, is read in it, and the next declaration after the body.
    let long = format!(
        "void() f =\n{{\nfloat({}) x;\n}};\nfloat z;\n",
        "a, ".repeat(300)
    );
    assert!(!fte_errors(&long).is_empty());
    assert_eq!(
        top_kinds(&long),
        [SyntaxKind::Declaration, SyntaxKind::Declaration]
    );
}

/// The statement kinds of the body of `f` in `in_body(body)`.
fn body_kinds(body: &str) -> Vec<SyntaxKind> {
    let parsed = fte(&in_body(body));
    let block = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::Block)
        .unwrap();
    block.children().map(|n| n.kind()).collect()
}

// spec FTEF-16.1: a statement that starts with a qualifier or a C type word declares locals,
// whatever follows it
#[test]
fn qualifier_or_c_word_alone_declares() {
    assert_eq!(body_kinds("int;"), [SyntaxKind::LocalDecl]);
    assert_eq!(body_kinds("static;"), [SyntaxKind::LocalDecl]);
}

// spec FTEF-16.1: in a `for` clause too, a name followed by a name declares a local, whatever
// follows them
#[test]
fn a_for_clause_type_and_name_declare() {
    let clause = fte(&in_body("for (Thing t) a = 1;"))
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ForClause)
        .unwrap();
    assert!(clause.children().any(|n| n.kind() == SyntaxKind::LocalDecl));
}

// spec FTEF-2.4: only the C type words combine; `float` is a type of its own name, so a C word
// after it is the declarator's name
#[test]
fn a_c_word_after_a_vanilla_type_is_a_name() {
    let ty = fte("float long x;\n")
        .syntax()
        .descendants()
        .find_map(SimpleType::cast)
        .unwrap();
    assert_eq!(ty.text(), "float");
}

// spec FTEF-15.3: a function-pointer parameter is `(*name)`; a `(` without the `*` ends the
// parameter
#[test]
fn a_parenthesis_without_a_star_is_no_function_pointer() {
    assert_eq!(
        fte_errors("void(float &(x)) f;\n").first(),
        Some(&Code::E0204)
    );
}

// spec FTEF-9.3: only `public`, `private`, `protected`, `virtual` and `nonvirtual` are labels
// spec FTEF-9.4: only a constructor, of function type, may have no name
#[test]
fn class_labels_and_nameless_members() {
    assert!(!fte_errors("class C {\n\tfoo:\n};\n").is_empty());
    assert!(!fte_errors("class C {\n\tfloat;\n};\n").is_empty());
    assert_eq!(fte_errors("class C {\n\tC(void);\n};\n"), []);
}

// spec FTEF-18.3: the rest of a state header that went wrong is one error node
#[test]
fn a_bad_state_header_is_an_error_node() {
    let parsed = fte("void() f = [1, g x y] {};\n");
    let state = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::State)
        .unwrap();
    let errors: Vec<String> = state
        .children()
        .filter(|n| n.kind() == SyntaxKind::Error)
        .map(|n| String::from_utf8_lossy(&n.text()).trim().to_owned())
        .collect();
    assert_eq!(errors, ["x y"]);
}
