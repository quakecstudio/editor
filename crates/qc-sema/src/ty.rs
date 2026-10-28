// SPDX-License-Identifier: GPL-3.0-or-later

//! Semantic types.
//!
//! Implements QC-10 of `docs/spec/language.md`: the five simple types, field types, and
//! function types, and the structural equality `qcc`'s `PR_FindType` gives them (QC-10.10).
//! A [`Type`] is a small, cloneable, hashable value, so the resolver compares two types by
//! value. Field and function types nest an inner [`Type`] in an [`std::rc::Rc`] so that a
//! deeply nested type stays cheap to clone.

use std::rc::Rc;

use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::{CContext, CScalar, CWords};

#[cfg(test)]
thread_local! {
    /// How many times [`Type::from_ast`] has run on this thread, for the tests that show
    /// lowering is linear in a type's size.
    pub(crate) static FROM_AST_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A QuakeC type (QC-10).
///
/// Two values are equal exactly when `qcc` would intern them as one type (QC-10.10):
/// function and field types compare structurally, parameter names excluded (QC-10.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// A type the parser left out or that did not resolve.
    Unknown,
    /// One of the five simple types (QC-10.1).
    Simple(SimpleType),
    /// `.T`, a field holding a `T` (QC-10.2).
    Field(Rc<Type>),
    /// `T(params)`, a function (QC-10.3).
    Function(Rc<FunctionType>),
    /// An FTEQCC type named by a word that is not one of the five: a class, struct, union,
    /// typedef, enum or accessor a program defines, as written, or the C scalar C type words
    /// such as `int` make, by the name FTEQCC registers it with (`__int32`, FTEF-2, FTEF-7.2,
    /// FTEF-20.9).
    Named(Rc<str>),
    /// An FTEQCC pointer `T *` (FTEF-4.1).
    Pointer(Rc<Type>),
}

/// The five simple types (QC-10.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimpleType {
    /// `void`, one word (QC-10.7).
    Void,
    /// `float`, one word.
    Float,
    /// `vector`, three floats (QC-10.1).
    Vector,
    /// `string`, one word.
    String,
    /// `entity`, one word.
    Entity,
}

/// What FTEQCC's `QCC_PR_ParseType` and `QCC_PR_ParseFunctionType` read at a token besides
/// the type's own words: the keywords and `ILP32` the C type words are read with (FTEF-20.9),
/// and whether `qcfuncs` is on (FTEF-21.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeContext {
    /// What the loop over C type words reads.
    pub words: CContext,
    /// Whether `qcfuncs` is on: off, `()` is variadic, a first parameter with no type an
    /// `__int32`, and an array parameter a pointer.
    pub qcfuncs: bool,
}

impl Default for TypeContext {
    /// FTEQCC's defaults: the default keywords, `ILP32` off and `qcfuncs` on.
    fn default() -> Self {
        Self {
            words: CContext::default(),
            qcfuncs: true,
        }
    }
}

/// A function type: a return type, parameter types and whether it is variadic (QC-10.3).
///
/// Parameter names are not part of the type (QC-10.3): two function types with different
/// parameter names are equal (QC-10.10). FTEQCC's messages print them, so they are kept beside
/// the types as [`ParamLabels`], which no comparison reads.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FunctionType {
    /// The return type: a simple type in vanilla (QC-10.3), any type in FTEQCC (FTEF-15.2).
    pub ret: Type,
    /// The parameter types, in order.
    pub params: Vec<Type>,
    /// Whether the parameter list is `(...)` (QC-10.4).
    pub variadic: bool,
    /// How each parameter is written, for messages only.
    pub labels: ParamLabels,
}

/// How a parameter is written: its name, its `optional` and out marks and its default value as
/// FTEQCC's `TypeName` prints them (FTEF-24.7).
#[derive(Clone, Debug, Default)]
pub struct ParamLabel {
    /// Its name.
    pub name: Option<String>,
    /// Whether `optional` marks it.
    pub optional: bool,
    /// Whether an out mark (`__out`, `__inout`, `&`) marks it.
    pub out: bool,
    /// Its default value as `QCC_VarAtOffset` prints it.
    pub default: Option<String>,
}

/// The parameters' labels of a function type; equal to any other, so they never tell two
/// types apart.
#[derive(Clone, Debug, Default)]
pub struct ParamLabels(pub Rc<[ParamLabel]>);

impl PartialEq for ParamLabels {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for ParamLabels {}

impl std::hash::Hash for ParamLabels {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

impl Type {
    /// Whether this type is, or holds, a variadic function type, which id's `PR_FindType` never
    /// finds again, so that no other type is ever the same as it (QC-10.10).
    #[must_use]
    pub fn holds_variadic(&self) -> bool {
        match self {
            Type::Function(func) => {
                func.variadic
                    || func.ret.holds_variadic()
                    || func.params.iter().any(Type::holds_variadic)
            }
            Type::Field(inner) | Type::Pointer(inner) => inner.holds_variadic(),
            Type::Simple(_) | Type::Named(_) | Type::Unknown => false,
        }
    }

    /// The number of words a value of this type occupies (`QC_TYPE_SIZE`, QC-10.1).
    ///
    /// A vector is three words; every other type, [`Type::Unknown`] included, is one.
    #[must_use]
    pub fn word_size(&self) -> u32 {
        match self {
            Self::Simple(SimpleType::Vector) => 3,
            Self::Unknown
            | Self::Simple(_)
            | Self::Field(_)
            | Self::Function(_)
            | Self::Named(_)
            | Self::Pointer(_) => 1,
        }
    }

    /// Whether this is a function type (QC-11.8: a global of function type is a function).
    #[must_use]
    pub fn is_function(&self) -> bool {
        matches!(self, Self::Function(_))
    }

    /// Whether this is a field type (QC-11.9: fields can only be global).
    #[must_use]
    pub fn is_field(&self) -> bool {
        matches!(self, Self::Field(_))
    }

    /// The class or other type a value of this type names, through one pointer: `Named` or
    /// `Pointer(Named)` (FTEF-17.3).
    #[must_use]
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Pointer(inner) => inner.type_name(),
            Self::Unknown | Self::Simple(_) | Self::Field(_) | Self::Function(_) => None,
        }
    }

    /// Lowers a syntactic [`ast::Type`] to a semantic [`Type`].
    ///
    /// A missing or malformed part becomes [`Type::Unknown`], since the parser never fails
    /// and may hand back a partial type (QCD-1.4). `..float` nests field types (QC-10.2). An
    /// FTEQCC type word that is not one of the five is [`Type::Named`] (FTEF-2, FTEF-7.2): C
    /// type words read as the parser read them, with the keywords and `ILP32` `context` gives
    /// at the type's offset (FTEF-20.9), name the scalar they make; a parameter list is read
    /// with the `qcfuncs` it gives (FTEF-21.8).
    #[must_use]
    pub fn from_ast(ty: &ast::Type, context: &dyn Fn(usize) -> TypeContext) -> Self {
        #[cfg(test)]
        FROM_AST_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
        match ty {
            ast::Type::Simple(simple) => match simple.kind() {
                Some(k) => Self::Simple(SimpleType::from_ast(k)),
                None => {
                    let text = simple.text();
                    if text.is_empty() {
                        Self::Unknown
                    } else {
                        Self::named(&text, context(simple.syntax().span().start).words)
                    }
                }
            },
            ast::Type::Field(field) => {
                let inner = field
                    .inner()
                    .map_or(Self::Unknown, |t| Self::from_ast(&t, context));
                Self::Field(Rc::new(inner))
            }
            ast::Type::Pointer(pointer) => {
                let inner = pointer
                    .inner()
                    .map_or(Self::Unknown, |t| Self::from_ast(&t, context));
                Self::Pointer(Rc::new(inner))
            }
            ast::Type::Function(func) => {
                let ret = func
                    .return_type()
                    .map_or(Self::Simple(SimpleType::Void), |r| {
                        Self::from_ast(&r, context)
                    });
                func.params().map_or_else(
                    || {
                        Self::Function(Rc::new(FunctionType {
                            ret: ret.clone(),
                            params: Vec::new(),
                            variadic: false,
                            labels: ParamLabels::default(),
                        }))
                    },
                    |list| Self::function_of(ret.clone(), &list, context),
                )
            }
        }
    }

    /// The function type returning `ret` with the parameters of `list`, read with `context`:
    /// variadic when the list is, or, with `qcfuncs` off after its `)`, when it is `()`
    /// (`if (!flag_qcfuncs) ftype->vargs = true; //'void name()' is vargs/undefined in C89`,
    /// FTEF-21.8).
    #[must_use]
    pub fn function_of(
        ret: Self,
        list: &ast::ParamList,
        context: &dyn Fn(usize) -> TypeContext,
    ) -> Self {
        let empty = list.params().next().is_none() && !list.is_variadic();
        let variadic = list.is_variadic() || (empty && !context(list.syntax().span().end).qcfuncs);
        let params = Self::params_of(list, context);
        let labels = Self::labels_of(list, &params);
        Self::Function(Rc::new(FunctionType {
            ret,
            params,
            variadic,
            labels,
        }))
    }

    /// The type a name of type words `words` makes, read with `context` (FTEF-20.9): the scalar
    /// they make when the loop over C type words takes them all, `type_float` the simple type
    /// and any other by the name FTEQCC registers it with, else the type named as written.
    #[must_use]
    pub fn named(words: &str, context: CContext) -> Self {
        let split = || words.split(' ');
        match CWords::read(split(), context) {
            (taken, Some(c_type)) if taken == split().count() => match c_type.scalar {
                CScalar::Float => Self::Simple(SimpleType::Float),
                scalar => Self::Named(Rc::from(scalar.name())),
            },
            _ => Self::Named(Rc::from(words)),
        }
    }

    /// How each parameter of `list`, of types `params`, is written (FTEF-24.7).
    fn labels_of(list: &ast::ParamList, params: &[Self]) -> ParamLabels {
        let labels: Vec<ParamLabel> = list
            .params()
            .zip(params)
            .map(|(param, ty)| {
                let words: Vec<String> = param
                    .syntax()
                    .children()
                    .filter(|n| n.kind() == qc_syntax::SyntaxKind::Qualifier)
                    .filter_map(|n| {
                        n.child_tokens().find(|t| !t.kind().is_trivia()).map(|t| {
                            t.text_lossless()
                                .trim_start_matches('_')
                                .to_ascii_lowercase()
                        })
                    })
                    .collect();
                ParamLabel {
                    name: param.name().map(|t| t.text_lossless()),
                    optional: words.iter().any(|w| w == "optional"),
                    out: words.iter().any(|w| w == "out" || w == "inout")
                        || param
                            .syntax()
                            .child_tokens()
                            .any(|t| t.kind() == qc_syntax::SyntaxKind::Amp),
                    default: param.default_value().map(|value| default_text(&value, ty)),
                }
            })
            .collect();
        ParamLabels(labels.into())
    }

    /// The parameter types of a parameter list (QC-10.3). FTEQCC's `(void)`, one `void` with no
    /// name, has none (FTEF-15.3).
    ///
    /// Each parameter's type is lowered once, so lowering a type is linear in its size: a
    /// parameter that is itself a function type would otherwise be lowered again at every
    /// level of nesting.
    #[must_use]
    pub fn params_of(list: &ast::ParamList, context: &dyn Fn(usize) -> TypeContext) -> Vec<Self> {
        let params: Vec<ast::Param> = list.params().collect();
        let types: Vec<Self> = params.iter().map(|p| Self::param_of(p, context)).collect();
        if let ([only], [ty]) = (params.as_slice(), types.as_slice())
            && only.name().is_none()
            && *ty == Self::Simple(SimpleType::Void)
        {
            return Vec::new();
        }
        types
    }
}

impl Type {
    /// A parameter's type, read with `context` (FTEF-15.3, FTEF-21.8): its type, or with
    /// `qcfuncs` off, for one that has none, `__int32` (`if (!flag_qcfuncs) t = type_integer;
    /// //c89 assumes int.`); a pointer for an unsized array (`//length omitted. just treat it as
    /// a pointer...?`), and for a sized one with `qcfuncs` off (`just turn it into a pointer and
    /// ditch the top-level size. C style.`).
    fn param_of(param: &ast::Param, context: &dyn Fn(usize) -> TypeContext) -> Self {
        let at = param.syntax().span().start;
        let qcfuncs = context(at).qcfuncs;
        let ty = match param.ty() {
            Some(t) => Self::from_ast(&t, context),
            None if !qcfuncs => Self::Named(Rc::from(CScalar::Integer.name())),
            None => Self::Unknown,
        };
        match param.array_suffixes().first() {
            Some(suffix) if suffix.is_empty() || !qcfuncs => Self::Pointer(Rc::new(ty)),
            Some(_) | None => ty,
        }
    }
}

impl SimpleType {
    /// The semantic simple type of an AST simple-type kind.
    #[must_use]
    pub fn from_ast(kind: ast::SimpleTypeKind) -> Self {
        match kind {
            ast::SimpleTypeKind::Void => Self::Void,
            ast::SimpleTypeKind::Float => Self::Float,
            ast::SimpleTypeKind::Vector => Self::Vector,
            ast::SimpleTypeKind::String => Self::String,
            ast::SimpleTypeKind::Entity => Self::Entity,
        }
    }

    /// The keyword that spells this type.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::String => "string",
            Self::Entity => "entity",
        }
    }
}

/// A default value as `QCC_VarAtOffset` prints the constant it is once cast to the parameter's
/// type `ty`: a float `%gf`, an integer `%#xi`, a vector `'%g %g %g'`, a string quoted; any
/// other expression as written.
fn default_text(value: &ast::Expr, ty: &Type) -> String {
    let written = String::from_utf8_lossy(&value.syntax().text())
        .trim()
        .to_owned();
    let number = |text: &str| -> Option<f64> {
        let text = text.trim();
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest.trim()),
            None => (false, text),
        };
        let parsed: f64 = digits.trim_end_matches(['f', 'i']).parse().ok()?;
        Some(if negative { -parsed } else { parsed })
    };
    match ty {
        Type::Simple(SimpleType::Float) => {
            number(&written).map_or(written.clone(), |v| format!("{}f", format_g(v)))
        }
        Type::Simple(SimpleType::Vector) => {
            let inner = written.trim_matches('\'');
            let parts: Option<Vec<f64>> = inner.split_whitespace().map(number).collect();
            match parts.as_deref() {
                Some([x, y, z]) => {
                    format!("'{} {} {}'", format_g(*x), format_g(*y), format_g(*z))
                }
                _ => written,
            }
        }
        Type::Named(name) if name.as_ref() == "__int32" => {
            let negative = written.trim_start().starts_with('-');
            let digits = written
                .trim()
                .trim_start_matches('-')
                .trim()
                .trim_end_matches('i');
            // `%#x` of the 32-bit integer the constant is, which C writes `0` for zero.
            digits
                .parse::<i64>()
                .ok()
                .map(|v| if negative { v.wrapping_neg() } else { v })
                .and_then(|v| u32::try_from(v.rem_euclid(1_i64 << 32)).ok())
                .map_or(written.clone(), |v| match v {
                    0 => "0i".to_owned(),
                    v => format!("{v:#x}i"),
                })
        }
        _ => written,
    }
}

/// C's `%g`: six significant digits, trailing zeros dropped, an exponent outside 1e-4 to 1e6.
fn format_g(value: f64) -> String {
    let scientific = format!("{value:.5e}");
    let (mantissa, exp) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |text: &str| {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim(mantissa), exp.unsigned_abs());
    }
    let decimals = usize::try_from(5_i32.saturating_sub(exp)).unwrap_or(0);
    trim(&format!("{value:.decimals$}"))
}
