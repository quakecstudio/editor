// SPDX-License-Identifier: GPL-3.0-or-later

//! Generated programs for the compiler differential: random statements and expressions over
//! globals, fields, locals, parameters and calls of every vanilla type, from a fixed seed, so the
//! operator table, temporaries, literal sharing, branches and calls meet in combinations no
//! hand-written case lists (QC-14 to QC-21). About one expression in a hundred takes an operand of
//! a random type, so the two compilers' type errors are compared too.
//!
//! The programs never end an expression statement with a field load, which id's `qcc` would
//! rewrite (QC-17.4, QC-27.11), never initialise a string, which id's `qcc` gives a stale word
//! (QC-20.6, QC-27.12), and declare `end_sys_globals` first, so that no vector literal is before
//! it (QC-25.7, QC-27.13).

/// How many programs are generated.
pub(crate) const PROGRAMS: usize = 300;

use crate::oracles::rng::Rng;

/// The value types expressions have.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    Float,
    Vector,
    Str,
    Entity,
}

const TYPES: [Ty; 4] = [Ty::Float, Ty::Vector, Ty::Str, Ty::Entity];

/// The declarations every program starts with.
const PRELUDE: &str = "float end_sys_globals;\n\
float f1, f2, f3;\nvector v1, v2;\nstring s1, s2;\nentity e1, e2;\n\
.float ff1, ff2;\n.vector fv;\n.string fs;\n.entity fe;\n.void() fth;\n\
float(float x, float y) add = #1;\nvector(vector a) vfn = #2;\nvoid(...) pr = #3;\n\
string() sfn = #4;\nfloat() rnd = #5;\nfloat K1 = 1;\nvector KV = '1 2 3';\n\
$frame w1 w2 w3\nvoid() th1;\n";

/// One function being generated: its locals so far, by type.
struct Body {
    locals: Vec<(Ty, String)>,
    params: Vec<(Ty, String)>,
}

struct Gen {
    rng: Rng,
    out: String,
    depth: usize,
    next_local: usize,
}

impl Gen {
    fn variables(&self, body: &Body, ty: Ty) -> Vec<String> {
        let globals: &[&str] = match ty {
            Ty::Float => &["f1", "f2", "f3", "K1"],
            Ty::Vector => &["v1", "v2", "KV"],
            Ty::Str => &["s1", "s2"],
            Ty::Entity => &["e1", "e2"],
        };
        let mut all: Vec<String> = globals.iter().map(|s| (*s).to_owned()).collect();
        if ty == Ty::Float {
            all.extend(["v1_x", "v2_y", "KV_z"].map(str::to_owned));
        }
        for (t, name) in body.locals.iter().chain(&body.params) {
            if *t == ty {
                all.push(name.clone());
            }
        }
        all
    }

    fn literal(&mut self, ty: Ty) -> String {
        let text = match ty {
            Ty::Float => self
                .rng
                .pick(&["0", "1", "-0", "2.5", "100", "-1", "3", "0.1"]),
            Ty::Vector => self
                .rng
                .pick(&["'1 2 3'", "'0 0 0'", "'-1 0.5 2'", "'1 2 3'"]),
            Ty::Str => self
                .rng
                .pick(&["\"a\"", "\"b\"", "\"\"", "\"a\\nb\"", "\"a\""]),
            Ty::Entity => return self.rng.pick(&["e1", "e2"]).to_owned(),
        };
        text.to_owned()
    }

    fn field(&mut self, ty: Ty) -> &'static str {
        match ty {
            Ty::Float => self.rng.pick(&["ff1", "ff2", "fv_x", "fv_z"]),
            Ty::Vector => "fv",
            Ty::Str => "fs",
            Ty::Entity => "fe",
        }
    }

    /// An expression of type `ty`, or now and then of another.
    fn expr(&mut self, body: &Body, ty: Ty) -> String {
        let ty = if self.rng.chance(1) {
            TYPES.get(self.rng.below(4)).copied().unwrap_or(ty)
        } else {
            ty
        };
        if self.depth > 3 {
            return self.leaf(body, ty);
        }
        self.depth = self.depth.saturating_add(1);
        let e = self.compound(body, ty);
        self.depth = self.depth.saturating_sub(1);
        e
    }

    fn leaf(&mut self, body: &Body, ty: Ty) -> String {
        match self.rng.below(3) {
            0 => self.literal(ty),
            1 => {
                let entity = self.rng.pick(&["e1", "e2"]);
                format!("{entity}.{}", self.field(ty))
            }
            _ => {
                let vars = self.variables(body, ty);
                vars.get(self.rng.below(vars.len()))
                    .cloned()
                    .unwrap_or_else(|| self.literal(ty))
            }
        }
    }

    fn compound(&mut self, body: &Body, ty: Ty) -> String {
        match ty {
            Ty::Float => match self.rng.below(12) {
                0 => {
                    let op = self.rng.pick(&["+", "-", "*", "/", "&", "|", "&&", "||"]);
                    let (a, b) = (self.expr(body, Ty::Float), self.expr(body, Ty::Float));
                    format!("{a} {op} {b}")
                }
                1 => {
                    let op = self.rng.pick(&["<", ">", "<=", ">=", "==", "!="]);
                    let (a, b) = (self.expr(body, Ty::Float), self.expr(body, Ty::Float));
                    format!("({a} {op} {b})")
                }
                2 => {
                    let t = TYPES.get(self.rng.below(4)).copied().unwrap_or(Ty::Float);
                    let op = self.rng.pick(&["==", "!="]);
                    let (a, b) = (self.expr(body, t), self.expr(body, t));
                    format!("({a} {op} {b})")
                }
                3 => {
                    let t = TYPES.get(self.rng.below(4)).copied().unwrap_or(Ty::Float);
                    format!("!{}", self.leaf(body, t))
                }
                4 => {
                    let (a, b) = (self.expr(body, Ty::Vector), self.expr(body, Ty::Vector));
                    format!("({a}) * ({b})")
                }
                5 => {
                    let (a, b) = (self.expr(body, Ty::Float), self.expr(body, Ty::Float));
                    format!("add({a}, {b})")
                }
                6 => "rnd()".to_owned(),
                7 => format!("({})", self.expr(body, Ty::Float)),
                8 => format!("e1.fe.{}", self.field(Ty::Float)),
                _ => self.leaf(body, Ty::Float),
            },
            Ty::Vector => match self.rng.below(7) {
                0 => {
                    let op = self.rng.pick(&["+", "-"]);
                    let (a, b) = (self.expr(body, Ty::Vector), self.expr(body, Ty::Vector));
                    format!("{a} {op} {b}")
                }
                1 => {
                    let (a, b) = (self.expr(body, Ty::Vector), self.expr(body, Ty::Float));
                    format!("({a}) * ({b})")
                }
                2 => {
                    let (a, b) = (self.expr(body, Ty::Float), self.expr(body, Ty::Vector));
                    format!("({a}) * ({b})")
                }
                3 => format!("vfn({})", self.expr(body, Ty::Vector)),
                _ => self.leaf(body, Ty::Vector),
            },
            Ty::Str => match self.rng.below(4) {
                0 => "sfn()".to_owned(),
                _ => self.leaf(body, Ty::Str),
            },
            Ty::Entity => self.leaf(body, Ty::Entity),
        }
    }

    /// A place of type `ty` to assign to.
    fn target(&mut self, body: &Body, ty: Ty) -> String {
        if ty != Ty::Entity && self.rng.chance(30) {
            let entity = self.rng.pick(&["e1", "e2"]);
            return format!("{entity}.{}", self.field(ty));
        }
        let vars: Vec<String> = self
            .variables(body, ty)
            .into_iter()
            .filter(|v| v != "K1" && v != "KV" && v != "KV_z")
            .collect();
        vars.get(self.rng.below(vars.len()))
            .cloned()
            .unwrap_or_else(|| "f1".to_owned())
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push('\t');
        }
    }

    fn statement(&mut self, body: &mut Body) {
        self.indent();
        let choice = if self.depth > 2 {
            self.rng.below(4)
        } else {
            self.rng.below(11)
        };
        let ty = TYPES.get(self.rng.below(4)).copied().unwrap_or(Ty::Float);
        match choice {
            0 | 1 => {
                let target = self.target(body, ty);
                let value = self.expr(body, ty);
                self.out.push_str(&format!("{target} = {value};\n"));
            }
            2 => {
                let mut args = Vec::new();
                for _ in 0..self.rng.below(5) {
                    let t = TYPES.get(self.rng.below(4)).copied().unwrap_or(Ty::Float);
                    args.push(self.expr(body, t));
                }
                self.out.push_str(&format!("pr({});\n", args.join(", ")));
            }
            3 => {
                let a = self.target(body, Ty::Float);
                let b = self.target(body, Ty::Float);
                let value = self.expr(body, Ty::Float);
                self.out.push_str(&format!("{a} = {b} = {value};\n"));
            }
            4 => {
                let name = format!("l{}", self.next_local);
                self.next_local = self.next_local.saturating_add(1);
                let (word, literal) = match ty {
                    Ty::Float => ("float", "1"),
                    Ty::Vector => ("vector", "'1 2 3'"),
                    // QC-27.12: id's qcc stores a stale word for a string initialiser.
                    Ty::Str => ("string", ""),
                    Ty::Entity => ("entity", ""),
                };
                if literal.is_empty() || self.rng.chance(50) {
                    self.out.push_str(&format!("local {word} {name};\n"));
                } else {
                    self.out
                        .push_str(&format!("local {word} {name} = {literal};\n"));
                }
                body.locals.push((ty, name));
            }
            5 => {
                let cond = self.expr(body, ty);
                self.out.push_str(&format!("if ({cond})\n"));
                self.nested(body);
                if self.rng.chance(40) {
                    self.indent();
                    self.out.push_str("else\n");
                    self.nested(body);
                }
            }
            6 => {
                let cond = self.expr(body, ty);
                self.out.push_str(&format!("while ({cond})\n"));
                self.nested(body);
            }
            7 => {
                self.out.push_str("do\n");
                self.nested(body);
                self.indent();
                let cond = self.expr(body, ty);
                self.out.push_str(&format!("while ({cond});\n"));
            }
            8 => {
                self.out.push_str("{\n");
                self.depth = self.depth.saturating_add(1);
                for _ in 0..=self.rng.below(3) {
                    self.statement(body);
                }
                self.depth = self.depth.saturating_sub(1);
                self.indent();
                self.out.push_str("}\n");
            }
            9 => {
                let entity = self.rng.pick(&["e1", "e2"]);
                self.out.push_str(&format!("{entity}.fth();\n"));
            }
            _ => {
                let value = self.expr(body, ty);
                self.out.push_str(&format!("return {value};\n"));
            }
        }
    }

    fn nested(&mut self, body: &mut Body) {
        self.depth = self.depth.saturating_add(1);
        self.statement(body);
        self.depth = self.depth.saturating_sub(1);
    }

    fn function(&mut self, index: usize) {
        let mut body = Body {
            locals: Vec::new(),
            params: Vec::new(),
        };
        let mut params = Vec::new();
        for p in 0..self.rng.below(3) {
            let ty = TYPES.get(self.rng.below(3)).copied().unwrap_or(Ty::Float);
            let word = match ty {
                Ty::Float => "float",
                Ty::Vector => "vector",
                Ty::Str => "string",
                Ty::Entity => "entity",
            };
            let name = format!("p{index}_{p}");
            params.push(format!("{word} {name}"));
            body.params.push((ty, name));
        }
        let state = if params.is_empty() && self.rng.chance(25) {
            let frame = self.rng.pick(&["$w1", "$w3", "4"]);
            format!("[{frame}, th1] ")
        } else {
            String::new()
        };
        self.out.push_str(&format!(
            "void({}) fn{index} = {state}{{\n",
            params.join(", ")
        ));
        self.depth = 1;
        for _ in 0..=self.rng.below(8) {
            self.statement(&mut body);
        }
        self.depth = 0;
        self.out.push_str("};\n");
    }
}

/// Program `n` of the generated family.
pub(crate) fn program(n: usize) -> String {
    let seed = u64::try_from(n)
        .unwrap_or(0)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0x1234_5678);
    let mut generator = Gen {
        rng: Rng(seed | 1),
        out: String::from(PRELUDE),
        depth: 0,
        next_local: 0,
    };
    for index in 0..=generator.rng.below(3) {
        generator.function(index);
    }
    generator.out.push_str("void() th1 = {};\n");
    generator.out
}
