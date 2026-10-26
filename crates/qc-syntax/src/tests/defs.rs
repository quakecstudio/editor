// SPDX-License-Identifier: GPL-3.0-or-later

//! The definitions `QCC_PR_ParseDefs` reads (`docs/spec/dialect-fteqcc.md` FTEF-21.17,
//! FTEF-20.14): an `extern` block's `;`s, definition and string, and the type names a
//! statement or a `for` initialiser starts a definition at, in any case under `caseinsens`.
//! Each program is also a case of the `defs` family of `cargo xtask oracles builtins`, which
//! compares the outcome with the pinned FTEQCC.

use qc_preproc::FteFlag;

use super::{fte_with, whole};
use crate::kind::SyntaxKind as K;
use crate::{Code, Dialect, Keywords, Parse, parse_preprocessed_flagged, parse_with};

/// `source` parsed under the FTEQCC profile.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// `source` parsed under the FTEQCC profile with `caseinsens` in force.
fn insensitive(source: &str) -> Parse {
    fte_with(source, Keywords::default().with_case_insensitive(true))
}

/// The messages of every error of `parsed`.
fn messages(parsed: &Parse) -> Vec<String> {
    parsed.errors().map(|d| d.message.clone()).collect()
}

/// The codes of every error of `parsed`.
fn codes(parsed: &Parse) -> Vec<Code> {
    parsed.errors().map(|d| d.code).collect()
}

/// How many nodes of `kind` `parsed` holds.
fn count(parsed: &Parse, kind: K) -> usize {
    parsed
        .syntax()
        .descendants()
        .filter(|n| n.kind() == kind)
        .count()
}

/// `body` as the statements of a function `g`.
fn in_body(body: &str) -> String {
    format!("void() g = {{ {body} }};\n")
}

// spec FTEF-21.17: `QCC_PR_ParseDefs` skips the `;`s and then reads a definition, so a block or
// an `extern "STRING"` that reaches the end of the text or a `}` with none is `"" is not a
// type` or `"}" is not a type`, at file level and in a body; a `;` after a definition and
// before the end of the text is the same
#[test]
fn a_block_needs_a_definition() {
    for source in [
        "extern \"C\"",
        "extern \"C\";\n",
        "extern \"C\" ;;;\n",
        "extern \"C\" \"QC\"",
        "float x;;\n",
        "void() g = {};;\n",
        "extern \"C\" { float x; } ;\n",
    ] {
        assert_eq!(messages(&fte(source)), ["\"\" is not a type"], "{source}");
    }
    for source in [
        "extern \"C\" { ; }\n",
        "extern \"C\" { float x; ;; }\n",
        "extern \"C\" { float x; ; }\n",
        &in_body("extern \"C\"; "),
        &in_body("extern \"C\" { ; }"),
        &in_body("extern \"C\" { float x; ; }"),
    ] {
        assert_eq!(messages(&fte(source)), ["\"}\" is not a type"], "{source}");
    }
    for source in [
        "extern \"C\" ;; float x;\n",
        "extern \"C\" { ;; float x; }\n",
        "extern \"C\" { ; float x; }\n",
        "float x;; float y;\n",
        "void() g = {};\n",
        "void g() {};\n",
        &in_body("extern \"C\" ; float x;"),
        &in_body("extern \"C\" { ;; float x; }"),
    ] {
        assert_eq!(codes(&fte(source)), [], "{source}");
    }
}

// spec FTEF-21.17: the string is compared as `strcasecmp(pr_token, ...)` compares it, up to its
// first NUL, so `"C\x00x"` is a `"C"` block, as is `"C":"x"` under `qccx`, whose `:` is a NUL
#[test]
fn the_linkage_ends_at_a_nul() {
    let bad_code = |parsed: Parse| codes(&parsed).contains(&Code::E0103);
    for linkage in ["\"C\\x00x\"", "\"c\\x00\"", "\"C\\x00\\x00QC\""] {
        let source = format!("extern {linkage} {{ string s = \"\\9\"; }}\n");
        assert!(bad_code(fte(&source)), "{source}");
    }
    for linkage in ["\"\\x00C\"", "\"Q\\x00C\"", "\"X\\x00C\""] {
        let source = format!("extern {linkage} {{ string s = \"\\9\"; }}\n");
        assert!(!bad_code(fte(&source)), "{source}");
    }
    let qccx = [(0, whole(&[(FteFlag::Qccx, true)]))];
    let flagged = |source: &str| {
        parse_preprocessed_flagged(source.as_bytes(), Dialect::Fteqcc, &qccx, &[], &[])
    };
    for linkage in ["\"C\":\"x\"", "\"C\": \"x\"", "\"C\":"] {
        let source = format!("extern {linkage} {{ string s = \"\\9\"; }}\n");
        assert!(bad_code(flagged(&source)), "{source}");
    }
    let source = "extern \"x\":\"C\" { string s = \"\\9\"; }\n";
    assert!(!bad_code(flagged(source)));
}

// spec FTEF-21.17: the string is the one token the lexer read, its adjacent parts joined; a
// string beside it that another text wrote, a macro's, is the definition's first token
#[test]
fn only_the_lexers_string_is_the_linkage() {
    let source = "extern \"C\" \"C\" { string s = \"\\9\"; }\n";
    let second = source.rfind("\"C\"").unwrap();
    let parse = |seams: &[usize]| {
        parse_preprocessed_flagged(source.as_bytes(), Dialect::Fteqcc, &[], &[], seams)
    };
    let joined = parse(&[]);
    assert_eq!(codes(&joined), []);
    assert_eq!(count(&joined, K::ExternBlock), 1);
    let apart = parse(&[second]);
    assert_eq!(messages(&apart)[0], "\"C\" is not a type");
    assert_eq!(count(&apart, K::ExternBlock), 1);
    // The first string is always the linkage, whatever the seams say.
    let first = source.find("\"C\"").unwrap();
    assert_eq!(codes(&parse(&[first])), codes(&joined));
    // A seam at no string is read as none.
    assert_eq!(codes(&parse(&[0, 3, source.len()])), codes(&joined));
}

// spec FTEF-21.17: a `typedef` in a block's body is a definition of the block, as a statement
#[test]
fn a_block_in_a_body_holds_typedefs() {
    let parsed = fte(&in_body("extern \"C\" { typedef x; }"));
    assert_eq!(codes(&parsed), []);
    assert_eq!(count(&parsed, K::TypedefDecl), 1);
}

// spec FTEF-20.14: under `caseinsens` a statement that starts with a type FTEQCC registers, in
// any case, defines locals, as `QCC_TypeForName` finds it
#[test]
fn registered_type_names_start_definitions_in_any_case() {
    for word in [
        "float",
        "Float",
        "FLOAT",
        "vector",
        "VECTOR",
        "string",
        "String",
        "entity",
        "Entity",
        "__int",
        "__Int",
        "__INT",
        "__uint8",
        "__Uint8",
        "__bint",
        "__BFLOAT",
        "__variant",
        "Variant",
        "__String",
        "__vector",
        "__Entity",
        "Void",
    ] {
        for body in [
            format!("{word} x;"),
            format!("{word} x, y;"),
            format!("{word} *x;"),
            format!("{word};"),
            format!("{word} x; {word} y;"),
            format!("for ({word} i; ; ) {{ break; }}"),
            format!("{word}() fn;"),
        ] {
            let source = in_body(&body);
            let parsed = insensitive(&source);
            assert_eq!(codes(&parsed), [], "{source}");
            assert!(
                count(&parsed, K::LocalDecl) + count(&parsed, K::ForStmt) > 0,
                "{source}"
            );
        }
    }
}

// spec FTEF-20.14: with `caseinsens` off a type is found by its exact name, so a vanilla type
// word in another case is no keyword
#[test]
fn registered_type_names_are_exact_without_caseinsens() {
    let tokens = |parsed: &Parse, kind: K| {
        parsed
            .syntax()
            .descendants_with_tokens()
            .filter(|e| e.kind() == kind)
            .count()
    };
    for (word, kind) in [
        ("Float", K::FloatKw),
        ("VECTOR", K::VectorKw),
        ("Entity", K::EntityKw),
        ("String", K::StringKw),
        ("Void", K::VoidKw),
    ] {
        let source = in_body(&format!("{word} x;"));
        let (exact, any_case) = (
            tokens(&fte(&source), kind),
            tokens(&insensitive(&source), kind),
        );
        assert!(exact < any_case, "{source}");
    }
    for body in [
        "float x;",
        "vector v;",
        "__int x;",
        "void;",
        "__variant;",
        "__string;",
    ] {
        let source = in_body(body);
        assert_eq!(codes(&fte(&source)), [], "{source}");
        assert_eq!(count(&fte(&source), K::LocalDecl), 1, "{source}");
    }
}

// spec FTEF-20.14: a name that is a type is no value: `Float = 1;` is a definition with no
// name, the error `QCC_PR_ParseDefs` gives, where a variable of that name is assigned in any
// other case
#[test]
fn a_type_name_is_not_assigned() {
    let source = in_body("Float = 1;");
    assert_ne!(codes(&insensitive(&source)), []);
    assert_eq!(count(&insensitive(&source), K::BinaryExpr), 0);
    assert_eq!(codes(&fte(&source)), []);
    assert_eq!(count(&fte(&source), K::LocalDecl), 0);
    let label = in_body("ENTITY:;");
    assert_ne!(codes(&insensitive(&label)), []);
    assert_eq!(codes(&fte(&label)), []);
}

// spec FTEF-20.14: a `for` initialiser reads a type with `QCC_PR_ParseType`, whose C type words
// are found in the case their keyword is, after a `,` too
#[test]
fn a_for_initialiser_reads_types_in_any_case() {
    for init in ["Integer i", "INTEGER i = 0", "Float i", "k = 0, Integer i"] {
        let source = in_body(&format!("local float k; for ({init}; ; ) {{ break; }}"));
        assert_eq!(codes(&insensitive(&source)), [], "{source}");
        assert_eq!(count(&insensitive(&source), K::LocalDecl), 2, "{source}");
        assert_eq!(count(&insensitive(&source), K::ForStmt), 1, "{source}");
    }
}

// spec FTEF-20.14: a type the program defines, by typedef or by a tag, starts a definition in the
// scope that defined it, in any case under `caseinsens`
#[test]
fn a_programs_types_start_definitions() {
    for define in [
        "typedef float Foo;\n",
        "struct Foo { float a; };\n",
        "enum Foo { A };\n",
        "class Foo { float a; };\n",
    ] {
        for word in ["Foo", "foo", "FOO"] {
            let exact = word == "Foo";
            let source = format!("{define}{}", in_body(&format!("{word} = 1;")));
            let found =
                |parsed: &Parse| count(parsed, K::LocalDecl) > 0 || !codes(parsed).is_empty();
            assert!(found(&insensitive(&source)), "{source}");
            assert_eq!(codes(&fte(&source)).is_empty(), !exact, "{source}");
        }
    }
    // A type the file defines is every later function's.
    let source = "typedef float Foo;\nvoid() g = {};\nvoid() h = { Foo = 1; };\n";
    assert_eq!(count(&fte(source), K::BinaryExpr), 0);
    // A typedef in a body is that body's: the next function sees a value.
    let source = "void() g = { typedef float Foo; Foo = 1; };\nvoid() h = { Foo = 1; };\n";
    let parsed = fte(source);
    assert_eq!(codes(&parsed).len(), 1, "{:?}", messages(&parsed));
    assert_eq!(count(&parsed, K::BinaryExpr), 1);
}

// spec FTEF-20.14: `sizeof` of a registered type's name, in any case under `caseinsens`, takes a
// type; a name that is none is a value
#[test]
fn sizeof_takes_registered_types() {
    let takes_type = |parsed: &Parse| {
        parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == K::SizeofExpr)
            .is_some_and(|sizeof| sizeof.descendants().any(|n| n.kind() == K::SimpleType))
    };
    for operand in [
        "__string",
        "__variant",
        "__vector",
        "variant",
        "__bint",
        "void",
    ] {
        let source = in_body(&format!("local float a = sizeof({operand});"));
        assert!(takes_type(&fte(&source)), "{source}");
    }
    let source = in_body("local float a = sizeof(__String);");
    assert!(takes_type(&insensitive(&source)));
    assert!(!takes_type(&fte(&source)));
}

// spec FTEF-20.14: `static;` and `const *x;` leave the type out, an `int`, where the token after
// the qualifiers is no name
#[test]
fn qualifiers_may_stand_alone() {
    for source in [
        "static;\n",
        "const;\n",
        "static static;\n",
        "const *x;\n",
        "var() fn;\n",
    ] {
        assert_eq!(codes(&fte(source)), [], "{source}");
        assert_eq!(codes(&fte(&in_body(source))), [], "{source}");
    }
}

// spec FTEF-20.14: a name that is a type starts a definition unless `::` touches it, as
// `strncmp(pr_file_p, "::", 2)` tests, so `Foo::m();` is a call and `Foo ::m();` a definition
#[test]
fn a_touching_scope_resolution_is_an_expression() {
    let define = "class Foo { float a; void() m = {}; };\n";
    for (body, locals) in [
        ("Foo::m();", 0),
        ("Foo :: m();", 1),
        ("Foo ::m();", 1),
        ("Foo:: m();", 0),
        ("float::x;", 1),
        ("__variant::x;", 0),
        ("__variant ::x;", 1),
    ] {
        let source = format!("{define}{}", in_body(body));
        assert_eq!(count(&fte(&source), K::LocalDecl), locals, "{source}");
    }
}

// spec FTEF-20.14: at file level a registered type's name alone is a definition of no name, as
// `QCC_PR_ParseDefs` reads every definition, in any case under `caseinsens`
#[test]
fn a_registered_name_alone_is_a_definition() {
    for word in [
        "__variant",
        "__int",
        "__string",
        "variant",
        "__bint",
        "__uint8",
    ] {
        let source = format!("{word};\n");
        assert_eq!(codes(&fte(&source)), [], "{source}");
        assert_eq!(count(&fte(&source), K::Declaration), 1, "{source}");
    }
    for word in ["__Variant", "__String", "Variant"] {
        let source = format!("{word};\n");
        assert_eq!(codes(&insensitive(&source)), [], "{source}");
        assert_eq!(count(&insensitive(&source), K::Declaration), 1, "{source}");
        assert_ne!(codes(&fte(&source)), [], "{source}");
    }
}
