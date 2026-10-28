// SPDX-License-Identifier: GPL-3.0-or-later

//! Declarations: declarators, qualifiers, attributes, array sizes, parameters and the
//! values of initialisers, as `QCC_PR_ParseDefs` and `PR_ParseDefs` read them (QC-11, FTEF-15,
//! FTEF-19).

use qc_diag::Span;
use qc_preproc::{FteFlag, PragmaDefault};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{CompilerFlags, Dialect, SyntaxKind, SyntaxNode};

use crate::cast::{ConstType, Literal};
use crate::ty::Type;

use super::body::{RawBody, RawParam, lower_body, next_think_of, omitted_type};
use super::expr::{RawUse, lower_expr};
use super::types::{EnumItem, RawClass, TypeKind, lower_enum};
use super::{Cx, first_word, token_text};

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
    /// initialised twice (FTE-22.6, FTEF-17.10); `None` for none, and under the vanilla
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
    /// Whether this stands for a type written with no declarator, `float (x);`, which only has
    /// its type names checked (FTEF-20.12).
    pub type_only: bool,
    /// For an accessor, its base type after `:`, `None` inside `Some` when it has none
    /// (FTEF-10.6).
    pub accessor: Option<Option<Type>>,
    /// The qualifiers written before the type (FTEF-15.1), in order.
    pub qualifiers: Vec<Qualifier>,
    /// A value initialiser, as the checks of QC-11.3 and FTEF-19.31 read it.
    pub value: Option<RawValue>,
    /// The first array dimension, when the declarator has one (FTEF-5, FTEF-19.8).
    pub array: Option<ArrayDim>,
    /// How many array suffixes the declarator has (FTEF-19.8).
    pub array_dims: usize,
    /// For an aggregate initialiser, the first token inside its braces and its span: a
    /// two-dimensional array needs a `{` there (FTEF-19.8).
    pub aggregate_first: Option<(String, Span)>,
    /// A braced initialiser, read against the type (FTEF-19.47).
    pub aggregate: Option<RawAggregate>,
    /// The parameters of the function type the declaration gives, as FTEQCC's redeclaration
    /// rules compare them (FTEF-19.6, FTEF-19.7); empty for any other type.
    pub params: Vec<RawParamInfo>,
    /// For an enum's name, how its value is found (FTEF-19.10).
    pub enum_item: Option<EnumItem>,
    /// For a `typedef`'s name, the type it names (FTEF-19.9).
    pub typedef: Option<Type>,
    /// For a `typedef` of a function type, its named parameters, which a definition of the
    /// type takes as its own (FTEF-7.5).
    pub typedef_params: Vec<RawParam>,
    /// The compiler flags and pragma defaults where the declaration starts, which
    /// `QCC_PR_ParseDefs` reads first (FTEF-21.1, FTEF-21.14).
    pub flags: CompilerFlags,
    /// Whether `assumevar` was on at its initialiser, which makes an initialised global no
    /// constant (FTEF-21.11).
    pub assumevar: bool,
    /// Whether FTEQCC's autoprototyping skipped the initialiser it wrote (FTEF-12.18): the
    /// definition is made constant by it, as one written with it (`isinitialised`), and is not
    /// initialised.
    pub skipped_init: bool,
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
    /// `auto`, which a later `register` undoes (FTEF-19.61).
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
            "auto" => Self::Auto,
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
    /// The compiler flags at the token after it, where FTEQCC casts or converts it once it has
    /// read it (FTEF-22.1).
    pub cast_flags: CompilerFlags,
    /// The expression as written, when it is [`ValueKind::Other`], which the check stage reads
    /// at global scope (FTEF-34.9).
    pub syntax: Option<ast::Expr>,
    /// The types written in its casts and `sizeof`, by where each starts (FTEF-34.9).
    pub written_types: Vec<(usize, Type)>,
}

/// The shape of a value initialiser (QC-11.3, FTEF-19.31).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// A literal, with the type the dialect gives it.
    Literal(Literal),
    /// A name by itself.
    Name(String),
    /// FTEQCC's `_(...)` translation intrinsic, and whether its argument is one string
    /// literal (FTEF-19.31).
    Translate(bool),
    /// An element of a named array, `name[index]` (FTEF-19.39).
    Indexed(String),
    /// A literal cast to a type, `(T)0`, a value of that type (FTEF-19.43).
    Cast(Type),
    /// Any other expression, whose type the model does not compute (FTEF-19.46).
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
    /// A size given by an expression the model does not evaluate (FTEF-19.46).
    Unknown,
}

/// A parameter of a declaration's function type, as FTEQCC compares two redeclarations
/// (FTEF-19.6, FTEF-19.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawParamInfo {
    /// Its name, which a call with too few arguments names (FTEF-32.3).
    pub name: Option<String>,
    /// Its type.
    pub ty: Type,
    /// Whether `optional` marks it.
    pub optional: bool,
    /// Whether `__out`, `__inout` or `&` marks it.
    pub out: bool,
    /// Its default value (FTE-18.2), if it has one.
    pub default: Option<RawValue>,
    /// The default value when it is a literal, compared as the "dupe initialisation"
    /// comparison compares values (FTE-22.6).
    pub default_literal: Option<LiteralValue>,
}

impl RawDecl {
    /// A declaration of `name` at `span` of type `ty`, with no initialiser.
    pub(super) fn new(name: String, span: Span, ty: Type) -> Self {
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
            type_only: false,
            accessor: None,
            qualifiers: Vec::new(),
            value: None,
            array: None,
            array_dims: 0,
            aggregate_first: None,
            aggregate: None,
            params: Vec::new(),
            enum_item: None,
            typedef: None,
            typedef_params: Vec::new(),
            flags: CompilerFlags::default(),
            assumevar: false,
            skipped_init: false,
            pointer_var: false,
            extern_base: None,
        }
    }

    /// The declaration as FTEQCC reads it while `autoprototype` is on (FTEF-12.18): the name and
    /// its type are made, and the initialiser is skipped over, a function's body or a value, a
    /// builtin number or a `_(\"...\")`, as `QCC_PR_ParseDefs` does with `if (autoprototype ||
    /// ...) { /* ignore the code and stuff */ }`.
    ///
    /// A parameter's default that names a definition is dropped (FTEF-12.21): what it names is
    /// not initialised in this pass, and `QCC_PR_ParseDefaultInitialiser` returns no default for
    /// it `if (autoprototype)`.
    #[must_use]
    pub(crate) fn prototyped(&self) -> Self {
        let params = self
            .params
            .iter()
            .map(|param| match param.default.as_ref().map(|d| &d.kind) {
                Some(ValueKind::Name(_) | ValueKind::Indexed(_) | ValueKind::Translate(_)) => {
                    RawParamInfo {
                        default: None,
                        default_literal: None,
                        ..param.clone()
                    }
                }
                Some(ValueKind::Literal(_) | ValueKind::Cast(_) | ValueKind::Other) | None => {
                    param.clone()
                }
            })
            .collect();
        Self {
            params,
            skipped_init: self.initialized,
            initialized: false,
            literal: None,
            body: None,
            builtin: false,
            next_think: None,
            uses: Vec::new(),
            value: None,
            ..self.clone()
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
    pub(super) fn type_name(token: &qc_syntax::SyntaxToken, kind: TypeKind) -> Self {
        let mut decl = Self::new(token.text_lossless(), token.span(), Type::Unknown);
        decl.type_kind = Some(kind);
        decl
    }
}

/// The value initialiser `expr` is, in the dialect `fte` says (QC-11.3, FTEF-19.31): a literal
/// with its type, which FTEQCC gives a number by its digits and suffix (FTEF-3.1) and vanilla
/// always `float` (QC-4); a plain name; or another expression.
pub(super) fn raw_value(expr: &ast::Expr, cx: Cx<'_>) -> RawValue {
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
        ast::Expr::Index(index) if fte => match index.base() {
            Some(ast::Expr::Name(base)) if base.scope().is_none() => base
                .name()
                .map_or(ValueKind::Other, |t| ValueKind::Indexed(t.text_lossless())),
            _ => ValueKind::Other,
        },
        // FTEF-19.43: `(T)0` is a value of `T`, which the definition's type must take.
        ast::Expr::Cast(cast) if fte && matches!(cast.operand(), Some(ast::Expr::Literal(_))) => {
            cast.ty()
                .map_or(ValueKind::Other, |t| ValueKind::Cast(cx.ty(&t)))
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
    let syntax = matches!(kind, ValueKind::Other).then(|| expr.clone());
    let written_types = match &syntax {
        Some(expr) => super::body::written_types(expr.syntax(), cx),
        None => Vec::new(),
    };
    RawValue {
        span,
        kind,
        first,
        string_len,
        cast_flags: cx.next_flags(span.end),
        syntax,
        written_types,
    }
}

/// Whether `call` calls FTEQCC's `_` translation intrinsic (FTEF-19.31).
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

/// A braced initialiser and the sizes of its declarator's array dimensions, each a number or
/// `None`, which `QCC_PR_ParseInitializerType_Internal` reads against the type (FTEF-19.47).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAggregate {
    /// The initialiser.
    pub init: RawBraces,
    /// Each dimension's size when it is a number; `None` for `[]` or another expression.
    pub sizes: Vec<Option<u32>>,
}

/// `{ ... }` in an initialiser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawBraces {
    /// Whether it names its members, `{ .a = 1 }`.
    pub designated: bool,
    /// The span of its `{`, where an error about it as an element stands.
    pub open: Span,
    /// Its elements, in order.
    pub elements: Vec<RawElement>,
}

/// One element of a braced initialiser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawElement {
    /// A braced one.
    Braces(RawBraces),
    /// A value, read as a definition's value is.
    Value(RawValue),
}

impl RawElement {
    /// The text and span of its first token.
    #[must_use]
    pub fn first(&self) -> (String, Span) {
        match self {
            Self::Braces(braces) => ("{".to_owned(), braces.open),
            Self::Value(value) => (value.first.clone(), value.span),
        }
    }
}

/// The braces `agg`, each element lowered.
fn raw_braces(agg: &ast::AggregateInit, cx: Cx<'_>) -> RawBraces {
    let mut tokens = agg
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| match e {
            qc_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => Some(t),
            qc_syntax::SyntaxElement::Token(_) | qc_syntax::SyntaxElement::Node(_) => None,
        });
    let open = tokens.next().map_or(agg.syntax().span(), |t| t.span());
    let designated = tokens.next().is_some_and(|t| t.text_lossless() == ".");
    let elements = agg
        .syntax()
        .children()
        .filter_map(|node| match ast::AggregateInit::cast(node.clone()) {
            Some(inner) => Some(RawElement::Braces(raw_braces(&inner, cx))),
            None => ast::Expr::cast(node).map(|expr| RawElement::Value(raw_value(&expr, cx))),
        })
        .collect();
    RawBraces {
        designated,
        open,
        elements,
    }
}

/// The braced initialiser of `declarator`, if it has one.
pub(super) fn raw_aggregate(
    declarator: &ast::Declarator,
    init: Option<&ast::Initializer>,
    cx: Cx<'_>,
) -> Option<RawAggregate> {
    let Some(ast::Initializer::Aggregate(syntax)) = init else {
        return None;
    };
    let sizes = declarator
        .array_suffixes()
        .map(|suffix| match suffix.children().find_map(ast::Expr::cast) {
            Some(ast::Expr::Literal(literal)) => literal
                .token()
                .and_then(|t| t.text_lossless().parse::<u32>().ok()),
            _ => None,
        })
        .collect();
    Some(RawAggregate {
        init: raw_braces(syntax, cx),
        sizes,
    })
}

pub(super) fn array_dim(
    declarator: &ast::Declarator,
    init: Option<&ast::Initializer>,
) -> Option<ArrayDim> {
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
pub(super) fn qualifiers_of(node: &SyntaxNode) -> Vec<Qualifier> {
    let mut out = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Qualifier => {
                let word = first_word(&child);
                // FTEF-19.61: `register` sets `isauto=false`.
                if word
                    .strip_prefix("__")
                    .unwrap_or(&word)
                    .eq_ignore_ascii_case("register")
                {
                    out.retain(|q| *q != Qualifier::Auto);
                    continue;
                }
                out.extend(Qualifier::of(&word, &token_text(&child)));
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
            name: param.name().map(|t| t.text_lossless()),
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

/// The value of a literal initialiser (FTE-22.6). A float is kept as its bits, so two
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
    /// reads, `None` when it reads none and names the builtin after the definition (FTE-23.17).
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

/// The number and engine name of a builtin function FTEQCC makes (FTE-23.17): it reuses a
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
    /// (FTE-23.17).
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
    pub(super) fn of_fteqcc(init: Option<&ast::Initializer>, cx: Cx<'_>) -> Option<Self> {
        let init = init?;
        match init {
            // A body is a definition, never a value two may share.
            ast::Initializer::Body(_) => None,
            ast::Initializer::Builtin(builtin) => {
                // FTE-23.17: a builtin is the `binum` FTEQCC binds (FTE-17.6), 0 when it reads
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
    pub(super) fn of_literal(
        expr: &ast::Expr,
        dialect: Dialect,
        flags: qc_lexer::LexFlags,
    ) -> Option<Self> {
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

/// The qualifier words of a declaration (FTEF-15.1, FTEF-9.2), as written.
pub(super) fn qualifiers(decl: &ast::Declaration) -> Vec<String> {
    decl.syntax()
        .children()
        .filter(|n| n.kind() == SyntaxKind::Qualifier)
        .map(|n| first_word(&n))
        .collect()
}

/// `base` under the `*`s and parameter lists a struct, union, class, enum or accessor declaration
/// took as its type's with `qcfuncs` on (FTEF-6.8, FTEF-21.7): `struct S { float a; } *p, q;`
/// declares two pointers, `enum { A }(float) h` a function. A type with no declarator after it,
/// `struct S { float a; } (x);`, is recorded as one, whose names are checked (FTEF-19.35).
pub(super) fn decl_base(decl: &SyntaxNode, base: Type, cx: Cx<'_>, out: &mut Vec<RawDecl>) -> Type {
    let mut base = decl
        .child_tokens()
        .filter(|t| t.kind() == SyntaxKind::Star)
        .fold(base, |inner, _| Type::Pointer(std::rc::Rc::new(inner)));
    let mut function = false;
    for list in decl
        .children()
        .filter(|n| n.kind() == SyntaxKind::FunctionType)
        .filter_map(|n| n.children().find_map(ast::ParamList::cast))
    {
        base = cx.function(base, &list);
        function = true;
    }
    if function && decl.children().all(|n| n.kind() != SyntaxKind::Declarator) {
        let mut raw = RawDecl::new(String::new(), decl.span(), base.clone());
        raw.type_only = true;
        out.push(raw);
    }
    base
}

pub(super) fn lower_declaration(decl: &ast::Declaration, cx: Cx<'_>, out: &mut Vec<RawDecl>) {
    let fte = cx.fte;
    // FTEF-8.2: an enum written as a type declares its names first.
    for en in decl.syntax().descendants().filter_map(ast::EnumDecl::cast) {
        lower_enum(&en, cx, out);
    }
    let base = decl.ty().map_or_else(omitted_type, |t| cx.ty(&t));
    let first = out.len();
    lower_declarators(decl.declarators(), &base, decl.ty(), cx, out);
    if decl.declarators().next().is_none()
        && fte
        && let Some(node) = decl.ty()
    {
        let span = node.syntax().span();
        let mut raw = RawDecl::new(String::new(), span, base.clone());
        raw.type_only = true;
        out.push(raw);
    }
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
pub(super) fn no_function(node: &ast::Type, ty: &Type) -> bool {
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
pub(super) fn declarator_type(base: &Type, declarator: &ast::Declarator, cx: Cx<'_>) -> Type {
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
pub(super) fn lower_declarators(
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
        // `isvar |= isptr>0`: a parenthesised name with no `*` is no pointer (FTEF-15.9).
        pointer_var |= declarator.pointer_form().is_some_and(|stars| stars > 0);
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
        raw.array_dims = declarator.array_suffixes().count();
        raw.aggregate = raw_aggregate(&declarator, init.as_ref(), cx);
        if let Some(ast::Initializer::Aggregate(agg)) = &init {
            raw.aggregate_first = agg
                .syntax()
                .descendants_with_tokens()
                .filter_map(|e| match e {
                    qc_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => Some(t),
                    qc_syntax::SyntaxElement::Token(_) | qc_syntax::SyntaxElement::Node(_) => None,
                })
                .nth(1)
                .map(|t| (t.text_lossless(), t.span()));
        }
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
pub(super) fn function_params(ty: &ast::Type) -> Option<ast::ParamList> {
    match ty {
        ast::Type::Function(func) => func.params(),
        ast::Type::Simple(_) | ast::Type::Field(_) | ast::Type::Pointer(_) => None,
    }
}
