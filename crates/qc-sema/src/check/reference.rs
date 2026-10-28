// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseRefArrayPointer, QCC_RefToDef, QCC_PR_GenerateAddressOf, QCC_LoadFromArray, QCC_LoadFromPointer, QCC_StoreToArray, QCC_StoreSRefToRef

//! References as FTEQCC builds, reads and stores them (FTEF-31): an index into an array, a
//! vector, a pointer or a string; the read that loads what a reference names; the address of a
//! reference; and the stores to an array's element and a string's character.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::SyntaxKind;
use qc_syntax::ast::{self, AstNode as _};

use crate::cast::ConstType;
use qc_syntax::c_scalar::CScalar;

use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::pointer::Pointee;
use crate::check::{Bitfield, ElementRef, Reference, Sym, Value};
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
        // FTEF-6.15: a bitfield's word is read, then its bits by `OP_BITEXTEND_*`
        // (`QCC_BitfieldToDef`); through a pointer it is loaded by its width.
        if let Some(bitfield) = value.bitfield
            && !matches!(value.reference, Reference::Pointer(_))
        {
            let word = self.read(
                Value {
                    bitfield: None,
                    ..value
                },
                span,
            )?;
            let op = if bitfield.signed {
                Op::BitextendI
            } else {
                Op::BitextendU
            };
            // `QCC_MakeUIntConst((bitofs<<8)|bits)`.
            let mask = bitfield
                .offset
                .checked_shl(8)
                .map_or(bitfield.bits, |high| high | bitfield.bits);
            let mask = Value::constant(Constant::uint(mask));
            let extended = self.emit(op, &word, &mask, span)?;
            return Ok(Value {
                ty: word.ty,
                ..extended
            });
        }
        match value.reference.clone() {
            Reference::Global => self.extend_narrow(value, span),
            Reference::Accessor(accessor) => self.accessor_get(&value, &accessor, span),
            Reference::Field(_) => {
                // FTEF-2.12: `QCC_PR_ExpandField` has no load of an 8- or 16-bit integer.
                if narrow_bits(&value.ty).is_some() {
                    let message = format!(
                        "QCC_PR_ExpandField: invalid field type {}",
                        self.env.type_name(&value.ty)
                    );
                    return self.fatal(Code::E0358, span, message);
                }
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
                // Emitted, or on a target without it as `QCC_PR_StatementFlags` replaces it:
                // `OP_LOAD_I` and `OP_LOAD_P` as `OP_LOAD_F`, `OP_LOAD_I64` as two plain
                // `OP_LOAD_FLD`s (FTEF-27.9, FTEF-27.11).
                self.emit(load, &Value::unknown(), &Value::unknown(), span)?;
                Ok(value)
            }
            Reference::ArrayHead => {
                let address = self.address_of(value, span)?;
                Ok(address)
            }
            Reference::Element(element) => {
                if let Some(bits) = narrow_bits(&value.ty) {
                    return self.narrow_element(value, &element.index, bits, span);
                }
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
                // `QCC_SupplyConversion(ref->index, ev_integer, true)`.
                let index = match &pointer.index {
                    Some(index) if self.etype_of(&index.ty) != Some(Etype::Integer) => {
                        let int = Type::Named("__int32".into());
                        Some(self.supplied(index, &int, span)?)
                    }
                    index => index.clone(),
                };
                self.load_from_pointer(value, index.as_ref(), span)
            }
            Reference::Character(index) => {
                let float_index = self.etype_of(&index.ty) == Some(Etype::Float);
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

    /// FTEF-2.12: a definition of an 8- or 16-bit integer, `ev_bitfld`, read as a bitfield at bit
    /// 0 is ([`Self::extend_bits`]).
    fn extend_narrow(&mut self, value: Value, span: Span) -> Checked {
        match narrow_bits(&value.ty) {
            Some(bits) if value.sym.def().is_some() => self.extend_bits(value, 0, bits, span),
            Some(_) | None => Ok(value),
        }
    }

    /// FTEF-2.12: an element of an array of 8- or 16-bit integers: at a constant index the
    /// bitfield `index * bits` bits in, its word's offset `& 31` (`ret.ofs += bitofs>>5`); at
    /// another, read through its address (`QCC_PR_GenerateAddressOf`) by the narrow load.
    fn narrow_element(&mut self, value: Value, index: &Value, bits: u32, span: Span) -> Checked {
        if let Some(constant) = index.known() {
            let at = if constant.ty == ConstType::Float {
                f32_index(constant.f())
            } else {
                constant.i()
            };
            let at = u32::from_le_bytes(at.to_le_bytes());
            let offset = at.wrapping_mul(bits) & 31;
            return self.extend_bits(value, offset, bits, span);
        }
        self.address_of(value.clone(), span)?;
        let narrow = Value {
            reference: Reference::Global,
            ..value
        };
        self.load_from_pointer(narrow, None, span)
    }

    /// FTEF-2.12: an element at a constant index of an array of 8- or 16-bit integers, or a
    /// field of one, as a store packs it (`QCC_StoreSRefToRef`, `dest->cast->type ==
    /// ev_bitfld`): a bitfield `index * bits` bits into its word, which is read as its
    /// `parentclass`; a field's index is "Unable to evaluate to numeric constant", and 0.
    pub(super) fn narrow_bitfield(&mut self, lhs: &Value, span: Span) -> Option<Value> {
        let bits = narrow_bits(&lhs.ty)?;
        let at = match &lhs.reference {
            Reference::Element(element) => {
                let constant = element.index.known()?;
                if constant.ty == ConstType::Float {
                    f32_index(constant.f())
                } else {
                    constant.i()
                }
            }
            // `QCC_Eval_Int` of the field, which is no number.
            Reference::Field(_) => {
                let message = "Unable to evaluate to numeric constant".to_owned();
                self.env.report(Code::E0359, span, message);
                0
            }
            _ => return None,
        };
        let signed = matches!(lhs.ty.fteqcc_scalar(), Some(CScalar::Int8 | CScalar::Int16));
        let parent = if signed { "__int32" } else { "__uint32" };
        Some(Value {
            ty: Type::Named(parent.into()),
            bitfield: Some(Bitfield {
                bits,
                offset: u32::from_le_bytes(at.to_le_bytes()).wrapping_mul(bits) & 31,
                word: 0,
                signed,
            }),
            ..lhs.clone()
        })
    }

    /// FTEF-2.12: a local of the 8- or 16-bit integer type `ty` as its initialiser packs it, a
    /// bitfield `offset` bits into a word of its `parentclass`.
    pub(super) fn narrow_local(&self, ty: &Type, offset: u32) -> Option<Value> {
        let bits = narrow_bits(ty)?;
        let signed = matches!(ty.fteqcc_scalar(), Some(CScalar::Int8 | CScalar::Int16));
        let parent = if signed { "__int32" } else { "__uint32" };
        Some(Value {
            bitfield: Some(Bitfield {
                bits,
                offset: offset & 31,
                word: 0,
                signed,
            }),
            ..Value::of(Type::Named(parent.into()))
        })
    }

    /// `QCC_BitfieldToDef`: a value of an 8- or 16-bit integer type `offset` bits into its word,
    /// extended by `OP_BITEXTEND_I`, or `OP_BITEXTEND_U` for an unsigned one, with
    /// `(offset<<8)|bits`, to its `parentclass`, `int` or `uint`.
    fn extend_bits(&mut self, value: Value, offset: u32, bits: u32, span: Span) -> Checked {
        let signed = matches!(
            value.ty.fteqcc_scalar(),
            Some(CScalar::Int8 | CScalar::Int16)
        );
        let (op, parent) = if signed {
            (Op::BitextendI, "__int32")
        } else {
            (Op::BitextendU, "__uint32")
        };
        let mask = offset.checked_shl(8).map_or(bits, |high| high | bits);
        let extended = self.emit(op, &value, &Value::constant(Constant::uint(mask)), span)?;
        Ok(Value {
            ty: Type::Named(parent.into()),
            sym: Sym::Temp,
            ..extended
        })
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
    /// pointee's type; a struct or union word by word (FTEF-6.14).
    fn load_from_pointer(&mut self, value: Value, index: Option<&Value>, span: Span) -> Checked {
        // FTEF-6.15, FTEF-27.10: a bitfield and an 8- or 16-bit integer by `OP_LOADP_I8`,
        // `OP_LOADP_U8` and the 16-bit loads, the index 0 when there is none.
        let narrow = match value.bitfield {
            Some(bitfield) => Some((bitfield.bits, bitfield.signed)),
            None => match value.ty.fteqcc_scalar() {
                Some(CScalar::Int8) => Some((8, true)),
                Some(CScalar::Uint8) => Some((8, false)),
                Some(CScalar::Int16) => Some((16, true)),
                Some(CScalar::Uint16) => Some((16, false)),
                _ => None,
            },
        };
        if let Some(narrow) = narrow {
            let op = match narrow {
                (8, true) => Op::LoadpI8,
                (8, false) => Op::LoadpU8,
                (16, true) => Op::LoadpI16,
                (16, false) => Op::LoadpU16,
                _ => {
                    let message = "ptr-to-bitfld has unsupported bit depth".to_owned();
                    return self.fatal(Code::E0353, span, message);
                }
            };
            let pointer = Value::of(Type::Pointer(value.ty.clone().into()));
            let zero = Value::constant(Constant::uint(0));
            let loaded = self.emit(op, &pointer, index.unwrap_or(&zero), span)?;
            return Ok(Value {
                ty: value.ty,
                bitfield: None,
                ..loaded
            });
        }
        let Some(etype) = self.etype_of(&value.ty) else {
            return Ok(value);
        };
        if matches!(etype, Etype::Struct | Etype::Union) {
            self.load_words_from_pointer(&value.ty, index, span)?;
            return Ok(Value {
                reference: Reference::Global,
                sym: Sym::Temp,
                ..value
            });
        }
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
            // A double is loaded word by word with no check of the target.
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

    /// FTEF-6.14: a struct or union of `ty` read through a pointer, `OP_LOADP_V` for each
    /// three words and `OP_LOADP_I` for each word after them (`QCC_PR_SimpleStatement`, whose
    /// opcode the target must have, a fatal error), each word's offset added to the index by
    /// `OP_ADD_I` when there is one.
    fn load_words_from_pointer(
        &mut self,
        ty: &Type,
        index: Option<&Value>,
        span: Span,
    ) -> Result<(), Fatal> {
        let Some(words) = self.env.layout(ty).map(|layout| layout.words) else {
            return Ok(());
        };
        let mut at: u32 = 0;
        while at < words {
            let vector = at.checked_add(2).is_some_and(|last| last < words);
            if let Some(index) = index {
                let offset = Value::constant(Constant::int(i32::try_from(at).unwrap_or(i32::MAX)));
                self.emit(Op::AddI, index, &offset, span)?;
            }
            let (op, step) = if vector {
                (Op::LoadpV, 3)
            } else {
                (Op::LoadpI, 1)
            };
            self.simple_statement(op, span)?;
            at = at.saturating_add(step);
        }
        Ok(())
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
        self.end_of_construct()?;
        if base.ty == Type::Unknown || at.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        let constant = match at.known() {
            Some(c) => Some(match eval_int(c) {
                Some(i) => i,
                None => {
                    // `QCC_Eval_Int`'s default, once for the bounds and once as the element is
                    // read, for an array's, a vector's and a `.vector` field's index (FTEF-31.8).
                    let evaluated = base.is_array_head()
                        || matches!(self.etype_of(&base.ty), Some(Etype::Vector))
                        || matches!(&base.ty, Type::Field(held) if self.etype_of(held) == Some(Etype::Vector));
                    if evaluated {
                        for _ in 0..2 {
                            let message = "Unable to evaluate to numeric constant".to_owned();
                            self.env.report(Code::E0360, span, message);
                        }
                    }
                    0
                }
            }),
            None => None,
        };
        let label = base.label();
        // FTEF-10.7: an accessor member's index, or an accessor's indexer, `get T[index]`.
        if let Some(element) = self.accessor_index(&base, &at) {
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
                let at = self.scaled_index(pointee, at, span)?;
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
                // FTEF-31.9: a constant vector's component at a constant index is a constant.
                let component = base.known().zip(constant).and_then(|(vector, i)| {
                    usize::try_from(i)
                        .ok()
                        .and_then(|i| vector.v().get(i).copied())
                });
                if let Some(component) = component {
                    return Ok(Value {
                        reference: Reference::element(label, at, true),
                        readonly: base.readonly,
                        ..Value::constant(Constant::float(component))
                    });
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
                let expected = expected_after(index.syntax());
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

    /// FTEF-31.7: an array's or a pointer's index scaled to words by the size of what it
    /// selects (`t->size`) when that is more than one word, `OP_MUL_F` for a float index, else
    /// the index made an integer and `OP_MUL_I`; an index of a narrow type (`t->bits`) is made an
    /// integer and multiplied by one, which emits nothing.
    fn scaled_index(&mut self, element: &Type, at: Value, span: Span) -> Checked {
        let words = match self.pointee(&Type::Pointer(element.clone().into())) {
            Pointee::Bits(_) => return self.converted(&at, Etype::Integer, span),
            Pointee::Words(Some(words)) if words > 1 => words,
            Pointee::Void | Pointee::Words(_) => return Ok(at),
        };
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
            reference: Reference::Character(Box::new(at)),
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
            Reference::Field(_) | Reference::Accessor(_) => Ok(pointer_to(&value.ty)),
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
            Reference::Character(_) => Ok(Value::unknown()),
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
            Reference::Character(index) => self.store_character(index, stored, span),
            Reference::Accessor(accessor) => self.accessor_set(lhs, accessor, stored, span),
            // FTEF-2.12: a field of an 8- or 16-bit integer is stored through its address.
            Reference::Field(_) => match narrow_bits(&lhs.ty) {
                Some(16) => self.small_store(Op::StorepI16, stored, None, span),
                Some(_) => self.small_store(Op::StorepI8, stored, None, span),
                None => Ok(()),
            },
            Reference::Global | Reference::ArrayHead | Reference::Element(_) => Ok(()),
            Reference::Pointer(pointer) => {
                // FTEF-6.15: a bitfield through a pointer stores no bit offset.
                if lhs.bitfield.is_some_and(|bitfield| bitfield.offset != 0) {
                    let message = format!("left operand ({}) has a bit offset", pointer.base);
                    self.env.report(Code::E0357, span, message);
                }
                // `QCC_SupplyConversion(dest->index, ev_integer, true)`.
                let index = match &pointer.index {
                    Some(index) if self.etype_of(&index.ty) != Some(Etype::Integer) => {
                        let int = Type::Named("__int32".into());
                        Some(self.supplied(index, &int, span)?)
                    }
                    index => index.clone(),
                };
                self.store_to_pointer(lhs, index.as_ref(), stored, span)
            }
        }
    }

    /// A string's character at `index` stored (`QCC_StoreSRefToRef`, `case REF_STRING:`):
    /// `OP_STOREP_C` for a float when the target has it, else `OP_STOREP_I8` when it has that,
    /// else `OP_STOREP_C` or `OP_STOREP_I8`; the value made a float or an integer for it, and the
    /// index an integer, added to the string by `OP_ADD_I` on a target whose stores take no
    /// offset; then [`Self::small_store`].
    fn store_character(&mut self, index: &Value, stored: &Value, span: Span) -> Result<(), Fatal> {
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
        let index = self.converted(index, Etype::Integer, span)?;
        if target.stores_at_offset() {
            return self.small_store(op, &value, Some(&index), span);
        }
        let string = Value::of(Type::Simple(SimpleType::String));
        self.emit(Op::AddI, &string, &index, span)?;
        self.small_store(op, &value, None, span)
    }
}

/// The token FTEQCC's parser expects where a `[` stands after what is no array: `)` inside
/// parentheses or a call's arguments, `]` inside an index, `;` otherwise.
pub(super) fn expected_after(node: &qc_syntax::SyntaxNode) -> &'static str {
    for node in node.ancestors() {
        match node.kind() {
            SyntaxKind::ParenExpr | SyntaxKind::ArgList => return ")",
            SyntaxKind::IndexExpr => return "]",
            k if ast::Stmt::can_cast(k) => return ";",
            _ => {}
        }
    }
    ";"
}

/// FTEF-31.8: `QCC_Eval_Int` of a constant: C's `(int)` of a float or double, an integer as it
/// is, a 64-bit one's low word; `None` for a vector or a string, "Unable to evaluate to numeric
/// constant".
fn eval_int(c: Constant) -> Option<i32> {
    match c.ty {
        ConstType::Float => Some(f32_index(c.f())),
        ConstType::Double => Some(f64_index(c.d())),
        ConstType::Integer | ConstType::Uint => Some(c.i()),
        ConstType::Int64 | ConstType::Uint64 => Some(low_word(c.i64())),
        ConstType::Vector | ConstType::String => None,
    }
}

/// `(unsigned)QCC_Eval_Int` of a double index, as [`f32_index`] (FTEF-31.8).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-31.8: QCC_Eval_Int of a double is C's double to 64-bit conversion"
)]
const fn f64_index(value: f64) -> i32 {
    low_word(value as i64)
}

/// The low 32 bits of `value`, as C's truncation to `int` keeps them.
const fn low_word(value: i64) -> i32 {
    let [a, b, c, d, ..] = value.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// `(unsigned)QCC_Eval_Int` of a float index: C's `(pint64_t)` of it, saturating on the arm64
/// build the differential is recorded on, then its low word (FTEF-31.8).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-31.8: QCC_Eval_Int of a float is C's float to 64-bit conversion"
)]
const fn f32_index(value: f32) -> i32 {
    low_word(value as i64)
}

/// C's `(float)` of a small `int`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-31.2: an element's size in words is exact as a float"
)]
const fn int_f32(value: i32) -> f32 {
    value as f32
}

/// The bits of an 8- or 16-bit integer type, `ev_bitfld`'s `bits` (FTEF-2.12).
fn narrow_bits(ty: &Type) -> Option<u32> {
    match ty.fteqcc_scalar() {
        Some(CScalar::Int8 | CScalar::Uint8) => Some(8),
        Some(CScalar::Int16 | CScalar::Uint16) => Some(16),
        _ => None,
    }
}
