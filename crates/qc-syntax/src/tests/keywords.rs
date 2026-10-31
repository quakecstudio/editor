// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's keywords as the parser reads them, with the flags in force at each token
//! (`docs/spec/dialect-fteqcc.md` FTEF-20).

use super::fte_with;
use crate::kind::SyntaxKind as K;
use crate::{Code, Dialect, Keyword, KeywordEdit, Keywords, Parse, parse_preprocessed_flagged};

/// `body` as the statements of a function `f`.
fn in_body(body: &str) -> String {
    format!("void() f =\n{{\n\t{body}\n}};\n")
}

/// The keywords with `keyword` set to `on`.
fn with(keyword: Keyword, on: bool) -> Keywords {
    Keywords::default().with(keyword, on)
}

/// The error codes of `source` read with `keywords`.
fn errors(source: &str, keywords: Keywords) -> Vec<Code> {
    fte_with(source, keywords)
        .errors()
        .map(|d| d.code)
        .collect()
}

/// Whether `parsed` has a node of `kind`.
fn has(parsed: &Parse, kind: K) -> bool {
    parsed.syntax().descendants().any(|n| n.kind() == kind)
}

/// Whether `source` read with `keywords` has a node of `kind`.
fn holds(source: &str, keywords: Keywords, kind: K) -> bool {
    has(&fte_with(source, keywords), kind)
}

// spec FTEF-20.1: the parser reads each keyword with the flags in force at its token, a mark
// holding from the byte it is at; under the vanilla dialect the words are id's, exactly
#[test]
fn keywords_change_where_they_are_marked() {
    let source = "float for;\nvoid() g = { for = 1; };\nvoid() h = { for (;;) {} };\n";
    let g = source.find("void() g").unwrap();
    let h = source.find("void() h").unwrap();
    let marks = [
        (0, KeywordEdit::whole(Keywords::default())),
        (g, KeywordEdit::whole(with(Keyword::For, false))),
        (h, KeywordEdit::whole(Keywords::default())),
    ];
    let parsed = parse_preprocessed_flagged(source.as_bytes(), Dialect::Fteqcc, &[], &marks, &[]);
    assert_eq!(parsed.errors().count(), 0);
    assert!(has(&parsed, K::ForStmt));
    assert!(!parsed.keywords_at(g).has(Keyword::For));
    assert!(parsed.keywords_at(h).has(Keyword::For));
    assert!(parsed.keywords_at(0).has(Keyword::For));
    assert_eq!(parsed.keywords().len(), 3);
    // With no mark the defaults hold.
    assert!(
        crate::parse_with(b"float x;\n", Dialect::Fteqcc)
            .keywords_at(9)
            .has(Keyword::For)
    );
    // Vanilla reads no `__` spelling as a keyword.
    let vanilla = crate::parse(in_body("__while (1) {}").as_bytes());
    assert!(!has(&vanilla, K::WhileStmt));
}

// spec FTEF-20.2: a `__` spelling is the keyword in any case whether or not it is on, a bare
// word only with it on, in any case under `caseinsens`; `QCC_PR_CheckName` takes no `__`
#[test]
fn check_keyword_spellings() {
    let off = with(Keyword::For, false);
    assert!(holds(&in_body("__FoR (;;) {}"), off, K::ForStmt));
    assert!(!holds(&in_body("for (;;) {}"), off, K::ForStmt));
    let any_case = Keywords::default().with_case_insensitive(true);
    assert!(holds(&in_body("FOR (;;) {}"), any_case, K::ForStmt));
    assert!(!holds(
        &in_body("FOR (;;) {}"),
        Keywords::default(),
        K::ForStmt
    ));
    assert!(holds(&in_body("IF (1) {}"), any_case, K::IfStmt));
    assert!(holds(
        &in_body("__while (1) {}"),
        Keywords::default(),
        K::WhileStmt
    ));
    // `do` expects `while` exactly.
    assert_eq!(
        errors(&in_body("do {} __while (1);"), Keywords::default()),
        [Code::E0204]
    );
    // An accessor's words are `QCC_PR_CheckName`'s.
    let accessor = "accessor A : float { GET float = { return 0; } };\n";
    assert_eq!(errors(accessor, any_case), []);
    assert_ne!(errors(accessor, Keywords::default()), []);
    let prefixed = "accessor A : float { __get float() = { return 0; } };\n";
    assert_ne!(errors(prefixed, Keywords::default()), []);
}

// spec FTEF-20.4: with `kce` off a definition named as a keyword turns it off from the next
// token: a global, a local, a field, a body's parameters, an enum's names, a next think function
// and a `using` alias; not a prototype's parameters, a typedef, a class member, a function named
// `break` or, on a Hexen II target, `float`
#[test]
fn definitions_turn_keywords_off_without_kce() {
    let lone = Keywords::default().with_coexist(false);
    let after = |source: &str, keywords: Keywords| {
        let parsed = fte_with(source, keywords);
        parsed.keywords_at(source.len())
    };
    for source in [
        "float for;\n",
        "void() g = { local float for; };\n",
        ".float for;\n",
        "void(float for) g = {};\n",
        "enum { for };\n",
        "void() g = [1, for] {};\n",
        "float x;\nvoid() g = { __using (x = for) {} };\n",
        "float for = 1, x;\n",
    ] {
        assert!(!after(source, lone).has(Keyword::For), "{source}");
        assert!(
            after(source, Keywords::default()).has(Keyword::For),
            "{source}"
        );
    }
    for source in [
        "void(float for) g;\n",
        "void(float for) g = #6;\n",
        "typedef float for;\n",
        "class C { float for; };\n",
        "struct s { float for; };\n",
        "float FOR;\n",
        "float __for;\n",
    ] {
        assert!(after(source, lone).has(Keyword::For), "{source}");
    }
    assert!(after("void() break = {};\n", lone).has(Keyword::Break));
    assert!(!after("float break;\n", lone).has(Keyword::Break));
    assert!(!after("float float;\n", lone).has(Keyword::Float));
    assert!(after("float float;\n", lone.with_hexen2_target(true)).has(Keyword::Float));
    // A body defines only its own type's parameters, and `Class::member` names no keyword.
    let stale = "void(float for) h;\nvoid() g = { for (;;) {} };\n";
    assert!(holds(stale, lone, K::ForStmt));
    assert!(after("void() C::for = {};\n", lone).has(Keyword::For));
    // A definition's mark follows a pragma's at the same token.
    let marks = [
        (0, KeywordEdit::whole(lone)),
        (9, KeywordEdit::whole(lone.with(Keyword::For, true))),
    ];
    let parsed = parse_preprocessed_flagged(
        b"float for\n;\nfloat y;\n",
        Dialect::Fteqcc,
        &[],
        &marks,
        &[],
    );
    assert!(parsed.keywords().windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(!parsed.keywords_at(12).has(Keyword::For));
    // The keyword is a name from the token after the definition.
    let source = "float for;\nvoid() g = { for (;;) {} };\n";
    assert!(!holds(source, lone, K::ForStmt));
    assert!(holds(source, Keywords::default(), K::ForStmt));
}

// spec FTEF-20.5: the statements each keyword starts, with its flag on or in its `__` spelling
#[test]
fn keyword_statements() {
    let on = |keyword| with(keyword, true);
    let defaults = Keywords::default();
    assert!(holds(
        &in_body("loop { break; }"),
        on(Keyword::Loop),
        K::LoopStmt
    ));
    assert!(!holds(&in_body("loop { break; }"), defaults, K::LoopStmt));
    assert!(holds(&in_body("__loop {}"), defaults, K::LoopStmt));
    let until = fte_with(&in_body("until (1) {}"), on(Keyword::Until));
    assert!(
        until
            .syntax()
            .descendants_with_tokens()
            .any(|e| e.kind() == K::UntilKw)
    );
    assert!(holds(
        &in_body("do {} until (1);"),
        on(Keyword::Until),
        K::DoStmt
    ));
    assert_eq!(errors(&in_body("do {} until (1);"), on(Keyword::Until)), []);
    assert!(holds(
        &in_body("state [1, f];"),
        on(Keyword::State),
        K::StateStmt
    ));
    assert!(holds(&in_body("__state [1, f];"), defaults, K::StateStmt));
    assert_eq!(errors(&in_body("__state 1;"), defaults), [Code::E0204]);
    assert!(holds(
        &in_body("thinktime e : 0.1;"),
        on(Keyword::Thinktime),
        K::ThinktimeStmt
    ));
    assert_eq!(
        errors(&in_body("__thinktime e 0.1;"), defaults),
        [Code::E0204]
    );
    assert!(holds(
        &in_body("using (x) {}"),
        on(Keyword::Using),
        K::UsingStmt
    ));
    let using = fte_with(&in_body("__using (a, b = c) {}"), defaults);
    assert_eq!(
        using
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::UsingAlias)
            .count(),
        2
    );
    assert!(holds(&in_body("__using a = b;"), defaults, K::UsingStmt));
    assert!(holds(&in_body("__exit;"), defaults, K::ExitStmt));
    assert!(!holds(&in_body("exit;"), defaults, K::ExitStmt));
    assert!(holds(
        &in_body("asm { DONE; MUL_F a, b, c; }"),
        defaults,
        K::AsmStmt
    ));
    assert!(!holds(
        &in_body("asm DONE;"),
        with(Keyword::Asm, false),
        K::AsmStmt
    ));
    // A keyword starts its statement whatever follows it; `break (` is a call.
    assert_eq!(
        errors(&in_body("for = 1;"), defaults).first(),
        Some(&Code::E0204)
    );
    assert!(!holds(&in_body("break();"), defaults, K::BreakStmt));
    assert!(holds(&in_body("break;"), defaults, K::BreakStmt));
}

// spec QCD-6.11: E0511 "Bad op code name NAME" for an `asm` instruction that names no opcode
#[test]
fn bad_asm_opcodes() {
    let parsed = fte_with(&in_body("asm NOT_AN_OP;"), Keywords::default());
    let found: Vec<(Code, String)> = parsed
        .errors()
        .map(|d| (d.code, d.message.clone()))
        .collect();
    assert_eq!(
        found,
        [(Code::E0511, "Bad op code name NOT_AN_OP".to_owned())]
    );
    assert_eq!(
        errors(
            &in_body("asm { local float x; DONE; }"),
            Keywords::default()
        ),
        []
    );
}

// spec FTEF-20.14: a body's statement defines locals at a registered type's name or at one of
// `QCC_PR_ParseStatement`'s words, exactly, with its keyword on
#[test]
fn local_definition_words() {
    let on = |keyword| with(keyword, true);
    let defaults = Keywords::default();
    assert!(holds(&in_body("long x;"), on(Keyword::Long), K::LocalDecl));
    assert!(!holds(
        &in_body("__long x;"),
        on(Keyword::Long),
        K::LocalDecl
    ));
    assert!(holds(
        &in_body("auto float x;"),
        on(Keyword::Auto),
        K::LocalDecl
    ));
    assert!(!holds(
        &in_body("__typedef float t;"),
        defaults,
        K::TypedefDecl
    ));
    assert!(holds(
        &in_body("typedef float t;"),
        defaults,
        K::TypedefDecl
    ));
    assert!(holds(
        &in_body("struct s { float a; } x;"),
        defaults,
        K::LocalDecl
    ));
    assert!(!holds(
        &in_body("var = 1;"),
        with(Keyword::Var, false),
        K::LocalDecl
    ));
    assert!(holds(&in_body("__int32 x;"), defaults, K::LocalDecl));
    let any_case = defaults.with_case_insensitive(true);
    assert!(!holds(&in_body("INT x;"), any_case, K::LocalDecl));
}

// spec FTEF-20.7: the declaration qualifiers with their keywords; `nonstatic` only outside a
// function, `deprecated` only as `__deprecated`
#[test]
fn qualifiers_follow_their_keywords() {
    assert_eq!(
        errors("const float x = 1;\n", with(Keyword::Const, false)),
        [Code::E0203]
    );
    assert_eq!(errors("unused float x;\n", with(Keyword::Unused, true)), []);
    assert_ne!(errors("unused float x;\n", Keywords::default()), []);
    assert_eq!(errors("__unused float x;\n", Keywords::default()), []);
    assert_eq!(errors("nonstatic float x;\n", Keywords::default()), []);
    assert!(!holds(
        &in_body("local nonstatic float x;"),
        Keywords::default(),
        K::Qualifier
    ));
    assert!(holds(
        "nonstatic float x;\n",
        Keywords::default(),
        K::Qualifier
    ));
    assert_ne!(errors("deprecated float x;\n", Keywords::default()), []);
    assert_eq!(
        errors("__Deprecated(\"old\") float x;\n", Keywords::default()),
        []
    );
}

// spec FTEF-20.8: the type keywords, `class` only with `qcfuncs` on at it
#[test]
fn type_keywords_follow_their_flags() {
    let struct_off = with(Keyword::Struct, false);
    assert_ne!(errors("struct s { float a; };\n", struct_off), []);
    assert_eq!(errors("__struct s { float a; };\n", struct_off), []);
    assert_ne!(errors("enum { A };\n", with(Keyword::Enum, false)), []);
    assert_ne!(
        errors("accessor A : float;\n", with(Keyword::Accessor, false)),
        []
    );
    assert_eq!(errors("class C { };\n", Keywords::default()), []);
    let no_qcfuncs = super::whole(&[(qc_preproc::FteFlag::Qcfuncs, false)]);
    let parsed = parse_preprocessed_flagged(
        b"class C { };\n",
        Dialect::Fteqcc,
        &[(0, no_qcfuncs)],
        &[(0, KeywordEdit::whole(Keywords::default()))],
        &[],
    );
    assert_ne!(parsed.errors().count(), 0);
    assert_ne!(
        errors("void(const float x) g = #6;\n", with(Keyword::Const, false)),
        []
    );
    assert_eq!(
        errors("volatile float v;\n", with(Keyword::Volatile, true)),
        []
    );
}

// spec FTEF-20.9: the C type words with their keywords in force
#[test]
fn c_type_words_follow_their_flags() {
    // With its keyword off a C word is a type's name, and the word after it the declarator's.
    assert_eq!(errors("long int l;\n", with(Keyword::Long, true)), []);
    let parsed = fte_with("long int l;\n", with(Keyword::Long, true));
    let ty = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == K::SimpleType)
        .unwrap();
    assert_eq!(ty.text(), b"long int");
    assert_ne!(errors("long int l;\n", Keywords::default()), []);
    assert_ne!(
        errors("unsigned int u;\n", with(Keyword::Unsigned, false)),
        []
    );
    assert_eq!(
        errors("__unsigned u;\n", with(Keyword::Unsigned, false)),
        []
    );
    assert_eq!(
        errors("register int r;\n", with(Keyword::Register, true)),
        []
    );
}

// spec FTEF-20.10: `optional` with its keyword, `inout`, `out` and `in` with `keyword_inout`
#[test]
fn parameter_qualifiers_follow_their_flags() {
    let inout = with(Keyword::Inout, true);
    for word in ["out", "in", "inout"] {
        let source = format!("void({word} float x) g = #6;\n");
        assert_eq!(errors(&source, inout), [], "{word}");
        assert_ne!(errors(&source, Keywords::default()), [], "{word}");
    }
    assert_ne!(
        errors(
            "void(optional float x) g = #6;\n",
            with(Keyword::Optional, false)
        ),
        []
    );
    assert_eq!(
        errors(
            "void(__OPTIONAL float x) g = #6;\n",
            with(Keyword::Optional, false)
        ),
        []
    );
}

// spec FTEF-20.11: a class's member words are always on, `__` in any case and under
// `caseinsens`
#[test]
fn class_member_words() {
    let defaults = Keywords::default();
    let qualified = |source: &str, keywords| {
        fte_with(source, keywords)
            .syntax()
            .descendants()
            .filter(|n| n.kind() == K::Qualifier)
            .count()
    };
    assert_eq!(qualified("class C { __STATIC float x; };\n", defaults), 1);
    assert_eq!(qualified("class C { STATIC float x; };\n", defaults), 0);
    assert_eq!(
        qualified(
            "class C { STATIC float x; };\n",
            defaults.with_case_insensitive(true)
        ),
        1
    );
    assert!(holds(
        "class C { __public: float x; };\n",
        defaults,
        K::AccessLabel
    ));
}

// spec FTEF-20.16: `enum`'s `class` and an accessor's `get`, `set` and `inline` are names
// matched exactly: `__get` ends the accessor's body as "expected }", `__class` after `enum` is no
// `class`, and a class member's `__get` stays a keyword (FTEF-20.11)
#[test]
fn enum_class_and_accessor_words_are_names() {
    let defaults = Keywords::default();
    let messages = |source: &str| -> Vec<String> {
        fte_with(source, defaults)
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect()
    };
    let size = "float(float x) size = { return x; };\n";
    assert_eq!(
        messages(&format!(
            "{size}accessor buf : float {{ __get float len = size; }};\n"
        )),
        ["expected }, found __get"]
    );
    assert_eq!(
        messages(&format!(
            "{size}accessor buf : float {{ __inline get float len = size; }};\n"
        )),
        ["expected }, found __inline"]
    );
    assert_eq!(
        messages(&format!(
            "{size}accessor buf : float {{ GET float len = size; }};\n"
        )),
        ["expected }, found GET"]
    );
    assert!(
        messages(&format!(
            "{size}accessor buf : float {{ get float len = size; }};\n"
        ))
        .is_empty()
    );
    assert!(messages("enum class X : float { A };\n").is_empty());
    assert_eq!(
        messages("enum __class X : float { A };\n"),
        ["expected {, found X"]
    );
    assert!(holds(
        "class C { __get float x; };\n",
        defaults,
        K::AccessorMember
    ));
}

// spec FTEF-20.12: a type followed by `;` is a definition of no name, at file level and in a
// body
#[test]
fn a_type_alone_defines_nothing() {
    let defaults = Keywords::default();
    assert_eq!(errors("float;\n", defaults), []);
    assert_eq!(errors("const float;\n", defaults), []);
    assert_eq!(errors(&in_body("float;"), defaults), []);
    assert_eq!(errors(&in_body("enum { A, B };"), defaults), []);
    assert_eq!(errors(&in_body("float = 3;"), defaults), [Code::E0201]);
}

// spec FTEF-20.13: the reacc keywords are off, so their `__` spellings are names
#[test]
fn reacc_keywords_are_names() {
    let defaults = Keywords::default();
    assert_eq!(errors("float __function;\n", defaults), []);
    assert_eq!(errors("float __CodeSys;\n", defaults), []);
    assert!(!holds(&in_body("exit;"), defaults, K::ExitStmt));
}

// spec FTEF-20.5: the views of the keyword statements give their parts
#[test]
fn keyword_statement_views() {
    use crate::ast::{self, AstNode as _};
    let node = |body: &str, kind: K| {
        fte_with(&in_body(body), Keywords::default())
            .syntax()
            .descendants()
            .find(|n| n.kind() == kind)
            .unwrap()
    };
    let asm = ast::AsmStmt::cast(node("asm { local float x; DONE; }", K::AsmStmt)).unwrap();
    assert_eq!(asm.locals().count(), 1);
    let looped = ast::LoopStmt::cast(node("__loop { break; }", K::LoopStmt)).unwrap();
    assert!(looped.body().is_some());
    let think = ast::ThinktimeStmt::cast(node("__thinktime e : 0.1;", K::ThinktimeStmt)).unwrap();
    assert_eq!(think.operands().count(), 2);
    let state = ast::StateStmt::cast(node("__state [1, f];", K::StateStmt)).unwrap();
    assert!(state.state().is_some());
    let using = ast::UsingStmt::cast(node("__using (a, b = c) {}", K::UsingStmt)).unwrap();
    assert!(using.body().is_some());
    let names: Vec<(String, String)> = using
        .aliases()
        .map(|a| {
            (
                a.target().unwrap().text_lossless(),
                a.alias().unwrap().text_lossless(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("a".to_owned(), "a".to_owned()),
            ("b".to_owned(), "c".to_owned())
        ]
    );
}
