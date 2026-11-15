// SPDX-License-Identifier: GPL-3.0-or-later

//! The map search's query language (P7-08, MAP-28): TrenchBroom's, as its manual's "Searching"
//! and "Query Language" describe it. The search box takes plain text or a query; a query is an
//! expression evaluated for every object of the types it applies to, and finds those it is
//! `true` for. Text, and input that is not a query, finds every object whose name, classname,
//! keys or values, textures, smart tags, or layer or group name holds it, ignoring case.

mod domain;
mod eval;
mod objects;
mod syntax;
mod value;

pub(crate) use objects::PointSizes;

use domain::{Field, ObjectType, TYPE_NAMES};
use syntax::{Binary, Builtin, Expr, Unary};
use value::Value;

/// A search, parsed: the query it is, or the text search it stands for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Query(Expr);

impl Query {
    /// The search `input` asks for, as TrenchBroom decides: input that is not an expression,
    /// that names something other than a field or a built-in function, or that is a single
    /// name or literal (`visible` and `locked` aside), is text to search for; empty input
    /// finds nothing (the manual's "Text Search and Queries").
    pub(crate) fn parse(input: &str) -> Self {
        let input = input.trim();
        let Some(expr) = Expr::parse(input) else {
            return Self::text(input);
        };
        let expr = expr.with_type_names_quoted();
        match expr.search_text(input) {
            Some(text) => Self::text(&text),
            None => Self(expr),
        }
    }

    /// The query a text search is: every searched field `like` the text.
    fn text(text: &str) -> Self {
        if text.is_empty() {
            return Self(Expr::Literal(Value::Bool(false)));
        }
        let like = |field: Field| {
            Expr::Call(
                Builtin::Like,
                vec![
                    Expr::Name(field.name().to_owned()),
                    Expr::Literal(Value::Text(text.to_owned())),
                ],
            )
        };
        let query = Field::ALL
            .into_iter()
            .filter(|field| field.searched_as_text())
            .map(like)
            .reduce(|a, b| Expr::Binary(Binary::Or, Box::new(a), Box::new(b)))
            .unwrap_or(Expr::Literal(Value::Bool(false)));
        Self(query)
    }

    /// The object types the query searches, each with the query specialized for it. Faces
    /// and layers are searched only on their own: a query finds faces or objects, never both,
    /// and finding a layer selects everything in it (the manual's "Which Objects a Query
    /// Searches").
    pub(crate) fn domain(&self) -> Vec<(ObjectType, Expr)> {
        let mut domain: Vec<(ObjectType, Expr)> = ObjectType::ALL
            .into_iter()
            .map(|ty| (ty, self.0.specialized(ty)))
            .filter(|(_, specialized)| specialized.can_match())
            .collect();
        let alone = |only: ObjectType| matches!(domain.as_slice(), [(ty, _)] if *ty == only);
        if !alone(ObjectType::Face) && !alone(ObjectType::Layer) {
            domain.retain(|(ty, _)| !matches!(ty, ObjectType::Face | ObjectType::Layer));
        }
        domain
    }
}

impl Expr {
    /// This expression with the type names written without quotes where `type` is compared
    /// with them, as in `type is brush` and `type in [brush, patch]`, made strings.
    fn with_type_names_quoted(self) -> Self {
        let is_type = |expr: &Self| matches!(expr, Self::Name(name) if name == Field::Type.name());
        let quote = |expr: Self| match expr {
            Self::Name(name) if TYPE_NAMES.contains(&name.as_str()) => {
                Self::Literal(Value::Text(name))
            }
            Self::Array(items) => Self::Array(
                items
                    .into_iter()
                    .map(|item| match item {
                        Self::Name(name) if TYPE_NAMES.contains(&name.as_str()) => {
                            Self::Literal(Value::Text(name))
                        }
                        other => other,
                    })
                    .collect(),
            ),
            other => other,
        };
        let pair = |lhs: Self, rhs: Self| {
            if is_type(&rhs) {
                (quote(lhs), rhs)
            } else if is_type(&lhs) {
                (lhs, quote(rhs))
            } else {
                (lhs.with_type_names_quoted(), rhs.with_type_names_quoted())
            }
        };
        let all = |items: Vec<Self>| {
            items
                .into_iter()
                .map(Self::with_type_names_quoted)
                .collect()
        };
        match self {
            Self::Binary(op @ (Binary::Equal | Binary::NotEqual), lhs, rhs) => {
                let (lhs, rhs) = pair(*lhs, *rhs);
                Self::Binary(op, Box::new(lhs), Box::new(rhs))
            }
            Self::Binary(op, lhs, rhs) => Self::Binary(
                op,
                Box::new(lhs.with_type_names_quoted()),
                Box::new(rhs.with_type_names_quoted()),
            ),
            Self::Call(function @ (Builtin::Is | Builtin::In | Builtin::Contains), arguments) => {
                match <[Self; 2]>::try_from(arguments) {
                    Ok([lhs, rhs]) => {
                        let (lhs, rhs) = pair(lhs, rhs);
                        Self::Call(function, vec![lhs, rhs])
                    }
                    Err(arguments) => Self::Call(function, all(arguments)),
                }
            }
            Self::Call(function, arguments) => Self::Call(function, all(arguments)),
            Self::Unknown(name, arguments) => Self::Unknown(name, all(arguments)),
            Self::Array(items) => Self::Array(all(items)),
            Self::Unary(op, operand) => Self::Unary(op, Box::new(operand.with_type_names_quoted())),
            Self::Index(term, key) => Self::Index(
                Box::new(term.with_type_names_quoted()),
                Box::new(key.with_type_names_quoted()),
            ),
            Self::Dot(term, key) => Self::Dot(Box::new(term.with_type_names_quoted()), key),
            Self::Literal(_) | Self::Name(_) => self,
        }
    }

    /// Whether every name it reads is a field and every function it calls built in.
    fn uses_only_known_names(&self) -> bool {
        match self {
            Self::Literal(_) => true,
            Self::Name(name) => Field::named(name).is_some(),
            Self::Unknown(..) => false,
            Self::Array(items) | Self::Call(_, items) => {
                items.iter().all(Self::uses_only_known_names)
            }
            Self::Unary(_, operand) | Self::Dot(operand, _) => operand.uses_only_known_names(),
            Self::Binary(_, lhs, rhs) | Self::Index(lhs, rhs) => {
                lhs.uses_only_known_names() && rhs.uses_only_known_names()
            }
        }
    }

    /// The text to search for when the expression parsed from `input` is not meant as a
    /// query: a string literal's text, any other literal's or a sign on one as typed, a lone
    /// name but a boolean field, or `input` when it names an unknown field or function.
    fn search_text(&self, input: &str) -> Option<String> {
        match self {
            Self::Literal(Value::Text(text)) => Some(text.clone()),
            Self::Literal(_) => Some(input.to_owned()),
            Self::Name(name) => match Field::named(name) {
                Some(field) if field.is_boolean() => None,
                _ => Some(name.clone()),
            },
            Self::Unary(Unary::Sign(_) | Unary::Not, operand)
                if matches!(**operand, Self::Literal(_)) =>
            {
                Some(input.to_owned())
            }
            _ => (!self.uses_only_known_names()).then(|| input.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests;
