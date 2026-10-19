// SPDX-License-Identifier: GPL-3.0-or-later

//! `#if` and `#elif` expressions (PRJ-5.5).
//!
//! One evaluator for both readers of a conditional: the project graph, which follows the
//! directives that decide which files a module reads (`qc-project`), and the preprocessor in an
//! FTEQCC dialect (FTEF-14.1), so the two always take the same side.

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
#[must_use]
pub fn atoi(text: &str) -> i64 {
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
    macros: &'a dyn Fn(&str) -> Option<String>,
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
                Some(i64::from((self.macros)(&name).is_some()))
            }
            Tok::Name(name) => Some((self.macros)(&name).map_or(0, |body| atoi(&body))),
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
/// (PRJ-5.5). `macros` gives the text of a defined macro and `None` for a name that is not
/// one.
#[must_use]
pub fn eval(expr: &str, macros: &dyn Fn(&str) -> Option<String>) -> bool {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::atoi;

    fn eval(expr: &str, macros: &HashMap<String, String>) -> bool {
        super::eval(expr, &|name: &str| macros.get(name).cloned())
    }

    fn macros() -> HashMap<String, String> {
        [
            ("ONE", "1"),
            ("TWO", "2 // two"),
            ("EMPTY", ""),
            ("WORD", "abc"),
            ("NEG", "-3"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    // spec PRJ-5.5: integers, names, defined, parentheses, ! and the comparisons, with C's precedence
    #[test]
    fn expressions() {
        let m = macros();
        let cases = [
            ("1", true),
            ("0", false),
            ("ONE", true),
            ("EMPTY", false),
            ("UNDEFINED", false),
            ("defined EMPTY", true),
            ("defined(UNDEFINED)", false),
            ("defined ( ONE )", true),
            ("!defined(ONE)", false),
            ("!!TWO", true),
            ("TWO == 2", true),
            ("TWO != 2", false),
            ("ONE < TWO", true),
            ("TWO <= 2", true),
            ("ONE > TWO", false),
            ("TWO >= 3", false),
            ("TWO >= 2", true),
            ("ONE != 2", true),
            ("2 < 2", false),
            ("2 > 2", false),
            ("2 <= 1", false),
            ("1 >= 2", false),
            ("NEG < 0", true),
            ("0 || 1 && 0", false),
            ("1 || 0 && 0", true),
            ("(1 || 0) && 0", false),
            ("1 == 1 && 2 == 2", true),
            ("1 < 2 == 1", true),
            ("defined(ONE) && !defined(TWO) || TWO == 2", true),
            ("ONE // trailing comment", true),
            ("ONE /* block */", true),
        ];
        for (expr, want) in cases {
            assert_eq!(eval(expr, &m), want, "{expr}");
        }
    }

    // spec PRJ-5.5: a name whose macro text is not a number is 0, and so is a number with a suffix
    #[test]
    fn values() {
        let m = macros();
        assert!(!eval("WORD", &m));
        assert!(eval("WORD == 0", &m));
        assert!(!eval("0x10", &m));
        assert!(eval("12abc == 12", &m));
    }

    // spec PRJ-5.5: an expression that does not parse is false
    #[test]
    fn malformed_is_false() {
        let m = macros();
        for expr in [
            "",
            "(",
            "(1",
            "1)",
            "1 +",
            "&& 1",
            "defined",
            "defined(",
            "defined(ONE",
            "defined 1",
            "1 2",
            "@",
            "== 1",
            "!",
        ] {
            assert!(!eval(expr, &m), "{expr:?}");
        }
        let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
        assert!(!eval(&deep, &m));
        let shallow = format!("{}1{}", "(".repeat(10), ")".repeat(10));
        assert!(eval(&shallow, &m));
        assert!(!eval(&"!".repeat(100), &m));
    }

    // spec PRJ-5.5: a macro's text is read as a decimal integer, as atoi reads it
    #[test]
    fn atoi_reads_like_c() {
        assert_eq!(atoi("42"), 42);
        assert_eq!(atoi("  -7x"), -7);
        assert_eq!(atoi("+5"), 5);
        assert_eq!(atoi(""), 0);
        assert_eq!(atoi("-"), 0);
        assert_eq!(atoi("x1"), 0);
        assert_eq!(atoi("99999999999999999999999"), i64::MAX);
        assert_eq!(atoi("-99999999999999999999999"), -i64::MAX);
    }
}
