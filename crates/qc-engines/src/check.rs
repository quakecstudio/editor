// SPDX-License-Identifier: GPL-3.0-or-later

//! The missing-builtin diagnostic: a call of a builtin the target engine lacks (ENGT-4).
//!
//! Implements ENGT-4 and reports QCD-7.17 (`W0311`). A module's calls are found in its syntax
//! trees and resolved through the semantic model's references (QC-12.5), so a call reaches the
//! diagnostic only when its callee name means a global builtin function. A builtin is keyed by
//! its number, the one the dialect's compiler binds (ENGT-4.12, ENGT-4.13), or, under an
//! FTEQCC dialect, by the engine name of `#0:NAME` (ENGT-4.5), `#"NAME"` or `:NAME`
//! (ENGT-4.14).

use std::collections::{HashMap, HashSet};

use qc_diag::{Code, Diagnostic, Span};
use qc_lexer::{Dialect, TokenKind};
use qc_project::ModuleKind;
use qc_sema::{DefKind, FileId, ModuleIndex};
use qc_syntax::Parse;
use qc_syntax::ast::{self, AstNode as _};

use crate::{Builtin, BuiltinKey, Engine};

/// How a module's source names a builtin: its number, or the engine name of `#0:NAME`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Number(u32),
    Name(String),
}

impl Key {
    /// The engine's entry for this key in its table for `kind`, if it has one.
    fn lookup(&self, engine: Engine, kind: ModuleKind) -> Option<Builtin> {
        match self {
            Self::Number(number) => engine.builtin(kind, BuiltinKey::Number(*number)),
            Self::Name(name) => engine
                .builtins(kind)
                .into_iter()
                .find(|b| matches!(b.key, BuiltinKey::Name(n) if n == name)),
        }
    }
}

/// A diagnostic and the file of the module it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file's index in the module's file graph, as in [`qc_sema::Location`].
    pub file: FileId,
    /// The diagnostic; its span is in that file.
    pub diagnostic: Diagnostic,
}

/// The `W0311` warnings for a module of `kind` checked against `engine` (ENGT-4.1, ENGT-4.8).
///
/// `parses` are the module's files in compilation order, `parses[i]` being [`FileId`]`(i)`,
/// parsed in `dialect`, and `index` is the module resolved from them
/// ([`qc_sema::resolve_module`]). Returns one
/// warning per call of a builtin the engine's table for `kind` lacks, in file and then source
/// order; none when the engine has no table for `kind` ([`Engine::has_table`]).
#[must_use]
pub fn missing_builtins(
    engine: Engine,
    kind: ModuleKind,
    parses: &[Parse],
    index: &ModuleIndex,
    dialect: Dialect,
) -> Vec<FileDiagnostic> {
    if !engine.has_table(kind) {
        return Vec::new();
    }
    let keys = builtin_keys(parses, dialect);
    let callees = callee_names(parses);
    let mut out = Vec::new();
    for reference in &index.refs {
        let location = reference.location;
        if !callees.contains(&(location.file, location.span)) {
            continue;
        }
        let Some(def) = index.def(reference.def) else {
            continue;
        };
        if def.kind != DefKind::Function {
            continue;
        }
        let Some(key) = keys.get(def.name.as_str()) else {
            continue;
        };
        if key.lookup(engine, kind).is_some() {
            continue;
        }
        out.push(FileDiagnostic {
            file: location.file,
            diagnostic: Diagnostic {
                code: Code::W0311,
                message: message(&def.name, key, engine, kind),
                span: location.span,
            },
        });
    }
    out.sort_by_key(|d| (d.file, d.diagnostic.span));
    out
}

/// `NAME is builtin #N, which ENGINE does not have` (ENGT-4.1), or `#0:ENGINE_NAME` for a
/// builtin looked up by name (ENGT-4.5), with the extensions of the first engine whose table
/// for `kind` lists the key with some (ENGT-4.8).
fn message(name: &str, key: &Key, engine: Engine, kind: ModuleKind) -> String {
    let shown = match key {
        Key::Number(number) => number.to_string(),
        Key::Name(engine_name) => format!("0:{engine_name}"),
    };
    let mut message = format!(
        "{name} is builtin #{shown}, which {} does not have",
        engine.name()
    );
    let extensions = Engine::ALL.into_iter().find_map(|other| {
        key.lookup(other, kind)
            .map(|b| b.extensions)
            .filter(|e| !e.is_empty())
    });
    if let Some(extensions) = extensions {
        message.push_str(" (");
        message.push_str(&extensions.join(", "));
        message.push(')');
    }
    message
}

/// Each global name's builtin key: the number of its first `#N` initialiser in compilation
/// order (ENGT-4.12), or for a name with none, the engine name of its first `#0:NAME`,
/// `#"NAME"` or `:NAME` initialiser (ENGT-4.5, ENGT-4.14).
fn builtin_keys(parses: &[Parse], dialect: Dialect) -> HashMap<String, Key> {
    let mut numbers: HashMap<String, u32> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    for parse in parses {
        let Some(tree) = parse.tree() else {
            continue;
        };
        for decl in tree.declarations() {
            for declarator in decl.declarators() {
                let Some(name) = declarator.name() else {
                    continue;
                };
                let Some(ast::Initializer::Builtin(builtin)) = declarator.initializer() else {
                    continue;
                };
                let literal = builtin.number().map(|t| t.text_lossless());
                if let Some(number) = literal.as_deref().and_then(|l| builtin_number(l, dialect)) {
                    numbers.entry(name.text_lossless()).or_insert(number);
                } else if let Some(engine_name) = engine_name(&builtin, literal.as_deref()) {
                    names.entry(name.text_lossless()).or_insert(engine_name);
                }
            }
        }
    }
    let mut keys: HashMap<String, Key> = names
        .into_iter()
        .map(|(name, engine_name)| (name, Key::Name(engine_name)))
        .collect();
    for (name, number) in numbers {
        keys.insert(name, Key::Number(number));
    }
    keys
}

/// The engine name of a builtin initialiser whose number literal, if it has one, is `literal`
/// (ENGT-4.5): the name of `#0:NAME` when the literal is zero as FTEQCC reads it, any number
/// form whose value is 0 (`0`, `00`, `.0`, `0x0`, `0i`), and the name of `#"NAME"`, `:"NAME"`
/// and `:NAME`, which have no number (ENGT-4.14).
fn engine_name(builtin: &ast::Builtin, literal: Option<&str>) -> Option<String> {
    // ENGT-4.9: `#` takes the literal as `binum` 0 whenever FTEQCC reads its value as 0; a name
    // is in the initialiser only under FTEQCC, so it is read so. ENGT-4.14: FTEQCC leaves
    // `binum` 0 when it reads the name in place of the number.
    if literal.is_some_and(|literal| fteqcc_value(literal) != Some(Value::Zero)) {
        return None;
    }
    builtin.engine_name()
}

/// The `binum` a compiler binds for a builtin number literal (ENGT-4.9, ENGT-4.12, ENGT-4.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    /// 0, the `binum` of a builtin bound by name.
    Zero,
    /// A number of at least 1.
    Number(u32),
    /// Anything else: a fraction, a negative number, one outside `int` or a literal type the
    /// compiler does not take.
    Other,
}

impl Value {
    /// The `binum` of an `int`.
    fn of_int(number: i32) -> Self {
        match u32::try_from(number) {
            Ok(0) => Self::Zero,
            Ok(number) => Self::Number(number),
            Err(_) => Self::Other,
        }
    }

    /// The `binum` of a `float` literal, `(int)pr_immediate._float` once
    /// `pr_immediate._float == (int)pr_immediate._float` holds (ENGT-4.12): the whole value,
    /// when it is from -2^31 to 2^31 - 1. Outside that range C's `(int)` is undefined, and the
    /// x86 build gives -2147483648 (VM-9.2), which fails the comparison.
    fn of_float(value: f32) -> Self {
        #[expect(
            clippy::float_cmp,
            reason = "spec ENGT-4.12: the compilers take `binum` only when `_float == (int)_float`"
        )]
        let whole = value.trunc() == value;
        let int_range = f64::from(i32::MIN)..-f64::from(i32::MIN);
        if !whole || !int_range.contains(&f64::from(value)) {
            return Self::Other;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec ENGT-4.12: a whole value in `int`'s range converts exactly"
        )]
        let number = value as i32;
        Self::of_int(number)
    }
}

/// The value of `literal` when it is one number token of `dialect`, and FTEQCC's integer
/// suffix, if it has one: the hex, octal, suffixed and fractional forms of FTEF-3.1 under an
/// FTEQCC dialect, decimals read with `atof` under the vanilla one (QC-4.1).
fn literal_value(literal: &str, dialect: Dialect) -> Option<(f32, &str)> {
    let lexed = qc_lexer::lex_with(literal.as_bytes(), dialect);
    let [token] = lexed.tokens.as_slice() else {
        return None;
    };
    let TokenKind::Float(value) = token.kind else {
        return None;
    };
    // The suffix letters `i`, `u` and `l` are no digit of any base.
    let digits = literal.trim_end_matches(['i', 'I', 'u', 'U', 'l', 'L']);
    let suffix = literal.get(digits.len()..).unwrap_or_default();
    Some((value, suffix))
}

/// The `binum` FTEQCC's `QCC_PR_ParseInitializerType_Internal` binds for `literal`
/// (ENGT-4.11, ENGT-4.13): `(int)` of a `float` literal (ENGT-4.12); an `i` literal's `int`,
/// its whole value kept to 32 bits; and no number for a `u` or `l` literal.
fn fteqcc_value(literal: &str) -> Option<Value> {
    let (value, suffix) = literal_value(literal, Dialect::Fteqcc)?;
    if suffix.is_empty() {
        return Some(Value::of_float(value));
    }
    if !suffix.eq_ignore_ascii_case("i") {
        return Some(Value::Other);
    }
    Some(Value::of_int(fteqcc_int(literal)))
}

/// `pr_immediate._int = num` for an FTEQCC integer literal: the digits in their base
/// (FTEF-3.1), times the sign, kept to 32 bits as C's conversion to `int` keeps them on every
/// build FTEQCC supports (ENGT-4.13).
fn fteqcc_int(literal: &str) -> i32 {
    let (negative, rest) = literal
        .strip_prefix('-')
        .map_or((false, literal), |rest| (true, rest));
    let (base, digits) = if let Some(hex) = rest.strip_prefix("0x").or(rest.strip_prefix("0X")) {
        (16, hex)
    } else if let Some(octal) = rest.strip_prefix('0') {
        (8, octal)
    } else {
        (10, rest)
    };
    let mut num: u64 = 0;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        num = num
            .wrapping_mul(u64::from(base))
            .wrapping_add(u64::from(digit));
    }
    if negative {
        num = num.wrapping_neg();
    }
    let [a, b, c, d, ..] = num.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// The number of a builtin number literal in `dialect` (ENGT-4.12): the `binum` the dialect's
/// compiler binds, when it is at least 1, `qcc`'s `(int)` of the literal's `float` value under
/// the vanilla dialect and FTEQCC's of ENGT-4.13 under an FTEQCC dialect.
pub(crate) fn builtin_number(text: &str, dialect: Dialect) -> Option<u32> {
    let value = if dialect.has_fteqcc_extensions() {
        fteqcc_value(text)?
    } else {
        Value::of_float(literal_value(text, dialect)?.0)
    };
    match value {
        Value::Number(number) => Some(number),
        Value::Zero | Value::Other => None,
    }
}

/// The file and span of every name that is the callee of a call (ENGT-4.2).
fn callee_names(parses: &[Parse]) -> HashSet<(FileId, Span)> {
    let mut callees = HashSet::new();
    for (index, parse) in parses.iter().enumerate() {
        let Some(tree) = parse.tree() else {
            continue;
        };
        for node in tree.syntax().descendants() {
            let Some(call) = ast::CallExpr::cast(node) else {
                continue;
            };
            if let Some(ast::Expr::Name(name)) = call.callee()
                && let Some(token) = name.name()
            {
                callees.insert((FileId(index), token.span()));
            }
        }
    }
    callees
}
