// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseDefs

//! Structs and unions: their definitions, redeclaration and members (FTEF-6).

use qc_diag::Code;

use crate::check::StructMemberFacts;
use crate::lower::{MemberBase, RawStruct, StructMember};
use crate::resolve::{ComparedMember, MAX_CLASS_DEPTH, Resolver, Scope, StructDef, StructMatch};
use crate::symbol::{FileId, Location};
use crate::ty::Type;

impl<'a> Resolver<'a> {
    /// Declares the structs of `structs` made before `end`, in order. The structs a function's
    /// body writes are its own (`QCC_PR_ParseStruct` with `pr_scope`, dropped with the block's
    /// locals, FTEF-6.10): a run of them is declared together, checked, and taken out of the
    /// file's types again.
    pub(in crate::resolve) fn declare_structs_before(
        &mut self,
        file: FileId,
        structs: &mut std::iter::Peekable<std::slice::Iter<'a, RawStruct>>,
        end: usize,
    ) {
        while let Some(st) = structs.next_if(|st| st.end <= end) {
            if !st.local {
                self.declare_struct(file, st);
                continue;
            }
            let saved = self.struct_defs.clone();
            self.declare_struct(file, st);
            while let Some(st) = structs.next_if(|st| st.local && st.end <= end) {
                self.declare_struct(file, st);
            }
            self.struct_defs = saved;
        }
    }

    /// `QCC_PR_ParseStruct` (FTEF-6.7 to FTEF-6.10): a struct's parent, its redefinition and its
    /// members. A struct a function's body writes lasts to the block's end and is not followed
    /// here.
    pub(in crate::resolve) fn declare_struct(&mut self, file: FileId, st: &RawStruct) {
        if !self.profile.type_declarations() {
            return;
        }
        let location = self.at(file, st.span);
        let mut parent_key = None;
        if let Some((parent, _)) = &st.parent {
            // `QCC_TypeForName(parentname)` follows a typedef to the struct it names.
            let parent = match self.resolved(&Type::Named(parent.as_str().into())) {
                Type::Named(n) => n
                    .strip_prefix("struct ")
                    .or_else(|| n.strip_prefix("union "))
                    .unwrap_or(&n)
                    .to_owned(),
                _ => parent.clone(),
            };
            let parent = &parent;
            let key = self.type_key(parent);
            // The struct's own name is no type yet (`newt = QCC_TypeForName(tname)` comes after
            // the parent): `struct S : S` is "not yet defined".
            let is_struct = self.struct_defs.contains_key(&key)
                || (st.local
                    && self.type_names.contains(parent.as_str())
                    && !self.other_sized_type_named(parent));
            let own = st.name.as_deref().is_some_and(|n| self.type_key(n) == key);
            let known = is_struct
                || matches!(
                    parent.as_str(),
                    "void" | "float" | "vector" | "string" | "entity"
                )
                || (!own && self.is_type_name(parent, &[], st.at));
            if !known {
                self.report(
                    Code::E0247,
                    location,
                    format!("Parent type {parent} was not yet defined"),
                );
                return;
            }
            if !is_struct {
                self.report(
                    Code::E0248,
                    location,
                    format!("Parent type {parent} is not a struct"),
                );
                return;
            }
            parent_key = Some(key);
        }
        let name = st.name.as_deref();
        let key = name.map(|n| self.type_key(n));
        let known = key.as_ref().and_then(|k| self.struct_defs.get(k)).cloned();
        let sized = name.is_some_and(|n| self.sized_type_named(n));
        if let Some(def) = &known
            && (def.defined || def.parent.is_some())
            && let Some(parent_key) = &parent_key
            && def.parent.as_ref() != Some(parent_key)
        {
            self.report(
                Code::E0249,
                location,
                "Redeclaration of struct with different parent type".to_owned(),
            );
            return;
        }
        if !st.body {
            if let (Some(key), None) = (&key, &known) {
                self.struct_defs.insert(
                    key.clone(),
                    StructDef {
                        name: name.unwrap_or_default().to_owned(),
                        members: Vec::new(),
                        parent: parent_key,
                        defined: false,
                        union: st.union,
                    },
                );
            }
            return;
        }
        let members: Vec<&StructMember> = st.members.iter().collect();
        self.check_struct_members(file, location, st, &members);
        // `oldparm = QCC_PR_FindStructMember(newt, parmname, &ofs, &bitofs)`: a member the
        // parent has with the name and type is the parent's, not counted, so a body of such
        // members alone "has no members" (FTEF-6.7).
        if let Some(parent_key) = &parent_key
            && !st.members.is_empty()
            && st
                .members
                .iter()
                .all(|m| self.parent_has_member(parent_key, m))
        {
            let kind = if st.union { "union" } else { "struct" };
            let message = format!("{kind} {} has no members", name.unwrap_or_default());
            self.report(Code::E0243, location, message);
            return;
        }
        let owned: Vec<StructMember> = st.members.clone();
        // `if (STRCMP(a->name, b->name)) return 1;`: the tags are compared exactly, so under
        // `-Fcaseinsens` a body given again under another spelling of the tag differs.
        // A `static` member is a global, among the members compared no more than counted.
        let compared = |members: &[StructMember]| -> Vec<ComparedMember> {
            members
                .iter()
                .filter(|m| !m.is_static)
                .map(|m| (m.name.clone(), m.ty.clone(), m.array.clone(), m.bits))
                .collect()
        };
        let same = |old: &StructDef| {
            old.name == name.unwrap_or_default() && compared(&old.members) == compared(&owned)
        };
        if sized
            && let Some(old) = known.as_ref()
            && !same(old)
        {
            let kind = if st.union { "union" } else { "struct" };
            self.report(
                Code::E0246,
                location,
                format!("{kind} {} redeclared differently", name.unwrap_or_default()),
            );
            return;
        }
        if sized && known.is_none() {
            // A type of another kind has the name, which no body of ours equals.
            let kind = if st.union { "union" } else { "struct" };
            self.report(
                Code::E0246,
                location,
                format!("{kind} {} redeclared differently", name.unwrap_or_default()),
            );
            return;
        }
        if let Some(key) = key {
            self.struct_defs.insert(
                key,
                StructDef {
                    name: name.unwrap_or_default().to_owned(),
                    members: owned,
                    parent: parent_key,
                    defined: true,
                    union: st.union,
                },
            );
        }
    }

    /// Whether the struct of `parent_key`, or one of its parents, has a member named and typed
    /// as `member` is (`QCC_PR_FindStructMember`, FTEF-6.11).
    pub(in crate::resolve) fn parent_has_member(
        &self,
        parent_key: &str,
        member: &StructMember,
    ) -> bool {
        let mut current = Some(parent_key.to_owned());
        for _ in 0..MAX_CLASS_DEPTH {
            let Some(key) = current else {
                return false;
            };
            let Some(def) = self.struct_defs.get(&key) else {
                return false;
            };
            if def.members.iter().any(|m| {
                let same_name = if self.profile.case_insensitive() {
                    m.name.eq_ignore_ascii_case(&member.name)
                } else {
                    m.name == member.name
                };
                same_name && m.ty == member.ty && m.array == member.array
            }) {
                return true;
            }
            current = def.parent.clone();
        }
        false
    }

    /// The checks of a struct's members (FTEF-6.7): each member's type is one, and a bitfield's
    /// is an integer's.
    pub(in crate::resolve) fn check_struct_members(
        &mut self,
        file: FileId,
        location: Location,
        st: &RawStruct,
        members: &[&StructMember],
    ) {
        for member in members {
            if self.check_type_names(&member.ty, &[], st.at, location) {
                return;
            }
            // FTEF-6.9: the width is `QCC_PR_IntConstExpr()`'s, whose names are values and
            // whose literal is an integer ("Value is not an integer constant").
            self.resolve_uses(file, None, &member.width_uses);
            if !member.width_integer {
                self.report(
                    Code::E0262,
                    location,
                    "Value is not an integer constant".to_owned(),
                );
                return;
            }
            // FTEF-19.41: a `virtual` function member is `Struct::member`, a function with no
            // body unless one follows.
            if member.is_virtual && member.ty.is_function() && !member.initialized {
                let owner = st.name.as_deref().unwrap_or("<struct>");
                let message = format!("function {owner}::{} has no body", member.name);
                self.report(Code::E0211, location, message);
                return;
            }
            if member
                .bits
                .is_some_and(|width| width.is_some_and(|w| w > 64))
            {
                self.report(Code::E0250, location, "too many bits".to_owned());
                return;
            }
            if self.incomplete_struct(&member.ty, st.name.as_deref()) {
                let message = format!("type {} not fully defined yet", member.ty.fteqcc_name());
                self.report(Code::E0255, location, message);
                return;
            }
            if member.bits.is_some() && !self.bitfield_type(&member.ty) {
                self.report(
                    Code::E0250,
                    location,
                    "bitfields must be integer types".to_owned(),
                );
                return;
            }
        }
    }

    /// Whether `ty` is a struct or union a member cannot hold yet, one with no body so far or
    /// the one being defined (`type == newt || ((type->type == ev_struct || ev_union) &&
    /// !type->size)`, FTEF-6.7).
    pub(in crate::resolve) fn incomplete_struct(&self, ty: &Type, own: Option<&str>) -> bool {
        let Type::Named(name) = ty else {
            return false;
        };
        let tagged = name
            .strip_prefix("struct ")
            .or_else(|| name.strip_prefix("union "))
            .map(str::trim);
        let bare = tagged.unwrap_or(name);
        if bare == "{}" {
            // A body with no tag, `struct { ... }`, has its size.
            return false;
        }
        let key = self.type_key(bare);
        if own.is_some_and(|own| self.type_key(own) == key) {
            return true;
        }
        match self.struct_defs.get(&key) {
            Some(def) => !def.defined,
            None => tagged.is_some() && !self.other_sized_type_named(bare),
        }
    }

    /// The struct or union a type name names, `struct S` or `S`, when it is one: written with
    /// its keyword, with or without a body (`QCC_PR_ParseStruct` returns the type of a tag,
    /// FTEF-6.8), or the name of one declared.
    pub(in crate::resolve) fn struct_type_name<'n>(&self, name: &'n str) -> Option<&'n str> {
        let tagged = name
            .strip_prefix("struct ")
            .or_else(|| name.strip_prefix("union "));
        let bare = tagged.unwrap_or(name);
        if tagged.is_some() {
            // `QCC_TypeForName(tname)`: a name that is another sized type is that type, so
            // `struct float` is `float`.
            return (bare.starts_with('{') || !self.other_sized_type_named(bare)).then_some(bare);
        }
        self.struct_defs
            .contains_key(&self.type_key(bare))
            .then_some(bare)
    }

    /// Whether `ty` is a struct or union (FTEF-9.11, FTEF-6.12).
    pub(in crate::resolve) fn is_struct_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(n) if self.struct_type_name(n).is_some())
    }

    /// The struct a member access is made on, when a name's type is one (FTEF-6.5), and whether
    /// more than one of its own members (the nearest struct with one) is named `name`, or by it
    /// in any case under `-Fcaseinsens`: `QCC_PR_FindStructMember`'s "multiple members found
    /// matching" (FTEF-6.11).
    pub(in crate::resolve) fn struct_matches(
        &self,
        scope: Option<Scope>,
        base: &MemberBase,
        name: &str,
    ) -> StructMatch {
        if !self.profile.type_declarations() {
            return StructMatch::NotStruct;
        }
        let MemberBase::Name(base) = base else {
            return StructMatch::NotStruct;
        };
        let Some(def) = (match scope {
            Some(scope) => self.lookup_in_scope(scope, base),
            None => self.lookup_used_global(base),
        }) else {
            return StructMatch::NotStruct;
        };
        let Some(Type::Named(type_name)) = self.index.def(def).map(|d| self.resolved(&d.ty)) else {
            return StructMatch::NotStruct;
        };
        let bare = type_name
            .strip_prefix("struct ")
            .or_else(|| type_name.strip_prefix("union "))
            .unwrap_or(&type_name);
        let mut current = Some(self.type_key(bare));
        let mut anonymous = false;
        for _ in 0..MAX_CLASS_DEPTH {
            let Some(key) = current else {
                break;
            };
            let Some(def) = self.struct_defs.get(&key) else {
                return StructMatch::NotStruct;
            };
            anonymous |= def.members.iter().any(|m| m.name.is_empty());
            // A member declared again with its name, type and size is one
            // (`oldparm && oldparm->arraysize == arraysize && !typecmp_lax`).
            let mut distinct: Vec<(&str, &Type, &[String])> = Vec::new();
            for m in &def.members {
                let same_name = if self.profile.case_insensitive() {
                    m.name.eq_ignore_ascii_case(name)
                } else {
                    m.name == name
                };
                let entry = (m.name.as_str(), &m.ty, m.array.as_slice());
                if same_name && !distinct.contains(&entry) {
                    distinct.push(entry);
                }
            }
            match distinct.len() {
                0 => {}
                1 => return StructMatch::One,
                _ => return StructMatch::Several(bare.to_owned()),
            }
            current = def.parent.clone();
        }
        if anonymous
            || !self
                .struct_defs
                .get(&self.type_key(bare))
                .is_some_and(|d| d.defined)
        {
            // An anonymous member's names are not followed (FTEF-6.11), and a struct with no
            // body has no members to miss.
            return StructMatch::One;
        }
        StructMatch::None(bare.to_owned())
    }
}

impl Resolver<'_> {
    /// The type of the member `name` of the struct or union `ty`, found in it or its parents
    /// as `QCC_PR_FindStructMember` finds it (FTEF-6.11), and whether the member is an array.
    pub(in crate::resolve) fn struct_member(
        &self,
        ty: &Type,
        name: &str,
    ) -> Option<StructMemberFacts> {
        let Type::Named(type_name) = self.resolved(ty) else {
            return None;
        };
        let bare = self.struct_type_name(&type_name)?;
        let mut key = Some(self.type_key(bare));
        for _ in 0..MAX_CLASS_DEPTH {
            let st = self.struct_defs.get(key.as_deref()?)?;
            if let Some(member) = st
                .members
                .iter()
                .find(|m| self.same_member_name(&m.name, name))
            {
                let bits = member.bits.flatten();
                return Some(StructMemberFacts {
                    ty: member.ty.clone(),
                    array: !member.array.is_empty(),
                    bits,
                    position: self.member_position(ty, name),
                });
            }
            key.clone_from(&st.parent);
        }
        None
    }

    /// Whether a struct member named `member` is the one an access names `name`, in any case
    /// under `-Fcaseinsens` (FTEF-6.11).
    pub(in crate::resolve) fn same_member_name(&self, member: &str, name: &str) -> bool {
        if self.profile.case_insensitive() {
            member.eq_ignore_ascii_case(name)
        } else {
            member == name
        }
    }
}
