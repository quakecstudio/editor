// SPDX-License-Identifier: GPL-3.0-or-later

//! What the index records of arrays, structs, named types and enum values for a compiler.

use super::fte_index_of as index;
use crate::resolve::{MemberShape, ModuleIndex};
use crate::symbol::DefId;
use crate::ty::{SimpleType, Type};

/// The definition named `name`.
fn def(index: &ModuleIndex, name: &str) -> DefId {
    let at = index.defs.iter().position(|d| d.name == name).unwrap();
    DefId::new(at)
}

fn named(name: &str) -> Type {
    Type::Named(name.into())
}

const FLOAT: Type = Type::Simple(SimpleType::Float);

// spec FTEF-19.8: an array's size is recorded, a written one or its initialiser's count; no
// array has none
#[test]
fn array_sizes_are_recorded() {
    let i = index("float a[3];\nstring s[] = { \"x\", \"y\" };\nfloat f;\n");
    assert_eq!(i.arrays.get(&def(&i, "a")), Some(&3));
    assert_eq!(i.arrays.get(&def(&i, "s")), Some(&2));
    assert_eq!(i.arrays.get(&def(&i, "f")), None);
}

// spec FTEF-6.13: a struct's words and each member's word, a parent's members first, a static
// member and a bitfield left out
#[test]
fn struct_layouts_are_recorded() {
    let i = index(
        "typedef struct { string m; vector v; float f; } P;\nP p;\n\
         struct base { float a; };\nstruct child : base { float b; static float s; int bits : 3; };\n\
         struct child c;\n",
    );
    let p = &i.structs[&named("P")];
    assert_eq!(p.words, 5);
    let member = |name: &str, ty: Type, word: u32| MemberShape {
        name: name.to_owned(),
        ty,
        word,
        count: 1,
    };
    assert_eq!(
        p.members,
        [
            member("m", Type::Simple(SimpleType::String), 0),
            member("v", Type::Simple(SimpleType::Vector), 1),
            member("f", FLOAT, 4),
        ]
    );
    let c = &i.structs[&named("struct child")];
    assert_eq!(c.words, 3);
    assert_eq!(c.members, [member("a", FLOAT, 0), member("b", FLOAT, 1)]);
}

// spec FTEF-6.13: a member array's elements are recorded, and a struct a member is of too
#[test]
fn member_arrays_and_nested_structs_are_recorded() {
    let i = index(
        "typedef struct { float x[2]; } In;\ntypedef struct { In i; float y; } Out;\nOut o;\n",
    );
    let out = &i.structs[&named("Out")];
    assert_eq!(out.words, 3);
    assert_eq!(out.members[0].word, 0);
    assert_eq!(out.members[1].word, 2);
    let inner = &i.structs[&named("In")];
    assert_eq!(inner.members[0].count, 2);
}

// spec FTEF-8.6: a named enum's type stands for its base type; FTEF-7.2: a typedef's for its
// type; a struct's name stands for nothing else
#[test]
fn named_types_are_aliased_to_their_types() {
    let i = index(
        "enum E { A };\nenum F : string { B = \"b\" };\ntypedef float T;\n\
         typedef struct { float f; } S;\nE e;\nF g;\nT t;\nS s;\nfloat plain;\n",
    );
    assert_eq!(i.aliases.get(&named("E")), Some(&FLOAT));
    assert_eq!(
        i.aliases.get(&named("F")),
        Some(&Type::Simple(SimpleType::String))
    );
    assert_eq!(i.aliases.get(&named("T")), Some(&FLOAT));
    assert!(!i.aliases.contains_key(&named("S")) || i.structs.contains_key(&named("S")));
    assert!(!i.aliases.contains_key(&FLOAT));
}

// spec FTEF-19.10: each enum item's value is recorded, a float's bits or an integer's
#[test]
fn enum_values_are_recorded() {
    let i = index("enum { A, B = 4 };\nenum : int { C, D };\n");
    let words = |name: &str| i.initial_values[&def(&i, name)].words().to_vec();
    assert_eq!(words("A"), [0.0_f32.to_bits()]);
    assert_eq!(words("B"), [4.0_f32.to_bits()]);
    assert_eq!(words("D"), [1]);
}

// spec FTEF-19.3: an initialised global is a constant unless `var` makes it a variable
#[test]
fn constants_are_recorded() {
    let i = index("float a = 1;\nvar float b = 1;\nfloat c;\nenum { E };\n");
    assert!(i.constants.contains(&def(&i, "a")));
    assert!(!i.constants.contains(&def(&i, "b")));
    assert!(!i.constants.contains(&def(&i, "c")));
    assert!(i.constants.contains(&def(&i, "E")));
}

// spec FTEF-6.13: a bitfield and a narrow integer are left out of a struct's members, even at
// the start of a word
#[test]
fn bitfields_and_narrow_members_are_left_out() {
    let i = index(
        "typedef struct { int b : 3; float f; } B;\nB bb;\n\
         typedef struct { __int8 c; float g; } N;\nN nn;\n",
    );
    let names = |ty: &str| -> Vec<String> {
        i.structs[&named(ty)]
            .members
            .iter()
            .map(|m| m.name.clone())
            .collect()
    };
    assert_eq!(names("B"), ["f"]);
    assert_eq!(names("N"), ["g"]);
}

// spec FTEF-32.14: a call of constants FTEQCC folds by its name is the folded constant, so a
// global initialised with it is a constant with no statement at global scope
#[test]
fn a_folded_builtin_call_initialises_a_global() {
    let i = index(
        "float(float f) floor = #1;\nfloat(float a, float b) pow = #2;\n\
         float(float a, float b) mod = #3;\nfloat(float a, float b) bitshift = #4;\n\
         float(float a, float b) min = #5;\n\
         static const float m = floor(7 / 2);\nstatic const float p = pow(2, 10);\n\
         static const float r = mod(7.5, 2);\nstatic const float s = bitshift(256, -4);\n\
         static const float n = min(3, -1);\n",
    );
    assert!(i.diagnostics.is_empty(), "{:?}", i.diagnostics);
    let value = |name: &str| f32::from_bits(i.initial_values[&def(&i, name)].words()[0]);
    assert_eq!(value("m"), 3.0);
    assert_eq!(value("p"), 1024.0);
    assert_eq!(value("r"), 1.0);
    assert_eq!(value("s"), 16.0);
    assert_eq!(value("n"), -1.0);
}
