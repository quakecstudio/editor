// SPDX-License-Identifier: GPL-3.0-or-later

//! Arrays of more than one dimension, `docs/spec/dialect-fteqcc.md` FTEF-5.6 to FTEF-5.8, on the
//! standard target. Each opcode list and record list was read from the pinned FTEQCC
//! (fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `-Tid`) on 2026-11-22, compiling the
//! same program.

use qc_progs::ProgsFile;
use qc_progs::opcode::Opcode::{
    self, AddF, Call1, Call2, DivF, Done, MulF, StoreF, StoreS, StoreV,
};

use super::{body, fte_progs, function, global, immediate};

/// The program the tests read: arrays of two and three dimensions, global and local.
const PROGRAM: &str = "void(string e) error = #10;\n\
    float g3[2][3];\n\
    float g3i[2][3] = { {1, 2, 3}, {4, 5, 6} };\n\
    vector gv[2][2];\n\
    float m_k;\n\
    void() m_const = { g3[1][2] = 5; g3[0][1] = 6; m_k = g3[1][0]; };\n\
    void(float i, float j) m_read = { m_k = g3[i][j]; };\n\
    void(float i, float j) m_store = { g3[i][j] = m_k; };\n\
    void(float j) m_rowconst = { m_k = g3[1][j]; };\n\
    void(float i) m_colconst = { m_k = g3[i][2]; };\n\
    void() m_init = { m_k = g3i[1][1]; };\n\
    void(float i) m_vec = { vector t; gv[i][1] = '1 2 3'; t = gv[1][i]; m_k = t_z; };\n\
    void() m_len = { m_k = g3.length + g3[0].length; };\n\
    void(float i) m_local = { float l[2][2]; l[1][1] = 4; l[i][0] = 3; m_k = l[i][1]; };\n\
    void(float i) m_linit = { float l[2][2] = { {1, 2}, {3, 4} }; m_k = l[i][1]; };\n\
    struct P2 { float a; float b; };\n\
    void(float i) m_sarr = { struct P2 ps[2] = { {1, 2}, {3, 4} }; m_k = ps[i].b; };\n\
    void(float i) m_varr = { vector vs[2] = { {1, 2, 3}, '4 5 6' }; vector t = vs[i]; m_k = t_y; };\n\
    float g333[2][2][2];\n\
    void(float i, float j, float k) m_three = { g333[i][j][k] = 7; g333[1][0][1] = 8; \
    m_k = g333[0].length + g333[0][1].length; };\n";

/// The opcodes of the function `name`'s body.
fn ops(progs: &ProgsFile, name: &str) -> Vec<u32> {
    body(progs, name).iter().map(|s| s.op).collect()
}

/// The numbers of `ops`.
fn numbers(ops: &[Opcode]) -> Vec<u32> {
    ops.iter().map(|op| u32::from(op.number())).collect()
}

/// The first parameter slot, `OFS_PARM0`, which a getter's index is written to.
const PARM0: i32 = 4;

// spec FTEF-5.6: a subscript of an element of an array of two or more dimensions indexes the
// array type's one member: each index scaled by the size of what it selects and added to the
// one before it, constants folded and a constant 0 left out, the element the whole array's,
// read and stored by its getter and setter; a vector element a word at a time; `.length` of a
// row its own size
#[test]
fn an_element_of_two_dimensions_is_indexed_through_its_row() {
    let p = fte_progs(PROGRAM);
    assert_eq!(ops(&p, "m_const"), numbers(&[StoreF, StoreF, StoreF, Done]));
    let constant = body(&p, "m_const");
    let g3 = global(&p, "g3");
    assert_eq!(
        (constant[0].b, constant[1].b, constant[2].a),
        (g3 + 5, g3 + 1, g3 + 3)
    );
    assert_eq!(
        ops(&p, "m_read"),
        numbers(&[MulF, AddF, Call1, StoreF, Done])
    );
    let read = body(&p, "m_read");
    let (i, j, three) = (global(&p, "i"), global(&p, "j"), immediate(&p, 3.0));
    assert_eq!((read[0].a, read[0].b), (i, three));
    assert_eq!((read[1].a, read[1].b, read[1].c), (read[0].c, j, PARM0));
    assert_eq!(read[2].a, global(&p, "ArrayGet*g3"));
    assert_eq!(
        ops(&p, "m_store"),
        numbers(&[MulF, AddF, StoreF, Call2, Done])
    );
    assert_eq!(body(&p, "m_store")[3].a, global(&p, "ArraySet*g3"));
    assert_eq!(ops(&p, "m_rowconst"), numbers(&[AddF, Call1, StoreF, Done]));
    assert_eq!(body(&p, "m_rowconst")[0].a, three);
    assert_eq!(
        ops(&p, "m_colconst"),
        numbers(&[MulF, AddF, Call1, StoreF, Done])
    );
    assert_eq!(
        ops(&p, "m_vec"),
        numbers(&[
            MulF, AddF, StoreV, Call2, MulF, AddF, StoreF, Call1, StoreF, AddF, Call1, StoreF,
            AddF, Call1, StoreF, StoreV, StoreF, Done
        ])
    );
    // `g3.length + g3[0].length` folds to 5.
    assert_eq!(ops(&p, "m_len"), numbers(&[StoreF, Done]));
    assert_eq!(body(&p, "m_len")[0].a, immediate(&p, 5.0));
    assert_eq!(
        ops(&p, "m_local"),
        numbers(&[StoreF, MulF, StoreF, Call2, MulF, AddF, Call1, StoreF, Done])
    );
    // `l[i][0]` adds no 0.
    assert_eq!(body(&p, "m_local")[1].c, PARM0);
    assert_eq!(
        ops(&p, "m_three"),
        numbers(&[MulF, MulF, AddF, AddF, StoreF, Call2, StoreF, StoreF, Done])
    );
    let three_dims = body(&p, "m_three");
    assert_eq!(three_dims[6].b, global(&p, "g333") + 5);
    assert_eq!(three_dims[7].a, immediate(&p, 4.0));
    assert!(function(&p, "ArrayGet*l").first_statement > 0);
}

// spec FTEF-5.7: the braces of an array type are its member's elements, a global's words and a
// local's stores, as are an array of structs' and of vectors' element braces
#[test]
fn an_array_types_braces_are_its_rows() {
    let p = fte_progs(PROGRAM);
    let g3i = usize::try_from(global(&p, "g3i")).unwrap();
    let words: Vec<f32> = p.globals[g3i..g3i + 6].iter().map(|w| w.as_f32()).collect();
    assert_eq!(words, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(ops(&p, "m_init"), numbers(&[StoreF, Done]));
    assert_eq!(body(&p, "m_init")[0].a, global(&p, "g3i") + 4);
    assert_eq!(
        ops(&p, "m_linit"),
        numbers(&[
            StoreF, StoreF, StoreF, StoreF, MulF, AddF, Call1, StoreF, Done
        ])
    );
    let linit = body(&p, "m_linit");
    let stored: Vec<i32> = linit[..4].iter().map(|s| s.b - linit[0].b).collect();
    assert_eq!(stored, [0, 1, 2, 3]);
    assert_eq!(
        ops(&p, "m_sarr"),
        numbers(&[
            StoreF, StoreF, StoreF, StoreF, MulF, AddF, Call1, StoreF, Done
        ])
    );
    assert_eq!(
        ops(&p, "m_varr"),
        numbers(&[
            StoreF, StoreF, StoreF, StoreV, MulF, DivF, Call1, StoreV, StoreF, Done
        ])
    );
    assert!(!ops(&p, "m_varr").contains(&u32::from(StoreS.number())));
}

/// Each global record of `progs` whose name starts with one of `roots` and then `[` or `.`:
/// its type number, whether it is saved, its offset from the first of its root and its name.
fn records(progs: &ProgsFile, roots: &[&str]) -> Vec<(u32, bool, i32, String)> {
    let named: Vec<(u32, i32, String)> = progs
        .global_defs
        .iter()
        .map(|d| {
            (
                d.type_bits,
                i32::try_from(d.ofs).unwrap(),
                super::string(progs, d.s_name),
            )
        })
        .filter(|(_, _, name)| {
            roots.iter().any(|root| {
                name.strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('[') || rest.starts_with('.'))
            })
        })
        .collect();
    named
        .iter()
        .map(|(bits, ofs, name)| {
            let root = roots.iter().find(|root| name.starts_with(**root)).unwrap();
            let first = named
                .iter()
                .filter(|(_, _, n)| n.starts_with(*root))
                .map(|(_, o, _)| *o)
                .min()
                .unwrap();
            (bits & 0x7fff, bits & 0x8000 != 0, ofs - first, name.clone())
        })
        .collect()
}

// spec FTEF-5.8: an element of an array type is a union record, never saved, then its member's
// elements, `NAME[i][j]`, a vector's `NAME[i].(null)[j]`; each element type its own array
// type, a size read as any integer constant
// spec FTE-13.19: a struct's member that is an array writes each element, one of one element
// too, and a member that is a struct its own members, with no record for the member itself
#[test]
fn an_array_types_records_are_its_rows_and_elements() {
    let p = fte_progs(&format!(
        "{PROGRAM}struct In {{ float x; vector y; }};\n\
         struct S1 {{ float a[2]; In in; float g[2][2]; }};\nS1 s1;\n\
         struct S2 {{ float a[1]; float g[1][2]; float b; }};\nS2 s2;\n\
         float hx[2][0x3];\n_Bool bb[2][3];\nint ii[2][3];\n\
         void() u = {{ vector w = gv[0][0]; m_k = s1.a[1] + s1.in.x + s1.g[1][1] + w_x; }};\n"
    ));
    let union = 15;
    let float = 2;
    let vector = 3;
    let row = |at, name: &str| (union, false, at, name.to_owned());
    let saved = |ty, at, name: &str| (ty, true, at, name.to_owned());
    assert_eq!(
        records(&p, &["g3"]),
        [
            row(0, "g3[0]"),
            saved(float, 0, "g3[0][0]"),
            saved(float, 1, "g3[0][1]"),
            saved(float, 2, "g3[0][2]"),
            row(3, "g3[1]"),
            saved(float, 3, "g3[1][0]"),
            saved(float, 4, "g3[1][1]"),
            saved(float, 5, "g3[1][2]"),
        ]
    );
    assert_eq!(
        records(&p, &["gv"]),
        [
            row(0, "gv[0]"),
            saved(vector, 0, "gv[0].(null)[0]"),
            saved(vector, 3, "gv[0].(null)[1]"),
            row(6, "gv[1]"),
            saved(vector, 6, "gv[1].(null)[0]"),
            saved(vector, 9, "gv[1].(null)[1]"),
        ]
    );
    assert_eq!(
        records(&p, &["s1"]),
        [
            saved(float, 0, "s1.a[0]"),
            saved(float, 1, "s1.a[1]"),
            saved(float, 2, "s1.in.x"),
            saved(vector, 3, "s1.in.y"),
            row(6, "s1.g[0]"),
            saved(float, 6, "s1.g[0][0]"),
            saved(float, 7, "s1.g[0][1]"),
            row(8, "s1.g[1]"),
            saved(float, 8, "s1.g[1][0]"),
            saved(float, 9, "s1.g[1][1]"),
        ]
    );
    assert_eq!(
        records(&p, &["s2"]),
        [
            saved(float, 0, "s2.a[0]"),
            row(1, "s2.g[0]"),
            saved(float, 1, "s2.g[0][0]"),
            saved(float, 2, "s2.g[0][1]"),
            saved(float, 3, "s2.b"),
        ]
    );
    assert!(records(&p, &["hx"]).contains(&saved(float, 5, "hx[1][2]")));
    let boolean = 19;
    let integer = 8;
    assert!(records(&p, &["bb"]).contains(&saved(boolean, 0, "bb[0][0]")));
    assert!(records(&p, &["ii"]).contains(&saved(integer, 0, "ii[0][0]")));
}

/// The numbers of FTE's opcodes `ops`.
fn fte_numbers(ops: &[qc_progs::Op]) -> Vec<u32> {
    ops.iter()
        .map(|op| u32::try_from(op.number()).unwrap())
        .collect()
}

// spec FTEF-31.32: on a target with the opcodes, a struct's member array, of one dimension or
// two, is the definition's own words from the member's, which its index counts from the
// definition's start: the address of the struct's first word and the index plus the member's
// word, each dimension's index checked by its own size
#[test]
fn a_member_array_is_indexed_from_its_struct() {
    use qc_progs::Op::{AddI, Boundcheck, ConvFtoi, Done, Globaladdress, LoadaF, MulI, StorepF};
    let p = fte_progs(
        "#pragma target fte\nstruct S { float x; float a[3]; float g[2][2]; };\nS s;\nfloat k;\n\
         void(float i) f = { s.a[i] = 3; };\n\
         void(float i, float j) h = { k = s.g[i][j]; };\n",
    );
    assert_eq!(
        ops(&p, "f"),
        fte_numbers(&[ConvFtoi, Boundcheck, AddI, Globaladdress, StorepF, Done])
    );
    let f = body(&p, "f");
    assert_eq!(f[3].a, global(&p, "s"));
    assert_eq!(p.globals[usize::try_from(f[2].a).unwrap()].bits(), 1);
    assert_eq!(
        ops(&p, "h"),
        fte_numbers(&[
            ConvFtoi, Boundcheck, MulI, AddI, ConvFtoi, Boundcheck, AddI, LoadaF, Done
        ])
    );
    let h = body(&p, "h");
    assert_eq!((h[1].b, h[5].b), (2, 2));
    assert_eq!(p.globals[usize::try_from(h[3].a).unwrap()].bits(), 4);
    assert_eq!(h[7].a, global(&p, "s"));
}
