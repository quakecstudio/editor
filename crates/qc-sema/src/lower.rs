// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-file lowering: the raw declarations a single file contributes, in source order.
//!
//! This is the semantic model's leaf query. It walks one file's syntax tree
//! (`qc_syntax::Parse`) and lists every name the file declares, with its type, kind and the
//! span of its name, plus the references each function body makes. It does no cross-file
//! resolution: that is [`crate::resolve`], which joins the files of a module in compilation
//! order (QC-23.3). Keeping this query per file is what lets an edit to one file re-lower
//! only that file (GOAL-1, the incremental model).

use qc_diag::{Code, Diagnostic, Span};
use qc_preproc::{Enabled, FteFlag, PragmaDefault};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::{CContext, CScalar, CWords};
use qc_syntax::{CompilerFlags, Dialect, Keyword, Keywords, Parse, SyntaxKind, SyntaxNode};

use crate::cast::{ConstType, Literal};
use crate::symbol::DefKind;
use crate::ty::{SimpleType, Type, TypeContext};

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
    /// Under FTEQCC, the value of its initialiser, which FTEQCC compares when a name is
    /// initialised twice (FTE-22.5, FTEF-17.10); `None` for none, and under the vanilla
    /// profile, which compares no two values (QCD-3.6).
    pub literal: Option<LiteralValue>,
    /// For a function definition, the lowered body; otherwise `None`.
    pub body: Option<RawBody>,
    /// Whether the initialiser is a builtin number, `= #n` (QC-13.3), which FTEQCC does not
    /// let replace a function with a body (FTEF-17.9).
    pub builtin: bool,
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
    /// For a type's name, the kind of type it names: a class, struct, union, typedef, enum or
    /// accessor, which may be declared more than once (FTEF-7.2).
    pub type_kind: Option<TypeKind>,
    /// The qualifiers written before the type (FTEF-15.1), in order.
    pub qualifiers: Vec<Qualifier>,
    /// A value initialiser, as the checks of QC-11.3 and FTEF-19.2 read it.
    pub value: Option<RawValue>,
    /// The first array dimension, when the declarator has one (FTEF-5, FTEF-19.8).
    pub array: Option<ArrayDim>,
    /// The parameters of the function type the declaration gives, as FTEQCC's redeclaration
    /// rules compare them (FTEF-19.6, FTEF-19.7); empty for any other type.
    pub params: Vec<RawParamInfo>,
    /// For an enum's name, how its value is found (FTEF-19.10).
    pub enum_item: Option<EnumItem>,
    /// For a `typedef`'s name, the type it names (FTEF-19.9).
    pub typedef: Option<Type>,
    /// The compiler flags and pragma defaults where the declaration starts, which
    /// `QCC_PR_ParseDefs` reads first (FTEF-21.1, FTEF-21.14).
    pub flags: CompilerFlags,
    /// Whether `assumevar` was on at its initialiser, which makes an initialised global no
    /// constant (FTEF-21.11).
    pub assumevar: bool,
    /// Whether it, or a declarator of its declaration before it, is C's function pointer
    /// `(*name)`, `isvar |= isptr>0`, a variable (FTEF-21.7).
    pub pointer_var: bool,
    /// For an `extern` declaration at file level, the base type `QCC_PR_ParseDefs` tests it
    /// against, which resolution reads with the typedefs it knows (FTEF-21.10).
    pub extern_base: Option<ExternBase>,
}

/// The base type of an `extern` declaration and what `QCC_PR_ParseDefs` reads with it: `if
/// (externfnc && !pr_scope && basetype->type != ev_function) { if (flag_qcfuncs) ...warning...;
/// externfnc=false; }` (FTEF-21.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternBase {
    /// The declaration's type before its declarators, a left out one's `int`.
    pub ty: Type,
    /// Whether `qcfuncs` was on after the type, which the warning needs.
    pub qcfuncs: bool,
}

/// A declaration qualifier FTEQCC's `QCC_PR_ParseDefs` reads before a type (FTEF-15.1), written
/// as such or after `__`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Qualifier {
    /// `extern`: the body is elsewhere (FTEF-15.5).
    Extern,
    /// `static`: visible in its file, or a local kept between calls.
    Static,
    /// `shared`: kept when the progs change (FTEF-19.11).
    Shared,
    /// `const`.
    Const,
    /// `var`: a variable, even if initialised (FTE-11).
    Var,
    /// `nonstatic`.
    Nonstatic,
    /// `noref` or `unused` (FTEF-17.8).
    Noref,
    /// `used`.
    Used,
    /// `nosave`.
    Nosave,
    /// `strip` or `ignore`.
    Strip,
    /// `inline`.
    Inline,
    /// `wrap` (FTEF-15.5).
    Wrap,
    /// `weak` (FTEF-15.5).
    Weak,
    /// `accumulate` (FTEF-15.5).
    Accumulate,
    /// `deprecated`, with its reason if it gives one (FTEF-19.13).
    Deprecated(Option<String>),
    /// `auto` or `register`.
    Auto,
    /// The attribute `alias(NAME)` or `alias("NAME")`: the definition names the one `NAME`
    /// names (FTEF-21.9).
    Alias(String),
}

impl Qualifier {
    /// The qualifier a qualifier node's word and text name; `None` for a word that is none. The
    /// parser made the node only of a qualifier with its keyword on, `__` and the word in any
    /// case, or the word in any case under `caseinsens` (FTEF-20.2, FTEF-20.7).
    fn of(word: &str, text: &str) -> Option<Self> {
        let bare = word.strip_prefix("__").unwrap_or(word).to_ascii_lowercase();
        Some(match bare.as_str() {
            "extern" => Self::Extern,
            "static" => Self::Static,
            "shared" => Self::Shared,
            "const" => Self::Const,
            "var" => Self::Var,
            "nonstatic" => Self::Nonstatic,
            "noref" | "unused" => Self::Noref,
            "used" => Self::Used,
            "nosave" => Self::Nosave,
            "strip" | "ignore" => Self::Strip,
            "inline" => Self::Inline,
            "wrap" => Self::Wrap,
            "weak" => Self::Weak,
            "accumulate" => Self::Accumulate,
            "auto" | "register" => Self::Auto,
            "deprecated" => Self::Deprecated(
                text.split_once('"')
                    .and_then(|(_, rest)| rest.rsplit_once('"'))
                    .map(|(reason, _)| reason.to_owned()),
            ),
            _ => return None,
        })
    }
}

/// A value initialiser, as the semantic model reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawValue {
    /// The initialiser's span.
    pub span: Span,
    /// What it is.
    pub kind: ValueKind,
    /// The text of its first token, which FTEQCC's "expected {, found X" names (FTEF-19.8).
    pub first: String,
    /// For a string literal, its bytes as FTEQCC reads them, `pr_immediate_strlen`
    /// (FTEF-19.8); `None` for any other value.
    pub string_len: Option<u32>,
}

/// The shape of a value initialiser (QC-11.3, FTEF-19.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// A literal, with the type the dialect gives it.
    Literal(Literal),
    /// A name by itself.
    Name(String),
    /// FTEQCC's `_(...)` translation intrinsic, and whether its argument is one string
    /// literal (FTEF-19.2).
    Translate(bool),
    /// Any other expression, whose type the model does not compute (FTEF-19.30).
    Other,
}

/// The first dimension of an array declarator (FTEF-5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayDim {
    /// A size the model reads: a number literal, or the count of an unsized array's
    /// initialiser.
    Sized(u32),
    /// `[]` with nothing to size it, which FTEQCC's `QCC_PR_GetDef` compares with no other
    /// size.
    Unsized,
    /// A size given by an expression the model does not evaluate (FTEF-19.30).
    Unknown,
}

/// A parameter of a declaration's function type, as FTEQCC compares two redeclarations
/// (FTEF-19.6, FTEF-19.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawParamInfo {
    /// Its type.
    pub ty: Type,
    /// Whether `optional` marks it.
    pub optional: bool,
    /// Whether `__out`, `__inout` or `&` marks it.
    pub out: bool,
    /// Its default value (FTE-18.2), if it has one.
    pub default: Option<RawValue>,
    /// The default value when it is a literal, compared as the "dupe initialisation"
    /// comparison compares values (FTE-22.5).
    pub default_literal: Option<LiteralValue>,
}

/// How an enum's name gets its value (FTEF-19.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumItem {
    /// Whether the enum is `enumflags`, whose values double.
    pub flags: bool,
    /// The enum's base type: `float` unless `: type` gives another (FTEF-8).
    pub base: Type,
    /// The value given after `=`, if any.
    pub value: Option<RawValue>,
    /// That value's number, as the bits of a `float`, when it is a number literal.
    pub number: Option<u32>,
    /// Whether this is the first name of an enum whose base type is written in FTEQCC's
    /// legacy form, before the `{` with no `:` (FTEF-19.10).
    pub legacy: bool,
    /// Whether this is the enum's first name, where its count starts (FTEF-19.10).
    pub first: bool,
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
            builtin: false,
            next_think: None,
            uses: Vec::new(),
            scope: None,
            class: None,
            type_kind: None,
            qualifiers: Vec::new(),
            value: None,
            array: None,
            params: Vec::new(),
            enum_item: None,
            typedef: None,
            flags: CompilerFlags::default(),
            assumevar: false,
            pointer_var: false,
            extern_base: None,
        }
    }

    /// Whether it is `static`: written so, or under `#pragma defaultstatic 1` and not
    /// `nonstatic` (FTEF-21.14).
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.has(&Qualifier::Static)
            || (self.flags.default_on(PragmaDefault::Static) && !self.has(&Qualifier::Nonstatic))
    }

    /// Whether it is `noref`: written so, or under `#pragma noref 1` (FTEF-12.6).
    #[must_use]
    pub fn is_noref(&self) -> bool {
        self.has(&Qualifier::Noref) || self.flags.default_on(PragmaDefault::Noref)
    }

    /// The name its `alias` attribute gives (FTEF-21.9).
    #[must_use]
    pub fn alias(&self) -> Option<&str> {
        self.qualifiers.iter().find_map(|q| match q {
            Qualifier::Alias(name) => Some(name.as_str()),
            _ => None,
        })
    }

    /// Whether the qualifier `q` marks it.
    #[must_use]
    pub fn has(&self, q: &Qualifier) -> bool {
        self.qualifiers.contains(q)
    }

    /// Whether an FTEQCC `wrap`, `weak` or `accumulate` qualifier lets the definition be given
    /// again (FTEF-15.5).
    #[must_use]
    pub fn redefinable(&self) -> bool {
        self.has(&Qualifier::Wrap) || self.has(&Qualifier::Weak) || self.has(&Qualifier::Accumulate)
    }

    /// The reason a `deprecated` qualifier gives, `Some("")` for none; `None` when the
    /// declaration is not deprecated (FTEF-19.13).
    #[must_use]
    pub fn deprecated(&self) -> Option<&str> {
        deprecated_of(&self.qualifiers)
    }

    /// The declaration of a type's name (FTEF-7.2).
    fn type_name(token: &qc_syntax::SyntaxToken, kind: TypeKind) -> Self {
        let mut decl = Self::new(token.text_lossless(), token.span(), Type::Unknown);
        decl.type_kind = Some(kind);
        decl
    }
}

/// The kind of type a type's name names (FTEF-7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    /// A class (FTEF-9).
    Class,
    /// A struct or union (FTEF-6).
    Struct,
    /// A typedef (FTEF-7).
    Typedef,
    /// A named enum (FTE-15.4).
    Enum,
    /// An accessor (FTEF-10).
    Accessor,
}

/// The value initialiser `expr` is, in the dialect `fte` says (QC-11.3, FTEF-19.2): a literal
/// with its type, which FTEQCC gives a number by its digits and suffix (FTEF-3.1) and vanilla
/// always `float` (QC-4); a plain name; or another expression.
fn raw_value(expr: &ast::Expr, cx: Cx<'_>) -> RawValue {
    let fte = cx.fte;
    let span = expr.syntax().span();
    let flags = cx.lex_flags_at(span.start);
    let kind = match expr {
        ast::Expr::Literal(literal) => match (literal.kind(), literal.token()) {
            (Some(ast::LiteralKind::Float), Some(token)) => {
                let text = token.text_lossless();
                // FTEF-13.3: a character constant is a float, its code, and FTEF-19.24: an
                // `__int32` under `assumeint`.
                if fte && !text.starts_with('\'') {
                    ValueKind::Literal(Literal::of_fteqcc_number(&text, flags))
                } else if fte && flags.has(qc_lexer::LexFlag::AssumeInt) {
                    ValueKind::Literal(Literal::of(ConstType::Integer))
                } else {
                    ValueKind::Literal(Literal::of(ConstType::Float))
                }
            }
            (Some(ast::LiteralKind::FrameRef), _) => {
                ValueKind::Literal(Literal::of(ConstType::Float))
            }
            (Some(ast::LiteralKind::Vector), _) => {
                ValueKind::Literal(Literal::of(ConstType::Vector))
            }
            (Some(ast::LiteralKind::String), _) => {
                ValueKind::Literal(Literal::of(ConstType::String))
            }
            (None, _) | (Some(ast::LiteralKind::Float), None) => ValueKind::Other,
        },
        ast::Expr::Name(name) => match (name.scope(), name.name()) {
            (None, Some(t)) => ValueKind::Name(t.text_lossless()),
            // FTEF-19.10: `Enum::value` names the enum's value.
            (Some(scope), Some(t)) => {
                ValueKind::Name(format!("{}::{}", scope.text_lossless(), t.text_lossless()))
            }
            (_, None) => ValueKind::Other,
        },
        ast::Expr::Call(call) if fte && is_translate(call) => {
            let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
            let string = matches!(
                args.as_slice(),
                [ast::Expr::Literal(l)] if l.kind() == Some(ast::LiteralKind::String)
            );
            ValueKind::Translate(string)
        }
        _ => ValueKind::Other,
    };
    let first = expr
        .syntax()
        .descendants_with_tokens()
        .find_map(|e| match e {
            qc_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => Some(t.text_lossless()),
            qc_syntax::SyntaxElement::Token(_) | qc_syntax::SyntaxElement::Node(_) => None,
        })
        .unwrap_or_default();
    let string_len = match expr {
        ast::Expr::Literal(literal) => string_len(literal),
        _ => None,
    };
    RawValue {
        span,
        kind,
        first,
        string_len,
    }
}

/// Whether `call` calls FTEQCC's `_` translation intrinsic (FTEF-19.2).
fn is_translate(call: &ast::CallExpr) -> bool {
    matches!(
        call.callee(),
        Some(ast::Expr::Name(name)) if name.scope().is_none()
            && name.name().is_some_and(|t| t.text_lossless() == "_")
    )
}

/// The first dimension of a declarator's array suffixes, sized by `init` when it is `[]`
/// (FTEF-5.1): `QCC_PR_ParseDefArray` counts an aggregate's elements, or a string's bytes and its
/// terminator.
/// The bytes of the string literal `literal` as FTEQCC's lexer reads them,
/// `pr_immediate_strlen` (FTEF-19.8); `None` when it is no string.
fn string_len(literal: &ast::Literal) -> Option<u32> {
    if literal.kind() != Some(ast::LiteralKind::String) {
        return None;
    }
    let text = literal.token()?.text_lossless();
    let lexed = qc_lexer::lex_with(text.as_bytes(), Dialect::Fteqcc);
    match lexed.tokens.as_slice() {
        [token] => match &token.kind {
            qc_lexer::TokenKind::String(bytes) => u32::try_from(bytes.len()).ok(),
            _ => None,
        },
        _ => None,
    }
}

fn array_dim(declarator: &ast::Declarator, init: Option<&ast::Initializer>) -> Option<ArrayDim> {
    let suffix = declarator.array_suffixes().next()?;
    let mut exprs = suffix.children().filter_map(ast::Expr::cast);
    let Some(size) = exprs.next() else {
        return Some(match init {
            Some(ast::Initializer::Aggregate(agg)) => {
                let count = agg
                    .syntax()
                    .children()
                    .filter(|n| {
                        ast::Expr::can_cast(n.kind()) || n.kind() == SyntaxKind::AggregateInit
                    })
                    .count();
                u32::try_from(count).map_or(ArrayDim::Unknown, ArrayDim::Sized)
            }
            // `dim[0] = pr_immediate_strlen+1;`: the string's bytes and its terminator.
            Some(ast::Initializer::Value(ast::Expr::Literal(literal)))
                if literal.kind() == Some(ast::LiteralKind::String) =>
            {
                string_len(literal)
                    .and_then(|n| n.checked_add(1))
                    .map_or(ArrayDim::Unknown, ArrayDim::Sized)
            }
            Some(_) => ArrayDim::Unknown,
            None => ArrayDim::Unsized,
        });
    };
    let ast::Expr::Literal(literal) = size else {
        return Some(ArrayDim::Unknown);
    };
    let text = literal
        .token()
        .map(|t| t.text_lossless())
        .unwrap_or_default();
    Some(
        text.parse::<u32>()
            .map_or(ArrayDim::Unknown, ArrayDim::Sized),
    )
}

/// The qualifiers of the `Qualifier` children of `node` (FTEF-15.1), and of its attribute lists
/// (FTEF-21.9), in order.
fn qualifiers_of(node: &SyntaxNode) -> Vec<Qualifier> {
    let mut out = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Qualifier => {
                out.extend(Qualifier::of(&first_word(&child), &token_text(&child)))
            }
            SyntaxKind::AttributeList => {
                let Some(list) = ast::AttributeList::cast(child) else {
                    continue;
                };
                out.extend(list.attributes().filter_map(|a| attribute_qualifier(&a)));
            }
            _ => {}
        }
    }
    out
}

/// The qualifier an attribute is (FTEF-21.9): `alias` the name or string it gives, `accumulate`,
/// `eraseable` a `noref` and `__deprecated` its reason; `None` for an unknown one, which the
/// parser warned of.
fn attribute_qualifier(attribute: &ast::Attribute) -> Option<Qualifier> {
    let name = attribute.name()?;
    let word = name.text_lossless();
    let mut arguments = attribute.arguments();
    // `QCC_PR_CheckName` as the parser read the names, and `__deprecated` in any case after
    // its `__` (FTEF-20.2).
    let deprecated = word
        .strip_prefix("__")
        .is_some_and(|rest| rest.eq_ignore_ascii_case("deprecated"));
    match if deprecated {
        "__deprecated"
    } else {
        word.as_str()
    } {
        "alias" => {
            let target =
                arguments.find(|t| matches!(t.kind(), SyntaxKind::Name | SyntaxKind::StringLit))?;
            let text = target.text_lossless();
            Some(Qualifier::Alias(
                if target.kind() == SyntaxKind::StringLit {
                    text.trim_matches('"').to_owned()
                } else {
                    text
                },
            ))
        }
        "accumulate" => Some(Qualifier::Accumulate),
        "eraseable" => Some(Qualifier::Noref),
        "__deprecated" => {
            let reason: String = arguments
                .filter(|t| t.kind() == SyntaxKind::StringLit)
                .map(|t| t.text_lossless().trim_matches('"').to_owned())
                .collect();
            Some(Qualifier::Deprecated(
                attribute
                    .arguments()
                    .any(|t| t.kind() == SyntaxKind::StringLit)
                    .then_some(reason),
            ))
        }
        _ => None,
    }
}

/// The parameters of a parameter list as FTEQCC's redeclaration rules compare them
/// (FTEF-19.6, FTEF-19.7): each one's type, its `optional` and out marks and its default
/// value.
fn param_infos(list: &ast::ParamList, cx: Cx<'_>) -> Vec<RawParamInfo> {
    let types = cx.params(list);
    let params: Vec<ast::Param> = list.params().collect();
    let mut out = Vec::new();
    for (param, ty) in params.iter().zip(types) {
        let words: Vec<String> = param
            .syntax()
            .children()
            .filter(|n| n.kind() == SyntaxKind::Qualifier)
            .map(|n| first_word(&n).trim_start_matches('_').to_ascii_lowercase())
            .collect();
        let default = param.default_value();
        let default_literal = default.as_ref().and_then(|e| LiteralValue::of_expr(e, cx));
        out.push(RawParamInfo {
            ty,
            optional: words.iter().any(|w| w == "optional"),
            out: words.iter().any(|w| w == "out" || w == "inout")
                || param
                    .syntax()
                    .child_tokens()
                    .any(|t| t.kind() == SyntaxKind::Amp),
            default: default.as_ref().map(|e| raw_value(e, cx)),
            default_literal,
        });
    }
    out
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

/// The value of a literal initialiser (FTE-22.5). A float is kept as its bits, so two
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
    /// Under FTEQCC, a builtin initialiser: the `binum` FTEQCC binds and the engine name it
    /// reads, `None` when it reads none and names the builtin after the definition (FTE-23.7).
    Builtin {
        /// The builtin number (FTE-17.6), 0 for a builtin bound by name alone (FTE-17.5).
        number: i32,
        /// The engine name (`ast::Builtin::engine_name`).
        name: Option<String>,
    },
    /// Under FTEQCC, any other initialiser, as its tokens' text: two written the same way
    /// have the same value (FTEF-17.10).
    Text(String),
}

/// The number and engine name of a builtin function FTEQCC makes (FTE-23.7): it reuses a
/// function only for a builtin with both the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuiltinFunction {
    /// The `binum`.
    pub number: i32,
    /// The engine name, or the definition's own name when the initialiser gives none.
    pub name: String,
}

impl LiteralValue {
    /// The builtin function FTEQCC makes for this value initialising a definition named
    /// `definition`, if it is a builtin: `QCC_PR_GenerateBuiltinFunction(def.sym, binum,
    /// *fname?fname:def.sym->name)` names it after the definition when it reads no engine name
    /// (FTE-23.7).
    #[must_use]
    pub fn builtin_function(&self, definition: &str) -> Option<BuiltinFunction> {
        let Self::Builtin { number, name } = self else {
            return None;
        };
        Some(BuiltinFunction {
            number: *number,
            name: name.clone().unwrap_or_else(|| definition.to_owned()),
        })
    }

    /// The value of an initialiser under FTEQCC: a literal's, or else its text (FTEF-17.10).
    fn of_fteqcc(init: Option<&ast::Initializer>, cx: Cx<'_>) -> Option<Self> {
        let init = init?;
        match init {
            // A body is a definition, never a value two may share.
            ast::Initializer::Body(_) => None,
            ast::Initializer::Builtin(builtin) => {
                // FTE-23.7: a builtin is the `binum` FTEQCC binds (FTE-17.6), 0 when it reads
                // a string or name in place of the number (FTE-17.5), and its engine name. One
                // FTEQCC binds no number for, or reads neither for, is an error, compared by
                // its text.
                let in_place_of_number = builtin
                    .syntax()
                    .child_tokens()
                    .any(|t| matches!(t.kind(), SyntaxKind::StringLit | SyntaxKind::Name));
                let number = match builtin.number() {
                    Some(token) => {
                        qc_syntax::builtin_binum(&token.text_lossless(), qc_syntax::Dialect::Fteqcc)
                    }
                    None => in_place_of_number.then_some(0),
                };
                Some(number.map_or_else(
                    || Self::Text(token_text(init.syntax())),
                    |number| Self::Builtin {
                        number,
                        // FTEF-12.12: its strings are read with the flags they were lexed with.
                        name: builtin.engine_name(cx.lex_flags_at(builtin.syntax().span().start)),
                    },
                ))
            }
            ast::Initializer::Aggregate(_) | ast::Initializer::Value(_) => {
                let literal = match init {
                    ast::Initializer::Value(expr) => Self::of_literal(
                        expr,
                        Dialect::Fteqcc,
                        cx.lex_flags_at(expr.syntax().span().start),
                    ),
                    ast::Initializer::Aggregate(_)
                    | ast::Initializer::Body(_)
                    | ast::Initializer::Builtin(_) => None,
                };
                literal.or_else(|| Some(Self::Text(token_text(init.syntax()))))
            }
        }
    }

    /// The value of the expression `expr` under FTEQCC's comparison, or the vanilla one: a
    /// literal's, or under FTEQCC its text (FTEF-17.10).
    fn of_expr(expr: &ast::Expr, cx: Cx<'_>) -> Option<Self> {
        let flags = cx.lex_flags_at(expr.syntax().span().start);
        if cx.fte {
            Self::of_literal(expr, Dialect::Fteqcc, flags)
                .or_else(|| Some(Self::Text(token_text(expr.syntax()))))
        } else {
            Self::of_literal(expr, Dialect::Vanilla, flags)
        }
    }

    /// The value of `expr` when it is a literal, read as the dialect's lexer reads it: a
    /// number's or a vector's words (QC-4, QC-5; FTEF-3, FTEF-13.3), with the compiler flags
    /// `flags` it was lexed with (FTEF-12.12), a string as written.
    fn of_literal(expr: &ast::Expr, dialect: Dialect, flags: qc_lexer::LexFlags) -> Option<Self> {
        let ast::Expr::Literal(literal) = expr else {
            return None;
        };
        let token = literal.token()?;
        let text = token.text_lossless();
        match literal.kind()? {
            ast::LiteralKind::Float | ast::LiteralKind::Vector => {
                let lexed = qc_lexer::lex_flagged(text.as_bytes(), dialect, flags, &|_| None);
                match lexed.tokens.as_slice() {
                    [token] => match token.kind {
                        qc_lexer::TokenKind::Float(value) => Some(Self::Float(value.to_bits())),
                        qc_lexer::TokenKind::Vector(value) => {
                            Some(Self::Vector(value.map(f32::to_bits)))
                        }
                        _ => None,
                    },
                    _ => None,
                }
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
    /// The names a state header's frame uses, resolved before its next-think name is
    /// declared, as `QCC_PR_ParseState` reads them (FTEF-19.14).
    pub state_uses: Vec<RawUse>,
    /// A state header's frame and next function when they are literals, which FTEQCC types
    /// (FTEF-19.14).
    pub state_literals: Vec<(StatePart, RawValue)>,
    /// The parameters with no name, each by its number from 1 and its span (FTEF-19.1).
    pub unnamed_params: Vec<(usize, Span)>,
    /// What lowering found wrong in the body that resolution does not see: a `typedef` that
    /// defines no type or has storage qualifiers (FTEF-19.9).
    pub diagnostics: Vec<Diagnostic>,
    /// The names the body's `typedef`s define, each with where it stands; a local's type may
    /// be one defined before it (FTEF-19.20).
    pub typedefs: Vec<(String, usize)>,
    /// The next think functions the body's `state` statements name, each with its span, which
    /// `QCC_PR_ParseState` declares as a header's (FTEF-20.5).
    pub next_thinks: Vec<(String, Span)>,
    /// The aliases the body's `using` statements make (FTEF-20.5).
    pub aliases: Vec<RawAlias>,
}

/// One alias a `using` statement makes (FTEF-20.5): `QCC_PR_GetDef (NULL, name, pr_scope,
/// false, 0, 0)` finds the definition, and `QCC_PR_DummyDef` makes the alias of its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAlias {
    /// The name of the definition aliased, and its span.
    pub target: (String, Span),
    /// The alias's name, and its span.
    pub alias: (String, Span),
    /// The byte offset where the statement the alias is made for ends, after which its name is
    /// no longer found; `None` for a `using` without parentheses, which holds to the end of the
    /// function.
    pub scope_end: Option<usize>,
}

/// A part of a state header `[frame, next]` (QC-13.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatePart {
    /// The frame.
    Frame,
    /// The next think function.
    Next,
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
    /// The qualifiers written before its type (FTEF-15.1).
    pub qualifiers: Vec<Qualifier>,
    /// A value initialiser, as the checks of QC-11.3 and FTEF-19.2 read it.
    pub value: Option<RawValue>,
    /// The initialiser's value when it is a literal (FTE-22.5).
    pub literal: Option<LiteralValue>,
    /// The first array dimension, when the declarator has one (FTEF-19.8).
    pub array: Option<ArrayDim>,
    /// The byte offset where the innermost `for` statement it is defined in ends, after which
    /// its name is no longer found (FTEF-19.19), or under `subscope` the block (FTEF-21.12);
    /// `None` outside any.
    pub scope_end: Option<usize>,
    /// The compiler flags and pragma defaults where its declaration starts (FTEF-21.1).
    pub flags: CompilerFlags,
    /// Whether it is a C function declaration with `qcfuncs` off, which declares a global
    /// constant function rather than a local (FTEF-21.10).
    pub global_function: bool,
}

impl RawLocal {
    /// Whether the qualifier `q` marks it.
    #[must_use]
    pub fn has(&self, q: &Qualifier) -> bool {
        self.qualifiers.contains(q)
    }

    /// Whether it is `static`: written so, or under `#pragma defaultstatic 1`, which
    /// `nonstatic` does not undo in a body (FTEF-21.14).
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.has(&Qualifier::Static) || self.flags.default_on(PragmaDefault::Static)
    }
}

/// The reason a `deprecated` qualifier among `qualifiers` gives, `Some("")` for none.
fn deprecated_of(qualifiers: &[Qualifier]) -> Option<&str> {
    qualifiers.iter().find_map(|q| match q {
        Qualifier::Deprecated(reason) => Some(reason.as_deref().unwrap_or("")),
        Qualifier::Extern
        | Qualifier::Static
        | Qualifier::Shared
        | Qualifier::Const
        | Qualifier::Var
        | Qualifier::Nonstatic
        | Qualifier::Noref
        | Qualifier::Used
        | Qualifier::Nosave
        | Qualifier::Strip
        | Qualifier::Inline
        | Qualifier::Wrap
        | Qualifier::Weak
        | Qualifier::Accumulate
        | Qualifier::Auto
        | Qualifier::Alias(_) => None,
    })
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
    /// What lowering found wrong with a declaration that resolution does not see: a `typedef`
    /// that defines no type or has storage qualifiers (FTEF-19.9).
    pub diagnostics: Vec<Diagnostic>,
    /// The tag of every `struct` or `union` the file writes, nested in another's members too,
    /// with where it stands: `QCC_PR_ParseStruct` makes it a type there (FTEF-19.20).
    pub struct_tags: Vec<(String, usize)>,
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

impl Cx<'_> {
    /// The lexer's flags at offset `at` (FTEF-12.12): the last that holds from at or before it,
    /// else the defaults.
    fn lex_flags_at(self, at: usize) -> qc_lexer::LexFlags {
        qc_lexer::LexFlags::at(self.lex, at)
    }

    /// The compiler flags at offset `at` (FTEF-21.1): the last that holds from at or before it,
    /// else the defaults.
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
        Type::from_ast(ty, &|at| self.type_context(at))
    }

    /// The parameter types of `list`, as [`Self::ty`] reads each.
    fn params(self, list: &ast::ParamList) -> Vec<Type> {
        Type::params_of(list, &|at| self.type_context(at))
    }

    /// The function type returning `ret` with the parameters of `list` (FTEF-21.8).
    fn function(self, ret: Type, list: &ast::ParamList) -> Type {
        Type::function_of(ret, list, &|at| self.type_context(at))
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
        if let Some(tag) = tag {
            out.struct_tags
                .push((tag.text_lossless(), tag.span().start));
        }
    }
    // Walk the top-level nodes in source order, so a name an `enum` or `typedef` declares is
    // visible to the declarations after it (QC-12.5, QC-23.3). Only an FTEQCC parse has
    // `enum` and `typedef` nodes.
    for node in tree.items() {
        match node.kind() {
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_declaration(&decl, cx, &mut out.decls);
                }
            }
            SyntaxKind::EnumDecl => {
                if let Some(en) = ast::EnumDecl::cast(node) {
                    lower_enum(&en, cx, &mut out.decls);
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
                    let base = match st.name() {
                        Some(token) => {
                            out.decls.push(RawDecl::type_name(&token, TypeKind::Struct));
                            Type::Named(std::rc::Rc::from(token.text_lossless().as_str()))
                        }
                        None => Type::Unknown,
                    };
                    for field in st.fields() {
                        record_fields(&field, &mut out.members);
                    }
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

/// An `enum`'s names become global float constants (FTE-15.1, FTE-15.3), and a named enum's
/// type name is recorded as a typedef's is (FTE-15.4).
fn lower_enum(en: &ast::EnumDecl, cx: Cx<'_>, out: &mut Vec<RawDecl>) {
    // FTEF-19.10: in the legacy form `enum float { ... }` the word before the `{` is the base
    // type, which `QCC_PR_ParseEnum` tries first (`QCC_PR_ParseType(false, true, false)`).
    let legacy_word = en.name().filter(|t| legacy_base(t, cx).is_some());
    if legacy_word.is_none()
        && let Some(token) = en.name()
    {
        out.push(RawDecl::type_name(&token, TypeKind::Enum));
    }
    // FTEF-19.10: `enumflags` doubles its values, and a base type given after `:`, or before
    // the `{` in the legacy form, types them.
    let flags = en
        .syntax()
        .child_tokens()
        .any(|t| t.kind() == SyntaxKind::EnumKw && cx.spells(&t, Keyword::Enumflags, "enumflags"));
    let base_node = en.syntax().children().find_map(ast::Type::cast);
    let base = match (&base_node, &legacy_word) {
        (Some(node), _) => cx.ty(node),
        (None, Some(word)) => legacy_base(word, cx).unwrap_or(Type::Unknown),
        // FTEF-21.13: `basetype = (flag_assume_integer?type_integer:type_float);`, as
        // `QCC_PR_ParseEnum` starts.
        (None, None)
            if cx
                .flags_at(en.syntax().span().start)
                .has(FteFlag::AssumeInteger) =>
        {
            Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
        }
        (None, None) => Type::Simple(SimpleType::Float),
    };
    // FTEF-19.10: a base type with no `:` before it is FTEQCC's legacy form, and `enum class`
    // names its values only as `Name::value`.
    let legacy = legacy_word.is_some()
        || (base_node.is_some()
            && !en
                .syntax()
                .child_tokens()
                .any(|t| t.kind() == SyntaxKind::Colon));
    // `strictenum = QCC_PR_CheckName("class");`
    let strict = en
        .syntax()
        .child_tokens()
        .any(|t| {
            t.kind() == SyntaxKind::Name
                && cx
                    .keywords_at(t.span().start)
                    .names("class", &t.text_lossless())
        })
        .then(|| en.name().map(|t| t.text_lossless()))
        .flatten();
    for (index, node) in en.enumerators().enumerate() {
        let Some(token) = node.child_tokens().find(|t| t.kind() == SyntaxKind::Name) else {
            continue;
        };
        // FTEF-17.10: FTEQCC makes each enum name a new constant with `QCC_PR_DummyDef`, which
        // looks for no earlier definition, so an enum declared again is no redeclaration.
        let name = match &strict {
            Some(scope) => format!("{scope}::{}", token.text_lossless()),
            None => token.text_lossless(),
        };
        // FTEF-19.10: a strict enum's values are of the enum's own type.
        let ty = strict.as_ref().map_or_else(
            || base.clone(),
            |scope| Type::Named(std::rc::Rc::from(scope.as_str())),
        );
        let mut decl = RawDecl::new(name, token.span(), ty);
        let value = node.children().find_map(ast::Expr::cast);
        if let Some(expr) = &value {
            lower_expr(expr, cx, &mut decl.uses);
        }
        let number = match value.as_ref().and_then(|e| {
            LiteralValue::of_literal(e, Dialect::Fteqcc, cx.lex_flags_at(e.syntax().span().start))
        }) {
            Some(LiteralValue::Float(bits)) => Some(bits),
            _ => None,
        };
        decl.enum_item = Some(EnumItem {
            flags,
            base: base.clone(),
            value: value.as_ref().map(|e| raw_value(e, cx)),
            number,
            legacy: legacy && index == 0,
            first: index == 0,
        });
        out.push(decl);
    }
}

/// The base type the word `token` before an enum's `{` names in FTEQCC's legacy form
/// (FTEF-19.10): a simple type, or a C type word or registered type name read with the keywords
/// in force (FTEF-2.8, FTEF-20.9); `None` for a name, the enum's own.
fn legacy_base(token: &qc_syntax::SyntaxToken, cx: Cx<'_>) -> Option<Type> {
    let word = token.text_lossless();
    let simple = match word.as_str() {
        "float" => SimpleType::Float,
        "vector" => SimpleType::Vector,
        "string" => SimpleType::String,
        "entity" => SimpleType::Entity,
        "void" => SimpleType::Void,
        word => {
            let context = cx.c_context(token.span().start);
            let (_, c_type) = CWords::read([word], context);
            return c_type.map(|_| Type::named(word, context));
        }
    };
    Some(Type::Simple(simple))
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
fn lower_typedef(td: &ast::TypedefDecl, cx: Cx<'_>, out: &mut FileDecls) {
    for field in td
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::StructField)
    {
        record_fields(&field, &mut out.members);
    }
    if let Some(en) = td.syntax().children().find_map(ast::EnumDecl::cast) {
        lower_enum(&en, cx, &mut out.decls);
    }
    out.diagnostics
        .extend(typedef_diagnostics(td, cx, TypedefScope::File));
    let Some(token) = td.name() else {
        return;
    };
    let mut decl = RawDecl::type_name(&token, TypeKind::Typedef);
    // FTEF-19.9: the type it names, which a typedef of the same name must name again. An
    // array typedef names a type the model does not follow.
    let array = td
        .syntax()
        .children()
        .any(|n| n.kind() == SyntaxKind::ArraySuffix);
    // FTEF-21.7: the name is a declarator's, so its parameter lists and C's `(*name)(params)`
    // make the function type it names.
    let pointer = td.pointer_depth() > 0 || td.pointer_form().is_some_and(|stars| stars > 1);
    let node = td.syntax().children().find_map(ast::Type::cast);
    // FTEF-19.21: with its type left out, a typedef names an `int`.
    let omitted = node.is_none()
        && td
            .syntax()
            .children()
            .all(|n| !matches!(n.kind(), SyntaxKind::StructType | SyntaxKind::EnumDecl));
    decl.typedef = Some(if omitted {
        Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
    } else {
        node.filter(|_| !array && !pointer)
            .map_or(Type::Unknown, |t| {
                let mut ty = cx.ty(&t);
                for list in td.prefix_params() {
                    ty = cx.function(ty, &list);
                }
                if let Some(list) = td.params().filter(|_| td.pointer_form().is_some()) {
                    ty = cx.function(ty, &list);
                }
                ty
            })
    });
    out.decls.push(decl);
}

/// Where a `typedef` is: `QCC_PR_ParseDefs` drops `extern` from a definition whose type is no
/// function's only `if (externfnc && !pr_scope ...)`, at file level (FTEF-21.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypedefScope {
    /// Outside every function.
    File,
    /// In a function's body, where the `extern` stays.
    Body,
}

/// FTEF-19.9: what is wrong with a `typedef` alone: `typedef;` and `typedef float;` define no
/// type, a warning; a qualifier is "bad combination of modifiers with typedef", but `auto`,
/// `register` and `nonstatic`, and `static`, `noref` and `nosave` when a `#pragma` makes them the
/// default (FTEF-21.14). Before it, as for any declaration at file level, `extern` on a type that
/// is no function's is dropped, with its warning when `qcfuncs` is on (FTEF-21.10), and more than one
/// storage class, `typedef` among them and `static` by its default, is "Conflicting storage
/// classes." (FTEF-19.16).
fn typedef_diagnostics(td: &ast::TypedefDecl, cx: Cx<'_>, scope: TypedefScope) -> Vec<Diagnostic> {
    let Some(token) = td.name() else {
        return vec![Diagnostic {
            code: Code::W0215,
            message: "typedef defines no types".to_owned(),
            span: td.syntax().span(),
        }];
    };
    let mut qualifiers = qualifiers_of(td.syntax());
    let flags = cx.flags_at(td.syntax().span().start);
    let mut out = Vec::new();
    let warn = |message: &str| Diagnostic {
        code: Code::W0214,
        message: message.to_owned(),
        span: token.span(),
    };
    let ty_node = td.syntax().children().find_map(ast::Type::cast);
    if scope == TypedefScope::File
        && qualifiers.contains(&Qualifier::Extern)
        && ty_node.as_ref().is_some_and(|t| no_function(t, &cx.ty(t)))
    {
        qualifiers.retain(|q| *q != Qualifier::Extern);
        let after = ty_node.as_ref().map_or(0, |t| t.syntax().span().end);
        if cx.next_flags(after).has(FteFlag::Qcfuncs) {
            out.push(warn("extern keyword may only apply to qc functions."));
        }
    }
    let has = |q: &Qualifier| qualifiers.contains(q);
    let statics = has(&Qualifier::Static)
        || (flags.default_on(PragmaDefault::Static) && !has(&Qualifier::Nonstatic));
    let storage = 1_usize
        .saturating_add(usize::from(has(&Qualifier::Extern)))
        .saturating_add(usize::from(has(&Qualifier::Auto)))
        .saturating_add(usize::from(statics))
        .saturating_add(usize::from(has(&Qualifier::Shared)))
        .saturating_add(usize::from(
            qualifiers.iter().any(|q| matches!(q, Qualifier::Alias(_))),
        ));
    if storage > 1 {
        out.push(warn("Conflicting storage classes."));
    }
    if has(&Qualifier::Const) && has(&Qualifier::Var) {
        out.push(warn("Conflicting constness."));
    }
    let bad = qualifiers.iter().any(|q| match q {
        Qualifier::Static => !flags.default_on(PragmaDefault::Static),
        Qualifier::Noref => !flags.default_on(PragmaDefault::Noref),
        Qualifier::Nosave => !flags.default_on(PragmaDefault::Nosave),
        Qualifier::Auto | Qualifier::Nonstatic => false,
        Qualifier::Extern
        | Qualifier::Shared
        | Qualifier::Const
        | Qualifier::Var
        | Qualifier::Used
        | Qualifier::Strip
        | Qualifier::Inline
        | Qualifier::Wrap
        | Qualifier::Weak
        | Qualifier::Accumulate
        | Qualifier::Deprecated(_)
        | Qualifier::Alias(_) => true,
    });
    if bad {
        out.push(Diagnostic {
            code: Code::E0224,
            message: format!(
                "bad combination of modifiers with typedef (defining {})",
                token.text_lossless()
            ),
            span: token.span(),
        });
    }
    out
}

/// An FTEQCC class: its name as a type, and its parent and members (FTEF-9). A member
/// function's body written in the class is lowered with the class as its scope.
fn lower_class(class: &ast::ClassDecl, cx: Cx<'_>, out: &mut FileDecls) {
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
                // FTEF-20.11: the label's word, `__` and any case as the parser took it.
                let word = first_word(&node).to_ascii_lowercase();
                if word.ends_with("nonvirtual") {
                    assume_virtual = false;
                } else if word.ends_with("virtual") {
                    assume_virtual = true;
                }
            }
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_member(&decl, &class_name, assume_virtual, cx, &mut raw.members);
                }
            }
            SyntaxKind::AccessorMember => {
                if let Some(member) = ast::AccessorMember::cast(node) {
                    out.members.extend(member.name().map(|t| t.text_lossless()));
                    lower_accessor_body(&member, &class_name, cx, &mut out.decls);
                }
            }
            _ => {}
        }
    }
    let mut decl = RawDecl::type_name(&name, TypeKind::Class);
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
    cx: Cx<'_>,
    out: &mut Vec<RawMember>,
) {
    let words = qualifiers(decl);
    // FTEF-20.11: a member's words are `__` and any case as the parser took them.
    let has = |word: &str| {
        words
            .iter()
            .any(|w| w.trim_start_matches('_').eq_ignore_ascii_case(word))
    };
    let virtual_ = has("virtual") || (assume_virtual && !has("nonvirtual") && !has("static"));
    let base = decl.ty().map_or(Type::Unknown, |t| cx.ty(&t));
    let mut declared = false;
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        declared = true;
        let ty = declarator_type(&base, &declarator, cx);
        let kind = if !ty.is_function() {
            MemberKind::Field
        } else if virtual_ {
            MemberKind::Virtual
        } else {
            MemberKind::Method
        };
        let init = declarator.initializer();
        let body = match &init {
            Some(ast::Initializer::Body(fn_body)) => Some(lower_body(fn_body, &ty, cx)),
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
            ty: cx.ty(&ast::Type::Function(func)),
            kind: MemberKind::Method,
            initialized: false,
            body: None,
        });
    }
}

/// An accessor: its name as a type, its members' names, and its member bodies as functions
/// named `Name::get_member` and `Name::set_member` (FTEF-10.1, FTEF-10.2).
fn lower_accessor(acc: &ast::AccessorDecl, cx: Cx<'_>, out: &mut FileDecls) {
    let Some(name) = acc.name() else {
        return;
    };
    let accessor = name.text_lossless();
    out.decls
        .push(RawDecl::type_name(&name, TypeKind::Accessor));
    for member in acc.members() {
        out.members.extend(member.name().map(|t| t.text_lossless()));
        lower_accessor_body(&member, &accessor, cx, &mut out.decls);
    }
}

/// The body of an accessor member, as a function whose parameters are `this`, `value` and the
/// index name (FTEF-10.2); a member whose value is a function is a use of it.
fn lower_accessor_body(
    member: &ast::AccessorMember,
    owner: &str,
    cx: Cx<'_>,
    out: &mut Vec<RawDecl>,
) {
    let set = member.is_set();
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
            lower_block(&block, cx, &mut body);
        }
        decl.body = Some(body);
        decl.initialized = true;
    } else if let Some(value) = member.value() {
        lower_expr(&value, cx, &mut decl.uses);
        decl.initialized = true;
    }
    out.push(decl);
}

fn lower_declaration(decl: &ast::Declaration, cx: Cx<'_>, out: &mut Vec<RawDecl>) {
    let fte = cx.fte;
    // FTEF-8.2: an enum written as a type declares its names first.
    for en in decl.syntax().descendants().filter_map(ast::EnumDecl::cast) {
        lower_enum(&en, cx, out);
    }
    let base = decl
        .ty()
        .map_or_else(|| omitted_type(decl.syntax(), fte), |t| cx.ty(&t));
    let first = out.len();
    lower_declarators(decl.declarators(), &base, decl.ty(), cx, out);
    // FTEF-15.1: the qualifiers before the type hold for every declarator.
    let qualifiers = qualifiers_of(decl.syntax());
    // FTEF-21.10: `extern` is tested against the base type, read once the type is.
    let extern_base = (fte && qualifiers.contains(&Qualifier::Extern)).then(|| {
        let after = decl.ty().map_or(0, |t| t.syntax().span().end);
        ExternBase {
            ty: base.clone(),
            qcfuncs: cx.next_flags(after).has(FteFlag::Qcfuncs),
        }
    });
    let flags = cx.flags_at(decl.syntax().span().start);
    for raw in out.iter_mut().skip(first) {
        if raw.enum_item.is_none() {
            raw.qualifiers.clone_from(&qualifiers);
            raw.flags = flags;
            raw.extern_base.clone_from(&extern_base);
        }
    }
}

/// Whether the declaration type `node`, lowered to `ty`, is surely no function's, which
/// `QCC_PR_ParseDefs` tests its `extern` against (FTEF-21.10): a vanilla or C type word, a
/// field or a pointer. A type a program names may be a function's typedef.
fn no_function(node: &ast::Type, ty: &Type) -> bool {
    match node {
        ast::Type::Field(_) | ast::Type::Pointer(_) => true,
        ast::Type::Function(_) => false,
        ast::Type::Simple(_) => match ty {
            Type::Simple(_) => true,
            Type::Named(name) => CScalar::named(name).is_some(),
            Type::Unknown | Type::Field(_) | Type::Function(_) | Type::Pointer(_) => false,
        },
    }
}

/// The type a declarator declares of a declaration whose type is `base` (FTEF-4.1, FTEF-21.7):
/// a pointer for each `*` before it, a function returning the type so far for each parameter
/// list before its name, and for C's function pointer `(*name)(params)` the function of those
/// parameters, a pointer to it for each `*` past the first; then a C-style declarator's own list
/// after the name (FTE-17.1).
fn declarator_type(base: &Type, declarator: &ast::Declarator, cx: Cx<'_>) -> Type {
    let mut ty = base.clone();
    for _ in 0..declarator.pointer_depth() {
        ty = Type::Pointer(std::rc::Rc::new(ty));
    }
    for list in declarator.prefix_params() {
        ty = cx.function(ty, &list);
    }
    if let Some(list) = declarator.params() {
        ty = cx.function(ty, &list);
    }
    if let Some(stars) = declarator.pointer_form() {
        for _ in 1..stars {
            ty = Type::Pointer(std::rc::Rc::new(ty));
        }
    }
    ty
}

/// The names `declarators` declare with the type `base`, whose syntax is `ty` when it has
/// default values to read.
fn lower_declarators(
    declarators: impl Iterator<Item = ast::Declarator>,
    base: &Type,
    ty_node: Option<ast::Type>,
    cx: Cx<'_>,
    out: &mut Vec<RawDecl>,
) {
    let fte = cx.fte;
    let mut pointer_var = false;
    for declarator in declarators {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let name = name_token.text_lossless();
        let span = name_token.span();
        // FTEF-4.1, FTE-17.1, FTEF-21.7: the declarator's `*`s and parameter lists.
        let ty = declarator_type(base, &declarator, cx);
        let init = declarator.initializer();
        pointer_var |= declarator.pointer_form().is_some();
        let mut raw = RawDecl::new(name, span, ty.clone());
        raw.pointer_var = pointer_var;
        // FTEF-21.11: `flag_assumevar` as `QCC_PR_ParseDefs` reads it, after the `=`.
        let after_eq = init
            .as_ref()
            .map_or(declarator.syntax().span().end, |i| i.syntax().span().start);
        raw.assumevar = cx.flags_at(after_eq).has(FteFlag::Assumevar);
        raw.initialized = init.is_some();
        // QCD-3.6: the vanilla profile compares no two values, so it keeps none.
        raw.literal = if fte {
            LiteralValue::of_fteqcc(init.as_ref(), cx)
        } else {
            None
        };
        raw.scope = declarator.scope().map(|t| t.text_lossless());
        raw.builtin = matches!(init, Some(ast::Initializer::Builtin(_)));
        raw.array = array_dim(&declarator, init.as_ref());
        if let Some(ast::Initializer::Value(expr)) = &init {
            raw.value = Some(raw_value(expr, cx));
        }
        // FTEF-19.6, FTEF-19.7: the parameters of the function type, from the declarator's
        // own list or the type's.
        if let Some(list) = declarator
            .params()
            .or_else(|| ty_node.as_ref().and_then(function_params))
        {
            raw.params = param_infos(&list, cx);
        }
        match &init {
            Some(ast::Initializer::Body(fn_body)) => {
                raw.next_think = next_think_of(fn_body);
                raw.body = Some(lower_body(fn_body, &ty, cx));
            }
            // FTE-10.2: a value initialiser may name other definitions, `const float B = A;`.
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, cx, &mut raw.uses),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, cx, &mut raw.uses);
                }
            }
            Some(ast::Initializer::Builtin(_)) | None => {}
        }
        // FTE-18.2: a parameter's default value may name definitions too.
        for list in ty_node
            .as_ref()
            .and_then(function_params)
            .into_iter()
            .chain(declarator.params())
        {
            for param in list.params() {
                if let Some(value) = param.default_value() {
                    lower_expr(&value, cx, &mut raw.uses);
                }
            }
        }
        // FTEF-5.1: an array's size expressions name definitions too.
        for suffix in declarator.array_suffixes() {
            for expr in suffix.children().filter_map(ast::Expr::cast) {
                lower_expr(&expr, cx, &mut raw.uses);
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

/// The next-think name of a state header `[frame, next]`, if present (QC-13.6).
fn next_think_of(fn_body: &ast::FunctionBody) -> Option<(String, Span)> {
    let state = fn_body.state()?;
    let token = state.next()?;
    Some((token.text_lossless(), token.span()))
}

fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, cx: Cx<'_>) -> RawBody {
    let mut body = RawBody::default();
    collect_params(
        fn_ty,
        fn_body,
        cx,
        &mut body.params,
        &mut body.unnamed_params,
    );
    // FTEF-18.3, FTEF-18.28: a state header's expressions are values of the function, read
    // before its next-think name is declared (FTEF-19.14).
    if let Some(state) = fn_body.state() {
        let cycle = state.is_cycle();
        for (index, expr) in state.exprs().enumerate() {
            lower_expr(&expr, cx, &mut body.state_uses);
            let part = if index == 0 || cycle {
                StatePart::Frame
            } else {
                StatePart::Next
            };
            let value = raw_value(&expr, cx);
            if matches!(value.kind, ValueKind::Literal(_)) {
                body.state_literals.push((part, value));
            }
        }
    }
    if let Some(block) = fn_body.block() {
        lower_block(&block, cx, &mut body);
    }
    body
}

/// The parameters named in the function type of the defining declaration (QC-13.2).
///
/// The parameter names come from the `FunctionType`'s `ParamList`; the semantic types come
/// from the same list, matched by position to [`Type::from_ast`]'s view of the function
/// type (QC-10.3). An FTEQCC parameter may have no name (FTEF-15.3); it declares nothing.
fn collect_params(
    fn_ty: &Type,
    fn_body: &ast::FunctionBody,
    cx: Cx<'_>,
    out: &mut Vec<RawParam>,
    unnamed: &mut Vec<(usize, Span)>,
) {
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
    let count = param_list.params().count();
    for (index, param) in param_list.params().enumerate() {
        let Some(name_token) = param.name() else {
            // FTEF-19.1: a parameter of a function with a body has a name, or FTEQCC's warning
            // "Parameter N of NAME is not named"; a lone `void` is no parameter (FTEF-15.3).
            let lone_void = count == 1
                && param
                    .ty()
                    .is_some_and(|t| cx.ty(&t) == Type::Simple(SimpleType::Void));
            if !lone_void {
                unnamed.push((index.saturating_add(1), param.syntax().span()));
            }
            continue;
        };
        let ty = fty
            .and_then(|f| f.params.get(index).cloned())
            .or_else(|| param.ty().map(|t| cx.ty(&t)))
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

fn lower_block(block: &ast::Block, cx: Cx<'_>, body: &mut RawBody) {
    let first = body.locals.len();
    for stmt in block.statements() {
        lower_stmt(&stmt, cx, body);
    }
    // FTEF-21.12: `if (pr_subscopedlocals) { //remove any new locals from the hashtable.`, read
    // once the `}` is, with the token after it lexed.
    let end = block.syntax().span().end;
    let subscope_somewhere = cx
        .flags
        .iter()
        .any(|(_, flags)| flags.has(FteFlag::PrSubscopedlocals));
    if cx.fte && subscope_somewhere && cx.next_flags(end).has(FteFlag::PrSubscopedlocals) {
        for local in body.locals.iter_mut().skip(first) {
            local.scope_end.get_or_insert(end);
        }
    }
    // FTEF-19.9: a `typedef` in a body is checked as one at file level.
    for td in block.syntax().children().filter_map(ast::TypedefDecl::cast) {
        body.diagnostics
            .extend(typedef_diagnostics(&td, cx, TypedefScope::Body));
        if let Some(name) = td.name() {
            body.typedefs
                .push((name.text_lossless(), name.span().start));
        }
    }
}

fn lower_stmt(stmt: &ast::Stmt, cx: Cx<'_>, body: &mut RawBody) {
    match stmt {
        ast::Stmt::Local(local) => lower_local(local, cx, body),
        // FTEF-21.17: a body's `extern` block holds local definitions.
        ast::Stmt::Extern(block) => {
            for local in block.items().into_iter().filter_map(ast::LocalDecl::cast) {
                lower_local(&local, cx, body);
            }
        }
        ast::Stmt::Block(block) => lower_block(block, cx, body),
        ast::Stmt::Return(ret) => {
            if let Some(expr) = ret.value() {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
        ast::Stmt::If(if_stmt) => {
            if let Some(cond) = if_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
            if let Some(then) = if_stmt.then_branch() {
                lower_stmt(&then, cx, body);
            }
            if let Some(els) = if_stmt.else_branch() {
                lower_stmt(&els, cx, body);
            }
        }
        ast::Stmt::While(while_stmt) => {
            if let Some(cond) = while_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
            if let Some(inner) = while_stmt.body() {
                lower_stmt(&inner, cx, body);
            }
        }
        ast::Stmt::Do(do_stmt) => {
            if let Some(inner) = do_stmt.body() {
                lower_stmt(&inner, cx, body);
            }
            if let Some(cond) = do_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
        }
        // FTEQCC `for (init; cond; step) body` (FTE-2). The init clause may declare a loop
        // local; the clauses hold expressions whose names are used (QC-12.5). The locals the
        // statement defines, in its clauses or its body, leave scope at its end (FTEF-19.19);
        // a `for` inside it has already ended its own.
        ast::Stmt::For(for_stmt) => {
            let first = body.locals.len();
            for clause in for_stmt.clauses() {
                if let Some(local) = clause.children().find_map(ast::LocalDecl::cast) {
                    lower_local(&local, cx, body);
                } else {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        lower_expr(&expr, cx, &mut body.uses);
                    }
                }
            }
            if let Some(inner) = for_stmt.body() {
                lower_stmt(&inner, cx, body);
            }
            let end = for_stmt.syntax().span().end;
            for local in body.locals.iter_mut().skip(first) {
                local.scope_end.get_or_insert(end);
            }
        }
        // FTEQCC `switch (e) { ... }` (FTE-3): the scrutinee, each `case` value and the
        // statements of the body are lowered.
        ast::Stmt::Switch(switch_stmt) => {
            if let Some(scrutinee) = switch_stmt.scrutinee() {
                lower_expr(&scrutinee, cx, &mut body.uses);
            }
            if let Some(block) = switch_stmt.block() {
                for node in block.syntax().children() {
                    if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                        // FTEF-18.10: a range's end is a value too.
                        for value in label.value().into_iter().chain(label.range_end()) {
                            lower_expr(&value, cx, &mut body.uses);
                        }
                    } else if let Some(stmt) = ast::Stmt::cast(node) {
                        lower_stmt(&stmt, cx, body);
                    }
                }
            }
        }
        // `break;`, `continue;` (FTE-4) and `;` (FTE-21.5) bind no names and use none, and
        // neither do a label and `goto` (FTEF-18.23), whose names are labels, not values.
        ast::Stmt::Break(_)
        | ast::Stmt::Continue(_)
        | ast::Stmt::Empty(_)
        | ast::Stmt::Label(_)
        | ast::Stmt::Goto(_)
        | ast::Stmt::Exit(_) => {}
        // FTEF-20.5: an `asm` statement's `local` declarations define locals; its operands are
        // code generation's (FTEF-19.30).
        ast::Stmt::Asm(asm) => {
            for local in asm.locals() {
                lower_local(&local, cx, body);
            }
        }
        // FTEF-20.5: `loop s` runs its statement.
        ast::Stmt::Loop(loop_stmt) => {
            if let Some(inner) = loop_stmt.body() {
                lower_stmt(&inner, cx, body);
            }
        }
        // FTEF-20.5: `thinktime e : t;` uses its operands and, unless the target is Hexen II's,
        // which has `OP_THINKTIME`, `nextthink` and `time`, which `QCC_PR_GetSRef` looks up by
        // name.
        ast::Stmt::Thinktime(thinktime) => {
            for operand in thinktime.operands() {
                lower_expr(&operand, cx, &mut body.uses);
            }
            let span = thinktime
                .syntax()
                .token_of(SyntaxKind::ThinktimeKw)
                .map_or_else(|| thinktime.syntax().span(), |t| t.span());
            if !cx.keywords_at(span.start).hexen2_target() {
                for name in ["nextthink", "time"] {
                    body.uses
                        .push(RawUse::new(name.to_owned(), span, UseKind::Value));
                }
            }
        }
        // FTEF-20.5: `state [frame, next];` reads a state header as a function's: its values,
        // and a next think function named alone, declared when it is new.
        ast::Stmt::State(state_stmt) => {
            if let Some(state) = state_stmt.state() {
                let named = state.next();
                for expr in state.exprs() {
                    lower_expr(&expr, cx, &mut body.uses);
                }
                if let Some(token) = named {
                    body.next_thinks.push((token.text_lossless(), token.span()));
                }
            }
        }
        // FTEF-20.5: `using` makes an alias of each definition it names, in parentheses for its
        // statement, else for the rest of the function.
        ast::Stmt::Using(using) => {
            let inner = using.body();
            let scope_end = inner.as_ref().map(|_| using.syntax().span().end);
            for alias in using.aliases() {
                let (Some(target), Some(name)) = (alias.target(), alias.alias()) else {
                    continue;
                };
                body.aliases.push(RawAlias {
                    target: (target.text_lossless(), target.span()),
                    alias: (name.text_lossless(), name.span()),
                    scope_end,
                });
            }
            if let Some(inner) = inner {
                lower_stmt(&inner, cx, body);
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            if let Some(expr) = expr_stmt.expr() {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
    }
}

/// The type of a declaration that writes none: under FTEQCC after a qualifier `int`,
/// `type_integer`, as `QCC_PR_ParseDefs` assumes when `hadmodifier` is set (FTEF-19.21); else
/// unknown.
fn omitted_type(node: &SyntaxNode, fte: bool) -> Type {
    if fte && node.children().any(|n| n.kind() == SyntaxKind::Qualifier) {
        Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
    } else {
        Type::Unknown
    }
}

fn lower_local(local: &ast::LocalDecl, cx: Cx<'_>, body: &mut RawBody) {
    let fte = cx.fte;
    let base = local
        .ty()
        .map_or_else(|| omitted_type(local.syntax(), fte), |t| cx.ty(&t));
    // QC-11.9 holds only in vanilla: FTEQCC allows a field or function local (FTE-21.2).
    let bad = !fte && (base.is_field() || base.is_function());
    for declarator in local.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let ty = declarator_type(&base, &declarator, cx);
        let init = declarator.initializer();
        // A local initialiser is a constant value; if it is a non-constant expression we
        // still record any names it uses, so references in it resolve (QC-12.5).
        match &init {
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, cx, &mut body.uses),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, cx, &mut body.uses);
                }
            }
            _ => {}
        }
        for suffix in declarator.array_suffixes() {
            for expr in suffix.children().filter_map(ast::Expr::cast) {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
        let value = match &init {
            Some(ast::Initializer::Value(expr)) => Some(raw_value(expr, cx)),
            _ => None,
        };
        let literal = if fte {
            LiteralValue::of_fteqcc(init.as_ref(), cx)
        } else {
            None
        };
        // FTEF-21.10: `if (!flag_qcfuncs && pr_scope) isconstant = externfnc = true; //in C, a
        // locally defined function refers to an external one.`, at a C-style declarator's `(`.
        let global_function = fte
            && declarator.params().is_some_and(|list| {
                !cx.flags_at(list.syntax().span().start)
                    .has(FteFlag::Qcfuncs)
            });
        body.locals.push(RawLocal {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
            initialized: init.is_some(),
            bad_field_or_function: bad,
            qualifiers: qualifiers_of(local.syntax()),
            value,
            literal,
            array: array_dim(&declarator, init.as_ref()),
            scope_end: None,
            flags: cx.flags_at(local.syntax().span().start),
            global_function,
        });
    }
}

/// What a member access is made on, read from the base expression's shape (FTEF-17.3).
fn member_base(base: &ast::Expr, cx: Cx<'_>) -> MemberBase {
    match base {
        ast::Expr::Name(name_ref) if name_ref.scope().is_none() => name_ref
            .name()
            .map_or(MemberBase::Unknown, |t| MemberBase::Name(t.text_lossless())),
        ast::Expr::Cast(cast) => cast
            .ty()
            .map_or(MemberBase::Unknown, |t| MemberBase::Type(cx.ty(&t))),
        ast::Expr::Call(call) => match call.callee() {
            Some(ast::Expr::Name(name_ref)) if name_ref.scope().is_none() => name_ref
                .name()
                .map_or(MemberBase::Unknown, |t| MemberBase::Call(t.text_lossless())),
            _ => MemberBase::Unknown,
        },
        ast::Expr::Paren(paren) => paren
            .expr()
            .map_or(MemberBase::Unknown, |e| member_base(&e, cx)),
        _ => MemberBase::Unknown,
    }
}

fn lower_expr(expr: &ast::Expr, cx: Cx<'_>, uses: &mut Vec<RawUse>) {
    let fte = cx.fte;
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
                lower_expr(&inner, cx, uses);
            }
        }
        ast::Expr::Not(not) => {
            if let Some(operand) = not.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        ast::Expr::Binary(bin) => {
            if let Some(lhs) = bin.lhs() {
                lower_expr(&lhs, cx, uses);
            }
            if let Some(rhs) = bin.rhs() {
                lower_expr(&rhs, cx, uses);
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
                lower_expr(base, cx, uses);
            }
            match field.field() {
                Some(ast::Expr::Name(name_ref)) if fte => {
                    if let Some(token) = name_ref.name() {
                        let mut use_ =
                            RawUse::new(token.text_lossless(), token.span(), UseKind::Member);
                        use_.scope = name_ref.scope().map(|t| t.text_lossless());
                        use_.base = base
                            .as_ref()
                            .map_or(MemberBase::Unknown, |b| member_base(b, cx));
                        uses.push(use_);
                    }
                }
                Some(field_expr) => lower_expr(&field_expr, cx, uses),
                None => {}
            }
        }
        ast::Expr::Call(call) => {
            let callee = call.callee();
            if let Some(callee) = &callee {
                lower_expr(callee, cx, uses);
            }
            // FTEF-17.4: `autocvar(name, default)` names the cvar, which is no value. (`sizeof`
            // and the type it may take are a `SizeofExpr`, FTEF-18.12.)
            let callee_name = match &callee {
                Some(ast::Expr::Name(n)) if fte => n.name().map(|t| t.text_lossless()),
                _ => None,
            };
            let skip = usize::from(callee_name.as_deref() == Some("autocvar"));
            if let Some(args) = call.args() {
                for arg in args.args().skip(skip) {
                    lower_expr(&arg, cx, uses);
                }
            }
        }
        // FTEQCC `e++`/`e--` (FTE-5): the operand is a value use.
        ast::Expr::Postfix(post) => {
            if let Some(operand) = post.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        // FTEQCC `cond ? a : b` (FTE-8): the condition and both arms are values.
        ast::Expr::Ternary(tern) => {
            if let Some(cond) = tern.condition() {
                lower_expr(&cond, cx, uses);
            }
            let (a, b) = tern.arms();
            if let Some(a) = a {
                lower_expr(&a, cx, uses);
            }
            if let Some(b) = b {
                lower_expr(&b, cx, uses);
            }
        }
        // FTEQCC `-e`, `+e`, `~e` (FTE-19.4), `++e`, `&e`, `*e` (FTEF-16.7) and `(type) e`
        // (FTE-19.6): the operand is a value.
        ast::Expr::Unary(unary) => {
            if let Some(operand) = unary.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        ast::Expr::Cast(cast) => {
            if let Some(operand) = cast.operand() {
                lower_expr(&operand, cx, uses);
            }
            for element in cast
                .syntax()
                .children()
                .filter_map(ast::AggregateInit::cast)
                .flat_map(|agg| agg.elements().collect::<Vec<_>>())
            {
                lower_expr(&element, cx, uses);
            }
        }
        // FTEQCC `e[i]` (FTE-13): the base and index are values.
        ast::Expr::Index(index) => {
            if let Some(base) = index.base() {
                lower_expr(&base, cx, uses);
            }
            if let Some(i) = index.index() {
                lower_expr(&i, cx, uses);
            }
        }
        // FTEQCC `[x, y, z]` (FTEF-16.3): each component is a value.
        ast::Expr::Vector(vector) => {
            for element in vector.elements() {
                lower_expr(&element, cx, uses);
            }
        }
        // FTEQCC `sizeof` (FTEF-18.12): an expression it measures names values; a type does
        // not.
        ast::Expr::Sizeof(sizeof) => {
            if let Some(operand) = sizeof.operand() {
                lower_expr(&operand, cx, uses);
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
