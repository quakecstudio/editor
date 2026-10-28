// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseField, QCC_PR_ParseRefArrayPointer

//! Member access, `a.b` and `a->b`, as FTEQCC types it (FTEF-24): an entity's field, a
//! vector's component, a vector field's component field, a string's length and a struct's or
//! union's member.

use qc_diag::{Code, Span};
use qc_preproc::Op;
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::CScalar;

use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::reference::expected_after;
use crate::check::{Bitfield, Constness, MemberArray, Reference, Value};
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
            && let Some(accessor) = self.accessor_member(&base, &name)
        {
            // FTEF-10.7: a member that takes an index is followed by its `[`
            // (`QCC_PR_Expect("[")`).
            let indexed = field
                .syntax()
                .parent()
                .and_then(ast::IndexExpr::cast)
                .and_then(|index| index.base())
                .is_some_and(|b| b.syntax() == field.syntax());
            if let Reference::Accessor(member) = &accessor.reference
                && member.facts.indexer.is_some()
                && !indexed
            {
                let found = token_after(field.syntax());
                let message = format!("expected [, found {found}");
                return self.fatal(Code::E0204, field.syntax().span(), message);
            }
            return Ok(accessor);
        }
        // FTEF-10.7: a member an accessor type does not have, but on an entity's, is "NAME is
        // not a member of TYPE", an error that leaves a `void` 0.
        if let Some((name, span)) = selector_name(&selector)
            && self.env.has_accessors(&base.ty)
            && self.etype_of(&base.ty) != Some(Etype::Entity)
        {
            let message = format!("{name} is not a member of {}", base.ty.fteqcc_name());
            self.env.report(Code::E0261, span, message);
            return Ok(Value {
                readonly: false,
                ..Value::of(Type::Simple(SimpleType::Void))
            });
        }
        let Some(shape) = self.env.shape(&base.ty) else {
            self.expr(&selector)?;
            return Ok(Value::unknown());
        };
        Ok(match shape.etype {
            // `QCC_PR_ParseField` reads the entity to select its field.
            Etype::Entity => {
                let entity = self.read(base, field.syntax().span())?;
                let value = self.entity_field(&entity, &selector)?;
                // FTEF-31.6: an array of fields with no index after it is its head, whose
                // address is taken.
                let indexed = field
                    .syntax()
                    .parent()
                    .and_then(ast::IndexExpr::cast)
                    .and_then(|index| index.base())
                    .is_some_and(|b| b.syntax() == field.syntax());
                if let Reference::Field(f) = &value.reference
                    && f.def.is_some_and(|def| self.env.is_array(def))
                    && !indexed
                {
                    let head = Value {
                        reference: Reference::ArrayHead(None),
                        ..value.clone()
                    };
                    self.address_of(head, field.syntax().span())?;
                }
                value
            }
            Etype::Vector => self.swizzle(&selector, Type::Simple(SimpleType::Float))?,
            Etype::Field if shape.holds == Some(Etype::Vector) => self.swizzle(
                &selector,
                Type::Field(Type::Simple(SimpleType::Float).into()),
            )?,
            Etype::String => self.string_method(&base, &selector)?,
            Etype::Struct | Etype::Union => {
                let (member, offset) = self.struct_member(&base.ty, &selector);
                let word = member.bitfield.map_or_else(
                    || offset.map_or(0, |(units, align)| units.saturating_mul(align) >> 5),
                    |bitfield| bitfield.word,
                );
                match &base.reference {
                    // `(*p).a` and `p[i].a` are members through the pointer.
                    Reference::Pointer(pointer) => {
                        let span = field.syntax().span();
                        let index = self.member_index(pointer.index.as_ref(), offset, span)?;
                        Value {
                            reference: Reference::pointer(pointer.base.clone(), Some(index)),
                            ..member
                        }
                    }
                    // `st.f` is the struct's own definition at the member's offset
                    // (`QCC_PR_BuildRef` of its sym), no temporary.
                    _ if member.is_array_head() => Value {
                        sym: base.sym.at(word),
                        name: base.name.clone(),
                        ..member
                    },
                    _ => Value {
                        sym: base.sym.at(word),
                        reference: base.reference.clone(),
                        ..member
                    },
                }
            }
            Etype::Field if matches!(shape.holds, Some(Etype::Struct | Etype::Union)) => {
                let Type::Field(held) = &base.ty else {
                    return Ok(Value::unknown());
                };
                let (member, _) = self.struct_member(held, &selector);
                Value::of(Type::Field(member.ty.into()))
            }
            // The pointer is read to select through it, as an entity is.
            Etype::Pointer => {
                let span = field.syntax().span();
                let pointer = self.read(base, span)?;
                self.pointer_member(&pointer, &selector, span)?
            }
            // FTEF-24.10: a value of a type with no members ends the expression before the
            // `.`: the statement goes on with the value, and the `.` is then "expected ;, found
            // ."; inside parentheses, a call's arguments or an index the construct expects its
            // own token at once.
            Etype::Void
            | Etype::Float
            | Etype::Field
            | Etype::Function
            | Etype::Integer
            | Etype::Uint
            | Etype::Int64
            | Etype::Uint64
            | Etype::Double
            | Etype::Enum
            | Etype::Boolean
            | Etype::Variant => {
                let expected = expected_after(field.syntax());
                let message = format!("expected {expected}, found .");
                let span = field.syntax().span();
                if self.pending.is_none() {
                    self.pending = Some((message, span));
                }
                return Ok(base);
            }
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
                let at = token.span().start;
                if let Some(member) = self.env.class_member(&base.ty, &name, at)
                    && let Some(def) = self.env.definition(member)
                {
                    return Ok(Value::field_of(base, name, def.ty.clone(), Some(member)));
                }
                if let Some(accessor) = self.accessor_member(base, &name) {
                    return Ok(accessor);
                }
                let found = self.env.reference(token.span().start).and_then(|id| {
                    self.env
                        .definition(id)
                        .filter(|def| def.member_of.is_none())
                        .map(|def| (id, def.ty.clone()))
                });
                let Some((id, ty)) = found else {
                    // FTEF-24.9: a name FTEQCC declares as it reads it, an intrinsic's or
                    // `nil`, is a value, and no field.
                    if let Some(shown) = intrinsic_value(&name) {
                        let message = format!("{shown} is not a field");
                        return self.fatal(Code::E0318, token.span(), message);
                    }
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

    /// FTEF-24.6: a member no definition has is made a `variant` definition by
    /// `QCC_PR_GetSRef(type_variant, ..., true)` and is the error "Unknown field \"NAME\" in class
    /// \"CLASS\"" (`QCC_PR_ParseWarning(ERR_UNKNOWNVALUE, ...)`), which does not end the
    /// statement; the member is a `variant` field of the entity, which may be assigned to.
    fn unknown_field(&mut self, base: &Value, name: &str, span: Span) -> Value {
        let class = match &base.ty {
            Type::Named(class) => self
                .env
                .class_name(&base.ty)
                .unwrap_or_else(|| class.to_string()),
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

    /// FTEF-24.4: a struct's or union's member, found in it or its parents, and its offset in
    /// units of its alignment, with that alignment, when the model lays the struct out
    /// (FTEF-6.19). One it does not have is reported by the resolver (FTEF-6.11), and its value
    /// is not followed.
    fn struct_member(&mut self, ty: &Type, selector: &ast::Expr) -> (Value, Option<(u32, u32)>) {
        let Some((name, _)) = selector_name(selector) else {
            return (Value::unknown(), None);
        };
        let Some(member) = self.env.struct_member(ty, &name) else {
            return (Value::unknown(), None);
        };
        // FTEF-6.15: a bitfield of fewer than 32 bits is read and stored by its bits, and so is
        // an 8- or 16-bit integer, `ev_bitfld` (FTEF-2.12).
        let narrow = match member.ty.fteqcc_scalar() {
            Some(CScalar::Int8 | CScalar::Uint8) => Some(8),
            Some(CScalar::Int16 | CScalar::Uint16) => Some(16),
            _ => None,
        };
        // FTEQCC reads and stores an 8- or 16-bit member at bit 0 of the word that holds it.
        let bitfield = match (member.bits.or(narrow), member.position) {
            (Some(bits), Some(position)) if bits != 32 => Some(Bitfield {
                bits,
                offset: if member.bits.is_some() {
                    position & 31
                } else {
                    0
                },
                word: position >> 5,
                signed: self.etype_of(&member.ty) != Some(Etype::Uint),
            }),
            _ => None,
        };
        // A bitfield's `bitofs / bits` more than its word; a narrow integer's in its own units,
        // `ofs*(32/align) + (mbitofs>>3)`.
        let align = alignment(&member.ty);
        let offset = match bitfield.filter(|_| member.bits.is_some()) {
            Some(bitfield) => {
                let element = bitfield.offset.checked_div(bitfield.bits).unwrap_or(0);
                Some((bitfield.word.saturating_add(element), align))
            }
            None => member
                .position
                .and_then(|position| position.checked_div(align))
                .map(|units| (units, align)),
        };
        // An array member is its head, its elements' type (FTEF-6.24).
        let reference = if member.array {
            Reference::ArrayHead(Some(MemberArray {
                length: member.length,
                word: member.position.map_or(0, |position| position >> 5),
                pointer: false,
            }))
        } else {
            Reference::Global
        };
        let value = Value {
            readonly: false,
            bitfield,
            reference,
            ..Value::of(member.ty)
        };
        (value, offset)
    }

    /// FTEF-6.19: the index of a member through a pointer (`QCC_PR_ParseRefArrayPointer`), its
    /// `offset` in units of its alignment: an index before it, of words, multiplied by
    /// `OP_MUL_I` into units of a narrower member's; with an index and an offset of 0 that
    /// index; else the offset, `OP_ADD_I` of the index made an integer and it when the target
    /// has `OP_ADD_I`, else `OP_ADD_F` of the index made a float and the offset as a float times
    /// `32 / align`. An offset the model does not know is an integer constant no check reads.
    fn member_index(
        &mut self,
        index: Option<&Value>,
        offset: Option<(u32, u32)>,
        span: Span,
    ) -> Checked {
        let Some((offset, align)) = offset else {
            return Ok(Value {
                constness: Constness::Opaque,
                ..Value::constant(Constant::int(0))
            });
        };
        let Ok(offset) = i32::try_from(offset) else {
            return Ok(Value {
                constness: Constness::Opaque,
                ..Value::constant(Constant::int(0))
            });
        };
        // `idx = OP_MUL_I(idx, t->align/p->type->align)`: an index of words counted in the
        // member's narrower units.
        let index = match index {
            Some(index) if align != 32 => {
                let index = self.converted(index, Etype::Integer, span)?;
                let per_word = Value::constant(Constant::int(if align == 8 { 4 } else { 2 }));
                Some(self.emit(Op::MulI, &index, &per_word, span)?)
            }
            index => index.cloned(),
        };
        let index = index.as_ref();
        if offset == 0
            && let Some(index) = index
        {
            return Ok(index.clone());
        }
        if self.env.target_at(span.start).valid(Op::AddI) {
            let offset = Value::constant(Constant::int(offset));
            let Some(index) = index else {
                return Ok(offset);
            };
            let index = self.converted(index, Etype::Integer, span)?;
            return self.emit(Op::AddI, &index, &offset, span);
        }
        let per_word = match align {
            8 => 4,
            16 => 2,
            _ => 1,
        };
        let scaled = offset.saturating_mul(per_word);
        #[expect(
            clippy::cast_precision_loss,
            reason = "spec FTEF-6.19: QCC_MakeFloatConst(ofs*(32/align)), C's int to float"
        )]
        let offset = Value::constant(Constant::float(scaled as f32));
        let Some(index) = index else {
            return Ok(offset);
        };
        let index = self.converted(index, Etype::Float, span)?;
        self.emit(Op::AddF, &index, &offset, span)
    }

    /// FTEF-24.5: through a pointer, a member of the struct or union it points at; a pointer to
    /// anything else is "indirection in TYPE - not a struct or union".
    fn pointer_member(&mut self, base: &Value, selector: &ast::Expr, span: Span) -> Checked {
        let Type::Pointer(pointee) = &base.ty else {
            return Ok(Value::unknown());
        };
        match self.env.shape(pointee).map(|s| s.etype) {
            // FTEF-6.14: read through the pointer at the member's offset, a bitfield's after
            // `bitofs / bits` more (FTEF-6.15).
            Some(Etype::Struct | Etype::Union) => {
                let (member, offset) = self.struct_member(pointee, selector);
                // A member array through the pointer is indexed through it (FTEF-6.24).
                if let Reference::ArrayHead(Some(array)) = member.reference {
                    return Ok(Value {
                        name: Some(base.label()),
                        reference: Reference::ArrayHead(Some(MemberArray {
                            pointer: true,
                            ..array
                        })),
                        ..member
                    });
                }
                let index = self.member_index(None, offset, span)?;
                Ok(Value {
                    reference: Reference::pointer(base.label(), Some(index)),
                    ..member
                })
            }
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

/// The name a value FTEQCC declares as it reads an unknown name has, as its messages print
/// it (`QCC_PR_ParseRefValue`): `nil` is the integer constant 0, `IMMEDIATE`; the intrinsics
/// are functions of their own names. `None` for any other name.
fn intrinsic_value(name: &str) -> Option<&str> {
    match name {
        "nil" => Some("IMMEDIATE"),
        "randomv" | "alloca" | "entnum" | "autocvar" | "used_model" | "used_sound" | "va_arg"
        | "..." | "_" | "random" => Some(name),
        _ => None,
    }
}

/// The bits a member of type `ty` is aligned to, `p->type->align`: 8 or 16 for a narrow
/// integer, else a word's 32.
fn alignment(ty: &Type) -> u32 {
    match ty.fteqcc_scalar() {
        Some(CScalar::Int8 | CScalar::Uint8) => 8,
        Some(CScalar::Int16 | CScalar::Uint16) => 16,
        _ => 32,
    }
}

/// The text of the token after `node` in its tree, empty at the end.
fn token_after(node: &qc_syntax::SyntaxNode) -> String {
    let end = node.span().end;
    node.ancestors()
        .last()
        .into_iter()
        .flat_map(|root| root.tokens().collect::<Vec<_>>())
        .find(|token| !token.kind().is_trivia() && token.span().start >= end)
        .map(|token| token.text_lossless())
        .unwrap_or_default()
}
