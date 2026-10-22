// SPDX-License-Identifier: GPL-3.0-or-later

//! The full FTEQCC dialect beyond the rerelease subset (`docs/spec/dialect-fteqcc.md`): each
//! construct parses under the FTEQCC profile, keeps its bytes, and is a diagnostic under the
//! vanilla profile.

use super::sexpr;
use crate::ast::{self, AstNode as _};
use crate::{Code, Dialect, Parse, SyntaxKind, parse, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The error codes of `source` under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<Code> {
    fte(source).errors().map(|d| d.code).collect()
}

/// Whether the vanilla parser rejects `source`.
fn vanilla_rejects(source: &str) -> bool {
    parse(source.as_bytes()).errors().next().is_some()
}

/// Asserts `source` parses with no error under the FTEQCC dialect, keeps its bytes, and is
/// rejected by the vanilla dialect.
fn fte_ok(source: &str) -> Parse {
    let parsed = fte(source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new(),
        "{source}\n{}",
        parsed.syntax().debug_tree()
    );
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    assert!(vanilla_rejects(source), "vanilla accepts {source}");
    parsed
}

/// `body` as the statements of a function, indented.
fn in_body(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
    format!("float a, b, c;\nentity e;\nvoid() f =\n{{\n{indented}}};\n")
}

/// The statements of the function `f` of `parsed`.
fn statements(parsed: &Parse) -> Vec<ast::Stmt> {
    let file = parsed.tree().unwrap();
    let body = file
        .declarations()
        .filter_map(|d| d.declarators().next())
        .find(|d| d.name().is_some_and(|n| n.text() == b"f"))
        .and_then(|d| d.initializer());
    let Some(ast::Initializer::Body(body)) = body else {
        panic!("f has no body");
    };
    body.block().unwrap().statements().collect()
}

/// The s-expression of the expression statement that is the body's first statement.
fn first_expr(stmt: &str) -> String {
    let parsed = fte_ok(&in_body(stmt));
    match statements(&parsed).first() {
        Some(ast::Stmt::Expr(expr)) => sexpr(&expr.expr().unwrap()),
        other => panic!("not an expression statement: {other:?}"),
    }
}

/// Whether the body's first statement declares locals.
fn declares_local(stmt: &str) -> bool {
    let parsed = fte(&in_body(stmt));
    matches!(statements(&parsed).first(), Some(ast::Stmt::Local(_)))
}

/// The first declaration's type, as the kinds of its nodes from the outside in.
fn type_shape(source: &str) -> Vec<SyntaxKind> {
    let parsed = fte_ok(source);
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let mut kinds = Vec::new();
    let mut node = decl.ty().map(|t| t.syntax().clone());
    while let Some(n) = node {
        kinds.push(n.kind());
        node = n.children().find(|c| ast::Type::can_cast(c.kind()));
    }
    kinds
}

/// The diagnostics of `source` under the FTEQCC dialect, warnings included.
fn fte_diagnostics(source: &str) -> Vec<(Code, String)> {
    fte(source)
        .diagnostics()
        .iter()
        .map(|d| (d.code, d.message.clone()))
        .collect()
}

// spec FTEF-2.1: `int` and `integer` are types.
// spec FTEF-2.7: the enabled bare words and any `__` word are C type words.
// spec FTEF-2.8: the words the loop takes make one type.
#[test]
fn c_scalar_types_parse() {
    for source in [
        "int x;\n",
        "integer x;\n",
        "unsigned int x;\n",
        "int unsigned x;\n",
        "__signed __char x;\n",
        "unsigned __short x;\n",
        "__long __long x;\n",
        "__long int x;\n",
        "__double x;\n",
        "__LONG __double x;\n",
        "__int x;\n",
        "_Bool x;\n",
    ] {
        let parsed = fte_ok(source);
        let decl = parsed.tree().unwrap().declarations().next().unwrap();
        let Some(ast::Type::Simple(simple)) = decl.ty() else {
            panic!("{source}");
        };
        assert_eq!(simple.text(), source.trim_end_matches(" x;\n"), "{source}");
    }
}

// spec FTEF-2.7: a bare word whose keyword is disabled is no C type word, so the parser reads it
// as a type's name, which the semantic model finds is no type (FTEF-19.20)
#[test]
fn disabled_c_words_are_names() {
    for word in ["long", "double", "signed", "short", "char"] {
        let source = format!("{word} __long x;\n");
        let parsed = fte(&source);
        let decl = parsed.tree().unwrap().declarations().next().unwrap();
        let Some(ast::Type::Simple(simple)) = decl.ty() else {
            panic!("{source}");
        };
        assert_eq!(simple.text(), word, "{source}");
    }
}

// spec FTEF-2.8: a leading `register` is a storage qualifier `QCC_PR_ParseDefs` reads before the
// type, which the C words then make
#[test]
fn a_leading_register_is_no_part_of_the_type() {
    let parsed = fte("__register int x;\n");
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let Some(ast::Type::Simple(simple)) = decl.ty() else {
        panic!("no type");
    };
    assert_eq!(simple.text(), "int");
    assert_eq!(parsed.errors().count(), 0);
}

// spec FTEF-2.8: a word the loop does not take, or one after a registered type name, is the
// declarator's name
#[test]
fn the_loop_ends_the_type() {
    for (source, ty) in [
        ("int int i;\n", "int"),
        ("unsigned unsigned u;\n", "unsigned"),
        ("int float f;\n", "int"),
        ("__long __long int x;\n", "__long __long"),
        ("__int unsigned x;\n", "__int"),
        ("unsigned _Bool b;\n", "unsigned"),
    ] {
        let parsed = fte(source);
        let decl = parsed.tree().unwrap().declarations().next().unwrap();
        let Some(ast::Type::Simple(simple)) = decl.ty() else {
            panic!("{source}");
        };
        assert_eq!(simple.text(), ty, "{source}");
        assert!(parsed.errors().next().is_some(), "{source}");
    }
}

// spec QCD-7.52: W0226 for an unsigned float, a long double and a `const` in a type
// spec FTEF-2.9: a narrowed type is a warning at its first word
#[test]
fn narrowed_types_warn() {
    assert_eq!(
        fte_diagnostics("unsigned float f;\n"),
        [(
            Code::W0226,
            "ignoring unsupported unsigned keyword, type will be signed".to_owned()
        )]
    );
    assert_eq!(
        fte_diagnostics("__long __double d;\n"),
        [(
            Code::W0226,
            "long doubles are not supported, using double".to_owned()
        )]
    );
    assert_eq!(fte_diagnostics("unsigned __long u;\n"), []);
}

// spec FTEF-2.10: in a body a bare enabled C word or a registered type name starts a
// definition, and another `__` C word is a value
#[test]
fn c_words_in_a_body() {
    let locals = |body: &str| {
        let source = format!("void() g = {{ {body} }};\n");
        fte(&source)
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LocalDecl)
            .count()
    };
    for body in [
        "int x;",
        "integer x;",
        "unsigned x;",
        "_Bool x;",
        "__int x;",
        "__double x;",
    ] {
        assert_eq!(locals(body), 1, "{body}");
    }
    for body in [
        "__long x;",
        "__short x;",
        "__unsigned x;",
        "__volatile float x;",
    ] {
        assert_eq!(locals(body), 0, "{body}");
    }
}

// spec FTEF-2.11: `const` in a type is read with a warning, `volatile` only after `__`
#[test]
fn const_and_volatile_in_types() {
    let const_warning = (Code::W0226, "ignoring unsupported const keyword".to_owned());
    assert_eq!(
        fte_diagnostics(".const float x;\n"),
        std::slice::from_ref(&const_warning)
    );
    assert_eq!(
        fte_diagnostics("float *const p;\n"),
        std::slice::from_ref(&const_warning)
    );
    assert_eq!(
        fte_diagnostics("void(const string s) f;\n"),
        [const_warning]
    );
    assert_eq!(fte_diagnostics("__volatile float v;\n"), []);
    assert_eq!(fte_diagnostics("void(__volatile float v) f;\n"), []);
    assert!(fte("void(volatile float v) f;\n").errors().next().is_some());
}

// spec FTEF-4.1: `T *` is a pointer type, any number of levels, after any base type.
// spec FTEF-15.6: `T a, *b` declares `b` a pointer.
#[test]
fn pointer_types_parse() {
    assert_eq!(
        type_shape("float *p;\n"),
        [SyntaxKind::PointerType, SyntaxKind::SimpleType]
    );
    assert_eq!(
        type_shape("__variant **pp;\n"),
        [
            SyntaxKind::PointerType,
            SyntaxKind::PointerType,
            SyntaxKind::SimpleType
        ]
    );
    let parsed = fte_ok("float a, *b;\n");
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let depths: Vec<usize> = decl.declarators().map(|d| d.pointer_depth()).collect();
    assert_eq!(depths, [0, 1]);
    // `.*float`: a field of a pointer.
    fte_ok(".*float fp;\n");
}

// spec FTEF-4.1: `(T *) e` casts to a pointer type.
// spec FTEF-4.2: `&e` takes an address.
// spec FTEF-4.4: `*e` dereferences.
// spec FTEF-16.7: `++e`, `--e`, `&e` and `*e` are terms.
#[test]
fn pointer_operators_parse() {
    assert_eq!(first_expr("a = (__variant *)b;"), "(= a (cast b))");
    assert_eq!(first_expr("a = &b;"), "(= a (unary None b))");
    assert_eq!(first_expr("a = *b;"), "(= a (unary None b))");
    assert_eq!(first_expr("++a;"), "(unary None a)");
    assert_eq!(first_expr("--a;"), "(unary None a)");
}

// spec FTEF-4.3: `p->m` selects a member through a pointer.
#[test]
fn arrow_member_access_parses() {
    assert_eq!(first_expr("a = e->b;"), "(= a (. e b))");
}

// spec FTEF-5.1: `T name[N]` declares a sized array.
// spec FTEF-5.2: several `[N]` declare more dimensions.
// spec FTEF-5.3: a sized array may be a local, a parameter or a struct member.
#[test]
fn sized_arrays_parse() {
    let parsed = fte_ok("float grid[4][8];\n");
    let declarator = parsed
        .tree()
        .unwrap()
        .declarations()
        .next()
        .unwrap()
        .declarators()
        .next()
        .unwrap();
    assert_eq!(declarator.array_suffixes().count(), 2);
    assert!(declares_local("float x[3];"));
    fte_ok("void(float v[3]) f;\n");
    fte_ok("typedef struct { float m[2][2]; } mat_t;\n");
}

// spec FTEF-6.1: `struct Name { ... };` defines a struct, and `struct Name;` declares it.
// spec FTEF-6.2: `union Name { ... };` is the same.
// spec FTEF-6.3: `struct Child : Parent` inherits.
// spec FTEF-6.4: a member may be an array, a pointer, a function, a nested struct or a bitfield.
#[test]
fn structs_and_unions_parse() {
    for source in [
        "struct point { float x; float y; };\n",
        "struct point;\n",
        "union value { float f; int i; };\n",
        "struct child : point { float z; };\n",
        "struct holder { float a[4]; entity *e; float get(int i); int bits : 3; };\n",
        "typedef struct { struct inner { int id; } *list; int count; } outer_t;\n",
    ] {
        fte_ok(source);
    }
    let parsed = fte_ok("struct point { float x; float y; };\n");
    let decl = parsed
        .syntax()
        .children()
        .find_map(ast::StructDecl::cast)
        .unwrap();
    assert_eq!(decl.name().unwrap().text(), b"point");
    assert_eq!(decl.fields().count(), 2);
}

// spec FTEF-6.5: a struct is initialised with braces and its members selected with `.`.
// spec FTEF-16.11: a brace initialiser may name the member each value initialises.
// spec FTEF-15.6: `struct { ... } a[4], *p;` declares variables of the struct.
#[test]
fn struct_values_parse() {
    fte_ok("point p = { 1, 2 };\n");
    fte_ok("point p = { .x = 1, .y = 2 };\n");
    let parsed = fte_ok("struct { float x; } seats[4], *seat;\n");
    let decl = parsed
        .syntax()
        .children()
        .find_map(ast::StructDecl::cast)
        .unwrap();
    let names: Vec<_> = decl
        .declarators()
        .map(|d| d.name().unwrap().text_lossless())
        .collect();
    assert_eq!(names, ["seats", "seat"]);
}

// spec FTEF-7.1: `typedef` names any type: a pointer to a tagged struct, a function type, an
// enum, an array.
// spec FTEF-7.2: a typedef's name is a type wherever a type is.
#[test]
fn typedefs_of_any_type_parse() {
    for source in [
        "typedef struct json_s *json_t;\n",
        "typedef void(entity) callback_t;\n",
        "typedef enum { RED, GREEN } colour_t;\n",
        "typedef float vec2[2];\n",
        "typedef int handle_t;\n",
    ] {
        fte_ok(source);
    }
    fte_ok("typedef int handle_t;\nhandle_t h;\nvoid(handle_t a) f;\n");
}

// spec FTEF-19.9: a `typedef` in a function body is a definition statement there, under FTEQCC
// only
#[test]
fn a_typedef_in_a_body_is_a_definition() {
    let parsed = fte_ok("void() f = { typedef float t; t x; };\n");
    assert!(
        parsed
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::TypedefDecl)
    );
    assert!(
        !parse(b"void() f = { typedef float t; };\n")
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::TypedefDecl)
    );
}

// spec FTEF-8.2: `enumflags` takes an element type and explicit values, and an enum may be
// written as a parameter's type.
#[test]
fn enumflags_parse() {
    fte_ok("enumflags { A, B, C };\n");
    fte_ok("enumflags : int { A = 4, B };\n");
    fte_ok("float(optional enumflags:float{WP_A, WP_B} flags) whichpack;\n");
}

// spec FTEF-8.4: an `enum` or `enumflags` written as a type has its body; the enum is named by
// its type name alone, so `enum colour_e c;` and `typedef enum colour_e C;` are errors
#[test]
fn an_enum_type_has_its_body() {
    assert_eq!(fte_errors("enum colour_e c;\n"), [Code::E0204]);
    assert_eq!(fte_errors("enumflags flags_e c;\n"), [Code::E0204]);
    assert_eq!(fte_errors("typedef enum colour_e C;\n"), [Code::E0204]);
    assert_eq!(fte_errors("enum colour_e { RED };\ncolour_e c;\n"), []);
    assert_eq!(fte_errors("typedef enum colour_e { RED } C;\n"), []);
}

// spec FTEF-9.1: `class Name : Parent { ... };` and the forward `class Name;`, after any
// declaration qualifiers.
// spec FTEF-9.2: member functions are `virtual`, `nonvirtual` or `static`, and `virtual:` sets
// the default.
// spec FTEF-9.3: data members and access labels.
// spec FTEF-9.4: a constructor without a return type, and members defined in the class body.
#[test]
fn classes_parse() {
    let source = "class Base;\n\
                  class Base { float x; };\n\
                  static class Thing : Base\n\
                  {\n\
                  public:\n\
                  \tvoid Thing(void);\n\
                  \tvirtual void Think(void);\n\
                  \tnonvirtual float Count(void);\n\
                  \tstatic Thing Make(float);\n\
                  private:\n\
                  \tentity m_owner;\n\
                  \tfloat m_list[4];\n\
                  virtual:\n\
                  \tvoid Touch(void) { m_owner = world; };\n\
                  };\n";
    let parsed = fte_ok(source);
    let classes: Vec<ast::ClassDecl> = parsed
        .syntax()
        .children()
        .filter_map(ast::ClassDecl::cast)
        .collect();
    assert_eq!(classes.len(), 3);
    assert!(!classes[0].has_body());
    assert_eq!(classes[2].name().unwrap().text(), b"Thing");
    assert_eq!(classes[2].parent().unwrap().text(), b"Base");
    assert_eq!(classes[2].members().count(), 7);
    fte_ok("class Ctor { Ctor(void); };\n");
}

// spec FTEF-9.4: `Class::member` defines a member function outside its class.
#[test]
fn member_definitions_parse() {
    let parsed = fte_ok("void\nThing::Think(void)\n{\n\tsuper::Think();\n};\n");
    let declarator = parsed
        .tree()
        .unwrap()
        .declarations()
        .next()
        .unwrap()
        .declarators()
        .next()
        .unwrap();
    assert_eq!(declarator.scope().unwrap().text(), b"Thing");
    assert_eq!(declarator.name().unwrap().text(), b"Think");
    fte_ok("Thing\nThing::Make(float a)\n{\n\treturn spawn(Thing);\n};\n");
}

// spec FTEF-10.1: `accessor Name : Base;` and with a body.
// spec FTEF-10.2: `get` and `set` members, indexed, inline and bound to functions.
// spec FTEF-10.3: a class may have `get` and `set` members.
#[test]
fn accessors_parse() {
    fte_ok("accessor strbuf : float;\n");
    let parsed = fte_ok(
        "accessor strbuf : float\n{\n\tinline get float asfloat[float idx] = {return idx;};\n\
         \tset string[float] = bufstr_set;\n\tget float length = buf_getsize;\n};\n",
    );
    let decl = parsed
        .syntax()
        .children()
        .find_map(ast::AccessorDecl::cast)
        .unwrap();
    let members: Vec<_> = decl.members().collect();
    assert_eq!(members.len(), 3);
    assert_eq!(members[0].name().unwrap().text(), b"asfloat");
    assert_eq!(members[0].index_name().unwrap().text(), b"idx");
    assert!(members[0].body().is_some());
    assert!(members[1].is_set());
    assert!(members[2].value().is_some());
    fte_ok("class Box { get float size = GetSize; };\n");
}

// spec FTEF-11.1: `__variant` is a type, and a pointer to it.
#[test]
fn variant_parses() {
    fte_ok("__variant(float n) externvalue;\n");
    fte_ok("void(__variant *p) f;\n");
}

// spec FTEF-15.1: qualifiers, with or without `__`, and `__deprecated("why")`, before a type;
// a name alone at file level is still not a type.
#[test]
fn declaration_qualifiers_parse() {
    for source in [
        "noref float x;\n",
        "static float x;\n",
        "shared float x;\n",
        "extern void() f;\n",
        "__used float x;\n",
        "__unused float x;\n",
        "__deprecated(\"use y\") float x;\n",
        "__deprecated float x;\n",
        "__wrap void() f = { };\n",
        "__accumulate void() f = { };\n",
        "nonstatic float x;\n",
        "strip float x;\n",
    ] {
        fte_ok(source);
    }
    assert_eq!(fte_errors("foo = 1;\n"), [Code::E0203]);
    // A C type word starts a declaration, so one with no name is the declaration's error.
    assert_eq!(fte_errors("int = 5;\n"), [Code::E0201]);
    assert_eq!(fte_errors("unsigned = 2;\n"), [Code::E0201]);
}

// spec FTEF-19.21: after a qualifier the type may be left out, at file level and in a body
#[test]
fn a_qualifier_may_leave_the_type_out() {
    for source in [
        "const x = 5;\n",
        "static x;\n",
        "nosave x, y;\n",
        "var x[2];\n",
        "void() f = { const x = 5; };\n",
    ] {
        assert_eq!(fte_errors(source), [], "{source}");
    }
}

// spec FTEF-15.2: a function returns any type and may be initialised with a value.
#[test]
fn function_return_types_and_values_parse() {
    fte_ok("jsonnode(string) json_parse = #0:json_parse;\n");
    fte_ok("entity*(int n) find_list = #0;\n");
    fte_ok("void() alias = original;\n");
    fte_ok("void() nothing = 0;\n");
}

// spec FTEF-15.3: parameters without names, with qualifiers, by reference, as function
// pointers, as arrays, with defaults, a typed `...` and a counted `...`.
#[test]
fn full_parameters_parse() {
    for source in [
        "float(float) rint;\n",
        "float(void) none;\n",
        "void(optional float a, __out float b, __in float c, __inout float d) f;\n",
        "void(float &out) f;\n",
        "void(float (*cb)(float)) f;\n",
        "void(float list[], int n) f;\n",
        "void(float a = 1, float b = a) f;\n",
        "void(string fmt, float ...) f;\n",
        "void(string fmt, ...count) f;\n",
    ] {
        fte_ok(source);
    }
}

// spec FTEF-15.4: a function type may have more than eight parameters, up to 136.
#[test]
fn many_parameters_parse() {
    let nine: Vec<String> = (0..9).map(|i| format!("float p{i}")).collect();
    fte_ok(&format!("void({}) f;\n", nine.join(", ")));
    let too_many: Vec<String> = (0..137).map(|i| format!("float p{i}")).collect();
    assert_eq!(
        fte_errors(&format!("void({}) f;\n", too_many.join(", "))),
        [Code::E0505]
    );
}

// spec FTEF-16.1: locals of class, pointer and C types; a product is no declaration.
#[test]
fn locals_by_shape() {
    assert!(declares_local("Thing t = spawn(Thing);"));
    assert!(declares_local("Thing *t;"));
    assert!(declares_local("int i = 0;"));
    assert!(declares_local("unsigned int i;"));
    assert!(declares_local("static float s;"));
    assert!(declares_local("float(float) fn = rint;"));
    assert!(!declares_local("a * b >> c;"));
    assert!(!declares_local("a * b + c;"));
    // A product alone ends where a declarator does, so it is read as one.
    assert!(declares_local("a * b;"));
}

// spec FTEF-16.2: casts to any type, and the shapes that stay parenthesised calls.
#[test]
fn casts_parse() {
    assert_eq!(first_expr("e = (Thing)e;"), "(= e (cast e))");
    assert_eq!(first_expr("a = (int)b;"), "(= a (cast b))");
    assert_eq!(first_expr("a = (float(entity e))b;"), "(= a (cast b))");
    assert_eq!(first_expr("e = (Thing *)e;"), "(= e (cast e))");
    // Shapes vanilla accepts too, which stay parenthesised expressions.
    for (stmt, want) in [
        ("a = (visible(e));", "(= a ((call visible e)))"),
        ("a = (sin(b * 2));", "(= a ((call sin (* b 2))))"),
        ("a = (b) - c;", "(= a (- (b) c))"),
    ] {
        let parsed = fte(&in_body(stmt));
        assert_eq!(parsed.errors().count(), 0, "{stmt}");
        let Some(ast::Stmt::Expr(expr)) = statements(&parsed).first().cloned() else {
            panic!("not an expression statement: {stmt}");
        };
        assert_eq!(sexpr(&expr.expr().unwrap()), want, "{stmt}");
    }
    fte_ok(&in_body("a = (vector){1, 2, 3};"));
}

// spec FTEF-16.3: `[x, y, z]` builds a vector from one to three expressions.
#[test]
fn vector_constructors_parse() {
    assert_eq!(
        first_expr("e.origin = [a, b + 1, c];"),
        "(= (. e origin) [a (+ b 1) c])"
    );
    assert_eq!(first_expr("e.origin = [a, b];"), "(= (. e origin) [a b])");
    fte_ok("const vector v = [0.0f, 0.0f, 0.0f];\n");
    assert!(!fte_errors(&in_body("a = [1, 2, 3, 4];")).is_empty());
}

// spec FTEF-16.4: `Class::member`, `super::member` and `::name`.
#[test]
fn scoped_names_parse() {
    let parsed = fte_ok(&in_body(
        "e.think = Thing::Think;\nsuper::Save(a);\na = ::b;",
    ));
    let mut scopes = Vec::new();
    for node in parsed.syntax().descendants().filter_map(ast::NameRef::cast) {
        if let Some(scope) = node.scope() {
            scopes.push(format!(
                "{}::{}",
                scope.text_lossless(),
                node.name().unwrap().text_lossless()
            ));
        }
    }
    assert_eq!(scopes, ["Thing::Think", "super::Save"]);
}

// spec FTEF-16.5: `if not (e)`.
#[test]
fn if_not_parses() {
    let parsed = fte_ok(&in_body("if not (a)\n\tb = 1;"));
    assert!(matches!(
        statements(&parsed).first(),
        Some(ast::Stmt::If(_))
    ));
}

// spec FTEF-16.6: a call's result may be indexed, called or have a member selected.
#[test]
fn postfix_after_calls_parses() {
    assert_eq!(first_expr("a = f().b;"), "(= a (. (call f) b))");
    assert_eq!(first_expr("a = f()[1];"), "(= a (index (call f) 1))");
}

// spec FTEF-16.8: adjacent string literals are one literal.
#[test]
fn adjacent_strings_parse() {
    let parsed = fte_ok("string s = \"a\" \"b\"\n\t\"c\";\n");
    let literals = parsed
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::Literal)
        .count();
    assert_eq!(literals, 1);
}

// spec FTEF-16.9: `for` clauses with several expressions and a declaration after a comma.
#[test]
fn for_clause_lists_parse() {
    fte_ok(&in_body("for (a = 3, b = 0; a; a--) { }"));
    fte_ok(&in_body("for (a = 0, int i = 1; i <= 3; i++) { }"));
}

// spec FTEF-16.10: a call's result may be assigned to under FTEQCC, not in vanilla.
#[test]
fn assignment_to_a_call_parses() {
    fte_ok(&in_body("a = f() = b;"));
    assert_eq!(
        parse(in_body("a = f() = b;").as_bytes())
            .errors()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [Code::E0508]
    );
}

// spec FTEF-16.12: a `case` label may stand in a block nested in the `switch` body.
#[test]
fn nested_case_labels_parse() {
    fte_ok(&in_body(
        "switch (a) {\ncase 3:\n\tif (b) {\n\tcase 2:\n\t\tc = 1;\n\t}\n\tbreak;\n}",
    ));
}

// spec FTEF-18.1: a global's initialiser may be any expression.
#[test]
fn expression_initialisers_parse() {
    let parsed = fte("float A = 1, B = 2;\nfloat C = A | B;\n");
    assert_eq!(
        fte_errors("float A = 1, B = 2;\nfloat C = A | B;\n"),
        Vec::<Code>::new()
    );
    let value = parsed
        .tree()
        .unwrap()
        .declarations()
        .filter_map(|d| d.declarators().next())
        .find(|d| d.name().is_some_and(|n| n.text() == b"C"))
        .and_then(|d| d.initializer());
    let Some(ast::Initializer::Value(expr)) = value else {
        panic!("C has no value: {value:?}");
    };
    assert_eq!(sexpr(&expr), "(| A B)");
}

// spec FTEF-18.2: a function-typed local may be given a body inside a function body.
#[test]
fn nested_function_bodies_parse() {
    for body in [
        "local void() g = { return; };\ng();",
        "void() g = { return; };\ng();",
    ] {
        let parsed = fte(&in_body(body));
        assert_eq!(fte_errors(&in_body(body)), Vec::<Code>::new(), "{body}");
        assert!(matches!(
            statements(&parsed).first(),
            Some(ast::Stmt::Local(_))
        ));
        let bodies = parsed
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::FunctionBody)
            .count();
        assert_eq!(bodies, 2, "{body}");
    }
}

// spec FTEF-15.8: a declaration's declarators may end with a `,` before the `;`, at file level
// and in a body, every declarator before it kept; a second `,` is no name; the vanilla parser
// refuses the `,`
#[test]
fn a_comma_may_end_the_declarators() {
    for source in [
        "void() f = #5, ;\n",
        "void() f = #5,;\n",
        "void() g, f = #5,;\n",
        "float a, b, ;\n",
    ] {
        fte_ok(source);
    }
    let parsed = fte("void() g, f = #5,;\n");
    let names: Vec<Vec<u8>> = parsed
        .tree()
        .into_iter()
        .flat_map(|tree| tree.declarations().collect::<Vec<_>>())
        .flat_map(|decl| decl.declarators().collect::<Vec<_>>())
        .filter_map(|declarator| declarator.name().map(|name| name.text().to_vec()))
        .collect();
    assert_eq!(names, [b"g".to_vec(), b"f".to_vec()]);
    fte_ok(&in_body("local float x, ;\nx = 1;"));
    assert!(!fte_errors("void() f = #5, , ;\n").is_empty());
}

// spec FTEF-19.26: under `qccx` `@e` is a unary term; without it `@` is none, and `^^` is no
// operator
#[test]
fn at_under_qccx_and_caret_caret() {
    let qccx = qc_lexer::LexFlags::default().with(qc_lexer::LexFlag::Qccx, true);
    let source = b"float x;\nstring s;\nvoid() g = { s = @x; };\n";
    let flagged = crate::parse_preprocessed_flagged(source, Dialect::Fteqcc, &[(0, qccx)], &[]);
    assert_eq!(flagged.errors().count(), 0, "{:?}", flagged.diagnostics());
    assert!(
        flagged
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::UnaryExpr)
    );
    let plain = crate::parse_preprocessed(source, Dialect::Fteqcc);
    assert!(plain.errors().next().is_some());
    let caret = fte("float x;\nvoid() g = { x = x ^^ x; };\n");
    assert!(caret.errors().next().is_some());
    assert_eq!(
        fte("float x;\nvoid() g = { x = x ^ x; };\n")
            .errors()
            .count(),
        0
    );
    // The parse keeps the flags it was given.
    assert_eq!(flagged.lex_flags_at(5), qccx);
    assert_eq!(plain.lex_flags_at(5), qc_lexer::LexFlags::default());
}
