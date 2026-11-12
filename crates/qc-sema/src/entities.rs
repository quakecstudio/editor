// SPDX-License-Identifier: GPL-3.0-or-later

//! The entity classes a QuakeC module defines, read from its spawn functions (P5-03, MAP-1),
//! and their export as an FGD file for other map editors.
//!
//! The engine spawns a map's entity by calling the function its `classname` names, with `self`
//! the entity, after setting each key that names a field (EDICT-8.2, EDICT-8.5, EDICT-8.6). So a
//! class is a `void()` function with a body, and its keys are the fields its code reads from
//! `self`. A function is taken as a class when a `/*QUAKED name ...*/` comment documents it,
//! the convention id's code and the editors after it use, or when nothing in the module names
//! it and its code reads or writes a field of `self`: only the engine calls it then, by name.
//! The engine's other entry points (EDICT-9.1) are left out.
//!
//! What each class reads comes from the intermediate layer (`crate::ir`), so it is the code as
//! the compiler generates it, through its own body, the functions it calls and the functions it
//! stores in a field of `self` (its `use`, `touch` or `think`):
//!
//! - a key is a field loaded from `self`, with the field's type;
//! - a key's default is a constant the class's own body stores into the field after loading it,
//!   `if (!self.wait) self.wait = 1;`;
//! - a spawnflag is a constant `self.spawnflags` is masked with, named by the constant's
//!   definition or else by the comment's flag at that bit;
//! - the model is the string the class passes to `setmodel`, and the size the vectors it passes
//!   to `setsize`; a class that passes `self.model` to `setmodel` is a brush entity.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crate::ir::{BodyStatement, ImmediateKind, Name};
use crate::{Db, DefId, DefKind, ModuleIndex, ModuleInput, SimpleType, Type, TypedBody};
use qc_preproc::Op;

/// The functions the engine calls by name that are not spawn functions (EDICT-9.1).
const ENTRY_POINTS: [&str; 9] = [
    "StartFrame",
    "PlayerPreThink",
    "PlayerPostThink",
    "SetNewParms",
    "ClientConnect",
    "PutClientInServer",
    "ClientKill",
    "SetChangeParms",
    "ClientDisconnect",
];

/// The global the engine sets to the entity it spawns (EDICT-8.6).
const SELF: &str = "self";
/// The field a map's flags are in (EDICT-8.4).
const SPAWNFLAGS: &str = "spawnflags";
/// The field the engine reads the class from (EDICT-8.5), which every entity has.
const CLASSNAME: &str = "classname";
/// The field a brush entity's model is in.
const MODEL: &str = "model";
/// The builtin that sets a model.
const SETMODEL: &str = "setmodel";
/// The builtin that sets a size.
const SETSIZE: &str = "setsize";

/// One entity class: a spawn function and what it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityClass {
    /// The classname, the spawn function's name (EDICT-8.5).
    pub name: String,
    /// The spawn function.
    pub function: DefId,
    /// Why the function is taken as a spawn function.
    pub found: Found,
    /// Whether it is a point or a brush entity.
    pub shape: Shape,
    /// The colour its comment gives, each component 0 to 1.
    pub color: Option<[f32; 3]>,
    /// The box its comment gives, or else the one it passes to `setsize`.
    pub size: Option<[[f32; 3]; 2]>,
    /// The model it passes to `setmodel`.
    pub model: Option<String>,
    /// Its comment's text after the first line, each line trimmed.
    pub doc: Vec<String>,
    /// The keys it reads, by name; `spawnflags` is [`Self::spawnflags`].
    pub keys: Vec<Key>,
    /// The spawnflags it tests or its comment names, by value.
    pub spawnflags: Vec<SpawnFlag>,
}

/// Why a function is taken as a spawn function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// A `/*QUAKED*/` comment documents it.
    Documented,
    /// Nothing in the module names it, and it reads or writes a field of `self`.
    Unreferenced,
}

/// Whether a class is placed at a point or made of the brushes it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A point entity.
    Point,
    /// A brush entity: its comment has `?` for a size, or it sets its model from `self.model`.
    Brush,
}

/// A key a class reads: a field of `self`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// The field's name.
    pub name: String,
    /// The field's type.
    pub ty: KeyType,
    /// The value the class gives it when the map does not, as a map writes it.
    pub default: Option<String>,
}

/// The type of a key, the field's (EDICT-8.2 converts the value by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyType {
    /// A float.
    Float,
    /// A vector.
    Vector,
    /// A string.
    String,
    /// An entity, by its number.
    Entity,
    /// A function, by its name.
    Function,
    /// Any other type.
    Other,
}

/// A spawnflag a class tests or its comment names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnFlag {
    /// The flag's value, a power of two.
    pub value: u32,
    /// Its name: the constant the class masks with, or else its comment's word for it.
    pub name: Option<String>,
}

/// A `/*QUAKED*/` comment.
#[derive(Clone, Debug, Default, PartialEq)]
struct Quaked {
    color: Option<[f32; 3]>,
    size: Option<[[f32; 3]; 2]>,
    brush: bool,
    flags: Vec<String>,
    doc: Vec<String>,
}

impl Db {
    /// The entity classes of the module `input` names, by classname.
    pub fn entity_classes(&mut self, input: &ModuleInput) -> Vec<EntityClass> {
        let analysis = self.analyze_module(input);
        let mut comments = BTreeMap::new();
        let mut constants = HashMap::new();
        for &file in &input.compiled {
            for (name, comment) in quaked_comments(self.file_text(file)) {
                comments.entry(name).or_insert(comment);
            }
        }
        for def in analysis.index.defs() {
            if def.kind != DefKind::Global || !def.initialized {
                continue;
            }
            let text = self.file_text(def.location.file);
            if let Some(value) = text.get(def.location.span.end..).and_then(literal_after) {
                constants.entry(def.name.clone()).or_insert(value);
            }
        }
        let vectors = analysis
            .index
            .defs()
            .iter()
            .filter(|def| key_type(&def.ty) == KeyType::Vector)
            .map(|def| def.name.clone())
            .collect();
        Classes {
            index: &analysis.index,
            constants: &constants,
            vectors,
        }
        .find(&comments)
    }
}

impl Db {
    /// The keys whose value names entities to fire, read from the module's code (MAP-8.8): a
    /// field passed to `find` as the value to match against `targetname`, as
    /// `find (world, targetname, self.target)` is, or compared with `targetname` by `==` or
    /// `!=`. Sorted, each once.
    pub fn targeting_keys(&mut self, input: &ModuleInput) -> Vec<String> {
        let analysis = self.analyze_module(input);
        let index = &analysis.index;
        let mut keys = std::collections::BTreeSet::new();
        for (id, def) in index.definitions() {
            if def.kind != DefKind::Function {
                continue;
            }
            let Some(body) = index.body(id) else {
                continue;
            };
            let code = body.generated();
            let field = |name: Option<Name>| -> Option<&str> {
                let Name::Made(made) = name? else {
                    return None;
                };
                let statement = code.get(usize::try_from(made).ok()?)?;
                if !is_load(statement.op) {
                    return None;
                }
                let Some(Name::Def(field, 0)) = statement.b else {
                    return None;
                };
                index
                    .def(field)
                    .filter(|def| def.kind == DefKind::Field)
                    .map(|def| def.name.as_str())
            };
            let mut params: [Option<Name>; FIND_ARGS] = [None; FIND_ARGS];
            for statement in code {
                if let Some(Name::Parameter(slot)) = statement.b
                    && is_store(statement.op)
                {
                    if let Some(param) = params.get_mut(usize::from(slot)) {
                        *param = statement.a;
                    }
                    continue;
                }
                if is_call(statement.op) {
                    let callee = match statement.a {
                        Some(Name::Def(id, 0)) => index.def(id).map(|d| d.name.as_str()),
                        _ => None,
                    };
                    let [_, against, value] = params;
                    let against = match against {
                        Some(Name::Def(id, 0)) => index.def(id).map(|d| d.name.as_str()),
                        _ => None,
                    };
                    if callee == Some(FIND)
                        && against == Some(TARGETNAME)
                        && let Some(key) = field(value)
                    {
                        keys.insert(key.to_owned());
                    }
                    params = [None; FIND_ARGS];
                    continue;
                }
                if matches!(statement.op, Op::EqS | Op::NeS) {
                    match (field(statement.a), field(statement.b)) {
                        (Some(TARGETNAME), Some(other)) | (Some(other), Some(TARGETNAME))
                            if other != TARGETNAME =>
                        {
                            keys.insert(other.to_owned());
                        }
                        _ => {}
                    }
                }
            }
        }
        keys.into_iter().collect()
    }
}

/// The builtin that finds an entity by a field's value, `find (start, field, match)`.
const FIND: &str = "find";
/// The field a targeting key's value is matched against.
const TARGETNAME: &str = "targetname";
/// The parameters `find` takes.
const FIND_ARGS: usize = 3;

/// What reading the classes needs.
struct Classes<'a> {
    index: &'a ModuleIndex,
    /// Each name's literal initialiser, the first declared.
    constants: &'a HashMap<String, Arg>,
    /// The names of the vector fields.
    vectors: HashSet<String>,
}

impl Classes<'_> {
    /// Every class, by classname.
    fn find(&self, comments: &BTreeMap<String, Quaked>) -> Vec<EntityClass> {
        let named: HashSet<DefId> = self.index.refs().iter().map(|r| r.def).collect();
        let mut classes = Vec::new();
        let mut seen = HashSet::new();
        for (id, def) in self.index.definitions() {
            if def.kind != DefKind::Function || !is_spawn_type(&def.ty) || !seen.insert(&def.name) {
                continue;
            }
            let Some(body) = self.index.body(id) else {
                continue;
            };
            let found = if comments.contains_key(&def.name) {
                Found::Documented
            } else if !named.contains(&id)
                && !ENTRY_POINTS.contains(&def.name.as_str())
                && self.touches_self(body)
            {
                Found::Unreferenced
            } else {
                continue;
            };
            let comment = comments.get(&def.name).cloned().unwrap_or_default();
            classes.push(self.class(id, &def.name, found, body, comment));
        }
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        classes
    }

    /// The class of the spawn function `id`.
    fn class(
        &self,
        id: DefId,
        name: &str,
        found: Found,
        body: &TypedBody,
        comment: Quaked,
    ) -> EntityClass {
        let mut flags: BTreeMap<u32, Option<String>> = BTreeMap::new();
        for (bit, word) in comment.flags.iter().enumerate() {
            let value = u32::try_from(bit)
                .ok()
                .and_then(|bit| 1_u32.checked_shl(bit));
            if let Some(value) = value
                && is_flag_name(word)
            {
                flags.insert(value, Some(word.clone()));
            }
        }
        let mut brush = comment.brush;
        let spawning: HashSet<DefId> = self.reachable(id, Follow::Calls).into_iter().collect();
        // Each field read while spawning or later, and written while spawning or at all.
        let mut read_spawning: BTreeMap<String, KeyType> = BTreeMap::new();
        let mut read_later: BTreeMap<String, KeyType> = BTreeMap::new();
        let mut written_spawning: HashSet<String> = HashSet::new();
        let mut written: HashSet<String> = HashSet::new();
        for function in self.reachable(id, Follow::CallsAndStored) {
            let Some(code) = self.index.body(function) else {
                continue;
            };
            let at_spawn = spawning.contains(&function);
            for statement in code.generated() {
                let builtin = self
                    .builtin_writes(statement)
                    .iter()
                    .map(|f| (*f).to_owned());
                if at_spawn {
                    written_spawning.extend(builtin.clone());
                }
                written.extend(builtin);
                let Some(field) = self.self_field(statement, true) else {
                    continue;
                };
                let (name, ty) = self.key_of(field);
                if is_load(statement.op) {
                    let reads = if at_spawn {
                        &mut read_spawning
                    } else {
                        &mut read_later
                    };
                    reads.entry(name).or_insert(ty);
                } else if statement.op == Op::Address {
                    if at_spawn {
                        written_spawning.insert(name.clone());
                    }
                    written.insert(name);
                }
            }
            for (value, constant) in self.masks(code.generated()) {
                let entry = flags.entry(value).or_insert(None);
                if constant.is_some() {
                    *entry = constant;
                }
            }
            brush |= self.sets_model_from_self(code.generated());
        }
        let mut defaults: HashMap<String, String> = HashMap::new();
        for (field, value) in self.defaults(body) {
            defaults.entry(field).or_insert(value);
        }
        // A key is a field the map's value reaches: read while spawning and not written then,
        // unless the class gives it a default, or read later and never written by the class.
        let mut keys: BTreeMap<String, KeyType> = BTreeMap::new();
        for (name, ty) in read_spawning {
            if !written_spawning.contains(&name) || defaults.contains_key(&name) {
                keys.insert(name, ty);
            }
        }
        for (name, ty) in read_later {
            if !written.contains(&name) {
                keys.entry(name).or_insert(ty);
            }
        }
        let keys: Vec<Key> = keys
            .into_iter()
            .filter(|(name, ty)| {
                name != CLASSNAME
                    && name != SPAWNFLAGS
                    && !matches!(ty, KeyType::Function | KeyType::Entity)
            })
            .map(|(name, ty)| Key {
                default: defaults.get(&name).cloned(),
                name,
                ty,
            })
            .collect();
        let calls = self.calls(body);
        let model = calls
            .iter()
            .find(|(callee, _)| callee == SETMODEL)
            .and_then(|(_, args)| args.get(1).cloned().flatten())
            .and_then(|arg| match arg {
                Arg::String(text) => Some(text),
                Arg::Float(_) | Arg::Vector(_) => None,
            });
        let size = comment.size.or_else(|| {
            calls
                .iter()
                .find(|(callee, _)| callee == SETSIZE)
                .and_then(|(_, args)| match (args.get(1), args.get(2)) {
                    (Some(Some(Arg::Vector(min))), Some(Some(Arg::Vector(max)))) => {
                        Some([*min, *max])
                    }
                    _ => None,
                })
        });
        EntityClass {
            name: name.to_owned(),
            function: id,
            found,
            shape: if brush { Shape::Brush } else { Shape::Point },
            color: comment.color,
            size: if brush { None } else { size },
            model,
            doc: comment.doc,
            keys,
            spawnflags: flags
                .into_iter()
                .map(|(value, name)| SpawnFlag { value, name })
                .collect(),
        }
    }

    /// The functions a spawn function runs: itself, what it calls and, with
    /// [`Follow::CallsAndStored`], what it stores in a field of `self`, and so on from those,
    /// each once, in the order found.
    fn reachable(&self, start: DefId, follow: Follow) -> Vec<DefId> {
        let mut found = vec![start];
        let mut seen: HashSet<DefId> = found.iter().copied().collect();
        let mut next = 0;
        while let Some(&function) = found.get(next) {
            next = next.saturating_add(1);
            let Some(body) = self.index.body(function) else {
                continue;
            };
            for statement in body.generated() {
                let callee = match statement.op {
                    op if is_call(op) => statement.a,
                    Op::StorepFnc if follow == Follow::CallsAndStored => statement.a,
                    _ => None,
                };
                if let Some(Name::Def(id, 0)) = callee
                    && self
                        .index
                        .def(id)
                        .is_some_and(|def| def.kind == DefKind::Function)
                    && seen.insert(id)
                {
                    found.push(id);
                }
            }
        }
        found
    }

    /// Whether a body reads or writes a field of `self`.
    fn touches_self(&self, body: &TypedBody) -> bool {
        body.generated().iter().any(|statement| {
            (is_load(statement.op) || statement.op == Op::Address)
                && self.self_field(statement, true).is_some()
        })
    }

    /// The field of `self` the statement names as `b`, when `applies` and its `a` is `self`.
    fn self_field(&self, statement: &BodyStatement, applies: bool) -> Option<&crate::Definition> {
        if !applies {
            return None;
        }
        let (Some(Name::Def(entity, 0)), Some(Name::Def(field, 0))) = (statement.a, statement.b)
        else {
            return None;
        };
        let def = self.index.def(field)?;
        let is_self = self.index.def(entity).is_some_and(|e| e.name == SELF);
        (is_self && def.kind == DefKind::Field).then_some(def)
    }

    /// The name of the field of `self` the statement at `index` loads or addresses.
    fn field_at(&self, code: &[BodyStatement], index: u32) -> Option<String> {
        let statement = code.get(usize::try_from(index).ok()?)?;
        let applies = is_load(statement.op) || statement.op == Op::Address;
        self.self_field(statement, applies)
            .map(|def| def.name.clone())
    }

    /// The fields a call to a builtin writes: `setsize` sets `mins`, `maxs` and `size`
    /// (BUILTIN-9.2), and `setmodel` sets `model` and the size as `setsize` does (BUILTIN-9.4).
    fn builtin_writes(&self, statement: &BodyStatement) -> &'static [&'static str] {
        let Some(Name::Def(id, 0)) = statement.a.filter(|_| is_call(statement.op)) else {
            return &[];
        };
        match self.index.def(id).map(|def| def.name.as_str()) {
            Some(SETSIZE) => &["mins", "maxs", "size"],
            Some(SETMODEL) => &["model", "mins", "maxs", "size"],
            _ => &[],
        }
    }

    /// The key a field is: a vector's component is the vector (QC-11.7).
    fn key_of(&self, field: &crate::Definition) -> (String, KeyType) {
        let parent = ["_x", "_y", "_z"]
            .iter()
            .find_map(|suffix| field.name.strip_suffix(suffix))
            .filter(|parent| self.vectors.contains(*parent));
        match parent {
            Some(parent) => (parent.to_owned(), KeyType::Vector),
            None => (field.name.clone(), key_type(&field.ty)),
        }
    }

    /// The values `self.spawnflags` is masked with, each with the constant it is named by.
    fn masks(&self, code: &[BodyStatement]) -> Vec<(u32, Option<String>)> {
        let mut found = Vec::new();
        for statement in code {
            if statement.op != Op::BitandF {
                continue;
            }
            for (left, right) in [(statement.a, statement.b), (statement.b, statement.a)] {
                let Some(Name::Made(index)) = left else {
                    continue;
                };
                if self.field_at(code, index).as_deref() != Some(SPAWNFLAGS) {
                    continue;
                }
                let flag = match right {
                    Some(Name::Immediate(value)) if value.kind == ImmediateKind::Float => {
                        flag_value(f32::from_bits(value.words[0])).map(|v| (v, None))
                    }
                    Some(Name::Def(id, 0)) => {
                        self.index
                            .def(id)
                            .and_then(|def| match self.constants.get(&def.name) {
                                Some(Arg::Float(value)) => {
                                    flag_value(*value).map(|v| (v, Some(def.name.clone())))
                                }
                                _ => None,
                            })
                    }
                    _ => None,
                };
                found.extend(flag);
            }
        }
        found
    }

    /// The constants a body stores into a field of `self` after it loads that field: the
    /// values it gives a key the map leaves out, each field's first.
    fn defaults(&self, body: &TypedBody) -> Vec<(String, String)> {
        let code = body.generated();
        let mut loaded = HashSet::new();
        let mut found = Vec::new();
        for statement in code {
            if is_load(statement.op) {
                if let Some(field) = self.self_field(statement, true) {
                    loaded.insert(field.name.clone());
                }
                continue;
            }
            if !is_store_pointer(statement.op) {
                continue;
            }
            let Some(Name::Made(target)) = statement.b else {
                continue;
            };
            let Some(field) = self.field_at(code, target) else {
                continue;
            };
            if !loaded.contains(&field) {
                continue;
            }
            if let Some(value) = self.value(statement.a, body) {
                found.push((field, value.text()));
            }
        }
        found
    }

    /// The constant an operand of `body` names.
    fn value(&self, name: Option<Name>, body: &TypedBody) -> Option<Arg> {
        match name? {
            Name::Immediate(value) => match value.kind {
                ImmediateKind::Float => Some(Arg::Float(f32::from_bits(value.words[0]))),
                ImmediateKind::Vector => Some(Arg::Vector(value.words.map(f32::from_bits))),
                ImmediateKind::Integer => None,
            },
            Name::String(index) => body
                .string(index)
                .map(|bytes| Arg::String(String::from_utf8_lossy(bytes).into_owned())),
            Name::Def(id, 0) => self.constants.get(&self.index.def(id)?.name).cloned(),
            _ => None,
        }
    }

    /// Each call a body makes to a function by name, with the constants it passes.
    fn calls(&self, body: &TypedBody) -> Vec<(String, Vec<Option<Arg>>)> {
        let mut args: Vec<Option<Arg>> = Vec::new();
        let mut found = Vec::new();
        for statement in body.generated() {
            if let Some(Name::Parameter(slot)) = statement.b
                && is_store(statement.op)
            {
                let slot = usize::from(slot);
                if args.len() <= slot {
                    args.resize(slot.saturating_add(1), None);
                }
                let value = self.value(statement.a, body);
                if let Some(arg) = args.get_mut(slot) {
                    *arg = value;
                }
                continue;
            }
            if is_call(statement.op) {
                if let Some(Name::Def(id, 0)) = statement.a
                    && let Some(def) = self.index.def(id)
                {
                    found.push((def.name.clone(), std::mem::take(&mut args)));
                }
                args.clear();
            }
        }
        found
    }

    /// Whether a body passes `self.model` to `setmodel`: a brush entity's model.
    fn sets_model_from_self(&self, code: &[BodyStatement]) -> bool {
        let mut model = false;
        for statement in code {
            if is_store(statement.op) && statement.b == Some(Name::Parameter(1)) {
                model = matches!(statement.a, Some(Name::Made(index))
                    if self.field_at(code, index).as_deref() == Some(MODEL));
            } else if is_call(statement.op) {
                let callee = match statement.a {
                    Some(Name::Def(id, 0)) => self.index.def(id).map(|d| d.name.as_str()),
                    _ => None,
                };
                if model && callee == Some(SETMODEL) {
                    return true;
                }
                model = false;
            }
        }
        false
    }
}

/// Which functions [`Classes::reachable`] follows from a body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Follow {
    /// The functions it calls: what runs while the entity spawns.
    Calls,
    /// Those, and the functions it stores in a field, its `think`, `use` or `touch`.
    CallsAndStored,
}

/// A constant passed to a function or stored.
#[derive(Clone, Debug, PartialEq)]
enum Arg {
    /// A float.
    Float(f32),
    /// A vector.
    Vector([f32; 3]),
    /// A string.
    String(String),
}

impl Arg {
    /// The value as a map writes it.
    fn text(&self) -> String {
        match self {
            Self::Float(x) => format!("{x}"),
            Self::Vector([x, y, z]) => format!("{x} {y} {z}"),
            Self::String(text) => text.clone(),
        }
    }
}

/// The literal a global's initialiser is, read from the source text after its name: `= 32`,
/// `= '0 0 1'` or `= "text"`. Vanilla lowering keeps no literal (QCD-3.6), and a spawnflag's or
/// a size's constant is one of these.
fn literal_after(text: &[u8]) -> Option<Arg> {
    let text = String::from_utf8_lossy(text.get(..text.len().min(256))?);
    let value = text.trim_start().strip_prefix('=')?.trim_start();
    let end = value.find([';', ',']).unwrap_or(value.len());
    let value = value.get(..end)?.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        let numbers: Vec<f32> = inner
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let [x, y, z] = numbers.as_slice() else {
            return None;
        };
        return Some(Arg::Vector([*x, *y, *z]));
    }
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return Some(Arg::String(inner.to_owned()));
    }
    value.parse().ok().map(Arg::Float)
}

/// Whether `ty` is `void()`, the type the engine calls a spawn function as (EDICT-8.6).
fn is_spawn_type(ty: &Type) -> bool {
    matches!(ty, Type::Function(f)
        if f.params.is_empty() && !f.variadic && f.ret == Type::Simple(SimpleType::Void))
}

/// A field's key type.
fn key_type(ty: &Type) -> KeyType {
    let Type::Field(inner) = ty else {
        return KeyType::Other;
    };
    match &**inner {
        Type::Simple(SimpleType::Float) => KeyType::Float,
        Type::Simple(SimpleType::Vector) => KeyType::Vector,
        Type::Simple(SimpleType::String) => KeyType::String,
        Type::Simple(SimpleType::Entity) => KeyType::Entity,
        Type::Function(_) => KeyType::Function,
        _ => KeyType::Other,
    }
}

/// Whether `op` loads a field.
const fn is_load(op: Op) -> bool {
    matches!(
        op,
        Op::LoadF | Op::LoadV | Op::LoadS | Op::LoadEnt | Op::LoadFld | Op::LoadFnc
    )
}

/// Whether `op` stores through a field's address.
const fn is_store_pointer(op: Op) -> bool {
    matches!(
        op,
        Op::StorepF | Op::StorepV | Op::StorepS | Op::StorepEnt | Op::StorepFld | Op::StorepFnc
    )
}

/// Whether `op` stores into a global or a parameter.
const fn is_store(op: Op) -> bool {
    matches!(
        op,
        Op::StoreF | Op::StoreV | Op::StoreS | Op::StoreEnt | Op::StoreFld | Op::StoreFnc
    )
}

/// Whether `op` calls a function.
const fn is_call(op: Op) -> bool {
    matches!(
        op,
        Op::Call0
            | Op::Call1
            | Op::Call2
            | Op::Call3
            | Op::Call4
            | Op::Call5
            | Op::Call6
            | Op::Call7
            | Op::Call8
    )
}

/// The flag a float holds, when it is one power of two.
fn flag_value(value: f32) -> Option<u32> {
    (0..32_u32)
        .filter_map(|bit| 1_u32.checked_shl(bit))
        .find(|flag| {
            #[expect(
                clippy::float_cmp,
                reason = "a spawnflag is a float holding exactly a power of two"
            )]
            let same = f64::from(value) == f64::from(*flag);
            same
        })
}

/// Whether a comment's flag word names a flag rather than holding its place.
fn is_flag_name(word: &str) -> bool {
    word.chars().any(|c| c.is_ascii_alphabetic()) && !word.eq_ignore_ascii_case("x")
}

/// Every `/*QUAKED name (r g b) (min) (max) FLAG...` comment in `text`, by name; space may
/// come between `/*` and `QUAKED`, as the rerelease writes it.
fn quaked_comments(text: &[u8]) -> Vec<(String, Quaked)> {
    let text = String::from_utf8_lossy(text);
    let mut found = Vec::new();
    let mut rest = text.as_ref();
    while let Some(start) = rest.find("/*") {
        let after = rest.get(start.saturating_add(2)..).unwrap_or("");
        let end = after.find("*/").unwrap_or(after.len());
        let comment = after.get(..end).unwrap_or("");
        if let Some(parsed) = comment
            .trim_start()
            .strip_prefix("QUAKED")
            .and_then(parse_quaked)
        {
            found.push(parsed);
        }
        rest = after.get(end..).unwrap_or("");
    }
    found
}

/// One comment's text after `/*QUAKED`.
fn parse_quaked(comment: &str) -> Option<(String, Quaked)> {
    let mut lines = comment.lines();
    let first = lines.next()?;
    let mut words = Words(first);
    let name = words.word()?.to_owned();
    let mut quaked = Quaked {
        color: words.triple(),
        ..Quaked::default()
    };
    if words.peek_is('?') {
        words.word();
        quaked.brush = true;
    } else if let (Some(min), Some(max)) = (words.triple(), words.triple()) {
        quaked.size = Some([min, max]);
    }
    while let Some(word) = words.word() {
        quaked.flags.push(word.to_owned());
    }
    quaked.doc = lines
        .map(str::trim)
        .skip_while(|line| line.is_empty())
        .map(str::to_owned)
        .collect();
    while quaked.doc.last().is_some_and(String::is_empty) {
        quaked.doc.pop();
    }
    Some((name, quaked))
}

/// The words of a comment's first line.
struct Words<'a>(&'a str);

impl<'a> Words<'a> {
    /// The next word, a run of characters that are not space.
    fn word(&mut self) -> Option<&'a str> {
        let text = self.0.trim_start();
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        let (word, rest) = text.split_at_checked(end)?;
        self.0 = rest;
        (!word.is_empty()).then_some(word)
    }

    /// Whether the next word starts with `c`.
    fn peek_is(&self, c: char) -> bool {
        self.0.trim_start().starts_with(c)
    }

    /// The next `(a b c)`, when it is one.
    fn triple(&mut self) -> Option<[f32; 3]> {
        let text = self.0.trim_start();
        let inner = text.strip_prefix('(')?;
        let close = inner.find(')')?;
        let numbers: Vec<f32> = inner
            .get(..close)?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let [a, b, c] = numbers.as_slice() else {
            return None;
        };
        self.0 = inner.get(close.saturating_add(1)..)?;
        Some([*a, *b, *c])
    }
}

/// The classes as an FGD file, the format Worldcraft, Hammer, TrenchBroom and J.A.C.K. read
/// entity definitions from: write it with `to_string` or `{}`.
#[derive(Clone, Copy, Debug)]
pub struct Fgd<'a>(pub &'a [EntityClass]);

impl fmt::Display for Fgd<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (n, class) in self.0.iter().enumerate() {
            if n > 0 {
                out.write_str("\n")?;
            }
            out.write_str(match class.shape {
                Shape::Point => "@PointClass",
                Shape::Brush => "@SolidClass",
            })?;
            if let Some([r, g, b]) = class.color {
                write!(out, " color({} {} {})", byte(r), byte(g), byte(b))?;
            }
            if let Some([[a, b, c], [d, e, f]]) = class.size {
                write!(out, " size({a} {b} {c}, {d} {e} {f})")?;
            }
            if let Some(model) = &class.model {
                write!(out, " model({{ \"path\": \"{}\" }})", quote(model))?;
            }
            let doc = class.doc.join(" ");
            writeln!(out, " = {} : \"{}\"", class.name, quote(doc.trim()))?;
            out.write_str("[\n")?;
            if !class.spawnflags.is_empty() {
                out.write_str("\tspawnflags(flags) =\n\t[\n")?;
                for flag in &class.spawnflags {
                    match &flag.name {
                        Some(name) => {
                            writeln!(out, "\t\t{} : \"{}\" : 0", flag.value, quote(name))?
                        }
                        None => writeln!(out, "\t\t{} : \"Flag {}\" : 0", flag.value, flag.value)?,
                    }
                }
                out.write_str("\t]\n")?;
            }
            for key in &class.keys {
                let ty = match (key.name.as_str(), key.ty) {
                    ("targetname", _) => "target_source",
                    ("target" | "killtarget", _) => "target_destination",
                    (_, KeyType::Float) => "float",
                    _ => "string",
                };
                write!(out, "\t{}({ty}) : \"{}\"", key.name, key.name)?;
                if let Some(default) = &key.default {
                    write!(out, " : \"{}\"", quote(default))?;
                }
                out.write_str("\n")?;
            }
            out.write_str("]\n")?;
        }
        Ok(())
    }
}

/// A colour component from 0 to 1 as a byte.
fn byte(component: f32) -> u8 {
    let scaled = (component.clamp(0.0, 1.0) * 255.0).round();
    (0..=u8::MAX)
        .find(|b| f32::from(*b) >= scaled)
        .unwrap_or(u8::MAX)
}

/// Text for an FGD string on one line: a double quote cannot be written in one, and each run of
/// space is one space.
fn quote(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'")
}

#[cfg(test)]
mod tests;
