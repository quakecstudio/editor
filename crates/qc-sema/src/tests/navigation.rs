// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation tests: go-to-definition and find-references (GOAL-1).

use super::{fte_index_of, index_of, nth_offset};
use crate::symbol::{DefId, FileId};

// spec QC-12.5: go-to-definition from a use jumps to the definition's name.
#[test]
fn goto_definition_from_use() {
    let source = "float health;\nvoid() f = { health = 1; };";
    let index = index_of(source);
    // The use of `health` is its second occurrence.
    let use_offset = nth_offset(source, "health", 1);
    let (_, location) = index
        .definition_at(FileId(0), use_offset)
        .expect("a definition");
    // It jumps to the declaration, the first occurrence.
    assert_eq!(location.span.start, nth_offset(source, "health", 0));
}

// spec QC-12.2: go-to-definition from a definition's own name resolves to itself.
#[test]
fn goto_definition_from_declaration() {
    let source = "float health;";
    let index = index_of(source);
    let decl_offset = nth_offset(source, "health", 0);
    let (def, location) = index
        .definition_at(FileId(0), decl_offset)
        .expect("a definition");
    assert_eq!(index.def(def).unwrap().name, "health");
    assert_eq!(location.span.start, decl_offset);
}

// spec QC-12.5: find-references lists every use of a definition.
#[test]
fn find_references_lists_all_uses() {
    let source = "float n;\nvoid() f = { n = 1; n = n + 1; };";
    let index = index_of(source);
    let decl_offset = nth_offset(source, "n", 0);
    let (def, _) = index
        .definition_at(FileId(0), decl_offset)
        .expect("a definition");
    // Uses of n in the body: `n = 1`, `n = n + 1` -> three uses.
    let without_decl = index.references(def, false);
    assert_eq!(without_decl.len(), 3);
    // With the declaration, four locations, the declaration first.
    let with_decl = index.references(def, true);
    assert_eq!(with_decl.len(), 4);
    assert_eq!(with_decl[0].span.start, decl_offset);
}

// spec QC-12.4: find-references does not confuse same-named locals of two functions.
#[test]
fn find_references_respects_function_scope() {
    let source = "void() f = { local float t; t = 1; };\nvoid() g = { local float t; t = 2; };";
    let index = index_of(source);
    // The `t` of f: the name in its `local float t;`. Search from after "float " to avoid
    // the `t` inside the word "float".
    let f_local_decl = nth_offset(source, "local float t", 0) + "local float ".len();
    let (f_def, _) = index.definition_at(FileId(0), f_local_decl).expect("f's t");
    let f_refs = index.references(f_def, false);
    // Only f's body uses t once (`t = 1`), which must be before g's local declaration.
    assert_eq!(f_refs.len(), 1);
    assert!(f_refs[0].span.start < nth_offset(source, "local float t", 1));
}

// spec QC-12.3: a use of a name that a local reused resolves to the global.
#[test]
fn use_of_reused_global_resolves_to_global() {
    let source = "float x;\nvoid() f = { local float x; x = 1; };";
    let index = index_of(source);
    let decl_offset = nth_offset(source, "x", 0);
    let (global_def, _) = index
        .definition_at(FileId(0), decl_offset)
        .expect("the global x");
    // The use `x = 1` resolves to the same global.
    let use_offset = nth_offset(source, "x", 2);
    let (used_def, _) = index
        .definition_at(FileId(0), use_offset)
        .expect("the use of x");
    assert_eq!(used_def, global_def);
}

// A position that is on no symbol returns nothing, without panicking.
#[test]
fn position_on_no_symbol() {
    let source = "float health;";
    let index = index_of(source);
    // Offset 0 is on `float`, not a name; and far past the end is nothing.
    assert!(index.definition_at(FileId(0), source.len() + 100).is_none());
    assert!(
        index
            .references_at(FileId(0), source.len() + 100, true)
            .is_empty()
    );
}

// spec QC-12.5: a name covers every byte from its first to its last, and not the byte after
// it, nor a keyword before it, nor a position in another file of the module.
#[test]
fn a_name_covers_exactly_its_own_bytes() {
    let source = "float health;\nvoid() f = { health = 1; };";
    let index = super::module_index_of(&[source, "float other;"]);
    let decl = nth_offset(source, "health", 0);
    let use_ = nth_offset(source, "health", 1);
    for offset in [decl, decl + 3, decl + 5, use_ + 3, use_ + 5] {
        let (def, _) = index.definition_at(FileId(0), offset).expect("on health");
        assert_eq!(index.def(def).unwrap().name, "health", "at {offset}");
    }
    for offset in [0, decl + 6, use_ + 6] {
        assert!(
            index.definition_at(FileId(0), offset).is_none(),
            "at {offset}"
        );
    }
    assert!(index.definition_at(FileId(1), use_).is_none());
    let (other, _) = index.definition_at(FileId(1), 6).expect("on other");
    assert_eq!(index.def(other).unwrap().name, "other");
    assert!(index.references(DefId(99), true).is_empty());
}

// spec FTEF-10.2: an accessor body's `this` and `value` have empty spans at the member's
// name, which cover only that position, so a position elsewhere is on no symbol.
#[test]
fn an_empty_span_covers_only_its_position() {
    let source = "accessor strbuf : float { get float at = { return this; }; };\n";
    let index = super::fte_index_of(source);
    let this = index
        .defs
        .iter()
        .position(|d| d.name == "this")
        .expect("the this parameter");
    let at = index.defs[this].location.span;
    assert_eq!(
        (at.start, at.end),
        (nth_offset(source, " at ", 0) + 1, at.start)
    );
    for offset in [0, nth_offset(source, "return", 0), source.len() - 1] {
        assert!(
            index.definition_at(FileId(0), offset).is_none(),
            "at {offset}"
        );
    }
}

// spec QC-12.5: references_at from a use gathers the definition's references.
#[test]
fn references_at_from_use() {
    let source = "float n;\nvoid() f = { n = 1; };";
    let index = index_of(source);
    let use_offset = nth_offset(source, "n", 1);
    let refs = index.references_at(FileId(0), use_offset, true);
    assert_eq!(refs.len(), 2);
}

// spec QC-11.5: a function's body after its prototype declares the same definition again, so
// go-to-definition from the body's name jumps to the prototype and find-references lists the
// body with the declarations.
#[test]
fn a_redeclaration_names_its_definition() {
    let source = "void() f;\nvoid() g = { f(); };\nvoid() f = { };";
    let index = index_of(source);
    let prototype = nth_offset(source, "f", 0);
    let body = nth_offset(source, "f", 2);
    let (def, location) = index.definition_at(FileId(0), body).expect("the prototype");
    assert_eq!(location.span.start, prototype);
    let starts: Vec<usize> = index
        .references(def, true)
        .iter()
        .map(|l| l.span.start)
        .collect();
    assert_eq!(starts, [prototype, body, nth_offset(source, "f", 1)]);
    // Without the declarations, only the call.
    assert_eq!(index.references(def, false).len(), 1);
}

// spec QC-11.5: a second declaration with another type is reported and still names the first
// definition.
#[test]
fn a_mismatched_redeclaration_names_its_definition() {
    let source = "float x;\nstring x;";
    let index = index_of(source);
    let second = nth_offset(source, "x", 1);
    let (def, location) = index.definition_at(FileId(0), second).expect("the first x");
    assert_eq!(location.span.start, nth_offset(source, "x", 0));
    assert_eq!(index.redeclarations.len(), 1);
    assert_eq!(index.redeclarations[0].def, def);
}

// spec FTEF-19.7: a declaration that replaces an unused definition (W0211) is a new
// definition, not a redeclaration of the one it replaces.
#[test]
fn a_replacing_declaration_is_no_redeclaration() {
    let source =
        "void() anglemod = #6;\nvoid() anglemod = {};\nfloat(float a) anglemod = #7;\n";
    let index = fte_index_of(source);
    let third = nth_offset(source, "anglemod", 2);
    let (def, location) = index.definition_at(FileId(0), third).expect("the new anglemod");
    assert_eq!(location.span.start, third);
    // The body redeclares the builtin; the third declaration redeclares nothing.
    assert_eq!(index.redeclarations.len(), 1);
    assert_ne!(index.redeclarations[0].def, def);
    assert_eq!(index.redeclarations[0].location.span.start, nth_offset(source, "anglemod", 1));
}
