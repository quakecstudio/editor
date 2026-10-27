// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseField, QCC_PR_ParseRefArrayPointer

//! Member access, `a.b` and `a->b`, as FTEQCC types it (FTEF-24): an entity's field, a
//! vector's component, a vector field's component field, a string's length and a struct's or
//! union's member.

use qc_diag::{Code, Span};
use qc_syntax::ast::{self, AstNode as _};

use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::{Constness, Value};
use crate::ty::{SimpleType, Type};

impl Checker<'_> {
    /// The value of the member access `field` (FTEF-24).
    pub(super) fn member(&mut self, field: &ast::FieldExpr) -> Checked {
        let base = self.expr_of(field.base())?;
        let Some(selector) = field.field() else {
            return Ok(Value::unknown());
        };
        // FTEF-24.8: an array's only member is `length`, an `int`; the resolver reports any
        // other (FTE-13).
        if base.is_array_head() {
            return Ok(match selector_name(&selector) {
                Some((name, _)) if name == "length" => length(),
                Some(_) | None => Value::unknown(),
            });
        }
        // FTEF-10.3: an accessor's member, read or stored through its functions.
        if let Some((name, _)) = selector_name(&selector)
            && let Some(accessor) = self.accessor_value(&base, &name)
        {
            return Ok(accessor);
        }
        let Some(shape) = self.env.shape(&base.ty) else {
            self.expr(&selector)?;
            return Ok(Value::unknown());
        };
        Ok(match shape.etype {
            // `QCC_PR_ParseField` reads the entity to select its field.
            Etype::Entity => {
                let entity = self.read(base, field.syntax().span())?;
                self.entity_field(&entity, &selector)?
            }
            Etype::Vector => self.swizzle(&selector, Type::Simple(SimpleType::Float))?,
            Etype::Field if shape.holds == Some(Etype::Vector) => self.swizzle(
                &selector,
                Type::Field(Type::Simple(SimpleType::Float).into()),
            )?,
            Etype::String => self.string_method(&base, &selector)?,
            Etype::Struct | Etype::Union => self.struct_member(&base.ty, &selector),
            Etype::Field if matches!(shape.holds, Some(Etype::Struct | Etype::Union)) => {
                let Type::Field(held) = &base.ty else {
                    return Ok(Value::unknown());
                };
                let member = self.struct_member(held, &selector);
                Value::of(Type::Field(member.ty.into()))
            }
            Etype::Pointer => self.pointer_member(&base, &selector)?,
            Etype::Void
            | Etype::Float
            | Etype::Field
            | Etype::Function
            | Etype::Integer
            | Etype::Uint
            | Etype::Int64
            | Etype::Uint64
            | Etype::Double
            | Etype::Variant
            | Etype::Enum
            | Etype::Boolean => Value::unknown(),
        })
    }

    /// FTEF-24.1: an entity's field, `QCC_PR_ParseField`: the selector, or the expression in
    /// parentheses after `.`, must be a field, whose value is what the field holds; anything else
    /// is "NAME is not a field", or "NAME is not a field of class CLASS" on a class's value.
    fn entity_field(&mut self, base: &Value, selector: &ast::Expr) -> Checked {
        let field = match selector {
            ast::Expr::Name(name) => {
                let Some(token) = name.name() else {
                    return Ok(Value::unknown());
                };
                let name = token.text_lossless();
                // A class's member is a field of its type (`QCC_PR_ParseEntClass`), looked up
                // as `CLASS::NAME` in the base's class and its parents first (FTEF-24.6).
                if let Some(member) = self.env.class_member(&base.ty, &name)
                    && let Some(def) = self.env.definition(member)
                {
                    return Ok(Value::field_of(base, name, def.ty.clone(), Some(member)));
                }
                if let Some(accessor) = self.accessor_value(base, &name) {
                    return Ok(accessor);
                }
                let found = self.env.reference(token.span().start).and_then(|id| {
                    self.env
                        .definition(id)
                        .filter(|def| def.member_of.is_none())
                        .map(|def| (id, def.ty.clone()))
                });
                let Some((id, ty)) = found else {
                    return Ok(self.unknown_field(base, &name, token.span()));
                };
                (ty, name.clone(), name, token.span(), Some(id))
            }
            ast::Expr::Paren(paren) => {
                let value = self.expr_of(paren.expr())?;
                let span = selector.syntax().span();
                let label = value.label();
                (
                    value.ty,
                    String::from_utf8_lossy(&selector.syntax().text()).into_owned(),
                    label,
                    span,
                    value.sym.def(),
                )
            }
            other => {
                self.expr(other)?;
                return Ok(Value::unknown());
            }
        };
        let (ty, name, label, span, def) = field;
        match self.env.shape(&ty).map(|s| s.etype) {
            Some(Etype::Field) => {
                let held = match ty {
                    Type::Field(held) => held.as_ref().clone(),
                    other => other,
                };
                Ok(Value::field_of(base, label, held, def))
            }
            Some(Etype::Variant) => Ok(Value::field_of(
                base,
                label,
                Type::Named("__variant".into()),
                def,
            )),
            None => Ok(Value::unknown()),
            Some(_) => {
                let message = match &base.ty {
                    Type::Named(class) => format!("{name} is not a field of class {class}"),
                    _ => format!("{name} is not a field"),
                };
                self.fatal(Code::E0318, span, message)
            }
        }
    }

    /// FTEF-10.3: an accessor member `NAME` of an accessor or class: the getter's type,
    /// read-only with no setter. Reads and writes through it call its functions.
    pub(super) fn accessor_value(&self, base: &Value, name: &str) -> Option<Value> {
        let facts = self.env.accessor(&base.ty, name)?;
        Some(Value {
            readonly: !facts.set,
            ..Value::of(facts.ty)
        })
    }

    /// FTEF-24.6: a member no definition has is made a `variant` definition by
    /// `QCC_PR_GetSRef(type_variant, ..., true)` and is the error "Unknown field \"NAME\" in class
    /// \"CLASS\"" (`QCC_PR_ParseWarning(ERR_UNKNOWNVALUE, ...)`), which does not end the
    /// statement; the member is a `variant` field of the entity, which may be assigned to.
    fn unknown_field(&mut self, base: &Value, name: &str, span: Span) -> Value {
        let class = match &base.ty {
            Type::Named(class) => class.to_string(),
            _ => "entity".to_owned(),
        };
        let message = format!("Unknown field \"{name}\" in class \"{class}\"");
        self.env.report(Code::E0319, span, message);
        Value::field_of(base, name.to_owned(), Type::Named("__variant".into()), None)
    }

    /// FTEF-24.2: a vector's single-channel swizzle, `x`, `y` or `z` (or `r`, `g`, `b`), is a
    /// float, and on a `.vector` field a `.float` field; any other is "unsupported vector swizzle
    /// '.NAME'".
    fn swizzle(&mut self, selector: &ast::Expr, ty: Type) -> Checked {
        let Some((name, span)) = selector_name(selector) else {
            return Ok(Value::unknown());
        };
        if matches!(name.as_str(), "x" | "y" | "z" | "r" | "g" | "b") {
            return Ok(Value {
                readonly: false,
                ..Value::of(ty)
            });
        }
        self.fatal(
            Code::E0314,
            span,
            format!("unsupported vector swizzle '.{name}'"),
        )
    }

    /// FTEF-24.3: a string's `length` is an `int` when the string is a constant, else "not a
    /// constant"; any other name is "unsupported string method NAME".
    fn string_method(&mut self, base: &Value, selector: &ast::Expr) -> Checked {
        let Some((name, span)) = selector_name(selector) else {
            return Ok(Value::unknown());
        };
        if name != "length" {
            return self.fatal(
                Code::E0315,
                span,
                format!("unsupported string method {name}"),
            );
        }
        if base.constness == Constness::Variable {
            return self.fatal(Code::E0316, span, "not a constant".to_owned());
        }
        Ok(length())
    }

    /// FTEF-24.4: a struct's or union's member, found in it or its parents. One it does not
    /// have is reported by the resolver (FTEF-6.11), and its value is not followed.
    fn struct_member(&mut self, ty: &Type, selector: &ast::Expr) -> Value {
        let Some((name, _)) = selector_name(selector) else {
            return Value::unknown();
        };
        match self.env.struct_member(ty, &name) {
            Some((member, false)) => Value {
                readonly: false,
                ..Value::of(member)
            },
            Some((_, true)) | None => Value::unknown(),
        }
    }

    /// FTEF-24.5: through a pointer, a member of the struct or union it points at; a pointer to
    /// anything else is "indirection in TYPE - not a struct or union".
    fn pointer_member(&mut self, base: &Value, selector: &ast::Expr) -> Checked {
        let Type::Pointer(pointee) = &base.ty else {
            return Ok(Value::unknown());
        };
        match self.env.shape(pointee).map(|s| s.etype) {
            Some(Etype::Struct | Etype::Union) => Ok(self.struct_member(pointee, selector)),
            None => Ok(Value::unknown()),
            Some(_) => {
                let span = selector.syntax().span();
                let message = format!(
                    "indirection in {} - not a struct or union",
                    self.env.type_name(pointee)
                );
                self.fatal(Code::E0317, span, message)
            }
        }
    }

    /// Reports the fatal error `code` (`QCC_PR_ParseError`), which ends the statement's checks
    /// (FTEF-23.6).
    pub(super) fn fatal(&mut self, code: Code, span: Span, message: String) -> Checked {
        self.env.report(code, span, message);
        Err(Fatal)
    }
}

/// The name a member access selects, and its span.
fn selector_name(selector: &ast::Expr) -> Option<(String, Span)> {
    let ast::Expr::Name(name) = selector else {
        return None;
    };
    let token = name.name()?;
    Some((token.text_lossless(), token.span()))
}

/// FTEF-24.8: a `length`, an `int` constant (`QCC_MakeIntConst`) whose value the model does not
/// keep (FTEF-26.5).
pub(super) fn length() -> Value {
    Value {
        constness: Constness::Opaque,
        ..Value::of(Type::Named("__int32".into()))
    }
}
