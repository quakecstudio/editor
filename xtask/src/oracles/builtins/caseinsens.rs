// SPDX-License-Identifier: GPL-3.0-or-later

//! The case-insensitive family of the builtin differential (P2-132): programs whose outcome
//! `-Fcaseinsens` decides (`docs/spec/dialect-fteqcc.md` FTEF-19.31): FTEQCC's name tables, the
//! type table and the definitions', are searched in any case, so a type name or a name written in
//! another case than its definition is found. Each program binds `f` first, so `f` is bound only
//! when FTEQCC compiled the rest. It is compiled with and without the option, the words of C
//! that `QCC_PR_ParseType` reads and the names of the type table in three cases each. The
//! program's statements that start with a type word in a body are the parser's (FTEF-20.6) and
//! are not among them.

use qc_syntax::Dialect;

use super::Case;
use super::directives::BIND;

/// The words a type is written with: C's, FTEQCC's table's and ones that are no type.
const TYPE_WORDS: &[&str] = &[
    "void",
    "float",
    "string",
    "vector",
    "entity",
    "int",
    "integer",
    "__int",
    "__uint",
    "unsigned",
    "signed",
    "long",
    "short",
    "char",
    "double",
    "_Bool",
    "bool",
    "variant",
    "__variant",
    "__string",
    "__vector",
    "__entity",
    "__int32",
    "__uint32",
    "__int64",
    "__uint64",
    "__double",
    "__bfloat",
    "__bint",
    "__int8",
    "__uint16",
    "invalid",
    "__field",
    "__function",
    "__pointer",
];

/// Where a type word stands in a program: `{}` is the word.
const PLACES: &[&str] = &[
    "{} x;\n",
    "{}() h = #6;\n",
    "void({} a) h = #6;\n",
    ".{} fl;\n",
    "{} *p;\n",
    "typedef {} t;\nt x;\n",
    "void() g = { local {} x; };\n",
    "{}(float a) h = #6;\n",
];

/// Programs that write a name in another case than its definition.
const NAMES: &[&str] = &[
    "float Foo;\nfloat y = foo;\n",
    "float foo;\nfloat y = FOO;\n",
    "float Foo = 1;\nfloat y = foo;\n",
    "const float Foo = 1;\nfloat y = foo;\n",
    "float Foo = 1;\nstring FOO = \"a\";\nfloat x = foo;\n",
    "float Foo = 1;\nstring FOO = \"a\";\nstring x = foo;\n",
    "float Foo;\nfloat foo;\nfloat Foo;\n",
    "float Foo;\nstring foo;\n",
    "void() Foo = #6;\nvoid() foo = #6;\n",
    "void() Foo = {};\nvoid() k = { foo(); };\n",
    "void() Foo = {};\nvoid() foo = {};\n",
    "void() foo;\nvoid() Foo = {};\n",
    "void() k = { foo(); };\nvoid() Foo = {};\n",
    "typedef float Foo;\nfoo x;\n",
    "typedef float foo;\nFOO x;\n",
    "typedef float Foo;\ntypedef string foo;\nFOO x = \"a\";\n",
    "typedef float Foo;\nvoid() g = { local foo x; };\n",
    "struct S { float a; };\ns x;\n",
    "struct S { float a; };\nstruct s x;\n",
    "struct S { float a; };\nstruct S x;\nvoid() g = { x.A = 1; };\n",
    "struct S { float a; };\nvoid() g = { struct s x; };\n",
    "union U { float a; vector b; };\nu x;\n",
    "enum E { A, B };\ne x;\n",
    "enum E { A, B };\nfloat y = a;\n",
    "enum E { A, B };\nfloat y = b;\n",
    "enum E { A, B };\nstring y = b;\n",
    "enum E { A, B };\nenum e { C };\n",
    "enum E { A, B };\nE x;\nfloat y = e::A;\n",
    "enum class E : float { A, B };\nE x = E::a;\n",
    "enum class E : float { A, B };\nE x = e::A;\n",
    "class C { float x; };\nc a;\n",
    "class C { float x; };\nvoid() g = { C a; a.X = 1; };\n",
    "class C { float x; };\nclass c2 : C { float y; };\nC2 a;\n",
    "class C { float x; void() f; };\nvoid() C::f = {};\nvoid() C::F = {};\n",
    "class C { float x; void() f; };\nvoid() c::f = {};\n",
    "class C { float x; void() f; };\nvoid() C::f = {};\nvoid() k = { local C a; a.F(); };\n",
    "class C { float x; };\nfloat C::x;\n",
    ".float Fl;\nvoid() g = { self.fl = 1; };\nentity self;\n",
    "entity self;\n.float Fl;\nvoid() g = { self.fl = 1; };\n",
    "entity self;\n.float fl;\nvoid() g = { self.FL = 1; };\n",
    "entity Self;\n.float fl;\nvoid() g = { self.fl = 1; };\n",
    "entity self;\n.float fl;\n.float FL;\nvoid() g = { self.fl = 1; };\n",
    "void(float A) g = { A = 1; };\n",
    "void(float A) g = { a = 1; };\n",
    "void(float a) g = { A = 1; };\n",
    "void(float a, float A) g = { a = 1; };\n",
    "void() g = { local float A; a = 1; };\n",
    "void() g = { local float a; A = 1; };\n",
    "void() g = { local float a; local string A; a = 1; };\n",
    "void() g = { local float a; local string A; A = \"a\"; };\n",
    "float a;\nvoid() g = { local float A; a = 1; };\n",
    "float A;\nvoid() g = { local string a; A = 1; };\n",
    "float A;\nvoid() g = { local string a; A = 1; a = \"x\"; };\n",
    "float A;\nvoid(string a) g = { A = 1; };\n",
    "vector V;\nfloat y = v_x;\n",
    "vector V;\nfloat y = V_X;\n",
    "vector v;\nfloat y = V_x;\n",
    "vector v = '1 2 3';\nfloat y = V_Y;\n",
    "void() g = { local vector V; V_x = 1; };\n",
    "void() g = { local vector V; v_x = 1; };\n",
    "void() Foo = #6;\nvoid() k = { FOO(); };\n",
    "void() foo = #6;\nvoid() k = { Foo(); };\n",
    "void() foo = #6;\nvoid() foo = #7;\n",
    "void() Foo = #6;\nvoid() foo = #6;\n",
    "float Foo;\nvoid() Foo = {};\n",
    "float Foo;\nvoid() foo = {};\n",
    ".float Foo;\n.float foo;\n",
    ".float Foo;\n.string foo;\n",
    ".float Foo;\nfloat foo;\n",
    "var float Foo = 1;\nfloat y = foo;\n",
    "static float Foo = 1;\nfloat y = foo;\n",
    "float Foo;\nfloat y = foo;\nstatic float foo = 2;\n",
    "static float Foo = 1;\nstatic float foo = 2;\n",
    "void() Foo;\nvoid() FOO = {};\n",
    "void(float a) Foo;\nvoid(float a) foo = {};\n",
    "void(float a) Foo;\nvoid(string a) foo = {};\n",
    "float Foo = 1;\nfloat foo = 1;\n",
    "float Foo = 1;\nfloat foo = 2;\n",
    "float Foo = 1;\nfloat y = foo + FOO;\n",
    "void() Foo = {};\nvoid() foo = #6;\n",
    "void() Foo = #6;\nvoid() foo = {};\n",
    "__accumulate void() Foo = {};\nvoid() foo = {};\n",
    "weak void() Foo = {};\nvoid() foo = {};\n",
    "void() Foo = {};\nwrap void() foo = {};\n",
    "enum E { A, B };\nfloat y = E::a;\n",
    "enum E { A, B };\nfloat y = e::a;\n",
    "enum E { A, B };\nfloat y = E::A;\n",
    "enum E { A, B };\nfloat y = a;\n",
    "enum class E : float { A, B };\nfloat y = E::A;\n",
    "enum class E : float { A, B };\nE y = E::a;\n",
    "enum class E : float { A, B };\nE y = a;\n",
    "class C { float x; void() f; };\nvoid() C::f = {};\nvoid() k = { local C a; C::F(a); };\n",
    "class C { float x; void() f; };\nvoid() C::f = {};\nvoid() k = { local C a; a.F(); };\n",
    "class C { float x; void() f; };\nvoid() C::f = {};\nvoid() k = { local C a; c::f(a); };\n",
    "class C { float x; };\nvoid() g = { local C a; a.X = 1; };\n",
    "vector v;\nvoid() g = { v.X = 1; };\n",
    "vector v;\nvoid() g = { v.LENGTH; };\n",
    "float a[2];\nvoid() g = { local float n = a.LENGTH; };\n",
    "float a[2];\nvoid() g = { local float n = a.length; };\n",
    "#define Foo 5\n#ifdef foo\nfloat x;\n#endif\nfloat y = x;\n",
    "$frame Aa Bb\nfloat x = $aa;\n",
    "$frame aa bb\nfloat x = $AA;\n",
    "float x = $Aa;\n$frame Aa\n",
    "#pragma flag enable caseinsens\nfloat Foo;\nfloat y = foo;\n",
    "float Foo;\nfloat y = foo;\n#pragma flag enable caseinsens\n",
    "float Foo;\n#pragma flag disable caseinsens\nfloat y = foo;\n",
    "float FLOAT;\nFloat x;\n",
    "float Float;\nfloat y = float;\n",
    "float Float;\nFloat = 1;\n",
    "float VOID;\nvoid() h = #6;\n",
    "float Void;\nVoid() h = #6;\n",
    "float String;\nstring s;\n",
    "float String;\nString s;\n",
];

/// The options the programs are compiled with: none, and the option under test.
const OPTIONS: &[&[&str]] = &[&[], &["-Fcaseinsens"]];

/// `word` in the cases the family writes it: as is, capitalised and upper case.
fn cased(word: &str) -> [String; 3] {
    let mut chars = word.chars();
    let capitalised = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default();
    [word.to_owned(), capitalised, word.to_uppercase()]
}

/// Every case of the family, `f` bound before each program.
pub(super) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for options in OPTIONS {
        let options: Vec<String> = options.iter().map(|&o| o.to_owned()).collect();
        for word in TYPE_WORDS {
            for written in cased(word) {
                for place in PLACES {
                    cases.push(Case {
                        options: options.clone(),
                        ..Case::new(
                            Dialect::Fteqcc,
                            format!("{BIND}{}", place.replace("{}", &written)),
                        )
                    });
                }
            }
        }
        for source in NAMES {
            cases.push(Case {
                options: options.clone(),
                ..Case::new(Dialect::Fteqcc, format!("{BIND}{source}"))
            });
        }
    }
    cases
}
