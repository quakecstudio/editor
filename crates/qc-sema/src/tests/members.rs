// SPDX-License-Identifier: GPL-3.0-or-later

//! The members of FTEQCC classes (`docs/spec/dialect-fteqcc.md`, FTEF-9, FTEF-10, FTEF-16.4,
//! FTEF-17): how a class body's labels, constructors, inline bodies and accessor members are
//! read, and which class a member access is resolved in when two classes have a member of the
//! same name.

use qc_diag::Code;

use super::fte_index_of;
use crate::resolve::ModuleIndex;
use std::path::PathBuf;

use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::{DefId, DefKind, FileId};

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
class C { A inner; static A Get(void) = #5; };
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
    // spec FTEF-24.5: a member through a pointer to a class is no struct's (the pinned FTEQCC,
    // 2026-10-27: line 15, `pb->m`)
    // spec FTEF-24.7: the class prints as `class B`
    assert_eq!(
        messages(&index),
        ["indirection in class B - not a struct or union"]
    );
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
// scope that is no class and names no type is "is not a type" (FTEF-19.33).
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
    // FTEF-19.33: the scope of `Scope::name` is read as a type name.
    assert_eq!(messages(&index), ["\"Nope\" is not a type"]);
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
    let source = "class M { float count; nonvirtual void Bump(void) = { this.count = this.count + 1; \
                  nope = 2; }; };\n";
    let index = fte_index_of(source);
    assert_eq!(messages(&index), ["Unknown value \"nope\"."]);
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
    assert_eq!(messages(&index), ["Unknown value \"nope\"."]);
    assert!(index.defs.iter().any(|d| d.name == "H::get_twice"));
    let params: Vec<_> = index
        .defs
        .iter()
        .filter(|d| d.kind == DefKind::Param)
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(params, ["this", "value"]);
}

// spec FTEF-10.2: an accessor member bound to a function, `get float size = GetSize;`, refers to
// that function, and a `set` member with a body defines the write function.
#[test]
fn an_accessor_member_may_name_its_function() {
    let source = "accessor box : float { get float size = GetSize; set float size = { }; };\n\
                  float(box this) GetSize = #1;\n";
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
    assert!(initialized("GetSize"));
    assert!(initialized("box::set_size"));
    assert!(index.defs.iter().all(|d| d.name != "box::get_size"));
}

// spec FTEF-17.14: `Class::member` declares a member the class does not, of the kind its type
// gives, and a later `Class::member` refers to it; a scope that is no class names the global
// `Scope::member`.
#[test]
fn a_scoped_definition_adds_a_member_or_a_global() {
    let source = "class A { float x; };\n\
                  float A::extra;\nfloat A::extra;\n\
                  void A::Extra(void);\nvoid A::Extra(void) { this.extra = 1; };\n\
                  float Other::g = 1;\nvoid() f = { local float n = Other::g; };\n";
    let index = fte_index_of(source);
    // FTEF-19.15: a member the class does not declare is a warning, and a scope that is no
    // class an error, after which the global is still recorded.
    assert_eq!(
        messages(&index),
        ["Other is not a class", "\"Other\" is not a type"]
    );
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
    // FTEF-19.33: the use's scope is no type, so the use names nothing.
    assert_eq!(target_of(&index, source, "Other::g", 1), None, "the use");
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

// spec FTEF-10.7: an accessor member is read by its `get` and stored by its `set`, its index
// made the indexer's type and the stored value the `set` member's
// spec QCD-4.54: E0348 for a read with no `get`
// spec QCD-4.55: E0349 for a store with no `set`
// spec FTEF-10.8: an accessor type and its base cast with a warning, and a member's body is
// checked with `this` of the accessor's type
// spec QCD-7.106: W0264 for a cast between an accessor type and its base
#[test]
fn accessor_reads_and_stores() {
    let defs = "string(float b, float i) bufstr_get = #0; \
                void(float b, float i, string s) bufstr_set = #0;\n\
                accessor strbuf : float { inline get string at[float idx] = \
                { return bufstr_get(this, idx); }; inline set string at[float idx] = \
                { bufstr_set(this, idx, value); }; get float size = { return 1; }; \
                set float only = { }; };\n\
                strbuf h; float a; string s;\n";
    let fte_messages = |source: &str| -> Vec<String> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        let file = FileId(0);
        db.set_file(file, PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![file],
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| d.diagnostic.message.clone())
        .collect()
    };
    let body = |statement: &str| {
        let messages = fte_messages(&format!("{defs}void() g = {{ {statement} }};\n"));
        // The accessor's own bodies warn of `this` passed as its base.
        messages
            .into_iter()
            .filter(|m| m != "Implicit cast from strbuf to float")
            .collect::<Vec<_>>()
    };
    for quiet in ["s = h.at[1];", "h.at[1] = s;", "a = h.size;"] {
        assert_eq!(body(quiet), Vec::<String>::new(), "{quiet}");
    }
    let cases = [
        ("h.at[1] = a;", vec!["Cannot cast from float to string"]),
        ("a = h.at[1];", vec!["Cannot cast from string to float"]),
        ("h.size = 2;", vec!["Accessor has no set function"]),
        ("a = h.only;", vec!["Accessor only has no get function"]),
        (
            "a = h.nope;",
            vec![
                "nope is not a member of strbuf",
                "Cannot cast from void to float",
            ],
        ),
        (
            "s = h.at[\"x\"];",
            vec!["Implicit type mismatch. Needed float, got string."],
        ),
        (
            "h.at[1] += \"x\";",
            vec!["Type mismatch on assignment. string += string is not supported"],
        ),
    ];
    for (statement, expected) in cases {
        assert_eq!(body(statement), expected, "{statement}");
    }
    assert_eq!(
        fte_messages(&format!("{defs}void() g = {{}};\n"))
            .iter()
            .filter(|m| *m == "Implicit cast from strbuf to float")
            .count(),
        2
    );
    let casts = "accessor H : float; H h; float b;\nvoid() g = { b = h; h = b; };\n";
    assert_eq!(
        fte_messages(casts),
        [
            "Implicit cast from H to float",
            "Implicit cast from float to H"
        ]
    );
    assert_eq!(
        fte_messages("accessor H : string; H h; float b;\nvoid() g = { b = h; };\n"),
        ["Cannot cast from string to float"]
    );
}

// spec FTEF-10.8: a literal of an accessor's base type casts to it with a warning, an operator
// casts an accessor operand to its opcode's type, negation refuses one, and a member that takes
// an index needs its `[`
// spec QCD-7.106: W0264 "Implicit cast from A to B"
#[test]
fn accessor_values_in_expressions() {
    let body = |statement: &str| -> Vec<(Code, String)> {
        let source = format!(
            "#pragma target fte\naccessor A : float {{ get float y = {{ return 1; }}; \
             get float z[__int32 i] = {{ return 1; }}; set float z[__int32 i] = {{}}; }};\n\
             A a; float f;\nvoid() g = {{ {statement} }};\n"
        );
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        let file = FileId(0);
        db.set_file(file, PathBuf::from("t.qc"), source.into_bytes());
        db.analyze_module(&ModuleInput {
            compiled: vec![file],
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
    };
    let cast = |from: &str, to: &str| (Code::W0264, format!("Implicit cast from {from} to {to}"));
    assert_eq!(body("a = 1;"), [cast("float", "A")]);
    assert_eq!(body("local A l = 0;"), [cast("float", "A")]);
    assert_eq!(body("a = 1i;"), []);
    assert_eq!(body("f = a * f;"), [cast("A", "float")]);
    assert_eq!(body("f = f + a;"), [cast("A", "float")]);
    assert_eq!(
        body("f = -a;"),
        [(Code::E0302, "type mismatch for -".to_owned())]
    );
    for (statement, found) in [("a.z = f;", "="), ("f = a.z + 1;", "+"), ("f = a.z;", ";")] {
        assert_eq!(
            body(statement),
            [(Code::E0204, format!("expected [, found {found}"))],
            "{statement}"
        );
    }
    assert_eq!(body("a.z[1] = f; f = a.z[1];"), []);
}

// spec FTEF-24.15: a member of a class declared but never defined
// spec QCD-4.72: E0371 "Class \"CLASS\" is not defined, cannot access member \"NAME\""
// spec FTEF-30.10: a conditional of more than three words is "oversized ternary result"
// spec QCD-4.71: E0370 "oversized ternary result"
#[test]
fn undefined_classes_and_oversized_conditionals() {
    let messages = |source: &str| -> Vec<(Code, String)> {
        fte_index_of(source)
            .diagnostics
            .iter()
            .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
            .collect()
    };
    assert_eq!(
        messages("class C; C c; float x;\nvoid() g = { x = c.y; };\n"),
        [(
            Code::E0371,
            "Class \"C\" is not defined, cannot access member \"y\"".to_owned()
        )]
    );
    assert_eq!(
        messages("class C : entity { float m; }; C c; float x;\nvoid() g = { x = c.y; };\n"),
        [(Code::E0319, "Unknown field \"y\" in class \"C\"".to_owned())]
    );
    assert_eq!(
        messages(
            "struct S { float a, b, c, d; }; S s1, s2; float c;\n\
             void() g = { local S r = c ? s1 : s2; };\n"
        ),
        [(Code::E0370, "oversized ternary result".to_owned())]
    );
}

// spec FTEF-17.18: `self` in a member function is `this`, with F331's warning when it is on
// spec QCD-7.109: W0316 "'self' used inside OO function, use 'this'."
// spec FTEF-17.16: an inline member function sees the members declared before it, where a
// warning pragma ahead of the class moves the text the parser read
#[test]
fn self_in_member_functions_is_this() {
    let messages = |pragma: &str| -> Vec<String> {
        let source = format!(
            "{pragma}\nentity self; .float y;\nclass C : entity {{ float x;\n\
             void() m = {{ this.x = 1; self.x = 2; self.y = 3; z = 1; }}; float z; }};\n"
        );
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        let file = FileId(0);
        db.set_file(file, PathBuf::from("t.qc"), source.into_bytes());
        db.analyze_module(&ModuleInput {
            compiled: vec![file],
            ..ModuleInput::default()
        })
        .diagnostics()
        .filter(|d| !d.diagnostic.message.ends_with("Assuming non-virtual."))
        .map(|d| format!("{}: {}", d.diagnostic.code.as_str(), d.diagnostic.message))
        .collect()
    };
    let this = "W0316: 'self' used inside OO function, use 'this'.";
    let unknown = "E0301: Unknown value \"z\".";
    assert_eq!(
        messages("#pragma warning enable F331"),
        [unknown, this, this]
    );
    assert_eq!(messages("#pragma warning disable F331"), [unknown]);
}

// spec FTEF-10.10: a class's accessor member may not be virtual or static
// spec QCD-4.78: E0377 "virtual accessors are not supported at this time"
// spec QCD-4.79: E0378 "static accessors are not supported"
#[test]
fn class_accessors_are_neither_virtual_nor_static() {
    let messages = |words: &str| -> Vec<(Code, String)> {
        fte_index_of(&format!(
            "class C : entity {{ {words} float x = {{ return 1; }}; }};\n"
        ))
        .diagnostics
        .iter()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
    };
    let virtual_ = (
        Code::E0377,
        "virtual accessors are not supported at this time".to_owned(),
    );
    let static_ = (Code::E0378, "static accessors are not supported".to_owned());
    assert!(messages("get").is_empty());
    assert_eq!(messages("virtual get"), std::slice::from_ref(&virtual_));
    assert_eq!(messages("get virtual"), std::slice::from_ref(&virtual_));
    assert_eq!(messages("static get"), std::slice::from_ref(&static_));
    assert_eq!(messages("const get"), std::slice::from_ref(&static_));
    assert_eq!(messages("virtual static get"), [virtual_, static_]);
}

// spec FTEF-9.20: `virtual`, `nonvirtual` and `inline` written on a member that is no function,
// and `inline` on a prototype or a virtual function, end FTEQCC
// spec QCD-4.80: E0379 for each
#[test]
fn words_only_a_member_function_takes() {
    let messages = |member: &str| -> Vec<(Code, String)> {
        fte_index_of(&format!("class C : entity {{ {member} }};\n"))
            .diagnostics
            .iter()
            .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
            .filter(|(_, m)| !m.ends_with("Assuming non-virtual.") && !m.ends_with("has no body"))
            .collect()
    };
    let e = |message: &str| vec![(Code::E0379, message.to_owned())];
    let virtuality = e("virtual keyword on member that is not a function");
    assert_eq!(messages("virtual float m;"), virtuality);
    assert_eq!(messages("nonvirtual float m;"), virtuality);
    assert_eq!(
        messages("inline float m;"),
        e("inline keyword on member that is not a function")
    );
    let prototype = e("inline keyword on function prototype or virtual function");
    assert_eq!(messages("inline void() m;"), prototype);
    assert_eq!(messages("virtual inline void() m = { };"), prototype);
    assert!(messages("inline void() m = { };").is_empty());
    assert!(messages("virtual: float x;").is_empty());
    assert_eq!(messages("virtual: inline void() m = { };"), prototype);
}

// spec FTEF-10.11: an accessor member whose value is a name declares that function with the
// member's function type
#[test]
fn accessor_members_declare_the_functions_they_name() {
    let messages = |source: &str| -> Vec<String> {
        fte_index_of(source)
            .diagnostics
            .iter()
            .map(|d| d.diagnostic.message.clone())
            .collect()
    };
    assert_eq!(
        messages("float() g = { return 1; };\naccessor A : float { get float x = g; };\n"),
        ["Type mismatch on redeclaration of g. float(A this), should be float()"]
    );
    assert_eq!(
        messages("accessor A : float { get float x = nope; };\n"),
        ["function nope has no body"]
    );
    assert!(
        messages(
            "accessor A : float { get float x[float i] = g; };\n\
             float(A this, float i) g = { return 1; };\n"
        )
        .is_empty()
    );
    assert!(
        messages("class C : entity { set float x = s; };\nvoid(C this, float value) s = {};\n")
            .is_empty()
    );
    assert_eq!(
        messages("class C : entity { get float x = g; };\nfloat() g = { return 1; };\n")
            .first()
            .map(String::as_str),
        Some("Type mismatch on redeclaration of g. float(), should be float(class C this)")
    );
}

// spec FTEF-24.1: `e.(expr)` selects the field the expression's value is, of the type it holds;
// a value that is no field is "NAME is not a field", NAME the value's (`QCC_GetSRefName`)
#[test]
fn fields_selected_by_an_expression() {
    let messages = |statement: &str| -> Vec<String> {
        fte_index_of(&format!(
            "#pragma target fte\n.float fa, fb; .string fs; var .float fv = fa; entity e; \
             float x; float c; float farr[3]; __variant va; class C : entity {{ float m; }}; \
             C cc;\nvoid() f = {{ {statement} }};\n"
        ))
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.clone())
        .collect()
    };
    for fine in [
        "x = e.(fa);",
        "x = e.(fv);",
        "e.(fv) = 1;",
        "x = e.(va);",
        "x = e.(c ? fa : fb);",
    ] {
        assert!(messages(fine).is_empty(), "{fine}: {:?}", messages(fine));
    }
    assert_eq!(
        messages("local string q = e.(fv);"),
        ["Cannot cast from float to string"]
    );
    assert_eq!(
        messages("x = e.(fs);"),
        ["Cannot cast from string to float"]
    );
    assert_eq!(messages("x = e.(x);"), ["x is not a field"]);
    assert_eq!(messages("x = e.(x + 1);"), ["float is not a field"]);
    assert_eq!(messages("x = e.(3);"), ["IMMEDIATE is not a field"]);
    assert_eq!(messages("x = e.(farr[1]);"), ["farr is not a field"]);
    assert_eq!(messages("x = cc.(x);"), ["x is not a field of class C"]);
}

// spec FTEF-10.7: an accessor's unnamed indexer, `get T[I]`, is what `base[index]` reads and
// stores, the index cast to its type and the value to the member's
#[test]
fn an_accessor_indexer_reads_and_stores() {
    let defs = "string(float b, float i) bufstr_get = #0; \
                void(float b, float i, string s) bufstr_set = #0;\n\
                accessor strbuf : float { inline get string[float idx] = \
                { return bufstr_get(this, idx); }; inline set string[float idx] = \
                { bufstr_set(this, idx, value); }; };\nstrbuf h; float x; string s;\n";
    let found = |statement: &str| -> Vec<String> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        let file = FileId(0);
        let source = format!("#pragma target fte\n{defs}void() f = {{ {statement} }};\n");
        db.set_file(file, PathBuf::from("t.qc"), source.into_bytes());
        db.analyze_module(&ModuleInput {
            compiled: vec![file],
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| d.diagnostic.message.clone())
        .filter(|m| m != "Implicit cast from strbuf to float")
        .collect()
    };
    assert!(found("s = h[1];").is_empty());
    assert!(found("h[2] = \"a\";").is_empty());
    assert_eq!(found("x = h[1];"), ["Cannot cast from string to float"]);
    assert_eq!(found("h[1] = 5;"), ["Cannot cast from float to string"]);
    assert_eq!(
        found("s = h[\"a\"];"),
        ["Implicit type mismatch. Needed float, got string."]
    );
}
