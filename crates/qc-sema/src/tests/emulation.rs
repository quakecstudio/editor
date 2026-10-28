// SPDX-License-Identifier: GPL-3.0-or-later

//! Opcodes the target lacks, as FTEQCC emulates or refuses them (`docs/spec/dialect-fteqcc.md`
//! FTEF-27). Each case's diagnostics are the pinned FTEQCC's on the standard target
//! (2026-10-27); a warning its settings make an error is `E0125` (F313).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn body(statements: &str) -> String {
    format!(
        "void() g = {{ local float a; local int i; local string s; local entity e; {statements} \
         }};\n"
    )
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-27.2: an integer opcode with no helper is "NAME function not defined", then refused
// spec FTEF-27.5: refused, with the FTE target suggested when it has the opcode
// spec QCD-4.26: E0320 "Opcode ... not valid for target"
// spec QCD-7.77: W0235 for an emulation's notes
#[test]
fn integer_opcodes_need_a_helper() {
    assert_eq!(
        diags(&body("i = i + i;")),
        [
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            d(
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
    assert_eq!(
        diags(&body("i = i * i;")),
        [
            d(
                Code::W0235,
                "MulInt function not defined: cannot emulate int*int"
            ),
            d(
                Code::E0320,
                "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
    let defined = "int(int a, int b) AddInt = { return a; };\n\
                   void() g = { local int i; i = i + i; };\n";
    assert_eq!(diags(defined), []);
}

// spec FTEF-27.4: an int converted to a float with no `itof` warns that denormals are unsafe,
// naming the value, which F313 makes an error
// spec QCD-7.79: W0237 for an emulation's denormal warnings
#[test]
fn conversions_warn_of_denormals() {
    assert_eq!(
        diags(&body("a = a + i;")),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
    assert_eq!(
        diags(&body("a = i < i;")),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
        ]
    );
}

// spec FTEF-27.3: a string offset warns, then converts the float and adds integers
// spec QCD-7.78: W0236 "OP_ADD_SF: string+float may be unsafe"
#[test]
fn string_offsets() {
    assert_eq!(
        diags(&body("s = s + a;")),
        [
            d(Code::W0236, "OP_ADD_SF: string+float may be unsafe"),
            d(Code::E0125, "ftoi emulation: denormals are unsafe"),
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            d(
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-27.3: an entity offset needs the nextent builtin, else ADD_EI is refused
#[test]
fn entity_offsets() {
    assert_eq!(
        diags(&body("e = e + 1;")),
        [
            d(Code::W0235, "the nextent builtin is not defined"),
            d(Code::E0320, "Opcode \"+|ADD_EI\" is not supported."),
        ]
    );
}

// spec FTEF-27.4: `%` with no `mod` function is expanded into opcodes the target has
#[test]
fn remainders_expand() {
    assert_eq!(diags(&body("a = a % a;")), []);
}

// spec FTEF-27.4: a float shift with no `pow`, `floor` or `bitshift` is refused, and an integer
// shift by a constant is a product
#[test]
fn shifts() {
    assert_eq!(
        diags(&body("a = a << a;")),
        [
            d(
                Code::W0235,
                "bitshift function not defined: cannot emulate OP_LSHIFT_F*"
            ),
            d(Code::E0320, "Opcode \"<<|LSHIFT_F\" is not supported."),
        ]
    );
    assert_eq!(
        diags(&body("i = i << 2i;")),
        [
            d(
                Code::W0235,
                "MulInt function not defined: cannot emulate int*int"
            ),
            d(
                Code::E0320,
                "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-27.3: `<=>` of strings needs `strcmp`, else it is a fatal error
// spec QCD-4.27: E0321 "strcmp function not defined: cannot emulate string<=>string"
#[test]
fn string_comparison_needs_strcmp() {
    assert_eq!(
        diags(&body("a = s <=> s;")),
        [d(
            Code::E0321,
            "strcmp function not defined: cannot emulate string<=>string"
        )]
    );
}

// spec FTEF-27.1: an opcode the target has is emitted as it is
#[test]
fn the_fte_target_has_them() {
    let source = format!(
        "#pragma target fte\n{}",
        body("i = i + i; i = i * i; a = a + i;")
    );
    assert_eq!(diags(&source), []);
}

// spec FTEF-27.6: `&&` of an entity and a float is `OP_AND_F` on the standard target, with no
// diagnostic
#[test]
fn logic_of_other_types() {
    assert_eq!(diags(&body("a = (e && a);")), []);
}

// spec FTEF-27.8: a cast of an operand emits its conversion, which is emulated in turn: an
// unsigned value converted to a float is `OP_CONV_ITOF`, then the utof warning
// spec QCD-7.80: W0238 "utof emulation: will break if NAME is big"
#[test]
fn operand_conversions_are_emulated() {
    let source = "void() g = { local float a; local __uint32 u; a = (a + u); };\n";
    assert_eq!(
        diags(source),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe u"),
            d(Code::W0238, "utof emulation: will break if u is big"),
        ]
    );
}

// spec FTEF-26.5: an enum's item and a `length` are constants, so their conversions fold and
// nothing is emulated (the pinned FTEQCC compiles this with no diagnostic, 2026-10-27)
#[test]
fn constants_are_not_emulated() {
    let source = "enum : int { StateOff, StateOn };\n.float state; entity self;\n\
                  string names[] = { \"a\", \"b\" };\n\
                  void() g = { local float a; if (self.state == StateOff) a = 1; \
                  for (float i = 0; i < names.length; i++) a = i; };\n";
    assert_eq!(diags(source), []);
}

fn typed(statements: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "void() g = {{ local float a; local vector v; local float *p; local float *q; \
         local int *r; local void *w; local int i; local __uint32 u; {statements} }};\n"
    ))
}

fn not_valid(op: &str) -> (Code, String) {
    d(
        Code::E0320,
        &format!("Opcode \"{op}\" not valid for target. Consider the use of: #pragma target fte"),
    )
}

fn no_helper(name: &str, what: &str) -> (Code, String) {
    d(
        Code::W0235,
        &format!("{name} function not defined: cannot emulate {what}"),
    )
}

// spec FTEF-27.7: unsigned `<` and `>=` are float comparisons of converted operands, the `>=`
// swapped, when the target has no integer comparison
#[test]
fn unsigned_comparisons() {
    let converted = [
        d(Code::E0125, "itof emulation: denormals are unsafe u"),
        d(Code::W0238, "utof emulation: will break if u is big"),
        d(Code::E0125, "itof emulation: denormals are unsafe u"),
        d(Code::W0238, "utof emulation: will break if u is big"),
    ];
    assert_eq!(typed("if (u < u) a = 1;"), converted);
    assert_eq!(typed("if (u >= u) a = 1;"), converted);
}

// spec FTEF-27.7: unsigned division warns that it assumes the high bit clear, then divides as
// signed
#[test]
fn unsigned_division() {
    assert_eq!(
        typed("u = u / u;"),
        [
            d(
                Code::W0235,
                "OP_DIV_U emulation: assuming the high bit is not set..."
            ),
            no_helper("DivInt", "int/int"),
            not_valid("/|DIV_I"),
        ]
    );
}

// spec FTEF-27.7: `^` of vectors is `(a & ~b) | (b & ~a)`, whose opcodes are replaced or emitted
// spec FTEF-27.9: `&` and `|` of vectors and a float bit clear are emitted with no check
#[test]
fn replaced_opcodes_are_not_checked() {
    for quiet in [
        "v = v ^ v;",
        "v = v | v;",
        "v |= v;",
        "a = a &~ 1;",
        "a &~= 1;",
        "a = !i;",
        "if (p > q) a = 1;",
    ] {
        assert_eq!(typed(quiet), [], "{quiet}");
    }
}

// spec FTEF-27.10: a pointer offset is scaled by the pointee's size in words and added with
// `OP_ADD_PIW`, whose emulation multiplies by 4 and adds
#[test]
fn pointer_offsets() {
    assert_eq!(
        typed("p = p + 1;"),
        [no_helper("AddInt", "int+int"), not_valid("+|ADD_I")]
    );
    assert_eq!(
        typed("p = p + i;"),
        [
            no_helper("MulInt", "int*int"),
            not_valid("*|MUL_I"),
            no_helper("AddInt", "int+int"),
            not_valid("+|ADD_I"),
        ]
    );
}

// spec FTEF-27.10: a subtraction's size is the unsigned size negated, which is truncated
// spec QCD-7.76: W0234 for a constant truncated
#[test]
fn pointer_subtraction_truncates_its_size() {
    assert_eq!(
        typed("p = p - 1;"),
        [
            d(
                Code::W0234,
                "Constant int operand 4294967295 will be truncated to -1"
            ),
            no_helper("AddInt", "int+int"),
            not_valid("+|ADD_I"),
        ]
    );
}

// spec FTEF-27.10: two pointers of different types are "incompatible pointer types", a fatal
// error; of one type, their difference is divided by the size
// spec QCD-4.33: E0327 for incompatible pointer types
#[test]
fn pointer_differences() {
    assert_eq!(
        typed("i = p - r;"),
        [d(Code::E0327, "incompatible pointer types")]
    );
    assert_eq!(
        typed("i = p - q;"),
        [
            no_helper("SubInt", "int-int"),
            not_valid("-|SUB_I"),
            no_helper("DivInt", "int/int"),
            not_valid("/|DIV_I"),
        ]
    );
}

// spec FTEF-27.10: under `undefwordsize`, pointer arithmetic on `void` is an error
// spec QCD-4.32: E0326 for pointer arithmetic that assumes a size
#[test]
fn undefined_word_size() {
    let undefined = "#pragma flag enable undefwordsize\n";
    let source = format!("{undefined}void() g = {{ local void *w; local int i; w = w + i; }};\n");
    assert_eq!(
        diags(&source),
        [
            d(Code::E0326, "void* maths was disabled."),
            no_helper("AddInt", "int+int"),
            not_valid("+|ADD_I"),
        ]
    );
}

const ADD_I: &str =
    "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte";
const ADD_INT: &str = "AddInt function not defined: cannot emulate int+int";
const MEMGETVAL: &str = "memgetval function not defined: cannot emulate OP_LOADP_*";

fn loadp(name: &str) -> (Code, String) {
    d(
        Code::E0320,
        &format!(
            "Opcode \"=|{name}\" not valid for target. Consider the use of: #pragma target fte"
        ),
    )
}

// spec FTEF-27.10: a struct pointed at is sized by its layout, a subtraction's size truncated
#[test]
fn struct_pointer_arithmetic_scales_by_the_layout() {
    let add = [d(Code::W0235, ADD_INT), d(Code::E0320, ADD_I)];
    assert_eq!(
        diags("struct S { float a; }; S *p; void() g = { p = p + 1; };\n"),
        add
    );
    assert_eq!(
        diags("struct S { float a; }; void() g = { local S *p = 0; p++; };\n"),
        add
    );
    assert_eq!(
        diags("struct S { float a; vector v; }; S *p; void() g = { p = p - 1; };\n"),
        [
            d(
                Code::W0234,
                "Constant int operand 4294967292 will be truncated to -4"
            ),
            d(Code::W0235, ADD_INT),
            d(Code::E0320, ADD_I),
        ]
    );
}

// spec FTEF-6.14: a member through a pointer is loaded by its type at its offset, the pointer
// read first, and a struct through a pointer word by word, each word a statement the target
// must have
#[test]
fn struct_reads_through_a_pointer() {
    let member = [d(Code::W0235, MEMGETVAL), loadp("LOADP_F")];
    for read in ["p->a", "p.a", "(*p).a", "p[0].a"] {
        let source = format!(
            "struct S {{ float a; }}; void() g = {{ local S *p = 0; local float f = {read}; }};\n"
        );
        assert_eq!(diags(&source), member, "{read}");
    }
    assert_eq!(
        diags(
            "struct S { float a; }; void() g = { local S **pp = 0; local float f = (*pp)->a; };\n"
        ),
        [
            d(Code::W0235, MEMGETVAL),
            loadp("LOADP_I"),
            d(Code::W0235, MEMGETVAL),
            loadp("LOADP_F"),
        ]
    );
    for read in ["*p", "p[0]"] {
        let source = format!(
            "struct S {{ float a; }}; void() g = {{ local S *p = 0; local S y = {read}; }};\n"
        );
        assert_eq!(diags(&source), [loadp("LOADP_I")], "{read}");
    }
    assert_eq!(
        diags("struct S { float a; vector v; }; void() g = { local S *p = 0; local S y = *p; };\n"),
        [loadp("LOADP_V")]
    );
    assert_eq!(
        diags("struct S { float a; }; void() g = { local S *p = 0; p->a = 1; };\n"),
        []
    );
}

const BITAND_I: &str =
    "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte";
const BITOR_I: &str =
    "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte";
const DIV_I: &str =
    "Opcode \"/|DIV_I\" not valid for target. Consider the use of: #pragma target fte";
const SUB_I: &str =
    "Opcode \"-|SUB_I\" not valid for target. Consider the use of: #pragma target fte";
const MUL_I: &str =
    "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte";

fn helper(name: &str, what: &str) -> (Code, String) {
    d(
        Code::W0235,
        &format!("{name} function not defined: cannot emulate {what}"),
    )
}

// spec FTEF-6.15: a bitfield store reads its word and packs the value in by `&`, `<<` and `|`,
// the shift by the member's bit offset
#[test]
fn a_bitfield_store_packs_its_word() {
    let pack = [
        helper("BitandInt", "int&int"),
        d(Code::E0320, BITAND_I),
        helper("BitorInt", "int|int"),
        d(Code::E0320, BITOR_I),
    ];
    assert_eq!(
        diags("struct S { int a : 3; } x; void() g = { x.a = 1; };\n"),
        pack
    );
    assert_eq!(
        diags("struct S { int a : 3; }; void() g = { S s; s.a = 1; };\n"),
        pack
    );
    assert_eq!(
        diags("struct S { int a : 3; int b : 3; } x; int i; void() g = { x.b = i; };\n"),
        [
            helper("BitandInt", "int&int"),
            d(Code::E0320, BITAND_I),
            helper("BitandInt", "int&int"),
            d(Code::E0320, BITAND_I),
            helper("MulInt", "int*int"),
            d(Code::E0320, MUL_I),
            helper("BitorInt", "int|int"),
            d(Code::E0320, BITOR_I),
        ]
    );
    assert_eq!(
        diags("struct S { int a : 32; } x; void() g = { x.a = 1; local int i = x.a; };\n"),
        []
    );
    assert_eq!(
        diags("#pragma target fte\nstruct S { int a : 3; } x; void() g = { x.a = 1; };\n"),
        []
    );
}

// spec FTEF-6.15: a bitfield read is extended from its word, and through a pointer loaded by its
// width, any width but 8 and 16 refused
// spec QCD-4.57: E0353 "ptr-to-bitfld has unsupported bit depth"
#[test]
fn a_bitfield_read_is_extended() {
    let extended = [
        helper("MulInt", "int*int"),
        d(Code::E0320, MUL_I),
        d(Code::E0125, "itof emulation: denormals are unsafe __int32"),
        helper("SubInt", "~int"),
        d(Code::E0320, SUB_I),
        helper("DivInt", "int/int"),
        d(Code::E0320, DIV_I),
        helper("SubInt", "~int"),
        d(Code::E0320, SUB_I),
        d(Code::E0320, DIV_I),
    ];
    assert_eq!(
        diags("struct S { int a : 3; } x; void() g = { local float f = x.a; };\n"),
        extended
    );
    let mut loaded = vec![d(Code::W0235, MEMGETVAL), loadp("LOADP_I")];
    loaded.extend(extended.iter().cloned());
    assert_eq!(
        diags("struct S { int a : 8; }; void() g = { local S *p = 0; local int i = p->a; };\n"),
        loaded
    );
    assert_eq!(
        diags("__int8 *q; void() g = { local int i = *q; };\n"),
        loaded
    );
    assert_eq!(
        diags("struct S { int a : 3; }; void() g = { local S *p = 0; local int i = p->a; };\n"),
        [d(Code::E0353, "ptr-to-bitfld has unsupported bit depth")]
    );
}

// spec FTEF-27.4: a constant shift is by `1 << n`, so a shift by 0 folds, and `>>` ends with the
// division as a plain statement
#[test]
fn constant_shifts_fold_by_their_power() {
    assert_eq!(diags(&body("i = i << 0;")), []);
    assert_eq!(
        diags(&body("i = i << 2;")),
        [helper("MulInt", "int*int"), d(Code::E0320, MUL_I)]
    );
    assert_eq!(
        diags(&body("i = i >> 0;")),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
            helper("SubInt", "~int"),
            d(Code::E0320, SUB_I),
            helper("SubInt", "~int"),
            d(Code::E0320, SUB_I),
            d(Code::E0320, DIV_I),
        ]
    );
    let uint16 = |index: &str| {
        diags(&format!(
            "__uint16 *q; void() g = {{ local int i = q[{index}]; }};\n"
        ))
    };
    let tail = [
        d(
            Code::W0235,
            "OP_DIV_U emulation: assuming the high bit is not set...",
        ),
        helper("DivInt", "int/int"),
        d(Code::E0320, DIV_I),
    ];
    let mut one = vec![d(Code::W0235, MEMGETVAL), loadp("LOADP_I")];
    one.extend(tail.iter().cloned());
    assert_eq!(uint16("1"), one);
    let mut zero = vec![
        d(Code::W0235, MEMGETVAL),
        loadp("LOADP_I"),
        helper("MulInt", "int*int"),
        d(Code::E0320, MUL_I),
    ];
    zero.extend(tail.iter().cloned());
    assert_eq!(uint16("0"), zero);
}

// spec FTEF-27.2: a power's helper is named `pow` in the warning
#[test]
fn a_power_warns_of_pow() {
    let found = diags(&body("i = i >> i;"));
    assert_eq!(found.first(), Some(&helper("pow", "int*^int")));
}

// spec FTEF-34.9: a global's `?:` stores its first arm by its size and its last by its type,
// each a statement at global scope, an `OP_STORE_I64` two `OP_STORE_FLD`s, and a double's last
// arm word by word with no statement reported
#[test]
fn a_global_ternary_stores_each_arm() {
    let scope = |ops: &[&str]| -> Vec<(Code, String)> {
        let mut found: Vec<(Code, String)> = ops
            .iter()
            .map(|op| {
                d(
                    Code::E0345,
                    &format!("Unable to generate statements at global scope ({op})."),
                )
            })
            .collect();
        found.push(d(Code::E0214, "initializer for 'x' is not constant"));
        found
    };
    for (ty, first, last) in [
        ("vector", "STORE_V", "STORE_V"),
        ("string", "STORE_F", "STORE_S"),
        ("entity", "STORE_F", "STORE_ENT"),
        (".float", "STORE_F", "STORE_FLD"),
        ("void()", "STORE_F", "STORE_FNC"),
        ("float*", "STORE_F", "STORE_F"),
    ] {
        let source = format!("float c; var {ty} a, b; {ty} x = c ? a : b;\n");
        assert_eq!(
            diags(&source),
            scope(&["IFNOT", first, "GOTO", last]),
            "{ty}"
        );
    }
    assert_eq!(
        diags("float c; __int64 a, b; __int64 x = c ? a : b;\n"),
        scope(&[
            "IFNOT",
            "STORE_FLD",
            "STORE_FLD",
            "GOTO",
            "STORE_FLD",
            "STORE_FLD"
        ])
    );
    assert_eq!(
        diags("float c; __double a, b; __double x = c ? a : b;\n"),
        scope(&["IFNOT", "STORE_FLD", "STORE_FLD", "GOTO"])
    );
    // An arm the statement before made is stored by that statement.
    assert_eq!(
        diags("float c, a, b; float x = c ? a + 1 : b;\n"),
        scope(&["IFNOT", "ADD_F", "GOTO", "STORE_F"])
    );
}

// spec FTEF-29.2: each compound assignment is its operator's opcode, emulated as the operator's
// spec FTEF-27.4: `^` of integers is `(a & ~b) | (b & ~a)`
#[test]
fn compound_assignments_emulate_their_operator() {
    for (op, first) in [
        ("-=", helper("SubInt", "int-int")),
        ("/=", helper("DivInt", "int/int")),
        ("|=", helper("BitorInt", "int|int")),
        ("&=", helper("BitandInt", "int&int")),
        ("&~=", helper("BitandInt", "int&int")),
        ("^=", helper("SubInt", "~int")),
        ("<<=", helper("pow", "int*^int")),
        (">>=", helper("pow", "int*^int")),
    ] {
        let found = diags(&format!("int i, j; void() g = {{ i {op} j; }};\n"));
        assert_eq!(found.first(), Some(&first), "{op}");
    }
    let not = [helper("SubInt", "~int"), d(Code::E0320, SUB_I)];
    let and = [helper("BitandInt", "int&int"), d(Code::E0320, BITAND_I)];
    let or = [helper("BitorInt", "int|int"), d(Code::E0320, BITOR_I)];
    let xor: Vec<(Code, String)> = [&not[..], &and[..], &not[..], &and[..], &or[..]].concat();
    assert_eq!(diags("int i, j; void() g = { i ^= j; };\n"), xor);
    assert_eq!(
        diags("int i, j; void() g = { i >>= j; };\n").len(),
        diags("int i, j; void() g = { i = i >> j; };\n").len()
    );
}

fn postfix(target: &str, statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "{target}\nfloat f, x; int i; string s; vector w; entity en; __double d; __uint u; \
         __int64 l; float *p; void *v; __int16 *c; struct st {{ int a : 3; int b : 5; }}; st b; \
         st *pb; __int8 c8;\nvoid() g = {{ {statement} }};\n"
    ))
}

// spec FTEF-29.5: a postfix adds 1 or -1 by its type's opcode, `--` too: `OP_ADD_I` of an int
// for an integer, a string and a pointer to a narrow type, `OP_ADD_U` for a uint, `OP_ADD_I64`,
// `OP_ADD_D`, and a pointer to words by `OP_ADD_PIW`
#[test]
fn a_postfix_adds_by_its_types_opcode() {
    let add = [helper("AddInt", "int+int"), d(Code::E0320, ADD_I)];
    for statement in ["i++;", "i--;", "u++;", "p--;", "v++;", "c--;", "x = i--;"] {
        let found = postfix("", statement);
        assert_eq!(found.get(..2), Some(&add[..]), "{statement}");
    }
    assert_eq!(postfix("", "f++; f--;"), []);
    let refused = |op: &str| {
        d(
            Code::E0320,
            &format!(
                "Opcode \"+|{op}\" not valid for target. Consider the use of: #pragma target fte"
            ),
        )
    };
    assert_eq!(postfix("", "l++;"), [refused("ADD_I64")]);
    assert_eq!(postfix("", "x = d--;").first(), Some(&refused("ADD_D")));
    assert_eq!(postfix("#pragma target fte", "p--; u++; l--; d++;"), []);
}

// spec FTEF-29.5: a string stepped by an integer warns of the store, a vector or an entity is
// "post increment operator not supported with this type"
// spec QCD-4.28: E0322 for a postfix of a type it does not step
#[test]
fn a_postfix_of_another_type_is_refused() {
    let fte = |statement: &str| postfix("#pragma target fte", statement);
    assert_eq!(
        fte("s++;"),
        [d(Code::W0240, "type mismatch: __int32 __int32 to string s")]
    );
    let unsupported = d(
        Code::E0322,
        "post increment operator not supported with this type",
    );
    for statement in ["w++;", "en--;", "x = w++;", "x = en++;"] {
        assert_eq!(
            fte(statement),
            std::slice::from_ref(&unsupported),
            "{statement}"
        );
    }
}

// spec FTEF-29.5: a bitfield not at its word's start is "post increment operator on bitfield";
// one at it, and an 8- or 16-bit integer, steps only when its value is discarded
// spec FTEF-2.12: an 8- or 16-bit integer is `ev_bitfld`
#[test]
fn a_postfix_of_a_bitfield() {
    let fte = |statement: &str| postfix("#pragma target fte", statement);
    let on_bitfield = d(Code::E0322, "post increment operator on bitfield");
    assert_eq!(fte("b.b++;"), std::slice::from_ref(&on_bitfield));
    assert_eq!(fte("x = b.b++;"), [on_bitfield]);
    let unsupported = d(
        Code::E0322,
        "post increment operator not supported with this type",
    );
    assert_eq!(fte("b.a++;"), []);
    assert_eq!(fte("c8--;"), []);
    assert_eq!(fte("for (c8++; x; b.a--) {}"), []);
    assert_eq!(fte("x = b.a--;"), std::slice::from_ref(&unsupported));
    assert_eq!(fte("x = c8++;"), [unsupported]);
    assert_eq!(
        fte("pb->a++;"),
        [d(Code::E0353, "ptr-to-bitfld has unsupported bit depth")]
    );
}

// spec FTEF-29.5: under `undefwordsize` a narrow pointer's step whose value is used is "not
// allowed to make assumptions about pointer sizes. sorry."; a discarded one steps
// spec QCD-4.32: E0326 for pointer arithmetic under `undefwordsize`
#[test]
fn a_narrow_pointers_step_under_undefwordsize() {
    let flag = "#pragma flag enable undefwordsize";
    let sorry = d(
        Code::E0322,
        "not allowed to make assumptions about pointer sizes. sorry.",
    );
    assert_eq!(postfix(flag, "x = c--;"), std::slice::from_ref(&sorry));
    assert_eq!(postfix(flag, "x = p[c++];"), [sorry]);
    let add = [helper("AddInt", "int+int"), d(Code::E0320, ADD_I)];
    for statement in ["c--;", "(c--);", "x = (c++, 1);", "x = 1, c++;"] {
        assert_eq!(postfix(flag, statement), add, "{statement}");
    }
    let blocked = d(
        Code::E0326,
        "Making assumptions about pointer sizes was blocked.",
    );
    assert_eq!(postfix(flag, "v++;").first(), Some(&blocked));
}

fn unable(op: &str) -> (Code, String) {
    d(
        Code::E0345,
        &format!("Unable to generate statements at global scope ({op})."),
    )
}

// spec FTEF-27.11: 64-bit bitwise opcodes and field loads are plain statements, refused fatally
// and never reported at global scope; a vector's `&` and `|` are forced
// spec FTEF-34.11: a refused opcode is still emitted, at global scope reported as such
#[test]
fn plain_statements_and_refused_opcodes() {
    let refused = |name: &str, op: &str| {
        d(
            Code::E0320,
            &format!(
                "Opcode \"{name}|{op}\" not valid for target. Consider the use of: #pragma target fte"
            ),
        )
    };
    assert_eq!(
        diags("__double d, e; __double z; void() h = { z = d & e; };\n"),
        [
            refused("=", "CONV_DI64"),
            refused("=", "CONV_DI64"),
            refused("&", "BITAND_I")
        ]
    );
    assert_eq!(
        diags("__int64 a, b, c; void() h = { c = a | b; };\n"),
        [refused("|", "BITOR_I")]
    );
    assert_eq!(
        diags("vector v, w; vector x = v & w;\n"),
        [d(Code::E0214, "initializer for 'x' is not constant")]
    );
    assert_eq!(
        diags("entity e; .__int64 fl; __int64 l = e.fl;\n"),
        [d(Code::E0214, "initializer for 'l' is not constant")]
    );
    assert_eq!(
        diags("string s; string t = s + 1;\n"),
        [
            d(Code::W0236, "OP_ADD_SF: string+float may be unsafe"),
            helper("AddInt", "int+int"),
            d(Code::E0320, ADD_I),
            unable("ADD_I"),
            d(Code::E0214, "initializer for 't' is not constant"),
        ]
    );
}

// spec FTEF-30.9: the arms' types are compared before the last arm is stored
#[test]
fn a_ternary_compares_before_its_last_store() {
    assert_eq!(
        diags("#pragma target fte\nfloat *p, *q; int i; float *r = i ? p : q;\n"),
        [
            unable("IFNOT"),
            unable("STORE_F"),
            unable("GOTO"),
            d(
                Code::W0244,
                "Type mismatch on ternary operator: float* vs float**"
            ),
            unable("STORE_P"),
            d(Code::E0214, "initializer for 'r' is not constant"),
        ]
    );
}
