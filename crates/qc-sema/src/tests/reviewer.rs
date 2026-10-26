// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R3 readiness review (TEST-9), written by a reviewer who did
//! not write the semantic model.

use std::path::PathBuf;
use std::rc::Rc;

use super::{fte_index_of, index_of, module_index_of, nth_offset};
use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;
use crate::ty::{SimpleType, Type};

// spec QC-12.5: go-to-definition covers a use's bytes `[start, end)`: the first and last byte
// of the name hit, the byte before and the byte after do not
#[test]
fn reviewer_goto_definition_covers_exactly_the_name() {
    let source = "float health;\nvoid() f = { health = 1; };";
    let index = index_of(source);
    let start = nth_offset(source, "health", 1);
    let end = start + "health".len();
    let decl = nth_offset(source, "health", 0);
    for offset in [start, end - 1] {
        let (_, location) = index.definition_at(FileId(0), offset).unwrap();
        assert_eq!(location.span.start, decl, "{offset}");
    }
    assert!(index.definition_at(FileId(0), start - 1).is_none());
    assert!(index.definition_at(FileId(0), end).is_none());
}

// spec QC-12.5: a position is looked up in its own file only: the same offset in another
// file of the module does not hit a use or a definition of the first
#[test]
fn reviewer_goto_definition_is_per_file() {
    let first = "float health;\nvoid() f = { health = 1; };";
    let second = "void() g = { };";
    let index = module_index_of(&[first, second]);
    let offset = nth_offset(first, "health", 1);
    assert!(index.definition_at(FileId(0), offset).is_some());
    assert!(index.definition_at(FileId(1), offset).is_none());
    assert!(index.references_at(FileId(1), offset, true).is_empty());
}

// spec QC-10.6: function types compare structurally with parameter names excluded, also
// when a parameter is itself a function, so a prototype and a body naming them differently
// declare one function
#[test]
fn reviewer_nested_function_types_compare_without_names() {
    let source = "void(void(float x) cb, .float fl) run;\n\
                  void(void(float y) other, .float g) run = { };";
    let index = index_of(source);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let run: Vec<_> = index.defs.iter().filter(|d| d.name == "run").collect();
    assert_eq!(run.len(), 1);
    let Type::Function(func) = &run[0].ty else {
        panic!("run is not a function: {:?}", run[0].ty);
    };
    assert!(matches!(&func.params[0], Type::Function(inner)
        if inner.params == [Type::Simple(SimpleType::Float)]));
    assert_eq!(
        func.params[1],
        Type::Field(Rc::new(Type::Simple(SimpleType::Float)))
    );
}

// spec FTEF-17.3: the type a value names goes through one pointer to a named type, and no
// other type names one
#[test]
fn reviewer_type_name_goes_through_a_pointer() {
    let named = Type::Named(Rc::from("Thing"));
    assert_eq!(named.type_name(), Some("Thing"));
    assert_eq!(Type::Pointer(Rc::new(named)).type_name(), Some("Thing"));
    for ty in [
        Type::Unknown,
        Type::Simple(SimpleType::Entity),
        Type::Field(Rc::new(Type::Named(Rc::from("Thing")))),
        Type::Pointer(Rc::new(Type::Simple(SimpleType::Float))),
    ] {
        assert_eq!(ty.type_name(), None, "{ty:?}");
    }
}

// (P2-R3 review) a file put in the Db with its path keeps it, and one given only text has none
#[test]
fn reviewer_file_paths_are_kept() {
    let mut db = Db::new();
    db.set_file(
        FileId(0),
        PathBuf::from("src/defs.qc"),
        b"float a;".to_vec(),
    );
    db.set_file_text(FileId(1), b"float b;".to_vec());
    assert_eq!(
        db.file_path(FileId(0)),
        Some(PathBuf::from("src/defs.qc").as_path())
    );
    assert_eq!(db.file_path(FileId(1)), None);
    assert_eq!(db.file_text(FileId(1)), b"float b;");
}

// spec FTEF-17.1: under the FTEQCC profile a member function's body finds the class's
// members before a global of the same name
#[test]
fn reviewer_member_before_global() {
    let source = "float hp;\nclass c { float hp; void() hurt = { hp = 1; }; };";
    let index = fte_index_of(source);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let use_at = nth_offset(source, "hp", 2);
    let (def, _) = index.definition_at(FileId(0), use_at).unwrap();
    assert!(index.def(def).unwrap().member_of.is_some());
}

/// `void(void(...void(float p0) p1...) p{depth-1}) run`, the innermost parameter typed `inner`
/// and every parameter named with `prefix`.
fn nested_prototype(depth: usize, prefix: &str, inner: &str) -> String {
    let mut ty = format!("void({inner} {prefix}0)");
    for level in 1..depth {
        ty = format!("void({ty} {prefix}{level})");
    }
    format!("{ty} run")
}

// spec QC-10.6: function types nested 200 deep compare by their parameter types at every
// level, names excluded: a prototype and a body naming every parameter differently are one
// function, and a body whose innermost parameter has another type is a mismatch
#[test]
fn reviewer_deeply_nested_function_types_compare_at_every_level() {
    let depth = 200;
    let same = format!(
        "{};\n{} = {{ }};",
        nested_prototype(depth, "a", "float"),
        nested_prototype(depth, "b", "float")
    );
    let index = index_of(&same);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.defs.iter().filter(|d| d.name == "run").count(), 1);
    let other = format!(
        "{};\n{} = {{ }};",
        nested_prototype(depth, "a", "float"),
        nested_prototype(depth, "b", "vector")
    );
    assert!(!index_of(&other).diagnostics.is_empty());
}

// spec QC-27.16: the FTEQCC forms nested past the limit that the parser stops, an aggregate,
// an initialiser list, a `?:` in its middle and an `=` chain, each 100000 deep, resolve through
// the semantic model with every diagnostic inside the file
#[test]
fn reviewer_deep_fteqcc_trees_resolve() {
    let n = 100_000;
    let in_body = |body: String| format!("float a;\nvoid() f = {{\n{body}\n}};\n");
    for source in [
        format!("{}float x;{};\n", "struct { ".repeat(n), "} s;".repeat(n)),
        format!("float x[1] = {}1{};\n", "{".repeat(n), "}".repeat(n)),
        in_body(format!("a = {}1{};", "a ? ".repeat(n), " : 1".repeat(n))),
        in_body(format!("{}1;", "a = ".repeat(n))),
    ] {
        let index = fte_index_of(&source);
        for d in &index.diagnostics {
            assert_eq!(d.file, FileId(0));
            assert!(d.diagnostic.span.end <= source.len(), "{d:?}");
        }
    }
}

// spec QC-12.1: a function with 2000 locals resolves each use to its own local, with no
// diagnostic, under both profiles
#[test]
fn reviewer_many_locals_each_resolve_to_their_own() {
    let n = 2000;
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("local float l{i};\n"));
    }
    for i in 0..n {
        body.push_str(&format!("l{i} = {i};\n"));
    }
    let source = format!("void() f = {{\n{body}}};\n");
    for index in [index_of(&source), fte_index_of(&source)] {
        assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
        for i in [0, 1000, n - 1] {
            let name = format!("l{i}");
            let use_at = nth_offset(&source, &format!("{name} ="), 0);
            let (def, _) = index.definition_at(FileId(0), use_at).unwrap();
            assert_eq!(index.def(def).unwrap().name, name);
        }
    }
}

// spec FTEF-17.10: under FTEQCC a builtin initialised again with the same number, written as a
// float or a fraction of zeros, or with the same `#0:NAME`, is accepted, and with another
// number is E0206, as the pinned FTEQCC compiles the first two and reports "incompatible
// redeclaration" for the third
#[test]
fn reviewer_builtins_initialised_again_compare_by_number() {
    for (source, redeclared) in [
        ("void() f = #16;\nvoid() f = #16.0;\n", false),
        ("void() f = #0:ex_a;\nvoid() f = #0:ex_a;\n", false),
        ("void() f = #5;\nvoid() f = #6;\n", true),
    ] {
        let index = fte_index_of(source);
        let codes: Vec<_> = index
            .diagnostics
            .iter()
            .filter(|d| d.diagnostic.code.is_error())
            .map(|d| d.diagnostic.code)
            .collect();
        let expected = if redeclared {
            vec![qc_diag::Code::E0206]
        } else {
            vec![]
        };
        assert_eq!(codes, expected, "{source}");
    }
}

/// The error codes of the FTEQCC module of `files`, read through the preprocessor in that
/// order (`Db::analyze_module`), each with the index of its file; with FTEQCC's
/// autoprototyping when `autoproto` is set.
fn fte_module_errors(files: &[&str], autoproto: bool) -> Vec<(usize, qc_diag::Code)> {
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    db.set_autoproto(autoproto);
    let mut input = ModuleInput::default();
    for (index, text) in files.iter().enumerate() {
        let id = FileId(index);
        db.set_file(
            id,
            PathBuf::from(format!("f{index}.qc")),
            text.as_bytes().to_vec(),
        );
        input.compiled.push(id);
    }
    db.analyze_module(&input)
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| (d.file.0, d.diagnostic.code))
        .collect()
}

// spec FTE-23.7: FTEQCC reuses the first function made with a number and engine name across
// the whole program, so after `g = #5:ex_a` in the first file, `f` given `#5:ex_a` twice in the
// second is E0206 there, as the pinned FTEQCC reports "incompatible redeclaration" at b.qc:2
#[test]
fn reviewer_the_reused_builtin_function_is_found_across_files() {
    let first = "void() g = #5:ex_a;\n";
    let second = "void() f = #5:ex_a;\nvoid() f = #5:ex_a;\n";
    assert_eq!(
        fte_module_errors(&[first, second], false),
        [(1, qc_diag::Code::E0206)]
    );
    assert!(fte_module_errors(&[second], false).is_empty());
}

// spec FTE-23.7: a builtin's number and engine name are compared after macro expansion, as the
// pinned FTEQCC compiles each of these with no error; a macro that gives a different number
// is still E0206
#[test]
fn reviewer_builtins_compare_after_macro_expansion() {
    for (source, redeclared) in [
        (
            "#define EN ex_a\nvoid() f = #5:EN;\nvoid() f = #5:ex_a;\n",
            false,
        ),
        ("#define N 5\nvoid() f = :N;\nvoid() f = #5;\n", false),
        (
            "#define EN ex_a\nvoid() f = :EN;\nvoid() f = :ex_a;\n",
            false,
        ),
        ("#define f h\nvoid() f = #5;\nvoid() h = #5:h;\n", false),
        ("#define NEG -5\nvoid() f = #5;\nvoid() f = :NEG;\n", true),
    ] {
        let expected = if redeclared {
            vec![(0, qc_diag::Code::E0206)]
        } else {
            vec![]
        };
        assert_eq!(fte_module_errors(&[source], false), expected, "{source}");
    }
}

// spec FTEF-17.8: a builtin first given to a `noref` definition is never compared, however
// often it is initialised again or with whatever engine name; the pinned FTEQCC compiles both
// with no error
#[test]
fn reviewer_a_noref_builtin_is_never_compared() {
    for source in [
        "noref void() f = #5:ex_a;\nvoid() f = #5:ex_b;\nvoid() f = #5:ex_b;\n",
        "noref void() f = #5:ex_a;\nvoid() f = #5:ex_b;\nvoid() f = #5:ex_a;\n",
    ] {
        assert!(fte_module_errors(&[source], false).is_empty(), "{source}");
    }
}

// spec FTE-22.3: autoprototyping reads the program twice, but a builtin is compared as in one
// reading (FTE-23.7): the pinned FTEQCC with `-Fautoproto` reports "incompatible
// redeclaration" for exactly the second and fourth of these
#[test]
fn reviewer_autoprototyping_compares_builtins_as_one_reading() {
    for (source, redeclared) in [
        ("void() g = #5:ex_a;\nvoid() f = #5:ex_a;\n", false),
        ("void() f = #5:ex_a;\nvoid() f = #5:ex_b;\n", true),
        ("void() f = #5;\nvoid() g = #5:f;\n", false),
        (
            "void() g = #5:ex_a;\nvoid() f = #5:ex_a;\nvoid() f = #5:ex_a;\n",
            true,
        ),
        ("void() f = :ex_a;\nvoid() f = #0:ex_a;\n", false),
    ] {
        let expected = if redeclared {
            vec![(0, qc_diag::Code::E0206)]
        } else {
            vec![]
        };
        assert_eq!(fte_module_errors(&[source], true), expected, "{source}");
    }
}

// spec FTE-23.7: the definitions of one declaration make their functions left to right, so
// after `f = #5:ex_a, g = #5:ex_a` initialising `g` again with `#5:ex_a` finds `f`'s function
// and is E0206, while a second `f` is not; the pinned FTEQCC agrees on both
#[test]
fn reviewer_one_declaration_makes_its_functions_in_order() {
    for (source, redeclared) in [
        (
            "void() f = #5:ex_a, g = #5:ex_a;\nvoid() g = #5:ex_a;\n",
            true,
        ),
        (
            "void() f = #5:ex_a, g = #5:ex_a;\nvoid() f = #5:ex_a;\n",
            false,
        ),
        (
            "void() f = #5:ex_a;\nvoid() f = #5:ex_a, g = #5:ex_a;\n",
            false,
        ),
        ("var void() f = #5;\nvar void() f = #5;\n", false),
        ("const void() f = #5;\nvoid() f = #5;\n", false),
    ] {
        let expected = if redeclared {
            vec![(0, qc_diag::Code::E0206)]
        } else {
            vec![]
        };
        assert_eq!(fte_module_errors(&[source], false), expected, "{source}");
    }
}

/// The error codes of `defines` then `void() f = USE;` as one FTEQCC module, read through the
/// preprocessor (`Db::analyze_module`), and the builtin function `f`'s initialiser lowers to.
fn fte_binding(defines: &str, use_: &str) -> (Vec<qc_diag::Code>, Option<(i32, String)>) {
    let mut db = Db::new();
    db.set_dialect(qc_syntax::Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("t.qc"),
        format!("{defines}\nvoid() f = {use_};\n").into_bytes(),
    );
    let errors = db
        .analyze_module(&ModuleInput {
            prelude: Vec::new(),
            compiled: vec![FileId(0)],
            ..ModuleInput::default()
        })
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect();
    let function = db
        .expanded_decls(FileId(0))
        .and_then(|decls| decls.decls.iter().find(|decl| decl.name == "f"))
        .and_then(|decl| decl.literal.as_ref())
        .and_then(|literal| literal.builtin_function("f"))
        .map(|function| (function.number, function.name));
    (errors, function)
}

fn bound(number: i32, name: &str) -> (Vec<qc_diag::Code>, Option<(i32, String)>) {
    (vec![], Some((number, name.to_owned())))
}

// The cases below were compiled with the pinned FTEQCC (fte-team/fteqw@f937b9d, `-Tid`) by the
// fifth P2-R3 review, and each binds `f` as the pinned FTEQCC does (`cargo xtask oracles
// builtins`, run with them added).

// spec FTEF-14.14: a paste in an argument of another macro is made before that macro reads it,
// and a variadic argument gives the engine name
#[test]
fn reviewer_a_builtin_name_made_by_a_paste_in_an_argument() {
    assert_eq!(
        fte_binding("#define E(x) #5:x\n#define F(x) E(x##_a)", "F(ex)"),
        bound(5, "ex_a")
    );
    assert_eq!(
        fte_binding("#define E(...) #5:__VA_ARGS__", "E(ex_a)"),
        bound(5, "ex_a")
    );
}

// spec FTEF-14.14: a paste whose left side is an object-like macro's name writes that name, which
// is expanded when it is read
#[test]
fn reviewer_a_pasted_macro_name_expands_into_the_builtin() {
    assert_eq!(
        fte_binding("#define Q #5\n#define E(x) Q##x", "E(:ex_a)"),
        bound(5, "ex_a")
    );
}

// spec FTEF-14.17: a macro given its own name as an argument is not expanded again in its own
// chunk, so the name is the engine name
#[test]
fn reviewer_a_macro_s_own_name_in_its_argument_is_the_engine_name() {
    assert_eq!(fte_binding("#define E(x) #5:x", "E(E)"), bound(5, "E"));
}

// spec FTEF-14.31: `#a` writes the argument's text, so a built-in constant in it is its name, not
// its value
#[test]
fn reviewer_a_stringified_built_in_constant_is_its_name() {
    assert_eq!(
        fte_binding("#define S(a) #a", ":S(__LINE__)"),
        bound(0, "__LINE__")
    );
}

// spec FTEF-14.29: a function-like macro an argument names, whose `(` comes after the expansion
// ends, is not called (FTEF-14.29), so `f` is not bound
#[test]
fn reviewer_a_call_after_the_expansion_ends_binds_nothing() {
    let (errors, function) = fte_binding("#define E(x) x\n#define G(x) #5:x", "E(G)(ex_a)");
    assert!(!errors.is_empty());
    assert_eq!(function, None);
}

// The cases below are the fourth P2-R2 and fifth P2-R3 reviews' (P2-80 to P2-84), each with the
// outcome the pinned FTEQCC gives, as `cargo xtask oracles builtins` records it.

/// Whether `defines` then `void() f = USE;` is refused, as FTEQCC refuses it.
fn refused(defines: &str, use_: &str) -> bool {
    !fte_binding(defines, use_).0.is_empty()
}

// spec FTEF-14.22: an argument ends at a `,` or `)` in a character constant or a comment, and a
// string ending in `\` runs on past its `"`
#[test]
fn reviewer_arguments_end_where_fteqcc_s_scanner_ends_them() {
    assert!(refused("#define E(n) :n", "E(',')"));
    assert!(refused("#define M(x) x", "M(#5 /* ) */)"));
    assert!(refused("#define E(n) n", "E(:\"ex\\\\\")"));
    assert_eq!(fte_binding("#define S(a) #a", ":S(e'x)"), bound(0, "e'x"));
}

// spec FTEF-14.27: a built-in constant an argument copies takes the place of the white space and
// `//` comment before it, at the line of the use's `)`
#[test]
fn reviewer_a_copied_line_number_touches_the_hash() {
    assert_eq!(
        fte_binding("#define E(n) n", "E(# __LINE__)"),
        bound(2, "f")
    );
    assert_eq!(
        fte_binding("#define E(n) n", "E(#//\n__LINE__)"),
        bound(3, "f")
    );
    assert_eq!(
        fte_binding("#define K(x) x", "K(#\n__LINE__)"),
        bound(3, "f")
    );
}

// spec FTEF-14.24: a stored body drops its comments and continuations, its quote flag flips at an
// escaped `"`, and a parameter named twice is refused
#[test]
fn reviewer_bodies_are_stored_as_fteqcc_stores_them() {
    assert_eq!(
        fte_binding("#define M(x) x\n#define K M /**/ (#5)", "K"),
        bound(5, "f")
    );
    assert_eq!(
        fte_binding("#define M(x) x\n#define K M \\\n(#5)", "K"),
        bound(5, "f")
    );
    assert!(refused("#define N :\"ex\\\"_a//\"", "N"));
    assert!(refused("#define E(n, n) n", "E(#5, 6)"));
}

// spec FTEF-14.26: `__VA_COUNT__` is FTEQCC's count and `,##__VA_ARGS__` with no variadic
// argument takes away the `,`
// spec FTEF-15.8: a declaration's declarators may end with a `,`
#[test]
fn reviewer_variadic_macros_count_and_paste_as_fteqcc_does() {
    assert_eq!(
        fte_binding("#define V(...) :__VA_COUNT__", "V(a, b)"),
        bound(1, "f")
    );
    assert!(refused("#define V(a, ...) #5:__VA_COUNT__", "V(a, b)"));
    assert_eq!(
        fte_binding("#define V(a, ...) a,##__VA_ARGS__", "V(#5)"),
        bound(5, "f")
    );
    assert_eq!(
        fte_binding("#define E(...) #5 __VA_ARGS__", "E(, )"),
        bound(5, "f")
    );
}

// spec FTEF-17.11: a bodyless function a macro writes is an error, whether or not it is
// referred to
#[test]
fn reviewer_a_bodyless_function_a_macro_writes_is_an_error() {
    let (errors, _) = fte_binding("#define V(a, ...) a,##__VA_ARGS__", "V(#5, g)");
    assert_eq!(errors, [qc_diag::Code::E0211]);
    let (errors, _) = fte_binding("#define V(a, ...) a __VA_ARGS__", "V(#5, :ex_a, b)");
    assert_eq!(errors, [qc_diag::Code::E0211]);
    assert_eq!(fte_binding("noref void() g;", "#5"), bound(5, "f"));
}
