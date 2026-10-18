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
    /// The return type, always a simple type in the surface language (QC-10.3).
    pub ret: SimpleType,
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
            Self::Unknown | Self::Simple(_) | Self::Field(_) | Self::Function(_) => 1,
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

    /// Lowers a syntactic [`ast::Type`] to a semantic [`Type`].
    ///
    /// A missing or malformed part becomes [`Type::Unknown`], since the parser never fails
    /// and may hand back a partial type (QCD-1.4). `..float` nests field types (QC-10.2).
    #[must_use]
    pub fn from_ast(ty: &ast::Type) -> Self {
        match ty {
            ast::Type::Simple(simple) => simple
                .kind()
                .map_or(Self::Unknown, |k| Self::Simple(SimpleType::from_ast(k))),
            ast::Type::Field(field) => {
                let inner = field.inner().map_or(Self::Unknown, |t| Self::from_ast(&t));
                Self::Field(Rc::new(inner))
            }
            ast::Type::Function(func) => {
                let ret = func
                    .return_type()
                    .and_then(|r| r.kind())
                    .map_or(SimpleType::Void, SimpleType::from_ast);
                let (params, variadic) = func.params().map_or((Vec::new(), false), |list| {
                    let variadic = list.is_variadic();
                    let params = list
                        .params()
                        .map(|p| p.ty().map_or(Self::Unknown, |t| Self::from_ast(&t)))
                        .collect();
                    (params, variadic)
                });
                Self::Function(Rc::new(FunctionType {
                    ret,
                    params,
                    variadic,
                }))
            }
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
