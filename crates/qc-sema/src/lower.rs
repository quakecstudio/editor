// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-file lowering: the raw declarations a single file contributes, in source order.
//!
//! This is the semantic model's leaf query. It walks one file's syntax tree
//! (`qc_syntax::Parse`) and lists every name the file declares, with its type, kind and the
//! span of its name, plus the references each function body makes. It does no cross-file
//! resolution: that is [`crate::resolve`], which joins the files of a module in compilation
//! order (QC-23.3). Keeping this query per file is what lets an edit to one file re-lower
//! only that file (GOAL-1, the incremental model).

use qc_diag::Span;
use qc_lexer::atof;
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::{Dialect, Parse, SyntaxKind, SyntaxNode};

use crate::symbol::DefKind;
use crate::ty::{FunctionType, SimpleType, Type};

/// A name a file declares, before cross-file resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDecl {
    /// The declared name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// Its declared type.
    pub ty: Type,
    /// Whether it is a `local` (QC-11.10); at file level it is a global, function or field.
    pub local: bool,
    /// Whether the declaration has an initialiser (QC-11.5).
    pub initialized: bool,
    /// The value of an initialiser that is a single literal, which FTEQCC compares when a
    /// name is initialised twice (FTE-22.2); `None` for any other initialiser or none.
    pub literal: Option<LiteralValue>,
    /// For a function definition, the lowered body; otherwise `None`.
    pub body: Option<RawBody>,
    /// For a function definition with a `[frame, next]` header, the next-think name and its
    /// span (QC-13.6): resolution declares it as a global `void()` if new.
    pub next_think: Option<(String, Span)>,
    /// The names its initialiser uses, when that is not a function body, and the names its
    /// parameters' default values use (FTE-10.2, FTE-18.2), in source order; resolved in the
    /// global scope after the declaration.
    pub uses: Vec<RawUse>,
    /// For an FTEQCC `Class::member` declarator, the class (FTEF-9.4).
    pub scope: Option<String>,
    /// For an FTEQCC `class`, its parent and members (FTEF-9.1).
    pub class: Option<RawClass>,
    /// Whether the name is a type's: a class, struct, union, typedef, enum or accessor, which
    /// may be declared more than once (FTEF-7.2).
    pub type_name: bool,
}

impl RawDecl {
    /// A declaration of `name` at `span` of type `ty`, with no initialiser.
    fn new(name: String, span: Span, ty: Type) -> Self {
        Self {
            name,
            span,
            ty,
            local: false,
            initialized: false,
            literal: None,
            body: None,
            next_think: None,
            uses: Vec::new(),
            scope: None,
            class: None,
            type_name: false,
        }
    }

    /// The declaration of a type's name (FTEF-7.2).
    fn type_name(token: &qc_syntax::SyntaxToken) -> Self {
        let mut decl = Self::new(token.text_lossless(), token.span(), Type::Unknown);
        decl.type_name = true;
        decl
    }
}

/// An FTEQCC class's parent and members (FTEF-9).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawClass {
    /// The parent class's name, if any.
    pub parent: Option<String>,
    /// The members, in declaration order.
    pub members: Vec<RawMember>,
}

/// One member of an FTEQCC class (FTEF-9.2, FTEF-9.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMember {
    /// The member's name.
    pub name: String,
    /// The byte span of its name.
    pub span: Span,
    /// Its type.
    pub ty: Type,
    /// What it is.
    pub kind: MemberKind,
    /// Whether it has a body or value.
    pub initialized: bool,
    /// A member function's body written in the class.
    pub body: Option<RawBody>,
}

/// What a class member is (FTEF-9.2, FTEF-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    /// A data member: a field of the entity.
    Field,
    /// A `virtual` member function: a field of function type each instance may override.
    Virtual,
    /// A `nonvirtual` or `static` member function: a global named `Class::member`.
    Method,
}

/// The value of a literal initialiser (FTE-22.2). A float is kept as its bits, so two
/// initialisers are the same value exactly when FTEQCC's word-by-word comparison finds them
/// equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiteralValue {
    /// A float literal (QC-4).
    Float(u32),
    /// A vector literal (QC-5).
    Vector([u32; 3]),
    /// A string literal as written, quotes included (QC-6).
    String(String),
}

impl LiteralValue {
    /// The value of a literal initialiser, if `init` is one.
    fn of(init: Option<&ast::Initializer>) -> Option<Self> {
        let Some(ast::Initializer::Value(ast::Expr::Literal(literal))) = init else {
            return None;
        };
        let token = literal.token()?;
        let text = token.text_lossless();
        match literal.kind()? {
            ast::LiteralKind::Float => Some(Self::Float(atof(text.as_bytes()).to_bits())),
            ast::LiteralKind::Vector => {
                let inner = text.trim_matches('\'');
                let mut parts = inner.split_ascii_whitespace().map(|p| atof(p.as_bytes()));
                let mut value = [0_u32; 3];
                for word in &mut value {
                    *word = parts.next().unwrap_or(0.0).to_bits();
                }
                Some(Self::Vector(value))
            }
            ast::LiteralKind::String => Some(Self::String(text)),
            ast::LiteralKind::FrameRef => None,
        }
    }
}

/// A function body's parameters, locals and the names it uses (QC-13).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawBody {
    /// The parameters, in order, with each one's name span (QC-13.2).
    pub params: Vec<RawParam>,
    /// The `local` declarations, in source order (QC-11.10).
    pub locals: Vec<RawLocal>,
    /// Every name used as a value in the body, in source order (QC-12.5).
    pub uses: Vec<RawUse>,
}

/// One parameter of a function definition (QC-13.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawParam {
    /// The parameter name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// The parameter type.
    pub ty: Type,
}

/// One `local` declaration inside a body (QC-11.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawLocal {
    /// The local's name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// The local's type.
    pub ty: Type,
    /// Whether it has an initialiser (QC-11.10).
    pub initialized: bool,
    /// Whether it is a `local` field or function, which is an error (QCD-3.8, QC-11.9).
    pub bad_field_or_function: bool,
}

/// One use of a name (QC-12.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawUse {
    /// The name used.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// How the name is used.
    pub kind: UseKind,
    /// The scope of an FTEQCC `Scope::name`: a class, `super`, or an enum (FTEF-16.4).
    pub scope: Option<String>,
    /// For a member use, what the access is made on.
    pub base: MemberBase,
}

impl RawUse {
    /// A use of `name` at `span`.
    fn new(name: String, span: Span, kind: UseKind) -> Self {
        Self {
            name,
            span,
            kind,
            scope: None,
            base: MemberBase::Unknown,
        }
    }
}

/// How a name is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseKind {
    /// As a value: a global, a parameter or a local (QC-12.5).
    Value,
    /// In an FTEQCC dialect, as the selector of a member access `e.name`: an entity field, a
    /// class member, or a vector member, `.length` or a struct field (FTE-9, FTE-13, FTE-14,
    /// FTEF-9.3).
    Member,
}

/// What a member access is made on, as far as the syntax tells (FTEF-17.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberBase {
    /// Something whose type is not known from its shape.
    Unknown,
    /// A name, whose definition's type is the base's.
    Name(String),
    /// A cast to a type.
    Type(Type),
    /// A call of a named function, whose return type is the base's.
    Call(String),
}

/// Every declaration a file contributes, in source order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileDecls {
    /// The file-level and body declarations, in source order.
    pub decls: Vec<RawDecl>,
    /// The field names of the structs and accessors the file defines (FTE-14.1, FTEF-6,
    /// FTEF-10): a member access may select one without naming a global.
    pub members: Vec<String>,
}

/// Lowers one parsed file to its raw declarations (QC-11, QC-13), in `dialect`.
///
/// Implements the declaration grammar of QC-11.1 and the function-body shape of QC-13.1, and
/// in an FTEQCC dialect the declaration forms of `docs/spec/dialect-fteqcc-rerelease.md`:
/// `enum` names and a `typedef` type name become global definitions (FTE-10, FTE-14,
/// FTE-15), a member access's selector is a member use that resolution tells apart from a
/// vector member, `.length` or a struct field (FTE-9, FTE-13, FTE-14), and the names of a
/// value initialiser and of parameter default values are recorded (FTE-10.2, FTE-18.2). The
/// full dialect's classes, structs, unions and accessors are lowered with their members, and a
/// `Class::member` definition with its class (FTEF-17). Never fails: a file with syntax errors
/// yields whatever declarations parsed (QCD-1.4).
#[must_use]
pub fn lower_file(parse: &Parse, dialect: Dialect) -> FileDecls {
    let mut out = FileDecls::default();
    let Some(tree) = parse.tree() else {
        return out;
    };
    let fte = dialect.has_fteqcc_extensions();
    // Walk the top-level nodes in source order, so a name an `enum` or `typedef` declares is
    // visible to the declarations after it (QC-12.5, QC-23.3). Only an FTEQCC parse has
    // `enum` and `typedef` nodes.
    for node in tree.syntax().children() {
        match node.kind() {
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_declaration(&decl, fte, &mut out.decls);
                }
            }
            SyntaxKind::EnumDecl => {
                if let Some(en) = ast::EnumDecl::cast(node) {
                    lower_enum(&en, &mut out.decls);
                }
            }
            SyntaxKind::TypedefDecl => {
                if let Some(td) = ast::TypedefDecl::cast(node) {
                    lower_typedef(&td, &mut out);
                }
            }
            SyntaxKind::ClassDecl => {
                if let Some(class) = ast::ClassDecl::cast(node) {
                    lower_class(&class, fte, &mut out);
                }
            }
            SyntaxKind::StructDecl => {
                if let Some(st) = ast::StructDecl::cast(node) {
                    if let Some(token) = st.name() {
                        out.decls.push(RawDecl::type_name(&token));
                    }
                    for field in st.fields() {
                        record_fields(&field, &mut out.members);
                    }
                }
            }
            SyntaxKind::AccessorDecl => {
                if let Some(acc) = ast::AccessorDecl::cast(node) {
                    lower_accessor(&acc, fte, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// An `enum`'s names become global float constants (FTE-15.1, FTE-15.3), and a named enum's
/// type name is recorded as a typedef's is (FTE-15.4).
fn lower_enum(en: &ast::EnumDecl, out: &mut Vec<RawDecl>) {
    if let Some(token) = en.name() {
        out.push(RawDecl::type_name(&token));
    }
    for node in en.enumerators() {
        let Some(token) = node.child_tokens().find(|t| t.kind() == SyntaxKind::Name) else {
            continue;
        };
        let mut decl = RawDecl::new(
            token.text_lossless(),
            token.span(),
            Type::Simple(SimpleType::Float),
        );
        decl.initialized = true;
        out.push(decl);
    }
}

/// The names of the fields of a struct node, recorded as member names (FTE-14.1, FTEF-6).
fn record_fields(field: &SyntaxNode, members: &mut Vec<String>) {
    members.extend(
        field
            .child_tokens()
            .filter(|t| t.kind() == SyntaxKind::Name)
            .map(|t| t.text_lossless()),
    );
}

/// A `typedef` declares the type name (FTE-14.1, FTEF-7.1), recorded as a global so a use of
/// it resolves. A struct's fields are member names, not globals, and an enum's names are
/// constants.
fn lower_typedef(td: &ast::TypedefDecl, out: &mut FileDecls) {
    for field in td
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::StructField)
    {
        record_fields(&field, &mut out.members);
    }
    if let Some(en) = td.syntax().children().find_map(ast::EnumDecl::cast) {
        for node in en.enumerators() {
            if let Some(token) = node.child_tokens().find(|t| t.kind() == SyntaxKind::Name) {
                let mut decl = RawDecl::new(
                    token.text_lossless(),
                    token.span(),
                    Type::Simple(SimpleType::Float),
                );
                decl.initialized = true;
                out.decls.push(decl);
            }
        }
    }
    let Some(token) = td.name() else {
        return;
    };
    out.decls.push(RawDecl::type_name(&token));
}

/// An FTEQCC class: its name as a type, and its parent and members (FTEF-9). A member
/// function's body written in the class is lowered with the class as its scope.
fn lower_class(class: &ast::ClassDecl, fte: bool, out: &mut FileDecls) {
    let Some(name) = class.name() else {
        return;
    };
    let class_name = name.text_lossless();
    let mut raw = RawClass {
        parent: class.parent().map(|t| t.text_lossless()),
        members: Vec::new(),
    };
    // FTEF-9.2: `virtual:` and `nonvirtual:` set the kind of the member functions after them.
    let mut assume_virtual = false;
    for node in class.syntax().children() {
        match node.kind() {
            SyntaxKind::AccessLabel => {
                let word = first_word(&node);
                if word.ends_with("nonvirtual") {
                    assume_virtual = false;
                } else if word.ends_with("virtual") {
                    assume_virtual = true;
                }
            }
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_member(&decl, &class_name, assume_virtual, fte, &mut raw.members);
                }
            }
            SyntaxKind::AccessorMember => {
                if let Some(member) = ast::AccessorMember::cast(node) {
                    out.members.extend(member.name().map(|t| t.text_lossless()));
                    lower_accessor_body(&member, &class_name, fte, &mut out.decls);
                }
            }
            _ => {}
        }
    }
    let mut decl = RawDecl::type_name(&name);
    decl.class = Some(raw);
    out.decls.push(decl);
}

/// The first word of a node, as written.
fn first_word(node: &SyntaxNode) -> String {
    node.child_tokens()
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text_lossless())
        .unwrap_or_default()
}

/// The qualifier words of a declaration (FTEF-15.1, FTEF-9.2), as written.
fn qualifiers(decl: &ast::Declaration) -> Vec<String> {
    decl.syntax()
        .children()
        .filter(|n| n.kind() == SyntaxKind::Qualifier)
        .map(|n| first_word(&n))
        .collect()
}

/// One member declaration of a class (FTEF-9.2 to FTEF-9.4).
fn lower_member(
    decl: &ast::Declaration,
    class: &str,
    assume_virtual: bool,
    fte: bool,
    out: &mut Vec<RawMember>,
) {
    let words = qualifiers(decl);
    let has = |word: &str| words.iter().any(|w| w.trim_start_matches('_') == word);
    let virtual_ = has("virtual") || (assume_virtual && !has("nonvirtual") && !has("static"));
    let base = decl.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    let mut declared = false;
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        declared = true;
        let ty = c_style_function_type(&base, &declarator).unwrap_or_else(|| base.clone());
        let kind = if !ty.is_function() {
            MemberKind::Field
        } else if virtual_ {
            MemberKind::Virtual
        } else {
            MemberKind::Method
        };
        let init = declarator.initializer();
        let body = match &init {
            Some(ast::Initializer::Body(fn_body)) => Some(lower_body(fn_body, &ty, fte)),
            _ => None,
        };
        out.push(RawMember {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
            kind,
            initialized: init.is_some(),
            body,
        });
    }
    // FTEF-9.4: `Name(void);`, a constructor written without a return type.
    if !declared
        && let Some(ast::Type::Function(func)) = decl.ty()
        && let Some(ast::Type::Simple(ret)) = func.return_type()
        && ret.text() == class
        && let Some(token) = ret
            .syntax()
            .child_tokens()
            .find(|t| t.kind() == SyntaxKind::Name)
    {
        out.push(RawMember {
            name: class.to_owned(),
            span: token.span(),
            ty: Type::from_ast(&ast::Type::Function(func)),
            kind: MemberKind::Method,
            initialized: false,
            body: None,
        });
    }
}

/// An accessor: its name as a type, its members' names, and its member bodies as functions
/// named `Name::get_member` and `Name::set_member` (FTEF-10.1, FTEF-10.2).
fn lower_accessor(acc: &ast::AccessorDecl, fte: bool, out: &mut FileDecls) {
    let Some(name) = acc.name() else {
        return;
    };
    let accessor = name.text_lossless();
    out.decls.push(RawDecl::type_name(&name));
    for member in acc.members() {
        out.members.extend(member.name().map(|t| t.text_lossless()));
        lower_accessor_body(&member, &accessor, fte, &mut out.decls);
    }
}

/// The body of an accessor member, as a function whose parameters are `this`, `value` and the
/// index name (FTEF-10.2); a member whose value is a function is a use of it.
fn lower_accessor_body(
    member: &ast::AccessorMember,
    owner: &str,
    fte: bool,
    out: &mut Vec<RawDecl>,
) {
    let set = first_word(member.syntax()) == "set"
        || member
            .syntax()
            .child_tokens()
            .any(|t| t.text_lossless() == "set");
    let field = member
        .name()
        .map_or_else(String::new, |t| t.text_lossless());
    let span = member
        .name()
        .map_or_else(|| member.syntax().span(), |t| t.span());
    let name = format!("{owner}::{}_{field}", if set { "set" } else { "get" });
    let mut decl = RawDecl::new(name, span, Type::Unknown);
    if let Some(fn_body) = member.body() {
        let mut body = RawBody::default();
        for param in ["this", "value"] {
            body.params.push(RawParam {
                name: param.to_owned(),
                span: Span::new(span.start, span.start),
                ty: Type::Unknown,
            });
        }
        if let Some(index) = member.index_name() {
            body.params.push(RawParam {
                name: index.text_lossless(),
                span: index.span(),
                ty: Type::Unknown,
            });
        }
        if let Some(block) = fn_body.block() {
            lower_block(&block, fte, &mut body);
        }
        decl.body = Some(body);
        decl.initialized = true;
    } else if let Some(value) = member.value() {
        lower_expr(&value, fte, &mut decl.uses);
        decl.initialized = true;
    }
    out.push(decl);
}

fn lower_declaration(decl: &ast::Declaration, fte: bool, out: &mut Vec<RawDecl>) {
    let base = decl.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let name = name_token.text_lossless();
        let span = name_token.span();
        // FTE-17.1: an FTEQCC C-style declarator `rettype name(params)` has its own
        // `ParamList`; its type is the function type of the return type and those
        // parameters, so it is the type a vanilla `rettype(params) name` declares.
        let ty = c_style_function_type(&base, &declarator).unwrap_or_else(|| base.clone());
        let init = declarator.initializer();
        let mut raw = RawDecl::new(name, span, ty.clone());
        raw.initialized = init.is_some();
        raw.literal = LiteralValue::of(init.as_ref());
        raw.scope = declarator.scope().map(|t| t.text_lossless());
        match &init {
            Some(ast::Initializer::Body(fn_body)) => {
                raw.next_think = next_think_of(fn_body);
                raw.body = Some(lower_body(fn_body, &ty, fte));
            }
            // FTE-10.2: a value initialiser may name other definitions, `const float B = A;`.
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, fte, &mut raw.uses),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, fte, &mut raw.uses);
                }
            }
            Some(ast::Initializer::Builtin(_)) | None => {}
        }
        // FTE-18.2: a parameter's default value may name definitions too.
        for list in decl
            .ty()
            .and_then(|t| function_params(&t))
            .into_iter()
            .chain(declarator.params())
        {
            for param in list.params() {
                if let Some(value) = param.default_value() {
                    lower_expr(&value, fte, &mut raw.uses);
                }
            }
        }
        // FTEF-5.1: an array's size expressions name definitions too.
        for suffix in declarator.array_suffixes() {
            for expr in suffix.children().filter_map(ast::Expr::cast) {
                lower_expr(&expr, fte, &mut raw.uses);
            }
        }
        out.push(raw);
    }
}

/// The parameter list of a function type (QC-10.3).
fn function_params(ty: &ast::Type) -> Option<ast::ParamList> {
    match ty {
        ast::Type::Function(func) => func.params(),
        ast::Type::Simple(_) | ast::Type::Field(_) | ast::Type::Pointer(_) => None,
    }
}

/// If `declarator` is an FTEQCC C-style function declarator (its own `(params)` list beside
/// the name), the function type built from the return type `ret` and the parameter types;
/// otherwise `None` (FTE-17.1).
fn c_style_function_type(ret: &Type, declarator: &ast::Declarator) -> Option<Type> {
    let list = declarator.params()?;
    Some(Type::Function(std::rc::Rc::new(FunctionType {
        ret: ret.clone(),
        params: Type::params_of(&list),
        variadic: list.is_variadic(),
    })))
}

/// The next-think name of a state header `[frame, next]`, if present (QC-13.6).
fn next_think_of(fn_body: &ast::FunctionBody) -> Option<(String, Span)> {
    let state = fn_body.state()?;
    let token = state.next()?;
    Some((token.text_lossless(), token.span()))
}

fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, fte: bool) -> RawBody {
    let mut body = RawBody::default();
    collect_params(fn_ty, fn_body, &mut body.params);
    if let Some(block) = fn_body.block() {
        lower_block(&block, fte, &mut body);
    }
    body
}

/// The parameters named in the function type of the defining declaration (QC-13.2).
///
/// The parameter names come from the `FunctionType`'s `ParamList`; the semantic types come
/// from the same list, matched by position to [`Type::from_ast`]'s view of the function
/// type (QC-10.3). An FTEQCC parameter may have no name (FTEF-15.3); it declares nothing.
fn collect_params(fn_ty: &Type, fn_body: &ast::FunctionBody, out: &mut Vec<RawParam>) {
    let Some(param_list) = param_list_of(fn_body) else {
        return;
    };
    // The function type's positional parameter types, for the vanilla `rettype(params) name`
    // order. For the FTEQCC C-style `rettype name(params)` order the return type is not a
    // function type, so each parameter's type comes from its own `Param` node instead.
    let fty = match fn_ty {
        Type::Function(fty) => Some(fty),
        _ => None,
    };
    for (index, param) in param_list.params().enumerate() {
        let Some(name_token) = param.name() else {
            continue;
        };
        let ty = fty
            .and_then(|f| f.params.get(index).cloned())
            .or_else(|| param.ty().map(|t| Type::from_ast(&t)))
            .unwrap_or(Type::Unknown);
        out.push(RawParam {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
        });
    }
}

/// The `ParamList` of the function that defines `fn_body` (QC-13.2): the vanilla function
/// type's list, or an FTEQCC C-style declarator's own `(params)` list (FTE-17.1).
fn param_list_of(fn_body: &ast::FunctionBody) -> Option<ast::ParamList> {
    let declarator = fn_body.syntax().parent()?;
    if let Some(list) = ast::Declarator::cast(declarator.clone()).and_then(|d| d.params()) {
        return Some(list);
    }
    let declaration = declarator.parent()?;
    let ty = declaration
        .children()
        .find_map(ast::Type::cast)
        .or_else(|| declarator.children().find_map(ast::Type::cast))?;
    function_params(&ty)
}

fn lower_block(block: &ast::Block, fte: bool, body: &mut RawBody) {
    for stmt in block.statements() {
        lower_stmt(&stmt, fte, body);
    }
}

fn lower_stmt(stmt: &ast::Stmt, fte: bool, body: &mut RawBody) {
    match stmt {
        ast::Stmt::Local(local) => lower_local(local, fte, body),
        ast::Stmt::Block(block) => lower_block(block, fte, body),
        ast::Stmt::Return(ret) => {
            if let Some(expr) = ret.value() {
                lower_expr(&expr, fte, &mut body.uses);
            }
        }
        ast::Stmt::If(if_stmt) => {
            if let Some(cond) = if_stmt.condition() {
                lower_expr(&cond, fte, &mut body.uses);
            }
            if let Some(then) = if_stmt.then_branch() {
                lower_stmt(&then, fte, body);
            }
            if let Some(els) = if_stmt.else_branch() {
                lower_stmt(&els, fte, body);
            }
        }
        ast::Stmt::While(while_stmt) => {
            if let Some(cond) = while_stmt.condition() {
                lower_expr(&cond, fte, &mut body.uses);
            }
            if let Some(inner) = while_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
        }
        ast::Stmt::Do(do_stmt) => {
            if let Some(inner) = do_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
            if let Some(cond) = do_stmt.condition() {
                lower_expr(&cond, fte, &mut body.uses);
            }
        }
        // FTEQCC `for (init; cond; step) body` (FTE-2). The init clause may declare a loop
        // local; the clauses hold expressions whose names are used (QC-12.5).
        ast::Stmt::For(for_stmt) => {
            for clause in for_stmt.clauses() {
                if let Some(local) = clause.children().find_map(ast::LocalDecl::cast) {
                    lower_local(&local, fte, body);
                } else {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        lower_expr(&expr, fte, &mut body.uses);
                    }
                }
            }
            if let Some(inner) = for_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
        }
        // FTEQCC `switch (e) { ... }` (FTE-3): the scrutinee, each `case` value and the
        // statements of the body are lowered.
        ast::Stmt::Switch(switch_stmt) => {
            if let Some(scrutinee) = switch_stmt.scrutinee() {
                lower_expr(&scrutinee, fte, &mut body.uses);
            }
            if let Some(block) = switch_stmt.block() {
                for node in block.syntax().children() {
                    if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                        if let Some(value) = label.value() {
                            lower_expr(&value, fte, &mut body.uses);
                        }
                    } else if let Some(stmt) = ast::Stmt::cast(node) {
                        lower_stmt(&stmt, fte, body);
                    }
                }
            }
        }
        // `break;`, `continue;` (FTE-4) and `;` (FTE-21.5) bind no names and use none.
        ast::Stmt::Break(_) | ast::Stmt::Continue(_) | ast::Stmt::Empty(_) => {}
        ast::Stmt::Expr(expr_stmt) => {
            if let Some(expr) = expr_stmt.expr() {
                lower_expr(&expr, fte, &mut body.uses);
            }
        }
    }
}

fn lower_local(local: &ast::LocalDecl, fte: bool, body: &mut RawBody) {
    let base = local.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    // QC-11.9 holds only in vanilla: FTEQCC allows a field or function local (FTE-21.2).
    let bad = !fte && (base.is_field() || base.is_function());
    for declarator in local.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let ty = c_style_function_type(&base, &declarator).unwrap_or_else(|| base.clone());
        let init = declarator.initializer();
        // A local initialiser is a constant value; if it is a non-constant expression we
        // still record any names it uses, so references in it resolve (QC-12.5).
        match &init {
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, fte, &mut body.uses),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, fte, &mut body.uses);
                }
            }
            _ => {}
        }
        for suffix in declarator.array_suffixes() {
            for expr in suffix.children().filter_map(ast::Expr::cast) {
                lower_expr(&expr, fte, &mut body.uses);
            }
        }
        body.locals.push(RawLocal {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
            initialized: init.is_some(),
            bad_field_or_function: bad,
        });
    }
}

/// What a member access is made on, read from the base expression's shape (FTEF-17.3).
fn member_base(base: &ast::Expr) -> MemberBase {
    match base {
        ast::Expr::Name(name_ref) if name_ref.scope().is_none() => name_ref
            .name()
            .map_or(MemberBase::Unknown, |t| MemberBase::Name(t.text_lossless())),
        ast::Expr::Cast(cast) => cast.ty().map_or(MemberBase::Unknown, |t| {
            MemberBase::Type(Type::from_ast(&t))
        }),
        ast::Expr::Call(call) => match call.callee() {
            Some(ast::Expr::Name(name_ref)) if name_ref.scope().is_none() => name_ref
                .name()
                .map_or(MemberBase::Unknown, |t| MemberBase::Call(t.text_lossless())),
            _ => MemberBase::Unknown,
        },
        ast::Expr::Paren(paren) => paren
            .expr()
            .map_or(MemberBase::Unknown, |e| member_base(&e)),
        _ => MemberBase::Unknown,
    }
}

fn lower_expr(expr: &ast::Expr, fte: bool, uses: &mut Vec<RawUse>) {
    match expr {
        ast::Expr::Name(name_ref) => {
            if let Some(token) = name_ref.name() {
                let mut use_ = RawUse::new(token.text_lossless(), token.span(), UseKind::Value);
                use_.scope = name_ref.scope().map(|t| t.text_lossless());
                uses.push(use_);
            }
        }
        ast::Expr::Literal(_) => {}
        ast::Expr::Paren(paren) => {
            if let Some(inner) = paren.expr() {
                lower_expr(&inner, fte, uses);
            }
        }
        ast::Expr::Not(not) => {
            if let Some(operand) = not.operand() {
                lower_expr(&operand, fte, uses);
            }
        }
        ast::Expr::Binary(bin) => {
            if let Some(lhs) = bin.lhs() {
                lower_expr(&lhs, fte, uses);
            }
            if let Some(rhs) = bin.rhs() {
                lower_expr(&rhs, fte, uses);
            }
        }
        ast::Expr::Field(field) => {
            // The base is a value. In vanilla the field selector is an entity-field name
            // used as a value (QC-15.7). In an FTEQCC dialect a selector that is a name may
            // also be a vector member (`.x`), `.length`, a struct field or a class member
            // (FTE-9, FTE-13, FTE-14, FTEF-9.3), so it is a member use, which resolution tells
            // apart by what the access is made on.
            let base = field.base();
            if let Some(base) = &base {
                lower_expr(base, fte, uses);
            }
            match field.field() {
                Some(ast::Expr::Name(name_ref)) if fte => {
                    if let Some(token) = name_ref.name() {
                        let mut use_ =
                            RawUse::new(token.text_lossless(), token.span(), UseKind::Member);
                        use_.scope = name_ref.scope().map(|t| t.text_lossless());
                        use_.base = base.as_ref().map_or(MemberBase::Unknown, member_base);
                        uses.push(use_);
                    }
                }
                Some(field_expr) => lower_expr(&field_expr, fte, uses),
                None => {}
            }
        }
        ast::Expr::Call(call) => {
            if let Some(callee) = call.callee() {
                lower_expr(&callee, fte, uses);
            }
            if let Some(args) = call.args() {
                for arg in args.args() {
                    lower_expr(&arg, fte, uses);
                }
            }
        }
        // FTEQCC `e++`/`e--` (FTE-5): the operand is a value use.
        ast::Expr::Postfix(post) => {
            if let Some(operand) = post.operand() {
                lower_expr(&operand, fte, uses);
            }
        }
        // FTEQCC `cond ? a : b` (FTE-8): the condition and both arms are values.
        ast::Expr::Ternary(tern) => {
            if let Some(cond) = tern.condition() {
                lower_expr(&cond, fte, uses);
            }
            let (a, b) = tern.arms();
            if let Some(a) = a {
                lower_expr(&a, fte, uses);
            }
            if let Some(b) = b {
                lower_expr(&b, fte, uses);
            }
        }
        // FTEQCC `-e`, `+e`, `~e` (FTE-19.4), `++e`, `&e`, `*e` (FTEF-16.7) and `(type) e`
        // (FTE-19.6): the operand is a value.
        ast::Expr::Unary(unary) => {
            if let Some(operand) = unary.operand() {
                lower_expr(&operand, fte, uses);
            }
        }
        ast::Expr::Cast(cast) => {
            if let Some(operand) = cast.operand() {
                lower_expr(&operand, fte, uses);
            }
            for element in cast
                .syntax()
                .children()
                .filter_map(ast::AggregateInit::cast)
                .flat_map(|agg| agg.elements().collect::<Vec<_>>())
            {
                lower_expr(&element, fte, uses);
            }
        }
        // FTEQCC `e[i]` (FTE-13): the base and index are values.
        ast::Expr::Index(index) => {
            if let Some(base) = index.base() {
                lower_expr(&base, fte, uses);
            }
            if let Some(i) = index.index() {
                lower_expr(&i, fte, uses);
            }
        }
        // FTEQCC `[x, y, z]` (FTEF-16.3): each component is a value.
        ast::Expr::Vector(vector) => {
            for element in vector.elements() {
                lower_expr(&element, fte, uses);
            }
        }
    }
}

/// The kind a top-level raw declaration resolves to, from its type (QC-11.8, QC-11.9).
#[must_use]
pub fn top_level_kind(ty: &Type) -> DefKind {
    if ty.is_function() {
        DefKind::Function
    } else if ty.is_field() {
        DefKind::Field
    } else {
        DefKind::Global
    }
}
