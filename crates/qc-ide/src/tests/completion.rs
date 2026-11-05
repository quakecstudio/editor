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
    let text = "float g;\n.float hp;\n\
                void(float p) f = { local float early; /*here*/ local float late; };\n\
                void() h = { local float other; };\n";
    let got = labels(text, "/*here*/", 0);
    assert_eq!(got, ["g", "hp", "f", "p", "early"]);
}

// spec QC-12.1: at file level only the globals are visible
#[test]
fn completion_at_file_level() {
    let text = "float g;\nvoid(float p) f = { local float l; };\n/*here*/\n";
    assert_eq!(labels(text, "/*here*/", 0), ["g", "f"]);
}

// After `.` on an entity only fields are offered, since a field access names a field
// (QC-16.6; the statement is the compiler's to pin).
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
    assert_eq!(items[0].detail, "float(entity e)");
    assert!(host.completions(&path("missing.qc"), 0).is_empty());
}

// spec QC-12.5: a global declared after the cursor, in the same file or a later one, does not
// resolve there, so it is not offered
#[test]
fn completion_leaves_out_later_globals() {
    let first = "float early;\nvoid() f = { /*here*/ };\nfloat late;\n";
    let second = "float later_file;\n";
    let mut host = super::host(&super::ProjectSpec {
        files: &[("a.qc", first), ("b.qc", second)],
        ..super::ProjectSpec::default()
    });
    let labels: Vec<String> = host
        .completions(&path("a.qc"), nth(first, "/*here*/", 0))
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert_eq!(labels, ["early", "f"]);
    // At the start of the second file, everything of the first is declared.
    let at_start: Vec<String> = host
        .completions(&path("b.qc"), 0)
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert_eq!(at_start, ["early", "f", "late"]);
}

/// The labels offered at the `n`-th `needle` of the one-file FTEQCC project `text`.
fn fteqcc_labels(text: &str, needle: &str, n: usize) -> Vec<String> {
    let mut host = super::host(&super::ProjectSpec {
        files: &[("a.qc", text)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..super::ProjectSpec::default()
    });
    host.completions(&path("a.qc"), nth(text, needle, n))
        .into_iter()
        .map(|c| c.label)
        .collect()
}

// spec FTE-9.1: after `.` on a vector its members are its components, `x`, `y` and `z`, in an
// FTEQCC module; in a vanilla module a vector has no members, so nothing is offered
#[test]
fn completion_after_a_vector_offers_its_components() {
    let text = "vector v;\n.float hp;\nvoid() f = { v.x = 1; };\n";
    assert_eq!(fteqcc_labels(text, "x = 1", 0), ["x", "y", "z"]);
    let local = "void() f = { local vector dir; dir.y = 2; };\n";
    assert_eq!(fteqcc_labels(local, "y = 2", 0), ["x", "y", "z"]);
    assert!(labels(text, "x = 1", 0).is_empty());
    // While the member is still being typed: `v.` and nothing after it.
    let typing = "vector v;\nvoid() f = { v.";
    let mut host = super::host(&super::ProjectSpec {
        files: &[("a.qc", typing)],
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..super::ProjectSpec::default()
    });
    let at_end: Vec<String> = host
        .completions(&path("a.qc"), typing.len())
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert_eq!(at_end, ["x", "y", "z"]);
}

// spec FTEF-17.3: after `this.` in a class method the members of its class and then of its
// parents come first, then the entity fields; after `self.` the fields
#[test]
fn completion_after_this_offers_the_class_members() {
    let text = "\
.float health;
class Base
{
\tfloat m_flBase;
\tvirtual void Spawned(void);
};
class Thing : Base
{
\tfloat m_flTime;
\tvirtual void Spawned(void);
};
entity self;
void Base::Spawned(void) { };
void Thing::Spawned(void) { this.m_flTime = 1; };
void() user = { self.health = 1; };
";
    let got = fteqcc_labels(text, "m_flTime = 1", 0);
    let at = |name: &str| got.iter().position(|label| label == name);
    let (time, base, health) = (at("m_flTime"), at("m_flBase"), at("health"));
    assert!(
        time.is_some() && base.is_some() && health.is_some(),
        "{got:?}"
    );
    assert!(
        time < base && base < health,
        "own, then the parent's, then fields: {got:?}"
    );
    let entity = fteqcc_labels(text, "health = 1", 0);
    assert!(entity.contains(&"health".to_owned()), "{entity:?}");
    assert!(!entity.contains(&"m_flTime".to_owned()), "{entity:?}");
}

// spec FTEF-17.3: the base is read from the file as written, so comments, indentation and a
// macro before the cursor do not move it; a chain reads each member's type, a typedef stands
// for its type, and a vector member's components the compiler makes are not offered
#[test]
fn completion_reads_the_base_through_real_code() {
    let text = "\
// A header comment.
#define TWO 2
typedef vector vec3_t;
class Thing
{
\tvector m_vecDir;
\tfloat m_flTime;
\tvirtual void Spawned(void);
};
vec3_t dir;
void Thing::Spawned(void)
{
    /* indented, after a macro */ local float n = TWO;
    this.m_flTime = n;
    this.m_vecDir.z = 1;
    dir.y = 2;
};
";
    let members = fteqcc_labels(text, "m_flTime = n", 0);
    assert!(members.contains(&"m_vecDir".to_owned()), "{members:?}");
    assert!(
        !members.iter().any(|m| m.starts_with("m_vecDir_")),
        "no generated components: {members:?}"
    );
    assert_eq!(fteqcc_labels(text, "z = 1", 0), ["x", "y", "z"], "a chain");
    assert_eq!(
        fteqcc_labels(text, "y = 2", 0),
        ["x", "y", "z"],
        "a typedef"
    );
    let paren = "vector v;\nvoid() f = { (v).x = 1; };\n";
    assert_eq!(
        fteqcc_labels(paren, "x = 1", 0),
        ["x", "y", "z"],
        "in parentheses"
    );
}

// spec FTEF-6.13: a struct's members are offered after `.`, and nothing else; a float has none
#[test]
fn completion_after_a_struct_offers_its_members() {
    let text = "struct S { float a; vector b; };\nS s;\n.float hp;\nfloat f;\n\
                void() g = { s.a = 1; f.x = 2; };\n";
    assert_eq!(fteqcc_labels(text, "a = 1", 0), ["a", "b"]);
    assert!(fteqcc_labels(text, "x = 2", 0).is_empty());
    // FTEF-4.1: after `->` a pointer's target; a vector takes `.`, not `->`.
    let pointer = "struct S { float a; vector b; };\nS *p;\nvector v;\n\
                   void() g = { p->a = 1; v->x = 2; };\n";
    assert_eq!(fteqcc_labels(pointer, "a = 1", 0), ["a", "b"]);
    assert!(fteqcc_labels(pointer, "x = 2", 0).is_empty());
}
