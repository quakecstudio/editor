// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R2 readiness review (TEST-9): the FTEQCC profile at the
//! nesting limit and its builtin initialisers, written by reviewers who did not write the
//! parser.

use super::{Program, errors};
use crate::QC_MAX_NESTING;
use crate::ast::{AstNode, Builtin, Declarator};
use crate::kind::SyntaxKind as K;
use crate::{Code, Dialect, SyntaxNode, parse_with};

/// The deepest chain of nodes from `root` to a leaf, counted without recursion.
fn tree_depth(root: &SyntaxNode) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root.clone(), 1_usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        for child in node.children() {
            stack.push((child, depth.saturating_add(1)));
        }
    }
    deepest
}

/// A program with `body` as the statements of `f`, parsed under the FTEQCC profile.
fn fte_body(body: &str) -> crate::Parse {
    let source = Program {
        body,
        ..Program::default()
    }
    .source();
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

// spec QC-27.15: the FTEQCC profile keeps the vanilla limit for vanilla constructs, so
// parentheses nest exactly as deep under both profiles before "nesting too deep"
#[test]
fn reviewer_fteqcc_keeps_the_vanilla_parenthesis_limit() {
    let parens = |n: usize| format!("a = {}a{};", "(".repeat(n), ")".repeat(n));
    assert_eq!(errors(&fte_body(&parens(QC_MAX_NESTING - 4))), vec![]);
    assert_eq!(
        errors(&fte_body(&parens(QC_MAX_NESTING - 3))),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
}

// spec QC-27.15: under the FTEQCC profile a chain of the FTEQCC operators that nest the
// expression before them (`?:`, `?:` with no middle, `->`, `[]`, the comma operator, a cast,
// `sizeof`) is an error past the limit, the tree stays bounded and every byte is kept
#[test]
fn reviewer_fteqcc_operator_chains_stop_at_the_limit() {
    let n = 20_000;
    for body in [
        format!("a = {}1;", "a ? 1 : ".repeat(n)),
        format!("a = {}1;", "a ?: ".repeat(n)),
        format!("e{};", "->en".repeat(n)),
        format!("a{};", "[0]".repeat(n)),
        format!("{}a;", "a, ".repeat(n)),
        format!("a = {}a;", "(float)".repeat(n)),
        format!("a = {}a;", "sizeof ".repeat(n)),
        format!("a = {}a;", "!".repeat(n)),
    ] {
        let parsed = fte_body(&body);
        assert!(
            parsed.errors().any(|d| d.code == Code::E0509),
            "{}",
            &body[..20]
        );
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
            "{}",
            &body[..20]
        );
        let source = Program {
            body: &body,
            ..Program::default()
        }
        .source();
        assert_eq!(parsed.syntax().text(), source.as_bytes());
    }
}

// spec QC-27.15: the FTEQCC statement forms that nest a statement (`do`, `for`, `switch`
// bodies and a labelled statement) are bounded like `if` and `while`
#[test]
fn reviewer_fteqcc_statement_nesting_is_bounded() {
    let n = 20_000;
    for body in [
        format!("{}a = 1;{}", "do ".repeat(n), " while (a);".repeat(n)),
        format!("{}a = 1;", "for (;;) ".repeat(n)),
        format!("{}a = 1;", "if (a) ".repeat(n)),
    ] {
        let parsed = fte_body(&body);
        assert!(
            parsed.errors().any(|d| d.code == Code::E0509),
            "{}",
            &body[..20]
        );
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
            "{}",
            &body[..20]
        );
    }
}

// spec QCD-1.4: a parse under the FTEQCC profile never fails: arbitrary FTEQCC punctuation
// soup gives back exactly its bytes, whatever errors it reports
#[test]
fn reviewer_fteqcc_soup_round_trips() {
    let soup = "class :: -> ?: ... #0: [++ .. --] $frame { ( ) } ; , *& ~ ^ << >>= %= &~= (+) ";
    let source = soup.repeat(200);
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    assert!(parsed.errors().next().is_some());
    for d in parsed.diagnostics() {
        assert!(d.span.start <= d.span.end && d.span.end <= source.len());
    }
}

// spec QCD-1.4: the FTEQCC profile parses the same bytes to the same tree and diagnostics
// every time, including after an error
#[test]
fn reviewer_fteqcc_parse_is_deterministic() {
    let source = b"float f(float a = 1, ...) { for (;;) { a ?: ; } }; class c : d { };";
    assert_eq!(
        parse_with(source, Dialect::Fteqcc),
        parse_with(source, Dialect::Fteqcc)
    );
}

/// Checks that `source`, parsed under the FTEQCC profile, reports "nesting too deep", keeps its
/// tree within 4 x `QC_MAX_NESTING` levels and gives back every byte.
fn assert_fte_bounded(name: &str, source: &str) {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert!(
        parsed.errors().any(|d| d.code == Code::E0509),
        "{name}: no E0509"
    );
    assert!(
        tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
        "{name}: tree too deep"
    );
    assert_eq!(parsed.syntax().text(), source.as_bytes(), "{name}");
}

// spec QC-27.15: the FTEQCC aggregates nest as declarations do: a struct or union inside a
// struct or union, 100000 deep, closed or left open, stops at the limit with every byte kept
#[test]
fn reviewer_fteqcc_aggregate_nesting_is_bounded() {
    let n = 100_000;
    for (name, source) in [
        (
            "struct",
            format!("{}float x;{};\n", "struct { ".repeat(n), "} s;".repeat(n)),
        ),
        (
            "union",
            format!("{}float x;{};\n", "union { ".repeat(n), "} s;".repeat(n)),
        ),
        ("unclosed struct", "struct { ".repeat(n)),
    ] {
        assert_fte_bounded(name, &source);
    }
}

// spec QC-27.15: an FTEQCC initialiser list `{ ... }` inside another, 100000 deep, is a level
// each
#[test]
fn reviewer_fteqcc_initialiser_lists_are_bounded() {
    let n = 100_000;
    assert_fte_bounded(
        "global list",
        &format!("float x[1] = {}1{};\n", "{".repeat(n), "}".repeat(n)),
    );
}

// spec QC-27.15: under the FTEQCC profile the operators that nest to the right or after their
// operand are bounded too: `?:` nested in its middle, `=` and `+=` chains, `~`, a prefix `*`
// and a postfix `++` chain, each 100000 deep
#[test]
fn reviewer_fteqcc_right_nesting_chains_are_bounded() {
    let n = 100_000;
    for (name, body) in [
        (
            "?: middle",
            format!("a = {}1{};", "a ? ".repeat(n), " : 1".repeat(n)),
        ),
        ("= chain", format!("{}1;", "a = ".repeat(n))),
        ("+= chain", format!("{}1;", "a += ".repeat(n))),
        ("~", format!("a = {}a;", "~".repeat(n))),
        ("prefix *", format!("a = {}a;", "*".repeat(n))),
        ("postfix ++", format!("a{};", "++".repeat(n))),
    ] {
        let source = Program {
            body: &body,
            ..Program::default()
        }
        .source();
        assert_fte_bounded(name, &source);
    }
}

// spec FTEF-18.32: the expressions FTEQCC reads outside a function body, an enum value, a
// parameter's default and a global's initialiser, count prefix operators and parentheses
// towards the limit like a body's
#[test]
fn reviewer_fteqcc_file_scope_expressions_are_bounded() {
    let n = 100_000;
    for (name, source) in [
        (
            "enum value",
            format!("enum {{ x = {}1{} }};\n", "(".repeat(n), ")".repeat(n)),
        ),
        (
            "parameter default",
            format!("void(float x = {}1{}) g;\n", "(".repeat(n), ")".repeat(n)),
        ),
        ("global -", format!("float x = {}1;\n", "-".repeat(n))),
    ] {
        assert_fte_bounded(name, &source);
    }
}

/// The engine name and FTEQCC `binum` of each builtin initialiser of `source`, parsed under the
/// FTEQCC profile, after checking that it parses with no error.
fn fte_builtins(source: &str) -> Vec<(Option<String>, Option<i32>)> {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(errors(&parsed), [], "{source}");
    parsed
        .syntax()
        .descendants()
        .filter_map(Builtin::cast)
        .map(|builtin| {
            let binum = builtin
                .number()
                .and_then(|t| crate::builtin_binum(&t.text_lossless(), Dialect::Fteqcc));
            (builtin.engine_name(qc_lexer::LexFlags::default()), binum)
        })
        .collect()
}

// spec FTE-17.5: the `:` builtin form follows any function declarator, with or without `=`:
// a C-style declarator, `var`, a list, a local, and a space after the `:`. The pinned FTEQCC
// compiles each with 0 warnings, binding the names given
#[test]
fn reviewer_fteqcc_colon_builtins_follow_any_function_declarator() {
    let name = |n: &str| (Some(n.to_owned()), None);
    for (source, expected) in [
        ("void bx() :ex_c;\n", vec![name("ex_c")]),
        ("void bx() = :ex_d;\n", vec![name("ex_d")]),
        (
            "void(float a) bx :ex_e, by :ex_f;\n",
            vec![name("ex_e"), name("ex_f")],
        ),
        ("var void() bx :ex_g;\n", vec![name("ex_g")]),
        ("void() bx : ex_h;\n", vec![name("ex_h")]),
        (
            "void() f = { local void() lf = :nm4; };\n",
            vec![name("nm4")],
        ),
    ] {
        assert_eq!(fte_builtins(source), expected, "{source}");
    }
}

// spec FTE-17.5: a `:` after a field of function type or after a function parameter is no
// builtin initialiser; the pinned FTEQCC rejects both
#[test]
fn reviewer_fteqcc_colon_is_no_builtin_on_a_field_or_parameter() {
    for source in [".void() fth :nm6;\n", "void(void() cb :nm7) bx = #1;\n"] {
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        assert_ne!(errors(&parsed), [], "{source}");
    }
}

// spec FTE-17.6: `:` takes the number forms `#` takes: a character literal is its code, a
// float above 2^24 is `(int)` of its `f32`, and an `i` literal in hex its `int`. The pinned
// FTEQCC binds `:'a'` as 97, `:'\n'` as 10, `:123456789` as 123456792, `#0x10i` as 16 and
// `bx :5` as 5
#[test]
fn reviewer_fteqcc_colon_numbers_bind_as_hash_numbers() {
    for (source, binum) in [
        ("void() bx = :'a';\n", 97),
        ("void() bx = :'\\n';\n", 10),
        ("void() bx = :123456789;\n", 123_456_792),
        ("void() bx = #0x10i;\n", 16),
        ("void() bx :5;\n", 5),
    ] {
        assert_eq!(fte_builtins(source), [(None, Some(binum))], "{source}");
    }
}

/// Whether `source`, parsed under the FTEQCC profile with `keywords` from its first byte, has a
/// node of `kind`.
fn holds_with(source: &str, keywords: crate::Keywords, kind: crate::SyntaxKind) -> bool {
    super::fte_with(source, keywords)
        .syntax()
        .descendants()
        .any(|n| n.kind() == kind)
}

// spec FTEF-20.2: only a token that starts with exactly `__` and goes on with the word is the
// keyword when it is off: `___loop` and `__ loop` are not `loop`, `__LoOp` is. The pinned FTEQCC
// refuses `void() g = { ___loop { break; } };` and `{ __ loop { break; } }`, and compiles
// `{ __Loop { __Break; } }`
#[test]
fn reviewer_only_a_double_underscore_spells_a_keyword() {
    use crate::{Keyword, Keywords, SyntaxKind as K};
    let off = Keywords::default().with(Keyword::Loop, false);
    let body = |stmts: &str| format!("void() g = {{ {stmts} }};\n");
    assert!(holds_with(&body("__LoOp { break; }"), off, K::LoopStmt));
    assert!(!holds_with(&body("___loop { break; }"), off, K::LoopStmt));
    assert!(!holds_with(&body("__ loop { break; }"), off, K::LoopStmt));
    assert!(!holds_with(&body("loop { break; }"), off, K::LoopStmt));
}

// spec FTEF-20.14: a body's statement defines locals at a name `QCC_TypeForName` finds, whatever
// that word's keyword: with `float`, `vector`, `entity` or `string` off, as `-std=hcc` and
// `#pragma keyword disable float` turn them off, `float x;` in a body is still a local. The
// pinned FTEQCC compiles each such program
#[test]
fn reviewer_a_type_word_whose_keyword_is_off_still_defines_a_local() {
    use crate::{Keyword, Keywords, SyntaxKind as K};
    for (keyword, stmts) in [
        (Keyword::Float, "float x; x = 1;"),
        (Keyword::Vector, "vector v; v = '1 2 3';"),
        (Keyword::Entity, "entity e;"),
        (Keyword::String, "string s; s = \"a\";"),
    ] {
        let source = format!("void() g = {{ {stmts} }};\n");
        let off = Keywords::default().with(keyword, false);
        let parsed = super::fte_with(&source, off);
        assert_eq!(parsed.errors().count(), 0, "{source}");
        assert!(holds_with(&source, off, K::LocalDecl), "{source}");
    }
}

// spec FTEF-21.17: the block's string is compared once its escapes are read and its prefix
// dropped: `"\x43"`, `"\{67}"`, `u8"C"` and `R"(C)"` are `"C"` and lex the block with
// `qcfuncs` off, so `"\9"` in it is "Bad character code", while `"\103"`, which under `qcfuncs`
// is no `C` (FTEF-12.12), and `"C "` are not. The pinned FTEQCC refuses the first four programs
// and compiles the other two
#[test]
fn reviewer_an_extern_block_s_string_is_compared_as_read() {
    let fte = |source: &str| parse_with(source.as_bytes(), Dialect::Fteqcc);
    let bad_code = |source: &str| {
        fte(source)
            .diagnostics()
            .iter()
            .any(|d| d.code == Code::E0103)
    };
    for linkage in ["\"\\x43\"", "\"\\{67}\"", "u8\"C\"", "R\"(C)\""] {
        let source = format!("extern {linkage} {{ string s = \"\\9\"; }}\n");
        assert!(bad_code(&source), "{source}");
    }
    for linkage in ["\"\\103\"", "\"C \""] {
        let source = format!("extern {linkage} {{ string s = \"\\9\"; }}\n");
        assert!(!bad_code(&source), "{source}");
    }
}

/// `source` parsed under the FTEQCC profile with `caseinsens` on or off.
fn fte_case(source: &str, insensitive: bool) -> crate::Parse {
    super::fte_with(
        source,
        crate::Keywords::default().with_case_insensitive(insensitive),
    )
}

// spec FTEF-20.14: under `caseinsens` every statement keyword, a loop's and a switch's and a
// jump's, is found in any case, as `QCC_PR_CheckKeyword` compares with `stricmp`; with it off
// the capitalised word is a name. The pinned FTEQCC compiles each body under `-Fcaseinsens`
#[test]
fn reviewer_statement_keywords_are_found_in_any_case_under_caseinsens() {
    for body in [
        "IF (1) {} ELSE {}",
        "While (1) { Break; }",
        "DO { Continue; } while (0);",
        "FOR (;;) { BREAK; }",
        "Switch (1) { Case 1: Break; Default: Break; }",
        "Goto l; l: ;",
        "Local float x; x = 1;",
        "Return;",
    ] {
        let source = format!("void() g = {{ {body} }};\n");
        assert_eq!(errors(&fte_case(&source, true)), vec![], "{body}");
    }
    assert!(
        fte_case("void() g = { IF (1) {} };\n", false)
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-21.17: `extern` is a keyword like the others, so under `caseinsens` `EXTERN "C" {`
// and `Extern "c" {` open a block, in a body too; with it off they are names and no block. The
// pinned FTEQCC compiles both blocks under `-Fcaseinsens`
#[test]
fn reviewer_an_extern_block_opens_in_any_case_under_caseinsens() {
    let blocks = |source: &str, insensitive: bool| {
        let parsed = fte_case(source, insensitive);
        let found = parsed
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::ExternBlock)
            .count();
        (found, errors(&parsed))
    };
    for source in [
        "EXTERN \"C\" { float x = 5; float y = x; }\n",
        "Extern \"c\" { float x = 5; float y = x; }\n",
    ] {
        assert_eq!(blocks(source, true), (1, vec![]), "{source}");
        assert_eq!(blocks(source, false).0, 0, "{source}");
    }
}

// spec FTEF-20.2: `__` before a keyword spells it in any case whether or not `caseinsens` is
// on
#[test]
fn reviewer_a_double_underscore_keyword_is_found_in_any_case_without_caseinsens() {
    for insensitive in [false, true] {
        for word in ["__LOOP", "__Loop", "__loop"] {
            let source = format!("void() g = {{ {word} {{ break; }} }};\n");
            assert_eq!(errors(&fte_case(&source, insensitive)), vec![], "{source}");
        }
    }
}

// spec FTEF-6.1: a class's member may follow a `public:` label, which FTEQCC reads and
// ignores; the pinned FTEQCC compiles `class C { public: float a; };`
#[test]
fn reviewer_a_class_member_may_follow_a_public_label() {
    let source = "class C { public: float a; };\nvoid() g = { C c; };\n";
    assert_eq!(errors(&fte_case(source, false)), vec![]);
}

// spec QC-27.15: 100000 nested `extern "C" {` blocks, closed or not, and 100000 unbraced
// `extern "C" ` before a definition, at file level and in a body, parse on a 2 MiB thread with
// "nesting too deep" and keep every byte; the pinned FTEQCC recurses without a limit
#[test]
fn reviewer_nested_extern_blocks_stop_at_the_limit_on_a_small_stack() {
    let n = 100_000;
    let open = "extern \"C\" { ".repeat(n);
    let bare = "extern \"C\" ".repeat(n);
    let shut = " }".repeat(n);
    let shapes = [
        format!("{open}float y;{shut}"),
        format!("{open}float y;"),
        format!("{bare}float y;"),
    ];
    for item in shapes {
        for source in [format!("{item}\n"), format!("void() g = {{ {item} }};\n")] {
            let bytes = source.clone().into_bytes();
            let (depth, kept, too_deep) = std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let parsed = parse_with(&bytes, Dialect::Fteqcc);
                    (
                        tree_depth(&parsed.syntax()),
                        parsed.syntax().text() == bytes,
                        parsed.errors().any(|d| d.code == Code::E0509),
                    )
                })
                .unwrap()
                .join()
                .unwrap();
            assert!(too_deep, "{}", &source[..30]);
            assert!(kept, "{}", &source[..30]);
            assert!(
                depth <= QC_MAX_NESTING.saturating_mul(4),
                "{}",
                &source[..30]
            );
        }
    }
}

// The eighth P2-R2 review's tests (TEST-9). Each program was run through the pinned FTEQCC
// first (`cargo xtask oracles builtins --record`, a scratch family, macOS arm64) and the test
// asserts what FTEQCC did: the first list of each test compiles there, the second does not.

/// The codes of the errors `source` gets under the FTEQCC profile, after checking that the
/// tree gives back every byte.
fn fte_codes(source: &str) -> Vec<Code> {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(parsed.syntax().text(), source.as_bytes(), "{source}");
    parsed.errors().map(|d| d.code).collect()
}

// spec FTEF-18.34: a struct, class, enum or function body that has its `}` is read whole
// whatever its columns, so a member or a local at column 0 is one and the declaration after
// the `}` is the file's; a body that lost its `}` ends at the next declaration line with an
// error. The pinned FTEQCC compiles the closed forms and refuses the open ones
#[test]
fn reviewer_a_closed_body_keeps_its_column_0_members() {
    for source in [
        "struct S {\nfloat a;\n\nfloat x;\n};\nfloat y;\n",
        "class C {\nfloat a;\nvoid() m = {\nfloat b;\nthis.a = b;\n};\n};\nfloat x;\n",
        "enum E {\nA,\nB\n};\nfloat x;\n",
        "void() g = {\nstruct S {\nfloat a;\n} x;\nx.a = 1;\n};\n",
        "void() g = {\n{\n{\nfloat a;\n}\n}\n};\n\nfloat y;\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
    }
    for source in [
        "struct S {\nfloat a;\n\nfloat x;\n",
        "struct S {\nfloat a;\n\nvoid() g = {};\n",
        "class C {\nfloat a;\n\nfloat x;\n",
        "void() g = {\n\tfloat a = 1;\n\nvoid() h = {\n\tfloat a = 2;\n};\n",
    ] {
        assert_eq!(fte_codes(source), [Code::E0204], "{source}");
    }
}

// spec FTEF-20.14: `struct`, `union`, `class`, `typedef`, `enum`, a struct body's labels and
// its `static` and `virtual` are keywords `QCC_PR_CheckKeyword` finds in any case under
// `caseinsens` and names without it. The pinned FTEQCC compiles each under `-Fcaseinsens`
// and refuses each without
#[test]
fn reviewer_type_keywords_and_labels_are_found_in_any_case_under_caseinsens() {
    for source in [
        "STRUCT S { float a; };\n",
        "Struct S { float a; };\n",
        "UNION U { float a; };\n",
        "CLASS C { float a; };\n",
        "TYPEDEF float t;\n",
        "Typedef float t; t x;\n",
        "ENUM E { A };\n",
        "struct S { PUBLIC: float a; };\n",
        "struct S { Private: float a; };\n",
        "class C { PUBLIC: float a; };\n",
        "class C { Protected: : float a; };\n",
        "class C { VIRTUAL void() m = {}; };\n",
        "class C { Static float a; };\n",
        "struct S { STATIC float a; float b; };\n",
        "struct S { Virtual void() m = {}; float b; };\n",
        "struct P { float a; }; STRUCT S : P { float b; };\n",
    ] {
        assert_eq!(errors(&fte_case(source, true)), vec![], "{source}");
        assert!(
            fte_case(source, false).errors().next().is_some(),
            "{source}"
        );
    }
}

// spec FTEF-6.7: a struct body's member may be a nested class, struct, union, enum or
// accessor, with or without a declarator, to any depth, a pointer to the struct itself and a
// bitfield after a `,` or with an expression's width; a class in a class, a typedef in a
// struct, a width with no name and a `,` after a width are refused. The pinned FTEQCC agrees
#[test]
fn reviewer_nested_types_are_members_as_parse_struct_reads_them() {
    for source in [
        "struct S { class C { float a; } c; };\n",
        "struct S { class C { float a; }; };\n",
        "class C { struct S { float a; } *p; };\n",
        "struct S { struct T { struct U { float a; } u; } t; };\n",
        "struct S { struct T { struct U { float a; }; }; };\n",
        "struct S { struct { struct { float a; }; }; };\n",
        "struct S { union { struct { float a; float b; }; vector v; }; };\n",
        "struct S { struct T { float a; } t[2]; };\n",
        "struct S { struct T { float a; } *t, u; };\n",
        "struct S { struct S *next; };\n",
        "struct S { S *next; };\n",
        "struct S { enum E { A } e; };\n",
        "struct S { accessor A : float; };\n",
        "struct S { int a, b : 3; };\n",
        "struct S { int a : (1 + 2); };\n",
        "struct S { int a : 1 + 2; };\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
    }
    for source in [
        "class C { class D { float a; }; };\n",
        "struct S { typedef float t; t x; };\n",
        "struct S { int : 3; };\n",
        "struct S { int a : 3, b; };\n",
        "struct S { float a : 3 = 1; };\n",
        "class C { ;; };\n",
        "class C { float a;; float b; };\n",
        "struct {};\n",
        "struct;\n",
        "struct S;;\n",
        "struct S { float a; };;\n",
    ] {
        assert_ne!(fte_codes(source), [], "{source}");
    }
}

// spec FTEF-7.4: a typedef's declarators are any definition's, through chains, arrays, fields
// and enums, and `typedef` is read any number of times; a name may be both a type and a
// variable or be defined twice as one type; the forms `QCC_PR_ParseDefs` refuses are refused.
// The pinned FTEQCC agrees on each
#[test]
fn reviewer_typedef_declarators_are_any_definition_s() {
    for source in [
        "typedef float t; typedef t u; typedef u v; v x = 1;\n",
        "typedef float t[2]; typedef t u[3]; u x;\n",
        "typedef typedef typedef float tt; tt x;\n",
        "typedef float t; t t;\n",
        "typedef float t; typedef t t;\n",
        "typedef float t, t;\n",
        "typedef float T, t;\n",
        "typedef float t,;\n",
        "typedef float t typedef;\n",
        "typedef struct S { float a; } S; S x;\n",
        "typedef struct S { float a; } s; s x; S y;\n",
        "typedef .float ft; .ft f2;\n",
        "typedef float t; .t f2;\n",
        "typedef enum { A } E, *PE;\n",
        "typedef enum E : int { A } T;\n",
        "typedef enumflags { A } E;\n",
        "typedef enum class E : float { A } T; T x = E::A;\n",
        "typedef accessor A : float { get float x = { return 1; } } B;\n",
        "void() g = { if (1) typedef float t; };\n",
        "void() g = { while (0) typedef float t; };\n",
        "typedef float t\nfloat x;\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
    }
    for source in [
        "typedef, float t;\n",
        "typedef float, t;\n",
        "typedef float t, , u;\n",
        "typedef float t u;\n",
        "typedef float t = 1, u;\n",
        "typedef float typedef t;\n",
    ] {
        assert_ne!(fte_codes(source), [], "{source}");
    }
}

// spec FTEF-9.8, FTEF-6.8: a struct's or class's name, with or without its keyword, stands
// before a field's `.` and as a function's return type; after a body `x = { 1 }, y = { 2 }`
// and `x[2] = { { 1 }, { 2 } }` are its declarators; a constructor is the member named exactly
// as the class, and a `~` or a `= {}` on it is refused; a `}` with no `;` before a declaration
// is refused and a definition's missing `;` before a line is not. The pinned FTEQCC agrees
#[test]
fn reviewer_a_type_name_stands_before_fields_functions_and_initialisers() {
    for source in [
        "struct S { float a; }; .S f2;\n",
        "struct S { float a; }; .struct S f2;\n",
        "class C { float a; }; .C f2;\n",
        "class C { float a; }; .class C f2;\n",
        "struct S { float a; }; S() h = #6;\n",
        "struct S { float a; }; S *() h = #6;\n",
        "class C { float a; }; C() h = #6;\n",
        "class C { float a; }; class C() h = #6;\n",
        "struct P { float a; }; void(struct S : P { float b; } x) h = #6;\n",
        "struct P { float a; }; struct S : P { float b; } x;\n",
        "struct S { float a; } x = { 1 }, y = { 2 };\n",
        "struct S { float a; } x[2] = { { 1 }, { 2 } };\n",
        "struct S { float a; } x\nfloat b;\n",
        "struct S { float a; }; S S;\n",
        "class C { float a; } C;\n",
        "class C { float C; };\n",
        "class C { void() C = {}; };\n",
        "struct { float a; };\n",
        "union { float a; };\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
    }
    for source in [
        "class C { ~C(); };\n",
        "class C { void() ~C; };\n",
        "class C { C() = {}; };\n",
        "struct S { float a; }\nfloat b;\n",
        "struct S { float a; } float b;\n",
        "class C { float a; }\nfloat b;\n",
        "enum E { A }\nfloat b;\n",
        "struct S { float a; } x, struct T { float b; } y;\n",
        "struct P { float a; }; struct : P { float b; } x;\n",
        "struct P { float a; }; typedef struct : P { float b; } T;\n",
        "struct P { float a; }; struct S : struct P { float b; };\n",
    ] {
        assert_ne!(fte_codes(source), [], "{source}");
    }
}

// The eighth review's tests for the mutants the qc-syntax measurement of `60e4985` left
// (Readiness run 40421258086): each pins a branch the earlier tests reached but did not check.

/// Parses `source` under the FTEQCC profile.
fn eighth(source: &str) -> crate::Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The codes of every error of `source` under the FTEQCC profile.
fn eighth_codes(source: &str) -> Vec<Code> {
    eighth(source).errors().map(|d| d.code).collect()
}

/// The first node of kind `N` in `source` under the FTEQCC profile.
fn eighth_first<N: AstNode>(source: &str) -> N {
    eighth(source)
        .syntax()
        .descendants()
        .find_map(N::cast)
        .unwrap()
}

/// Parses `source` under the FTEQCC profile with `qcfuncs` off from its first byte.
fn qcfuncs_off(source: &str) -> crate::Parse {
    let edit = qc_preproc::FlagEdit::of_flags(
        &[qc_preproc::FteFlag::Qcfuncs],
        qc_preproc::CompilerFlags::default().with(qc_preproc::FteFlag::Qcfuncs, false),
    );
    crate::parse_preprocessed_flagged(source.as_bytes(), Dialect::Fteqcc, &[(0, edit)], &[], &[])
}

/// The names of `declarators`.
fn names(declarators: impl Iterator<Item = Declarator>) -> Vec<String> {
    declarators
        .map(|d| String::from_utf8_lossy(d.name().unwrap().text()).into_owned())
        .collect()
}

// spec FTEF-15.6: a class, enum or accessor declaration declares the variables after its body,
// and the view gives each one
#[test]
fn reviewer_eighth_class_enum_and_accessor_views_give_their_declarators() {
    use crate::ast::{AccessorDecl, ClassDecl, EnumDecl};
    let class: ClassDecl = eighth_first("class C { float a; } c1, c2;");
    assert_eq!(names(class.declarators()), ["c1", "c2"]);
    let r#enum: EnumDecl = eighth_first("enum { A, B } e1, *e2;");
    assert_eq!(names(r#enum.declarators()), ["e1", "e2"]);
    let accessor: AccessorDecl = eighth_first("accessor A : float { get float x = f; } a1;");
    assert_eq!(names(accessor.declarators()), ["a1"]);
}

// spec FTEF-18.33: `switch` takes any statement as its body under FTEQCC, and the view gives
// the statement, or the block when the body is one
#[test]
fn reviewer_eighth_switch_view_gives_its_statement_or_block() {
    use crate::ast::SwitchStmt;
    let source = "float a;\nvoid() f = { switch (a) return; };";
    assert_eq!(eighth_codes(source), []);
    let switch: SwitchStmt = eighth_first(source);
    assert_eq!(
        switch.statement().map(|s| s.syntax().kind()),
        Some(K::ReturnStmt)
    );
    assert!(switch.block().is_none());
    let source = "float a;\nvoid() f = { switch (a) { case 1: break; } };";
    assert_eq!(eighth_codes(source), []);
    let switch: SwitchStmt = eighth_first(source);
    assert!(switch.block().is_some());
    assert_eq!(
        switch.statement().map(|s| s.syntax().kind()),
        Some(K::Block)
    );
    // Vanilla reads only a `{` body: with none the switch holds an empty block and the
    // statement after it is the function's.
    let vanilla = "float a;\nvoid() f = { switch (a) return; };";
    let parsed = parse_with(vanilla.as_bytes(), Dialect::Vanilla);
    let switch = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == K::SwitchStmt)
        .unwrap();
    assert_eq!(
        switch.children().map(|n| n.kind()).collect::<Vec<_>>(),
        [K::NameRef, K::Block]
    );
    assert_eq!(
        switch
            .descendants()
            .filter(|n| n.kind() == K::ReturnStmt)
            .count(),
        0
    );
}

// spec FTEF-21.7: `(*name)` is C's function pointer form and the view counts its `*`s, after
// any `*`s of the type and before the name, and no `(` means no form
#[test]
fn reviewer_eighth_pointer_form_counts_the_stars_inside_the_parentheses() {
    use crate::ast::StructDecl;
    let source = "struct S { float a; } *(*x)(float), (**y)(float), *p;";
    let parsed = qcfuncs_off(source);
    assert_eq!(parsed.errors().count(), 0);
    let decl = parsed
        .syntax()
        .descendants()
        .find_map(StructDecl::cast)
        .unwrap();
    assert_eq!(
        decl.declarators()
            .map(|d| d.pointer_form())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), None]
    );
}

// spec FTEF-6.7: a `static` or `virtual` member of function type takes a body or a state
// header after `=`, any other member an expression; a member with neither reads on
#[test]
fn reviewer_eighth_static_members_take_a_body_only_when_functions() {
    let function =
        "struct S { float a; static void() m = { return; }; static void() n = [1, n] {}; };";
    assert_eq!(eighth_codes(function), []);
    let value = "struct S { float a; static float c = 1; static vector v = '1 2 3'; };";
    assert_eq!(eighth_codes(value), []);
    // A `{` after `=` of a member of another type is read as an expression, which it is not,
    // never as a body.
    let braces = "struct S { float a; static vector v = { 1, 2, 3 }; };";
    assert_eq!(eighth_codes(braces), [Code::E0201]);
    let bare = "struct S { float a; static float b { 1 }; };";
    assert_eq!(eighth_codes(bare), [Code::E0204]);
    let statements = "struct S { float a; static float b { return; }; };";
    assert_eq!(eighth_codes(statements), [Code::E0204]);
    let bodyless = "struct S { float a; static void() m { return; }; };";
    assert_eq!(eighth_codes(bodyless), []);
}

// spec FTEF-8.5: `enum class` with no `:` after its name is "expected :" and the body is not
// read as enumerators
#[test]
fn reviewer_eighth_a_strict_enum_without_a_colon_reads_no_enumerators() {
    let parsed = eighth("enum class E { A, B };");
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        [Code::E0204]
    );
    assert!(
        !parsed
            .syntax()
            .descendants()
            .any(|n| n.kind() == K::Enumerator)
    );
    let parsed = eighth("enum class E : int { A, B };");
    assert_eq!(parsed.errors().count(), 0);
    assert_eq!(
        parsed
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::Enumerator)
            .count(),
        2
    );
}

// spec FTEF-16.9: a `for` initialiser that starts with `const`, `volatile`, a class keyword or
// a tagged type declares a local, each word on its own
#[test]
fn reviewer_eighth_each_type_form_starts_a_for_initialiser() {
    for init in [
        "const float i = 0",
        "class C i",
        "struct S i",
        "union U i",
        "accessor A i",
    ] {
        let source = format!(
            "class C {{ float a; }}; struct S {{ float a; }}; union U {{ float a; }};\n\
             accessor A : float {{ get float x = f; }};\n\
             void() f = {{ for ({init}; 0; ) {{ }} }};"
        );
        let parsed = eighth(&source);
        assert_eq!(
            parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
            [],
            "{init}"
        );
        assert!(
            parsed
                .syntax()
                .descendants()
                .any(|n| n.kind() == K::LocalDecl),
            "{init}: no local"
        );
    }
    // `volatile` starts a declaration there too, which the local's reader then refuses as a
    // qualifier it does not take; an expression would be refused otherwise.
    let volatile = "void() f = { for (volatile float i = 0; 0; ) { } };";
    assert_eq!(eighth_codes(volatile), [Code::E0204, Code::E0201]);
    // In a cast and after `sizeof` each word on its own names a type too (FTEF-2.11: the
    // `const` is ignored); `volatile` is read as a type form and then refused in the cast.
    assert_eq!(
        eighth_codes("void() f = { float x; x = (const float) 1; x = sizeof(const float); };"),
        []
    );
    assert_eq!(
        eighth_codes("void() f = { float x; x = (volatile float) 1; };"),
        [Code::E0204]
    );
}

// spec FTEF-9.9: `public`, `private`, `protected` and `virtual:` are labels, `virtual` without
// a `:` a qualifier; labels before the class's `}` are "is not a type"
#[test]
fn reviewer_eighth_class_labels_and_members() {
    assert_eq!(
        eighth_codes(
            "class C { public: float a; private: float b; protected: : float c; virtual: float d; nonvirtual: float e; virtual void() m; };"
        ),
        []
    );
    let labels = |source: &str| {
        eighth(source)
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::AccessLabel)
            .count()
    };
    assert_eq!(
        labels(
            "class C { public: float a; private: float b; protected: : float c; virtual: float d; nonvirtual: float e; virtual void() m; };"
        ),
        5
    );
    assert_eq!(eighth_codes("class C { public: };"), [Code::E0203]);
    assert_eq!(
        eighth("class C { public: };")
            .syntax()
            .descendants()
            .map(|n| n.kind())
            .collect::<Vec<_>>(),
        [K::SourceFile, K::ClassDecl, K::AccessLabel]
    );
    assert_eq!(eighth_codes("class C { public: private: };"), [Code::E0203]);
    assert_eq!(eighth_codes("class C { };"), []);
    assert_eq!(eighth_codes("class C { float a; };"), []);
}

// spec FTEF-9.4, FTEF-6.8, FTEF-10.6: a constructor is the class's own name before `(void);`,
// `struct float` is `float`, a pointer to a struct is no "Struct or union in class", and an
// accessor with its base where a member's type stands is refused
#[test]
fn reviewer_eighth_class_member_types() {
    assert_eq!(eighth_codes("class C { C(); };"), []);
    assert_eq!(eighth_codes("class C { D(); };"), [Code::E0203]);
    assert_eq!(
        eighth_codes("struct S { float a; }; class C { struct S s; };"),
        [Code::E0254]
    );
    assert_eq!(
        eighth_codes("struct S { float a; }; class C { struct S *p; };"),
        []
    );
    assert_eq!(
        eighth_codes("class C { struct float a; struct int b; float *p; float c; };"),
        []
    );
    assert_eq!(
        eighth_codes("class C { accessor A : float x; };"),
        [Code::E0258]
    );
    assert_eq!(
        eighth_codes("accessor A : float { get float x = f; }; class C { accessor A x; };"),
        []
    );
    // get and set members, inline or not, are accessor members, not declarations
    assert_eq!(
        eighth_codes(
            "class C { get float x = f; set float y = g; inline get float z = h; inline set float w = k; };"
        ),
        []
    );
}

// spec FTEF-18.27: a construct keyword at column 0 ends a body that lost its `}`; a plain name
// at column 0 is a statement, and under the vanilla profile every name is
#[test]
fn reviewer_eighth_only_construct_words_end_a_lost_body() {
    let statement = "float x;\nvoid() f = {\n\tfloat y;\nx = 1;\n};\n";
    assert_eq!(eighth_codes(statement), []);
    // With no `}` at all the statement is still the body's, and only the end of the text is
    // missing.
    let unclosed = "float x;\nvoid() f = {\n\tfloat y;\nx = 1;\n";
    assert_eq!(eighth_codes(unclosed), [Code::E0201]);
    assert_eq!(
        eighth(unclosed)
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::FunctionBody)
            .count(),
        1
    );
    let lost = "struct S {\n\tfloat a;\ntypedef float t;\nfloat z;\n";
    assert!(
        eighth_codes(lost).contains(&Code::E0204),
        "{:?}",
        eighth_codes(lost)
    );
    let vanilla = "float class;\nvoid() f = {\n\tlocal float y;\nclass = 1;\n};\n";
    assert_eq!(
        parse_with(vanilla.as_bytes(), Dialect::Vanilla)
            .errors()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        []
    );
}

// spec FTEF-15.1: at file level a type word alone, with no declarator, is read as a
// declaration and its missing name reported, not as "is not a type"
#[test]
fn reviewer_eighth_a_bare_type_word_is_a_declaration() {
    assert_eq!(eighth_codes("float = 1;"), [Code::E0201]);
    assert_eq!(eighth_codes("float 1;"), [Code::E0201]);
    assert_eq!(eighth_codes("[1, f] void() g = {};"), [Code::E0203]);
}

// spec FTEF-6.7: a `,` with no type before it is "element missing type"; `static` members do
// not count towards "has no members"
#[test]
fn reviewer_eighth_struct_member_counting() {
    assert_eq!(
        eighth_codes("struct S { , float a; };"),
        [Code::E0201, Code::E0203]
    );
    assert_eq!(eighth_codes("struct S { float a, b; };"), []);
    assert_eq!(eighth_codes("struct S { static float a; };"), [Code::E0243]);
    assert_eq!(
        eighth_codes("struct S { static float a, b; };"),
        [Code::E0243]
    );
    assert_eq!(eighth_codes("struct S { static float a; float b; };"), []);
    assert_eq!(eighth_codes("struct S { float a; };"), []);
}

// spec FTEF-6.8, FTEF-21.7: with `qcfuncs` on a member `(*name)(params)` is "expected function
// arg list", `(name)(params)` and `**name` are not; with it off `(*name)` is C's form
#[test]
fn reviewer_eighth_qcfuncs_decides_the_member_pointer_form() {
    assert_eq!(
        eighth_codes("struct S { float (*fp)(float); };"),
        [Code::E0204]
    );
    assert_eq!(eighth_codes("struct S { float (fp)(float); };"), []);
    assert_eq!(eighth_codes("struct S { float **pp; float *p; };"), []);
    let parsed = qcfuncs_off("struct S { float (*fp)(float); };");
    assert_eq!(parsed.errors().map(|d| d.code).collect::<Vec<_>>(), []);
}
