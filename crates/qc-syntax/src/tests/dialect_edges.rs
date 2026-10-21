// SPDX-License-Identifier: GPL-3.0-or-later

//! The edges of the full FTEQCC parse (`docs/spec/dialect-fteqcc.md`): the shapes on either
//! side of each rule that tells one construct from another, and the typed view of each node.

use crate::ast::{self, AstNode as _};
use crate::{Code, Dialect, Parse, SyntaxKind, SyntaxToken, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The codes of every diagnostic of `source` under the FTEQCC dialect, warnings included.
fn fte_codes(source: &str) -> Vec<Code> {
    fte(source).diagnostics().iter().map(|d| d.code).collect()
}

/// `body`, indented, as the statements of a function, with globals for it to use.
fn in_body(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
    format!("float a, b, c;\nentity e;\nvoid() f =\n{{\n{indented}}};\n")
}

/// The codes of every diagnostic of `body` as a function body under the FTEQCC dialect.
fn body_codes(body: &str) -> Vec<Code> {
    fte_codes(&in_body(body))
}

/// The kinds of the statements of the function `f` in `body`.
fn statement_kinds(body: &str) -> Vec<SyntaxKind> {
    let parsed = fte(&in_body(body));
    let block = parsed
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::FunctionBody)
        .and_then(|n| n.children().find(|c| c.kind() == SyntaxKind::Block))
        .unwrap();
    block
        .children()
        .map(|n| n.kind())
        .filter(|k| *k != SyntaxKind::Error)
        .collect()
}

/// Whether the first statement of `body` declares locals.
fn declares_local(body: &str) -> bool {
    statement_kinds(body).first() == Some(&SyntaxKind::LocalDecl)
}

/// The kinds of the top-level nodes of `source`, trivia and directives left out.
fn top_kinds(source: &str) -> Vec<SyntaxKind> {
    fte(source)
        .syntax()
        .children()
        .map(|n| n.kind())
        .filter(|k| *k != SyntaxKind::Directive)
        .collect()
}

/// The first node of kind `kind` in `source`.
fn first_node(source: &str, kind: SyntaxKind) -> crate::SyntaxNode {
    fte(source)
        .syntax()
        .descendants()
        .find(|n| n.kind() == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in {source}"))
}

/// The kinds of the non-trivia tokens directly in `node`.
fn token_kinds(node: &crate::SyntaxNode) -> Vec<SyntaxKind> {
    node.child_tokens()
        .map(|t| t.kind())
        .filter(|k| !k.is_trivia())
        .collect()
}

fn text(token: Option<SyntaxToken>) -> String {
    token.map(|t| t.text_lossless()).unwrap_or_default()
}

// spec FTEF-15.1: `const`, `var` and `nosave` before a declaration are qualifiers of their own
// kinds, the other qualifiers names, at file level and in a body
#[test]
fn qualifier_tokens_have_their_kinds() {
    for (source, kind) in [
        ("const float x = 1;\n", SyntaxKind::ConstKw),
        ("var float x;\n", SyntaxKind::VarKw),
        ("nosave float x;\n", SyntaxKind::NosaveKw),
        ("static float x;\n", SyntaxKind::Name),
    ] {
        let qualifier = first_node(source, SyntaxKind::Qualifier);
        assert_eq!(token_kinds(&qualifier), [kind], "{source}");
    }
    let local = first_node(&in_body("nosave float x;"), SyntaxKind::LocalDecl);
    let qualifier = local
        .children()
        .find(|n| n.kind() == SyntaxKind::Qualifier)
        .unwrap();
    assert_eq!(token_kinds(&qualifier), [SyntaxKind::NosaveKw]);
}

// spec FTEF-9.2: a class member's `const` and `var` qualifiers are of their own kinds
#[test]
fn class_member_qualifier_kinds() {
    let class = first_node(
        "class C { const float k; var float v; static float s; };\n",
        SyntaxKind::ClassDecl,
    );
    let kinds: Vec<Vec<SyntaxKind>> = class
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::Qualifier)
        .map(|q| token_kinds(&q))
        .collect();
    assert_eq!(
        kinds,
        [
            vec![SyntaxKind::ConstKw],
            vec![SyntaxKind::VarKw],
            vec![SyntaxKind::Name]
        ]
    );
}

// spec FTE-21.1: a statement declares locals only when it starts with a type, and a statement
// word names none, so `for x;` or `switch x;` declares no local of a type `for`
#[test]
fn statement_words_never_name_a_type() {
    for body in ["for x;", "switch x;", "break x;", "continue x;", "while x;"] {
        assert!(!declares_local(body), "{body}");
        assert!(!body_codes(body).is_empty(), "{body}");
    }
}

// spec FTEF-6.1: `union tag x;` in a body declares a local of the tagged type
#[test]
fn tagged_union_locals() {
    assert!(declares_local("union tag x;"));
    assert_eq!(body_codes("union tag x;"), []);
}

// spec FTEF-6.1: at file level `struct tag x;` declares a variable of the tagged type, not a
// struct; `struct tag;` and `struct tag { ... };` declare the struct
#[test]
fn tagged_struct_variables_and_struct_declarations() {
    assert_eq!(top_kinds("struct tag x;\n"), [SyntaxKind::Declaration]);
    assert_eq!(top_kinds("union tag x;\n"), [SyntaxKind::Declaration]);
    assert_eq!(top_kinds("struct tag;\n"), [SyntaxKind::StructDecl]);
    assert_eq!(top_kinds("struct tag : base;\n"), [SyntaxKind::StructDecl]);
    assert_eq!(
        top_kinds("struct tag { float a; };\nfloat b;\n"),
        [SyntaxKind::StructDecl, SyntaxKind::Declaration]
    );
    assert_eq!(
        top_kinds("struct { float a; };\nfloat b;\n"),
        [SyntaxKind::StructDecl, SyntaxKind::Declaration]
    );
    assert_eq!(fte_codes("struct tag { float a; };\nfloat b;\n"), []);
    // The declaration after the `;` is read on the same line too.
    assert_eq!(
        top_kinds("struct tag { float a; }; float b;\n"),
        [SyntaxKind::StructDecl, SyntaxKind::Declaration]
    );
    // A struct with no `;` after it is reported, and the next declaration still read.
    let source = "struct tag { float a; }\n.float b;\n";
    assert_eq!(fte_codes(source), [Code::E0204]);
    assert_eq!(
        top_kinds(source),
        [SyntaxKind::StructDecl, SyntaxKind::Declaration]
    );
}

// spec FTEF-7.1: `typedef struct { ... } Name;` and `typedef struct tag { ... } Name;` define
// the struct there; `typedef struct tag Name;` names a struct defined elsewhere
#[test]
fn typedef_struct_forms() {
    for source in [
        "typedef struct { float a; } T;\n",
        "typedef struct tag { float a; } T;\n",
        "typedef union { float a; } T;\n",
        "typedef union tag { float a; } T;\n",
    ] {
        let typedef = first_node(source, SyntaxKind::TypedefDecl);
        let direct: Vec<SyntaxKind> = typedef.children().map(|n| n.kind()).collect();
        assert_eq!(direct, [SyntaxKind::StructType], "{source}");
        assert_eq!(fte_codes(source), [], "{source}");
        let decl = ast::TypedefDecl::cast(typedef).unwrap();
        assert_eq!(text(decl.name()), "T", "{source}");
    }
    for source in ["typedef struct tag T;\n", "typedef union tag T;\n"] {
        let typedef = first_node(source, SyntaxKind::TypedefDecl);
        let direct: Vec<SyntaxKind> = typedef.children().map(|n| n.kind()).collect();
        assert_eq!(direct, [SyntaxKind::SimpleType], "{source}");
        assert_eq!(fte_codes(source), [], "{source}");
    }
    // `typedef struct { ... } *P;` names a pointer to it.
    let source = "typedef struct { float a; } *P;\n";
    assert_eq!(fte_codes(source), []);
    let typedef = first_node(source, SyntaxKind::TypedefDecl);
    let stars = token_kinds(&typedef)
        .into_iter()
        .filter(|k| *k == SyntaxKind::Star)
        .count();
    assert_eq!(stars, 1);
    // `typedef enum { ... } Name;` defines the enum there.
    for source in [
        "typedef enum { A, B } E;\n",
        "typedef enumflags { A, B } F;\n",
    ] {
        let typedef = first_node(source, SyntaxKind::TypedefDecl);
        let direct: Vec<SyntaxKind> = typedef.children().map(|n| n.kind()).collect();
        assert_eq!(direct, [SyntaxKind::EnumDecl], "{source}");
        assert_eq!(fte_codes(source), [], "{source}");
    }
    // An array typedef, `typedef float V[3];`.
    let source = "typedef float V[3];\n";
    assert_eq!(fte_codes(source), []);
    assert!(
        first_node(source, SyntaxKind::TypedefDecl)
            .children()
            .any(|n| n.kind() == SyntaxKind::ArraySuffix)
    );
}

// spec FTE-15.1: an enum's last enumerator may have a `,` after it, and an enum may have none
#[test]
fn enum_trailing_comma_and_empty_enum() {
    assert_eq!(fte_codes("enum { A, B, };\n"), []);
    assert_eq!(fte_codes("enum { };\n"), []);
    let decl = ast::EnumDecl::cast(first_node("enum { A, B, };\n", SyntaxKind::EnumDecl)).unwrap();
    assert_eq!(decl.enumerators().count(), 2);
}

// spec FTE-15.4: a named enum's name is its type name, after `class` when there is one, and
// an anonymous enum has none, its base type included
#[test]
fn enum_names() {
    let name = |source: &str| {
        let decl = ast::EnumDecl::cast(first_node(source, SyntaxKind::EnumDecl)).unwrap();
        text(decl.name())
    };
    assert_eq!(name("enum Color { RED };\n"), "Color");
    assert_eq!(name("enum class Color : float { RED };\n"), "Color");
    assert_eq!(name("enum : MyType { RED };\n"), "");
    assert_eq!(name("enum { RED };\n"), "");
}

// spec FTEF-15.6: `T a, *b` declares `b` a pointer, whose type is a pointer to `T`
#[test]
fn pointer_type_inner() {
    let source = "float *p;\n";
    assert_eq!(fte_codes(source), []);
    let decl = fte(source).tree().unwrap().declarations().next().unwrap();
    let Some(ast::Type::Pointer(pointer)) = decl.ty() else {
        let shape = first_node(source, SyntaxKind::Declaration);
        panic!("not a pointer: {}", shape.debug_tree());
    };
    let Some(ast::Type::Simple(inner)) = pointer.inner() else {
        panic!("no inner type");
    };
    assert_eq!(inner.kind(), Some(ast::SimpleTypeKind::Float));
}

/// The source text of each "Statement does not do anything" warning in `body`.
fn pointless(body: &str) -> Vec<String> {
    let source = in_body(body);
    let parsed = fte(&source);
    assert_eq!(parsed.errors().count(), 0, "{source}");
    parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code == Code::W0313)
        .map(|d| String::from_utf8_lossy(&source.as_bytes()[d.span.range()]).into_owned())
        .collect()
}

// spec FTEF-18.25: the first and last clauses of a `for` start with the flag clear, whatever an
// earlier condition set, and leave it as it was before them; the condition keeps what it sets
#[test]
fn for_clauses_and_the_useful_flag() {
    assert_eq!(
        pointless("if (f()) c = 1;\nfor (a; a < 1; b) c = 1;"),
        ["a", "b"]
    );
    assert_eq!(
        pointless("for (a = 1; a; b++) c = 1;\nif (a, b) c = 1;"),
        ["a"]
    );
    assert_eq!(
        pointless("for (a = 1; f(); b++) c = 1;\nif (a, b) c = 1;"),
        Vec::<String>::new()
    );
    // The condition starts with the flag an earlier condition left, so its `a` is not warned.
    assert_eq!(
        pointless("if (f()) c = 1;\nfor (; a, a < 1; ) c = 1;"),
        Vec::<String>::new()
    );
    assert_eq!(pointless("for (; a, a < 1; ) c = 1;"), ["a"]);
}

// spec FTEF-16.9: the first clause of a `for` may declare locals with `local`, a type word or a
// C type word
#[test]
fn for_clause_declarations() {
    for body in [
        "for (local float i = 0; i < 3; i++) c = 1;",
        "for (float i = 0; i < 3; i++) c = 1;",
        "for (int i = 0; i < 3; i++) c = 1;",
    ] {
        assert_eq!(body_codes(body), [], "{body}");
        let clause = first_node(&in_body(body), SyntaxKind::ForClause);
        assert!(
            clause.children().any(|n| n.kind() == SyntaxKind::LocalDecl),
            "{body}"
        );
    }
}

// spec FTEF-18.25: only a cast to `void` itself makes a discarded expression `void`
#[test]
fn only_a_void_cast_is_void() {
    assert_eq!(pointless("(void) a;"), Vec::<String>::new());
    assert_eq!(pointless("(float) a;"), ["(float) a"]);
    assert_eq!(pointless("(void()) a;"), ["(void()) a"]);
    assert_eq!(pointless("(vector) a;"), ["(vector) a"]);
}

// spec FTEF-16.8: only string literals join, so a number before a string is no literal of
// both
#[test]
fn only_strings_join() {
    assert!(!body_codes("a = 1 \"b\";").is_empty());
    assert!(!body_codes("a = \"b\" 1;").is_empty());
    assert_eq!(body_codes("a = \"b\" \"c\";"), []);
}

// spec FTEF-16.4: `::` names the global scope only before a name
#[test]
fn global_scope_needs_a_name() {
    assert_eq!(body_codes("a = ::b;"), []);
    assert!(body_codes("a = ::1;").contains(&Code::E0201));
}

// spec FTEF-4.4: `*e` dereferences, and is assigned to; `&e` takes an address, which is not
// spec QC-27.10: an assignment's left side must be assignable
#[test]
fn dereference_is_assignable() {
    assert_eq!(body_codes("*e = 1;"), []);
    assert_eq!(body_codes("&e = 1;"), [Code::E0508]);
    assert_eq!(body_codes("~a = 1;"), [Code::E0508]);
}

// spec FTEF-16.11: `.member = value` in a brace initialiser names a member; any other word
// before `=` is an error
#[test]
fn designated_initialisers() {
    assert_eq!(fte_codes("vector v = { .x = 1, .y = 2 };\n"), []);
    assert!(fte_codes("vector v = { a b = 1 };\n").contains(&Code::E0204));
    assert!(fte_codes("vector v = { ~a = 1 };\n").contains(&Code::E0508));
}

// spec FTEF-9.1: a class with a body has one, a forward declaration has none
// spec FTEF-10.3: a class's `get` and `set` members are its accessor members
#[test]
fn class_views() {
    let classes: Vec<ast::ClassDecl> = fte("class A;\nclass B { };\nclass C { get float size = Size; set float size = SetSize; float x; };\n")
        .syntax()
        .children()
        .filter_map(ast::ClassDecl::cast)
        .collect();
    assert_eq!(classes.len(), 3);
    assert!(!classes[0].has_body());
    assert!(classes[1].has_body());
    assert_eq!(classes[1].accessor_members().count(), 0);
    let members: Vec<ast::AccessorMember> = classes[2].accessor_members().collect();
    assert_eq!(members.len(), 2);
    assert!(!members[0].is_set());
    assert!(members[1].is_set());
    assert_eq!(text(members[0].name()), "size");
    assert_eq!(classes[2].members().count(), 1);
}

// spec FTEF-10.1: an accessor's name is the name after `accessor`, its base type aside
// spec FTEF-10.2: a `get` member is no `set` member, whatever names it holds
#[test]
fn accessor_views() {
    let source = "accessor buf : float { get float len = size; inline get float at[float i] = { return i; }; };\n";
    assert_eq!(fte_codes(source), []);
    let decl = ast::AccessorDecl::cast(first_node(source, SyntaxKind::AccessorDecl)).unwrap();
    assert_eq!(text(decl.name()), "buf");
    let members: Vec<ast::AccessorMember> = decl.members().collect();
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|m| !m.is_set()));
    let decl = ast::AccessorDecl::cast(first_node("accessor strbuf;\n", SyntaxKind::AccessorDecl))
        .unwrap();
    assert_eq!(text(decl.name()), "strbuf");
}

// spec FTEF-10.1: an accessor's body lists its members; the statement names no separator, and
// this profile reads a `;` or a `,` after each
#[test]
fn accessor_member_separators() {
    for source in [
        "accessor buf : float { get float len = size; set float len = resize; };\n",
        "accessor buf : float { get float len = size, set float len = resize };\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
        let decl = ast::AccessorDecl::cast(first_node(source, SyntaxKind::AccessorDecl)).unwrap();
        assert_eq!(decl.members().count(), 2, "{source}");
    }
}

// spec FTEF-16.1: a statement declares locals when it starts with a C type word or a
// qualifier, or a name followed by a name, by `*`s and a name that ends a declarator, or by a
// parameter list and a name; a parameter list of nested lists is one list
#[test]
fn local_declaration_shapes() {
    for body in [
        "int x;",
        "long x;",
        "static float x;",
        "Thing t;",
        "Thing *t = world;",
        "Thing **t, *u;",
        "Thing *t[2];",
        "Thing(float x) cb;",
        "Thing(float(float) g, float h) cb;",
        "Thing(float(float), float) cb;",
    ] {
        assert!(declares_local(body), "{body}");
        assert_eq!(body_codes(body), [], "{body}");
    }
    for body in [
        "a * b + c;",
        "a * (b) + c;",
        "a(b);",
        "a(b) + c;",
        "a(b) = c;",
        "a;",
        "a = b;",
    ] {
        assert!(!declares_local(body), "{body}");
    }
    // `Type *(params) name` is none of the shapes, so it is read as an expression.
    assert!(!declares_local("Thing *(float) cb;"));
    // A `;`, `{` or `}` ends the scan for the list's `)`: `T(a; b) c` is no declaration.
    assert!(!declares_local("Thing(a; b) c;"));
    assert!(!declares_local("Thing(a { b) c;"));
    assert!(!declares_local("Thing(a } b) c;"));
}

// spec FTEF-16.2: a cast to a type name, to a pointer to one, or to a function type of one,
// before a term; a parenthesised product or call is no cast
#[test]
fn cast_shapes() {
    let cast = |stmt: &str| {
        let source = in_body(stmt);
        fte(&source)
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CastExpr)
    };
    for stmt in [
        "a = (Thing) b;",
        "a = (Thing *) b;",
        "a = (Thing **) b;",
        "a = (Thing(float)) b;",
        "a = (Thing(float(float) g)) b;",
        "a = (Thing(float x, ...)) b;",
        "a = (Thing()) b;",
        "a = (Thing(...)) b;",
        "a = (Thing(.float f)) b;",
        "a = (Thing(optional float f)) b;",
        "a = (Thing(Other o)) b;",
        "a = (Thing(Other *o)) b;",
        "a = (Thing(int)) b;",
        "a = (Thing) (b);",
        "a = (Thing) !b;",
    ] {
        assert!(cast(stmt), "{stmt}");
        assert_eq!(body_codes(stmt), [], "{stmt}");
    }
    for stmt in [
        "a = (b);",
        "a = (b) - c;",
        "a = (b * (c));",
        "a = (b * c);",
        "a = (b * c) - d;",
        "a = (f(b));",
        "a = (f(b))(c);",
        "a = (f(b)) - c;",
        "a = (f(1)) (c);",
        "a = (f(b c)) - d;",
    ] {
        assert!(!cast(stmt), "{stmt}");
    }
    // A `;` or `{` ends the scan for the function type's `)`.
    assert!(!cast("a = (Thing(Other o;)) b;"));
    assert!(!cast("a = (Thing(Other o {)) b;"));
}

// spec FTEF-18.28: a cycle whose two constant frames are equal runs either way
// spec QCD-3.12: E0212 only for a range against the cycle's direction
#[test]
fn equal_frame_cycles_are_no_error() {
    for header in ["[++ 1 .. 1]", "[-- 1 .. 1]", "[++ 1 .. 2]", "[-- 2 .. 1]"] {
        let source = format!("void() f = {header} {{}};\n");
        assert_eq!(fte_codes(&source), [], "{source}");
    }
    assert_eq!(fte_codes("void() f = [-- 1 .. 2] {};\n"), [Code::E0212]);
    assert_eq!(fte_codes("void() f = [++ 2 .. 1] {};\n"), [Code::E0212]);
}

// spec FTEF-9.2: `virtual:` and `nonvirtual:` set the default and `virtual` before a member
// qualifies it
// spec FTEF-9.3: `public:`, `private:` and `protected:` are access labels; a member named so
// is a member
// spec FTEF-10.3: `get` and `set` start accessor members, after `inline` too; `inline` before
// anything else qualifies a member
#[test]
fn class_member_kinds() {
    let members = |body: &str| -> Vec<SyntaxKind> {
        let indented: String = body.lines().map(|line| format!("\t{line}\n")).collect();
        let source = format!("class C {{\n{indented}}};\n");
        assert_eq!(fte_codes(&source), [], "{source}");
        first_node(&source, SyntaxKind::ClassDecl)
            .children()
            .map(|n| n.kind())
            .collect()
    };
    assert_eq!(
        members("public:\nprivate:\nprotected:\nvirtual:\nnonvirtual:"),
        [SyntaxKind::AccessLabel; 5]
    );
    assert_eq!(
        members("virtual void Think(void);\nfloat public;\nfloat get;"),
        [SyntaxKind::Declaration; 3]
    );
    assert_eq!(
        members(
            "get float a = A;\nset float a = B;\ninline get float b = C;\ninline set float b = D;"
        ),
        [SyntaxKind::AccessorMember; 4]
    );
    assert_eq!(
        members("inline float Twice(float x) { return x * 2; };"),
        [SyntaxKind::Declaration]
    );
    // A label word not followed by `:` is a name.
    assert_eq!(members("Thing public;"), [SyntaxKind::Declaration]);
}

// spec FTEF-18.12: `sizeof` takes a C type word or a tagged type as a type, and a name and
// `*`s as a type only inside its parentheses
#[test]
fn sizeof_types() {
    let sizeof_type = |stmt: &str| {
        let source = in_body(stmt);
        let node = first_node(&source, SyntaxKind::SizeofExpr);
        node.children().any(|n| ast::Type::can_cast(n.kind()))
    };
    for stmt in [
        "a = sizeof(int);",
        "a = sizeof(struct tag);",
        "a = sizeof(union tag);",
        "a = sizeof(..float);",
        "a = sizeof(float);",
        "a = sizeof(Thing *);",
    ] {
        assert!(sizeof_type(stmt), "{stmt}");
        assert_eq!(body_codes(stmt), [], "{stmt}");
    }
    for stmt in ["a = sizeof(b);", "a = sizeof b;", "a = sizeof(b) * c;"] {
        assert!(!sizeof_type(stmt), "{stmt}");
    }
    // Outside parentheses `Thing *` is the start of a product, which `)` cannot end.
    assert!(!body_codes("a = (sizeof Thing *);").is_empty());
}

// spec QCD-1.4: a state header with no body after it is reported, and the next declaration is
// still read
#[test]
fn a_state_header_without_a_body() {
    let source = "void() f = [1, g];\nvoid() h = {};\n";
    let declarations = fte(source)
        .syntax()
        .children()
        .filter(|n| n.kind() == SyntaxKind::Declaration)
        .count();
    assert_eq!(declarations, 2);
    assert!(fte_codes(source).contains(&Code::E0204));
}

// spec FTEF-15.4: a function type of exactly 136 parameters is no error
#[test]
fn the_parameter_limit_itself_is_allowed() {
    let params: Vec<String> = (0..136).map(|i| format!("float p{i}")).collect();
    assert_eq!(fte_codes(&format!("void({}) f;\n", params.join(", "))), []);
}

// spec FTEF-6.1: `struct tag` and `union tag` name a tagged type by its keyword
#[test]
fn tagged_type_keywords() {
    for (source, keyword) in [
        ("struct tag x;\n", SyntaxKind::StructKw),
        ("union tag x;\n", SyntaxKind::UnionKw),
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
        let ty = first_node(source, SyntaxKind::SimpleType);
        assert_eq!(token_kinds(&ty), [keyword, SyntaxKind::Name], "{source}");
    }
    // A struct written as a member's type, with or without a tag.
    for source in [
        "struct outer { struct { float a; } inner; };\n",
        "struct outer { struct tag { float a; } inner; };\n",
        "struct outer { union { float a; } inner; };\n",
    ] {
        assert_eq!(fte_codes(source), [], "{source}");
        let fields = first_node(source, SyntaxKind::StructField);
        let ty = fields
            .children()
            .find(|n| n.kind() == SyntaxKind::SimpleType)
            .unwrap();
        assert!(
            ty.children().any(|n| n.kind() == SyntaxKind::StructType),
            "{source}"
        );
    }
}
