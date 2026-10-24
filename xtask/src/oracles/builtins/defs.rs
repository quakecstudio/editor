// SPDX-License-Identifier: GPL-3.0-or-later

//! The `QCC_PR_ParseDefs` family of the builtin differential (P2-130, P2-131): `extern` blocks
//! (`docs/spec/dialect-fteqcc.md` FTEF-21.17) in the forms they nest, end and compare their
//! string in, at file level and in a body, and a type name written in another case where
//! `-Fcaseinsens` finds it (FTEF-19.29, FTEF-20.14). Each program binds `f` first, so it is
//! accepted only when FTEQCC read the whole of it.

use qc_syntax::Dialect;

use super::Case;
use super::directives::BIND;

/// How many levels the nesting forms are repeated to.
const DEPTHS: [usize; 3] = [1, 2, 3];

/// The two linkages every shape is written with.
const CORE_LINKAGES: &[&str] = &["\"C\"", "\"QC\""];

/// The other strings an `extern` is written with: the linkages in other spellings, strings that
/// differ from them only after a NUL or in what follows, and parts that join. They are read with
/// the shapes of [`PROBES`].
const LINKAGES: &[&str] = &[
    "\"c\"",
    "\"qc\"",
    "\"\"",
    "\"X\"",
    "\"C \"",
    "\"C\\x00x\"",
    "\"Q\\x00C\"",
    "\"\\x00C\"",
    "\"\" \"C\"",
    "\"C\" \"\"",
    "\"C\" \"QC\"",
    "\"QC\" \"C\"",
    "\"C\"\n\"X\"",
    "\"C\" /* c */ \"X\"",
];

/// The shapes the other linkages are read with: the probe `string s = "\9";`, which is an error
/// where `qcfuncs` is off, in and after a block, a single definition, and none.
const PROBES: &[&str] = &[
    "{ string s = \"\\9\"; }",
    "{ float y; } string s = \"\\9\";",
    "string s = \"\\9\";",
    "{ }",
    "float y;",
];

/// What follows the string and what it holds: the forms of `extern`'s block and of its single
/// definition, with a probe `string s = "\9";` that is an error where `qcfuncs` is off.
const SHAPES: &[&str] = &[
    "{ string s = \"\\9\"; }",
    "{ float y; } string s = \"\\9\";",
    "{ }",
    "{ } float y;",
    "{ ; }",
    "{ ;; }",
    "{ ; float y; }",
    "{ float y; ; }",
    "{ float y; ;; }",
    "{ ;; float y; ;; }",
    "{ float y; } ;",
    "{ float y; };",
    "float y;",
    "; float y;",
    ";;; float y;",
    "string s = \"\\9\";",
    ";",
    ";;;",
    "",
    "\"QC\"",
    "\"C\" {",
    "\"C\" { }",
    "{",
    "{ float y;",
    "}",
    "float y; }",
    "extern \"C\" { float y; }",
    "{ extern \"C\" float y; }",
    "{ extern \"C\" ; }",
    "{ extern \"C\" }",
    "{ extern \"C\" { } }",
    "{ extern \"C\" { ; } }",
    "{ extern \"C\" { ; } float y; }",
    "{ extern \"QC\" { string s = \"\\9\"; } }",
    "{ extern \"QC\" { float y; } string s = \"\\9\"; }",
    "extern \"C\"",
    "extern \"C\";",
    "extern \"C\" extern \"C\" float y;",
    "static float y;",
    "static;",
    "const;",
    "const float y;",
    "float y; float z;",
];

/// A program's text for `site`: a file's own declarations, a function's body, or a body behind
/// `local`.
fn at_sites(item: &str) -> [String; 3] {
    [
        format!("{item}\n"),
        format!("void() g = {{ {item} }};\n"),
        format!("void() g = {{ local {item} }};\n"),
    ]
}

/// `n` nested blocks in each of the forms, closed and not, and unbraced.
fn nesting(n: usize, linkage: &str) -> Vec<String> {
    let open = format!("extern {linkage} {{ ").repeat(n);
    let bare = format!("extern {linkage} ").repeat(n);
    let mut items = vec![
        format!("{open}float y;{}", " }".repeat(n)),
        format!("{open}float y;"),
        format!("{open}{}", " }".repeat(n)),
        open.clone(),
        format!("{open}; float y;{}", " }".repeat(n)),
        format!("{open}float y;{}", " }".repeat(n.saturating_sub(1))),
        format!("{open}float y;{} }} }}", " }".repeat(n)),
        format!("{bare}float y;"),
        format!("{bare};"),
        bare.clone(),
        format!("{bare}; float y;"),
        format!("{bare}{{ float y; }}"),
        format!("{bare}string s = \"\\9\";"),
    ];
    items.push(format!("{open}{bare}float y;{}", " }".repeat(n)));
    items
}

/// The forms of the `extern` branch: `QCC_PR_ParseDefs`' `;` skipping and its required
/// definition, its nesting and its string, at file level and in a body.
fn extern_programs() -> Vec<String> {
    let mut sources = Vec::new();
    for linkage in CORE_LINKAGES {
        for shape in SHAPES {
            sources.extend(at_sites(&format!("extern {linkage} {shape}")));
        }
    }
    for linkage in LINKAGES {
        for shape in PROBES {
            sources.extend(at_sites(&format!("extern {linkage} {shape}")));
        }
    }
    for depth in DEPTHS {
        for linkage in CORE_LINKAGES {
            for item in nesting(depth, linkage) {
                sources.extend(at_sites(&item));
            }
        }
    }
    // FTEF-21.10: `extern` on a definition, which is no block: in a body and at file level, alone
    // and with the other qualifiers and a typedef.
    for item in [
        "extern float y;",
        "extern float y = 1;",
        "extern vector v;",
        "extern const float y;",
        "extern static float y;",
        "static extern float y;",
        "extern typedef float y;",
        "typedef extern float y;",
        "extern typedef void() y;",
        "typedef extern void() y;",
        "extern float y, z;",
        "extern float y; extern float y;",
    ] {
        sources.extend(at_sites(item));
        sources.extend(at_sites(&format!("float y;\n{item}")));
    }
    // FTEF-20.12: a qualifier and no type or name, beside an `extern` block and alone.
    for item in [
        "static;",
        "const;",
        "var;",
        "noref;",
        "static",
        "const",
        "static static;",
    ] {
        sources.extend(at_sites(item));
        sources.extend(at_sites(&format!("extern \"C\" {item}")));
        sources.extend(at_sites(&format!("extern \"C\" {{ {item} }}")));
    }
    // FTEF-21.17: an `extern` after another qualifier is no block, and a block takes a
    // qualifier only first.
    for qualifier in ["static", "const", "var", "noref", "local", "nosave"] {
        for tail in [
            "\"C\" { float y; }",
            "\"C\" float y;",
            "\"C\";",
            "\"C\"",
            "float y;",
        ] {
            sources.extend(at_sites(&format!("{qualifier} extern {tail}")));
            if qualifier != "local" {
                sources.extend(at_sites(&format!("extern {qualifier} {tail}")));
            }
        }
    }
    // FTEF-17.3: the `;` after a definition and a function body, at the end of the text and
    // before another definition.
    for tail in ["", ";", ";;", "; ;", ";\n;\n", ";;; float y;", " /* c */ ;"] {
        for item in [
            "float x",
            "float x = 1",
            "float x, y",
            "void() k = {}",
            "void() k = { }",
            "void k() {}",
            "void() k = [1, k] {}",
            "void(float a) k = { return; }",
            "enum { A, B };",
            "struct s { float a; };",
            "class c { float a; };",
            "typedef float t",
            "float() k = #5",
            "extern \"C\" { float x; }",
            "extern \"C\" float x",
            "extern \"C\" void k() {}",
        ] {
            let item = if item.contains('{') || item.contains('}') {
                item.to_owned()
            } else {
                format!("{item};")
            };
            sources.push(format!("{item}{tail}\n"));
            sources.push(format!("{item}{tail}"));
        }
    }
    sources
}

/// The `#define`d strings of the block: a macro's string is one the lexer read on its own, and
/// the string beside it is another (FTEF-21.17), a joined pair inside one macro is not.
fn macro_programs() -> Vec<String> {
    let probe = "{ string s = \"\\9\"; }";
    let mut sources = Vec::new();
    for define in [
        "#define C \"C\"\n",
        "#define C \"C\" \"x\"\n",
        "#define C \"\" \"C\"\n",
        "#define C \"QC\"\n",
        "#define C \"C\\x00x\"\n",
        "#define C extern \"C\"\n",
        "#define D \"C\"\n#define C D\n",
        "#define D \"C\"\n#define C D D\n",
        "#define D \"C\"\n#define C D \"x\"\n",
        "#define D \"C\"\n#define C \"x\" D\n",
    ] {
        for source in [
            format!("extern C {probe}"),
            format!("extern C C {probe}"),
            format!("extern C C C {probe}"),
            format!("extern \"C\" C {probe}"),
            format!("extern C \"x\" {probe}"),
            format!("extern C \"C\" {probe}"),
            format!("extern \"C\" \"x\" C {probe}"),
            "extern C float y;".to_owned(),
            "extern C C float y;".to_owned(),
            format!("C {probe}"),
            format!("C C {probe}"),
        ] {
            for item in at_sites(&source) {
                sources.push(format!("{define}{item}"));
            }
        }
    }
    for define in [
        "#define C(a) a\n",
        "#define C(a) a \"x\"\n",
        "#define C(a) \"x\" a\n",
        "#define C(a, b) a b\n",
        "#define C(a) #a\n",
        "#define C(a) #a #a\n",
        "#define D \"C\"\n#define C(a) a D\n",
    ] {
        for source in [
            format!("extern C(\"C\") {probe}"),
            format!("extern C(\"C\") C(\"C\") {probe}"),
            format!("extern C(\"C\", \"x\") {probe}"),
            format!("extern C(\"C\", \"\") {probe}"),
            format!("extern C(\"C\") \"x\" {probe}"),
            format!("extern \"C\" C(\"x\") {probe}"),
            format!("C(\"C\") {probe}"),
            "extern C(\"C\") float y;".to_owned(),
        ] {
            for item in at_sites(&source) {
                sources.push(format!("{define}{item}"));
            }
        }
    }
    sources
}

/// `qccx`'s `"a":"b"` string joins with a NUL, which ends `pr_token`.
fn qccx_programs() -> Vec<String> {
    let probe = "{ string s = \"\\9\"; }";
    let mut sources = Vec::new();
    for linkage in [
        "\"C\":\"x\"",
        "\"C\": \"x\"",
        "\"C\" : \"x\"",
        "\"C\":",
        "\"C\":\"\"",
        "\"x\":\"C\"",
        "\":\":\"C\"",
        "\"C\":\"x\":\"y\"",
        "\"C\" \"x\":\"y\"",
        "\"C\":\"x\" \"y\"",
        "\"QC\":\"x\"",
        "\"C\"",
    ] {
        for item in at_sites(&format!("extern {linkage} {probe}")) {
            sources.push(format!("#pragma flag enable qccx\n{item}"));
            sources.push(item);
        }
    }
    sources
}

/// The programs with no option.
fn plain_programs() -> Vec<String> {
    let mut sources = extern_programs();
    sources.extend(macro_programs());
    sources.extend(qccx_programs());
    sources
}

/// The names FTEQCC registers as types (`qccmain.c`, `QCC_PR_NewType`), which `QCC_TypeForName`
/// finds, in any case under `-Fcaseinsens` (FTEF-19.29).
const REGISTERED: &[&str] = &[
    "void",
    "float",
    "vector",
    "string",
    "entity",
    "__double",
    "__int",
    "__int32",
    "__uint",
    "__uint32",
    "__int64",
    "__uint64",
    "__int16",
    "__uint16",
    "__int8",
    "__uint8",
    "__bint",
    "__bfloat",
    "__variant",
    "variant",
    "__string",
    "__vector",
    "__entity",
];

/// The words every site and spelling is read with; the other registered names are read with
/// [`REDUCED_SITES`], since they take the same paths.
const CORE_WORDS: &[&str] = &[
    "void",
    "float",
    "vector",
    "string",
    "entity",
    "__int",
    "__variant",
];

/// A C type word, which is a type to `QCC_PR_ParseType` and no registered name, so a `for`
/// initialiser reads it in any case and a statement only as written (FTEF-20.14).
const C_WORDS: &[&str] = &["integer"];

/// The sites the other registered names are read at.
const REDUCED_SITES: &[&str] = &[
    "void() g = { T x; };\n",
    "void() g = { T; };\n",
    "void() g = { T *x; };\n",
    "void() g = { for (T i; ; ) { break; } };\n",
    "void() g = { local float a = sizeof(T); };\n",
    "void() g = { T:; };\n",
    "T x;\n",
    "T;\n",
    "void(T a) fn = #6;\n",
];

/// The spellings of `word` the differential reads: as written, with its first letter, every
/// letter and every other letter in upper case.
fn spellings(word: &str) -> Vec<String> {
    let first = word.find(|c: char| c != '_').unwrap_or(0);
    let capitalised: String = word
        .char_indices()
        .map(|(i, c)| {
            if i == first {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    let alternate: String = word
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 1 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    let mut all = vec![
        word.to_owned(),
        capitalised,
        word.to_ascii_uppercase(),
        alternate,
    ];
    all.dedup();
    all
}

/// The places a type name is read where a name that is no type is a value or an error the
/// profile reports as FTEQCC does: a statement's start, a `for` initialiser, a declaration's
/// type, a parameter, a field. `T` is the spelling under test.
const TYPE_SITES: &[&str] = &[
    "void() g = { T x; };\n",
    "void() g = { T x, y; };\n",
    "void() g = { T *x; };\n",
    "void() g = { T; };\n",
    "void() g = { T x[2]; };\n",
    "void() g = { local T x; };\n",
    "void() g = { static T x; };\n",
    "void() g = { const T x; };\n",
    "void() g = { T x; T y; };\n",
    "void() g = { T x; { T y; } };\n",
    "void() g = { if (1) { T x; } };\n",
    "void() g = { for (T i; ; ) { break; } };\n",
    "void() g = { local float i; for (i = 0, T j; ; ) { break; } };\n",
    "void() g = { T() fn; };\n",
    "void() g = { T(float a) fn; };\n",
    "void() g = { .T fld; };\n",
    "void() g = { local float a = sizeof(T); };\n",
    "void() g = { T:; };\n",
    "void() g = { T = 1; };\n",
    "void() g = { T(1); };\n",
    "void() g = { T::x; };\n",
    "void() g = { float T; T = 1; };\n",
    "void() g = { float T; T x; };\n",
    "void() g = { extern \"C\" { T x; } };\n",
    "float T;\nvoid() g = { T = 1; };\n",
    "T x;\n",
    "T *x;\n",
    "T;\n",
    "static T x;\n",
    "T x[2];\n",
    "T() fn;\n",
    ".T fld;\n",
    ".T *fld;\n",
    "void(T a) fn = #6;\n",
    "void(T a, T b) fn = #6;\n",
    "void(T) fn = #6;\n",
    "void(float a, T) fn = #6;\n",
    "T(T a) fn = #6;\n",
    "extern \"C\" { T x; }\n",
    "extern \"C\" T x;\n",
];

/// Places a name that is no type is not diagnosed, which FTEF-19.30 lists: a `typedef`'s target,
/// a struct's or class's member, a cast's and a `sizeof` operand's `*`. The programs read them
/// where the name is a type, so a spelling is read here only when the option finds it.
const DECLARED_SITES: &[&str] = &[
    "void() g = { typedef T tt; tt x; };\n",
    "void() g = { local float a = sizeof (T *); };\n",
    "typedef T tt;\ntt x;\n",
    "struct s { T a; };\n",
    "struct s { T a, b; T *c; };\n",
    "union u { T a; };\n",
];

/// Sites for the five vanilla type words, which a class may hold.
const VANILLA_SITES: &[&str] = &["class c { T a; };\n", "class c { T a; void() m = {}; };\n"];

/// Sites that need a type a float converts to.
const FLOAT_SITES: &[&str] = &[
    "void() g = { local float a = 1; a = (T) a; };\n",
    "void() g = { local T x = 1; };\n",
    "void() g = { for (T i = 0; i < 1; i++) {} };\n",
    "void() g = { local float i; for (i = 0, T j = 1; ; ) { break; } };\n",
    "void() g = { T x = 1; x = 2; };\n",
    "T x = 1;\nvoid() g = { x = 2; };\n",
];

/// The bodies FTEQCC reads as definitions under `-Fcaseinsens` where this profile read
/// expressions (P2-131).
const ISSUE_BODIES: &[&str] = &[
    "Float x;",
    "VECTOR v;",
    "Entity e;",
    "String s;",
    "FLOAT x; x = 1;",
    "__Int x;",
    "Float;",
    "for (Float i = 0; i < 1; i++) {}",
];

/// The user types: a typedef, a struct, an enum and a class, named in another case.
const USER_TYPES: &[&str] = &[
    "typedef float Foo;\n",
    "typedef float foo;\n",
    "typedef vector Foo;\n",
    "struct Foo { float a; };\n",
    "class Foo { float a; };\n",
    "enum Foo { A };\n",
];

/// Where a user type's name is read, `T` the spelling used.
const USER_SITES: &[&str] = &[
    "void() g = { T x; };\n",
    "void() g = { T *x; };\n",
    "void() g = { for (T i; ; ) { break; } };\n",
    "void() g = { local float a = sizeof(T); };\n",
    "void() g = { T() fn; };\n",
    "void() g = { local T x; };\n",
    "void() g = { static T x; };\n",
    "void() g = { T = 1; };\n",
    "void() g = { T; };\n",
    "T x;\n",
    "T *x;\n",
    "void(T a) fn = #6;\n",
];

/// A class's member read through its name, with the `::` touching it or not: a type's name
/// starts a definition unless `::` follows it right away (FTEF-20.14).
const CLASS_SITES: &[&str] = &[
    "void() g = { T::m(); };\n",
    "void() g = { T ::m(); };\n",
    "void() g = { T:: m(); };\n",
    "void() g = { T :: m(); };\n",
    "void() g = { local float x; T::m(); x = 1; };\n",
    "void() T::n = {};\n",
    "class Bar : T { };\nvoid() g = { Bar::m(); };\n",
];

/// Where a user type is a struct's member.
const USER_MEMBER_SITES: &[&str] = &["struct s { T a; };\n"];

/// Programs that define a macro and use its name in another case: the macro table is searched in
/// any case under `-Fcaseinsens` (FTEF-14.73). `#undef` of a name with an upper case letter
/// crashes the pinned FTEQCC (`Hash_Remove` reads an empty bucket), so only lower case names are
/// undefined and a macro is redefined only when it was defined in lower case.
fn macro_table_programs() -> Vec<String> {
    let mut sources = Vec::new();
    for define in [
        "#define Foo 5\n",
        "#define foo 5\n",
        "#define FOO 5\n",
        "#define fOo 5\n",
        "#define Foo(a) a\n",
        "#define foo(a) a\n",
        "#define Foo 5\n#define Bar Foo\n",
        "#define foo 5\n#define bar foo\n",
        "#define foo 5\n#define FOO 6\n",
        "#define foo 5\n#undef foo\n",
        "#define foo 5\n#undef foo\n#define Foo 6\n",
        "#define Foo 5\n#undef foo\n",
        "#define Foo 5\n#undef bar\n",
    ] {
        for use_ in [
            "float x = foo;",
            "float x = Foo;",
            "float x = FOO;",
            "float x = BAR;",
            "float x = bar;",
            "float x = Bar;",
            "float x = foo(5);",
            "float x = Foo(5);",
            "float x = FOO(5);",
            "#ifdef foo\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#ifdef Foo\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#ifdef FOO\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#ifndef foo\nfloat x = zzz;\n#endif",
            "#ifndef FOO\nfloat x = zzz;\n#endif",
            "#if defined(foo)\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#if defined(FOO)\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#if !defined(Foo)\nfloat x = zzz;\n#endif",
            "#if foo == 5\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#if FOO == 5\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#ifdef zzz\n#elif defined(FOO)\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#ifdef zzz\n#elifdef foo\nfloat x = 5;\n#else\nfloat x = zzz;\n#endif",
            "#undef foo\nfloat x = foo;",
            "#undef foo\nfloat x = Foo;",
            "#undef bar\nfloat x = BAR;",
        ] {
            // A call of an object-like macro is no definition's value.
            let call = use_.contains("(5)");
            if call != define.contains("(a)") {
                continue;
            }
            sources.push(format!("{define}{use_}\n"));
        }
    }
    sources
}

/// The command-line options the type programs are compiled with.
const CASE_OPTIONS: &[&[&str]] = &[&["-Fcaseinsens"], &[]];

/// Every case of the family, `f` bound before each program.
pub(super) fn cases() -> Vec<Case> {
    let program = |source: &str| format!("{BIND}{source}");
    let mut cases: Vec<Case> = plain_programs()
        .iter()
        .map(|source| Case::new(Dialect::Fteqcc, program(source)))
        .collect();
    let with = |options: &[&str], source: String| Case {
        options: options.iter().map(|&o| o.to_owned()).collect(),
        ..Case::new(Dialect::Fteqcc, program(&source))
    };
    for options in CASE_OPTIONS {
        for source in macro_table_programs() {
            cases.push(with(options, source));
        }
        let insensitive = !options.is_empty();
        for body in ISSUE_BODIES {
            cases.push(with(options, format!("void() g = {{ {body} }};\n")));
        }
        cases.push(with(
            options,
            "float Float;\nvoid() g = { Float = 1; };\n".to_owned(),
        ));
        for word in REGISTERED.iter().chain(C_WORDS) {
            let core = CORE_WORDS.contains(word);
            let sites = if core { TYPE_SITES } else { REDUCED_SITES };
            let declared_sites = if core {
                DECLARED_SITES
            } else {
                DECLARED_SITES.get(..2).unwrap_or_default()
            };
            let all = spellings(word);
            let kept = if core { all.len() } else { 3 };
            for spelling in all.into_iter().take(kept) {
                let read = insensitive || spelling == *word;
                for site in sites {
                    cases.push(with(options, site.replace('T', &spelling)));
                }
                let declared = declared_sites.iter().filter(|_| read);
                let vanilla = VANILLA_SITES
                    .iter()
                    .filter(|_| read && matches!(*word, "float" | "vector" | "string" | "entity"));
                let float = FLOAT_SITES.iter().filter(|_| read && *word == "float");
                for site in declared.chain(vanilla).chain(float) {
                    cases.push(with(options, site.replace('T', &spelling)));
                }
            }
        }
        for spelling in ["Foo", "foo", "FOO"] {
            for site in CLASS_SITES {
                let class = "class Foo { float a; void() m = {}; };\n";
                cases.push(with(
                    options,
                    format!("{class}{}", site.replace('T', spelling)),
                ));
            }
        }
        for define in USER_TYPES {
            for spelling in ["Foo", "foo", "FOO", "fOO"] {
                for site in USER_SITES {
                    let source = format!("{define}{}", site.replace('T', spelling));
                    cases.push(with(options, source));
                }
                for site in USER_MEMBER_SITES.iter().filter(|_| insensitive) {
                    let source = format!("{define}{}", site.replace('T', spelling));
                    cases.push(with(options, source));
                }
            }
        }
    }
    cases
}
