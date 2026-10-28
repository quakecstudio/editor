// SPDX-License-Identifier: GPL-3.0-or-later

//! References as FTEQCC indexes, reads, stores and takes the address of them
//! (`docs/spec/dialect-fteqcc.md` FTEF-31). Each case's diagnostics are the pinned FTEQCC's on
//! the standard target unless it sets another (2026-10-27); a warning its settings make an error
//! is `E0125` (F313).

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

fn body(statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "entity e; .float fl; .vector fv;\nvoid() g = {{ local float a, arr[4]; \
         local vector v, varr[4]; local string s; local int i; local float *p; local int *q; \
         local vector *vp; {statement} }};\n"
    ))
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

fn itof(name: &str) -> (Code, String) {
    d(
        Code::E0125,
        &format!("itof emulation: denormals are unsafe {name}"),
    )
}

fn not_valid(op: &str) -> (Code, String) {
    d(
        Code::E0320,
        &format!("Opcode \"{op}\" not valid for target. Consider the use of: #pragma target fte"),
    )
}

// spec FTEF-31.2: a constant index is checked against an array's length and a vector's 3
// spec QCD-7.91: W0249 for a constant index past an array's end
// spec QCD-4.40: E0333 for a vector's index out of bounds
#[test]
fn constant_indexes() {
    for quiet in ["a = arr[1];", "a = v[1];", "a = e.fv[1];", "v = varr[1];"] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(
        body("a = arr[5];"),
        [d(
            Code::W0249,
            "(constant) array index out of bounds (0 <= 5 < 4)"
        )]
    );
    assert_eq!(
        body("a = v[3];"),
        [d(Code::E0333, "(vector) array index out of bounds")]
    );
    assert_eq!(
        body("a = e.fv[4];"),
        [d(Code::E0333, "(.vector) array index out of bounds")]
    );
}

// spec FTEF-31.2: a variable index is read as a float by the standard target's `ArrayGet`, and
// stored by its `ArraySet`, so an integer index is converted for each
#[test]
fn variable_indexes() {
    for case in ["a = arr[i];", "arr[i] = 1;", "a = v[i];", "a = e.fv[i];"] {
        assert_eq!(body(case), [itof("i")], "{case}");
    }
    assert_eq!(body("a = v[a];"), []);
    for twice in ["arr[i] += 1;", "arr[i]++;"] {
        assert_eq!(body(twice), [itof("i"), itof("i")], "{twice}");
    }
    assert_eq!(
        body("arr[i] = s;"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
}

// spec FTEF-31.2: an element of more than one word scales its index, `OP_MUL_I` on the
// standard target needing `MulInt`
#[test]
fn a_vector_arrays_index_is_scaled() {
    assert_eq!(
        body("v = varr[i];")[..2],
        [
            d(
                Code::W0235,
                "MulInt function not defined: cannot emulate int*int"
            ),
            not_valid("*|MUL_I"),
        ]
    );
}

// spec FTEF-31.2: a class's array member is an array of fields, its index converted once
#[test]
fn an_array_of_fields() {
    let source = "entity self;\nclass C { int arr[4]; nonvirtual void() m; };\n\
                  void() C::m = { local int i; arr[i] = 0i; arr[1] = 0i; };\n";
    assert_eq!(diags(source), [itof("i")]);
}

// spec FTEF-31.2: indexing what is no array is FTEQCC's parse error at the `[`, naming what
// was expected there
#[test]
fn indexing_no_array() {
    assert_eq!(body("a = a[1];"), [d(Code::E0204, "expected ;, found [")]);
    assert_eq!(body("a = (a[1]);"), [d(Code::E0204, "expected ), found [")]);
}

// spec FTEF-31.3: a read through a pointer is `OP_LOADP_*`, emulated by `memgetval` or
// refused; a store through one, by offset, needs no emulation
#[test]
fn pointer_access() {
    let loadp_f = [
        d(
            Code::W0235,
            "memgetval function not defined: cannot emulate OP_LOADP_*",
        ),
        not_valid("=|LOADP_F"),
    ];
    for case in ["a = *p;", "a = p[i];", "a = p[1];"] {
        assert_eq!(body(case), loadp_f, "{case}");
    }
    assert_eq!(body("v = vp[1];"), [not_valid("=|LOADP_V")]);
    for quiet in ["p[1] = 2;", "*q = 1;", "p[i] = 2;", "vp[1] = v;"] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
}

// spec FTEF-31.1: a string's character is read by `OP_LOADP_C` for a float index
// spec FTEF-31.2: a string's character is read by `OP_LOADP_C` for a float index, and stored
// by `OP_STOREP_I8`, emulated as a word's load, mask and or
#[test]
fn string_characters() {
    assert_eq!(
        body("a = s[1];"),
        [d(
            Code::E0320,
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma \
             target fte"
        )]
    );
    assert_eq!(
        body("s[1] = 65;"),
        [
            d(
                Code::W0235,
                "memgetval function not defined: cannot emulate OP_LOADP_*"
            ),
            not_valid("=|LOADP_I"),
            d(
                Code::W0235,
                "BitandInt function not defined: cannot emulate int&int"
            ),
            not_valid("&|BITAND_I"),
            d(
                Code::W0235,
                "BitorInt function not defined: cannot emulate int|int"
            ),
            not_valid("||BITOR_I"),
        ]
    );
}

// spec FTEF-31.4: the address of a definition, an element or an array needs
// `OP_GLOBALADDRESS`, which the standard target lacks; a field's and a pointer's element's do
// not
// spec QCD-4.39: E0331 for an address-of the target does not support
#[test]
fn addresses_on_the_standard_target() {
    let unsupported = "Address-of operator is not supported in this form without extensions. \
                       Consider the use of: #pragma target fte";
    assert_eq!(body("p = &a;"), [d(Code::E0331, unsupported)]);
    assert_eq!(body("p = &arr[1];"), [d(Code::E0331, unsupported)]);
    let array = "Address-of operator is not supported in this form without extensions. \
                 Consider the use of either '#pragma target fte' or '#pragma flag enable \
                 brokenarray'";
    assert_eq!(body("p = &arr;"), [d(Code::E0331, array)]);
    assert_eq!(body("arr = arr;"), [d(Code::E0331, array)]);
    assert_eq!(body("p = &e.fl;"), []);
    assert_eq!(
        body("p = &p[1];"),
        [
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            not_valid("+|ADD_I"),
        ]
    );
}

// spec FTEF-31.4: on the FTE target the address of a local warns once per local under
// `localptr`, and the address of a temporary is an error that leaves the value
// spec QCD-7.92: W0250 for the address of a local
// spec QCD-4.41: E0335 for the address of a temporary
#[test]
fn addresses_on_the_fte_target() {
    let source = "#pragma target fte\n#pragma warning enable localptr\n\
                  void() g = { local float a, arr[4]; local float *p; p = &a; p = &arr; \
                  p = &arr[1]; p = &(a+a); };\n";
    let local = |name: &str| {
        d(
            Code::W0250,
            &format!(
                "Address-of operator on local float {name} is unsafe if recursing (and still \
                 bloated when not). Consider use of 'static' or 'auto'."
            ),
        )
    };
    assert_eq!(
        diags(source),
        [
            local("a"),
            local("arr"),
            d(Code::E0335, "Address-of temp on line t.qc:3"),
            d(Code::E0213, "Cannot cast from float to float*"),
        ]
    );
}

// spec FTEF-31.3: a pointer to `void` must be cast before it is read
// spec QCD-4.42: E0336 for a read through a pointer to void
#[test]
fn a_void_pointer_is_not_read() {
    let source = "void() g = { local void *w; local float a; a = *w; };\n";
    assert_eq!(
        diags(source),
        [d(
            Code::E0336,
            "ptr-to-variants must be cast to a different type before being read"
        )]
    );
}

// spec FTEF-31.5: under `lax` a compound result of another type is stored by `ArraySet`, which
// refuses it, naming the element `base[index]`
// spec QCD-4.43: E0337 for a type mismatch on an indexed store
#[test]
fn an_indexed_store_of_another_type() {
    let source = "#pragma flag enable lax\nvoid() g = { local float arr[4]; local vector v; \
                  local int i; arr[i] *= v; };\n";
    assert_eq!(
        diags(source),
        [
            itof("i"),
            d(
                Code::W0243,
                "Implicit type mismatch on assignment to arr[i]. Needed float, got vector."
            ),
            d(Code::W0240, "type mismatch: vector vector to float arr[i]"),
            d(
                Code::E0337,
                "Type mismatch on indexed assignment of arr: vector vector, needed float."
            ),
        ]
    );
}

// spec FTEF-31.8: a constant index is `QCC_Eval_Int` of it; a vector or a string has no value
// spec QCD-4.61: E0360 "Unable to evaluate to numeric constant" for such an index
#[test]
fn a_constant_index_is_evaluated_as_an_integer() {
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nfloat arr[1]; float x; vector v; float *p; entity e; .vector fv;\n\
             void() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let out = |i: &str| format!("(constant) array index out of bounds (0 <= {i} < 1)");
    for (index, shown) in [
        ("13.0d", "13"),
        ("(__int64)13i", "13"),
        ("(__uint)13i", "13"),
        ("5000000000ll", "705032704"),
        ("(13.5 / 0.0)", "-1"),
    ] {
        assert_eq!(
            messages(&format!("x = arr[{index}];")),
            [out(shown)],
            "{index}"
        );
    }
    let unable = "Unable to evaluate to numeric constant".to_owned();
    for statement in ["x = arr[\"s\"];", "x = v[\"s\"];", "x = e.fv['1 2 3'];"] {
        assert_eq!(
            messages(statement),
            [unable.clone(), unable.clone()],
            "{statement}"
        );
    }
    assert_eq!(
        messages("x = p[\"s\"];"),
        ["Implicit type mismatch. Needed integer, got string."]
    );
}

// spec FTEF-6.22: a struct or union with no tag has its members, its layout and its name
#[test]
fn a_struct_with_no_tag_is_a_type_of_its_own() {
    let fte = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        fte("typedef struct { float a; vector v; } T; T t; void() g = { t.a = \"s\"; };"),
        ["Cannot cast from string to float"]
    );
    assert_eq!(
        fte("typedef struct { float a; vector v; } T; T t; void() g = { t.zz = 1; };"),
        ["zz is not a member of <struct>"]
    );
    assert_eq!(
        fte("union { float a; vector v; } u; void() g = { u.q = 1; };"),
        ["q is not a member of <union>"]
    );
    assert_eq!(
        fte(
            "struct { float a; vector v; } anon; float arr[1]; void() g = { arr[sizeof(anon)] = \
             1; anon.a = \"s\"; };"
        ),
        [
            "(constant) array index out of bounds (0 <= 16 < 1)",
            "Cannot cast from string to float"
        ]
    );
    assert_eq!(
        fte(
            "typedef union { float a; vector v; } U; float arr[1]; void() g = { arr[sizeof(U)] = 1; };"
        ),
        ["(constant) array index out of bounds (0 <= 12 < 1)"]
    );
    let two = |a: &str, b: &str| {
        fte(&format!(
            "typedef {a} A; typedef {b} B; A a; B b; void() g = {{ a = b; }};"
        ))
    };
    assert_eq!(
        two("struct { float a; }", "struct { float a; }"),
        Vec::<String>::new()
    );
    assert_eq!(
        two("struct { float a; }", "struct { vector a; }"),
        ["Cannot cast from struct <struct> to struct <struct>"]
    );
    assert_eq!(
        two("union { float a; }", "struct { float a; }"),
        ["Cannot cast from struct <struct> to union <union>"]
    );
}

/// The definitions every case of [`REFERENCES`] reads.
const REFERENCE_DEFS: &str = "struct S { int a : 3; int b : 5; __int8 c; __uint16 h; float f; vector v; }; S st; S *ps; S sarr[3]; S **pps; float f; int i; __uint u; __int64 l; vector v; string s; entity e; float b; float farr[4]; vector varr[3]; int iarr[4]; __int8 carr[4]; __uint8 ucarr[4]; __int16 harr[2]; float *p; int *pi; vector *pv; __int8 *pc; __uint16 *ph; void *pw; .float fl; .vector fv; .int fi; float x_f; int x_i; vector x_v; string x_s;";

/// Reads, comparisons, stores, steps and addresses of each kind of reference, on the standard
/// target and the FTE target: the pinned FTEQCC's diagnostics for each (2026-10-27).
const REFERENCES: &[(&str, &str, &[&str])] = &[
    (
        "id",
        "pw = &st.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "st.b++;", &["post increment operator on bitfield"]),
    (
        "id",
        "pw = &st.b;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &st.c;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &st.h;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_f = st.f;", &[]),
    (
        "id",
        "x_i = st.f;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (st.f == st.f);", &[]),
    ("id", "st.f = st.f;", &[]),
    ("id", "st.f += 1;", &[]),
    ("id", "st.f++;", &[]),
    ("id", "--st.f;", &[]),
    (
        "id",
        "pw = &st.f;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = st.f;", &["Cannot cast from float to string"]),
    ("id", "x_f = st.v;", &["Cannot cast from vector to float"]),
    ("id", "x_i = st.v;", &["Cannot cast from vector to __int32"]),
    ("id", "b = (st.v == st.v);", &[]),
    ("id", "st.v = st.v;", &[]),
    ("id", "st.v += 1;", &["Implicit cast from float to vector"]),
    (
        "id",
        "st.v++;",
        &["post increment operator not supported with this type"],
    ),
    ("id", "--st.v;", &["++ operator on unsupported type"]),
    (
        "id",
        "pw = &st.v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = st.v;", &["Cannot cast from vector to string"]),
    (
        "id",
        "x_f = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "x_i = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "b = (ps->a == ps->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "ps->a = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "ps->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "ps->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "--ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("id", "pw = &ps->a;", &[]),
    (
        "id",
        "x_s = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "id",
        "pw = &ps->c;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &ps->h;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (ps->f == ps->f);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps->f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps->f += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps->f++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &ps->f;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "id",
        "x_i = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "id",
        "b = (ps->v == ps->v);",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps->v = ps->v;",
        &["Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte"],
    ),
    (
        "id",
        "ps->v += 1;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "id",
        "ps->v++;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "id",
        "--ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "id",
        "pw = &ps->v;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "id",
        "pw = &sarr[1].a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = sarr[1].v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "id",
        "x_i = sarr[1].v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("id", "b = (sarr[1].v == sarr[1].v);", &[]),
    ("id", "sarr[1].v = sarr[1].v;", &[]),
    (
        "id",
        "sarr[1].v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "id",
        "sarr[1].v++;",
        &["post increment operator not supported with this type"],
    ),
    ("id", "--sarr[1].v;", &["++ operator on unsupported type"]),
    (
        "id",
        "pw = &sarr[1].v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = sarr[1].v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "id",
        "x_f = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "x_i = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "b = ((*pps)->a == (*pps)->a);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "(*pps)->a = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "(*pps)->a += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "(*pps)->a++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "--(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "id",
        "pw = &(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    ("id", "x_f = farr[2];", &[]),
    (
        "id",
        "x_i = farr[2];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (farr[2] == farr[2]);", &[]),
    ("id", "farr[2] = farr[2];", &[]),
    ("id", "farr[2] += 1;", &[]),
    ("id", "farr[2]++;", &[]),
    ("id", "--farr[2];", &[]),
    (
        "id",
        "pw = &farr[2];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = farr[2];",
        &["Cannot cast from float to string"],
    ),
    (
        "id",
        "x_f = farr[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "id",
        "x_i = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (farr[i] == farr[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "farr[i] = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "farr[i] += 1;",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "farr[i]++;",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "--farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "pw = &farr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Cannot cast from float to string",
        ],
    ),
    ("id", "x_f = farr[f];", &[]),
    (
        "id",
        "x_i = farr[f];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (farr[f] == farr[f]);", &[]),
    ("id", "farr[f] = farr[f];", &[]),
    ("id", "farr[f] += 1;", &[]),
    ("id", "farr[f]++;", &[]),
    ("id", "--farr[f];", &[]),
    (
        "id",
        "pw = &farr[f];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = farr[f];",
        &["Cannot cast from float to string"],
    ),
    (
        "id",
        "x_f = varr[1];",
        &["Cannot cast from vector to float"],
    ),
    (
        "id",
        "x_i = varr[1];",
        &["Cannot cast from vector to __int32"],
    ),
    ("id", "b = (varr[1] == varr[1]);", &[]),
    ("id", "varr[1] = varr[1];", &[]),
    (
        "id",
        "varr[1] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "id",
        "varr[1]++;",
        &["post increment operator not supported with this type"],
    ),
    ("id", "--varr[1];", &["++ operator on unsupported type"]),
    (
        "id",
        "pw = &varr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = varr[1];",
        &["Cannot cast from vector to string"],
    ),
    (
        "id",
        "pw = &varr[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = iarr[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "id",
        "b = (iarr[i] == iarr[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "iarr[i] = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "iarr[i]++;",
        &[
            "itof emulation: denormals are unsafe i",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "--iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "pw = &iarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "id",
        "x_f = iarr[1];",
        &["itof emulation: denormals are unsafe iarr"],
    ),
    ("id", "x_i = iarr[1];", &[]),
    (
        "id",
        "b = (iarr[1] == iarr[1]);",
        &["Result of comparison is constant"],
    ),
    ("id", "iarr[1] = iarr[1];", &[]),
    (
        "id",
        "iarr[1] += 1;",
        &[
            "itof emulation: denormals are unsafe iarr",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "iarr[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--iarr[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &iarr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = iarr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "id",
        "pw = &carr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "b = (carr[i] == carr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "carr[i] = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "carr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "carr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "b = (ucarr[i] == ucarr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ucarr[i] = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ucarr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ucarr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "b = (harr[i] == harr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "harr[i] = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "harr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "harr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &harr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (p[0] == p[0]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[0] = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[0] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[0]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "pw = &p[0];", &[]),
    (
        "id",
        "x_s = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (p[i] == p[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[i] = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &p[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (p[f] == p[f]);",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "p[f] = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "p[f] += 1;",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "p[f]++;",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "--p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "pw = &p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (*p == *p);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "*p = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "*p += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--*p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "pw = &*p;", &[]),
    (
        "id",
        "x_s = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_i = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "b = (pi[i] == pi[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pi[i] = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pi[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &pi[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "id",
        "x_f = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "id",
        "x_i = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "id",
        "b = (pv[i] == pv[i]);",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pv[i] = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pv[i] += 1;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "id",
        "pv[i]++;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "id",
        "--pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "id",
        "pw = &pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "id",
        "pw = &pc[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "pw = &*pc;", &[]),
    (
        "id",
        "pw = &ph[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "pw = &*ph;", &[]),
    (
        "id",
        "x_f = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (s[0] == s[0]);",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "s[0] = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitorInt function not defined: cannot emulate int|int",
            "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "s[0] += 1;",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitorInt function not defined: cannot emulate int|int",
            "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "s[0]++;",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitorInt function not defined: cannot emulate int|int",
            "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitandInt function not defined: cannot emulate int&int",
            "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
            "BitorInt function not defined: cannot emulate int|int",
            "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &s[0];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    (
        "id",
        "x_s = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "pw = &s[i];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    ("id", "x_f = e.fl;", &[]),
    (
        "id",
        "x_i = e.fl;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (e.fl == e.fl);", &[]),
    ("id", "e.fl = e.fl;", &[]),
    ("id", "e.fl += 1;", &[]),
    ("id", "e.fl++;", &[]),
    ("id", "--e.fl;", &[]),
    ("id", "pw = &e.fl;", &[]),
    ("id", "x_s = e.fl;", &["Cannot cast from float to string"]),
    ("id", "x_f = e.fv;", &["Cannot cast from vector to float"]),
    ("id", "x_i = e.fv;", &["Cannot cast from vector to __int32"]),
    ("id", "b = (e.fv == e.fv);", &[]),
    ("id", "e.fv = e.fv;", &[]),
    ("id", "e.fv += 1;", &["Implicit cast from float to vector"]),
    (
        "id",
        "e.fv++;",
        &["post increment operator not supported with this type"],
    ),
    ("id", "--e.fv;", &["++ operator on unsupported type"]),
    ("id", "pw = &e.fv;", &[]),
    ("id", "x_s = e.fv;", &["Cannot cast from vector to string"]),
    ("id", "x_i = e.fi;", &[]),
    ("id", "b = (e.fi == e.fi);", &[]),
    ("id", "e.fi = e.fi;", &[]),
    (
        "id",
        "e.fi++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--e.fi;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "pw = &e.fi;", &[]),
    (
        "id",
        "x_s = e.fi;",
        &["Implicit cast from __int32 to string"],
    ),
    ("id", "x_f = v[1];", &[]),
    (
        "id",
        "x_i = v[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (v[1] == v[1]);", &[]),
    ("id", "v[1] = v[1];", &[]),
    ("id", "v[1] += 1;", &[]),
    ("id", "v[1]++;", &[]),
    ("id", "--v[1];", &[]),
    (
        "id",
        "pw = &v[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = v[1];", &["Cannot cast from float to string"]),
    (
        "id",
        "x_f = v[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "id",
        "x_i = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (v[i] == v[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "id",
        "pw = &v[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Cannot cast from float to string",
        ],
    ),
    ("id", "x_f = v_y;", &[]),
    (
        "id",
        "x_i = v_y;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (v_y == v_y);", &[]),
    ("id", "v_y = v_y;", &[]),
    ("id", "v_y += 1;", &[]),
    ("id", "v_y++;", &[]),
    ("id", "--v_y;", &[]),
    (
        "id",
        "pw = &v_y;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = v_y;", &["Cannot cast from float to string"]),
    (
        "id",
        "x_f = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "x_i = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "b = (farr == farr);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "farr = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "farr += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "farr++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "--farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "pw = &farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "x_s = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    ("id", "x_f = st;", &["Cannot cast from struct S to float"]),
    ("id", "x_i = st;", &["Cannot cast from struct S to __int32"]),
    ("id", "st = st;", &[]),
    ("id", "st += 1;", &["Cannot cast from float to struct S"]),
    (
        "id",
        "st++;",
        &["post increment operator not supported with this type"],
    ),
    ("id", "--st;", &["++ operator on unsupported type"]),
    (
        "id",
        "pw = &st;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = st;", &["Cannot cast from struct S to string"]),
    ("fte", "x_f = st.a;", &[]),
    ("fte", "x_i = st.a;", &[]),
    ("fte", "b = (st.a == st.a);", &[]),
    ("fte", "st.a = st.a;", &[]),
    ("fte", "st.a += 1;", &[]),
    ("fte", "st.a++;", &[]),
    ("fte", "--st.a;", &[]),
    ("fte", "pw = &st.a;", &[]),
    (
        "fte",
        "x_s = st.a;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = st.b;", &[]),
    ("fte", "x_i = st.b;", &[]),
    ("fte", "b = (st.b == st.b);", &[]),
    ("fte", "st.b = st.b;", &[]),
    ("fte", "st.b += 1;", &[]),
    ("fte", "st.b++;", &["post increment operator on bitfield"]),
    ("fte", "--st.b;", &[]),
    ("fte", "pw = &st.b;", &[]),
    (
        "fte",
        "x_s = st.b;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = st.c;", &[]),
    ("fte", "x_i = st.c;", &[]),
    ("fte", "b = (st.c == st.c);", &[]),
    ("fte", "st.c = st.c;", &[]),
    ("fte", "st.c += 1;", &[]),
    ("fte", "st.c++;", &[]),
    ("fte", "--st.c;", &[]),
    ("fte", "pw = &st.c;", &[]),
    (
        "fte",
        "x_s = st.c;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_i = st.h;", &[]),
    ("fte", "st.h = st.h;", &[]),
    ("fte", "st.h += 1;", &[]),
    ("fte", "st.h++;", &[]),
    ("fte", "--st.h;", &[]),
    ("fte", "pw = &st.h;", &[]),
    (
        "fte",
        "x_s = st.h;",
        &["Implicit cast from __uint32 to string"],
    ),
    ("fte", "x_f = st.f;", &[]),
    ("fte", "x_i = st.f;", &[]),
    ("fte", "b = (st.f == st.f);", &[]),
    ("fte", "st.f = st.f;", &[]),
    ("fte", "st.f += 1;", &[]),
    ("fte", "st.f++;", &[]),
    ("fte", "--st.f;", &[]),
    ("fte", "pw = &st.f;", &[]),
    ("fte", "x_s = st.f;", &["Cannot cast from float to string"]),
    ("fte", "x_f = st.v;", &["Cannot cast from vector to float"]),
    (
        "fte",
        "x_i = st.v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (st.v == st.v);", &[]),
    ("fte", "st.v = st.v;", &[]),
    ("fte", "st.v += 1;", &["Implicit cast from float to vector"]),
    (
        "fte",
        "st.v++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--st.v;", &["++ operator on unsupported type"]),
    ("fte", "pw = &st.v;", &[]),
    ("fte", "x_s = st.v;", &["Cannot cast from vector to string"]),
    (
        "fte",
        "x_f = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "x_i = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "b = (ps->a == ps->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "ps->a = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "ps->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "ps->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "--ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("fte", "pw = &ps->a;", &[]),
    (
        "fte",
        "x_s = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("fte", "x_f = ps->c;", &[]),
    ("fte", "x_i = ps->c;", &[]),
    ("fte", "b = (ps->c == ps->c);", &[]),
    ("fte", "ps->c = ps->c;", &[]),
    ("fte", "ps->c += 1;", &[]),
    ("fte", "ps->c++;", &[]),
    ("fte", "--ps->c;", &[]),
    ("fte", "pw = &ps->c;", &[]),
    (
        "fte",
        "x_s = ps->c;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_i = ps->h;", &[]),
    ("fte", "ps->h = ps->h;", &[]),
    ("fte", "ps->h += 1;", &[]),
    ("fte", "ps->h++;", &[]),
    ("fte", "--ps->h;", &[]),
    ("fte", "pw = &ps->h;", &[]),
    (
        "fte",
        "x_s = ps->h;",
        &["Implicit cast from __uint32 to string"],
    ),
    ("fte", "x_f = ps->f;", &[]),
    ("fte", "x_i = ps->f;", &[]),
    ("fte", "b = (ps->f == ps->f);", &[]),
    ("fte", "ps->f = ps->f;", &[]),
    ("fte", "ps->f += 1;", &[]),
    ("fte", "ps->f++;", &[]),
    ("fte", "--ps->f;", &[]),
    ("fte", "pw = &ps->f;", &[]),
    ("fte", "x_s = ps->f;", &["Cannot cast from float to string"]),
    ("fte", "x_f = ps->v;", &["Cannot cast from vector to float"]),
    (
        "fte",
        "x_i = ps->v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (ps->v == ps->v);", &[]),
    ("fte", "ps->v = ps->v;", &[]),
    (
        "fte",
        "ps->v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "ps->v++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--ps->v;", &["++ operator on unsupported type"]),
    ("fte", "pw = &ps->v;", &[]),
    (
        "fte",
        "x_s = ps->v;",
        &["Cannot cast from vector to string"],
    ),
    ("fte", "x_f = sarr[1].a;", &[]),
    ("fte", "x_i = sarr[1].a;", &[]),
    ("fte", "b = (sarr[1].a == sarr[1].a);", &[]),
    ("fte", "sarr[1].a = sarr[1].a;", &[]),
    ("fte", "sarr[1].a += 1;", &[]),
    ("fte", "sarr[1].a++;", &[]),
    ("fte", "--sarr[1].a;", &[]),
    ("fte", "pw = &sarr[1].a;", &[]),
    (
        "fte",
        "x_s = sarr[1].a;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = sarr[i].c;", &[]),
    ("fte", "x_i = sarr[i].c;", &[]),
    ("fte", "b = (sarr[i].c == sarr[i].c);", &[]),
    ("fte", "sarr[i].c = sarr[i].c;", &[]),
    ("fte", "sarr[i].c += 1;", &[]),
    ("fte", "sarr[i].c++;", &[]),
    ("fte", "--sarr[i].c;", &[]),
    ("fte", "pw = &sarr[i].c;", &[]),
    (
        "fte",
        "x_s = sarr[i].c;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = sarr[i].f;", &[]),
    ("fte", "x_i = sarr[i].f;", &[]),
    ("fte", "b = (sarr[i].f == sarr[i].f);", &[]),
    ("fte", "sarr[i].f = sarr[i].f;", &[]),
    ("fte", "sarr[i].f += 1;", &[]),
    ("fte", "sarr[i].f++;", &[]),
    ("fte", "--sarr[i].f;", &[]),
    ("fte", "pw = &sarr[i].f;", &[]),
    (
        "fte",
        "x_s = sarr[i].f;",
        &["Cannot cast from float to string"],
    ),
    (
        "fte",
        "x_f = sarr[1].v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "fte",
        "x_i = sarr[1].v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (sarr[1].v == sarr[1].v);", &[]),
    ("fte", "sarr[1].v = sarr[1].v;", &[]),
    (
        "fte",
        "sarr[1].v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "sarr[1].v++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--sarr[1].v;", &["++ operator on unsupported type"]),
    ("fte", "pw = &sarr[1].v;", &[]),
    (
        "fte",
        "x_s = sarr[1].v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "fte",
        "x_f = (*pps)->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "x_i = (*pps)->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "b = ((*pps)->a == (*pps)->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "(*pps)->a = (*pps)->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "(*pps)->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "(*pps)->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "fte",
        "--(*pps)->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("fte", "pw = &(*pps)->a;", &[]),
    (
        "fte",
        "x_s = (*pps)->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("fte", "x_f = farr[2];", &[]),
    ("fte", "x_i = farr[2];", &[]),
    ("fte", "b = (farr[2] == farr[2]);", &[]),
    ("fte", "farr[2] = farr[2];", &[]),
    ("fte", "farr[2] += 1;", &[]),
    ("fte", "farr[2]++;", &[]),
    ("fte", "--farr[2];", &[]),
    ("fte", "pw = &farr[2];", &[]),
    (
        "fte",
        "x_s = farr[2];",
        &["Cannot cast from float to string"],
    ),
    ("fte", "x_f = farr[i];", &[]),
    ("fte", "x_i = farr[i];", &[]),
    ("fte", "b = (farr[i] == farr[i]);", &[]),
    ("fte", "farr[i] = farr[i];", &[]),
    ("fte", "farr[i] += 1;", &[]),
    ("fte", "farr[i]++;", &[]),
    ("fte", "--farr[i];", &[]),
    ("fte", "pw = &farr[i];", &[]),
    (
        "fte",
        "x_s = farr[i];",
        &["Cannot cast from float to string"],
    ),
    ("fte", "x_f = farr[f];", &[]),
    ("fte", "x_i = farr[f];", &[]),
    ("fte", "b = (farr[f] == farr[f]);", &[]),
    ("fte", "farr[f] = farr[f];", &[]),
    ("fte", "farr[f] += 1;", &[]),
    ("fte", "farr[f]++;", &[]),
    ("fte", "--farr[f];", &[]),
    ("fte", "pw = &farr[f];", &[]),
    (
        "fte",
        "x_s = farr[f];",
        &["Cannot cast from float to string"],
    ),
    (
        "fte",
        "x_f = varr[1];",
        &["Cannot cast from vector to float"],
    ),
    (
        "fte",
        "x_i = varr[1];",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (varr[1] == varr[1]);", &[]),
    ("fte", "varr[1] = varr[1];", &[]),
    (
        "fte",
        "varr[1] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "varr[1]++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--varr[1];", &["++ operator on unsupported type"]),
    ("fte", "pw = &varr[1];", &[]),
    (
        "fte",
        "x_s = varr[1];",
        &["Cannot cast from vector to string"],
    ),
    (
        "fte",
        "x_f = varr[i];",
        &["Cannot cast from vector to float"],
    ),
    (
        "fte",
        "x_i = varr[i];",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (varr[i] == varr[i]);", &[]),
    ("fte", "varr[i] = varr[i];", &[]),
    (
        "fte",
        "varr[i] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "varr[i]++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--varr[i];", &["++ operator on unsupported type"]),
    ("fte", "pw = &varr[i];", &[]),
    (
        "fte",
        "x_s = varr[i];",
        &["Cannot cast from vector to string"],
    ),
    ("fte", "x_f = iarr[i];", &[]),
    ("fte", "x_i = iarr[i];", &[]),
    ("fte", "b = (iarr[i] == iarr[i]);", &[]),
    ("fte", "iarr[i] = iarr[i];", &[]),
    ("fte", "iarr[i] += 1;", &[]),
    ("fte", "iarr[i]++;", &[]),
    ("fte", "--iarr[i];", &[]),
    ("fte", "pw = &iarr[i];", &[]),
    (
        "fte",
        "x_s = iarr[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = iarr[1];", &[]),
    ("fte", "x_i = iarr[1];", &[]),
    ("fte", "b = (iarr[1] == iarr[1]);", &[]),
    ("fte", "iarr[1] = iarr[1];", &[]),
    ("fte", "iarr[1] += 1;", &[]),
    ("fte", "iarr[1]++;", &[]),
    ("fte", "--iarr[1];", &[]),
    ("fte", "pw = &iarr[1];", &[]),
    (
        "fte",
        "x_s = iarr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = carr[1];", &[]),
    ("fte", "x_i = carr[1];", &[]),
    ("fte", "b = (carr[1] == carr[1]);", &[]),
    ("fte", "carr[1] = carr[1];", &[]),
    ("fte", "carr[1] += 1;", &[]),
    ("fte", "carr[1]++;", &[]),
    ("fte", "--carr[1];", &[]),
    ("fte", "pw = &carr[1];", &[]),
    (
        "fte",
        "x_s = carr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = carr[i];", &[]),
    ("fte", "x_i = carr[i];", &[]),
    ("fte", "b = (carr[i] == carr[i]);", &[]),
    ("fte", "carr[i] = carr[i];", &[]),
    ("fte", "carr[i] += 1;", &[]),
    ("fte", "carr[i]++;", &[]),
    ("fte", "--carr[i];", &[]),
    ("fte", "pw = &carr[i];", &[]),
    (
        "fte",
        "x_s = carr[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_i = ucarr[i];", &[]),
    ("fte", "ucarr[i] = ucarr[i];", &[]),
    ("fte", "ucarr[i] += 1;", &[]),
    ("fte", "ucarr[i]++;", &[]),
    ("fte", "--ucarr[i];", &[]),
    ("fte", "pw = &ucarr[i];", &[]),
    (
        "fte",
        "x_s = ucarr[i];",
        &["Implicit cast from __uint32 to string"],
    ),
    ("fte", "x_f = harr[i];", &[]),
    ("fte", "x_i = harr[i];", &[]),
    ("fte", "b = (harr[i] == harr[i]);", &[]),
    ("fte", "harr[i] = harr[i];", &[]),
    ("fte", "pw = &harr[i];", &[]),
    (
        "fte",
        "x_s = harr[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = harr[1];", &[]),
    ("fte", "x_i = harr[1];", &[]),
    ("fte", "b = (harr[1] == harr[1]);", &[]),
    ("fte", "harr[1] = harr[1];", &[]),
    ("fte", "harr[1] += 1;", &[]),
    ("fte", "harr[1]++;", &[]),
    ("fte", "--harr[1];", &[]),
    ("fte", "pw = &harr[1];", &[]),
    (
        "fte",
        "x_s = harr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = p[0];", &[]),
    ("fte", "x_i = p[0];", &[]),
    ("fte", "b = (p[0] == p[0]);", &[]),
    ("fte", "p[0] = p[0];", &[]),
    ("fte", "p[0] += 1;", &[]),
    ("fte", "p[0]++;", &[]),
    ("fte", "--p[0];", &[]),
    ("fte", "pw = &p[0];", &[]),
    ("fte", "x_s = p[0];", &["Cannot cast from float to string"]),
    ("fte", "x_f = p[i];", &[]),
    ("fte", "x_i = p[i];", &[]),
    ("fte", "b = (p[i] == p[i]);", &[]),
    ("fte", "p[i] = p[i];", &[]),
    ("fte", "p[i] += 1;", &[]),
    ("fte", "p[i]++;", &[]),
    ("fte", "--p[i];", &[]),
    ("fte", "pw = &p[i];", &[]),
    ("fte", "x_s = p[i];", &["Cannot cast from float to string"]),
    ("fte", "x_f = p[f];", &[]),
    ("fte", "x_i = p[f];", &[]),
    ("fte", "b = (p[f] == p[f]);", &[]),
    ("fte", "p[f] = p[f];", &[]),
    ("fte", "p[f] += 1;", &[]),
    ("fte", "p[f]++;", &[]),
    ("fte", "--p[f];", &[]),
    ("fte", "pw = &p[f];", &[]),
    ("fte", "x_s = p[f];", &["Cannot cast from float to string"]),
    ("fte", "x_f = *p;", &[]),
    ("fte", "x_i = *p;", &[]),
    ("fte", "b = (*p == *p);", &[]),
    ("fte", "*p = *p;", &[]),
    ("fte", "*p += 1;", &[]),
    ("fte", "*p++;", &["Statement does not do anything"]),
    ("fte", "--*p;", &[]),
    ("fte", "pw = &*p;", &[]),
    ("fte", "x_s = *p;", &["Cannot cast from float to string"]),
    ("fte", "x_f = pi[i];", &[]),
    ("fte", "x_i = pi[i];", &[]),
    ("fte", "b = (pi[i] == pi[i]);", &[]),
    ("fte", "pi[i] = pi[i];", &[]),
    ("fte", "pi[i] += 1;", &[]),
    ("fte", "pi[i]++;", &[]),
    ("fte", "--pi[i];", &[]),
    ("fte", "pw = &pi[i];", &[]),
    (
        "fte",
        "x_s = pi[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = pv[i];", &["Cannot cast from vector to float"]),
    (
        "fte",
        "x_i = pv[i];",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (pv[i] == pv[i]);", &[]),
    ("fte", "pv[i] = pv[i];", &[]),
    (
        "fte",
        "pv[i] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "pv[i]++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--pv[i];", &["++ operator on unsupported type"]),
    ("fte", "pw = &pv[i];", &[]),
    (
        "fte",
        "x_s = pv[i];",
        &["Cannot cast from vector to string"],
    ),
    ("fte", "x_f = pc[i];", &[]),
    ("fte", "x_i = pc[i];", &[]),
    ("fte", "b = (pc[i] == pc[i]);", &[]),
    ("fte", "pc[i] = pc[i];", &[]),
    ("fte", "pc[i] += 1;", &[]),
    ("fte", "pc[i]++;", &[]),
    ("fte", "--pc[i];", &[]),
    ("fte", "pw = &pc[i];", &[]),
    (
        "fte",
        "x_s = pc[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = pc[1];", &[]),
    ("fte", "x_i = pc[1];", &[]),
    ("fte", "b = (pc[1] == pc[1]);", &[]),
    ("fte", "pc[1] = pc[1];", &[]),
    ("fte", "pc[1] += 1;", &[]),
    ("fte", "pc[1]++;", &[]),
    ("fte", "--pc[1];", &[]),
    ("fte", "pw = &pc[1];", &[]),
    (
        "fte",
        "x_s = pc[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = *pc;", &[]),
    ("fte", "x_i = *pc;", &[]),
    ("fte", "b = (*pc == *pc);", &[]),
    ("fte", "*pc = *pc;", &[]),
    ("fte", "*pc += 1;", &[]),
    ("fte", "*pc++;", &["Statement does not do anything"]),
    ("fte", "--*pc;", &[]),
    ("fte", "pw = &*pc;", &[]),
    (
        "fte",
        "x_s = *pc;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_i = ph[1];", &[]),
    ("fte", "ph[1] = ph[1];", &[]),
    ("fte", "ph[1] += 1;", &[]),
    ("fte", "ph[1]++;", &[]),
    ("fte", "--ph[1];", &[]),
    ("fte", "pw = &ph[1];", &[]),
    (
        "fte",
        "x_s = ph[1];",
        &["Implicit cast from __uint32 to string"],
    ),
    ("fte", "x_i = *ph;", &[]),
    ("fte", "*ph = *ph;", &[]),
    ("fte", "*ph += 1;", &[]),
    ("fte", "*ph++;", &["Statement does not do anything"]),
    ("fte", "--*ph;", &[]),
    ("fte", "pw = &*ph;", &[]),
    (
        "fte",
        "x_s = *ph;",
        &["Implicit cast from __uint32 to string"],
    ),
    ("fte", "x_f = s[0];", &[]),
    ("fte", "x_i = s[0];", &[]),
    ("fte", "b = (s[0] == s[0]);", &[]),
    ("fte", "s[0] = s[0];", &[]),
    ("fte", "s[0] += 1;", &[]),
    ("fte", "s[0]++;", &[]),
    ("fte", "--s[0];", &[]),
    (
        "fte",
        "pw = &s[0];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    ("fte", "x_s = s[0];", &["Cannot cast from float to string"]),
    ("fte", "x_f = s[i];", &[]),
    ("fte", "x_i = s[i];", &[]),
    ("fte", "b = (s[i] == s[i]);", &[]),
    ("fte", "s[i] = s[i];", &[]),
    ("fte", "s[i] += 1;", &[]),
    ("fte", "s[i]++;", &[]),
    ("fte", "--s[i];", &[]),
    (
        "fte",
        "pw = &s[i];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    (
        "fte",
        "x_s = s[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = e.fl;", &[]),
    ("fte", "x_i = e.fl;", &[]),
    ("fte", "b = (e.fl == e.fl);", &[]),
    ("fte", "e.fl = e.fl;", &[]),
    ("fte", "e.fl += 1;", &[]),
    ("fte", "e.fl++;", &[]),
    ("fte", "--e.fl;", &[]),
    ("fte", "pw = &e.fl;", &[]),
    ("fte", "x_s = e.fl;", &["Cannot cast from float to string"]),
    ("fte", "x_f = e.fv;", &["Cannot cast from vector to float"]),
    (
        "fte",
        "x_i = e.fv;",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (e.fv == e.fv);", &[]),
    ("fte", "e.fv = e.fv;", &[]),
    ("fte", "e.fv += 1;", &["Implicit cast from float to vector"]),
    (
        "fte",
        "e.fv++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--e.fv;", &["++ operator on unsupported type"]),
    ("fte", "pw = &e.fv;", &[]),
    ("fte", "x_s = e.fv;", &["Cannot cast from vector to string"]),
    ("fte", "x_f = e.fi;", &[]),
    ("fte", "x_i = e.fi;", &[]),
    ("fte", "b = (e.fi == e.fi);", &[]),
    ("fte", "e.fi = e.fi;", &[]),
    ("fte", "e.fi += 1;", &[]),
    ("fte", "e.fi++;", &[]),
    ("fte", "--e.fi;", &[]),
    ("fte", "pw = &e.fi;", &[]),
    (
        "fte",
        "x_s = e.fi;",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = v[1];", &[]),
    ("fte", "x_i = v[1];", &[]),
    ("fte", "b = (v[1] == v[1]);", &[]),
    ("fte", "v[1] = v[1];", &[]),
    ("fte", "v[1] += 1;", &[]),
    ("fte", "v[1]++;", &[]),
    ("fte", "--v[1];", &[]),
    ("fte", "pw = &v[1];", &[]),
    ("fte", "x_s = v[1];", &["Cannot cast from float to string"]),
    ("fte", "x_f = v[i];", &[]),
    ("fte", "x_i = v[i];", &[]),
    ("fte", "b = (v[i] == v[i]);", &[]),
    ("fte", "v[i] = v[i];", &[]),
    ("fte", "v[i] += 1;", &[]),
    ("fte", "v[i]++;", &[]),
    ("fte", "--v[i];", &[]),
    ("fte", "pw = &v[i];", &[]),
    ("fte", "x_s = v[i];", &["Cannot cast from float to string"]),
    ("fte", "x_f = v_y;", &[]),
    ("fte", "x_i = v_y;", &[]),
    ("fte", "b = (v_y == v_y);", &[]),
    ("fte", "v_y = v_y;", &[]),
    ("fte", "v_y += 1;", &[]),
    ("fte", "v_y++;", &[]),
    ("fte", "--v_y;", &[]),
    ("fte", "pw = &v_y;", &[]),
    ("fte", "x_s = v_y;", &["Cannot cast from float to string"]),
    ("fte", "x_f = farr;", &["Cannot cast from float* to float"]),
    (
        "fte",
        "x_i = farr;",
        &["Implicit cast from float* to __int32"],
    ),
    ("fte", "b = (farr == farr);", &[]),
    (
        "fte",
        "farr = farr;",
        &["left operand must be an l-value (did you mean farr[0]?)"],
    ),
    ("fte", "pw = &farr;", &[]),
    (
        "fte",
        "x_s = farr;",
        &["Implicit cast from float* to string"],
    ),
    ("fte", "x_f = st;", &["Cannot cast from struct S to float"]),
    (
        "fte",
        "x_i = st;",
        &["Cannot cast from struct S to __int32"],
    ),
    ("fte", "st = st;", &[]),
    ("fte", "st += 1;", &["Cannot cast from float to struct S"]),
    (
        "fte",
        "st++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--st;", &["++ operator on unsupported type"]),
    ("fte", "pw = &st;", &[]),
    ("fte", "x_s = st;", &["Cannot cast from struct S to string"]),
];

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
#[test]
fn every_reference_is_read_as_fteqcc_reads_it() {
    for &(target, statement, expected) in REFERENCES {
        let pragma = if target == "fte" {
            "#pragma target fte\n"
        } else {
            ""
        };
        let found: Vec<String> = diags(&format!(
            "{pragma}{REFERENCE_DEFS}\nvoid() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect();
        assert_eq!(found, expected, "{target} {statement}");
    }
}

// spec FTEF-6.23: an 8- or 16-bit struct member is a bitfield at bit 0 of its word
#[test]
fn a_narrow_member_is_a_bitfield_at_bit_zero() {
    let defs = "struct S { int a : 3; int b : 5; __int8 c; __uint16 h; float f; }; S st; S *ps;";
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!("{defs}\nvoid() g = {{ {statement} }};\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    let bitcopy = [
        "BitandInt function not defined: cannot emulate int&int",
        "Opcode \"&|BITAND_I\" not valid for target. Consider the use of: #pragma target fte",
        "BitorInt function not defined: cannot emulate int|int",
        "Opcode \"||BITOR_I\" not valid for target. Consider the use of: #pragma target fte",
    ];
    assert_eq!(messages("st.h = 1;"), bitcopy);
    assert_eq!(messages("st.c = 1;"), bitcopy);
    let mut pointer = vec![
        "memgetval function not defined: cannot emulate OP_LOADP_*",
        "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
    ];
    pointer.extend(bitcopy);
    assert_eq!(messages("ps->h = 2;"), pointer);
}
