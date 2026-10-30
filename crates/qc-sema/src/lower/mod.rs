// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-file lowering: the raw declarations a single file contributes, in source order.
//!
//! This is the semantic model's leaf query. It walks one file's syntax tree
//! (`qc_syntax::Parse`) and lists every name the file declares, with its type, kind and the
//! span of its name, plus the references each function body makes. It does no cross-file
//! resolution: that is [`crate::resolve`], which joins the files of a module in compilation
//! order (QC-23.3). Keeping this query per file is what lets an edit to one file re-lower
//! only that file (GOAL-1, the incremental model).

mod body;
mod decl;
mod expr;
mod types;

pub use body::{RawAlias, RawBody, RawLocal, RawParam, StatePart};
pub use decl::{
    ArrayDim, BuiltinFunction, ExternBase, LiteralValue, Qualifier, RawAggregate, RawBraces,
    RawDecl, RawElement, RawParamInfo, RawValue, ValueKind, declares,
};
pub use expr::{MemberBase, RawUse, UseKind};
pub use types::{
    EnumItem, IntConst, MemberKind, MemberStorage, Qualification, RawClass, RawMember, RawStruct,
    StructMember, TypeKind,
};

use qc_diag::{Diagnostic, Span};
use qc_preproc::{Enabled, FteFlag};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::CContext;
use qc_syntax::{CompilerFlags, Dialect, Keyword, Keywords, Parse, SyntaxKind, SyntaxNode};

use crate::symbol::DefKind;
use crate::ty::{FunctionType, Type, TypeContext};

use decl::{decl_base, lower_declaration, lower_declarators};
use types::{
    anonymous_tag, lower_accessor, lower_class, lower_enum, lower_struct, lower_typedef,
    nested_type, record_fields,
};

/// Every declaration a file contributes, in source order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileDecls {
    /// The file-level and body declarations, in source order.
    pub decls: Vec<RawDecl>,
    /// The field names of the structs and accessors the file defines (FTE-14.1, FTEF-6,
    /// FTEF-10): a member access may select one without naming a global.
    pub members: Vec<String>,
    /// What lowering found wrong with a declaration that resolution does not see: a `typedef`
    /// that defines no type or has storage qualifiers (FTEF-19.9).
    pub diagnostics: Vec<Diagnostic>,
    /// The tag of every `struct` or `union` the file writes, nested in another's members too,
    /// with where it stands: `QCC_PR_ParseStruct` makes it a type there (FTEF-19.20).
    pub struct_tags: Vec<(String, usize)>,
    /// Every `struct` and `union` the file writes, nested ones too, in source order
    /// (FTEF-6.7).
    pub structs: Vec<RawStruct>,
    /// The members of every accessor and class accessor the file writes (FTEF-10.3).
    pub accessors: Vec<RawAccessor>,
}

/// A `get` or `set` member of an accessor or a class (FTEF-10.3): reading or storing the member
/// calls the function its body or value gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAccessor {
    /// The accessor's or class's name.
    pub owner: String,
    /// The member's name, empty for the indexer `get T[index]`.
    pub name: String,
    /// The member's type.
    pub ty: Type,
    /// Whether it is a `set` member.
    pub set: bool,
    /// The type of its index, `[Type name]`, when it has one.
    pub indexer: Option<Type>,
}

/// What lowering reads besides the tree: whether the dialect is FTEQCC's, the compiler flags
/// FTEQCC's lexer read the file's tokens with (FTEF-12.12) and the keywords its parser read them
/// with (FTEF-20.1), each from the offset it holds.
#[derive(Clone, Copy)]
struct Cx<'a> {
    fte: bool,
    flags: &'a [(usize, qc_syntax::CompilerFlags)],
    /// The lexer's flags, which differ from `flags` at the token after an `extern` block
    /// (FTEF-21.17).
    lex: &'a [(usize, qc_lexer::LexFlags)],
    /// Where each of the file's tokens starts, trivia aside, in order.
    tokens: &'a [usize],
    keywords: &'a [(usize, Keywords)],
}

/// Whether a parameter of `list` writes a struct, union or enum as its type, which only
/// [`Cx::params`] reads (FTEF-15.10).
fn writes_a_type(list: &ast::ParamList) -> bool {
    list.params().any(|param| {
        matches!(param.ty(), Some(ast::Type::Simple(simple)) if simple.syntax().children().next().is_some())
    })
}

impl Cx<'_> {
    /// The lexer's flags at offset `at` (FTEF-12.12): the last that holds from at or before it,
    /// else the defaults.
    fn lex_flags_at(self, at: usize) -> qc_lexer::LexFlags {
        qc_lexer::LexFlags::at(self.lex, at)
    }

    /// The compiler flags at offset `at` (FTEF-21.1): the last that holds from at or before it,
    /// else the defaults.
    /// The marks of `marks` that hold somewhere in `span`: the one in force at its start and
    /// those after it, before its end.
    fn marks_in<T: Copy>(marks: &[(usize, T)], span: Span) -> Vec<(usize, T)> {
        let first = marks
            .partition_point(|&(at, _)| at <= span.start)
            .saturating_sub(1);
        marks
            .iter()
            .skip(first)
            .take_while(|&&(at, _)| at < span.end)
            .copied()
            .collect()
    }

    fn flags_at(self, at: usize) -> CompilerFlags {
        CompilerFlags::at(self.flags, at)
    }

    /// The compiler flags at the first token that starts at or after `at`, the token FTEQCC
    /// looks at once it has read what ends at `at` (FTEF-21.1); the flags at `at` past the last.
    fn next_flags(self, at: usize) -> CompilerFlags {
        let index = self.tokens.partition_point(|&start| start < at);
        self.flags_at(self.tokens.get(index).copied().unwrap_or(at))
    }

    /// The keywords at offset `at` (FTEF-20.1).
    fn keywords_at(self, at: usize) -> Keywords {
        Keywords::at(self.keywords, at)
    }

    /// What the loop over C type words read at offset `at` (FTEF-20.9).
    fn c_context(self, at: usize) -> CContext {
        CContext {
            keywords: self.keywords_at(at),
            ilp32: self.lex_flags_at(at).has(qc_lexer::LexFlag::Ilp32),
        }
    }

    /// What a type is read with at offset `at`: its C type words' context and `qcfuncs`
    /// (FTEF-20.9, FTEF-21.8).
    fn type_context(self, at: usize) -> TypeContext {
        TypeContext {
            words: self.c_context(at),
            qcfuncs: self.flags_at(at).has(FteFlag::Qcfuncs),
        }
    }

    /// The semantic type of `ty`, its C type words read as the parser read them (FTEF-20.9).
    fn ty(self, ty: &ast::Type) -> Type {
        match ty {
            ast::Type::Simple(simple) => nested_type(simple, self)
                .unwrap_or_else(|| Type::from_ast(ty, &|at| self.type_context(at))),
            ast::Type::Field(field) => Type::Field(std::rc::Rc::new(
                field.inner().map_or(Type::Unknown, |t| self.ty(&t)),
            )),
            ast::Type::Pointer(pointer) => Type::Pointer(std::rc::Rc::new(
                pointer.inner().map_or(Type::Unknown, |t| self.ty(&t)),
            )),
            ast::Type::Function(function) => {
                let read = Type::from_ast(ty, &|at| self.type_context(at));
                match (read, function.params().filter(writes_a_type)) {
                    (Type::Function(read), Some(list)) => {
                        Type::Function(std::rc::Rc::new(FunctionType {
                            params: self.params(&list),
                            ..read.as_ref().clone()
                        }))
                    }
                    (read, _) => read,
                }
            }
        }
    }

    /// The parameter types of `list`, as [`Self::ty`] reads each: a struct, union or enum a
    /// parameter's type writes is that type (FTEF-15.10).
    fn params(self, list: &ast::ParamList) -> Vec<Type> {
        let mut types = Type::params_of(list, &|at| self.type_context(at));
        for (param, ty) in list.params().zip(types.iter_mut()) {
            if let Some(ast::Type::Simple(simple)) = param.ty()
                && param.array_suffixes().is_empty()
                && let Some(nested) = nested_type(&simple, self)
            {
                *ty = nested;
            }
        }
        types
    }

    /// The function type returning `ret` with the parameters of `list` (FTEF-21.8).
    fn function(self, ret: Type, list: &ast::ParamList) -> Type {
        match Type::function_of(ret, list, &|at| self.type_context(at)) {
            Type::Function(function) if writes_a_type(list) => {
                Type::Function(std::rc::Rc::new(FunctionType {
                    params: self.params(list),
                    ..function.as_ref().clone()
                }))
            }
            other => other,
        }
    }

    /// Whether `token` is `keyword` spelled `word`, as the parser read it (FTEF-20.2).
    fn spells(self, token: &qc_syntax::SyntaxToken, keyword: Keyword, word: &str) -> bool {
        self.keywords_at(token.span().start).checks(
            Enabled::By(keyword),
            word,
            &token.text_lossless(),
        )
    }
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
    let tokens: Vec<usize> = tree
        .syntax()
        .tokens()
        .filter(|t| !t.kind().is_trivia())
        .map(|t| t.span().start)
        .collect();
    let cx = Cx {
        fte,
        flags: parse.flags(),
        lex: parse.lex_flags(),
        tokens: &tokens,
        keywords: parse.keywords(),
    };
    for node in tree
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::StructType)
    {
        let tag = node.children_with_tokens().find_map(|e| match e {
            qc_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::Name => Some(t),
            qc_syntax::SyntaxElement::Token(_) | qc_syntax::SyntaxElement::Node(_) => None,
        });
        // FTEF-20.15: a body's struct is a type to its block's end, recorded with the body.
        let in_body = node.ancestors().any(|a| a.kind() == SyntaxKind::Block);
        if let Some(tag) = tag.filter(|_| !in_body) {
            out.struct_tags
                .push((tag.text_lossless(), tag.span().start));
        }
        out.structs.extend(lower_struct(&node, cx));
        for field in node
            .children()
            .filter(|n| n.kind() == SyntaxKind::StructField)
        {
            record_fields(&field, &mut out.members);
        }
    }
    // `QCC_PR_ParseStruct` makes a struct at its `}`, after the types its body holds: an inner
    // struct, class, accessor or enum is made before the struct around it.
    out.structs.sort_by_key(|st| (st.end, st.at));
    // FTEF-6.8: `struct tag` with no body names the struct by its tag, a type from there on.
    for simple in tree
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SimpleType)
    {
        let mut tokens = simple.child_tokens().filter(|t| !t.kind().is_trivia());
        if let (Some(keyword), Some(tag)) = (tokens.next(), tokens.next())
            && matches!(keyword.kind(), SyntaxKind::StructKw | SyntaxKind::UnionKw)
            && tag.kind() == SyntaxKind::Name
        {
            out.struct_tags
                .push((tag.text_lossless(), tag.span().start));
        }
    }
    out.struct_tags.sort_by_key(|(_, at)| *at);
    // Walk the top-level nodes in source order, so a name an `enum` or `typedef` declares is
    // visible to the declarations after it (QC-12.5, QC-23.3). Only an FTEQCC parse has
    // `enum` and `typedef` nodes.
    for node in tree.items() {
        // FTEF-9.8, FTEF-10.5: a class or accessor written where a type stands, in a typedef,
        // a member, a parameter, a cast or a body, is `QCC_PR_ParseEntClass`'s or
        // `QCC_PR_ParseType`'s declaration of it, before the declaration that holds it; so is
        // an enum in a struct's, class's or accessor's body (FTEF-8.2), a declaration's and a
        // typedef's own being lowered with them.
        for nested in node.descendants().filter(|n| *n != node) {
            match nested.kind() {
                SyntaxKind::EnumDecl
                    if node.kind() != SyntaxKind::Declaration
                        && nested.parent().as_ref() != Some(&node) =>
                {
                    if let Some(en) = ast::EnumDecl::cast(nested) {
                        lower_enum(&en, cx, &mut out.decls);
                    }
                }
                SyntaxKind::ClassDecl => {
                    if let Some(class) = ast::ClassDecl::cast(nested) {
                        lower_class(&class, cx, &mut out);
                    }
                }
                SyntaxKind::AccessorDecl => {
                    if let Some(acc) = ast::AccessorDecl::cast(nested) {
                        lower_accessor(&acc, cx, &mut out);
                    }
                }
                _ => {}
            }
        }
        match node.kind() {
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_declaration(&decl, cx, &mut out.decls);
                }
            }
            SyntaxKind::EnumDecl => {
                if let Some(en) = ast::EnumDecl::cast(node) {
                    let base = lower_enum(&en, cx, &mut out.decls);
                    let base = decl_base(en.syntax(), base, cx, &mut out.decls);
                    // FTEF-8.5: `enum E { A } x;` declares `x` of the enum's type, an anonymous
                    // enum's of its base.
                    let first = out.decls.len();
                    lower_declarators(en.declarators(), &base, None, cx, &mut out.decls);
                    let flags = cx.flags_at(en.syntax().span().start);
                    for raw in out.decls.iter_mut().skip(first) {
                        raw.flags = flags;
                    }
                }
            }
            SyntaxKind::TypedefDecl => {
                if let Some(td) = ast::TypedefDecl::cast(node) {
                    lower_typedef(&td, cx, &mut out);
                }
            }
            SyntaxKind::ClassDecl => {
                if let Some(class) = ast::ClassDecl::cast(node) {
                    lower_class(&class, cx, &mut out);
                }
            }
            SyntaxKind::StructDecl => {
                if let Some(st) = ast::StructDecl::cast(node) {
                    let union = st
                        .syntax()
                        .descendants_with_tokens()
                        .any(|e| e.kind() == SyntaxKind::UnionKw);
                    let base = match st.name() {
                        Some(token) => {
                            out.decls.push(RawDecl::type_name(&token, TypeKind::Struct));
                            Type::Named(std::rc::Rc::from(token.text_lossless().as_str()))
                        }
                        // A body with no tag is a struct type of its own (FTEF-6.12).
                        None => {
                            let tag = st
                                .syntax()
                                .children()
                                .find(|n| n.kind() == SyntaxKind::StructType)
                                .map_or_else(|| "{}".to_owned(), |node| anonymous_tag(&node));
                            let keyword = if union { "union" } else { "struct" };
                            Type::Named(std::rc::Rc::from(format!("{keyword} {tag}").as_str()))
                        }
                    };
                    let base = decl_base(st.syntax(), base, cx, &mut out.decls);
                    // FTEF-6.1: `struct { ... } a[4], *p;` declares variables too.
                    let first = out.decls.len();
                    lower_declarators(st.declarators(), &base, None, cx, &mut out.decls);
                    let flags = cx.flags_at(st.syntax().span().start);
                    for raw in out.decls.iter_mut().skip(first) {
                        raw.flags = flags;
                    }
                }
            }
            SyntaxKind::AccessorDecl => {
                if let Some(acc) = ast::AccessorDecl::cast(node) {
                    lower_accessor(&acc, cx, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// The text of a node's tokens, trivia left out, joined by a space.
fn token_text(node: &SyntaxNode) -> String {
    let words: Vec<String> = node
        .descendants_with_tokens()
        .filter_map(|e| match e {
            qc_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => Some(t.text_lossless()),
            qc_syntax::SyntaxElement::Token(_) | qc_syntax::SyntaxElement::Node(_) => None,
        })
        .collect();
    words.join(" ")
}

/// The first word of a node, as written.
fn first_word(node: &SyntaxNode) -> String {
    node.child_tokens()
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text_lossless())
        .unwrap_or_default()
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
