// SPDX-License-Identifier: GPL-3.0-or-later

//! Stores and members through pointers, and the 8- and 16-bit integers, as FTEQCC compiles them
//! (`docs/spec/dialect-fteqcc.md` FTEF-2.12, FTEF-6.18, FTEF-6.19, FTEF-31.7). Each case's
//! diagnostics are the pinned FTEQCC's (2026-10-27).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId::new(0);

const DEFS: &str = "struct t { float f; vector v; }; struct q { float f; __int8 b; __int8 c; \
                    __uint16 h; }; struct s { int a : 3; int b : 5; };\nt *pt; t st; q *pq; s \
                    *ps; float *p; __int64 *pl; __int64 l; __int8 *pc; __int8 c8; vector w; \
                    int i, c, x; float f; string str;\n";

fn diags(target: &str, statement: &str) -> Vec<(Code, String)> {
    let source = format!("{target}\n{DEFS}void() g = {{ {statement} }};\n");
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.into_bytes());
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

fn id(statement: &str) -> Vec<(Code, String)> {
    diags("", statement)
}

fn fte(statement: &str) -> Vec<(Code, String)> {
    diags("#pragma target fte", statement)
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

fn helper(name: &str, what: &str) -> (Code, String) {
    d(
        Code::W0235,
        &format!("{name} function not defined: cannot emulate {what}"),
    )
}

fn refused(spelling: &str, op: &str) -> (Code, String) {
    d(
        Code::E0320,
        &format!(
            "Opcode \"{spelling}|{op}\" not valid for target. Consider the use of: #pragma target \
             fte"
        ),
    )
}

fn memgetval() -> [(Code, String); 2] {
    [
        helper("memgetval", "OP_LOADP_*").clone(),
        refused("=", "LOADP_I"),
    ]
}

fn and() -> [(Code, String); 2] {
    [helper("BitandInt", "int&int"), refused("&", "BITAND_I")]
}

fn or() -> [(Code, String); 2] {
    [helper("BitorInt", "int|int"), refused("|", "BITOR_I")]
}

fn mul() -> [(Code, String); 2] {
    [helper("MulInt", "int*int"), refused("*", "MUL_I")]
}

fn add() -> [(Code, String); 2] {
    [helper("AddInt", "int+int"), refused("+", "ADD_I")]
}

// spec FTEF-6.18: a store of 8 or 16 bits the target lacks is the word read, the value masked
// and shifted into it and the word stored; constants fold
// spec QCD-4.26: E0320 for each opcode the emulation emits and the target refuses
#[test]
fn a_narrow_store_is_emulated_on_its_word() {
    let word = memgetval();
    assert_eq!(
        id("*pc = c;"),
        [&word[..], &and()[..], &and()[..], &or()[..]].concat()
    );
    // The value 3 masked and shifted folds, its byte cleared and or-ed in.
    assert_eq!(id("*pc = 3;"), [&word[..], &and()[..], &or()[..]].concat());
    // A value of 0 leaves nothing to or in.
    assert_eq!(id("*pc = 0;"), [&word[..], &and()[..]].concat());
    // A float is made the integer the store takes, "Numerical overflow" for a fraction.
    assert_eq!(
        id("*pc = 3.5;")[0],
        d(
            Code::W0234,
            "Numerical overflow. 3.500000 will be rounded to 3"
        )
    );
    assert_eq!(fte("*pc = c;"), []);
    assert_eq!(fte("pc[i] = c;"), []);
}

// spec FTEF-6.18: through a pointer a bitfield stores by its bits, 8 or 16, else "ptr-to-bitfld
// has unsupported bit depth", one that does not start its word first "has a bit offset"
// spec QCD-4.57: E0353 for a bitfield of another width through a pointer
// spec QCD-4.58: E0357 "left operand (NAME) has a bit offset"
#[test]
fn a_bitfield_through_a_pointer_stores_by_its_bits() {
    let depth = d(Code::E0353, "ptr-to-bitfld has unsupported bit depth");
    assert_eq!(fte("ps->a = 1;"), std::slice::from_ref(&depth));
    assert_eq!(
        fte("ps->b = 1;"),
        [
            d(Code::E0357, "left operand (ps) has a bit offset"),
            depth.clone()
        ]
    );
    assert_eq!(id("ps->a += 1;"), [depth]);
}

// spec FTEF-6.18: a struct through a pointer at an index is stored word by word, each word's
// offset added to the index; a 64-bit value by two words when the target has no
// `OP_STOREP_I64`
// spec FTEF-31.7: the index is scaled by the size of what the pointer points at
#[test]
fn a_struct_through_a_pointer_stores_word_by_word() {
    assert_eq!(id("pt[i] = st;"), [&mul()[..], &add()[..]].concat());
    assert_eq!(id("pl[i] = l;"), [&mul()[..], &add()[..]].concat());
    assert_eq!(id("*pt = st;"), []);
    assert_eq!(id("*pl = l;"), []);
    assert_eq!(fte("pt[i] = st;"), []);
}

// spec FTEF-6.18: a target whose stores take no offset adds the index to the pointer by
// `OP_ADD_PIW`, which `undefwordsize` blocks
// spec QCD-4.32: E0326 "Making assumptions about pointer sizes was blocked."
#[test]
fn a_store_without_an_offset_adds_the_index_to_the_pointer() {
    let blocked = d(
        Code::E0326,
        "Making assumptions about pointer sizes was blocked.",
    );
    let dp = |statement: &str| diags("#pragma target dp_20240101", statement);
    assert_eq!(dp("p[i] = 1;"), std::slice::from_ref(&blocked));
    assert_eq!(dp("*pl = l;"), std::slice::from_ref(&blocked));
    assert_eq!(dp("pl[i] = l;"), [blocked.clone(), blocked.clone()]);
    assert_eq!(dp("pt[i] = st;"), [blocked.clone(), blocked.clone()]);
    assert_eq!(dp("*pt = st;"), [blocked]);
}

// spec FTEF-31.7: a pointer's index is multiplied by the size in words of what it points at
#[test]
fn a_pointers_index_is_scaled_by_its_pointee() {
    let vector = "local vector *pv; w = pv[i];";
    assert_eq!(
        id(vector),
        [&mul()[..], &[refused("=", "LOADP_V")][..]].concat()
    );
    assert_eq!(id("f = p[i];"), memgetval_f());
    assert_eq!(id("w = pt[2].v;"), [refused("=", "LOADP_V")]);
}

fn memgetval_f() -> Vec<(Code, String)> {
    vec![helper("memgetval", "OP_LOADP_*"), refused("=", "LOADP_F")]
}

// spec FTEF-6.19: a member through a pointer is at an index: its offset, added to an index before
// it by `OP_ADD_I`, or `OP_ADD_F` on a target without it; a narrower member's index counted in
// its units
#[test]
fn a_member_through_a_pointer_is_at_its_offset() {
    // F313 makes the emulation's warnings errors.
    let itof = d(Code::E0125, "itof emulation: denormals are unsafe temp_0");
    let ftoi = d(Code::E0125, "ftoi emulation: denormals are unsafe");
    assert_eq!(id("pt[i].v = w;"), [&mul()[..], &[itof, ftoi][..]].concat());
    let blocked = d(
        Code::E0326,
        "Making assumptions about pointer sizes was blocked.",
    );
    assert_eq!(diags("#pragma target dp_20240101", "pt->v = w;"), [blocked]);
    assert_eq!(fte("pq[i].h = 1;"), []);
    assert_eq!(id("pq[i].h = 1;")[..4], [&mul()[..], &mul()[..]].concat());
}

// spec FTEF-2.12: an 8- or 16-bit integer is cast as its parent and named by its own name, read
// by extending its bits, and no bitfield's base
// spec QCD-7.62: W0220 for an implicit cast
#[test]
fn a_narrow_integer_is_its_parent_cast_and_read() {
    let cast = |from: &str, to: &str| d(Code::W0220, &format!("Implicit cast from {from} to {to}"));
    assert_eq!(fte("c8 = str;"), [cast("string", "__int8")]);
    assert_eq!(fte("*pc = str;"), [cast("string", "__int8")]);
    assert_eq!(fte("str = c8;"), [cast("__int32", "string")]);
    assert_eq!(fte("c8 = pc;"), [cast("__int8*", "__int8")]);
    assert_eq!(fte("x = c8;"), []);
    // Read, it is extended by `OP_BITEXTEND_I`, two shifts on the id target.
    assert_eq!(id("x = c8;")[..2], mul());
}

// spec FTEF-2.12: an element of a narrow array at a constant index is a bitfield, extended when
// read and packed when stored; at another index read through its address
// spec QCD-4.39: E0331 for the address the id target cannot take
#[test]
fn a_narrow_arrays_element() {
    let narrow = |target: &str, statement: &str| {
        diags(target, &format!("local __int8 arr8[8]; {statement}"))
    };
    let shifts = [
        &mul()[..],
        &[d(
            Code::E0125,
            "itof emulation: denormals are unsafe temp_0",
        )][..],
    ]
    .concat();
    assert_eq!(narrow("", "f = arr8[1];")[..3], shifts[..]);
    assert_eq!(narrow("", "arr8[1] = 3;"), [&and()[..], &or()[..]].concat());
    assert_eq!(narrow("", "arr8[1] = 0;"), and());
    assert_eq!(
        narrow("#pragma target fte", "arr8[1] = 3; f = arr8[1];"),
        []
    );
    assert_eq!(
        narrow("", "f = arr8[i];").first().map(|(code, _)| *code),
        Some(Code::E0331)
    );
}

// spec FTEF-2.12: a narrow local is initialised packed into its word, a char array from a string
// a character then zeros to each element
#[test]
fn a_narrow_local_is_packed() {
    assert_eq!(id("local __int8 l = 3;"), [&and()[..], &or()[..]].concat());
    assert_eq!(
        id("local __uint16 l = c;"),
        [&and()[..], &and()[..], &or()[..]].concat()
    );
    // 'a' and 'b' are or-ed in, the terminator only cleared.
    assert_eq!(
        id("local __int8 s[3] = \"ab\";"),
        [&and()[..], &or()[..], &and()[..], &or()[..], &and()[..]].concat()
    );
    assert_eq!(fte("local __int8 l = 3; local __int8 s[] = \"ab\";"), []);
}

// spec FTEF-2.12: a narrow field has no load, and its store's index is no number
// spec QCD-4.59: E0358 "QCC_PR_ExpandField: invalid field type TYPE"
// spec QCD-4.60: E0359 "Unable to evaluate to numeric constant"
#[test]
fn a_narrow_field() {
    let field = |target: &str, statement: &str| {
        let source =
            format!("{target}\n.__int8 fc; entity e; int x;\nvoid() g = {{ {statement} }};\n");
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FILE, PathBuf::from("t.qc"), source.into_bytes());
        let analysis = db.analyze_module(&ModuleInput {
            compiled: vec![FILE],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        });
        analysis
            .diagnostics()
            .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
            .collect::<Vec<_>>()
    };
    let invalid = d(Code::E0358, "QCC_PR_ExpandField: invalid field type __int8");
    let fte = "#pragma target fte";
    for statement in ["x = e.fc;", "e.fc += 1;", "e.fc++;"] {
        assert_eq!(
            field(fte, statement),
            std::slice::from_ref(&invalid),
            "{statement}"
        );
    }
    let unable = d(Code::E0359, "Unable to evaluate to numeric constant");
    assert_eq!(field(fte, "e.fc = 1;"), std::slice::from_ref(&unable));
    // On the id target the value is packed, then stored through the field's address.
    assert_eq!(
        field("", "e.fc = 1;"),
        [
            &[unable][..],
            &and()[..],
            &or()[..],
            &memgetval()[..],
            &and()[..],
            &and()[..],
            &or()[..],
        ]
        .concat()
    );
}

// spec FTEF-27.14: a pointer's prefix step is its pointee's size, `--` negated with the warning
#[test]
fn a_pointers_prefix_step_is_its_size() {
    let truncated = |unsigned: &str, negated: &str| {
        d(
            Code::W0234,
            &format!("Constant int operand {unsigned} will be truncated to {negated}"),
        )
    };
    let defs = "vector *pv; void *pw;";
    assert_eq!(fte(&format!("{defs} ++pv; ++p; ++pw;")), []);
    assert_eq!(fte("--p;"), [truncated("4294967295", "-1")]);
    assert_eq!(
        fte(&format!("{defs} --pv;")),
        [truncated("4294967293", "-3")]
    );
    assert_eq!(
        fte(&format!("{defs} --pw;")),
        [truncated("4294967295", "-1")]
    );
    assert_eq!(id("++p;"), add());
    let [helper, refusal] = add();
    assert_eq!(id("--p;"), [truncated("4294967295", "-1"), helper, refusal]);
}

// spec FTEF-2.12: an 8- or 16-bit integer is read as its word, `__int32` or `__uint32`, a name
// and an element alike
// spec FTEF-19.31: a local's value is cast to its type, an array's element included
#[test]
fn narrow_reads_and_elements_are_cast_as_their_words() {
    let defs = "float farr[4]; int iarr[2]; __int8 n8; __uint8 u8; __uint8 arr[4];";
    let cases = [
        (
            "local string q = farr[1];",
            "Cannot cast from float to string",
        ),
        (
            "local string q = farr[i];",
            "Cannot cast from float to string",
        ),
        (
            "local entity q = iarr[1];",
            "Implicit cast from __int32 to entity",
        ),
        (
            "local string q = n8;",
            "Implicit cast from __int32 to string",
        ),
        (
            "local string q = u8;",
            "Implicit cast from __uint32 to string",
        ),
        (
            "local string q = arr[1];",
            "Implicit cast from __uint32 to string",
        ),
        (
            "local string q = arr[i];",
            "Implicit cast from __uint32 to string",
        ),
    ];
    for (statement, message) in cases {
        let found = diags(&format!("#pragma target fte\n{defs}"), statement);
        assert_eq!(
            found.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(),
            [message],
            "{statement}"
        );
    }
    assert!(
        diags(
            &format!("#pragma target fte\n{defs}"),
            "local float q = farr[1];"
        )
        .is_empty()
    );
}

// spec FTEF-31.14: a string's character index, and the value stored to it, are supplied as
// FTEQCC converts them, a type that does not convert refused
// spec FTEF-6.14: a struct of two words is read through a pointer by `OP_LOADP_I` twice
// spec FTEF-31.2: a `.vector` field's component at a constant index past 2 is fatal, and
// another field type takes no index
#[test]
fn characters_vector_fields_and_two_word_structs() {
    let defs = "#pragma target fte\nstring txt; entity e; .vector fv; .float ff; vector v; \
                float farr[4];";
    let found = |statement: &str| -> Vec<String> {
        diags(defs, statement).into_iter().map(|(_, m)| m).collect()
    };
    let mismatch = |got: &str| format!("Implicit type mismatch. Needed integer, got {got}.");
    assert_eq!(found("local float q = txt[\"a\"];"), [mismatch("string")]);
    assert_eq!(found("local float q = txt[v];"), [mismatch("vector")]);
    assert_eq!(found("txt[e] = 1;"), [mismatch("entity")]);
    assert_eq!(
        found("txt[1] = \"b\";"),
        ["Cannot cast from string to float"]
    );
    assert!(found("txt[1] = 1; local float q = txt[1];").is_empty());
    assert!(found("local .float q = fv[1];").is_empty());
    assert_eq!(
        found("local .float q = fv[3];"),
        ["(.vector) array index out of bounds"]
    );
    assert_eq!(
        found("local string q = fv[1];"),
        ["Cannot cast from .float to string"]
    );
    assert_eq!(
        found("local .float q = ff[1];"),
        [
            "Missing semicolon at end of definition",
            "Statement does not do anything"
        ]
    );
    let unevaluated = "Unable to evaluate to numeric constant";
    assert_eq!(
        found("local float q = farr[\"a\"];"),
        [unevaluated, unevaluated]
    );
    assert_eq!(
        found("local float q = v[\"a\"];"),
        [unevaluated, unevaluated]
    );
    let two = "struct t2 { float a, b; }; t2 *p2; t2 x2;";
    assert_eq!(
        diags(two, "x2 = *p2;"),
        [(
            Code::E0320,
            "Opcode \"=|LOADP_I\" not valid for target. Consider the use of: #pragma target fte"
                .to_owned()
        )]
    );
}

// spec FTEF-31.15: an array's element read through `OP_FETCH_GBL_*` with an index that is no
// float frees the index twice; a local array has the length prefix too
// spec FTEF-31.2: an element at an index that is no constant is read as the target allows, the
// index converted for it
#[test]
fn array_reads_by_target() {
    let defs = "float farr[4]; vector varr[3]; vector v; float qf; vector wv; float fi; int ii; \
                __int8 narr[4]; int n;";
    let found = |target: &str, statement: &str| -> Vec<String> {
        diags(&format!("#pragma target {target}\n{defs}"), statement)
            .into_iter()
            .map(|(_, m)| m)
            .collect()
    };
    let itof = |name: &str| format!("itof emulation: denormals are unsafe {name}");
    let freed = |name: &str| format!("INTERNAL: over-freed refcount to {name}");
    let address = "Address-of operator is not supported in this form without extensions. \
                   Consider the use of: #pragma target fte";
    assert_eq!(found("hexen2", "qf = farr[ii];"), [itof("ii"), freed("ii")]);
    assert_eq!(
        found("hexen2", "local float la[3]; qf = la[ii];"),
        [itof("ii"), freed("ii")]
    );
    assert_eq!(
        found("hexen2", "local int li = 1; qf = farr[li];"),
        [itof("li"), freed("li")]
    );
    assert_eq!(found("id", "qf = farr[ii];"), [itof("ii")]);
    assert_eq!(found("id", "qf = v[ii];"), [itof("ii")]);
    assert_eq!(found("hexen2", "qf = v[ii];"), [itof("ii")]);
    assert_eq!(
        found("id", "n = narr[fi];"),
        ["ftoi emulation: denormals are unsafe", address]
    );
    assert_eq!(found("dp", "n = narr[ii];"), [address]);
    for target in ["id", "hexen2", "dp", "fte"] {
        for statement in [
            "qf = farr[fi];",
            "wv = varr[fi];",
            "local float la[3]; qf = la[fi];",
            "qf = v[fi];",
        ] {
            assert!(found(target, statement).is_empty(), "{target}: {statement}");
        }
    }
    for statement in [
        "qf = farr[ii];",
        "wv = varr[ii];",
        "qf = v[ii];",
        "n = narr[ii];",
    ] {
        assert!(found("fte", statement).is_empty(), "fte: {statement}");
        assert!(
            found("dp", statement).iter().all(|m| m == address),
            "dp: {statement}"
        );
    }
}

// spec FTEF-6.15: a bitfield is read as its own type, `__uint` for an unsigned one
// spec FTEF-2.12: an 8- or 16-bit integer, an element of an array of them included, is read as
// its word and stored as its own type
// spec FTEF-31.1: a string's character is a float at a float index, else an `__int32`
#[test]
fn reads_and_stores_of_bitfields_characters_and_narrow_integers() {
    let defs = "#pragma target fte\nstruct B { __uint b:4; int a:4; }; B bf; string txt; \
                __uint8 narr[4]; __uint8 un8;";
    let found = |statement: &str| -> Vec<String> {
        diags(defs, statement).into_iter().map(|(_, m)| m).collect()
    };
    for (statement, message) in [
        (
            "local string q = bf.b;",
            "Implicit cast from __uint to string",
        ),
        (
            "local entity q = bf.a;",
            "Implicit cast from __int32 to entity",
        ),
        (
            "local string q = txt[1];",
            "Cannot cast from float to string",
        ),
        (
            "local string q = txt[1.5];",
            "Cannot cast from float to string",
        ),
        (
            "local string q = txt[i];",
            "Implicit cast from __int32 to string",
        ),
        (
            "local string q = narr[1];",
            "Implicit cast from __uint32 to string",
        ),
        (
            "local string q = un8;",
            "Implicit cast from __uint32 to string",
        ),
        ("narr[1] = \"a\";", "Implicit cast from string to __uint8"),
        ("narr[i] = \"a\";", "Implicit cast from string to __uint8"),
        (
            "narr[1] = 1.5;",
            "Numerical overflow. 1.500000 will be rounded to 1",
        ),
    ] {
        assert_eq!(found(statement), [message], "{statement}");
    }
}
