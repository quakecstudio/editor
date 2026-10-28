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
// spec QCD-4.61: E0359 "Unable to evaluate to numeric constant" for such an index
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
