// SPDX-License-Identifier: GPL-3.0-or-later

//! Semantic types.
//!
//! Implements QC-10 of `docs/spec/language.md`: the five simple types, field types, and
//! function types, and the structural equality `qcc`'s `PR_FindType` gives them (QC-10.6).
//! A [`Type`] is a small, cloneable, hashable value, so the resolver compares two types by
//! value. Field and function types nest an inner [`Type`] in an [`std::rc::Rc`] so that a
//! deeply nested type stays cheap to clone.

use std::rc::Rc;

use qc_syntax::ast;

#[cfg(test)]
thread_local! {
    /// How many times [`Type::from_ast`] has run on this thread, for the tests that show
    /// lowering is linear in a type's size.
    pub(crate) static FROM_AST_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A QuakeC type (QC-10).
///
/// Two values are equal exactly when `qcc` would intern them as one type (QC-10.6):
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
    /// typedef, enum or accessor a program defines, or a C type word such as `int`
    /// (FTEF-2, FTEF-7.2), as written.
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

/// A function type: a return type, parameter types and whether it is variadic (QC-10.3).
///
/// Parameter names are not part of the type (QC-10.3); only the types and the count are
/// stored, so two function types with different parameter names are equal (QC-10.6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FunctionType {
    /// The return type: a simple type in vanilla (QC-10.3), any type in FTEQCC (FTEF-15.2).
    pub ret: Type,
    /// The parameter types, in order.
    pub params: Vec<Type>,
    /// Whether the parameter list is `(...)` (QC-10.4).
    pub variadic: bool,
}

impl Type {
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

    /// Whether this is an FTEQCC type whose meaning the semantic model does not follow: a
    /// named type, a pointer, or one holding either or [`Type::Unknown`]. Two types are then
    /// [`Type::compatible`] whatever they are (FTEF-17.12).
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        match self {
            Self::Unknown | Self::Named(_) | Self::Pointer(_) => true,
            Self::Simple(_) => false,
            Self::Field(inner) => inner.is_opaque(),
            Self::Function(func) => func.ret.is_opaque() || func.params.iter().any(Self::is_opaque),
        }
    }

    /// Whether a redeclaration with type `other` declares the same thing (QC-11.5): equal
    /// types, or, under FTEQCC, types either of which [`Type::is_opaque`] (FTEF-17.12).
    #[must_use]
    pub fn compatible(&self, other: &Self) -> bool {
        self == other || self.is_opaque() || other.is_opaque()
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
    /// FTEQCC type word that is not one of the five is [`Type::Named`] (FTEF-2, FTEF-7.2).
    #[must_use]
    pub fn from_ast(ty: &ast::Type) -> Self {
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
                        Self::Named(Rc::from(text.as_str()))
                    }
                }
            },
            ast::Type::Field(field) => {
                let inner = field.inner().map_or(Self::Unknown, |t| Self::from_ast(&t));
                Self::Field(Rc::new(inner))
            }
            ast::Type::Pointer(pointer) => {
                let inner = pointer
                    .inner()
                    .map_or(Self::Unknown, |t| Self::from_ast(&t));
                Self::Pointer(Rc::new(inner))
            }
            ast::Type::Function(func) => {
                let ret = func
                    .return_type()
                    .map_or(Self::Simple(SimpleType::Void), |r| Self::from_ast(&r));
                let (params, variadic) = func.params().map_or((Vec::new(), false), |list| {
                    (Self::params_of(&list), list.is_variadic())
                });
                Self::Function(Rc::new(FunctionType {
                    ret,
                    params,
                    variadic,
                }))
            }
        }
    }

    /// The parameter types of a parameter list (QC-10.3). FTEQCC's `(void)`, one `void` with no
    /// name, has none (FTEF-15.3).
    ///
    /// Each parameter's type is lowered once, so lowering a type is linear in its size: a
    /// parameter that is itself a function type would otherwise be lowered again at every
    /// level of nesting.
    #[must_use]
    pub fn params_of(list: &ast::ParamList) -> Vec<Self> {
        let params: Vec<ast::Param> = list.params().collect();
        let types: Vec<Self> = params
            .iter()
            .map(|p| p.ty().map_or(Self::Unknown, |t| Self::from_ast(&t)))
            .collect();
        if let ([only], [ty]) = (params.as_slice(), types.as_slice())
            && only.name().is_none()
            && *ty == Self::Simple(SimpleType::Void)
        {
            return Vec::new();
        }
        types
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
