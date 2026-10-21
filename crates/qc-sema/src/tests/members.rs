// SPDX-License-Identifier: GPL-3.0-or-later

//! The members of FTEQCC classes (`docs/spec/dialect-fteqcc.md`, FTEF-9, FTEF-10, FTEF-16.4,
//! FTEF-17): how a class body's labels, constructors, inline bodies and accessor members are
//! read, and which class a member access is resolved in when two classes have a member of the
//! same name.

use qc_diag::Code;

use super::fte_index_of;
use crate::resolve::ModuleIndex;
use crate::symbol::{DefId, DefKind};

/// The messages of an index's errors.
fn messages(index: &ModuleIndex) -> Vec<String> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

/// The definition named `name` that is a member of the class named `class`.
fn member(index: &ModuleIndex, class: &str, name: &str) -> DefId {
    let class = index
        .defs
        .iter()
        .position(|d| d.name == class && d.member_of.is_none())
        .map(DefId)
        .expect("the class");
    index
        .defs
        .iter()
        .position(|d| d.name == name && d.member_of == Some(class))
        .map(DefId)
        .expect("the member")
}

/// The definition that the reference covering the last byte of the `n`-th occurrence of
/// `needle` in `source` resolves to.
fn target_of(index: &ModuleIndex, source: &str, needle: &str, n: usize) -> Option<DefId> {
    let start = last_byte(source, needle, n);
    index
        .refs
        .iter()
        .find(|r| r.location.span.start <= start && start < r.location.span.end)
        .map(|r| r.def)
}

/// The offset of the last byte of the `n`-th occurrence of `needle` in `source`.
fn last_byte(source: &str, needle: &str, n: usize) -> usize {
    let (at, _) = source.match_indices(needle).nth(n).expect("the needle");
    at.checked_add(needle.len())
        .and_then(|end| end.checked_sub(1))
        .expect("a non-empty needle")
}

/// Two classes with a member `m` each, `A` first, so a member access whose base's class is
/// not known resolves to `A`'s by name (FTEF-17.3); every access below whose base is a `B`
/// must find `B`'s. `C::inner` and `C::Get()` are of class `A` while the globals `inner` and
/// `Get` are of class `B`.
const TWO_CLASSES: &str = "\
entity self;
class A { float m; };
class B { float m; nonvirtual void Touch(void); };
class C { A inner; static A Get(void); };
B b;
B *pb;
B inner;
B Get(void) { return b; };
B make_b(void) { return b; };
void B::Touch(void) { this.m = 0; };
void() user = {
\tb.m = 1;
\t((B)self).m = 2;
\tmake_b().m = 3;
\tpb->m = 4;
\tC::inner.m = 5;
\tC::Get().m = 6;
};
";

// spec FTEF-17.3: the member after `.` is the member of the class of the base when that is
// known from a name's declared type, `this`, a cast in parentheses, a call's return type or a
// pointer to a class.
#[test]
fn a_member_access_resolves_in_the_class_of_its_base() {
    let index = fte_index_of(TWO_CLASSES);
    assert_eq!(messages(&index), Vec::<String>::new());
    let b_m = member(&index, "B", "m");
    for needle in ["this.m", "b.m", "self).m", "make_b().m", "pb->m"] {
        assert_eq!(
            target_of(&index, TWO_CLASSES, needle, 0),
            Some(b_m),
            "{needle}"
        );
    }
}

// spec FTEF-17.3: a scoped base, `Class::name` or `Class::name()`, is not the global of the
// same name: its class is not taken from that global's type.
#[test]
fn a_scoped_base_is_not_the_global_of_its_name() {
    let index = fte_index_of(TWO_CLASSES);
    let a_m = member(&index, "A", "m");
    assert_eq!(
        target_of(&index, TWO_CLASSES, "inner.m", 0),
        Some(a_m),
        "C::inner.m"
    );
    assert_eq!(
        target_of(&index, TWO_CLASSES, "Get().m", 0),
        Some(a_m),
        "C::Get().m"
    );
}

// spec FTEF-16.4: `super::member` is the member of the parent of the function's class, and a
// scope that is no class and names nothing is "Unknown value".
#[test]
fn super_names_the_parent_member() {
    let source = "\
class P { virtual void Go(void); };
class Q : P { virtual void Go(void); };
void P::Go(void) { };
void Q::Go(void) { super::Go(); };
void() f = { Nope::thing(); };
";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), ["Unknown value \"Nope::thing\""]);
    let parent = member(&index, "P", "Go");
    assert_eq!(target_of(&index, source, "super::Go", 0), Some(parent));
}

// spec FTEF-9.2: `virtual:` makes the member functions after it virtual and `nonvirtual:`
// ends that; a member's own `nonvirtual` or `static` overrides the label, and a member
// function with no qualifier and no label is not virtual.
#[test]
fn access_labels_set_the_default_member_kind() {
    let index = fte_index_of(
        "class K {\n\
         \tvirtual: void V(void); nonvirtual void X(void); static void S(void);\n\
         \tpublic: void Pub(void);\n\
         \tnonvirtual: void N(void);\n\
         };\n\
         class L { void Plain(void); };\n",
    );
    let kind = |class: &str, name: &str| index.defs[member(&index, class, name).0].kind;
    assert_eq!(kind("K", "V"), DefKind::Field);
    assert_eq!(kind("K", "X"), DefKind::Function);
    assert_eq!(kind("K", "S"), DefKind::Function);
    assert_eq!(
        kind("K", "Pub"),
        DefKind::Field,
        "`public:` keeps `virtual:`"
    );
    assert_eq!(kind("K", "N"), DefKind::Function);
    assert_eq!(kind("L", "Plain"), DefKind::Function);
}

// spec FTEF-9.4: `Name(void);` with no return type in class `Name` is its constructor, a
// member named after the class at that name; a member function returning the class is no
// constructor.
#[test]
fn a_constructor_is_a_member_named_after_its_class() {
    let source = "class C { C(void); Other(void); };\nvoid C::C(void) { };\n";
    let index = fte_index_of(source);
    let ctor = member(&index, "C", "C");
    let at = index.defs[ctor.0].location.span;
    assert_eq!(source.get(at.range()), Some("C"));
    assert_eq!(at.start, source.find("C(void)").unwrap());
    assert_eq!(target_of(&index, source, "C::C", 0), Some(ctor));
    assert_eq!(
        index.defs.iter().filter(|d| d.member_of.is_some()).count(),
        1,
        "`Other(void);` names no member"
    );
    let factory = fte_index_of("class D { D() clone; };\n");
    let named_d = factory.defs.iter().filter(|d| d.name == "D").count();
    assert_eq!(named_d, 1, "only the class itself is named D");
}

// spec FTEF-9.4: a member function's body written in the class is resolved in the class: its
// names are the class's members, and a name that is none is "Unknown value".
#[test]
fn an_inline_member_body_resolves_in_its_class() {
    let source = "class M { float count; nonvirtual void Bump(void) = { count = count + 1; \
                  nope = 2; }; };\n";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), ["Unknown value \"nope\""]);
    let count = member(&index, "M", "count");
    assert_eq!(index.refs.iter().filter(|r| r.def == count).count(), 2);
}

// spec FTEF-10.3: a `get` or `set` member of a class is an accessor member: its name may be
// selected on an instance, and its body is a function `Class::get_name` with `this` and
// `value`.
#[test]
fn a_class_accessor_member_is_a_function_and_a_member_name() {
    let source = "class H { float v; get float twice = { return nope; }; };\nH h;\n\
                  void() f = { local float n = h.twice; };\n";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), ["Unknown value \"nope\""]);
    assert!(index.defs.iter().any(|d| d.name == "H::get_twice"));
    let params: Vec<_> = index
        .defs
        .iter()
        .filter(|d| d.kind == DefKind::Param)
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(params, ["this", "value"]);
}

// spec FTEF-10.2: an accessor member bound to a function, `get float size = GetSize;`, is a
// use of that function, and a member with neither a function nor a body defines nothing.
#[test]
fn an_accessor_member_may_name_its_function() {
    let source = "float(float h) GetSize = #1;\n\
                  accessor box : float { get float size = GetSize; set float size; };\n";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), Vec::<String>::new());
    let get_size = index
        .defs
        .iter()
        .position(|d| d.name == "GetSize")
        .map(DefId)
        .unwrap();
    assert_eq!(index.refs.iter().filter(|r| r.def == get_size).count(), 1);
    let initialized = |name: &str| {
        index
            .defs
            .iter()
            .find(|d| d.name == name)
            .unwrap()
            .initialized
    };
    assert!(initialized("box::get_size"));
    assert!(!initialized("box::set_size"));
}

// spec FTEF-17.2: `Class::member` declares a member the class does not, of the kind its type
// gives, and a later `Class::member` refers to it; a scope that is no class names the global
// `Scope::member`.
#[test]
fn a_scoped_definition_adds_a_member_or_a_global() {
    let source = "class A { float x; };\n\
                  float A::extra;\nfloat A::extra;\n\
                  void A::Extra(void);\nvoid A::Extra(void) { extra = 1; };\n\
                  float Other::g = 1;\nvoid() f = { local float n = Other::g; };\n";
    let index = fte_index_of(source);
    // FTEF-19.15: a member the class does not declare is a warning, and a scope that is no
    // class an error, after which the global is still recorded.
    assert_eq!(messages(&index), ["Other is not a class"]);
    let warnings: Vec<&str> = index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code == Code::W0216)
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(
        warnings,
        [
            "A::extra is not part of class A",
            "A::Extra is not part of class A"
        ]
    );
    let extra = member(&index, "A", "extra");
    assert_eq!(index.defs[extra.0].kind, DefKind::Field);
    assert!(!index.defs[extra.0].initialized);
    assert_eq!(index.refs.iter().filter(|r| r.def == extra).count(), 2);
    let function = member(&index, "A", "Extra");
    assert_eq!(index.defs[function.0].kind, DefKind::Function);
    assert!(index.defs[function.0].initialized);
    let global = index
        .defs
        .iter()
        .position(|d| d.name == "Other::g")
        .map(DefId)
        .unwrap();
    assert_eq!(index.defs[global.0].member_of, None);
    assert_eq!(
        target_of(&index, source, "Other::g", 0),
        None,
        "the definition"
    );
    assert_eq!(
        target_of(&index, source, "Other::g", 1),
        Some(global),
        "the use"
    );
}

// spec FTEF-16.4: `Scope::name` whose scope is no class, as an enum's, names the global `name`;
// `super::name` that the parent class does not have is "Unknown value".
#[test]
fn a_scope_that_is_no_class_names_a_global() {
    let source = "enum E { RED, GREEN };\n\
                  class P { };\nclass Q : P { nonvirtual void Go(void); };\n\
                  void Q::Go(void) { local float n = E::GREEN; super::missing(); };\n";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), ["Unknown value \"super::missing\""]);
    let green = index
        .defs
        .iter()
        .position(|d| d.name == "GREEN")
        .map(DefId)
        .unwrap();
    assert_eq!(target_of(&index, source, "E::GREEN", 0), Some(green));
}
