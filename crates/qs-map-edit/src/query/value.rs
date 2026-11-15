// SPDX-License-Identifier: GPL-3.0-or-later

//! The values a query computes with, as TrenchBroom's expression language defines them (its
//! manual, "Expression Language": "Evaluation", "Type Conversion", "Null and Undefined",
//! "Comparison Terms", "Algebraic Terms", "Vec3", "BBox", "Pattern Matching" and "Membership
//! Testing").

use core::cmp::Ordering;

/// A value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// The absence of a value: a field an object lacks, a key a map lacks.
    Undefined,
    /// An explicit empty value.
    Null,
    /// True or false.
    Bool(bool),
    /// A floating point number.
    Number(f64),
    /// A string.
    Text(String),
    /// A list of values.
    Array(Vec<Self>),
    /// Key-value pairs, each key once.
    Map(Vec<(String, Self)>),
    /// A vector.
    Vector([f64; 3]),
    /// An axis-aligned box: its least corner, then its greatest.
    Bounds([[f64; 3]; 2]),
}

/// An evaluation the language makes an error: the object it is evaluated for does not match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fails;

/// An arithmetic operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arithmetic {
    /// `+`.
    Add,
    /// `-`.
    Subtract,
    /// `*`.
    Multiply,
    /// `/`.
    Divide,
}

impl Arithmetic {
    /// The operator applied to two numbers.
    fn numbers(self, a: f64, b: f64) -> f64 {
        match self {
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Multiply => a * b,
            Self::Divide => a / b,
        }
    }
}

/// The corner names of a box, and the component names of a vector.
const MIN: &str = "min";
const MAX: &str = "max";
const COMPONENTS: [&str; 3] = ["x", "y", "z"];

impl Value {
    /// The value as a number: a boolean as 1 or 0, a string holding a number, blank as 0,
    /// null as 0.
    pub(crate) fn number(&self) -> Result<f64, Fails> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Self::Null => Ok(0.0),
            Self::Text(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return Ok(0.0);
                }
                text.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .ok_or(Fails)
            }
            Self::Undefined | Self::Array(_) | Self::Map(_) | Self::Vector(_) | Self::Bounds(_) => {
                Err(Fails)
            }
        }
    }

    /// The value as a boolean: a string is false when it is `false` or empty, a number when
    /// it is 0, null always.
    pub(crate) fn truth(&self) -> Result<bool, Fails> {
        match self {
            Self::Bool(b) => Ok(*b),
            Self::Text(text) => Ok(!(text.is_empty() || text == "false")),
            Self::Number(n) => Ok(*n != 0.0),
            Self::Null => Ok(false),
            Self::Undefined | Self::Array(_) | Self::Map(_) | Self::Vector(_) | Self::Bounds(_) => {
                Err(Fails)
            }
        }
    }

    /// The value as a vector: a vector, or a string of exactly three numbers.
    pub(crate) fn vector(&self) -> Result<[f64; 3], Fails> {
        match self {
            Self::Vector(v) => Ok(*v),
            Self::Text(text) => {
                let numbers: Vec<f64> = text
                    .split_whitespace()
                    .map(|word| word.parse::<f64>().ok().filter(|n| n.is_finite()))
                    .collect::<Option<_>>()
                    .ok_or(Fails)?;
                <[f64; 3]>::try_from(numbers).map_err(|_| Fails)
            }
            Self::Undefined
            | Self::Null
            | Self::Bool(_)
            | Self::Number(_)
            | Self::Array(_)
            | Self::Map(_)
            | Self::Bounds(_) => Err(Fails),
        }
    }

    /// How this compares with `other`: undefined below everything, then null; a boolean, a
    /// number and a string converted to the left's type, or to a number or a vector as the
    /// manual's table says; strings, arrays, maps, vectors and boxes lexicographically.
    pub(crate) fn compare(&self, other: &Self) -> Result<Ordering, Fails> {
        let numbers = |a: f64, b: f64| a.partial_cmp(&b).ok_or(Fails);
        let vectors = |a: [f64; 3], b: [f64; 3]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| x.partial_cmp(&y).ok_or(Fails))
                .find(|order| *order != Ok(Ordering::Equal))
                .unwrap_or(Ok(Ordering::Equal))
        };
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => Ok(Ordering::Equal),
            (Self::Undefined, _) => Ok(Ordering::Less),
            (_, Self::Undefined) => Ok(Ordering::Greater),
            (Self::Null, _) => Ok(Ordering::Less),
            (_, Self::Null) => Ok(Ordering::Greater),
            (Self::Bool(a), Self::Bool(_) | Self::Number(_) | Self::Text(_)) => {
                Ok(a.cmp(&other.truth()?))
            }
            (Self::Number(_) | Self::Text(_), Self::Bool(b)) => Ok(self.truth()?.cmp(b)),
            (Self::Number(_), Self::Number(_) | Self::Text(_))
            | (Self::Text(_), Self::Number(_)) => numbers(self.number()?, other.number()?),
            (Self::Text(a), Self::Text(b)) => Ok(a.cmp(b)),
            (Self::Array(a), Self::Array(b)) => {
                for (x, y) in a.iter().zip(b) {
                    let order = x.compare(y)?;
                    if order != Ordering::Equal {
                        return Ok(order);
                    }
                }
                Ok(a.len().cmp(&b.len()))
            }
            (Self::Map(a), Self::Map(b)) => {
                for ((ka, va), (kb, vb)) in a.iter().zip(b) {
                    let order = ka.cmp(kb).then(va.compare(vb)?);
                    if order != Ordering::Equal {
                        return Ok(order);
                    }
                }
                Ok(a.len().cmp(&b.len()))
            }
            (Self::Vector(a), Self::Vector(_) | Self::Text(_)) => vectors(*a, other.vector()?),
            (Self::Text(_), Self::Vector(b)) => vectors(self.vector()?, *b),
            (Self::Bounds([a_min, a_max]), Self::Bounds([b_min, b_max])) => {
                let order = vectors(*a_min, *b_min)?;
                if order == Ordering::Equal {
                    vectors(*a_max, *b_max)
                } else {
                    Ok(order)
                }
            }
            _ => Err(Fails),
        }
    }

    /// Whether this equals `other`, as `==` and `is` compare.
    pub(crate) fn equals(&self, other: &Self) -> Result<bool, Fails> {
        Ok(self.compare(other)? == Ordering::Equal)
    }

    /// `self op other`: undefined when either is; numbers, booleans and number strings as
    /// numbers; `+` joins two strings, two arrays or two maps; vectors add and subtract, and
    /// multiply and divide by a vector or a number, a string read as a vector or a number.
    pub(crate) fn arithmetic(&self, op: Arithmetic, other: &Self) -> Result<Self, Fails> {
        match (self, other) {
            (Self::Undefined, _) | (_, Self::Undefined) => Ok(Self::Undefined),
            (Self::Null, _) | (_, Self::Null) => Err(Fails),
            (Self::Text(a), Self::Text(b)) if op == Arithmetic::Add => {
                Ok(Self::Text(format!("{a}{b}")))
            }
            (Self::Array(a), Self::Array(b)) if op == Arithmetic::Add => {
                Ok(Self::Array(a.iter().chain(b).cloned().collect()))
            }
            (Self::Map(a), Self::Map(b)) if op == Arithmetic::Add => {
                let mut merged: Vec<(String, Self)> = a
                    .iter()
                    .filter(|(key, _)| !b.iter().any(|(k, _)| k == key))
                    .cloned()
                    .collect();
                merged.extend(b.iter().cloned());
                Ok(Self::Map(merged))
            }
            (Self::Vector(_), _) | (_, Self::Vector(_)) => self.vector_arithmetic(op, other),
            (Self::Bool(_) | Self::Number(_) | Self::Text(_), _) => {
                Ok(Self::Number(op.numbers(self.number()?, other.number()?)))
            }
            _ => Err(Fails),
        }
    }

    /// Arithmetic with a vector on one side.
    fn vector_arithmetic(&self, op: Arithmetic, other: &Self) -> Result<Self, Fails> {
        let scales = matches!(op, Arithmetic::Multiply | Arithmetic::Divide);
        let pair = |a: [f64; 3], b: [f64; 3]| {
            let mut out = a;
            for (o, y) in out.iter_mut().zip(b) {
                *o = op.numbers(*o, y);
            }
            Self::Vector(out)
        };
        let operand = |value: &Self| -> Result<[f64; 3], Fails> {
            value.vector().or_else(|Fails| {
                if scales {
                    value.number().map(|n| [n; COMPONENTS.len()])
                } else {
                    Err(Fails)
                }
            })
        };
        Ok(pair(operand(self)?, operand(other)?))
    }

    /// `-self`, or `+self` for [`Sign::Plus`]: undefined stays so; a boolean or
    /// a string becomes a number; a vector negates each component.
    pub(crate) fn signed(&self, sign: Sign) -> Result<Self, Fails> {
        let by = match sign {
            Sign::Plus => 1.0,
            Sign::Minus => -1.0,
        };
        match self {
            Self::Undefined => Ok(Self::Undefined),
            Self::Vector(v) => Ok(Self::Vector(v.map(|c| c * by))),
            Self::Bool(_) | Self::Number(_) | Self::Text(_) => {
                Ok(Self::Number(self.number()? * by))
            }
            Self::Null | Self::Array(_) | Self::Map(_) | Self::Bounds(_) => Err(Fails),
        }
    }

    /// `!self`: a boolean inverted, undefined kept.
    pub(crate) fn not(&self) -> Result<Self, Fails> {
        match self {
            Self::Undefined => Ok(Self::Undefined),
            Self::Bool(b) => Ok(Self::Bool(!b)),
            _ => Err(Fails),
        }
    }

    /// `self[key]`: a map's value under a string key, or undefined; an array's element at a
    /// number, counted from the end when negative; a vector's component by number or name; a
    /// box's corner by name. Undefined indexed is undefined.
    pub(crate) fn index(&self, key: &Self) -> Result<Self, Fails> {
        match (self, key) {
            (_, Self::Undefined) => Err(Fails),
            (Self::Undefined, _) => Ok(Self::Undefined),
            (Self::Map(pairs), Self::Text(key)) => Ok(pairs
                .iter()
                .find(|(k, _)| k == key)
                .map_or(Self::Undefined, |(_, v)| v.clone())),
            (Self::Array(items), Self::Number(n)) => {
                Self::place(*n, items.len()).and_then(|at| items.get(at).cloned().ok_or(Fails))
            }
            (Self::Vector(v), Self::Number(n)) => Self::place(*n, v.len())
                .and_then(|at| v.get(at).copied().map(Self::Number).ok_or(Fails)),
            (Self::Vector(v), Self::Text(name)) => COMPONENTS
                .iter()
                .zip(v)
                .find(|(c, _)| *c == name)
                .map(|(_, n)| Self::Number(*n))
                .ok_or(Fails),
            (Self::Bounds([min, max]), Self::Text(name)) => match name.as_str() {
                MIN => Ok(Self::Vector(*min)),
                MAX => Ok(Self::Vector(*max)),
                _ => Err(Fails),
            },
            _ => Err(Fails),
        }
    }

    /// The place index `n` names among `len`: rounded toward zero, from the end when
    /// negative.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "the index is rounded toward zero and checked against the length, as the \
                  manual's subscripts are"
    )]
    fn place(n: f64, len: usize) -> Result<usize, Fails> {
        let n = n.trunc();
        let at = if n < 0.0 { len as f64 + n } else { n };
        if at < 0.0 || at >= len as f64 {
            return Err(Fails);
        }
        Ok(at as usize)
    }

    /// Whether `self` matches the glob `pattern`, ignoring case: a string itself, any string
    /// of an array, any key or string value of a map. Never an error (the manual's "Pattern
    /// Matching").
    pub(crate) fn like(&self, pattern: &Self) -> bool {
        let Self::Text(pattern) = pattern else {
            return false;
        };
        let pattern = Pattern::new(pattern);
        match self {
            Self::Text(text) => pattern.finds(text),
            Self::Array(items) => items
                .iter()
                .any(|item| matches!(item, Self::Text(text) if pattern.finds(text))),
            Self::Map(pairs) => pairs.iter().any(|(key, value)| {
                pattern.finds(key) || matches!(value, Self::Text(text) if pattern.finds(text))
            }),
            _ => false,
        }
    }

    /// Whether `self` holds `value`: an array an element equal to it, a map it as a key, a box
    /// a point or a whole box. Never an error (the manual's "Membership Testing").
    pub(crate) fn contains(&self, value: &Self) -> bool {
        match (self, value) {
            (Self::Array(items), _) => items.iter().any(|item| item.equals(value) == Ok(true)),
            (Self::Map(pairs), Self::Text(key)) => pairs.iter().any(|(k, _)| k == key),
            (Self::Bounds([min, max]), Self::Vector(point)) => {
                (0..COMPONENTS.len()).all(|i| match (min.get(i), max.get(i), point.get(i)) {
                    (Some(lo), Some(hi), Some(p)) => lo <= p && p <= hi,
                    _ => false,
                })
            }
            (Self::Bounds(_), Self::Bounds([inner_min, inner_max])) => {
                self.contains(&Self::Vector(*inner_min)) && self.contains(&Self::Vector(*inner_max))
            }
            _ => false,
        }
    }

    /// The box with corners `a` and `b`, put in order component by component.
    pub(crate) fn bounds(a: [f64; 3], b: [f64; 3]) -> Self {
        let mut min = a;
        let mut max = b;
        for (lo, hi) in min.iter_mut().zip(max.iter_mut()) {
            if *hi < *lo {
                core::mem::swap(lo, hi);
            }
        }
        Self::Bounds([min, max])
    }

    /// Whether two boxes overlap.
    pub(crate) fn intersects(&self, other: &Self) -> Result<Self, Fails> {
        match (self, other) {
            (Self::Undefined, _) | (_, Self::Undefined) => Ok(Self::Undefined),
            (Self::Bounds([a_min, a_max]), Self::Bounds([b_min, b_max])) => {
                let overlaps = (0..COMPONENTS.len()).all(|i| {
                    match (a_min.get(i), a_max.get(i), b_min.get(i), b_max.get(i)) {
                        (Some(a0), Some(a1), Some(b0), Some(b1)) => a0 <= b1 && b0 <= a1,
                        _ => false,
                    }
                });
                Ok(Self::Bool(overlaps))
            }
            _ => Err(Fails),
        }
    }

    /// The distance between two vectors, each a vector or a string holding one.
    pub(crate) fn distance_to(&self, other: &Self) -> Result<Self, Fails> {
        if matches!(self, Self::Undefined) || matches!(other, Self::Undefined) {
            return Ok(Self::Undefined);
        }
        let (a, b) = (self.vector()?, other.vector()?);
        let squared: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
        Ok(Self::Number(squared.sqrt()))
    }
}

/// Which unary sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sign {
    /// `+`.
    Plus,
    /// `-`.
    Minus,
}

/// One part of a glob pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Glob {
    /// `*`: any run of characters.
    Any,
    /// `?`: one character.
    One,
    /// `%`: one digit.
    Digit,
    /// `%*`: any run of digits.
    Digits,
    /// A character itself, lower case.
    Char(char),
}

/// A glob pattern, as `like` reads one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pattern(Vec<Glob>);

impl Pattern {
    /// Reads `pattern`: `*`, `?`, `%` and `%*` are wildcards, and a backslash before one of
    /// them or another backslash makes it a character; any other backslash is one itself.
    pub(crate) fn new(pattern: &str) -> Self {
        let mut parts = Vec::new();
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            let part = match c {
                '\\' => match chars.peek() {
                    Some(&next @ ('?' | '*' | '%' | '\\')) => {
                        chars.next();
                        Glob::Char(next)
                    }
                    _ => Glob::Char('\\'),
                },
                '*' => Glob::Any,
                '?' => Glob::One,
                '%' if chars.peek() == Some(&'*') => {
                    chars.next();
                    Glob::Digits
                }
                '%' => Glob::Digit,
                other => Glob::Char(other),
            };
            parts.extend(match part {
                Glob::Char(c) => c.to_lowercase().map(Glob::Char).collect(),
                wild => vec![wild],
            });
        }
        Self(parts)
    }

    /// Whether the pattern has a wildcard; without one it is found anywhere in a text.
    fn is_wild(&self) -> bool {
        self.0.iter().any(|part| !matches!(part, Glob::Char(_)))
    }

    /// Whether `text` matches, ignoring case: wholly with a wildcard, else holding the
    /// pattern anywhere.
    pub(crate) fn finds(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        if self.is_wild() {
            return self.whole(&text);
        }
        let needle: Vec<char> = self
            .0
            .iter()
            .filter_map(|part| match part {
                Glob::Char(c) => Some(*c),
                _ => None,
            })
            .collect();
        needle.is_empty() || text.windows(needle.len()).any(|w| w == needle.as_slice())
    }

    /// Whether the whole of `text` matches, ignoring case.
    pub(crate) fn matches(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        self.whole(&text)
    }

    /// Whether `text`, lower case, matches the whole pattern: each place of `row` says
    /// whether the parts so far can end there.
    fn whole(&self, text: &[char]) -> bool {
        let mut row: Vec<bool> = core::iter::once(true)
            .chain(text.iter().map(|_| false))
            .collect();
        for part in &self.0 {
            let after = |test: &dyn Fn(char) -> bool| -> Vec<bool> {
                core::iter::once(false)
                    .chain(row.iter().zip(text).map(|(&r, &c)| r && test(c)))
                    .collect()
            };
            row = match *part {
                Glob::Char(want) => after(&|c| c == want),
                Glob::One => after(&|_| true),
                Glob::Digit => after(&|c: char| c.is_ascii_digit()),
                Glob::Any => row
                    .iter()
                    .scan(false, |seen, &r| {
                        *seen |= r;
                        Some(*seen)
                    })
                    .collect(),
                Glob::Digits => {
                    let first = row.first().copied().unwrap_or(false);
                    core::iter::once(first)
                        .chain(row.iter().skip(1).zip(text).scan(first, |prev, (&r, c)| {
                            *prev = r || (*prev && c.is_ascii_digit());
                            Some(*prev)
                        }))
                        .collect()
                }
            };
        }
        row.last().copied().unwrap_or(false)
    }
}
