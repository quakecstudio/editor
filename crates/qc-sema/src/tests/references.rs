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
        fte("union { float a; vector v; } uu; float arr[1]; void() g = { arr[sizeof(uu)] = 1; };"),
        ["(constant) array index out of bounds (0 <= 12 < 1)"]
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

// spec FTEF-6.25: a call through a struct's function member reads the member type's parameters
#[test]
fn a_struct_function_member_is_called_by_its_type() {
    let found: Vec<String> = diags(
        "#pragma target fte\ntypedef struct { float(entity e = __NULL__) get; float(float a) two; } \
         api_t; api_t w; float x; void() g = { x = w.get(); x = w.two(); };\n",
    )
    .into_iter()
    .map(|(_, message)| message)
    .collect();
    assert_eq!(
        found,
        ["too few parameters on call to w, a will be UNDEFINED"]
    );
}

// spec FTEF-6.28: a member of a struct or union of size 0 is the fatal error "NAME was not
// defined yet"
// spec QCD-4.63: E0362 for a member of a struct of size 0
// spec FTEF-6.27: a bitfield's width is a signed integer, its layout C's unsigned arithmetic
#[test]
fn a_struct_of_no_size_has_no_members() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("#pragma target fte\n{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("struct T; T *pt; float x; void() g = { x = pt->a; };"),
        ["T was not defined yet"]
    );
    assert_eq!(
        messages("struct T2 { }; T2 t; float x; void() g = { x = t.a; };"),
        ["struct T2 has no members", "T2 was not defined yet"]
    );
    assert_eq!(
        messages("struct S { int a : -1; }; S s; void() g = { s.a = 1; };"),
        ["S was not defined yet"]
    );
    assert_eq!(
        messages("struct S { int a : -3; int b : 2; }; S s; float x; void() g = { x = s.b; };"),
        ["S was not defined yet"]
    );
    assert_eq!(
        messages("struct S { int a : -1; int b; }; S s; float x; void() g = { x = s.b; };"),
        Vec::<String>::new()
    );
}

// spec FTEF-31.12: a narrow element's address through a pointer is a byte offset by `OP_ADD_I`
#[test]
fn a_narrow_elements_address_is_a_byte_offset() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!("{source}\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    let add = [
        "AddInt function not defined: cannot emulate int+int",
        "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
    ];
    let mul = [
        "MulInt function not defined: cannot emulate int*int",
        "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
    ];
    assert_eq!(
        messages("__int8 *pc; int i; void() f = { local __int8 *q = &pc[i]; };"),
        add
    );
    assert_eq!(
        messages("__int8 *pc; void() f = { local __int8 *q = &pc[2i]; };"),
        add
    );
    assert_eq!(
        messages("__int16 *pc; int i; void() f = { local __int16 *q = &pc[i]; };"),
        [mul, add].concat()
    );
    assert!(messages("__uint16 *pc; void() f = { local __uint16 *q = &pc[0]; };").is_empty());
    assert_eq!(
        messages("__int16 *pc; void() f = { local __int16 *q = &pc[3]; };"),
        add
    );
    assert_eq!(
        messages("__int8 *pc; void() f = { local __int8 *q = &pc[1.5]; };"),
        [
            &["Numerical overflow. 1.500000 will be rounded to 1"][..],
            &add[..]
        ]
        .concat()
    );
    assert!(
        messages(
            "#pragma target fte\n__int16 *pc; int i; void() f = { local __int16 *q = &pc[i]; };"
        )
        .is_empty()
    );
}

/// The definitions every case of [`REFERENCES`] reads.
const REFERENCE_DEFS: &str = "struct S { int a : 3; int b : 5; __int8 c; __uint16 h; float f; vector v; }; S st; S *ps; S sarr[3]; S **pps; struct S4 { float b; float a[3]; vector av[2]; }; S4 s4; S4 *ps4; typedef struct { float q; int r[2]; } T; T t; float f; int i; __uint u; __int64 l; vector v; string s; entity e; float b; float farr[4]; vector varr[3]; int iarr[4]; __int8 carr[4]; __uint8 ucarr[4]; __int16 harr[2]; float *p; int *pi; vector *pv; __int8 *pc; __uint16 *ph; void *pw; .float fl; .vector fv; .int fi; float x_f; int x_i; vector x_v; string x_s;";

/// Reads, comparisons, stores, steps and addresses of each kind of reference, on the standard,
/// FTE, Hexen II, DarkPlaces and KK7 targets: the pinned FTEQCC's diagnostics for each.
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
        "x_f = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe return",
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
    ("id", "x_f = s4.a[1];", &[]),
    (
        "id",
        "x_i = s4.a[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (s4.a[1] == s4.a[1]);", &[]),
    ("id", "s4.a[1] = s4.a[1];", &[]),
    ("id", "s4.a[1] += 1;", &[]),
    ("id", "s4.a[1]++;", &[]),
    ("id", "--s4.a[1];", &[]),
    (
        "id",
        "pw = &s4.a[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = s4.a[1];",
        &["Cannot cast from float to string"],
    ),
    (
        "id",
        "pw = &s4.a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "id",
        "x_i = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (s4.a[4] == s4.a[4]);",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "id",
        "s4.a[4] = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "id",
        "s4.a[4] += 1;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "id",
        "s4.a[4]++;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "id",
        "--s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "id",
        "pw = &s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "id",
        "x_i = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (s4.a[3.5] == s4.a[3.5]);",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "id",
        "s4.a[3.5] = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "id",
        "s4.a[3.5] += 1;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "id",
        "s4.a[3.5]++;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "id",
        "--s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "id",
        "pw = &s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "pw = &s4.av[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (ps4->a[i] == ps4->a[i]);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[i] = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[i] += 1;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[i]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_i = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "b = (ps4->a[1] == ps4->a[1]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[1] = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[1] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &ps4->a[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "id",
        "x_f = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "x_i = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "b = (s4.a == s4.a);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "s4.a = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "s4.a += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "s4.a++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "--s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "pw = &s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "x_s = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "id",
        "x_f = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float* to float",
        ],
    ),
    (
        "id",
        "x_i = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to __int32",
        ],
    ),
    (
        "id",
        "b = (ps4->a == ps4->a);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "ps4->a = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "left operand must be an l-value (did you mean ps4[0]?)",
        ],
    ),
    (
        "id",
        "pw = &ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to string",
        ],
    ),
    ("id", "x_f = t.q;", &[]),
    (
        "id",
        "x_i = t.q;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("id", "b = (t.q == t.q);", &[]),
    ("id", "t.q = t.q;", &[]),
    ("id", "t.q += 1;", &[]),
    ("id", "t.q++;", &[]),
    ("id", "--t.q;", &[]),
    (
        "id",
        "pw = &t.q;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("id", "x_s = t.q;", &["Cannot cast from float to string"]),
    (
        "id",
        "pw = &t.r[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_f = t.r[1];",
        &["itof emulation: denormals are unsafe t"],
    ),
    ("id", "x_i = t.r[1];", &[]),
    (
        "id",
        "b = (t.r[1] == t.r[1]);",
        &["Result of comparison is constant"],
    ),
    ("id", "t.r[1] = t.r[1];", &[]),
    (
        "id",
        "t.r[1] += 1;",
        &[
            "itof emulation: denormals are unsafe t",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "id",
        "t.r[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "--t.r[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "pw = &t.r[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "id",
        "x_s = t.r[1];",
        &["Implicit cast from __int32 to string"],
    ),
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
    ("fte", "x_f = s4.a[1];", &[]),
    ("fte", "x_i = s4.a[1];", &[]),
    ("fte", "b = (s4.a[1] == s4.a[1]);", &[]),
    ("fte", "s4.a[1] = s4.a[1];", &[]),
    ("fte", "s4.a[1] += 1;", &[]),
    ("fte", "s4.a[1]++;", &[]),
    ("fte", "--s4.a[1];", &[]),
    ("fte", "pw = &s4.a[1];", &[]),
    (
        "fte",
        "x_s = s4.a[1];",
        &["Cannot cast from float to string"],
    ),
    ("fte", "x_f = s4.a[i];", &[]),
    ("fte", "x_i = s4.a[i];", &[]),
    ("fte", "b = (s4.a[i] == s4.a[i]);", &[]),
    ("fte", "s4.a[i] = s4.a[i];", &[]),
    ("fte", "s4.a[i] += 1;", &[]),
    ("fte", "s4.a[i]++;", &[]),
    ("fte", "--s4.a[i];", &[]),
    ("fte", "pw = &s4.a[i];", &[]),
    (
        "fte",
        "x_s = s4.a[i];",
        &["Cannot cast from float to string"],
    ),
    (
        "fte",
        "x_f = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "x_i = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "b = (s4.a[4] == s4.a[4]);",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "fte",
        "s4.a[4] = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "fte",
        "s4.a[4] += 1;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "s4.a[4]++;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "--s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "pw = &s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "fte",
        "x_s = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "fte",
        "x_f = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "x_i = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "b = (s4.a[3.5] == s4.a[3.5]);",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "s4.a[3.5] = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "s4.a[3.5] += 1;",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "s4.a[3.5]++;",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "--s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "pw = &s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "fte",
        "x_s = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "Cannot cast from float to string",
        ],
    ),
    (
        "fte",
        "x_f = s4.av[i];",
        &["Cannot cast from vector to float"],
    ),
    (
        "fte",
        "x_i = s4.av[i];",
        &["Cannot cast from vector to __int32"],
    ),
    ("fte", "b = (s4.av[i] == s4.av[i]);", &[]),
    ("fte", "s4.av[i] = s4.av[i];", &[]),
    (
        "fte",
        "s4.av[i] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "fte",
        "s4.av[i]++;",
        &["post increment operator not supported with this type"],
    ),
    ("fte", "--s4.av[i];", &["++ operator on unsupported type"]),
    ("fte", "pw = &s4.av[i];", &[]),
    (
        "fte",
        "x_s = s4.av[i];",
        &["Cannot cast from vector to string"],
    ),
    ("fte", "x_f = ps4->a[i];", &[]),
    ("fte", "x_i = ps4->a[i];", &[]),
    ("fte", "b = (ps4->a[i] == ps4->a[i]);", &[]),
    ("fte", "ps4->a[i] = ps4->a[i];", &[]),
    ("fte", "ps4->a[i] += 1;", &[]),
    ("fte", "ps4->a[i]++;", &[]),
    ("fte", "--ps4->a[i];", &[]),
    ("fte", "pw = &ps4->a[i];", &[]),
    (
        "fte",
        "x_s = ps4->a[i];",
        &["Cannot cast from float to string"],
    ),
    ("fte", "x_f = ps4->a[1];", &[]),
    ("fte", "x_i = ps4->a[1];", &[]),
    ("fte", "b = (ps4->a[1] == ps4->a[1]);", &[]),
    ("fte", "ps4->a[1] = ps4->a[1];", &[]),
    ("fte", "ps4->a[1] += 1;", &[]),
    ("fte", "ps4->a[1]++;", &[]),
    ("fte", "--ps4->a[1];", &[]),
    ("fte", "pw = &ps4->a[1];", &[]),
    (
        "fte",
        "x_s = ps4->a[1];",
        &["Cannot cast from float to string"],
    ),
    ("fte", "x_f = s4.a;", &["Cannot cast from float* to float"]),
    (
        "fte",
        "x_i = s4.a;",
        &["Implicit cast from float* to __int32"],
    ),
    ("fte", "b = (s4.a == s4.a);", &[]),
    (
        "fte",
        "s4.a = s4.a;",
        &["left operand must be an l-value (did you mean s4[0]?)"],
    ),
    ("fte", "pw = &s4.a;", &[]),
    (
        "fte",
        "x_s = s4.a;",
        &["Implicit cast from float* to string"],
    ),
    (
        "fte",
        "x_f = ps4->a;",
        &["Cannot cast from float* to float"],
    ),
    (
        "fte",
        "x_i = ps4->a;",
        &["Implicit cast from float* to __int32"],
    ),
    ("fte", "b = (ps4->a == ps4->a);", &[]),
    (
        "fte",
        "ps4->a = ps4->a;",
        &["left operand must be an l-value (did you mean ps4[0]?)"],
    ),
    ("fte", "pw = &ps4->a;", &[]),
    (
        "fte",
        "x_s = ps4->a;",
        &["Implicit cast from float* to string"],
    ),
    ("fte", "x_f = t.q;", &[]),
    ("fte", "x_i = t.q;", &[]),
    ("fte", "b = (t.q == t.q);", &[]),
    ("fte", "t.q = t.q;", &[]),
    ("fte", "t.q += 1;", &[]),
    ("fte", "t.q++;", &[]),
    ("fte", "--t.q;", &[]),
    ("fte", "pw = &t.q;", &[]),
    ("fte", "x_s = t.q;", &["Cannot cast from float to string"]),
    ("fte", "x_f = t.r[i];", &[]),
    ("fte", "x_i = t.r[i];", &[]),
    ("fte", "b = (t.r[i] == t.r[i]);", &[]),
    ("fte", "t.r[i] = t.r[i];", &[]),
    ("fte", "t.r[i] += 1;", &[]),
    ("fte", "t.r[i]++;", &[]),
    ("fte", "--t.r[i];", &[]),
    ("fte", "pw = &t.r[i];", &[]),
    (
        "fte",
        "x_s = t.r[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("fte", "x_f = t.r[1];", &[]),
    ("fte", "x_i = t.r[1];", &[]),
    ("fte", "b = (t.r[1] == t.r[1]);", &[]),
    ("fte", "t.r[1] = t.r[1];", &[]),
    ("fte", "t.r[1] += 1;", &[]),
    ("fte", "t.r[1]++;", &[]),
    ("fte", "--t.r[1];", &[]),
    ("fte", "pw = &t.r[1];", &[]),
    (
        "fte",
        "x_s = t.r[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "hexen2",
        "pw = &st.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "st.b++;",
        &["post increment operator on bitfield"],
    ),
    (
        "hexen2",
        "pw = &st.b;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &st.c;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &st.h;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "x_f = st.f;", &[]),
    (
        "hexen2",
        "x_i = st.f;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (st.f == st.f);", &[]),
    ("hexen2", "st.f = st.f;", &[]),
    ("hexen2", "st.f += 1;", &[]),
    ("hexen2", "st.f++;", &[]),
    ("hexen2", "--st.f;", &[]),
    (
        "hexen2",
        "pw = &st.f;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = st.f;",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "x_f = st.v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "hexen2",
        "x_i = st.v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("hexen2", "b = (st.v == st.v);", &[]),
    ("hexen2", "st.v = st.v;", &[]),
    (
        "hexen2",
        "st.v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "hexen2",
        "st.v++;",
        &["post increment operator not supported with this type"],
    ),
    ("hexen2", "--st.v;", &["++ operator on unsupported type"]),
    (
        "hexen2",
        "pw = &st.v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = st.v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "hexen2",
        "x_f = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "x_i = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "b = (ps->a == ps->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "ps->a = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "ps->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "ps->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "--ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("hexen2", "pw = &ps->a;", &[]),
    (
        "hexen2",
        "x_s = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "hexen2",
        "pw = &ps->c;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &ps->h;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (ps->f == ps->f);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps->f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps->f++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &ps->f;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "hexen2",
        "x_i = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "hexen2",
        "b = (ps->v == ps->v);",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps->v = ps->v;",
        &["Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte"],
    ),
    (
        "hexen2",
        "ps->v += 1;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "hexen2",
        "ps->v++;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "hexen2",
        "--ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "hexen2",
        "pw = &ps->v;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "hexen2",
        "pw = &sarr[1].a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = sarr[1].v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "hexen2",
        "x_i = sarr[1].v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("hexen2", "b = (sarr[1].v == sarr[1].v);", &[]),
    ("hexen2", "sarr[1].v = sarr[1].v;", &[]),
    (
        "hexen2",
        "sarr[1].v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "hexen2",
        "sarr[1].v++;",
        &["post increment operator not supported with this type"],
    ),
    (
        "hexen2",
        "--sarr[1].v;",
        &["++ operator on unsupported type"],
    ),
    (
        "hexen2",
        "pw = &sarr[1].v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = sarr[1].v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "hexen2",
        "x_f = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
        "x_i = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
        "b = ((*pps)->a == (*pps)->a);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
        "(*pps)->a += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
        "(*pps)->a++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
        "--(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "hexen2",
        "pw = &(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    ("hexen2", "x_f = farr[2];", &[]),
    (
        "hexen2",
        "x_i = farr[2];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (farr[2] == farr[2]);", &[]),
    ("hexen2", "farr[2] = farr[2];", &[]),
    ("hexen2", "farr[2] += 1;", &[]),
    ("hexen2", "farr[2]++;", &[]),
    ("hexen2", "--farr[2];", &[]),
    (
        "hexen2",
        "pw = &farr[2];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = farr[2];",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "pw = &farr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "x_f = farr[f];", &[]),
    (
        "hexen2",
        "x_i = farr[f];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (farr[f] == farr[f]);", &[]),
    ("hexen2", "farr[f] = farr[f];", &[]),
    ("hexen2", "farr[f] += 1;", &[]),
    ("hexen2", "farr[f]++;", &[]),
    ("hexen2", "--farr[f];", &[]),
    (
        "hexen2",
        "pw = &farr[f];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = farr[f];",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "x_f = varr[1];",
        &["Cannot cast from vector to float"],
    ),
    (
        "hexen2",
        "x_i = varr[1];",
        &["Cannot cast from vector to __int32"],
    ),
    ("hexen2", "b = (varr[1] == varr[1]);", &[]),
    ("hexen2", "varr[1] = varr[1];", &[]),
    (
        "hexen2",
        "varr[1] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "hexen2",
        "varr[1]++;",
        &["post increment operator not supported with this type"],
    ),
    ("hexen2", "--varr[1];", &["++ operator on unsupported type"]),
    (
        "hexen2",
        "pw = &varr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = varr[1];",
        &["Cannot cast from vector to string"],
    ),
    (
        "hexen2",
        "pw = &varr[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &iarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = iarr[1];",
        &["itof emulation: denormals are unsafe iarr"],
    ),
    ("hexen2", "x_i = iarr[1];", &[]),
    (
        "hexen2",
        "b = (iarr[1] == iarr[1]);",
        &["Result of comparison is constant"],
    ),
    ("hexen2", "iarr[1] = iarr[1];", &[]),
    (
        "hexen2",
        "iarr[1] += 1;",
        &[
            "itof emulation: denormals are unsafe iarr",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "iarr[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--iarr[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &iarr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = iarr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "hexen2",
        "pw = &carr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "b = (carr[i] == carr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "carr[i] = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "carr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "carr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "b = (ucarr[i] == ucarr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ucarr[i] = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ucarr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ucarr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "b = (harr[i] == harr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "harr[i] = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "harr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "harr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &harr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (p[0] == p[0]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "p[0] = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "p[0]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "pw = &p[0];", &[]),
    (
        "hexen2",
        "x_s = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (p[i] == p[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "p[i] = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "p[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &p[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
        "p[f] = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "p[f]++;",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "--p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
        "x_s = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (*p == *p);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "*p = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--*p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "pw = &*p;", &[]),
    (
        "hexen2",
        "x_s = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_i = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "b = (pi[i] == pi[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pi[i] = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pi[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &pi[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "hexen2",
        "x_f = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "hexen2",
        "x_i = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
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
        "hexen2",
        "pv[i] += 1;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "hexen2",
        "pv[i]++;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "hexen2",
        "--pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
        "x_s = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "hexen2",
        "pw = &pc[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &pc[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "pw = &*pc;", &[]),
    (
        "hexen2",
        "pw = &ph[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "pw = &*ph;", &[]),
    (
        "hexen2",
        "x_f = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (s[0] == s[0]);",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
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
        "hexen2",
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
        "hexen2",
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
        "hexen2",
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
        "hexen2",
        "pw = &s[0];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    (
        "hexen2",
        "x_s = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "pw = &s[i];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    ("hexen2", "x_f = e.fl;", &[]),
    (
        "hexen2",
        "x_i = e.fl;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (e.fl == e.fl);", &[]),
    ("hexen2", "e.fl = e.fl;", &[]),
    ("hexen2", "e.fl += 1;", &[]),
    ("hexen2", "e.fl++;", &[]),
    ("hexen2", "--e.fl;", &[]),
    ("hexen2", "pw = &e.fl;", &[]),
    (
        "hexen2",
        "x_s = e.fl;",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "x_f = e.fv;",
        &["Cannot cast from vector to float"],
    ),
    (
        "hexen2",
        "x_i = e.fv;",
        &["Cannot cast from vector to __int32"],
    ),
    ("hexen2", "b = (e.fv == e.fv);", &[]),
    ("hexen2", "e.fv = e.fv;", &[]),
    (
        "hexen2",
        "e.fv += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "hexen2",
        "e.fv++;",
        &["post increment operator not supported with this type"],
    ),
    ("hexen2", "--e.fv;", &["++ operator on unsupported type"]),
    ("hexen2", "pw = &e.fv;", &[]),
    (
        "hexen2",
        "x_s = e.fv;",
        &["Cannot cast from vector to string"],
    ),
    ("hexen2", "x_i = e.fi;", &[]),
    ("hexen2", "b = (e.fi == e.fi);", &[]),
    ("hexen2", "e.fi = e.fi;", &[]),
    (
        "hexen2",
        "e.fi++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--e.fi;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("hexen2", "pw = &e.fi;", &[]),
    (
        "hexen2",
        "x_s = e.fi;",
        &["Implicit cast from __int32 to string"],
    ),
    ("hexen2", "x_f = v[1];", &[]),
    (
        "hexen2",
        "x_i = v[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (v[1] == v[1]);", &[]),
    ("hexen2", "v[1] = v[1];", &[]),
    ("hexen2", "v[1] += 1;", &[]),
    ("hexen2", "v[1]++;", &[]),
    ("hexen2", "--v[1];", &[]),
    (
        "hexen2",
        "pw = &v[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = v[1];",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "x_f = v[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "hexen2",
        "x_i = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (v[i] == v[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "hexen2",
        "pw = &v[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Cannot cast from float to string",
        ],
    ),
    ("hexen2", "x_f = v_y;", &[]),
    (
        "hexen2",
        "x_i = v_y;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (v_y == v_y);", &[]),
    ("hexen2", "v_y = v_y;", &[]),
    ("hexen2", "v_y += 1;", &[]),
    ("hexen2", "v_y++;", &[]),
    ("hexen2", "--v_y;", &[]),
    (
        "hexen2",
        "pw = &v_y;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = v_y;",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "x_f = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_i = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "b = (farr == farr);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "farr = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "farr += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "farr++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "--farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "pw = &farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_s = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_f = st;",
        &["Cannot cast from struct S to float"],
    ),
    (
        "hexen2",
        "x_i = st;",
        &["Cannot cast from struct S to __int32"],
    ),
    ("hexen2", "st = st;", &[]),
    (
        "hexen2",
        "st += 1;",
        &["Cannot cast from float to struct S"],
    ),
    (
        "hexen2",
        "st++;",
        &["post increment operator not supported with this type"],
    ),
    ("hexen2", "--st;", &["++ operator on unsupported type"]),
    (
        "hexen2",
        "pw = &st;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = st;",
        &["Cannot cast from struct S to string"],
    ),
    ("hexen2", "x_f = s4.a[1];", &[]),
    (
        "hexen2",
        "x_i = s4.a[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (s4.a[1] == s4.a[1]);", &[]),
    ("hexen2", "s4.a[1] = s4.a[1];", &[]),
    ("hexen2", "s4.a[1] += 1;", &[]),
    ("hexen2", "s4.a[1]++;", &[]),
    ("hexen2", "--s4.a[1];", &[]),
    (
        "hexen2",
        "pw = &s4.a[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = s4.a[1];",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "pw = &s4.a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "hexen2",
        "x_i = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (s4.a[4] == s4.a[4]);",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "hexen2",
        "s4.a[4] = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "hexen2",
        "s4.a[4] += 1;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "hexen2",
        "s4.a[4]++;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "hexen2",
        "--s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "hexen2",
        "pw = &s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "hexen2",
        "x_i = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (s4.a[3.5] == s4.a[3.5]);",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "hexen2",
        "s4.a[3.5] = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "hexen2",
        "s4.a[3.5] += 1;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "hexen2",
        "s4.a[3.5]++;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "hexen2",
        "--s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "hexen2",
        "pw = &s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "pw = &s4.av[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (ps4->a[i] == ps4->a[i]);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps4->a[i] = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps4->a[i]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_i = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "b = (ps4->a[1] == ps4->a[1]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps4->a[1] = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps4->a[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &ps4->a[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "hexen2",
        "x_f = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_i = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "b = (s4.a == s4.a);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "s4.a = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "s4.a += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "s4.a++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "--s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "pw = &s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_s = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "hexen2",
        "x_f = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float* to float",
        ],
    ),
    (
        "hexen2",
        "x_i = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to __int32",
        ],
    ),
    (
        "hexen2",
        "b = (ps4->a == ps4->a);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "ps4->a = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "left operand must be an l-value (did you mean ps4[0]?)",
        ],
    ),
    (
        "hexen2",
        "pw = &ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to string",
        ],
    ),
    ("hexen2", "x_f = t.q;", &[]),
    (
        "hexen2",
        "x_i = t.q;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("hexen2", "b = (t.q == t.q);", &[]),
    ("hexen2", "t.q = t.q;", &[]),
    ("hexen2", "t.q += 1;", &[]),
    ("hexen2", "t.q++;", &[]),
    ("hexen2", "--t.q;", &[]),
    (
        "hexen2",
        "pw = &t.q;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = t.q;",
        &["Cannot cast from float to string"],
    ),
    (
        "hexen2",
        "pw = &t.r[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_f = t.r[1];",
        &["itof emulation: denormals are unsafe t"],
    ),
    ("hexen2", "x_i = t.r[1];", &[]),
    (
        "hexen2",
        "b = (t.r[1] == t.r[1]);",
        &["Result of comparison is constant"],
    ),
    ("hexen2", "t.r[1] = t.r[1];", &[]),
    (
        "hexen2",
        "t.r[1] += 1;",
        &[
            "itof emulation: denormals are unsafe t",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "hexen2",
        "t.r[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "--t.r[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "pw = &t.r[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "hexen2",
        "x_s = t.r[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = st.a;", &[]),
    ("dp", "x_i = st.a;", &[]),
    ("dp", "b = (st.a == st.a);", &[]),
    ("dp", "st.a = st.a;", &[]),
    ("dp", "st.a += 1;", &[]),
    ("dp", "st.a++;", &[]),
    ("dp", "--st.a;", &[]),
    (
        "dp",
        "pw = &st.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = st.a;",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = st.b;", &[]),
    ("dp", "x_i = st.b;", &[]),
    ("dp", "b = (st.b == st.b);", &[]),
    ("dp", "st.b = st.b;", &[]),
    ("dp", "st.b += 1;", &[]),
    ("dp", "st.b++;", &["post increment operator on bitfield"]),
    ("dp", "--st.b;", &[]),
    (
        "dp",
        "pw = &st.b;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = st.b;",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = st.c;", &[]),
    ("dp", "x_i = st.c;", &[]),
    ("dp", "b = (st.c == st.c);", &[]),
    ("dp", "st.c = st.c;", &[]),
    ("dp", "st.c += 1;", &[]),
    ("dp", "st.c++;", &[]),
    ("dp", "--st.c;", &[]),
    (
        "dp",
        "pw = &st.c;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = st.c;",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "dp",
        "x_i = st.h;",
        &["OP_DIV_U emulation: assuming the high bit is not set..."],
    ),
    (
        "dp",
        "st.h = st.h;",
        &["OP_DIV_U emulation: assuming the high bit is not set..."],
    ),
    (
        "dp",
        "st.h += 1;",
        &["OP_DIV_U emulation: assuming the high bit is not set..."],
    ),
    (
        "dp",
        "st.h++;",
        &["OP_DIV_U emulation: assuming the high bit is not set..."],
    ),
    (
        "dp",
        "--st.h;",
        &["OP_DIV_U emulation: assuming the high bit is not set..."],
    ),
    (
        "dp",
        "pw = &st.h;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = st.h;",
        &[
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "Implicit cast from __uint32 to string",
        ],
    ),
    ("dp", "x_f = st.f;", &[]),
    ("dp", "x_i = st.f;", &[]),
    ("dp", "b = (st.f == st.f);", &[]),
    ("dp", "st.f = st.f;", &[]),
    ("dp", "st.f += 1;", &[]),
    ("dp", "st.f++;", &[]),
    ("dp", "--st.f;", &[]),
    (
        "dp",
        "pw = &st.f;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = st.f;", &["Cannot cast from float to string"]),
    ("dp", "x_f = st.v;", &["Cannot cast from vector to float"]),
    ("dp", "x_i = st.v;", &["Cannot cast from vector to __int32"]),
    ("dp", "b = (st.v == st.v);", &[]),
    ("dp", "st.v = st.v;", &[]),
    ("dp", "st.v += 1;", &["Implicit cast from float to vector"]),
    (
        "dp",
        "st.v++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--st.v;", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &st.v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = st.v;", &["Cannot cast from vector to string"]),
    (
        "dp",
        "x_f = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "x_i = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "b = (ps->a == ps->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "ps->a = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "ps->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "ps->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "--ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("dp", "pw = &ps->a;", &[]),
    (
        "dp",
        "x_s = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "dp",
        "x_f = ps->c;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = ps->c;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (ps->c == ps->c);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->c = ps->c;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->c += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->c++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--ps->c;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &ps->c;", &[]),
    (
        "dp",
        "x_s = ps->c;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "dp",
        "x_i = ps->h;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
        ],
    ),
    (
        "dp",
        "ps->h = ps->h;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->h += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->h++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--ps->h;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &ps->h;", &[]),
    (
        "dp",
        "x_s = ps->h;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "Implicit cast from __uint32 to string",
        ],
    ),
    (
        "dp",
        "x_f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (ps->f == ps->f);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps->f += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps->f++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &ps->f;",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "dp",
        "x_i = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "dp",
        "b = (ps->v == ps->v);",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps->v = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps->v += 1;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps->v++;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "dp",
        "--ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "dp",
        "pw = &ps->v;",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    ("dp", "x_f = sarr[1].a;", &[]),
    ("dp", "x_i = sarr[1].a;", &[]),
    ("dp", "b = (sarr[1].a == sarr[1].a);", &[]),
    ("dp", "sarr[1].a = sarr[1].a;", &[]),
    ("dp", "sarr[1].a += 1;", &[]),
    ("dp", "sarr[1].a++;", &[]),
    ("dp", "--sarr[1].a;", &[]),
    (
        "dp",
        "pw = &sarr[1].a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = sarr[1].a;",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "dp",
        "pw = &sarr[i].c;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_f = sarr[i].f;", &[]),
    ("dp", "x_i = sarr[i].f;", &[]),
    ("dp", "b = (sarr[i].f == sarr[i].f);", &[]),
    ("dp", "sarr[i].f = sarr[i].f;", &[]),
    ("dp", "sarr[i].f += 1;", &[]),
    ("dp", "sarr[i].f++;", &[]),
    ("dp", "--sarr[i].f;", &[]),
    (
        "dp",
        "pw = &sarr[i].f;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = sarr[i].f;",
        &["Cannot cast from float to string"],
    ),
    (
        "dp",
        "x_f = sarr[1].v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "dp",
        "x_i = sarr[1].v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("dp", "b = (sarr[1].v == sarr[1].v);", &[]),
    ("dp", "sarr[1].v = sarr[1].v;", &[]),
    (
        "dp",
        "sarr[1].v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "dp",
        "sarr[1].v++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--sarr[1].v;", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &sarr[1].v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = sarr[1].v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "dp",
        "x_f = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
        "x_i = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
        "b = ((*pps)->a == (*pps)->a);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
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
        "dp",
        "(*pps)->a += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
        "(*pps)->a++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
        "--(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "dp",
        "pw = &(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    ("dp", "x_f = farr[2];", &[]),
    ("dp", "x_i = farr[2];", &[]),
    ("dp", "b = (farr[2] == farr[2]);", &[]),
    ("dp", "farr[2] = farr[2];", &[]),
    ("dp", "farr[2] += 1;", &[]),
    ("dp", "farr[2]++;", &[]),
    ("dp", "--farr[2];", &[]),
    (
        "dp",
        "pw = &farr[2];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = farr[2];",
        &["Cannot cast from float to string"],
    ),
    ("dp", "x_f = farr[i];", &[]),
    ("dp", "x_i = farr[i];", &[]),
    ("dp", "b = (farr[i] == farr[i]);", &[]),
    ("dp", "farr[i] = farr[i];", &[]),
    ("dp", "farr[i] += 1;", &[]),
    ("dp", "farr[i]++;", &[]),
    ("dp", "--farr[i];", &[]),
    (
        "dp",
        "pw = &farr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = farr[i];",
        &["Cannot cast from float to string"],
    ),
    ("dp", "x_f = farr[f];", &[]),
    ("dp", "x_i = farr[f];", &[]),
    ("dp", "b = (farr[f] == farr[f]);", &[]),
    ("dp", "farr[f] = farr[f];", &[]),
    ("dp", "farr[f] += 1;", &[]),
    ("dp", "farr[f]++;", &[]),
    ("dp", "--farr[f];", &[]),
    (
        "dp",
        "pw = &farr[f];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = farr[f];",
        &["Cannot cast from float to string"],
    ),
    (
        "dp",
        "x_f = varr[1];",
        &["Cannot cast from vector to float"],
    ),
    (
        "dp",
        "x_i = varr[1];",
        &["Cannot cast from vector to __int32"],
    ),
    ("dp", "b = (varr[1] == varr[1]);", &[]),
    ("dp", "varr[1] = varr[1];", &[]),
    (
        "dp",
        "varr[1] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "dp",
        "varr[1]++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--varr[1];", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &varr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = varr[1];",
        &["Cannot cast from vector to string"],
    ),
    (
        "dp",
        "x_f = varr[i];",
        &["Cannot cast from vector to float"],
    ),
    (
        "dp",
        "x_i = varr[i];",
        &["Cannot cast from vector to __int32"],
    ),
    ("dp", "b = (varr[i] == varr[i]);", &[]),
    ("dp", "varr[i] = varr[i];", &[]),
    (
        "dp",
        "varr[i] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "dp",
        "varr[i]++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--varr[i];", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &varr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = varr[i];",
        &["Cannot cast from vector to string"],
    ),
    ("dp", "x_f = iarr[i];", &[]),
    ("dp", "x_i = iarr[i];", &[]),
    ("dp", "b = (iarr[i] == iarr[i]);", &[]),
    ("dp", "iarr[i] = iarr[i];", &[]),
    ("dp", "iarr[i] += 1;", &[]),
    ("dp", "iarr[i]++;", &[]),
    ("dp", "--iarr[i];", &[]),
    (
        "dp",
        "pw = &iarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = iarr[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = iarr[1];", &[]),
    ("dp", "x_i = iarr[1];", &[]),
    ("dp", "b = (iarr[1] == iarr[1]);", &[]),
    ("dp", "iarr[1] = iarr[1];", &[]),
    ("dp", "iarr[1] += 1;", &[]),
    ("dp", "iarr[1]++;", &[]),
    ("dp", "--iarr[1];", &[]),
    (
        "dp",
        "pw = &iarr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = iarr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = carr[1];", &[]),
    ("dp", "x_i = carr[1];", &[]),
    ("dp", "b = (carr[1] == carr[1]);", &[]),
    ("dp", "carr[1] = carr[1];", &[]),
    ("dp", "carr[1] += 1;", &[]),
    ("dp", "carr[1]++;", &[]),
    ("dp", "--carr[1];", &[]),
    (
        "dp",
        "pw = &carr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = carr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "dp",
        "x_f = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (carr[i] == carr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "carr[i] = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "carr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "carr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pw = &carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_f = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (ucarr[i] == ucarr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ucarr[i] = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ucarr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ucarr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pw = &ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_f = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (harr[i] == harr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "harr[i] = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "harr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "harr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pw = &harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_f = harr[1];", &[]),
    ("dp", "x_i = harr[1];", &[]),
    ("dp", "b = (harr[1] == harr[1]);", &[]),
    ("dp", "harr[1] = harr[1];", &[]),
    ("dp", "harr[1] += 1;", &[]),
    ("dp", "harr[1]++;", &[]),
    ("dp", "--harr[1];", &[]),
    (
        "dp",
        "pw = &harr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = harr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "dp",
        "x_f = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (p[0] == p[0]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "p[0] = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "p[0] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "p[0]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &p[0];", &[]),
    (
        "dp",
        "x_s = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (p[i] == p[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "p[i] = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "p[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "p[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &p[i];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = p[f];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = p[f];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (p[f] == p[f]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "p[f] = p[f];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "p[f] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "p[f]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--p[f];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &p[f];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = p[f];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (*p == *p);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "*p = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "*p += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--*p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &*p;", &[]),
    (
        "dp",
        "x_s = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (pi[i] == pi[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pi[i] = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pi[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pi[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &pi[i];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "dp",
        "x_f = pv[i];",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "dp",
        "x_i = pv[i];",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "dp",
        "b = (pv[i] == pv[i]);",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pv[i] = pv[i];",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pv[i] += 1;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pv[i]++;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "dp",
        "--pv[i];",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "dp",
        "pw = &pv[i];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = pv[i];",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "dp",
        "x_f = pc[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "x_i = pc[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "b = (pc[i] == pc[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "pc[i] = pc[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "pc[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "pc[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    (
        "dp",
        "--pc[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
        ],
    ),
    ("dp", "pw = &pc[i];", &[]),
    (
        "dp",
        "x_s = pc[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "dp",
        "x_f = pc[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = pc[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (pc[1] == pc[1]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pc[1] = pc[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pc[1] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pc[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--pc[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &pc[1];", &[]),
    (
        "dp",
        "x_s = pc[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "dp",
        "x_f = *pc;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = *pc;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (*pc == *pc);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "*pc = *pc;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "*pc += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--*pc;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &*pc;", &[]),
    (
        "dp",
        "x_s = *pc;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "dp",
        "x_i = ph[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
        ],
    ),
    (
        "dp",
        "ph[1] = ph[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ph[1] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ph[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--ph[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &ph[1];", &[]),
    (
        "dp",
        "x_s = ph[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "Implicit cast from __uint32 to string",
        ],
    ),
    (
        "dp",
        "x_i = *ph;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
        ],
    ),
    (
        "dp",
        "*ph = *ph;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "*ph += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--*ph;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "pw = &*ph;", &[]),
    (
        "dp",
        "x_s = *ph;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "Implicit cast from __uint32 to string",
        ],
    ),
    (
        "dp",
        "x_f = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (s[0] == s[0]);",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "s[0] = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "s[0] += 1;",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "s[0]++;",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pw = &s[0];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    (
        "dp",
        "x_s = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_i = s[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "OP_DIV_U emulation: assuming the high bit is not set...",
        ],
    ),
    (
        "dp",
        "s[i] = s[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "s[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "s[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "--s[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "pow function not defined: cannot emulate int*^int",
            "Opcode \"*^|POW_I\" is not supported.",
            "OP_DIV_U emulation: assuming the high bit is not set...",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "pw = &s[i];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    ("dp", "x_f = e.fl;", &[]),
    ("dp", "x_i = e.fl;", &[]),
    ("dp", "b = (e.fl == e.fl);", &[]),
    ("dp", "e.fl = e.fl;", &[]),
    ("dp", "e.fl += 1;", &[]),
    ("dp", "e.fl++;", &[]),
    ("dp", "--e.fl;", &[]),
    ("dp", "pw = &e.fl;", &[]),
    ("dp", "x_s = e.fl;", &["Cannot cast from float to string"]),
    ("dp", "x_f = e.fv;", &["Cannot cast from vector to float"]),
    ("dp", "x_i = e.fv;", &["Cannot cast from vector to __int32"]),
    ("dp", "b = (e.fv == e.fv);", &[]),
    ("dp", "e.fv = e.fv;", &[]),
    ("dp", "e.fv += 1;", &["Implicit cast from float to vector"]),
    (
        "dp",
        "e.fv++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--e.fv;", &["++ operator on unsupported type"]),
    ("dp", "pw = &e.fv;", &[]),
    ("dp", "x_s = e.fv;", &["Cannot cast from vector to string"]),
    ("dp", "x_f = e.fi;", &[]),
    ("dp", "x_i = e.fi;", &[]),
    ("dp", "b = (e.fi == e.fi);", &[]),
    ("dp", "e.fi = e.fi;", &[]),
    ("dp", "e.fi += 1;", &[]),
    ("dp", "e.fi++;", &[]),
    ("dp", "--e.fi;", &[]),
    ("dp", "pw = &e.fi;", &[]),
    (
        "dp",
        "x_s = e.fi;",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = v[1];", &[]),
    ("dp", "x_i = v[1];", &[]),
    ("dp", "b = (v[1] == v[1]);", &[]),
    ("dp", "v[1] = v[1];", &[]),
    ("dp", "v[1] += 1;", &[]),
    ("dp", "v[1]++;", &[]),
    ("dp", "--v[1];", &[]),
    (
        "dp",
        "pw = &v[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = v[1];", &["Cannot cast from float to string"]),
    ("dp", "x_f = v[i];", &[]),
    ("dp", "x_i = v[i];", &[]),
    ("dp", "b = (v[i] == v[i]);", &[]),
    ("dp", "v[i] = v[i];", &[]),
    ("dp", "v[i] += 1;", &[]),
    ("dp", "v[i]++;", &[]),
    ("dp", "--v[i];", &[]),
    (
        "dp",
        "pw = &v[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = v[i];", &["Cannot cast from float to string"]),
    ("dp", "x_f = v_y;", &[]),
    ("dp", "x_i = v_y;", &[]),
    ("dp", "b = (v_y == v_y);", &[]),
    ("dp", "v_y = v_y;", &[]),
    ("dp", "v_y += 1;", &[]),
    ("dp", "v_y++;", &[]),
    ("dp", "--v_y;", &[]),
    (
        "dp",
        "pw = &v_y;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = v_y;", &["Cannot cast from float to string"]),
    (
        "dp",
        "x_f = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "x_i = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "b = (farr == farr);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "farr = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "farr += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "farr++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "--farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "pw = &farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "x_s = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    ("dp", "x_f = st;", &["Cannot cast from struct S to float"]),
    ("dp", "x_i = st;", &["Cannot cast from struct S to __int32"]),
    ("dp", "st = st;", &[]),
    ("dp", "st += 1;", &["Cannot cast from float to struct S"]),
    (
        "dp",
        "st++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--st;", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &st;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = st;", &["Cannot cast from struct S to string"]),
    ("dp", "x_f = s4.a[1];", &[]),
    ("dp", "x_i = s4.a[1];", &[]),
    ("dp", "b = (s4.a[1] == s4.a[1]);", &[]),
    ("dp", "s4.a[1] = s4.a[1];", &[]),
    ("dp", "s4.a[1] += 1;", &[]),
    ("dp", "s4.a[1]++;", &[]),
    ("dp", "--s4.a[1];", &[]),
    (
        "dp",
        "pw = &s4.a[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = s4.a[1];",
        &["Cannot cast from float to string"],
    ),
    ("dp", "x_f = s4.a[i];", &[]),
    ("dp", "x_i = s4.a[i];", &[]),
    ("dp", "b = (s4.a[i] == s4.a[i]);", &[]),
    ("dp", "s4.a[i] = s4.a[i];", &[]),
    ("dp", "s4.a[i] += 1;", &[]),
    ("dp", "s4.a[i]++;", &[]),
    ("dp", "--s4.a[i];", &[]),
    (
        "dp",
        "pw = &s4.a[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = s4.a[i];",
        &["Cannot cast from float to string"],
    ),
    (
        "dp",
        "x_f = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "dp",
        "x_i = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "dp",
        "b = (s4.a[4] == s4.a[4]);",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "dp",
        "s4.a[4] = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "dp",
        "s4.a[4] += 1;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "dp",
        "s4.a[4]++;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "dp",
        "--s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "dp",
        "pw = &s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "x_i = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "b = (s4.a[3.5] == s4.a[3.5]);",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "s4.a[3.5] = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "s4.a[3.5] += 1;",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "s4.a[3.5]++;",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "--s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
        ],
    ),
    (
        "dp",
        "pw = &s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Numerical overflow. 3.500000 will be rounded to 3",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = s4.av[i];",
        &["Cannot cast from vector to float"],
    ),
    (
        "dp",
        "x_i = s4.av[i];",
        &["Cannot cast from vector to __int32"],
    ),
    ("dp", "b = (s4.av[i] == s4.av[i]);", &[]),
    ("dp", "s4.av[i] = s4.av[i];", &[]),
    (
        "dp",
        "s4.av[i] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "dp",
        "s4.av[i]++;",
        &["post increment operator not supported with this type"],
    ),
    ("dp", "--s4.av[i];", &["++ operator on unsupported type"]),
    (
        "dp",
        "pw = &s4.av[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = s4.av[i];",
        &["Cannot cast from vector to string"],
    ),
    (
        "dp",
        "x_f = ps4->a[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = ps4->a[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (ps4->a[i] == ps4->a[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps4->a[i] = ps4->a[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps4->a[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps4->a[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--ps4->a[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &ps4->a[i];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = ps4->a[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_i = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "b = (ps4->a[1] == ps4->a[1]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "ps4->a[1] = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps4->a[1] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "ps4->a[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "--ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Making assumptions about pointer sizes was blocked.",
        ],
    ),
    (
        "dp",
        "pw = &ps4->a[1];",
        &["Making assumptions about pointer sizes was blocked."],
    ),
    (
        "dp",
        "x_s = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "dp",
        "x_f = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "x_i = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "b = (s4.a == s4.a);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "s4.a = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "s4.a += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "s4.a++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "--s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "pw = &s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "dp",
        "x_s = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    ("dp", "x_f = t.q;", &[]),
    ("dp", "x_i = t.q;", &[]),
    ("dp", "b = (t.q == t.q);", &[]),
    ("dp", "t.q = t.q;", &[]),
    ("dp", "t.q += 1;", &[]),
    ("dp", "t.q++;", &[]),
    ("dp", "--t.q;", &[]),
    (
        "dp",
        "pw = &t.q;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("dp", "x_s = t.q;", &["Cannot cast from float to string"]),
    ("dp", "x_f = t.r[i];", &[]),
    ("dp", "x_i = t.r[i];", &[]),
    ("dp", "b = (t.r[i] == t.r[i]);", &[]),
    ("dp", "t.r[i] = t.r[i];", &[]),
    ("dp", "t.r[i] += 1;", &[]),
    ("dp", "t.r[i]++;", &[]),
    ("dp", "--t.r[i];", &[]),
    (
        "dp",
        "pw = &t.r[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = t.r[i];",
        &["Implicit cast from __int32 to string"],
    ),
    ("dp", "x_f = t.r[1];", &[]),
    ("dp", "x_i = t.r[1];", &[]),
    ("dp", "b = (t.r[1] == t.r[1]);", &[]),
    ("dp", "t.r[1] = t.r[1];", &[]),
    ("dp", "t.r[1] += 1;", &[]),
    ("dp", "t.r[1]++;", &[]),
    ("dp", "--t.r[1];", &[]),
    (
        "dp",
        "pw = &t.r[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "dp",
        "x_s = t.r[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "kk7",
        "pw = &st.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "st.b++;", &["post increment operator on bitfield"]),
    (
        "kk7",
        "pw = &st.b;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &st.c;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &st.h;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_f = st.f;", &[]),
    (
        "kk7",
        "x_i = st.f;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (st.f == st.f);", &[]),
    ("kk7", "st.f = st.f;", &[]),
    ("kk7", "st.f += 1;", &[]),
    ("kk7", "st.f++;", &[]),
    ("kk7", "--st.f;", &[]),
    (
        "kk7",
        "pw = &st.f;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = st.f;", &["Cannot cast from float to string"]),
    ("kk7", "x_f = st.v;", &["Cannot cast from vector to float"]),
    (
        "kk7",
        "x_i = st.v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("kk7", "b = (st.v == st.v);", &[]),
    ("kk7", "st.v = st.v;", &[]),
    ("kk7", "st.v += 1;", &["Implicit cast from float to vector"]),
    (
        "kk7",
        "st.v++;",
        &["post increment operator not supported with this type"],
    ),
    ("kk7", "--st.v;", &["++ operator on unsupported type"]),
    (
        "kk7",
        "pw = &st.v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = st.v;", &["Cannot cast from vector to string"]),
    (
        "kk7",
        "x_f = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "x_i = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "b = (ps->a == ps->a);",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "ps->a = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "ps->a += 1;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "ps->a++;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "--ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    ("kk7", "pw = &ps->a;", &[]),
    (
        "kk7",
        "x_s = ps->a;",
        &["ptr-to-bitfld has unsupported bit depth"],
    ),
    (
        "kk7",
        "pw = &ps->c;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &ps->h;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (ps->f == ps->f);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps->f = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps->f += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps->f++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &ps->f;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ps->f;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "kk7",
        "x_i = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "kk7",
        "b = (ps->v == ps->v);",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps->v = ps->v;",
        &["Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte"],
    ),
    (
        "kk7",
        "ps->v += 1;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "kk7",
        "ps->v++;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "kk7",
        "--ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "kk7",
        "pw = &ps->v;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ps->v;",
        &[
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "kk7",
        "pw = &sarr[1].a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = sarr[1].v;",
        &["Cannot cast from vector to float"],
    ),
    (
        "kk7",
        "x_i = sarr[1].v;",
        &["Cannot cast from vector to __int32"],
    ),
    ("kk7", "b = (sarr[1].v == sarr[1].v);", &[]),
    ("kk7", "sarr[1].v = sarr[1].v;", &[]),
    (
        "kk7",
        "sarr[1].v += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "kk7",
        "sarr[1].v++;",
        &["post increment operator not supported with this type"],
    ),
    ("kk7", "--sarr[1].v;", &["++ operator on unsupported type"]),
    (
        "kk7",
        "pw = &sarr[1].v;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = sarr[1].v;",
        &["Cannot cast from vector to string"],
    ),
    (
        "kk7",
        "x_f = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
        "x_i = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
        "b = ((*pps)->a == (*pps)->a);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
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
        "kk7",
        "(*pps)->a += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
        "(*pps)->a++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
        "--(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    (
        "kk7",
        "pw = &(*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = (*pps)->a;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "ptr-to-bitfld has unsupported bit depth",
        ],
    ),
    ("kk7", "x_f = farr[2];", &[]),
    (
        "kk7",
        "x_i = farr[2];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (farr[2] == farr[2]);", &[]),
    ("kk7", "farr[2] = farr[2];", &[]),
    ("kk7", "farr[2] += 1;", &[]),
    ("kk7", "farr[2]++;", &[]),
    ("kk7", "--farr[2];", &[]),
    (
        "kk7",
        "pw = &farr[2];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = farr[2];",
        &["Cannot cast from float to string"],
    ),
    (
        "kk7",
        "x_f = farr[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "kk7",
        "x_i = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (farr[i] == farr[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "farr[i] = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "farr[i] += 1;",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "farr[i]++;",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "--farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "pw = &farr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = farr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Cannot cast from float to string",
        ],
    ),
    ("kk7", "x_f = farr[f];", &[]),
    (
        "kk7",
        "x_i = farr[f];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (farr[f] == farr[f]);", &[]),
    ("kk7", "farr[f] = farr[f];", &[]),
    ("kk7", "farr[f] += 1;", &[]),
    ("kk7", "farr[f]++;", &[]),
    ("kk7", "--farr[f];", &[]),
    (
        "kk7",
        "pw = &farr[f];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = farr[f];",
        &["Cannot cast from float to string"],
    ),
    (
        "kk7",
        "x_f = varr[1];",
        &["Cannot cast from vector to float"],
    ),
    (
        "kk7",
        "x_i = varr[1];",
        &["Cannot cast from vector to __int32"],
    ),
    ("kk7", "b = (varr[1] == varr[1]);", &[]),
    ("kk7", "varr[1] = varr[1];", &[]),
    (
        "kk7",
        "varr[1] += 1;",
        &["Implicit cast from float to vector"],
    ),
    (
        "kk7",
        "varr[1]++;",
        &["post increment operator not supported with this type"],
    ),
    ("kk7", "--varr[1];", &["++ operator on unsupported type"]),
    (
        "kk7",
        "pw = &varr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = varr[1];",
        &["Cannot cast from vector to string"],
    ),
    (
        "kk7",
        "pw = &varr[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe return",
        ],
    ),
    (
        "kk7",
        "x_i = iarr[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "kk7",
        "b = (iarr[i] == iarr[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "iarr[i] = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "iarr[i]++;",
        &[
            "itof emulation: denormals are unsafe i",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "--iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "pw = &iarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = iarr[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "kk7",
        "x_f = iarr[1];",
        &["itof emulation: denormals are unsafe iarr"],
    ),
    ("kk7", "x_i = iarr[1];", &[]),
    (
        "kk7",
        "b = (iarr[1] == iarr[1]);",
        &["Result of comparison is constant"],
    ),
    ("kk7", "iarr[1] = iarr[1];", &[]),
    (
        "kk7",
        "iarr[1] += 1;",
        &[
            "itof emulation: denormals are unsafe iarr",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "iarr[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--iarr[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &iarr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = iarr[1];",
        &["Implicit cast from __int32 to string"],
    ),
    (
        "kk7",
        "pw = &carr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "b = (carr[i] == carr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "carr[i] = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "carr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "carr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = carr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "b = (ucarr[i] == ucarr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ucarr[i] = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ucarr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ucarr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ucarr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "b = (harr[i] == harr[i]);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "harr[i] = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "harr[i] += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "harr[i]++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = harr[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &harr[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (p[0] == p[0]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[0] = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[0] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[0]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "pw = &p[0];", &[]),
    (
        "kk7",
        "x_s = p[0];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (p[i] == p[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[i] = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[i] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "p[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &p[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = p[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
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
        "kk7",
        "p[f] = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "p[f] += 1;",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "p[f]++;",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "--p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
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
        "kk7",
        "x_s = p[f];",
        &[
            "ftoi emulation: denormals are unsafe",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (*p == *p);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "*p = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "*p += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--*p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "pw = &*p;", &[]),
    (
        "kk7",
        "x_s = *p;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_i = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "b = (pi[i] == pi[i]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pi[i] = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pi[i]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &pi[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = pi[i];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from __int32 to string",
        ],
    ),
    (
        "kk7",
        "x_f = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to float",
        ],
    ),
    (
        "kk7",
        "x_i = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to __int32",
        ],
    ),
    (
        "kk7",
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
        "kk7",
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
        "kk7",
        "pv[i] += 1;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to vector",
        ],
    ),
    (
        "kk7",
        "pv[i]++;",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "post increment operator not supported with this type",
        ],
    ),
    (
        "kk7",
        "--pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "++ operator on unsupported type",
        ],
    ),
    (
        "kk7",
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
        "kk7",
        "x_s = pv[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"=|LOADP_V\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from vector to string",
        ],
    ),
    (
        "kk7",
        "pw = &pc[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &pc[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "pw = &*pc;", &[]),
    (
        "kk7",
        "pw = &ph[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "pw = &*ph;", &[]),
    (
        "kk7",
        "x_f = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (s[0] == s[0]);",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
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
        "kk7",
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
        "kk7",
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
        "kk7",
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
        "kk7",
        "pw = &s[0];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    (
        "kk7",
        "x_s = s[0];",
        &[
            "Opcode \"<LOADP_C>|LOADP_C\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "pw = &s[i];",
        &["Cannot use addressof operator ('&') on a global. Please use the FTE target."],
    ),
    ("kk7", "x_f = e.fl;", &[]),
    (
        "kk7",
        "x_i = e.fl;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (e.fl == e.fl);", &[]),
    ("kk7", "e.fl = e.fl;", &[]),
    ("kk7", "e.fl += 1;", &[]),
    ("kk7", "e.fl++;", &[]),
    ("kk7", "--e.fl;", &[]),
    ("kk7", "pw = &e.fl;", &[]),
    ("kk7", "x_s = e.fl;", &["Cannot cast from float to string"]),
    ("kk7", "x_f = e.fv;", &["Cannot cast from vector to float"]),
    (
        "kk7",
        "x_i = e.fv;",
        &["Cannot cast from vector to __int32"],
    ),
    ("kk7", "b = (e.fv == e.fv);", &[]),
    ("kk7", "e.fv = e.fv;", &[]),
    ("kk7", "e.fv += 1;", &["Implicit cast from float to vector"]),
    (
        "kk7",
        "e.fv++;",
        &["post increment operator not supported with this type"],
    ),
    ("kk7", "--e.fv;", &["++ operator on unsupported type"]),
    ("kk7", "pw = &e.fv;", &[]),
    ("kk7", "x_s = e.fv;", &["Cannot cast from vector to string"]),
    ("kk7", "x_i = e.fi;", &[]),
    ("kk7", "b = (e.fi == e.fi);", &[]),
    ("kk7", "e.fi = e.fi;", &[]),
    (
        "kk7",
        "e.fi++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--e.fi;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "pw = &e.fi;", &[]),
    (
        "kk7",
        "x_s = e.fi;",
        &["Implicit cast from __int32 to string"],
    ),
    ("kk7", "x_f = v[1];", &[]),
    (
        "kk7",
        "x_i = v[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (v[1] == v[1]);", &[]),
    ("kk7", "v[1] = v[1];", &[]),
    ("kk7", "v[1] += 1;", &[]),
    ("kk7", "v[1]++;", &[]),
    ("kk7", "--v[1];", &[]),
    (
        "kk7",
        "pw = &v[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = v[1];", &["Cannot cast from float to string"]),
    (
        "kk7",
        "x_f = v[i];",
        &["itof emulation: denormals are unsafe i"],
    ),
    (
        "kk7",
        "x_i = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (v[i] == v[i]);",
        &[
            "itof emulation: denormals are unsafe i",
            "itof emulation: denormals are unsafe i",
        ],
    ),
    (
        "kk7",
        "pw = &v[i];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = v[i];",
        &[
            "itof emulation: denormals are unsafe i",
            "Cannot cast from float to string",
        ],
    ),
    ("kk7", "x_f = v_y;", &[]),
    (
        "kk7",
        "x_i = v_y;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (v_y == v_y);", &[]),
    ("kk7", "v_y = v_y;", &[]),
    ("kk7", "v_y += 1;", &[]),
    ("kk7", "v_y++;", &[]),
    ("kk7", "--v_y;", &[]),
    (
        "kk7",
        "pw = &v_y;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = v_y;", &["Cannot cast from float to string"]),
    (
        "kk7",
        "x_f = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "x_i = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "b = (farr == farr);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "farr = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "farr += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "farr++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "--farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "pw = &farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "x_s = farr;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    ("kk7", "x_f = st;", &["Cannot cast from struct S to float"]),
    (
        "kk7",
        "x_i = st;",
        &["Cannot cast from struct S to __int32"],
    ),
    ("kk7", "st = st;", &[]),
    ("kk7", "st += 1;", &["Cannot cast from float to struct S"]),
    (
        "kk7",
        "st++;",
        &["post increment operator not supported with this type"],
    ),
    ("kk7", "--st;", &["++ operator on unsupported type"]),
    (
        "kk7",
        "pw = &st;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = st;", &["Cannot cast from struct S to string"]),
    ("kk7", "x_f = s4.a[1];", &[]),
    (
        "kk7",
        "x_i = s4.a[1];",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (s4.a[1] == s4.a[1]);", &[]),
    ("kk7", "s4.a[1] = s4.a[1];", &[]),
    ("kk7", "s4.a[1] += 1;", &[]),
    ("kk7", "s4.a[1]++;", &[]),
    ("kk7", "--s4.a[1];", &[]),
    (
        "kk7",
        "pw = &s4.a[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = s4.a[1];",
        &["Cannot cast from float to string"],
    ),
    (
        "kk7",
        "pw = &s4.a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "kk7",
        "x_i = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (s4.a[4] == s4.a[4]);",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "kk7",
        "s4.a[4] = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "(constant) array index out of bounds (0 <= 4 < 3)",
        ],
    ),
    (
        "kk7",
        "s4.a[4] += 1;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "kk7",
        "s4.a[4]++;",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "kk7",
        "--s4.a[4];",
        &["(constant) array index out of bounds (0 <= 4 < 3)"],
    ),
    (
        "kk7",
        "pw = &s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = s4.a[4];",
        &[
            "(constant) array index out of bounds (0 <= 4 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "kk7",
        "x_i = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (s4.a[3.5] == s4.a[3.5]);",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "kk7",
        "s4.a[3.5] = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "(constant) array index out of bounds (0 <= 3 < 3)",
        ],
    ),
    (
        "kk7",
        "s4.a[3.5] += 1;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "kk7",
        "s4.a[3.5]++;",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "kk7",
        "--s4.a[3.5];",
        &["(constant) array index out of bounds (0 <= 3 < 3)"],
    ),
    (
        "kk7",
        "pw = &s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = s4.a[3.5];",
        &[
            "(constant) array index out of bounds (0 <= 3 < 3)",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "pw = &s4.av[i];",
        &[
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (ps4->a[i] == ps4->a[i]);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[i] = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[i] += 1;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[i]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "MulInt function not defined: cannot emulate int*int",
            "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ps4->a[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_i = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "b = (ps4->a[1] == ps4->a[1]);",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[1] = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[1] += 1;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a[1]++;",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &ps4->a[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ps4->a[1];",
        &[
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float to string",
        ],
    ),
    (
        "kk7",
        "x_f = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "x_i = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "b = (s4.a == s4.a);",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "s4.a = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "s4.a += 1;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "s4.a++;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "--s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "pw = &s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "x_s = s4.a;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of either '#pragma target fte' or '#pragma flag enable brokenarray'",
        ],
    ),
    (
        "kk7",
        "x_f = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Cannot cast from float* to float",
        ],
    ),
    (
        "kk7",
        "x_i = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to __int32",
        ],
    ),
    (
        "kk7",
        "b = (ps4->a == ps4->a);",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "ps4->a = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "left operand must be an l-value (did you mean ps4[0]?)",
        ],
    ),
    (
        "kk7",
        "pw = &ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = ps4->a;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float* to string",
        ],
    ),
    ("kk7", "x_f = t.q;", &[]),
    (
        "kk7",
        "x_i = t.q;",
        &["ftoi emulation: denormals are unsafe"],
    ),
    ("kk7", "b = (t.q == t.q);", &[]),
    ("kk7", "t.q = t.q;", &[]),
    ("kk7", "t.q += 1;", &[]),
    ("kk7", "t.q++;", &[]),
    ("kk7", "--t.q;", &[]),
    (
        "kk7",
        "pw = &t.q;",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    ("kk7", "x_s = t.q;", &["Cannot cast from float to string"]),
    (
        "kk7",
        "pw = &t.r[i];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_f = t.r[1];",
        &["itof emulation: denormals are unsafe t"],
    ),
    ("kk7", "x_i = t.r[1];", &[]),
    (
        "kk7",
        "b = (t.r[1] == t.r[1]);",
        &["Result of comparison is constant"],
    ),
    ("kk7", "t.r[1] = t.r[1];", &[]),
    (
        "kk7",
        "t.r[1] += 1;",
        &[
            "itof emulation: denormals are unsafe t",
            "ftoi emulation: denormals are unsafe",
        ],
    ),
    (
        "kk7",
        "t.r[1]++;",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "--t.r[1];",
        &[
            "AddInt function not defined: cannot emulate int+int",
            "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "pw = &t.r[1];",
        &[
            "Address-of operator is not supported in this form without extensions. Consider the use of: #pragma target fte",
        ],
    ),
    (
        "kk7",
        "x_s = t.r[1];",
        &["Implicit cast from __int32 to string"],
    ),
];

/// Each case of `cases`, read in a body after [`REFERENCE_DEFS`] on its target.
fn check_references(cases: &[(&str, &str, &[&str])]) {
    for &(target, statement, expected) in cases {
        let pragma = if target == "id" {
            String::new()
        } else {
            format!("#pragma target {target}\n")
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

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_1() {
    check_references(&REFERENCES[0..300]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_2() {
    check_references(&REFERENCES[300..600]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_3() {
    check_references(&REFERENCES[600..900]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_4() {
    check_references(&REFERENCES[900..1200]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_5() {
    check_references(&REFERENCES[1200..1500]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_6() {
    check_references(&REFERENCES[1500..1800]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_7() {
    check_references(&REFERENCES[1800..2100]);
}

// spec FTEF-31.1: each reference read, compared, stored to, stepped and addressed as FTEQCC does
// spec FTEF-6.20: a struct value's member is its struct's definition at the member's word
// spec FTEF-6.21: an 8- or 16-bit integer loaded through its address is an `int` or `uint`
// spec FTEF-31.10: a constant index is the array's definition at that word
// spec FTEF-31.11: the address of a string's character is the fatal error E0361
// spec QCD-4.62: E0361 for the address of a string's character
// spec FTEF-29.10: an array's head steps as a pointer and is no l-value
// spec FTEF-6.24: a struct's member array is indexed at the member's word
#[test]
fn every_reference_is_read_as_fteqcc_reads_it_8() {
    check_references(&REFERENCES[2100..2118]);
}

// spec FTEF-31.2: an element keeps what its base is: a variable's component and an entity's
// `.vector` component store, a constant's and a field's are constants, and a field's components
// are the same symbol
#[test]
fn elements_keep_their_bases() {
    let found = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\n.vector fv; .float ff; vector v; const vector cv = [1, 2, 3]; \
             float *p; float q; float *pp; string s; float i;\nvoid() g = {{ local entity e; \
             local .vector lf; {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    for quiet in [
        "e.fv[1] = 2;",
        "e.fv[1]++;",
        "v[1] = 2;",
        "p[1] = 2;",
        "q = v[0] < v[1];",
        "lf[1] = ff;",
        "local .float lq = lf[1];",
        "pp = &cv[1];",
        "pp = &v[1];",
        "q = e.(fv[1]);",
        "q = e.(lf[1]);",
        "q = s[1] < s[1];",
        "s[1] = 3;",
        "q = lf[i] == lf[i];",
        "q = fv[i] == fv[i];",
    ] {
        assert!(found(quiet).is_empty(), "{quiet}");
    }
    assert_eq!(
        found("cv[1] = 2;"),
        ["Assignment to const", "Assignment to constant cv"]
    );
    assert_eq!(
        found("fv[1] = ff;"),
        ["Assignment to const", "Assignment to constant fv"]
    );
    assert_eq!(found("p[1]();"), ["not a function"]);
    for same in [
        "q = fv[1] == fv[1];",
        "q = v[1] < v[1];",
        "q = v[0] < v[0];",
    ] {
        assert_eq!(found(same), ["Result of comparison is constant"], "{same}");
    }
    assert_eq!(found("cv[1]++;"), ["postincrement: lhs is readonly"]);
    assert_eq!(
        found("lf[1]++;"),
        ["post increment operator not supported with this type"]
    );
    assert_eq!(
        found("pp = &fv[1];"),
        ["Implicit cast from .float* to float*"]
    );
    // An entity's array of fields is read as the array's own definition, with its length
    // before it on hexen2 (FTEF-31.15).
    let fields = diags(
        "#pragma target hexen2\n.float fa[4]; float q; int ii;\n\
         void() g = { local entity e; q = e.fa[ii]; };\n",
    );
    assert_eq!(
        fields,
        [
            itof("ii"),
            d(Code::W0319, "INTERNAL: over-freed refcount to ii")
        ]
    );
    // Under `qccx` a string's index is the string offset, a string.
    let offset = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\n#pragma flag enable qccx\nstring s, s2; float q, i;\n\
             void() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let denormals = "string offsetting emulation: denormals are unsafe";
    assert_eq!(offset("s2 = s[i];"), [denormals]);
    assert_eq!(
        offset("q = s[i];"),
        [denormals, "Implicit cast from string to float"]
    );
}

// spec FTEF-31.16: an array of fields is indexed in elements of the field's type, and a class's
// member array has no length prefix
#[test]
fn arrays_of_fields_count_their_elements() {
    let found = |target: &str, statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target {target}\nentity self; float q; int ii; vector v; .float fa[4]; \
             .vector gv[2];\nclass C : entity {{ float arr[4]; vector varr[2]; \
             nonvirtual void() m = {{ local entity e; {statement} }}; }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    let itof = "itof emulation: denormals are unsafe ii";
    let over_freed = "INTERNAL: over-freed refcount to ii";
    assert_eq!(found("hexen2", "q = this.arr[ii];"), [itof]);
    assert_eq!(found("hexen2", "this.arr[ii] = 1;"), [itof]);
    assert_eq!(found("hexen2", "q = e.fa[ii];"), [itof, over_freed]);
    let scaled = [
        "MulInt function not defined: cannot emulate int*int",
        "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte",
    ];
    for statement in ["v = this.varr[ii];", "v = e.gv[ii];"] {
        for target in ["standard", "hexen2"] {
            let messages = found(target, statement);
            assert_eq!(
                messages.get(..2),
                Some(&scaled.map(String::from)[..]),
                "{target} {statement}"
            );
            assert_eq!(messages.len(), 3, "{target} {statement}");
        }
    }
    assert!(found("fte", "v = this.varr[ii]; v = e.gv[ii];").is_empty());
}
