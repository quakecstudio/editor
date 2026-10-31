// SPDX-License-Identifier: GPL-3.0-or-later

//! The 1000 random FTEQCC programs of the grammar differential (DIFF-13.1), from a fixed
//! seed: every declaration form of `docs/spec/dialect-fteqcc.md` (FTEF-15, FTEF-19), with each
//! qualifier spelt as its keyword and with the leading `__` that `QCC_PR_CheckKeyword` takes
//! (FTEF-15.1), enums, structs and unions, typedefs, classes with their member forms (FTEF-9,
//! FTEF-17), accessors (FTEF-10), and statements of every kind (FTEF-18). A program is mutated
//! by one token in [`MUTATED`] percent of cases, so rejections are compared as well as
//! acceptances.

use crate::oracles::rng::Rng;

/// How many programs a check generates, 1000 (DIFF-13.1).
pub(crate) const PROGRAMS: usize = 1000;

/// The percentage of programs that are mutated by one token (DIFF-13.1).
const MUTATED: usize = 35;

/// What every program declares first: the system globals and fields FTEQCC looks for, and the
/// builtins the statements call.
const PRELUDE: &str = "entity self;\nentity world;\nfloat time;\n.float nextthink;\n\
.vector origin;\n.void() think;\nvoid end_sys_fields;\nvoid(string s) print = #1;\n\
float(float a) fabs = #43;\nfloat(float a) rint = #36;\n";

/// The qualifiers a global may take (FTEF-15.1), and those written only with a leading `__`.
const QUALIFIERS: [&str; 8] = [
    "noref",
    "nosave",
    "static",
    "shared",
    "var",
    "strip",
    "nonstatic",
    "weak",
];
const UNDERSCORED: [&str; 6] = ["auto", "register", "unused", "used", "ignore", "deprecated"];

/// The basic types a value of the programs has.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    Float,
    Vector,
    Str,
    Entity,
    Int,
}

const TYPES: [Ty; 5] = [Ty::Float, Ty::Vector, Ty::Str, Ty::Entity, Ty::Int];

impl Ty {
    fn word(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Vector => "vector",
            Self::Str => "string",
            Self::Entity => "entity",
            Self::Int => "int",
        }
    }

    /// A constant of the type, for an initialiser; none for an entity.
    fn constant(self, rng: &mut Rng) -> Option<String> {
        let n = rng.below(9);
        match self {
            Self::Float => Some(format!("{n}")),
            Self::Int => Some(format!("{n}i")),
            Self::Vector => Some(format!("'{n} 1 2'")),
            Self::Str => Some(format!("\"s{n}\"")),
            Self::Entity => None,
        }
    }
}

/// The program being written and the names it has declared so far.
struct Gen {
    rng: Rng,
    out: String,
    /// The globals declared so far, with their types.
    globals: Vec<(Ty, String)>,
    /// The next number a declared name takes.
    next: usize,
}

impl Gen {
    fn name(&mut self, prefix: &str) -> String {
        self.next = self.next.saturating_add(1);
        format!("{prefix}{}", self.next)
    }

    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn ty(&mut self) -> Ty {
        TYPES
            .get(self.rng.below(TYPES.len()))
            .copied()
            .unwrap_or(Ty::Float)
    }

    /// Qualifiers for a global: none mostly, else one or two, in either spelling (FTEF-15.1).
    fn qualifiers(&mut self) -> String {
        let mut words = String::new();
        let count = if self.rng.chance(60) {
            0
        } else {
            self.rng.below(2).saturating_add(1)
        };
        for _ in 0..count {
            let word = if self.rng.chance(70) {
                let word = self.rng.pick(&QUALIFIERS);
                if self.rng.chance(30) {
                    format!("__{word}")
                } else {
                    word.to_owned()
                }
            } else {
                let word = self.rng.pick(&UNDERSCORED);
                if word == "deprecated" && self.rng.chance(50) {
                    "__deprecated(\"old\")".to_owned()
                } else {
                    format!("__{word}")
                }
            };
            words.push_str(&word);
            words.push(' ');
        }
        words
    }

    /// A float expression over the names `locals` and the float globals, `depth` levels deep.
    fn float_expr(&mut self, locals: &[(Ty, String)], depth: usize) -> String {
        if depth == 0 || self.rng.chance(35) {
            let floats: Vec<&str> = locals
                .iter()
                .chain(&self.globals)
                .filter(|(ty, _)| *ty == Ty::Float)
                .map(|(_, name)| name.as_str())
                .collect();
            if floats.is_empty() || self.rng.chance(40) {
                return format!("{}", self.rng.below(10));
            }
            let pick = self.rng.below(floats.len());
            return floats
                .get(pick)
                .map_or_else(|| "1".to_owned(), |n| (*n).to_owned());
        }
        let left = self.float_expr(locals, depth.saturating_sub(1));
        let right = self.float_expr(locals, depth.saturating_sub(1));
        match self.rng.below(12) {
            0 => format!("({left} + {right})"),
            1 => format!("({left} - {right})"),
            2 => format!("({left} * {right})"),
            3 => format!("({left} / {right})"),
            4 => format!("({left} & {right})"),
            5 => format!("({left} | {right})"),
            6 => format!("({left} && {right})"),
            7 => format!("({left} || {right})"),
            8 => format!("({left} < {right})"),
            9 => format!("({left} == {right})"),
            10 => format!("!{left}"),
            _ => format!("fabs({left})"),
        }
    }

    /// A statement of a body whose locals are `locals`, inside `loops` loops.
    fn statement(&mut self, locals: &mut Vec<(Ty, String)>, depth: usize, loops: usize) {
        let pick = if depth == 0 {
            self.rng.below(3)
        } else {
            self.rng.below(12)
        };
        match pick {
            0 => {
                let ty = self.ty();
                let name = self.name("l");
                let local = if self.rng.chance(50) { "local " } else { "" };
                match ty.constant(&mut self.rng).filter(|_| self.rng.chance(60)) {
                    Some(value) => self.line(&format!("{local}{} {name} = {value};", ty.word())),
                    None => self.line(&format!("{local}{} {name};", ty.word())),
                }
                locals.push((ty, name));
            }
            1 => {
                let target = locals
                    .iter()
                    .rev()
                    .find(|(ty, _)| *ty == Ty::Float)
                    .cloned();
                let value = self.float_expr(locals, 2);
                match target {
                    Some((_, name)) => {
                        let op = self.rng.pick(&["=", "+=", "-=", "*=", "|="]);
                        self.line(&format!("{name} {op} {value};"));
                    }
                    None => self.line("print(\"x\");"),
                }
            }
            2 => self.line("print(\"p\");"),
            3 => {
                let cond = self.float_expr(locals, 2);
                self.line(&format!("if ({cond})"));
                self.block(locals, depth, loops);
                if self.rng.chance(40) {
                    self.line("else");
                    self.block(locals, depth, loops);
                }
            }
            4 => {
                let cond = self.float_expr(locals, 1);
                self.line(&format!("while ({cond})"));
                self.block(locals, depth, loops.saturating_add(1));
            }
            5 => {
                self.line("do");
                self.block(locals, depth, loops.saturating_add(1));
                let cond = self.float_expr(locals, 1);
                self.line(&format!("while ({cond});"));
            }
            6 => {
                let name = self.name("i");
                if self.rng.chance(50) {
                    self.line(&format!("for (float {name} = 0; {name} < 3; {name}++)"));
                    self.block(locals, depth, loops.saturating_add(1));
                } else {
                    self.line(&format!("local float {name};"));
                    locals.push((Ty::Float, name.clone()));
                    self.line(&format!("for ({name} = 0; {name} < 3; {name} += 1)"));
                    self.block(locals, depth, loops.saturating_add(1));
                }
            }
            7 => {
                let value = self.float_expr(locals, 1);
                self.line(&format!("switch ({value})"));
                self.line("{");
                let cases = self.rng.below(3).saturating_add(1);
                for n in 0..cases {
                    if self.rng.chance(25) {
                        self.line(&format!("case {n} .. {}:", n.saturating_add(1)));
                    } else {
                        self.line(&format!("case {n}:"));
                    }
                    self.statement(locals, depth.saturating_sub(1), loops);
                    self.line("break;");
                }
                if self.rng.chance(50) {
                    self.line("default:");
                    self.statement(locals, depth.saturating_sub(1), loops);
                }
                self.line("}");
            }
            8 if loops > 0 => {
                let word = self.rng.pick(&["break;", "continue;"]);
                self.line(word);
            }
            9 => {
                let target = locals
                    .iter()
                    .rev()
                    .find(|(ty, _)| *ty == Ty::Float)
                    .cloned();
                let cond = self.float_expr(locals, 1);
                if let Some((_, name)) = target {
                    self.line(&format!("{name} = {cond} ? 1 : 2;"));
                } else {
                    self.line(&format!("if ({cond}) print(\"t\");"));
                }
            }
            10 => {
                let target = locals
                    .iter()
                    .rev()
                    .find(|(ty, _)| *ty == Ty::Float)
                    .cloned();
                if let Some((_, name)) = target {
                    let op = self.rng.pick(&["++", "--"]);
                    self.line(&format!("{name}{op};"));
                } else {
                    self.line(";");
                }
            }
            _ => self.block(locals, depth.saturating_sub(1), loops),
        }
    }

    /// `{ ... }`, a few statements deep.
    fn block(&mut self, locals: &mut Vec<(Ty, String)>, depth: usize, loops: usize) {
        self.line("{");
        let count = self.rng.below(3).saturating_add(1);
        let mark = locals.len();
        for _ in 0..count {
            self.statement(locals, depth.saturating_sub(1), loops);
        }
        locals.truncate(mark);
        self.line("}");
    }

    /// A function body returning `ret`, its parameters `params`, closed by `end`: `};` for a
    /// definition with `=`, `}` for C's form.
    fn body(&mut self, ret: Option<Ty>, params: &[(Ty, String)], end: &str) {
        self.line("{");
        let mut locals: Vec<(Ty, String)> = params.to_vec();
        let count = self.rng.below(4).saturating_add(1);
        for _ in 0..count {
            self.statement(&mut locals, 3, 0);
        }
        if let Some(ty) = ret {
            let value = match ty {
                Ty::Float => Some(self.float_expr(&locals, 2)),
                other => other.constant(&mut self.rng),
            };
            match value {
                Some(value) => self.line(&format!("return {value};")),
                None => self.line("return world;"),
            }
        }
        self.line(end);
    }

    /// Parameters for a function: up to three, of basic types.
    fn params(&mut self) -> Vec<(Ty, String)> {
        let count = self.rng.below(4);
        (0..count)
            .map(|_| {
                let ty = self.ty();
                let name = self.name("p");
                (ty, name)
            })
            .collect()
    }

    fn param_list(params: &[(Ty, String)]) -> String {
        let list: Vec<String> = params
            .iter()
            .map(|(ty, n)| format!("{} {n}", ty.word()))
            .collect();
        if list.is_empty() {
            "void".to_owned()
        } else {
            list.join(", ")
        }
    }

    /// A return type: `void` (none) or a basic type.
    fn ret(&mut self) -> Option<Ty> {
        if self.rng.chance(50) {
            None
        } else {
            Some(self.ty())
        }
    }

    fn ret_word(ret: Option<Ty>) -> &'static str {
        ret.map_or("void", Ty::word)
    }

    /// One file-level declaration.
    fn declaration(&mut self) {
        match self.rng.below(12) {
            0 | 1 => {
                let qualifiers = self.qualifiers();
                let ty = self.ty();
                let name = self.name("g");
                match ty.constant(&mut self.rng).filter(|_| self.rng.chance(50)) {
                    Some(value) => {
                        self.line(&format!("{qualifiers}{} {name} = {value};", ty.word()))
                    }
                    None => self.line(&format!("{qualifiers}{} {name};", ty.word())),
                }
                self.globals.push((ty, name));
            }
            2 => {
                let ty = self.ty();
                let name = self.name("k");
                let spelling = if self.rng.chance(30) {
                    "__const"
                } else {
                    "const"
                };
                if let Some(value) = ty.constant(&mut self.rng) {
                    self.line(&format!("{spelling} {} {name} = {value};", ty.word()));
                }
            }
            3 | 4 => {
                let ret = self.ret();
                let params = self.params();
                let name = self.name("f");
                let list = Self::param_list(&params);
                let word = Self::ret_word(ret);
                if self.rng.chance(50) {
                    self.line(&format!("{word}({list}) {name} ="));
                    self.body(ret, &params, "};");
                } else {
                    self.line(&format!("{word} {name}({list})"));
                    self.body(ret, &params, "}");
                }
            }
            5 => {
                let ret = self.ret();
                let params = self.params();
                let name = self.name("f");
                let list = Self::param_list(&params);
                let word = Self::ret_word(ret);
                self.line(&format!("{word}({list}) {name};"));
                self.line(&format!("{word}({list}) {name} ="));
                self.body(ret, &params, "};");
            }
            6 => {
                let ty = self.ty();
                let name = self.name("fld");
                self.line(&format!(".{} {name};", ty.word()));
            }
            7 => {
                let name = self.name("t");
                let ty = self.ty();
                self.line(&format!("typedef {} {name};", ty.word()));
                let var = self.name("g");
                self.line(&format!("{name} {var};"));
                self.globals.push((ty, var));
            }
            8 => {
                let a = self.name("E");
                let b = self.name("E");
                let c = self.name("E");
                if self.rng.chance(50) {
                    self.line(&format!("enum {{ {a}, {b} = 4, {c} }};"));
                } else {
                    self.line(&format!("enumflags {{ {a}, {b}, {c} }};"));
                }
            }
            9 => {
                let name = self.name("S");
                let word = self.rng.pick(&["struct", "union"]);
                let var = self.name("g");
                if self.rng.chance(50) {
                    self.line(&format!("{word} {name} {{ float a; vector b; }};"));
                    self.line(&format!("{word} {name} {var};"));
                } else {
                    self.line(&format!("typedef {word} {{ float a; string b; }} {name};"));
                    self.line(&format!("{name} {var};"));
                }
            }
            10 => self.class(),
            _ => self.accessor(),
        }
    }

    /// A class with a field, a method defined after it, a virtual and a nonvirtual method
    /// written in it (FTEF-9, FTEF-17).
    fn class(&mut self) {
        let class = self.name("C");
        let field = self.name("m");
        let method = self.name("M");
        let inline = self.name("N");
        self.line(&format!("class {class} : entity"));
        self.line("{");
        self.line(&format!("float {field};"));
        self.line(&format!("void() {method};"));
        let kind = self
            .rng
            .pick(&["virtual", "nonvirtual", "__virtual", "__nonvirtual"]);
        self.line(&format!(
            "{kind} float(float x) {inline} = {{ return x + {field}; }};"
        ));
        self.line("};");
        self.line(&format!("void() {class}::{method} ="));
        self.line("{");
        self.line(&format!("{field} = {inline}(2);"));
        self.line("};");
    }

    /// An accessor of a float with a getter and a setter (FTEF-10).
    fn accessor(&mut self) {
        let name = self.name("A");
        self.line(&format!("accessor {name} : float"));
        self.line("{");
        let get = if self.rng.chance(50) {
            "get"
        } else {
            "inline get"
        };
        self.line(&format!("{get} float twice = {{ return this * 2; }};"));
        self.line("set float twice = { this = value / 2; };");
        self.line("};");
        let var = self.name("g");
        self.line(&format!("{name} {var};"));
    }
}

/// Program number `n`: the prelude and a few declarations, mutated by one token in [`MUTATED`]
/// percent of programs.
pub(crate) fn program(n: usize) -> String {
    let seed = u64::try_from(n)
        .unwrap_or(0)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0x5EED);
    let mut generator = Gen {
        rng: Rng(seed | 1),
        out: String::from(PRELUDE),
        globals: Vec::new(),
        next: 0,
    };
    let count = generator.rng.below(4).saturating_add(1);
    for _ in 0..count {
        generator.declaration();
    }
    if generator.rng.chance(MUTATED) {
        mutate(&mut generator.rng, &generator.out)
    } else {
        generator.out
    }
}

/// `text` with one token, after the prelude, deleted, doubled, or followed by a stray
/// punctuation mark.
fn mutate(rng: &mut Rng, text: &str) -> String {
    let (prelude, rest) = text.split_at(PRELUDE.len().min(text.len()));
    let tokens: Vec<&str> = rest.split_inclusive(&[' ', '\n'][..]).collect();
    let at = rng.below(tokens.len());
    let mut out = String::from(prelude);
    for (index, token) in tokens.iter().enumerate() {
        if index != at {
            out.push_str(token);
            continue;
        }
        match rng.below(3) {
            0 => {
                if token.ends_with('\n') {
                    out.push('\n');
                }
            }
            1 => {
                out.push_str(token);
                out.push_str(token);
            }
            _ => {
                out.push_str(token);
                out.push_str(rng.pick(&[";", ")", "}", ",", "=", "{", "("]));
                out.push(' ');
            }
        }
    }
    out
}
