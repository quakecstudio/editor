// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolution of the full FTEQCC dialect (`docs/spec/dialect-fteqcc.md`, FTEF-17): classes and
//! their members, type names, intrinsics, and the redeclaration and undefined-function rules
//! FTEQCC applies.

use std::path::PathBuf;

use qc_diag::Code;
use qc_syntax::Dialect;

use super::fte_index_of;
use crate::db::Db;
use crate::expand::ModuleInput;
use crate::resolve::ModuleIndex;
use crate::symbol::{DefId, DefKind, FileId};
use crate::ty::Type;

/// The error codes of an index.
fn errors(index: &ModuleIndex) -> Vec<Code> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect()
}

/// The messages of an index's errors.
fn messages(index: &ModuleIndex) -> Vec<String> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

/// The definitions named `name`, in order.
fn defs_named(index: &ModuleIndex, name: &str) -> Vec<DefId> {
    index
        .defs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.name == name)
        .map(|(i, _)| DefId(i))
        .collect()
}

/// The definition each use of `name` in `source` resolves to, in order of the uses.
fn targets(index: &ModuleIndex, source: &str, name: &str) -> Vec<DefId> {
    let mut out = Vec::new();
    for (offset, _) in source.match_indices(name) {
        let boundary = |b: Option<u8>| b.is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_'));
        let before = offset
            .checked_sub(1)
            .and_then(|i| source.as_bytes().get(i).copied());
        let after = offset
            .checked_add(name.len())
            .and_then(|i| source.as_bytes().get(i).copied());
        if !boundary(before) || !boundary(after) {
            continue;
        }
        if let Some(r) = index.refs.iter().find(|r| r.location.span.start == offset) {
            out.push(r.def);
        }
    }
    out
}

const CLASSES: &str = "\
entity self;
.float health;
.void() think;
float time;
entity() spawn = #14;
class Base
{
\tfloat m_flBase;
\tvirtual void Spawned(void);
\tnonvirtual void Helper(void);
};
class Thing : Base
{
\tvoid Thing(void);
\tfloat m_flTime;
\tvirtual void Spawned(void);
\tstatic Thing Make(void);
};
void Base::Spawned(void) { m_flBase = 1; };
void Base::Helper(void) { };
void Thing::Thing(void) { m_flTime = time; m_flBase = 2; };
void Thing::Spawned(void) { super::Spawned(); Helper(); think = Base::Helper; };
Thing Thing::Make(void) { Thing t = spawn(Thing); t.m_flTime = 3; t.health = 1; return t; };
void() user = { Thing t = Thing::Make(); t.Spawned(); ((Thing)self).m_flTime = 4; };
";

// spec FTEF-9.1: a class and its parent; members of the parent are members of the child.
// spec FTEF-17.1: a class's name is a type and a value, `spawn(Thing)`.
// spec FTEF-17.2: members resolve through the class and its parents, before the globals.
#[test]
fn class_members_resolve() {
    let index = fte_index_of(CLASSES);
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let base_field = defs_named(&index, "m_flBase");
    assert_eq!(base_field.len(), 1);
    // `m_flBase` is used in Base::Spawned and in Thing::Thing, both the parent's member.
    assert_eq!(
        targets(&index, CLASSES, "m_flBase")
            .iter()
            .filter(|d| **d == base_field[0])
            .count(),
        2
    );
    let thing = defs_named(&index, "Thing")[0];
    assert!(targets(&index, CLASSES, "Thing").contains(&thing));
    // A member is not a global by its name.
    assert!(index.defs[base_field[0].0].member_of.is_some());
}

// spec FTEF-9.2: a virtual member function is a field, a nonvirtual or static one a function.
// spec FTEF-9.3: a data member is a field of the class.
// spec FTEF-9.4: `Class::member` defines the member its class declares, and refers to it.
#[test]
fn member_kinds_and_definitions() {
    let index = fte_index_of(CLASSES);
    let kind = |name: &str| index.defs[defs_named(&index, name)[0].0].kind;
    assert_eq!(kind("Spawned"), DefKind::Field);
    assert_eq!(kind("Helper"), DefKind::Function);
    assert_eq!(kind("Make"), DefKind::Function);
    assert_eq!(kind("m_flTime"), DefKind::Field);
    // Base and Thing each declare `Spawned`; each definition refers to its own.
    let spawned = defs_named(&index, "Spawned");
    assert_eq!(spawned.len(), 2);
    let defined: Vec<_> = targets(&index, CLASSES, "Spawned");
    assert!(defined.contains(&spawned[0]) && defined.contains(&spawned[1]));
    assert!(index.defs[spawned[1].0].initialized);
}

// spec FTEF-17.3: a member access resolves through the class of a typed local, a cast and a
// call's return type, and an entity field resolves on a class instance.
#[test]
fn member_access_follows_the_base_type() {
    let index = fte_index_of(CLASSES);
    let time = defs_named(&index, "m_flTime")[0];
    let uses = targets(&index, CLASSES, "m_flTime");
    assert_eq!(uses.iter().filter(|d| **d == time).count(), 3);
    let health = defs_named(&index, "health")[0];
    assert!(targets(&index, CLASSES, "health").contains(&health));
}

// spec FTEF-17.3: an unknown member is reported; a vector member, `.length` and a struct field
// are no definitions, and a member of a class not known from the base resolves by name.
#[test]
fn unknown_members_are_reported() {
    let index = fte_index_of(
        "class A { float m_a; };\nentity e;\nvoid() f = { e.m_a = 1; e.nothing = 2; };\n",
    );
    assert_eq!(messages(&index), ["Unknown value \"nothing\""]);
    let m_a = defs_named(&index, "m_a")[0];
    assert_eq!(index.refs.iter().filter(|r| r.def == m_a).count(), 1);
}

// spec FTEF-16.4: `super::member` is the parent's member, `Class::member` the class's, and
// `::name` a global.
#[test]
fn scoped_names_resolve() {
    let index = fte_index_of(CLASSES);
    let helper = defs_named(&index, "Helper")[0];
    assert!(targets(&index, CLASSES, "Helper").contains(&helper));
    let global = fte_index_of("float g;\nvoid() f = { local float g; ::g = 1; };\n");
    assert_eq!(errors(&global), Vec::<Code>::new());
    let unknown = fte_index_of("class A { };\nvoid() f = { A::missing(); };\n");
    assert_eq!(messages(&unknown), ["Unknown value \"A::missing\""]);
    // `super::` in a class whose parent is the entity is not checked.
    let entity_parent =
        fte_index_of("class A { virtual void B(void); };\nvoid A::B(void) { super::B(); };\n");
    assert_eq!(errors(&entity_parent), Vec::<Code>::new());
}

// spec FTEF-9.5: every class defines `spawnfunc_Name`, never undefined, which a program may
// define itself.
#[test]
fn classes_define_spawn_functions() {
    let index = fte_index_of("class A { };\nvoid() f = { spawnfunc_A(); };\n");
    assert_eq!(errors(&index), Vec::<Code>::new());
    let defined =
        fte_index_of("class A { };\nvoid() spawnfunc_A = { };\nvoid() f = { spawnfunc_A(); };\n");
    assert_eq!(errors(&defined), Vec::<Code>::new());
}

// spec FTEF-17.1: a type's name may be declared again: a class forward, a typedef twice.
// spec FTEF-7.2: a typedef's name is a type a declaration uses.
#[test]
fn type_names_may_be_declared_again() {
    let index = fte_index_of(
        "class A;\nclass A { float x; };\ntypedef float handle_t;\ntypedef float handle_t;\n\
         handle_t h;\nstruct s;\nstruct s { float y; };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new());
    let h = defs_named(&index, "h")[0];
    assert_eq!(index.defs[h.0].ty, Type::Named("handle_t".into()));
}

// spec FTEF-17.4: `this`, `super`, `prior` and the intrinsics are no names to define, and
// `autocvar` and `sizeof` take a name and a type word.
#[test]
fn intrinsics_resolve() {
    let index = fte_index_of(
        "float() f = { local float n = sizeof(int) + autocvar(cl_speed, 3) + randomv(); \
         _(\"text\"); return this ? 0 : 1; };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
}

// spec FTEF-17.10: a second initialiser written the same way, a builtin of the same number and
// an enum declared again are no redeclaration; a different one is.
// spec FTEF-8.2: an enum's names are constants.
#[test]
fn equal_initialisers_are_accepted() {
    let index = fte_index_of(
        "const vector v = [0, 0, 0];\nconst vector v = [0, 0, 0];\n\
         void() b = #12;\nvoid() b = #12;\n\
         enum { A, B };\nenum { A, B };\nconst float B = 7;\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let different = fte_index_of("void() b = #12;\nvoid() b = #13;\n");
    assert_eq!(errors(&different), [Code::E0206]);
}

// spec FTEF-17.10: a builtin's number is the `binum` FTEQCC binds (FTE-17.6): `#16777217i` is
// not `#16777216`, and `#4294967313i` is `#17`
#[test]
fn builtins_compare_by_the_number_fteqcc_binds() {
    let int = fte_index_of("void() b = #16777217i;\nvoid() b = #16777216;\n");
    assert_eq!(errors(&int), [Code::E0206]);
    let wrapped = fte_index_of("void() b = #4294967313i;\nvoid() b = #17;\n");
    assert_eq!(
        errors(&wrapped),
        Vec::<Code>::new(),
        "{:?}",
        messages(&wrapped)
    );
}

// spec FTEF-17.10: an initialiser that is no literal is compared by its text, token for token:
// the same tokens spaced differently are the same value, other tokens are not.
#[test]
fn other_initialisers_compare_token_for_token() {
    let spaced = fte_index_of("const float a = 1 + 2;\nconst float a = 1+2;\n");
    assert_eq!(
        errors(&spaced),
        Vec::<Code>::new(),
        "{:?}",
        messages(&spaced)
    );
    let other = fte_index_of("const float a = 1 + 2;\nconst float a = 1 + 3;\n");
    assert_eq!(errors(&other), [Code::E0206]);
}

// spec FTEF-17.6: a redeclaration whose type involves a type name, a pointer or an unknown
// type is no mismatch; one between two simple types still is.
// spec FTEF-11.2: `__variant` mixes with a concrete type without a type error.
#[test]
fn opaque_types_are_compatible() {
    let index = fte_index_of(
        "typedef float bool;\nbool flag;\nfloat flag;\n__variant(float) get;\nfloat(float) get;\n\
         float *p;\nentity p;\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let mismatch = fte_index_of("float x;\nstring x;\n");
    assert_eq!(errors(&mismatch), [Code::E0205]);
}

// spec FTEF-17.7: a function with no body is an error only where it is referred to, and a
// member function is not checked.
#[test]
fn undefined_functions_need_a_reference() {
    let unused = fte_index_of("void() never;\nclass A { nonvirtual void M(void); };\n");
    assert_eq!(errors(&unused), Vec::<Code>::new());
    let used = fte_index_of("void() needed;\nvoid() f = { needed(); };\n");
    assert_eq!(errors(&used), [Code::E0211]);
}

// spec FTEF-15.5: `wrap`, `weak` and `accumulate` definitions may be given again, `extern` has
// its body elsewhere, and a `var` function is a variable.
#[test]
fn redefinable_external_and_variable_functions() {
    let index = fte_index_of(
        "void() a = { };\n__wrap void() a = { prior(); };\n\
         __weak void() w = { };\nvoid() w = { };\n\
         __accumulate void() c = { };\n__accumulate void() c = { };\n\
         extern void() e;\nvar void() v;\n\
         void() user = { e(); v(); };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let v = defs_named(&index, "v")[0];
    assert_eq!(index.defs[v.0].kind, DefKind::Global);
    let twice = fte_index_of("void() a = { };\nvoid() a = { };\n");
    assert_eq!(errors(&twice), [Code::E0206]);
}

// spec FTEF-10.2: an accessor's members are no globals, and their bodies see `this`, `value`
// and the index.
#[test]
fn accessor_members_resolve() {
    let index = fte_index_of(
        "float(float b, float i) bufstr_get = #0;\n\
         accessor strbuf : float { inline get float at[float idx] = { return bufstr_get(this, idx); }; \
         set float at[float idx] = { bufstr_get(this, value); }; };\n\
         strbuf b;\nvoid() f = { local float n = b.at; };\n",
    );
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
}

// spec FTEF-12.6: `#pragma noref 1` marks the definitions after it, across files, until
// `#pragma noref 0`.
// spec FTEF-17.8: a `noref` definition may be initialised again with another value.
#[test]
fn noref_allows_reinitialisation() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    let files = [
        (
            "ext.qc",
            "#pragma noref 1\nstring() info = #626;\n#pragma noref 0\nfloat() plain = #1;\n",
        ),
        ("mod.qc", "string() info = #0;\nfloat() plain = #2;\n"),
    ];
    for (index, (name, text)) in files.iter().enumerate() {
        db.set_file(FileId(index), PathBuf::from(name), text.as_bytes().to_vec());
    }
    let input = ModuleInput {
        prelude: Vec::new(),
        compiled: vec![FileId(0), FileId(1)],
        autoproto: None,
    };
    let analysis = db.analyze_module(&input);
    let codes: Vec<_> = analysis.diagnostics().map(|d| d.diagnostic.code).collect();
    assert_eq!(codes, [Code::E0206], "only the plain builtin is redeclared");
    let noref = fte_index_of("noref float x = 1;\nfloat x = 2;\n");
    assert_eq!(errors(&noref), Vec::<Code>::new());
}

// spec FTEF-4.1: a pointer declarator's type is a pointer to the declaration's type.
// spec FTEF-15.6: `struct { ... } a[4], *p;` declares variables of the struct.
#[test]
fn pointer_declarators_and_struct_variables() {
    let index =
        fte_index_of("struct seat_s { float x; } seats[4], *seat;\nvoid() f = { seat->x = 1; };\n");
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let seat = defs_named(&index, "seat")[0];
    assert_eq!(
        index.defs[seat.0].ty,
        Type::Pointer(std::rc::Rc::new(Type::Named("seat_s".into())))
    );
}

// spec FTEF-8.2: an enum written as a typedef's type declares its names, which are
// constants a later declaration uses.
#[test]
fn a_typedef_enum_declares_its_names() {
    let index = fte_index_of("typedef enum { RED, GREEN } color_t;\ncolor_t c = GREEN;\n");
    assert_eq!(errors(&index), Vec::<Code>::new(), "{:?}", messages(&index));
    let green = defs_named(&index, "GREEN");
    assert_eq!(green.len(), 1);
    assert_eq!(index.refs.iter().filter(|r| r.def == green[0]).count(), 1);
    assert_eq!(defs_named(&index, "color_t").len(), 1);
}

// spec FTEF-8.2: an enum written as a parameter's type declares its names.
// spec FTEF-15.3: an unnamed parameter declares nothing, and `(void)` has no parameters.
#[test]
fn parameter_forms_resolve() {
    let index = fte_index_of(
        "float(float, optional enumflags:float{WP_A, WP_B} flags) which;\nfloat(void) none;\n\
         void() f = { which(WP_A, WP_B); none(); };\n",
    );
    let codes = errors(&index);
    assert_eq!(codes, [Code::E0211, Code::E0211], "{:?}", messages(&index));
    let none = defs_named(&index, "none")[0];
    let Type::Function(func) = &index.defs[none.0].ty else {
        panic!("not a function");
    };
    assert!(func.params.is_empty());
}
