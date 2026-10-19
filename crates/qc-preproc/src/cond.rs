// SPDX-License-Identifier: GPL-3.0-or-later

//! `#if` and `#elif` expressions (PRJ-5.5).
//!
//! One evaluator for both readers of a conditional: the project graph, which follows the
//! directives that decide which files a module reads (`qc-project`), and the preprocessor in an
//! FTEQCC dialect (FTEF-14.1), so the two always take the same side. Under the vanilla profile
//! the expression has C's precedence (PRJ-5.5); under an FTEQCC profile it is read as FTEQCC's
//! `ParsePrecompilerIf` reads it (FTEF-14.11).

use qc_lexer::Dialect;

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

/// Whether an `#if` or `#elif` expression is true in `dialect`. `macros` gives the text of a
/// defined macro and `None` for a name that is not one.
///
/// Under the vanilla profile the expression has C's precedence and one that does not parse is
/// false (PRJ-5.5); under an FTEQCC profile it is read as FTEQCC reads it (FTEF-14.11).
#[must_use]
pub fn eval(expr: &str, macros: &dyn Fn(&str) -> Option<String>, dialect: Dialect) -> bool {
    if dialect.has_fteqcc_extensions() {
        let mut reader = FteIf {
            text: expr.as_bytes(),
            pos: 0,
            depth: 0,
            macros,
        };
        return reader.level(FTE_TOPLEVEL).is_some_and(|value| value != 0);
    }
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

/// `PPI_NOT` in FTEQCC's `ParsePrecompilerIf`: the operand of `!`.
const FTE_NOT: u8 = 1;
/// `PPI_COMPARISON`: `<=`, `>=`, `<`, `>`, `!=` and `==`, one level, to the right.
const FTE_COMPARISON: u8 = 3;
/// `PPI_LOGICAL`: `||` and `&&`, one level, to the right.
const FTE_LOGICAL: u8 = 4;
/// `PPI_TOPLEVEL`: a whole expression.
const FTE_TOPLEVEL: u8 = 5;

/// The comparison operators of `PPI_COMPARISON`, in the order FTEQCC tries them.
const FTE_COMPARISONS: [&[u8]; 6] = [b"<=", b">=", b"<", b">", b"!=", b"=="];

/// FTEQCC's `#if` reader (FTEF-14.11): `ParsePrecompilerIf` over the expression's bytes.
struct FteIf<'a> {
    text: &'a [u8],
    pos: usize,
    depth: u32,
    macros: &'a dyn Fn(&str) -> Option<String>,
}

/// `qcc_iswhite` in FTEQCC's `cmdlib.h`.
const fn fte_white(byte: u8) -> bool {
    matches!(byte, b' ' | b'\r' | b'\n' | b'\t' | 0x0b)
}

impl FteIf<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.text.get(self.pos.checked_add(ahead)?).copied()
    }

    fn starts_with(&self, word: &[u8]) -> bool {
        self.text
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(word))
    }

    fn advance(&mut self, len: usize) {
        self.pos = self.pos.saturating_add(len).min(self.text.len());
    }

    /// `while (*pr_file_p && qcc_iswhite(*pr_file_p) && *pr_file_p != '\n') pr_file_p++;`
    fn skip_white(&mut self) {
        while self.peek(0).is_some_and(|b| fte_white(b) && b != b'\n') {
            self.advance(1);
        }
    }

    /// `while (*pr_file_p == ' ' || *pr_file_p == '\t') pr_file_p++;`
    fn skip_blanks(&mut self) {
        while matches!(self.peek(0), Some(b' ' | b'\t')) {
            self.advance(1);
        }
    }

    /// `QCC_PR_SimpleGetToken`: whitespace skipped, then the bytes up to whitespace, `,`, `;`,
    /// `(`, `)`, `]` or `//`. `None` at the end of the line, at a comment, or when no byte is
    /// taken.
    fn word(&mut self) -> Option<String> {
        while let Some(b) = self.peek(0) {
            if !fte_white(b) {
                break;
            }
            if b == b'\n' {
                return None;
            }
            self.advance(1);
        }
        if self.peek(0) == Some(b'/') && matches!(self.peek(1), Some(b'/' | b'*')) {
            return None;
        }
        let start = self.pos;
        while let Some(b) = self.peek(0) {
            let stop = fte_white(b)
                || matches!(b, b',' | b';' | b')' | b'(' | b']')
                || (b == b'/' && self.peek(1) == Some(b'/'));
            if stop {
                break;
            }
            self.advance(1);
        }
        let word = self.text.get(start..self.pos)?;
        (!word.is_empty()).then(|| String::from_utf8_lossy(word).into_owned())
    }

    /// `ParsePrecompilerIf(level)` one nesting deeper, for a parenthesis, a `!` or an
    /// operator's right side: `None` past the nesting limit.
    fn nested(&mut self, level: u8) -> Option<i64> {
        self.depth = self.depth.checked_add(1).filter(|d| *d <= MAX_DEPTH)?;
        let value = self.level(level);
        self.depth = self.depth.saturating_sub(1);
        value
    }

    /// `ParsePrecompilerIf(level)`: `None` where FTEQCC reports an error.
    fn level(&mut self, level: u8) -> Option<i64> {
        let Some(below) = level.checked_sub(1) else {
            return self.value();
        };
        let value = self.level(below)?;
        self.skip_white();
        match level {
            // `eval = ParsePrecompilerIf(level)||eval;`: the right side is read first, at the
            // same level, so `||` and `&&` share one level and group to the right.
            FTE_LOGICAL if self.starts_with(b"||") => {
                self.advance(2);
                let rhs = self.nested(level)?;
                Some(i64::from(rhs != 0 || value != 0))
            }
            FTE_LOGICAL if self.starts_with(b"&&") => {
                self.advance(2);
                let rhs = self.nested(level)?;
                Some(i64::from(rhs != 0 && value != 0))
            }
            FTE_COMPARISON => {
                let Some(op) = FTE_COMPARISONS.into_iter().find(|op| self.starts_with(op)) else {
                    return Some(value);
                };
                self.advance(op.len());
                let rhs = self.nested(level)?;
                let result = match op {
                    b"<=" => value <= rhs,
                    b">=" => value >= rhs,
                    b"<" => value < rhs,
                    b">" => value > rhs,
                    b"!=" => value != rhs,
                    _ => value == rhs,
                };
                Some(i64::from(result))
            }
            _ => Some(value),
        }
    }

    /// The `PPI_VALUE` case: `(expr)`, `!value`, `defined NAME` or `defined(NAME)`, or a word,
    /// a macro's text or the word itself read with `atoi`.
    fn value(&mut self) -> Option<i64> {
        self.skip_white();
        if self.peek(0) == Some(b'(') {
            self.advance(1);
            let value = self.nested(FTE_TOPLEVEL)?;
            self.skip_blanks();
            // "unclosed bracket condition"
            if self.peek(0) != Some(b')') {
                return None;
            }
            self.advance(1);
            return Some(value);
        }
        if self.peek(0) == Some(b'!') {
            self.advance(1);
            return Some(i64::from(self.nested(FTE_NOT)? == 0));
        }
        if self.starts_with(b"defined") {
            self.advance(7);
            self.skip_blanks();
            let brackets = self.peek(0) == Some(b'(');
            if brackets {
                self.advance(1);
            }
            let name = self.word().unwrap_or_default();
            let defined = (self.macros)(&name).is_some();
            if brackets {
                self.skip_blanks();
                // "unclosed defined condition"
                if self.peek(0) != Some(b')') {
                    return None;
                }
                self.advance(1);
            }
            return Some(i64::from(defined));
        }
        // "unexpected end-of-line"
        let word = self.word()?;
        Some((self.macros)(&word).map_or_else(|| atoi(&word), |body| atoi(&body)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use qc_lexer::Dialect;

    use super::atoi;

    fn eval(expr: &str, macros: &HashMap<String, String>) -> bool {
        super::eval(
            expr,
            &|name: &str| macros.get(name).cloned(),
            Dialect::Vanilla,
        )
    }

    fn fte_eval(expr: &str, macros: &HashMap<String, String>) -> bool {
        super::eval(
            expr,
            &|name: &str| macros.get(name).cloned(),
            Dialect::Fteqcc,
        )
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

    // spec PRJ-5.5: under the vanilla profile `&&` binds tighter than `||`, as in C
    // spec FTEF-14.11: under the FTEQCC profile `||` and `&&` share one level and group to the right
    #[test]
    fn logical_precedence_by_profile() {
        let m = macros();
        // C: 1 || (0 && 0) is 1. FTEQCC: 1 || (0 && 0), the right side read first, is 1 too.
        assert!(eval("1 || 0 && 0", &m));
        assert!(fte_eval("1 || 0 && 0", &m));
        // The mirror. C: (0 && 0) || 1 is 1. FTEQCC: 0 && (0 || 1) is 0.
        assert!(eval("0 && 0 || 1", &m));
        assert!(!fte_eval("0 && 0 || 1", &m));
        // C: (1 && 0) || 1 is 1. FTEQCC: 1 && (0 || 1) is 1.
        assert!(eval("1 && 0 || 1", &m));
        assert!(fte_eval("1 && 0 || 1", &m));
        // C: 0 || (1 && 0) is 0. FTEQCC: 0 || (1 && 0) is 0.
        assert!(!eval("0 || 1 && 0", &m));
        assert!(!fte_eval("0 || 1 && 0", &m));
        // C: 1 || (1 && 0) is 1, and so is FTEQCC's reading; parentheses group as written.
        assert!(fte_eval("1 || 1 && 0", &m));
        assert!(!fte_eval("(1 || 1) && 0", &m));
    }

    // spec FTEF-14.11: the comparisons share one level and group to the right, below `||` and `&&`
    #[test]
    fn fteqcc_comparisons() {
        let m = macros();
        // C: (1 < 2) == 1 is 1. FTEQCC: 1 < (2 == 1) is 0.
        assert!(eval("1 < 2 == 1", &m));
        assert!(!fte_eval("1 < 2 == 1", &m));
        for (expr, want) in [
            ("TWO == 2", true),
            ("TWO != 2", false),
            ("ONE < TWO", true),
            ("TWO <= 2", true),
            ("ONE > TWO", false),
            ("TWO >= 2", true),
            ("NEG < 0", true),
            ("1 == 1 && 2 == 2", true),
            ("defined(ONE) && !defined(TWO) || TWO == 2", true),
            ("defined(ONE) && defined(UNDEFINED) || 0", false),
            ("!defined(QWSSQC)&& !defined(MENU)", true),
        ] {
            assert_eq!(fte_eval(expr, &m), want, "{expr}");
        }
    }

    // spec FTEF-14.11: a value is a whitespace-delimited word read with atoi, defined takes a name with or without parentheses, and ! negates a value
    #[test]
    fn fteqcc_values() {
        let m = macros();
        for (expr, want) in [
            ("1", true),
            ("0", false),
            ("ONE", true),
            ("EMPTY", false),
            ("UNDEFINED", false),
            ("WORD", false),
            ("defined EMPTY", true),
            ("defined(UNDEFINED)", false),
            ("defined ( ONE )", true),
            ("!ONE", false),
            ("!!TWO", true),
            ("(ONE)", true),
            // `QCC_PR_SimpleGetToken` stops only at whitespace, `,`, `;`, `(`, `)` and `]`, so
            // `1||0` is one word, read as 1.
            ("0||1", false),
            ("1||0", true),
            ("12abc", true),
            // What follows a whole expression is not read.
            ("1 2", true),
            ("ONE // comment", true),
        ] {
            assert_eq!(fte_eval(expr, &m), want, "{expr}");
        }
    }

    // spec FTEF-14.11: an expression FTEQCC reports as an error is false
    #[test]
    fn fteqcc_errors_are_false() {
        let m = macros();
        for expr in [
            "",
            "(1",
            "defined(ONE",
            "1 ||",
            "!",
            "(",
            "// only a comment",
        ] {
            assert!(!fte_eval(expr, &m), "{expr:?}");
        }
        let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
        assert!(!fte_eval(&deep, &m));
        let shallow = format!("{}1{}", "(".repeat(5), ")".repeat(5));
        assert!(fte_eval(&shallow, &m));
    }

    // spec PRJ-5.5: `>` compares as in C
    #[test]
    fn greater_than() {
        let m = macros();
        assert!(eval("2 > 1", &m));
        assert!(eval("TWO > ONE", &m));
        assert!(!eval("1 > 2", &m));
    }

    // spec FTEF-14.11: each comparison compares as its operator says, at the boundary too
    #[test]
    fn fteqcc_comparison_operators() {
        let m = macros();
        for (expr, want) in [
            ("1 <= 2", true),
            ("3 <= 2", false),
            ("3 >= 2", true),
            ("1 >= 2", false),
            ("2 > 1", true),
            ("2 > 2", false),
            ("1 > 2", false),
            ("2 < 2", false),
            ("1 < 2", true),
            ("1 != 2", true),
        ] {
            assert_eq!(fte_eval(expr, &m), want, "{expr}");
        }
    }

    // spec FTEF-14.11: a word ends at `//` but not at a lone `/` or `*`, and a comment can
    // start only a word
    #[test]
    fn fteqcc_words_and_slashes() {
        let m = macros();
        for (expr, want) in [
            ("1*", true),
            ("ONE/2", false),
            ("ONE// c", true),
            ("ONE//", true),
            ("/* c */ 1", false),
        ] {
            assert_eq!(fte_eval(expr, &m), want, "{expr}");
        }
    }
}
