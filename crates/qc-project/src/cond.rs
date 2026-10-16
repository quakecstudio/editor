// SPDX-License-Identifier: GPL-3.0-or-later

//! `#if` and `#elif` expressions (PRJ-5.5).

use std::collections::HashMap;

/// Deepest nesting of `!` and parentheses an expression may have; a deeper one is false, so
/// no input exhausts the stack.
const MAX_DEPTH: u32 = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Num(i64),
    Name(String),
    LParen,
    RParen,
    Not,
    AndAnd,
    OrOr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// The decimal integer at the start of `text`, as C's `atoi` reads it: leading blanks, an
/// optional sign, then digits; 0 when there are none. Saturates instead of overflowing.
pub(crate) fn atoi(text: &str) -> i64 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..).unwrap_or_default()),
        Some(b'+') => (false, text.get(1..).unwrap_or_default()),
        _ => (false, text),
    };
    let mut value = 0_i64;
    for byte in digits.bytes().take_while(u8::is_ascii_digit) {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte.wrapping_sub(b'0')));
    }
    if negative {
        value.saturating_neg()
    } else {
        value
    }
}

fn tokens(expr: &str) -> Option<Vec<Tok>> {
    let bytes = expr.as_bytes();
    let mut out = Vec::new();
    let mut i = 0_usize;
    while let Some(&byte) = bytes.get(i) {
        let next = bytes.get(i.saturating_add(1)).copied();
        let (tok, len) = match (byte, next) {
            (b' ' | b'\t' | b'\r' | b'\n', _) => {
                i = i.saturating_add(1);
                continue;
            }
            (b'/', Some(b'/' | b'*')) => break,
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'&', Some(b'&')) => (Tok::AndAnd, 2),
            (b'|', Some(b'|')) => (Tok::OrOr, 2),
            (b'=', Some(b'=')) => (Tok::Eq, 2),
            (b'!', Some(b'=')) => (Tok::Ne, 2),
            (b'!', _) => (Tok::Not, 1),
            (b'<', Some(b'=')) => (Tok::Le, 2),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', Some(b'=')) => (Tok::Ge, 2),
            (b'>', _) => (Tok::Gt, 1),
            _ if byte.is_ascii_alphanumeric() || byte == b'_' => {
                let len = bytes
                    .get(i..)?
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
                let word = expr.get(i..i.checked_add(len)?)?;
                let tok = if byte.is_ascii_digit() {
                    Tok::Num(atoi(word))
                } else {
                    Tok::Name(word.to_owned())
                };
                (tok, len)
            }
            _ => return None,
        };
        out.push(tok);
        i = i.saturating_add(len);
    }
    Some(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    macros: &'a HashMap<String, String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos = self.pos.saturating_add(1);
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<Tok> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos = self.pos.saturating_add(1);
        tok
    }

    fn or(&mut self, depth: u32) -> Option<i64> {
        let mut value = self.and(depth)?;
        while self.eat(&Tok::OrOr) {
            let rhs = self.and(depth)?;
            value = i64::from(value != 0 || rhs != 0);
        }
        Some(value)
    }

    fn and(&mut self, depth: u32) -> Option<i64> {
        let mut value = self.equality(depth)?;
        while self.eat(&Tok::AndAnd) {
            let rhs = self.equality(depth)?;
            value = i64::from(value != 0 && rhs != 0);
        }
        Some(value)
    }

    fn equality(&mut self, depth: u32) -> Option<i64> {
        let mut value = self.relation(depth)?;
        loop {
            if self.eat(&Tok::Eq) {
                value = i64::from(value == self.relation(depth)?);
            } else if self.eat(&Tok::Ne) {
                value = i64::from(value != self.relation(depth)?);
            } else {
                return Some(value);
            }
        }
    }

    fn relation(&mut self, depth: u32) -> Option<i64> {
        let mut value = self.unary(depth)?;
        loop {
            if self.eat(&Tok::Lt) {
                value = i64::from(value < self.unary(depth)?);
            } else if self.eat(&Tok::Le) {
                value = i64::from(value <= self.unary(depth)?);
            } else if self.eat(&Tok::Gt) {
                value = i64::from(value > self.unary(depth)?);
            } else if self.eat(&Tok::Ge) {
                value = i64::from(value >= self.unary(depth)?);
            } else {
                return Some(value);
            }
        }
    }

    fn unary(&mut self, depth: u32) -> Option<i64> {
        let depth = depth.checked_add(1).filter(|d| *d <= MAX_DEPTH)?;
        match self.next()? {
            Tok::Not => Some(i64::from(self.unary(depth)? == 0)),
            Tok::LParen => {
                let value = self.or(depth)?;
                self.eat(&Tok::RParen).then_some(value)
            }
            Tok::Num(value) => Some(value),
            Tok::Name(name) if name == "defined" => {
                let paren = self.eat(&Tok::LParen);
                let Tok::Name(name) = self.next()? else {
                    return None;
                };
                if paren && !self.eat(&Tok::RParen) {
                    return None;
                }
                Some(i64::from(self.macros.contains_key(&name)))
            }
            Tok::Name(name) => Some(self.macros.get(&name).map_or(0, |body| atoi(body))),
            Tok::RParen
            | Tok::AndAnd
            | Tok::OrOr
            | Tok::Eq
            | Tok::Ne
            | Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge => None,
        }
    }
}

/// Whether an `#if` or `#elif` expression is true; one that does not parse is false
/// (PRJ-5.5).
pub(crate) fn eval(expr: &str, macros: &HashMap<String, String>) -> bool {
    let Some(toks) = tokens(expr) else {
        return false;
    };
    let mut parser = Parser {
        toks,
        pos: 0,
        macros,
    };
    match parser.or(0) {
        Some(value) => parser.pos == parser.toks.len() && value != 0,
        None => false,
    }
}
