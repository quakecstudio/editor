// SPDX-License-Identifier: GPL-3.0-or-later

//! A watch expression's value (WORLD-9.5, BRIDGE-4.9 "watch"): a constant, a global or a local
//! of the frame's function, or a field path such as `self.owner.health`.

use std::fmt;

use qc_progs::Progs;
use qc_types::{Etype, Slot};

use super::World;
use super::inspect::{QcValue, named};
use super::state::{RawState, typed_value};

/// Why an expression has no value (WORLD-9.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// The text is not a constant or a path of names.
    Syntax,
    /// No local, global or field has this name.
    Unknown(String),
    /// A field is read from what is not an entity.
    NotAnEntity(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax => f.write_str("not a constant or a path of names"),
            Self::Unknown(name) => write!(f, "nothing is named {name}"),
            Self::NotAnEntity(name) => write!(f, "{name} is not an entity"),
        }
    }
}

impl World {
    /// The value of `expression` (WORLD-9.5): a number, a vector `'x y z'` or a string `"..."`;
    /// or a name, a local of `function` (one of its definitions within its locals) before a
    /// global, then any number of `.field`s, each read from the entity before it.
    ///
    /// # Errors
    ///
    /// [`EvalError`] for text that is neither, a name nothing has, or a field read from what
    /// is not an entity.
    pub fn evaluate(
        &self,
        progs: &Progs<'_>,
        function: Option<i32>,
        expression: &str,
    ) -> Result<QcValue, EvalError> {
        let text = expression.trim();
        if let Some(value) = constant(text) {
            return value;
        }
        let mut names = text.split('.');
        let first = names
            .next()
            .filter(|n| is_name(n))
            .ok_or(EvalError::Syntax)?;
        let mut value = self.variable(progs, function, first)?;
        let mut path = first.to_owned();
        for field in names {
            if !is_name(field) {
                return Err(EvalError::Syntax);
            }
            let QcValue::Entity(edict) = value else {
                return Err(EvalError::NotAnEntity(path));
            };
            let def = progs
                .ed_find_field(field.as_bytes())
                .ok_or_else(|| EvalError::Unknown(field.to_owned()))?;
            value = self
                .field_value(progs, edict, def)
                .map(|named| named.value)
                .ok_or_else(|| EvalError::Unknown(field.to_owned()))?;
            path.push('.');
            path.push_str(field);
        }
        Ok(value)
    }

    /// The local of `function` named `name`, else the global.
    fn variable(
        &self,
        progs: &Progs<'_>,
        function: Option<i32>,
        name: &str,
    ) -> Result<QcValue, EvalError> {
        let range = function
            .and_then(|f| usize::try_from(f).ok())
            .and_then(|f| progs.functions().get(f))
            .map(|f| (f.parm_start, f.parm_start.saturating_add(f.locals)));
        let mut global = None;
        for def in progs.global_defs().iter() {
            let Some((def_name, etype, first)) = named(progs, def) else {
                continue;
            };
            if def_name != name {
                continue;
            }
            let ofs = i32::try_from(first).unwrap_or(i32::MAX);
            let local = range.is_some_and(|(start, end)| ofs >= start && ofs < end);
            if local {
                return Ok(self.global_value(progs, etype, first));
            }
            // Another function's local is not a global.
            let anyones_local = progs
                .functions()
                .iter()
                .any(|f| ofs >= f.parm_start && ofs < f.parm_start.saturating_add(f.locals));
            if global.is_none() && !anyones_local {
                global = Some((etype, first));
            }
        }
        global
            .map(|(etype, first)| self.global_value(progs, etype, first))
            .ok_or_else(|| EvalError::Unknown(name.to_owned()))
    }

    fn global_value(&self, progs: &Progs<'_>, etype: Etype, first: usize) -> QcValue {
        let slots: Vec<Slot> = (0..etype.slot_count())
            .map(|i| RawState::global(self, first.saturating_add(i)))
            .collect();
        typed_value(progs, self, etype, &slots)
    }
}

/// A name: a letter or `_`, then letters, digits and `_`.
fn is_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A constant's value, `None` for text that is not one.
fn constant(text: &str) -> Option<Result<QcValue, EvalError>> {
    let first = text.bytes().next()?;
    if first.is_ascii_digit() || first == b'-' || first == b'.' {
        return Some(
            text.parse()
                .map(QcValue::Float)
                .map_err(|_| EvalError::Syntax),
        );
    }
    if let Some(inner) = text.strip_prefix('\'') {
        let inner = inner.strip_suffix('\'')?;
        let parts: Vec<f32> = inner
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let vector = <[f32; 3]>::try_from(parts).ok()?;
        return Some(Ok(QcValue::Vector(vector)));
    }
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(Ok(QcValue::String(inner.as_bytes().to_vec())))
}
