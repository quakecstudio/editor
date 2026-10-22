// SPDX-License-Identifier: GPL-3.0-or-later

//! The flags that change in the middle of a file as the parser reads them
//! (`docs/spec/dialect-fteqcc.md` FTEF-21): `extern "C"` and `extern "QC"` blocks and the flags
//! they set, the operator priorities, `qcfuncs`' declarations, the attributes and `typedef`'s
//! qualifiers. Each program is also a case of the flag family of `cargo xtask oracles builtins`,
//! which compares the outcome with the pinned FTEQCC.

use qc_lexer::LexFlag;
use qc_preproc::{CompilerFlags, FlagEdit, FteFlag, Priorities};

use super::{sexpr, whole};
use crate::ast::{self, AstNode as _};
use crate::{Code, Dialect, Parse, SyntaxKind, parse_preprocessed_flagged, parse_with};

/// `source` parsed as a source file under the FTEQCC profile.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// `source`, a preprocessor's text, parsed under the FTEQCC profile with the flag changes
/// `edits`, each at its byte.
fn flagged(source: &str, edits: &[(usize, FlagEdit)]) -> Parse {
    parse_preprocessed_flagged(source.as_bytes(), Dialect::Fteqcc, edits, &[])
}

/// The codes of every diagnostic, warnings included.
fn codes(parsed: &Parse) -> Vec<Code> {
    parsed.diagnostics().iter().map(|d| d.code).collect()
}

/// The byte of the first occurrence of `needle` in `source`.
fn at(source: &str, needle: &str) -> usize {
    source.find(needle).unwrap()
}

/// The edit a `#pragma flag` that names `set` makes.
fn pragma(set: &[(FteFlag, bool)]) -> FlagEdit {
    let names: Vec<FteFlag> = set.iter().map(|&(flag, _)| flag).collect();
    let mut now = CompilerFlags::default();
    for &(flag, on) in set {
        now = now.with(flag, on);
        if flag == FteFlag::Cpriority {
            now = now.with_priorities(if on { Priorities::C } else { Priorities::Qc });
        }
    }
    FlagEdit::of_flags(&names, now)
}

/// The expression `return`ed by the first function of `parsed`, as an s-expression.
fn returned(parsed: &Parse) -> String {
    let ret = parsed
        .syntax()
        .descendants()
        .find_map(ast::ReturnStmt::cast)
        .unwrap();
    sexpr(&ret.value().unwrap())
}

// spec FTEF-21.2: `extern` and a string is a block of definitions, or one definition after any
// `;`; its tokens are lexed with the flags the string sets, `"C"` in any case, its parts joined
#[test]
fn extern_blocks_are_read() {
    let block = fte("extern \"C\" { float x; float y; }\nfloat z;\n");
    assert_eq!(block.errors().count(), 0);
    let tree = block.tree().unwrap();
    assert_eq!(tree.declarations().count(), 3);
    assert!(
        block
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ExternBlock)
    );
    let one = fte("extern \"C\" ;; float x;\n");
    assert_eq!(one.errors().count(), 0);
    assert_eq!(one.tree().unwrap().declarations().count(), 1);
    for source in [
        "extern \"C\" { string s = \"\\9\"; }\n",
        "extern \"c\" { string s = \"\\9\"; }\n",
        "extern \"\" \"C\" { string s = \"\\9\"; }\n",
        "extern \"C\" string s = \"\\9\";\n",
    ] {
        assert!(codes(&fte(source)).contains(&Code::E0103), "{source}");
    }
    for source in [
        "extern \"QC\" { string s = \"\\9\"; }\n",
        "extern \"C++\" { string s = \"\\9\"; }\n",
        "string s = \"\\9\";\n",
    ] {
        assert!(!codes(&fte(source)).contains(&Code::E0103), "{source}");
    }
    let off = [(0, whole(&[(FteFlag::Qcfuncs, false)]))];
    let source = "extern \"QC\" { string s = \"\\9\"; }\n";
    assert!(flagged(source, &off).errors().next().is_none());
    // The flags at each token, `qcfuncs` from the token after the string.
    let parsed = fte("extern \"C\" { float x; }\nfloat y;\n");
    let source = "extern \"C\" { float x; }\nfloat y;\n";
    assert!(
        parsed
            .lex_flags_at(at(source, "\"C\""))
            .has(LexFlag::QcFuncs)
    );
    assert!(!parsed.lex_flags_at(at(source, "{")).has(LexFlag::QcFuncs));
    assert!(parsed.flags_at(at(source, "x;")).has(FteFlag::Assumevar));
    assert!(!parsed.flags_at(at(source, "y;")).has(FteFlag::Assumevar));
}

// spec FTEF-21.2: a block's flags hold to the token after its last, which FTEQCC lexed before it
// restored them, so the word after a `}` is the block's and the string after it is not
#[test]
fn a_blocks_flags_end_after_the_next_token() {
    let source = "extern \"C\" { float y; } string s = \"\\9\";\n";
    let parsed = fte(source);
    assert!(parsed.errors().next().is_none());
    assert!(
        !parsed
            .lex_flags_at(at(source, "string"))
            .has(LexFlag::QcFuncs)
    );
    assert!(parsed.lex_flags_at(at(source, "s =")).has(LexFlag::QcFuncs));
    let off = [(0, whole(&[(FteFlag::Qcfuncs, false)]))];
    let source = "extern \"QC\" { float y; }\nstring s = \"\\9\";\n";
    let parsed = flagged(source, &off);
    assert!(parsed.lex_flags_at(at(source, "y;")).has(LexFlag::QcFuncs));
    assert!(
        parsed
            .lex_flags_at(at(source, "string"))
            .has(LexFlag::QcFuncs)
    );
    assert!(!parsed.lex_flags_at(at(source, "s =")).has(LexFlag::QcFuncs));
}

// spec FTEF-21.2: a file that ends in a block is "expected }", after another qualifier the
// string is no block, and in a body a block's definitions are locals
#[test]
fn extern_block_edges() {
    assert!(
        fte("extern \"C\" {\n")
            .errors()
            .any(|d| d.message.starts_with("expected }"))
    );
    assert!(
        fte("const extern \"C\" { float x; }\n")
            .errors()
            .next()
            .is_some()
    );
    let body = fte("void() g = { extern \"C\" { float y = 1; } y = 2; };\n");
    assert!(body.errors().next().is_none());
    let block = body
        .syntax()
        .descendants()
        .find_map(ast::ExternBlock::cast)
        .unwrap();
    assert!(
        block
            .items()
            .iter()
            .all(|n| n.kind() == SyntaxKind::LocalDecl)
    );
    assert!(
        fte("void() g = { local extern \"C\" { float y = 1; } y = 2; };\n")
            .errors()
            .next()
            .is_none()
    );
    assert!(
        fte("void() g = { extern \"C\" { y = 2; } };\n")
            .errors()
            .next()
            .is_some()
    );
    assert!(fte("extern \"C\" { }\n").errors().next().is_none());
}

// spec FTEF-21.1: the preprocessor's changes and the blocks' replay in FTEQCC's order
// spec FTEF-21.3: a pragma in a block that names one of the six flags holds to its end, one
// that names another flag after it
#[test]
fn pragmas_in_blocks() {
    let source = "extern \"C\" {\nfloat x; }\nstring s = \"\\9\";\n";
    let on = [(at(source, "float"), pragma(&[(FteFlag::Qcfuncs, true)]))];
    let parsed = flagged(source, &on);
    assert!(
        parsed
            .lex_flags_at(at(source, "float"))
            .has(LexFlag::QcFuncs)
    );
    assert!(parsed.errors().next().is_none());
    let source = "extern \"C\" {\nfloat x; }\nstring s = \"\\xff\";\n";
    let utf8 = [(at(source, "float"), pragma(&[(FteFlag::Utf8strings, true)]))];
    let parsed = flagged(source, &utf8);
    assert!(
        parsed
            .lex_flags_at(at(source, "\"\\x"))
            .has(LexFlag::Utf8Strings)
    );
    assert!(
        parsed
            .lex_flags_at(at(source, "\"\\x"))
            .has(LexFlag::QcFuncs)
    );
}

// spec FTEF-21.4: the priorities a pragma that names `cpriority` sets hold, through an `extern
// "C"` block that sets only the flag and after one that restores the flag
// spec FTEF-21.5: C's priorities
#[test]
fn operator_priorities() {
    let c = [(0, pragma(&[(FteFlag::Cpriority, true)]))];
    let program = |e: &str| format!("float a, b, c, d, x;\nfloat() h = {{ return {e}; }};\n");
    let qc = |e: &str| returned(&fte(&program(e)));
    let cp = |e: &str| returned(&flagged(&program(e), &c));
    assert_eq!(qc("a == b & c"), "(== a (& b c))");
    assert_eq!(cp("a == b & c"), "(& (== a b) c)");
    assert_eq!(qc("!a == b"), "(! (== a b))");
    assert_eq!(cp("!a == b"), "(== (! a) b)");
    assert_eq!(qc("a || b ? c : d"), "(|| a (?: b c d))");
    assert_eq!(cp("a || b ? c : d"), "(?: (|| a b) c d)");
    assert_eq!(cp("x = a ? b : c"), "(= x (?: a b c))");
    assert_eq!(
        cp("a + b * c << d < x == a & b ^ c | d && x || a"),
        "(|| (&& (| (^ (& (== (< (<< (+ a (* b c)) d) x) a) b) c) d) x) a)"
    );
    assert_eq!(qc("a << b + c"), "(+ (<< a b) c)");
    assert_eq!(cp("a << b + c"), "(<< a (+ b c))");
    assert_eq!(cp("a = b = c"), "(= a (= b c))");
    // `extern "C"` sets the flag, not the priorities.
    let block = "extern \"C\" { float a, b; float() h = { return !a == b; }; }\n";
    assert_eq!(returned(&fte(block)), "(! (== a b))");
}

// spec FTEF-21.6: the operand of `!` or `~` warns of each binary operator it takes
// spec QCD-7.68: W0227 "suggest parenthesis for unary operator that applies to multiple terms"
#[test]
fn unary_operands_that_take_operators() {
    let warnings = |e: &str| {
        let parsed = fte(&format!("float a, b;\nfloat() h = {{ return {e}; }};\n"));
        parsed
            .diagnostics()
            .iter()
            .filter(|d| d.code == Code::W0227)
            .count()
    };
    assert_eq!(warnings("!a == b"), 1);
    assert_eq!(warnings("!a * b"), 1);
    assert_eq!(warnings("~a & b"), 1);
    assert_eq!(warnings("!(a) == b"), 1);
    assert_eq!(warnings("!a == b == a"), 2);
    assert_eq!(warnings("!a && b"), 0);
    assert_eq!(warnings("!(a == b)"), 0);
    assert_eq!(warnings("-a == b"), 0);
    let message = fte("float a, b;\nfloat() h = { return !a * b; };\n")
        .diagnostics()
        .iter()
        .find(|d| d.code == Code::W0227)
        .map(|d| d.message.clone());
    assert_eq!(
        message.as_deref(),
        Some("suggest parenthesis for unary operator that applies to multiple terms")
    );
    let c = [(0, pragma(&[(FteFlag::Cpriority, true)]))];
    let parsed = flagged("float a, b;\nfloat() h = { return !a == b; };\n", &c);
    assert_eq!(codes(&parsed), []);
}

// spec FTEF-21.7: with `qcfuncs` off a declaration's `*` and parameter lists are each
// declarator's, and `(*name)(params)` is C's function pointer, which a type read with `qcfuncs`
// on refuses as its first declarator
#[test]
fn c_declarators() {
    let off = [(0, whole(&[(FteFlag::Qcfuncs, false)]))];
    let parsed = flagged("float *a, b;\n", &off);
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    assert!(matches!(decl.ty(), Some(ast::Type::Simple(_))));
    let depths: Vec<usize> = decl.declarators().map(|d| d.pointer_depth()).collect();
    assert_eq!(depths, [1, 0]);
    let qc = fte("float *a, b;\n");
    let decl = qc.tree().unwrap().declarations().next().unwrap();
    assert!(matches!(decl.ty(), Some(ast::Type::Pointer(_))));
    let think = flagged(".void() think;\n", &off);
    let decl = think.tree().unwrap().declarations().next().unwrap();
    assert!(matches!(decl.ty(), Some(ast::Type::Field(_))));
    let declarator = decl.declarators().next().unwrap();
    assert_eq!(declarator.prefix_params().len(), 1);
    let pointer = flagged("float (*fp)(float), x;\n", &off);
    assert!(pointer.errors().next().is_none());
    let declarator = pointer
        .tree()
        .unwrap()
        .declarations()
        .next()
        .unwrap()
        .declarators()
        .next()
        .unwrap();
    assert_eq!(declarator.pointer_form(), Some(1));
    assert!(declarator.params().is_some());
    assert_eq!(declarator.name().unwrap().text(), b"fp");
    assert!(fte("float (*fp)(float);\n").errors().next().is_some());
    assert!(fte("float a, (*fp)(float);\n").errors().next().is_none());
    assert!(fte("float a, (float x) g;\n").errors().next().is_none());
    assert!(
        flagged("void(float() cb) g;\n", &off)
            .errors()
            .next()
            .is_some()
    );
    assert!(fte("void(float() cb) g;\n").errors().next().is_none());
    assert!(
        flagged("typedef void (*cb)(float);\n", &off)
            .errors()
            .next()
            .is_none()
    );
    assert!(
        flagged("typedef float *fp;\n", &off)
            .errors()
            .next()
            .is_none()
    );
}

// spec FTEF-21.8: with `qcfuncs` off a first parameter that is a lone name is the parameter's
// name
#[test]
fn c_parameters() {
    let off = [(0, whole(&[(FteFlag::Qcfuncs, false)]))];
    let parsed = flagged("void h(x);\n", &off);
    let param = parsed
        .syntax()
        .descendants()
        .find_map(ast::Param::cast)
        .unwrap();
    assert!(param.ty().is_none());
    assert_eq!(param.name().unwrap().text(), b"x");
    let parsed = flagged("void h(x, y);\n", &off);
    let params: Vec<ast::Param> = parsed
        .syntax()
        .descendants()
        .filter_map(ast::Param::cast)
        .collect();
    assert!(params[1].ty().is_some());
    let parsed = fte("void h(x);\n");
    let param = parsed
        .syntax()
        .descendants()
        .find_map(ast::Param::cast)
        .unwrap();
    assert!(param.ty().is_some());
}

// spec FTEF-21.9: attribute lists after a qualifier, and at file level with `attributes` on;
// their warnings
// spec QCD-7.69: W0228 "Unknown attribute \"NAME\"" and "accumulating an accumulating
// accumulation"
#[test]
fn attribute_lists() {
    let parsed = fte("const [[eraseable, alias(y), __deprecated(\"a\")]] float x;\n");
    assert_eq!(codes(&parsed), []);
    let list = parsed
        .syntax()
        .descendants()
        .find_map(ast::AttributeList::cast)
        .unwrap();
    let names: Vec<String> = list
        .attributes()
        .map(|a| a.name().unwrap().text_lossless())
        .collect();
    assert_eq!(names, ["eraseable", "alias", "__deprecated"]);
    assert!(fte("[[eraseable]] float x;\n").errors().next().is_some());
    let on = [(0, whole(&[(FteFlag::Attributes, true)]))];
    assert!(
        flagged("[[eraseable]] float x;\n", &on)
            .errors()
            .next()
            .is_none()
    );
    assert!(fte("const [[]] float x;\n").errors().next().is_none());
    assert!(fte("const [] float x;\n").errors().next().is_some());
    assert!(
        fte("const [[__deprecated(5)]] float x;\n")
            .errors()
            .any(|d| d.message == "expected string")
    );
    let unknown = flagged("[[foo(1, 2)]] float x;\n", &on);
    let messages: Vec<String> = unknown
        .diagnostics()
        .iter()
        .filter(|d| d.code == Code::W0228)
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(
        messages,
        ["Unknown attribute \"foo\"", "Unknown attribute \"2\""]
    );
    let twice = fte("const __accumulate [[accumulate]] void() h = { };\n");
    assert_eq!(
        twice.diagnostics()[0].message,
        "accumulating an accumulating accumulation"
    );
    assert!(
        fte("void() g = { static [[eraseable]] float x; };\n")
            .errors()
            .next()
            .is_none()
    );
    assert!(
        fte("void() g = { local [[eraseable]] float x; };\n")
            .errors()
            .next()
            .is_some()
    );
}

// spec FTEF-21.14: `typedef` among the other qualifiers, before or after them, and a type left
// out after it
#[test]
fn typedef_qualifiers() {
    for source in [
        "static typedef float t;\n",
        "typedef static float t;\n",
        "noref typedef float t;\n",
        "typedef static x;\n",
        "typedef x;\n",
    ] {
        let parsed = fte(source);
        assert!(parsed.errors().next().is_none(), "{source}");
        let td = parsed
            .syntax()
            .descendants()
            .find_map(ast::TypedefDecl::cast)
            .unwrap();
        assert!(td.name().is_some(), "{source}");
    }
    let td = fte("typedef x;\n")
        .syntax()
        .descendants()
        .find_map(ast::TypedefDecl::cast)
        .unwrap();
    assert!(td.syntax().children().find_map(ast::Type::cast).is_none());
}

// spec FTEF-21.14: in a body, `typedef` after the qualifiers a definition starts with
#[test]
fn typedef_qualifiers_in_a_body() {
    let parsed = fte("void() g = { const typedef float t; };\n");
    assert!(parsed.errors().next().is_none());
    assert!(
        parsed
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::TypedefDecl)
    );
}

// spec FTEF-21.1: the token after a block is lexed with its flags and read with them restored
#[test]
fn the_token_after_a_block_is_read_restored() {
    let source = "extern \"C\" int a; float *p, q;\n";
    let parsed = fte(source);
    let float = at(source, "float");
    assert!(!parsed.lex_flags_at(float).has(LexFlag::QcFuncs));
    assert!(parsed.flags_at(float).has(FteFlag::Qcfuncs));
    assert!(!parsed.flags_at(float).has(FteFlag::AssumeInteger));
    let decl = parsed.tree().unwrap().declarations().nth(1).unwrap();
    assert!(matches!(decl.ty(), Some(ast::Type::Pointer(_))));
}
