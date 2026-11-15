// SPDX-License-Identifier: GPL-3.0-or-later

//! A query's syntax: the parts of TrenchBroom's expression language its manual's "Query
//! Language" lists under "Syntax", read as its "Expression Language" defines them. Names and
//! literals, arrays, `a.b` and `a[b]`, the unary `+`, `-` and `!`, the binary operators by its
//! "Binary Operator Precedence" table, and the built-in functions, called as `f(a, b)` or,
//! taking two arguments, infix as `a f b`, which binds more tightly than any operator. Map
//! literals, ranges, switch and case terms, `%` and the bitwise operators, which that table
//! leaves out, are not read: text using them is searched for as text, as `light%` is.

use super::value::{Arithmetic, Sign, Value};

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unary {
    /// `+` or `-`.
    Sign(Sign),
    /// `!`.
    Not,
}

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binary {
    /// `+`, `-`, `*`, `/` or `%`.
    Arithmetic(Arithmetic),
    /// `<`.
    Less,
    /// `<=`.
    LessOrEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterOrEqual,
    /// `==`.
    Equal,
    /// `!=`.
    NotEqual,
    /// `&&`.
    And,
    /// `||`.
    Or,
}

impl Binary {
    /// Its precedence, higher binding more tightly, from the manual's table.
    const fn precedence(self) -> u8 {
        match self {
            Self::Arithmetic(Arithmetic::Multiply | Arithmetic::Divide) => SCALING,
            Self::Arithmetic(Arithmetic::Add | Arithmetic::Subtract) => ADDING,
            Self::Less | Self::LessOrEqual | Self::Greater | Self::GreaterOrEqual => ORDERING,
            Self::Equal | Self::NotEqual => EQUALITY,
            Self::And => AND,
            Self::Or => LOWEST,
        }
    }
}

/// The precedence of `*` and `/`.
const SCALING: u8 = 12;
/// Of `+` and `-`.
const ADDING: u8 = 11;
/// Of `<`, `<=`, `>` and `>=`.
const ORDERING: u8 = 9;
/// Of `==` and `!=`.
const EQUALITY: u8 = 8;
/// Of `&&`.
const AND: u8 = 4;
/// The lowest, `||`'s.
const LOWEST: u8 = 3;

/// How many arguments `vec` takes: a vector's components.
const VECTOR_ARGUMENTS: usize = 3;

/// A built-in function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    /// `vec(x, y, z)`.
    Vec,
    /// `bbox(min, max)`.
    Bbox,
    /// `is(a, b)`, `a == b`.
    Is,
    /// `like(value, pattern)`.
    Like,
    /// `contains(container, value)`.
    Contains,
    /// `in(value, container)`.
    In,
    /// `distanceTo(a, b)`.
    DistanceTo,
    /// `intersects(a, b)`.
    Intersects,
}

impl Builtin {
    /// Every built-in function.
    const ALL: [Self; 8] = [
        Self::Vec,
        Self::Bbox,
        Self::Is,
        Self::Like,
        Self::Contains,
        Self::In,
        Self::DistanceTo,
        Self::Intersects,
    ];

    /// Its name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Vec => "vec",
            Self::Bbox => "bbox",
            Self::Is => "is",
            Self::Like => "like",
            Self::Contains => "contains",
            Self::In => "in",
            Self::DistanceTo => "distanceTo",
            Self::Intersects => "intersects",
        }
    }

    /// How many arguments it takes.
    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Vec => VECTOR_ARGUMENTS,
            Self::Bbox
            | Self::Is
            | Self::Like
            | Self::Contains
            | Self::In
            | Self::DistanceTo
            | Self::Intersects => 2,
        }
    }

    /// The function of `name`.
    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    /// A literal.
    Literal(Value),
    /// A name: a field of the object.
    Name(String),
    /// `[a, b, c]`.
    Array(Vec<Self>),
    /// A unary operator and its operand.
    Unary(Unary, Box<Self>),
    /// A binary operator and its operands.
    Binary(Binary, Box<Self>, Box<Self>),
    /// A built-in function called.
    Call(Builtin, Vec<Self>),
    /// A call of a name that is no built-in function.
    Unknown(String, Vec<Self>),
    /// `a[b]`.
    Index(Box<Self>, Box<Self>),
    /// `a.b`.
    Dot(Box<Self>, String),
}

impl Expr {
    /// The expression `text` is, or `None` when it is not one.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let tokens = Token::lex(text)?;
        let mut parser = Parser {
            tokens,
            at: 0,
            depth: 0,
        };
        let expr = parser.expression()?;
        parser.peek().is_none().then_some(expr)
    }
}

/// A symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Symbol {
    Open,
    Close,
    OpenBracket,
    CloseBracket,
    Comma,
    Dot,
    Bang,
    Operator(Binary),
}

/// A token.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Text(String),
    Name(String),
    Symbol(Symbol),
}

impl Token {
    /// The tokens of `text`, or `None` when it holds a character no token starts with.
    fn lex(text: &str) -> Option<Vec<Self>> {
        let mut tokens = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            let token = if c.is_ascii_digit() {
                let mut number = String::new();
                while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    number.push(d);
                    chars.next();
                }
                let mut ahead = chars.clone();
                if ahead.next() == Some('.') && ahead.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                    number.push('.');
                    while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                        number.push(d);
                        chars.next();
                    }
                }
                Self::Number(number.parse().ok()?)
            } else if c == '_' || c.is_ascii_alphabetic() {
                let mut name = String::new();
                while let Some(&d) = chars
                    .peek()
                    .filter(|d| **d == '_' || d.is_ascii_alphanumeric())
                {
                    name.push(d);
                    chars.next();
                }
                Self::Name(name)
            } else if c == '"' || c == '\'' {
                chars.next();
                let mut string = String::new();
                loop {
                    match chars.next()? {
                        '\\' if chars.peek() == Some(&c) => {
                            chars.next();
                            string.push(c);
                        }
                        '\\' if chars.peek() == Some(&'\\') && {
                            let mut ahead = chars.clone();
                            ahead.next();
                            ahead.peek() == Some(&c)
                        } =>
                        {
                            chars.next();
                            string.push('\\');
                        }
                        end if end == c => break,
                        other => string.push(other),
                    }
                }
                Self::Text(string)
            } else {
                chars.next();
                let next = chars.peek().copied();
                let mut two = |symbol: Symbol| {
                    chars.next();
                    Some(symbol)
                };
                let symbol = match (c, next) {
                    ('<', Some('=')) => two(Symbol::Operator(Binary::LessOrEqual)),
                    ('>', Some('=')) => two(Symbol::Operator(Binary::GreaterOrEqual)),
                    ('=', Some('=')) => two(Symbol::Operator(Binary::Equal)),
                    ('!', Some('=')) => two(Symbol::Operator(Binary::NotEqual)),
                    ('&', Some('&')) => two(Symbol::Operator(Binary::And)),
                    ('|', Some('|')) => two(Symbol::Operator(Binary::Or)),
                    ('<', _) => Some(Symbol::Operator(Binary::Less)),
                    ('>', _) => Some(Symbol::Operator(Binary::Greater)),
                    ('!', _) => Some(Symbol::Bang),
                    ('+', _) => Some(Symbol::Operator(Binary::Arithmetic(Arithmetic::Add))),
                    ('-', _) => Some(Symbol::Operator(Binary::Arithmetic(Arithmetic::Subtract))),
                    ('*', _) => Some(Symbol::Operator(Binary::Arithmetic(Arithmetic::Multiply))),
                    ('/', _) => Some(Symbol::Operator(Binary::Arithmetic(Arithmetic::Divide))),
                    ('(', _) => Some(Symbol::Open),
                    (')', _) => Some(Symbol::Close),
                    ('[', _) => Some(Symbol::OpenBracket),
                    (']', _) => Some(Symbol::CloseBracket),
                    (',', _) => Some(Symbol::Comma),
                    ('.', _) => Some(Symbol::Dot),
                    _ => None,
                };
                Self::Symbol(symbol?)
            };
            tokens.push(token);
        }
        Some(tokens)
    }
}

/// How deeply terms may nest, so a long run of brackets cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

/// Reads an expression from tokens.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at = self.at.saturating_add(1);
        token
    }

    /// Takes `symbol` when it is next.
    fn take(&mut self, symbol: Symbol) -> bool {
        let found = self.peek() == Some(&Token::Symbol(symbol));
        if found {
            self.next();
        }
        found
    }

    fn expect(&mut self, symbol: Symbol) -> Option<()> {
        self.take(symbol).then_some(())
    }

    fn expression(&mut self) -> Option<Expr> {
        self.depth = self.depth.saturating_add(1);
        let expr = if self.depth > MAX_DEPTH {
            None
        } else {
            self.binary(LOWEST)
        };
        self.depth = self.depth.saturating_sub(1);
        expr
    }

    /// Binary operators of at least `least` precedence, left to right.
    fn binary(&mut self, least: u8) -> Option<Expr> {
        let mut lhs = self.infix()?;
        while let Some(Token::Symbol(Symbol::Operator(op))) = self.peek() {
            let op = *op;
            if op.precedence() < least {
                break;
            }
            self.next();
            let rhs = self.binary(op.precedence().saturating_add(1))?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    /// Simple terms joined by infix calls, left to right.
    fn infix(&mut self) -> Option<Expr> {
        let mut lhs = self.simple()?;
        while let Some(Token::Name(name)) = self.peek() {
            let Some(function) = Builtin::named(name).filter(|f| f.arity() == 2) else {
                break;
            };
            self.next();
            let rhs = self.simple()?;
            lhs = Expr::Call(function, vec![lhs, rhs]);
        }
        Some(lhs)
    }

    /// A unary operator on a simple term, or a term and its subscripts.
    fn simple(&mut self) -> Option<Expr> {
        let unary = match self.peek() {
            Some(Token::Symbol(Symbol::Bang)) => Some(Unary::Not),
            Some(Token::Symbol(Symbol::Operator(Binary::Arithmetic(Arithmetic::Add)))) => {
                Some(Unary::Sign(Sign::Plus))
            }
            Some(Token::Symbol(Symbol::Operator(Binary::Arithmetic(Arithmetic::Subtract)))) => {
                Some(Unary::Sign(Sign::Minus))
            }
            _ => None,
        };
        if let Some(unary) = unary {
            self.next();
            self.depth = self.depth.saturating_add(1);
            let operand = if self.depth > MAX_DEPTH {
                None
            } else {
                self.simple()
            };
            self.depth = self.depth.saturating_sub(1);
            return Some(Expr::Unary(unary, Box::new(operand?)));
        }
        let mut term = self.primary()?;
        loop {
            if self.take(Symbol::Dot) {
                let Some(Token::Name(key)) = self.next() else {
                    return None;
                };
                term = Expr::Dot(Box::new(term), key);
            } else if self.take(Symbol::OpenBracket) {
                let key = self.expression()?;
                self.expect(Symbol::CloseBracket)?;
                term = Expr::Index(Box::new(term), Box::new(key));
            } else {
                return Some(term);
            }
        }
    }

    /// The expressions up to `close`, separated by commas.
    fn list(&mut self, close: Symbol) -> Option<Vec<Expr>> {
        let mut items = Vec::new();
        if self.take(close) {
            return Some(items);
        }
        loop {
            items.push(self.expression()?);
            if self.take(close) {
                return Some(items);
            }
            self.expect(Symbol::Comma)?;
        }
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.next()? {
            Token::Number(n) => Some(Expr::Literal(Value::Number(n))),
            Token::Text(text) => Some(Expr::Literal(Value::Text(text))),
            Token::Name(name) => Some(match name.as_str() {
                "true" => Expr::Literal(Value::Bool(true)),
                "false" => Expr::Literal(Value::Bool(false)),
                "null" => Expr::Literal(Value::Null),
                "undefined" => Expr::Literal(Value::Undefined),
                _ if self.take(Symbol::Open) => {
                    let arguments = self.list(Symbol::Close)?;
                    match Builtin::named(&name) {
                        Some(function) => Expr::Call(function, arguments),
                        None => Expr::Unknown(name, arguments),
                    }
                }
                _ => Expr::Name(name),
            }),
            Token::Symbol(Symbol::OpenBracket) => {
                Some(Expr::Array(self.list(Symbol::CloseBracket)?))
            }
            Token::Symbol(Symbol::Open) => {
                let expr = self.expression()?;
                self.expect(Symbol::Close)?;
                Some(expr)
            }
            Token::Symbol(_) => None,
        }
    }
}
