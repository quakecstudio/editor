// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking function bodies: the type of every expression and the diagnostics of expressions
//! and statements, as FTEQCC's `QCC_PR_Expression` and `QCC_PR_ParseStatement` and `qcc`'s
//! `PR_Expression` and `PR_ParseStatement` check them (`docs/design/semantic-model.md`).
//!
//! The resolver runs the check of each body where it resolves the body, so the definitions and
//! types the check sees are those declared before the body in compilation order, as in the
//! one-pass compilers. It reads them through [`Env`], which the resolver implements; the check
//! knows nothing else of resolution.

mod accessor;
mod assign;
mod call;
mod compare;
pub(crate) mod constant;
pub(crate) mod convert;
mod emulate;
pub(crate) mod etype;
mod expr;
mod flow;
pub(crate) mod fold;
mod format;
mod member;
pub(crate) mod opcodes;
mod pointer;
pub(crate) mod reference;
mod statement;
mod term;
mod truth;
mod vanilla;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::rc::Rc;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_preproc::OpcodeTarget;
use qc_syntax::CompilerFlags;
use qc_syntax::ast::{self, AstNode as _};

use crate::cast::{ConstType, Literal, Source};
use crate::lower::LiteralValue;
use crate::resolve::{AccessorFacts, Layout};
use crate::symbol::{DefId, Definition};
use crate::ty::Type;
use constant::{Constant, unquoted};

use etype::Etype;

/// What the check stage reads of the module around the body it checks.
pub(crate) trait Env {
    /// The module's temporaries (FTEF-27.16).
    fn temps(&mut self) -> &mut Temps;
    /// The definition the name written at offset `at` of the body's text resolved to.
    fn reference(&self, at: usize) -> Option<DefId>;
    /// The text of a string constant definition, its quotes removed (FTEF-33.1).
    fn string_text(&self, def: DefId) -> Option<String>;
    /// The accessor member `member` (empty for the indexer) of the accessor or class `ty`
    /// names, through a class's parents (FTEF-10.3).
    fn accessor(&self, ty: &Type, member: &str) -> Option<AccessorFacts>;
    /// Whether the type `ty` names declares accessor members, `t->accessors`, through a class's
    /// parents.
    fn has_accessors(&self, ty: &Type) -> bool;
    /// Whether `ty` is an accessor type (`ev_accessor`, FTEF-10.8), which opcodes never take.
    fn is_accessor(&self, ty: &Type) -> bool;
    /// The type of the local whose name starts at offset `at`, through typedefs.
    fn local_type(&self, at: usize) -> Option<Type>;
    /// FTEF-17.17: the global `autocvar_name` of type `ty`, which an `autocvar` call makes and
    /// gives the default `value`: whether a value it was given before differs, which ends the
    /// call.
    fn autocvar(&mut self, name: &str, ty: &Type, value: CvarValue, span: Span) -> bool;
    /// FTEF-17.19: `path` named for the program's `list` of sounds, models, textures or files,
    /// which warns of a backslash the first time the list has it.
    fn record_path(&mut self, list: PathList, path: &str, span: Span);
    /// Whether the local whose name starts at offset `at` is `static` (FTEF-21.14).
    fn local_static(&self, at: usize) -> bool;
    /// FTEF-19.62: the local whose name starts at offset `at` is an array of `size` elements.
    fn size_local(&mut self, at: usize, size: u32);
    /// The type a cast or `sizeof` writes starting at offset `at`, through typedefs.
    fn written_type(&self, at: usize) -> Option<Type>;
    /// A definition of the module.
    fn definition(&self, def: DefId) -> Option<&Definition>;
    /// Whether a definition is an array (`def->arraysize`, FTEF-19.8).
    fn is_array(&self, def: DefId) -> bool;
    /// Whether `ty` is a strict enum, `enum class`, whose values no cast takes (FTEF-19.10).
    fn strict_enum(&self, ty: &Type) -> bool;
    /// Whether the field `def` is the module's first, at offset 0 (FTEF-31.17).
    fn first_field(&self, def: DefId) -> bool;
    /// Whether a definition is a constant, and its value (`QCC_SRef_EvalConst`, FTEF-26.5).
    fn constness(&self, def: DefId) -> Constness;
    /// `FILE:LINE` of offset `span.start`, as `s_filen` and `pr_source_line` print it.
    fn file_line(&self, span: Span) -> String;
    /// The parameters of a function definition as a call reads them (FTEF-32), `None` for a
    /// definition with none recorded.
    fn params(&self, def: DefId) -> Option<Vec<ParamFacts>>;
    /// The length of an array definition, when the model reads it (`def->arraysize`).
    fn array_length(&self, def: DefId) -> Option<u32>;
    /// Whether a definition is a function's local or parameter (`sym->scope`).
    fn is_local(&self, def: DefId) -> bool;
    /// Whether FTEQCC made a definition constant (`def->constant`, FTEF-19.3), which makes it
    /// read-only to an assignment (FTEF-29.1) whether or not its value is known.
    fn constant(&self, def: DefId) -> bool;
    /// What `ty` is at bottom, through typedefs, enums, accessors and truth values; `None` for
    /// a type the model does not follow, on which nothing is checked.
    fn shape(&self, ty: &Type) -> Option<Shape>;
    /// The compiler flags in force at offset `at` (FTEF-21.1).
    fn flags_at(&self, at: usize) -> CompilerFlags;
    /// The lexer's flags in force at offset `at` (FTEF-12.12).
    fn lex_flags_at(&self, at: usize) -> LexFlags;
    /// The output target and the opcodes it has at offset `at` (FTEF-25).
    fn target_at(&self, at: usize) -> OpcodeTarget;
    /// Whether a definition named `name` is visible where the body is, as
    /// `QCC_PR_GetSRef(NULL, name, pr_scope, false, ...)` finds an emulation's helper.
    fn defines(&self, name: &str) -> bool;
    /// Whether the body is a member function's (`pr_classtype`).
    fn in_class(&self) -> bool;
    /// The class of `this` in a member function's body, `pr_classtype` (FTEF-17.14).
    fn this_type(&self) -> Option<Type>;
    /// Whether the module follows FTEQCC (PRJ-2.8).
    fn fteqcc(&self) -> bool;
    /// Reports a diagnostic at `span` of the body's text, under the warning settings in force
    /// there.
    fn report(&mut self, code: Code, span: Span, message: String);
    /// The member `name` of the class `ty` names, found in it or its parents (`CLASS::NAME`,
    /// FTEF-24.6), as the body sees it at offset `at` (FTEF-17.16); `None` when `ty` is no
    /// class or none has the member.
    fn class_member(&self, ty: &Type, name: &str, at: usize) -> Option<DefId>;
    /// The name of the class `ty` names, its typedefs followed (`QCC_TypeForName`); `None`
    /// when it names none.
    fn class_name(&self, ty: &Type) -> Option<String>;
    /// Whether `ty` names a class declared but never defined, which has no parent class
    /// (FTEF-24.15).
    fn class_undefined(&self, ty: &Type) -> bool;
    /// The type the definition `def` names, when it stands for a type's name (FTEF-17.1).
    fn type_named(&self, def: DefId) -> Option<Type>;
    /// Whether `ty` is a struct or union of size 0, named with no body yet or laid out to no
    /// word (FTEF-6.28).
    fn struct_of_no_size(&self, ty: &Type) -> bool;
    /// Whether `a` and `b` are one type by FTEQCC's `typecmp_lax`, comparing what fields,
    /// pointers and functions hold (FTEF-30.3).
    fn typecmp_lax(&self, a: &Type, b: &Type) -> bool;
    /// Whether `a` and `b` are one type by FTEQCC's `typecmp` of values' types (FTEF-30.3).
    fn typecmp(&self, a: &Type, b: &Type) -> bool;
    /// `ty` as FTEQCC's messages print it, `TypeName` (FTEF-24.7).
    fn type_name(&self, ty: &Type) -> String;
    /// The member `name` of the struct or union `ty`, through its parents
    /// (`QCC_PR_FindStructMember`, FTEF-6.11).
    fn struct_member(&self, ty: &Type, name: &str) -> Option<StructMemberFacts>;
    /// FTEQCC's implicit cast of `source` to `to` at `span` (`QCC_EvaluateCast(src, to, true)`,
    /// FTEF-19.31), reported there; whether it is refused, "Cannot cast", a fatal error.
    fn cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool;
    /// A cast the program writes, `QCC_EvaluateCast(src, to, false)` (FTEF-30.6), reported at
    /// `span`; whether it is refused, "Cannot cast", a fatal error.
    fn explicit_cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool;
    /// `QCC_SupplyConversion(value, wanted, true)` of a value of type `ty` (FTEF-22.3),
    /// reported at `span`; whether it is refused, a fatal error.
    fn supply(&mut self, ty: &Type, wanted: &Type, span: Span) -> bool;
    /// The function whose body is checked (`pr_scope`).
    fn function(&self) -> FunctionFacts;
    /// The line of offset `span.start`, counted from 1, as `pr_source_line` holds it.
    fn line(&self, span: Span) -> u32;
    /// The items of the strict enum `ty`, in the order they were declared, each with its value
    /// when the model knows it; empty for any other type.
    fn enum_items(&self, ty: &Type) -> Vec<(String, Option<Constant>)>;
    /// The size of `ty` as FTEQCC lays it out (FTEF-6.13), `None` when the model does not follow
    /// it.
    fn layout(&self, ty: &Type) -> Option<Layout>;
    /// Whether an error, which ends FTEQCC's parse of the function (`QCC_PR_ParseError`), was
    /// reported in the body; a warning made an error is none.
    fn failed(&self) -> bool;
}

/// What the checks of a body read of its function (`pr_scope`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FunctionFacts {
    /// Its name, `pr_scope->name`.
    pub(crate) name: String,
    /// The type it returns, `pr_scope->type->aux_type`.
    pub(crate) ret: Type,
    /// Whether `accumulate` marks it, so that its parts are one function.
    pub(crate) accumulate: bool,
    /// Whether it takes `...`, so it has a `__va_list`.
    pub(crate) variadic: bool,
}

/// What a type is at bottom, as FTEQCC's checker reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    /// The basic type, an accessor or truth value taken for its `parentclass`.
    pub(crate) etype: Etype,
    /// For a field or a pointer, the basic type it holds or points at.
    pub(crate) holds: Option<Etype>,
}

/// The type of a checked expression and what the checker knows of its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Value {
    /// Its type; [`Type::Unknown`] after an error, on which no further error is reported.
    pub(crate) ty: Type,
    /// Whether it is a constant, and its value (`QCC_SRef_EvalConst`, FTEF-26).
    pub(crate) constness: Constness,
    /// The name of the definition it is, which FTEQCC's messages print (`sym->name`).
    pub(crate) name: Option<String>,
    /// The symbol that holds it, which the comparison warnings read (FTEF-28).
    pub(crate) sym: Sym,
    /// Whether an assignment to it is to a constant (`ref->readonly`, FTEF-29.1).
    pub(crate) readonly: bool,
    /// What it refers to, which a store names and a compound assignment may update in place.
    pub(crate) reference: Reference,
    /// Whether it is the operand of a postfix `++` or `--` (`ref->postinc`).
    pub(crate) postinc: bool,
    /// A string constant's text, its quotes and adjacent parts' quotes removed, which a format
    /// string's check reads (FTEF-33.1).
    pub(crate) text: Option<Rc<str>>,
    /// The bitfield it is, read and stored by its bits (FTEF-6.15).
    pub(crate) bitfield: Option<Bitfield>,
    /// The word offset of the temporary that holds it, `temp_N` (FTEF-27.16).
    pub(crate) temp: Option<u32>,
    /// Whether it is a call's return value, an alias of `OFS_RETURN` that a statement which
    /// clobbers the aliases copies to a temporary first (`QCC_ClobberDef`, FTEF-30.13).
    pub(crate) returned: bool,
}

/// FTEQCC's temporaries (`tempsinfo`), kept across the module's functions: each one's word
/// offset, size and references. `opt_overlaptemps` gives a statement's result the first free
/// one of its size, else a new one after the last (`QCC_GetTemp`), and a statement frees its
/// operands first (`QCC_FreeTemp`) (FTEF-27.16).
#[derive(Debug, Default)]
pub(crate) struct Temps {
    slots: Vec<TempSlot>,
}

/// One temporary: its word offset, its size in words and its references.
#[derive(Debug)]
struct TempSlot {
    offset: u32,
    size: u32,
    refs: u32,
}

impl Temps {
    /// A temporary of `size` words for a statement's result.
    pub(crate) fn take(&mut self, size: u32) -> u32 {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.refs == 0 && slot.size == size)
        {
            slot.refs = 1;
            return slot.offset;
        }
        let offset = self
            .slots
            .last()
            .map_or(0, |last| last.offset.saturating_add(last.size));
        self.slots.push(TempSlot {
            offset,
            size,
            refs: 1,
        });
        offset
    }

    /// Frees a reference to the temporary at `offset`.
    pub(crate) fn free(&mut self, offset: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.offset == offset) {
            slot.refs = slot.refs.saturating_sub(1);
        }
    }

    /// Frees every temporary, as a statement's end leaves none referenced.
    pub(crate) fn release_all(&mut self) {
        for slot in &mut self.slots {
            slot.refs = 0;
        }
    }
}

/// What holds a value, `QCC_sref_t`'s `sym` as the comparison warnings read it (FTEF-28.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sym {
    /// A temporary the compiler made.
    Temp,
    /// A definition, whether it is constant (`sym->constant`), and the word in it the value is
    /// at (`ofs`), which the comparison warnings compare too.
    Def(DefId, bool, u32),
    /// An immediate constant, which FTEQCC shares among equal values.
    Immediate(Constant),
}

impl Sym {
    /// The definition it is, if any.
    pub(crate) const fn def(self) -> Option<DefId> {
        match self {
            Self::Def(def, _, _) => Some(def),
            Self::Temp | Self::Immediate(_) => None,
        }
    }

    /// The same definition `words` further on (`ofs + words`); a temporary or an immediate at
    /// another word is none the comparison warnings know.
    pub(crate) const fn at(self, words: u32) -> Self {
        match self {
            Self::Def(def, constant, ofs) => Self::Def(def, constant, ofs.saturating_add(words)),
            Self::Temp | Self::Immediate(_) => Self::Temp,
        }
    }

    /// Whether it is constant and no temporary (`sym->constant && !sym->temp`).
    pub(crate) const fn constant(self) -> bool {
        match self {
            Self::Temp => false,
            Self::Def(_, constant, _) => constant,
            Self::Immediate(_) => true,
        }
    }
}

impl Value {
    /// A value of `ty` that is no literal.
    pub(crate) const fn of(ty: Type) -> Self {
        Self {
            ty,
            constness: Constness::Variable,
            name: None,
            sym: Sym::Temp,
            readonly: true,
            reference: Reference::Global,
            postinc: false,
            text: None,
            bitfield: None,
            temp: None,
            returned: false,
        }
    }

    /// The constant `constant`, of its own type.
    pub(crate) fn constant(constant: Constant) -> Self {
        Self {
            constness: Constness::Known(constant),
            sym: Sym::Immediate(constant),
            ..Self::of(constant.ty.value_type())
        }
    }

    /// Its value, when it is a constant the model knows the value of.
    pub(crate) const fn known(&self) -> Option<Constant> {
        match self.constness {
            Constness::Known(constant) => Some(constant),
            Constness::Variable | Constness::Opaque => None,
        }
    }

    /// Whether it is a constant 0 of an integer, float or pointer type (`QCC_SRef_IsNull`).
    pub(crate) fn null(&self) -> bool {
        self.known().is_some_and(Constant::null)
    }

    /// What a cast reads of it: a constant of its own type, or a null constant of any type
    /// (`QCC_SRef_IsNull`), as a literal, whose nullness the cast reads (FTEF-19.31), anything
    /// else by its type.
    pub(crate) fn source(&self) -> Source<'_> {
        match self.known() {
            Some(constant) if constant.ty.value_type() == self.ty || constant.null() => {
                Source::Literal(Literal {
                    ty: constant.ty,
                    null: constant.null(),
                })
            }
            Some(_) | None => Source::Typed(&self.ty),
        }
    }

    /// The name FTEQCC prints for it (`QCC_GetSRefName`, FTEF-27.4): `IMMEDIATE` for an
    /// immediate, its definition's name, or for a temporary the name of its type.
    pub(crate) fn label(&self) -> String {
        match (self.sym, self.name.as_deref()) {
            (Sym::Immediate(_), _) => "IMMEDIATE".to_owned(),
            (_, Some(name)) => name.to_owned(),
            // `cast->name`: a function type parsed is named `__function` (FTEF-32.11).
            (_, None) if self.ty.is_function() => "__function".to_owned(),
            (_, None) => self.ty.fteqcc_name(),
        }
    }

    /// The name `QCC_VarAtOffset` prints for it, the emulation warnings' and the refcount's:
    /// `temp_N` for a temporary a statement made (FTEF-27.16), else [`Value::label`].
    pub(crate) fn at_offset(&self) -> String {
        match (self.name.as_deref(), self.temp) {
            (None, Some(offset)) if !matches!(self.sym, Sym::Immediate(_)) => {
                format!("temp_{offset}")
            }
            _ => self.label(),
        }
    }

    /// The value of an expression the checker does not follow, on which nothing is reported.
    pub(crate) const fn unknown() -> Self {
        Self::of(Type::Unknown)
    }

    /// `QCC_GetRefName`: `base.field` for a field, `base[index]` for an element or a string's
    /// character, `base->index` through a pointer (`TEMP` for no index), else its label, an
    /// accessor's member included (FTEF-29.8).
    pub(crate) fn ref_name(&self) -> String {
        let index_name =
            |index: Option<&Value>| index.map_or_else(|| "TEMP".to_owned(), Value::label);
        match &self.reference {
            Reference::Field(field) => format!("{}.{}", field.base, field.field),
            Reference::Element(element) => format!("{}[{}]", element.base, element.index.label()),
            Reference::Character(index) => format!("{}[{}]", self.label(), index.label()),
            Reference::Pointer(pointer) => {
                format!("{}->{}", pointer.base, index_name(pointer.index.as_ref()))
            }
            // The accessor branch writes its name and then `break`s, so the base's is returned.
            Reference::Accessor(_) | Reference::Global | Reference::ArrayHead(_) => self.label(),
        }
    }

    /// How FTEQCC's store names its target (`QCC_StoreSRefToRef`): `base.field` for a field,
    /// `base[index]` for an element at an index that is no immediate, else its label.
    pub(crate) fn place(&self) -> String {
        match &self.reference {
            Reference::Field(field) => format!("{}.{}", field.base, field.field),
            // `base[index]`, but for an immediate index, which names no symbol.
            Reference::Element(element) if !matches!(element.index.sym, Sym::Immediate(_)) => {
                format!("{}[{}]", element.base, element.index.label())
            }
            Reference::Element(element) => element.base.clone(),
            Reference::Global
            | Reference::ArrayHead(_)
            | Reference::Pointer(_)
            | Reference::Character(_)
            | Reference::Accessor(_) => self.label(),
        }
    }

    /// Whether it is an array named bare (`REF_ARRAYHEAD`).
    pub(crate) const fn is_array_head(&self) -> bool {
        matches!(self.reference, Reference::ArrayHead(_))
    }

    /// The field `field` of the entity `base`, holding a `ty`, which may be assigned to; `def`
    /// the field's definition.
    pub(crate) fn field_of(base: &Self, field: String, ty: Type, def: Option<DefId>) -> Self {
        Self {
            readonly: false,
            reference: Reference::field(base.label(), field, def),
            ..Self::of(ty)
        }
    }
}

/// What a call reads of a parameter of the function it calls (`QCC_typeparam_s`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParamFacts {
    /// Its name, `paramname`.
    pub(crate) name: Option<String>,
    /// Whether it has a default value, `defltvalue`.
    pub(crate) has_default: bool,
    /// Whether `optional` marks it.
    pub(crate) optional: bool,
    /// Whether it is an `out` parameter, stored back after the call.
    pub(crate) out: bool,
}

/// A struct or union member as the checks read it (FTEF-6.11, FTEF-6.15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StructMemberFacts {
    /// Its type, a bitfield's being its base integer type.
    pub(crate) ty: Type,
    /// Whether it is an array.
    pub(crate) array: bool,
    /// An array's first size, when it is an integer literal (FTEF-6.24).
    pub(crate) length: Option<u32>,
    /// A bitfield's width in bits, when the model knows it.
    pub(crate) bits: Option<u32>,
    /// The bit it starts at from the struct's start, when the model lays the struct out.
    pub(crate) position: Option<u32>,
}

/// An array a struct's member is (FTEF-6.24): its first size, when the model knows it, and the
/// word of the struct it starts at, which its index is added to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemberArray {
    /// Its first size.
    pub(crate) length: Option<u32>,
    /// The word of the struct it starts at.
    pub(crate) word: u32,
    /// Whether the struct is reached through a pointer, the value's name, whose elements are
    /// read and stored through it.
    pub(crate) pointer: bool,
}

/// A bitfield a value is (`ev_bitfld`, FTEF-6.15): its width and whether its base is signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bitfield {
    /// The width in bits.
    pub(crate) bits: u32,
    /// The bit it starts at within its word, `bitofs`.
    pub(crate) offset: u32,
    /// The word it is in, from the struct's start.
    pub(crate) word: u32,
    /// Whether its base is `int`, read by `OP_BITEXTEND_I`; `uint` is `OP_BITEXTEND_U`.
    pub(crate) signed: bool,
}

/// What a value refers to, as FTEQCC's references are of kinds (`QCC_ref_t::type`). The kinds
/// that carry names are boxed, so that a value stays small on the checker's stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    /// A definition, temporary or immediate, read directly (`REF_GLOBAL`).
    Global,
    /// An entity's field (`REF_FIELD`).
    Field(Box<FieldRef>),
    /// An array named bare, whose type is its elements' (`REF_ARRAYHEAD`): reading it takes its
    /// address (FTEF-31.4). A struct's member array carries its length and its word in the
    /// struct, which no definition holds (FTEF-6.24).
    ArrayHead(Option<MemberArray>),
    /// A word of a definition at an index (`REF_ARRAY`, `REF_GLOBAL` with an index).
    Element(Box<ElementRef>),
    /// What a pointer points at (`REF_POINTER`), at an index or not (FTEF-31.3).
    Pointer(Box<PointerRef>),
    /// A string's character (`REF_STRING`) at its index, a float or an integer (FTEF-31.2).
    Character(Box<Value>),
    /// An accessor's member, read and stored through its functions (`REF_ACCESSOR`, FTEF-10.7).
    Accessor(Box<AccessorRef>),
}

/// An accessor's member: what it is, its name and its index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessorRef {
    /// The member, as its accessor declares it.
    pub(crate) facts: AccessorFacts,
    /// Its name, empty for the indexer.
    pub(crate) name: String,
    /// Its index, when it takes one and one was given.
    pub(crate) index: Option<Value>,
}

/// An entity's field: the labels of the entity and of the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldRef {
    /// The entity's label.
    pub(crate) base: String,
    /// The field's label.
    pub(crate) field: String,
    /// The field's definition, when it is one; an array of fields is indexed (FTEF-31.2).
    pub(crate) def: Option<DefId>,
}

/// An array's element or a vector's component; the index is the model's to read only when it is
/// no constant (FTEF-31.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ElementRef {
    /// The definition's label.
    pub(crate) base: String,
    /// The index.
    pub(crate) index: Value,
    /// Whether the definition is a vector, indexed by component.
    pub(crate) vector: bool,
}

/// What a pointer points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PointerRef {
    /// The pointer's label.
    pub(crate) base: String,
    /// The index, an element's.
    pub(crate) index: Option<Value>,
}

impl Reference {
    /// The field `field` of the entity labelled `base`, `def` its definition.
    pub(crate) fn field(base: String, field: String, def: Option<DefId>) -> Self {
        Self::Field(Box::new(FieldRef { base, field, def }))
    }

    /// The element at `index` of the definition labelled `base`, a vector's component when
    /// `vector`.
    pub(crate) fn element(base: String, index: Value, vector: bool) -> Self {
        Self::Element(Box::new(ElementRef {
            base,
            index,
            vector,
        }))
    }

    /// The accessor member `name`, described by `facts`, at `index`.
    pub(crate) fn accessor(facts: AccessorFacts, name: String, index: Option<Value>) -> Self {
        Self::Accessor(Box::new(AccessorRef { facts, name, index }))
    }

    /// What the pointer labelled `base` points at, at `index`.
    pub(crate) fn pointer(base: String, index: Option<Value>) -> Self {
        Self::Pointer(Box::new(PointerRef { base, index }))
    }
}

/// Whether a value is a constant (`QCC_SRef_EvalConst`, FTEF-26.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Constness {
    /// Not a constant: no folding reads it.
    Variable,
    /// A constant the model knows the value of.
    Known(Constant),
    /// A constant whose value the model does not follow; nothing that its value decides, folding
    /// or emulation, is checked.
    Opaque,
}

/// The lists of files FTEQCC keeps as it reads the program's calls (FTEF-17.19).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PathList {
    /// `precache_sound`, `sound`, `localsound` and `used_sound`.
    Sound,
    /// `precache_model`, `setmodel` and `used_model`.
    Model,
    /// `precache_texture`.
    Texture,
    /// `precache_file`.
    File,
}

/// The default an `autocvar` call gives its cvar, as the call compares it with the cvar's
/// earlier value (FTEF-17.17).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CvarValue {
    /// A number or vector constant.
    Number(Constant),
    /// A string immediate's text.
    Text(String),
    /// A value the model keeps no words of.
    Unknown,
}

impl CvarValue {
    /// The value a global's initialiser `literal` gives it.
    pub(crate) fn of_literal(literal: &LiteralValue) -> Self {
        match literal {
            LiteralValue::Float(bits) => Self::Number(Constant::float(f32::from_bits(*bits))),
            LiteralValue::Vector(bits) => Self::Number(Constant::vector(bits.map(f32::from_bits))),
            LiteralValue::String(text) => Self::Text(unquoted(text)),
            LiteralValue::Builtin { .. } | LiteralValue::Text(_) => Self::Unknown,
        }
    }

    /// The initialiser that gives a global this value, which a later definition of it is
    /// compared with; `None` for one of a type the model keeps no literal of.
    pub(crate) fn literal(&self) -> Option<LiteralValue> {
        match self {
            Self::Number(c) if c.ty == ConstType::Float => Some(LiteralValue::Float(c.u())),
            Self::Number(c) if c.ty == ConstType::Vector => {
                Some(LiteralValue::Vector(c.v().map(f32::to_bits)))
            }
            Self::Text(text) => Some(LiteralValue::String(format!("\"{text}\""))),
            Self::Number(_) | Self::Unknown => None,
        }
    }
}

/// A checked function body: the type of each expression, by where it starts and ends in the
/// body's text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypedBody {
    types: HashMap<(usize, usize), Type>,
}

impl TypedBody {
    /// The type of the expression at `span`, if the check typed one there.
    #[must_use]
    pub fn type_at(&self, span: Span) -> Option<&Type> {
        self.types.get(&(span.start, span.end))
    }
}

/// FTEF-34.9: checks the initialiser `expr` of the global `name` of type `ty` at global scope:
/// read, cast to `ty`, and "initializer for 'NAME' is not constant" when it is no constant.
/// FTEF-34.9: checks a global's initialiser `expr` at global scope, and returns what it folds
/// to.
pub(crate) fn check_initialiser(
    env: &mut dyn Env,
    expr: &ast::Expr,
    ty: &Type,
    name: &str,
) -> Option<crate::InitialValue> {
    let mut checker = expr::Checker::new(env);
    checker.global = true;
    checker.initialiser(expr, ty, name)
}

/// Whether a value of basic type `from` stored to `to` is converted by opcodes (FTEF-22).
pub(crate) fn converts(from: etype::Etype, to: etype::Etype) -> bool {
    !emulate::conversion_opcodes(from, to).is_empty()
}

/// FTEF-19.62: the size `expr` of a global array, read at global scope and folded; one that is
/// no constant is "Array length is not a constant". The size, when it is one.
pub(crate) fn array_length(env: &mut dyn Env, expr: &ast::Expr) -> Option<u32> {
    let mut checker = expr::Checker::new(env);
    checker.global = true;
    checker.array_length(expr).ok().flatten()
}

/// Checks `body`, reporting through `env`, and returns its types.
pub(crate) fn check_body(env: &mut dyn Env, body: &ast::FunctionBody) -> TypedBody {
    if !env.fteqcc() {
        return vanilla::check_body(env, body);
    }
    let mut checker = expr::Checker::new(env);
    checker.nested = body
        .syntax()
        .ancestors()
        .skip(1)
        .any(|node| node.kind() == qc_syntax::SyntaxKind::FunctionBody);
    if let Some(block) = body.block()
        && checker.start_function(block.syntax().span())
    {
        checker.block(&block);
        let end = block
            .syntax()
            .token_of(qc_syntax::SyntaxKind::RBrace)
            .map_or_else(|| block.syntax().span(), |brace| brace.span());
        checker.finish_function(end);
    }
    checker.finish()
}
