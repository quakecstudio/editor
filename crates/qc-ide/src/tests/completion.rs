// SPDX-License-Identifier: GPL-3.0-or-later

//! Completion.

use super::{nth, one, path};
use crate::DefKind;

/// The labels offered at the `n`-th `needle` of the one-file project `text`.
fn labels(text: &str, needle: &str, n: usize) -> Vec<String> {
    let mut host = one(text);
    host.completions(&path("a.qc"), nth(text, needle, n))
        .into_iter()
        .map(|c| c.label)
        .collect()
}

// spec QC-12.1: in a body, the globals and the function's own parameters and earlier locals
// are visible, not another function's locals or a later local
#[test]
fn completion_offers_what_is_in_scope() {
    let text = "float g;\n.float hp;\nvoid(float p) f = { local float early; /*here*/ local float late; };\n\
                void() h = { local float other; };\n";
    let got = labels(text, "/*here*/", 0);
    assert_eq!(got, ["g", "hp", "f", "p", "early", "h"]);
}

// spec QC-12.1: at file level only the globals are visible
#[test]
fn completion_at_file_level() {
    let text = "float g;\nvoid(float p) f = { local float l; };\n/*here*/\n";
    assert_eq!(labels(text, "/*here*/", 0), ["g", "f"]);
}

// spec QC-16.6: after `.` only fields are offered
#[test]
fn completion_after_a_member_access() {
    let text = "entity e;\n.float hp;\n.vector org;\nvoid() f = { e.hp = 1; };\n";
    let got = labels(text, "hp =", 0);
    assert_eq!(got, ["hp", "org", "org_x", "org_y", "org_z"]);
    // Before the `.` the entity is being typed, so every global is offered.
    let before_dot = labels(text, ".hp =", 0);
    assert!(before_dot.contains(&"e".to_owned()), "{before_dot:?}");
}

// spec QC-11.1: each item carries its kind and type
#[test]
fn completion_items_have_kind_and_type() {
    let text = "float(entity e) heal;\n/*here*/\n";
    let mut host = one(text);
    let items = host.completions(&path("a.qc"), nth(text, "/*here*/", 0));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, DefKind::Function);
    assert_eq!(items[0].detail, "float(entity)");
    assert!(host.completions(&path("missing.qc"), 0).is_empty());
}
