// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R2 readiness review (TEST-9): the FTEQCC profile at the
//! nesting limit and its builtin initialisers, written by reviewers who did not write the
//! parser.

use super::{Program, errors};
use crate::QC_MAX_NESTING;
use crate::ast::{AstNode as _, Builtin};
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
