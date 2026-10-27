// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseRefArrayPointer, QCC_RefToDef, QCC_PR_GenerateAddressOf, QCC_LoadFromArray, QCC_LoadFromPointer, QCC_StoreToArray, QCC_PR_Statement_SmallStore

//! References as FTEQCC builds, reads and stores them (FTEF-31): an index into an array, a
//! vector, a pointer or a string; the read that loads what a reference names; the address of a
//! reference; and the stores to an array's element and a string's character.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::SyntaxKind;
use qc_syntax::ast::{self, AstNode as _};

use crate::cast::ConstType;
use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::{ElementRef, Reference, Sym, Value};
use crate::ty::{SimpleType, Type};

impl Checker<'_> {
    /// The value of `expr`, read (`QCC_PR_Expression`, `QCC_RefToDef`).
    pub(super) fn value(&mut self, expr: Option<ast::Expr>) -> Checked {
        let Some(expr) = expr else {
            return Ok(Value::unknown());
        };
        let reference = self.expr(&expr)?;
        self.read(reference, expr.syntax().span())
    }

    /// FTEF-31.1: `QCC_RefToDef`, what a reference names, read: an array named bare is its
    /// address (FTEF-31.4); an element at an index that is no constant is loaded as the target
    /// allows (FTEF-31.2); what a pointer points at is loaded by the `OP_LOADP_*` of its type,
    /// a `void` or `variant` "ptr-to-variants must be cast to a different type before being
    /// read" (FTEF-31.3); a string's character by `OP_LOADP_C` or `OP_LOADP_U8`.
    pub(super) fn read(&mut self, value: Value, span: Span) -> Checked {
        if value.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(value);
        }
        match value.reference.clone() {
            Reference::Global => Ok(value),
            Reference::Field(_) => {
                // FTEF-34.9: an entity's field is loaded by `OP_LOAD_*` of its type.
                let load = match self.etype_of(&value.ty) {
                    Some(Etype::Vector) => Op::LoadV,
                    Some(Etype::String) => Op::LoadS,
                    Some(Etype::Entity) => Op::LoadEnt,
                    Some(Etype::Field) => Op::LoadFld,
                    Some(Etype::Function) => Op::LoadFnc,
                    Some(Etype::Integer | Etype::Uint) => Op::LoadI,
                    Some(Etype::Pointer) => Op::LoadP,
                    Some(Etype::Int64 | Etype::Uint64 | Etype::Double) => Op::LoadI64,
                    _ => Op::LoadF,
                };
                self.generate(load, span);
                Ok(value)
            }
            Reference::ArrayHead => {
                let address = self.address_of(value, span)?;
                Ok(address)
            }
            Reference::Element(element) => {
                if element.index.known().is_none() {
                    self.load_from_array(&value, &element.index, element.vector, span)?;
                }
                Ok(Value {
                    reference: Reference::Global,
                    sym: Sym::Temp,
                    ..value
                })
            }
            Reference::Pointer(pointer) => {
                if let Some(index) = &pointer.index
                    && self.etype_of(&index.ty) != Some(Etype::Integer)
                {
                    self.converted(index, Etype::Integer, span)?;
                }
                self.load_from_pointer(value, span)
            }
            Reference::Character { float_index } => {
                let op = if float_index { Op::LoadpC } else { Op::LoadpU8 };
                let string = Value::of(Type::Simple(SimpleType::String));
                let loaded = self.emit(op, &string, &Value::unknown(), span)?;
                Ok(Value {
                    ty: value.ty,
                    ..loaded
                })
            }
        }
    }

    /// FTEF-31.2: `QCC_LoadFromArray`'s index: as an integer by `OP_LOADA_*` when the target has
    /// them, or `OP_GLOAD_*`; as a float by `OP_FETCH_GBL_*` for a global array with its length
    /// before it; else by the `ArrayGet` function, a float, a vector's component index divided
    /// by 3.
    fn load_from_array(
        &mut self,
        element: &Value,
        index: &Value,
        vector: bool,
        span: Span,
    ) -> Result<(), Fatal> {
        let target = self.env.target_at(span.start);
        let index_etype = self.etype_of(&index.ty);
        let length_prefix = !vector
            && element.sym.def().is_some_and(|def| !self.env.is_local(def))
            && target.valid(Op::FetchGblF);
        let as_integer = target.valid(Op::LoadaF) || target.valid(Op::GloadF);
        if as_integer {
            if index_etype != Some(Etype::Integer) {
                self.converted(index, Etype::Integer, span)?;
            }
        } else if length_prefix {
            if index_etype != Some(Etype::Float) {
                self.converted(index, Etype::Float, span)?;
            }
        } else {
            let float = self.converted(index, Etype::Float, span)?;
            if vector {
                let three = Value::constant(Constant::float(3.0));
                self.emit(Op::DivF, &float, &three, span)?;
            }
        }
        Ok(())
    }

    /// FTEF-31.3: `QCC_LoadFromPointer`: the index made an integer, then the load of the
    /// pointee's type.
    fn load_from_pointer(&mut self, value: Value, span: Span) -> Checked {
        let Some(etype) = self.etype_of(&value.ty) else {
            return Ok(value);
        };
        let op = match etype {
            Etype::Int64 | Etype::Uint64 => Op::LoadpI64,
            Etype::Float => Op::LoadpF,
            Etype::String => Op::LoadpS,
            Etype::Vector => Op::LoadpV,
            Etype::Entity => Op::LoadpEnt,
            Etype::Field => Op::LoadpFld,
            Etype::Function => Op::LoadpFnc,
            Etype::Integer | Etype::Uint => Op::LoadpI,
            Etype::Pointer if self.env.target_at(span.start).valid(Op::LoadpP) => Op::LoadpP,
            Etype::Pointer => Op::LoadpI,
            Etype::Void | Etype::Variant => {
                let message = "ptr-to-variants must be cast to a different type before being read";
                return self.fatal(Code::E0336, span, message.to_owned());
            }
            // A struct, union or double is loaded word by word with no check of the target.
            Etype::Struct | Etype::Union | Etype::Double | Etype::Enum | Etype::Boolean => {
                return Ok(Value {
                    reference: Reference::Global,
                    sym: Sym::Temp,
                    ..value
                });
            }
        };
        let pointer = Value::of(Type::Pointer(value.ty.clone().into()));
        let loaded = self.emit(op, &pointer, &Value::unknown(), span)?;
        Ok(Value {
            ty: value.ty,
            ..loaded
        })
    }

    /// FTEF-31.2: `base[index]` (`QCC_PR_ParseRefArrayPointer`): an array's element, with the
    /// warning "(constant) array index out of bounds (0 <= I < N)" for a constant index past
    /// its end and the index scaled by an element of more than one word; what a pointer points
    /// at; a string's character, or under `qccx` the string offset; a vector's or a `.vector`
    /// field's component, a constant index past 2 the fatal error "(vector) array index out of
    /// bounds" or "(.vector) ..."; anything else is no array, and FTEQCC's parser stops at the
    /// `[`, "expected ;, found [".
    pub(super) fn index(&mut self, index: &ast::IndexExpr) -> Checked {
        let span = index.syntax().span();
        let base = self.expr_of(index.base())?;
        let at = self.value(index.index())?;
        if base.ty == Type::Unknown || at.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        let constant = at.known().map(|c| {
            if c.ty == ConstType::Float {
                f32_index(c.f())
            } else {
                c.i()
            }
        });
        let label = base.label();
        // FTEF-10.3: an accessor's indexer, `get T[index]`.
        if let Some(element) = self.accessor_value(&base, "") {
            return Ok(element);
        }
        // An array of fields, `this.arr[i]`: the field the index selects, loaded from the array
        // of fields as the index is read, an entity's field to read or store.
        if let Reference::Field(field) = &base.reference
            && let Some(def) = field.def
            && self.env.is_array(def)
        {
            let length = self.env.array_length(def);
            self.bounds(constant, length, span);
            if at.known().is_none() {
                let field = Value {
                    sym: Sym::Def(def, false),
                    ..Value::of(base.ty.clone())
                };
                self.load_from_array(&field, &at, false, span)?;
            }
            return Ok(base);
        }
        if base.is_array_head() {
            let length = base.sym.def().and_then(|def| self.env.array_length(def));
            self.bounds(constant, length, span);
            let at = self.scaled_index(&base.ty, at, span)?;
            return Ok(Value {
                reference: Reference::element(label, at, false),
                ..base
            });
        }
        match (self.etype_of(&base.ty), &base.ty) {
            (Some(Etype::Pointer), Type::Pointer(pointee)) => {
                let pointer = self.read(base.clone(), span)?;
                Ok(Value {
                    readonly: false,
                    name: pointer.name.or(Some(label.clone())),
                    reference: Reference::pointer(label, Some(at)),
                    ..Value::of(pointee.as_ref().clone())
                })
            }
            (Some(Etype::String), _) => {
                let string = self.read(base, span)?;
                self.string_index(string, at, span)
            }
            // An entity's `.vector` field indexed is the field's component, `e.(fv[i])`.
            (Some(Etype::Vector), _) if matches!(base.reference, Reference::Field(_)) => {
                if constant.is_some_and(|i| u32::from_le_bytes(i.to_le_bytes()) >= 3) {
                    let message = "(.vector) array index out of bounds".to_owned();
                    return self.fatal(Code::E0333, span, message);
                }
                Ok(Value {
                    reference: Reference::element(label, at, false),
                    readonly: base.readonly,
                    ..Value::of(Type::Simple(SimpleType::Float))
                })
            }
            (Some(Etype::Vector), _) => {
                if constant.is_some_and(|i| u32::from_le_bytes(i.to_le_bytes()) >= 3) {
                    let message = "(vector) array index out of bounds".to_owned();
                    return self.fatal(Code::E0333, span, message);
                }
                Ok(Value {
                    reference: Reference::element(label, at, true),
                    ..Value {
                        name: base.name,
                        sym: base.sym,
                        readonly: base.readonly,
                        ..Value::of(Type::Simple(SimpleType::Float))
                    }
                })
            }
            (Some(Etype::Field), Type::Field(held))
                if self.etype_of(held) == Some(Etype::Vector) =>
            {
                if constant.is_some_and(|i| u32::from_le_bytes(i.to_le_bytes()) >= 3) {
                    let message = "(.vector) array index out of bounds".to_owned();
                    return self.fatal(Code::E0333, span, message);
                }
                Ok(Value {
                    reference: Reference::element(label, at, false),
                    ..Value {
                        name: base.name,
                        sym: base.sym,
                        readonly: base.readonly,
                        ..Value::of(Type::Field(Type::Simple(SimpleType::Float).into()))
                    }
                })
            }
            _ => {
                let expected = expected_after(index);
                let message = format!("expected {expected}, found [");
                self.fatal(Code::E0204, span, message)
            }
        }
    }

    /// "(constant) array index out of bounds (0 <= I < N)" for a constant index past an array's
    /// end.
    fn bounds(&mut self, constant: Option<i32>, length: Option<u32>, span: Span) {
        if let (Some(i), Some(length)) = (constant, length)
            && u32::from_le_bytes(i.to_le_bytes()) >= length
        {
            let message = format!("(constant) array index out of bounds (0 <= {i} < {length})");
            self.env.report(Code::W0249, span, message);
        }
    }

    /// An array's index scaled to words when an element takes more than one: made an integer
    /// when the target has `OP_ADD_I` and the index is one, then multiplied by the element's
    /// size, `OP_MUL_F` for a float index and `OP_MUL_I` otherwise.
    fn scaled_index(&mut self, element: &Type, at: Value, span: Span) -> Checked {
        let words = match self.etype_of(element) {
            Some(Etype::Vector) => 3,
            Some(Etype::Int64 | Etype::Uint64 | Etype::Double) => 2,
            _ => 1,
        };
        if words == 1 {
            return Ok(at);
        }
        if self.etype_of(&at.ty) == Some(Etype::Float) {
            let size = Value::constant(Constant::float(int_f32(words)));
            return self.emit(Op::MulF, &at, &size, span);
        }
        let at = self.converted(&at, Etype::Integer, span)?;
        self.emit(Op::MulI, &at, &Value::constant(Constant::int(words)), span)
    }

    /// FTEF-31.2: a string's index: under `qccx` the string offset, a float index the warning
    /// "string offsetting emulation: denormals are unsafe" and `OP_ADD_F`, else `OP_ADD_I`; else
    /// its character, a float for a float index and an integer otherwise.
    fn string_index(&mut self, string: Value, at: Value, span: Span) -> Checked {
        let float_index = self.etype_of(&at.ty) == Some(Etype::Float);
        if self.env.flags_at(span.start).has(FteFlag::Qccx) {
            let offset = if float_index {
                let message = "string offsetting emulation: denormals are unsafe".to_owned();
                self.env.report(Code::W0237, span, message);
                self.emit(Op::AddF, &string, &at, span)?
            } else {
                let at = self.converted(&at, Etype::Integer, span)?;
                self.emit(Op::AddI, &string, &at, span)?
            };
            return Ok(Value {
                ty: Type::Simple(SimpleType::String),
                readonly: true,
                ..offset
            });
        }
        let ty = if float_index {
            Type::Simple(SimpleType::Float)
        } else {
            Type::Named("__int32".into())
        };
        Ok(Value {
            readonly: string.readonly,
            name: string.name,
            reference: Reference::Character { float_index },
            ..Value::of(ty)
        })
    }

    /// FTEF-31.4: `QCC_PR_GenerateAddressOf`: a field's address by `OP_ADDRESS`; a
    /// definition's, an element's or an array's by `OP_GLOBALADDRESS`, which a target without it
    /// refuses, the fatal error "Address-of operator is not supported in this form without
    /// extensions", with the warning for a local not `static` "Address-of operator on local TYPE
    /// NAME is unsafe if recursing" once per definition; a pointer's element by `OP_ADD_PIW`.
    pub(super) fn address_of(&mut self, value: Value, span: Span) -> Checked {
        let pointer_to = |ty: &Type| Value::of(Type::Pointer(ty.clone().into()));
        match value.reference.clone() {
            Reference::Field(_) => Ok(pointer_to(&value.ty)),
            Reference::Global | Reference::ArrayHead | Reference::Element(_) => {
                if !self.env.target_at(span.start).valid(Op::Globaladdress) {
                    let suggestion = if value.is_array_head() {
                        "either '#pragma target fte' or '#pragma flag enable brokenarray'"
                    } else {
                        ": #pragma target fte"
                    };
                    let message = format!(
                        "Address-of operator is not supported in this form without extensions. \
                         Consider the use of{}{suggestion}",
                        if value.is_array_head() { " " } else { "" }
                    );
                    return self.fatal(Code::E0331, span, message);
                }
                if let Some(def) = value.sym.def()
                    && self.env.is_local(def)
                    && self.addressed.insert(def)
                {
                    let message = format!(
                        "Address-of operator on local {} {} is unsafe if recursing (and still \
                         bloated when not). Consider use of 'static' or 'auto'.",
                        self.env.type_name(&value.ty),
                        value.label()
                    );
                    self.env.report(Code::W0250, span, message);
                }
                if let Reference::Element(element) = &value.reference
                    && element.index.known().is_none()
                {
                    self.converted(&element.index, Etype::Integer, span)?;
                }
                Ok(pointer_to(&value.ty))
            }
            Reference::Pointer(pointer) => {
                // `&p[i]` is `p + i`, by `OP_ADD_PIW`.
                if let Some(index) = &pointer.index
                    && index.known().is_none_or(|c| c.i() != 0)
                {
                    let index = self.converted(index, Etype::Integer, span)?;
                    let pointer = pointer_to(&value.ty);
                    self.emit(Op::AddPiw, &pointer, &index, span)?;
                }
                Ok(pointer_to(&value.ty))
            }
            Reference::Character { .. } => Ok(Value::unknown()),
        }
    }

    /// `&e` (`QCC_PR_RefTerm`): under `qccx` a float is a pointer to a float, and a string,
    /// field, entity or function a float; a temporary not reached through a pointer is the
    /// error "Address-of temp on line FILE:LINE" and stays the value it is; anything else is
    /// its address (FTEF-31.4).
    pub(super) fn address_term(&mut self, operand: Value, span: Span) -> Checked {
        if self.env.flags_at(span.start).has(FteFlag::Qccx) {
            match self.etype_of(&operand.ty) {
                Some(Etype::Float) => {
                    return Ok(Value::of(Type::Pointer(
                        Type::Simple(SimpleType::Float).into(),
                    )));
                }
                Some(Etype::String | Etype::Field | Etype::Entity | Etype::Function) => {
                    return Ok(Value::of(Type::Simple(SimpleType::Float)));
                }
                _ => {}
            }
        }
        let temporary = operand.sym == Sym::Temp
            && operand.name.is_none()
            && operand.reference == Reference::Global;
        if temporary {
            let message = format!("Address-of temp on line {}", self.env.file_line(span));
            self.env.report(Code::E0335, span, message);
            return Ok(operand);
        }
        self.address_of(operand, span)
    }

    /// FTEF-31.5: the store to an element at an index that is no constant (`QCC_StoreToArray`):
    /// by its address when the target has `OP_GLOBALADDRESS` or `OP_GSTOREP_F`, the index made
    /// an integer; else by the `ArraySet` function, a value of another type than the element's
    /// the fatal error "Type mismatch on indexed assignment of NAME: A VALUE, needed B.", a
    /// vector's component index divided by 3, the index made a float. A string's character is
    /// stored by `OP_STOREP_C` or `OP_STOREP_I8`, emulated as a word's load, mask and or.
    pub(super) fn store_reference(
        &mut self,
        lhs: &Value,
        stored: &Value,
        span: Span,
    ) -> Result<(), Fatal> {
        match &lhs.reference {
            Reference::Element(element) if element.index.known().is_none() => {
                let ElementRef {
                    base,
                    index,
                    vector,
                } = element.as_ref();
                let target = self.env.target_at(span.start);
                if target.valid(Op::Globaladdress) || target.valid(Op::GstorepF) {
                    if self.etype_of(&index.ty) != Some(Etype::Integer) {
                        self.converted(index, Etype::Integer, span)?;
                    }
                    return Ok(());
                }
                let null = stored.known().is_some_and(Constant::null);
                if !null && self.etype_of(&stored.ty) != self.etype_of(&lhs.ty) {
                    let message = format!(
                        "Type mismatch on indexed assignment of {base}: {} {}, needed {}.",
                        self.env.type_name(&stored.ty),
                        stored.label(),
                        self.env.type_name(&lhs.ty)
                    );
                    return self.fatal(Code::E0337, span, message).map(|_| ());
                }
                let float = self.converted(index, Etype::Float, span)?;
                if *vector {
                    let three = Value::constant(Constant::float(3.0));
                    self.emit(Op::DivF, &float, &three, span)?;
                }
                Ok(())
            }
            Reference::Character { .. } => self.store_character(stored, span),
            Reference::Global
            | Reference::Field(_)
            | Reference::ArrayHead
            | Reference::Element(_)
            | Reference::Pointer(_) => Ok(()),
        }
    }

    /// A string's character stored: `OP_STOREP_C` for a float when the target has it, else
    /// `OP_STOREP_I8` when it has that, else `OP_STOREP_C` or `OP_STOREP_I8`; the value made a
    /// float or an integer for it; a target without it emulates the store
    /// (`QCC_PR_Statement_SmallStore`): the word loaded with `OP_LOADP_I`, the byte masked out
    /// with `OP_BITAND_I` and the value or-ed in with `OP_BITOR_I`.
    fn store_character(&mut self, stored: &Value, span: Span) -> Result<(), Fatal> {
        let target = self.env.target_at(span.start);
        let float = self.etype_of(&stored.ty) == Some(Etype::Float);
        let op = if (float && target.valid(Op::StorepC))
            || (!target.valid(Op::StorepI8) && target.valid(Op::StorepC))
        {
            Op::StorepC
        } else {
            Op::StorepI8
        };
        let to = if op == Op::StorepC {
            Etype::Float
        } else {
            Etype::Integer
        };
        let value = self.converted(stored, to, span)?;
        if target.valid(op) {
            return Ok(());
        }
        let value = if op == Op::StorepC {
            self.converted(&value, Etype::Uint, span)?
        } else {
            value
        };
        let word = Value::of(Type::Named("__int32".into()));
        let pointer = Value::of(Type::Pointer(word.ty.clone().into()));
        let loaded = self.emit(Op::LoadpI, &pointer, &Value::unknown(), span)?;
        let mask = Value::constant(Constant::uint(0xff));
        let masked = self.emit(Op::BitandI, &value, &mask, span)?;
        let cleared = self.emit(Op::BitandI, &loaded, &Value::unknown(), span)?;
        self.emit(Op::BitorI, &cleared, &masked, span)?;
        Ok(())
    }
}

/// The token FTEQCC's parser expects where a `[` stands after what is no array: `)` inside
/// parentheses or a call's arguments, `]` inside an index, `;` otherwise.
fn expected_after(index: &ast::IndexExpr) -> &'static str {
    for node in index.syntax().ancestors() {
        match node.kind() {
            SyntaxKind::ParenExpr | SyntaxKind::ArgList => return ")",
            SyntaxKind::IndexExpr => return "]",
            k if ast::Stmt::can_cast(k) => return ";",
            _ => {}
        }
    }
    ";"
}

/// C's `(int)` of a float index (`QCC_Eval_Int`).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-31.2: QCC_Eval_Int of a float constant is C's float to int conversion"
)]
const fn f32_index(value: f32) -> i32 {
    value as i32
}

/// C's `(float)` of a small `int`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-31.2: an element's size in words is exact as a float"
)]
const fn int_f32(value: i32) -> f32 {
    value as f32
}
