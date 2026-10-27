// SPDX-License-Identifier: GPL-3.0-or-later

//! Name uses outside function bodies and in FTEQCC member access (P2-34): a file-level
//! initialiser (FTE-10.2), a parameter's default value (FTE-18.2) and the selector of a
//! member access (FTE-9, FTE-13, FTE-14) are references, and unknown when they resolve to
//! nothing.

use qc_diag::Code;

use super::fte_index_of;
use crate::resolve::ModuleIndex;
use crate::symbol::DefId;

/// The definition named `name`, the first if there are several.
fn def_named(index: &ModuleIndex, name: &str) -> DefId {
    DefId(index.defs.iter().position(|d| d.name == name).unwrap())
}

/// The number of references to the definition named `name`.
fn refs_to(index: &ModuleIndex, name: &str) -> usize {
    index.references(def_named(index, name), false).len()
}

/// The names reported as unknown values, in order.
fn unknown(index: &ModuleIndex) -> Vec<String> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code == Code::E0301)
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

// spec FTE-10.2: a `const` initialised from another names it, so the use is a reference at
// the name in the initialiser.
#[test]
fn a_global_initialiser_references_a_known_name() {
    let source = "const float A = 1;\nconst float B = A;\n";
    let index = fte_index_of(source);
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    let a = def_named(&index, "A");
    let uses = index.references(a, false);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].span.start, source.rfind('A').unwrap());
}

// spec FTE-10.2: a name in a global initialiser that names nothing is an unknown value.
#[test]
fn a_global_initialiser_reports_an_unknown_name() {
    let index = fte_index_of("const float B = NOPE + 1;\n");
    assert_eq!(unknown(&index), ["Unknown value \"NOPE\""]);
}

// spec FTE-10.2: an initialiser is resolved where it is, so a name declared after it is not
// yet known; an aggregate's elements are uses too (FTE-13.1).
#[test]
fn a_global_initialiser_sees_only_earlier_names() {
    let index = fte_index_of("float A[] = { B, C };\nfloat B;\n");
    assert_eq!(
        unknown(&index),
        ["Unknown value \"B\"", "Unknown value \"C\""]
    );
}

// spec FTE-18.2: a parameter's default value names a definition, a reference to it.
#[test]
fn a_default_value_references_a_known_name() {
    let index = fte_index_of(
        "float() SUB_True = { return 1; };\n\
         void(float(entity e) predicate = (float(entity e))SUB_True) f;\n\
         void g(float x = SUB_True) { };\n",
    );
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert_eq!(refs_to(&index, "SUB_True"), 2);
}

// spec FTE-18.2: a default value that names nothing is an unknown value.
#[test]
fn a_default_value_reports_an_unknown_name() {
    let index = fte_index_of("void(float a = missing_default) f;\n");
    assert_eq!(unknown(&index), ["Unknown value \"missing_default\""]);
}

// spec FTE-9.1: the selector of an entity field access is a reference to the field, in a
// body and through `->` (FTE-19.2).
#[test]
fn a_member_selector_references_a_known_field() {
    let index =
        fte_index_of(".float health;\nentity e;\nvoid() f = { e.health = 1; e->health = 2; };\n");
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert_eq!(refs_to(&index, "health"), 2);
}

// spec FTEF-24.6: an entity's member no definition has is "Unknown field" (the pinned FTEQCC,
// 2026-10-27)
#[test]
fn a_member_selector_reports_an_unknown_name() {
    let index = fte_index_of("entity e;\nvoid() f = { e.nofield = 1; };\n");
    let errors: Vec<&str> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(errors, ["Unknown field \"nofield\" in class \"entity\""]);
}

// spec FTE-9.1: `.x`, `.y` and `.z` of a vector name no definition, so they are neither
// references nor unknown, even beside a field of the same name.
#[test]
fn vector_members_are_not_references() {
    let index = fte_index_of("vector v;\nvoid() f = { v.x = v.y + v.z; };\n");
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert!(index.refs.iter().all(|r| index.defs[r.def.0].name == "v"));
}

// spec FTE-13.3: `.length` of an array names no definition.
#[test]
fn length_is_not_a_reference() {
    let index = fte_index_of("float A[] = { 1, 2 };\nconst float N = A.length - 1;\n");
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert_eq!(refs_to(&index, "A"), 1);
}

// spec FTE-14.2: a field of a struct the module defines is a member, not a reference, in a
// body and in an initialiser.
#[test]
fn struct_fields_are_not_references() {
    let index = fte_index_of(
        "typedef struct { string mdl; float frame; } poseInfo;\n\
         poseInfo poses[] = { { \"a\", 1 } };\n\
         float first = poses[0].frame;\n\
         void() f = { local string m = poses[0].mdl; };\n",
    );
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert_eq!(refs_to(&index, "poses"), 2);
}

// spec FTE-21.2: a local of field type selects as the field it holds, so the selector is a
// reference to the local; a name that is a value but not a field is not a reference.
#[test]
fn a_field_typed_local_selector_is_a_reference() {
    let index = fte_index_of(
        ".float health;\nentity e;\nfloat plain;\n\
         void() f = { local .float fld = health; e.fld = 1; e.plain = 2; };\n",
    );
    assert!(unknown(&index).is_empty(), "{:?}", unknown(&index));
    assert_eq!(refs_to(&index, "fld"), 1);
    assert_eq!(refs_to(&index, "plain"), 0);
}
