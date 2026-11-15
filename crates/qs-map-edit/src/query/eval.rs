// SPDX-License-Identifier: GPL-3.0-or-later

//! Evaluating a query's expression for one object, its fields read from it.

use super::domain::{Field, Fields};
use super::syntax::{Binary, Builtin, Expr, Unary};
use super::value::{Fails, Value};

impl Expr {
    /// The value of this expression for the object whose fields `fields` reads.
    pub(crate) fn evaluate(&self, fields: &dyn Fields) -> Result<Value, Fails> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Name(name) => Field::named(name)
                .map(|field| fields.field(field))
                .ok_or(Fails),
            Self::Array(items) => items
                .iter()
                .map(|item| item.evaluate(fields))
                .collect::<Result<_, _>>()
                .map(Value::Array),
            Self::Unary(op, operand) => {
                let value = operand.evaluate(fields)?;
                match op {
                    Unary::Sign(sign) => value.signed(*sign),
                    Unary::Not => value.not(),
                }
            }
            Self::Binary(op, lhs, rhs) => Self::binary(*op, lhs, rhs, fields),
            Self::Call(function, arguments) => {
                let values: Vec<Value> = arguments
                    .iter()
                    .map(|argument| argument.evaluate(fields))
                    .collect::<Result<_, _>>()?;
                Self::call(*function, &values)
            }
            Self::Unknown(..) => Err(Fails),
            Self::Index(term, key) => term.evaluate(fields)?.index(&key.evaluate(fields)?),
            Self::Dot(term, key) => term.evaluate(fields)?.index(&Value::Text(key.clone())),
        }
    }

    /// `lhs op rhs`; `&&` and `||` evaluate `rhs` only when `lhs` leaves the answer open.
    fn binary(op: Binary, lhs: &Self, rhs: &Self, fields: &dyn Fields) -> Result<Value, Fails> {
        let logical = |value: Value| -> Result<Option<bool>, Fails> {
            match value {
                Value::Undefined => Ok(None),
                Value::Bool(b) => Ok(Some(b)),
                Value::Null => Ok(Some(false)),
                _ => Err(Fails),
            }
        };
        let left = lhs.evaluate(fields)?;
        let compare = |right: &Value, test: fn(core::cmp::Ordering) -> bool| {
            left.compare(right).map(|order| Value::Bool(test(order)))
        };
        match op {
            Binary::And | Binary::Or => {
                let deciding = op == Binary::Or;
                match logical(left)? {
                    None => Ok(Value::Undefined),
                    Some(b) if b == deciding => Ok(Value::Bool(b)),
                    Some(_) => {
                        Ok(logical(rhs.evaluate(fields)?)?.map_or(Value::Undefined, Value::Bool))
                    }
                }
            }
            Binary::Arithmetic(arithmetic) => left.arithmetic(arithmetic, &rhs.evaluate(fields)?),
            Binary::Less => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_lt),
            Binary::LessOrEqual => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_le),
            Binary::Greater => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_gt),
            Binary::GreaterOrEqual => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_ge),
            Binary::Equal => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_eq),
            Binary::NotEqual => compare(&rhs.evaluate(fields)?, core::cmp::Ordering::is_ne),
        }
    }

    /// A built-in function called with `values`.
    fn call(function: Builtin, values: &[Value]) -> Result<Value, Fails> {
        if values.len() != function.arity() {
            return Err(Fails);
        }
        let any_undefined = values.iter().any(|v| matches!(v, Value::Undefined));
        match (function, values) {
            (Builtin::Vec, _) if any_undefined => Ok(Value::Undefined),
            (Builtin::Vec, [x, y, z]) => Ok(Value::Vector([x.number()?, y.number()?, z.number()?])),
            (Builtin::Bbox, _) if any_undefined => Ok(Value::Undefined),
            (Builtin::Bbox, [min, max]) => Ok(Value::bounds(min.vector()?, max.vector()?)),
            (Builtin::Is, [a, b]) => a.equals(b).map(Value::Bool),
            (Builtin::Like, [value, pattern]) => Ok(Value::Bool(value.like(pattern))),
            (Builtin::Contains, [container, value]) | (Builtin::In, [value, container]) => {
                Ok(Value::Bool(container.contains(value)))
            }
            (Builtin::DistanceTo, [a, b]) => a.distance_to(b),
            (Builtin::Intersects, [a, b]) => a.intersects(b),
            _ => Err(Fails),
        }
    }

    /// Whether this query matches the object: it evaluates to `true`, not an error or any
    /// other value.
    pub(crate) fn holds(&self, fields: &dyn Fields) -> bool {
        self.evaluate(fields) == Ok(Value::Bool(true))
    }
}
