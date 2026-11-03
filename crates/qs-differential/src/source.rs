// SPDX-License-Identifier: GPL-3.0-or-later

//! A seeded generator of QuakeC source programs for the compiler differential (DIFF-14): each
//! program is well typed in the FTEQCC dialect, so the pinned FTEQCC and `qc-codegen` both
//! compile it, and its functions are called by the DIFF-12 comparison. The forms a program
//! combines are counted by name, so a run can show that every form DIFF-14.1 names appears.
//!
//! Every loop is bounded by a counter the program itself keeps, so no call runs away; every
//! array index is masked into the array; and no builtin that needs the engine's state beyond
//! the mock's is called, so the two progs' calls end the same way.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_types::Etype;

use crate::calls::Rng;

/// A generated program: its source, the forms it uses and how often each appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The whole program: the system definitions, the entry points, the generated
    /// declarations and functions, and the padding.
    pub source: String,
    /// Each form of DIFF-14.1 the program uses, with how many times.
    pub forms: BTreeMap<&'static str, usize>,
}

/// The forms DIFF-14.1 names, each counted when a program uses it.
pub const FORMS: [&str; 40] = [
    "global",
    "field",
    "constant",
    "enum",
    "array",
    "struct",
    "function",
    "default-parameter",
    "variadic",
    "local",
    "assign",
    "compound-assign",
    "field-assign",
    "array-set",
    "array-get",
    "struct-member",
    "vector-component",
    "vector-literal",
    "string-literal",
    "arithmetic",
    "comparison",
    "logic",
    "bitwise",
    "not",
    "negate",
    "increment",
    "ternary",
    "call",
    "builtin-call",
    "if",
    "if-else",
    "while",
    "do-while",
    "for",
    "switch",
    "break",
    "continue",
    "return",
    "dot-product",
    "vector-scale",
];

/// How many functions a program has, at most.
const FUNCTIONS: usize = 6;

/// How deep an expression nests, at most.
const DEPTH: usize = 3;

/// The builtins a program may call, as id's `defs.qc` declares them: each is pure or reads
/// only the mock's state, so both progs' calls agree.
const BUILTINS: &str = "void(string e) error = #10;\nfloat(vector v) vlen = #12;\nvector(vector v) normalize = #9;\n\
float(vector v) vectoyaw = #13;\nfloat(float v) rint = #36;\nfloat(float v) floor = #37;\n\
float(float v) ceil = #38;\nfloat(float f) fabs = #43;\nstring(float f) ftos = #26;\n\
string(vector v) vtos = #27;\n";

/// The bodies of the engine's entry points (EDICT-9.1), so the setup runs.
const ENTRY_POINTS: &str = "void() main = {};\nvoid() StartFrame = {};\n\
void() info_notnull = {};\nvoid() PlayerPreThink = {};\nvoid() PlayerPostThink = {};\n\
void() ClientKill = {};\nvoid() ClientConnect = {};\nvoid() PutClientInServer = {};\n\
void() ClientDisconnect = {};\nvoid() SetNewParms = {};\nvoid() SetChangeParms = {};\n";

/// Globals after the program's own: a parameter passed with `STORE_V` reads three slots, so a
/// one-word immediate passed last would read past the globals without them (VM-1.4).
const PADDING: &str = "vector padding;\n";

/// The basic types a generated value has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Float,
    Vector,
    Str,
    Entity,
}

const TYPES: [Ty; 4] = [Ty::Float, Ty::Vector, Ty::Str, Ty::Entity];

impl Ty {
    const fn word(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Vector => "vector",
            Self::Str => "string",
            Self::Entity => "entity",
        }
    }

    /// The zero of the type, which a local is set to before it is read.
    const fn zero(self) -> &'static str {
        match self {
            Self::Float => "0",
            Self::Vector => "'0 0 0'",
            Self::Str => "\"\"",
            Self::Entity => "world",
        }
    }

    /// The prefix of the globals, fields, locals and parameters of this type.
    const fn tag(self) -> &'static str {
        match self {
            Self::Float => "f",
            Self::Vector => "v",
            Self::Str => "s",
            Self::Entity => "e",
        }
    }
}

/// A name in scope with its type.
#[derive(Clone, Debug)]
struct Name {
    text: String,
    ty: Ty,
}

/// A function the program defines, as later functions may call it.
#[derive(Clone, Debug)]
struct Function {
    name: String,
    params: Vec<Ty>,
    ret: Ty,
}

/// The generator's state for one program.
struct Gen {
    rng: Rng,
    out: String,
    forms: BTreeMap<&'static str, usize>,
    globals: Vec<Name>,
    fields: Vec<Name>,
    constants: Vec<Name>,
    functions: Vec<Function>,
    /// The locals and parameters of the function being written.
    locals: Vec<Name>,
    /// The loop counters in scope, each bounded by the loop that made it.
    in_loop: usize,
    /// Nesting of statements, to stop the bodies growing without bound.
    depth: usize,
}

/// The system part of a `defs.qc`: the globals of EDICT-2.1 and the fields of EDICT-3.1, in
/// slot order, each section closed as id's `defs.qc` closes it.
fn system_defs() -> String {
    let type_name = |etype: Etype| match etype {
        Etype::Float => "float",
        Etype::Vector => "vector",
        Etype::String => "string",
        Etype::Entity => "entity",
        Etype::Function => "void()",
        Etype::Void | Etype::Field | Etype::Pointer => "void",
    };
    let mut text = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(text, "{} {};", type_name(var.etype), var.name).unwrap_or(());
    }
    text.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(text, ".{} {};", type_name(var.etype), var.name).unwrap_or(());
    }
    text.push_str("void end_sys_fields;\n");
    text.push_str(BUILTINS);
    text
}

/// The program number `n` of the seed `seed` (DIFF-14.1): the same text for the same pair.
#[must_use]
pub fn program(seed: u64, n: u64) -> Program {
    let mut generator = Gen {
        rng: Rng::new(&seed.to_le_bytes(), u32::try_from(n).unwrap_or(u32::MAX)),
        out: String::new(),
        forms: BTreeMap::new(),
        globals: Vec::new(),
        fields: Vec::new(),
        constants: Vec::new(),
        functions: Vec::new(),
        locals: Vec::new(),
        in_loop: 0,
        depth: 0,
    };
    generator.declarations();
    let count = generator.below(FUNCTIONS).saturating_add(1);
    for i in 0..count {
        generator.function(i);
    }
    let mut source = system_defs();
    source.push_str(ENTRY_POINTS);
    source.push_str(&generator.out);
    source.push_str(PADDING);
    Program {
        source,
        forms: generator.forms,
    }
}

impl Gen {
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.rng.below(u64::try_from(bound).unwrap_or(u64::MAX))).unwrap_or(0)
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn form(&mut self, name: &'static str) {
        let entry = self.forms.entry(name).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// One of `items`, `default` when there are none.
    fn choose<T: Copy>(&mut self, items: &[T], default: T) -> T {
        self.rng.pick(items).copied().unwrap_or(default)
    }

    /// One of `names`, or the first float global when there are none, which there always is.
    fn pick_name(&mut self, names: &[Name]) -> Name {
        self.rng.pick(names).cloned().unwrap_or_else(|| Name {
            text: "gf0".to_owned(),
            ty: Ty::Float,
        })
    }

    fn pick_type(&mut self) -> Ty {
        self.choose(&TYPES, Ty::Float)
    }

    // -- declarations --------------------------------------------------------------------

    fn declarations(&mut self) {
        for ty in TYPES {
            let count = self.below(3).saturating_add(1);
            for i in 0..count {
                let text = format!("g{}{i}", ty.tag());
                writeln!(self.out, "{} {text};", ty.word()).unwrap_or(());
                self.globals.push(Name { text, ty });
                self.form("global");
            }
        }
        for ty in TYPES {
            let text = format!("f{}", ty.tag());
            writeln!(self.out, ".{} {text};", ty.word()).unwrap_or(());
            self.fields.push(Name { text, ty });
            self.form("field");
        }
        let count = self.below(3).saturating_add(1);
        for i in 0..count {
            let value = self.rng.float();
            let text = format!("K{i}");
            writeln!(self.out, "const float {text} = {};", literal(value)).unwrap_or(());
            self.constants.push(Name {
                text,
                ty: Ty::Float,
            });
            self.form("constant");
        }
        if self.chance(50) {
            self.out.push_str("enum { EN_A, EN_B = 3, EN_C };\n");
            for text in ["EN_A", "EN_B", "EN_C"] {
                self.constants.push(Name {
                    text: text.to_owned(),
                    ty: Ty::Float,
                });
            }
            self.form("enum");
        }
        self.out.push_str("float arr[4];\nstring sarr[3];\n");
        self.form("array");
        self.out
            .push_str("typedef struct { float a; vector v; string s; } S;\nS gst;\n");
        self.form("struct");
    }

    // -- functions -----------------------------------------------------------------------

    fn function(&mut self, index: usize) {
        let params: Vec<Ty> = (0..self.below(4)).map(|_| self.pick_type()).collect();
        let ret = self.pick_type();
        let name = format!("fn{index}");
        self.locals.clear();
        let mut head = format!("{}(", ret.word());
        for (i, ty) in params.iter().enumerate() {
            if i > 0 {
                head.push_str(", ");
            }
            let text = format!("p{}{i}", ty.tag());
            write!(head, "{} {text}", ty.word()).unwrap_or(());
            // A trailing parameter may carry a default (FTE-18.7).
            if i.saturating_add(1) == params.len() && *ty == Ty::Float && self.chance(30) {
                write!(head, " = {}", literal(self.rng.float())).unwrap_or(());
                self.form("default-parameter");
            }
            self.locals.push(Name { text, ty: *ty });
        }
        let variadic = params.len() < 4 && self.chance(15);
        if variadic {
            if !params.is_empty() {
                head.push_str(", ");
            }
            head.push_str("...");
            self.form("variadic");
        }
        writeln!(self.out, "{head}) {name} = {{").unwrap_or(());
        self.form("function");
        let locals = self.below(4);
        for i in 0..locals {
            let ty = self.pick_type();
            let text = format!("l{}{i}", ty.tag());
            writeln!(self.out, "\tlocal {} {text};", ty.word()).unwrap_or(());
            self.locals.push(Name { text, ty });
            self.form("local");
        }
        // The counter every loop of this function bounds itself with.
        // One counter per loop nesting depth, so a loop inside another never resets the
        // outer loop's bound.
        self.out
            .push_str("\tlocal float n, m, k;\n\tn = 0;\n\tm = 0;\n\tk = 0;\n");
        // A local starts with whatever its slot held (VM-15.7), which the two layouts need not
        // share, so each is stored before it is read.
        for i in 0..locals {
            let Some(local) = self.locals.get(params.len().saturating_add(i)) else {
                break;
            };
            writeln!(self.out, "\t{} = {};", local.text, local.ty.zero()).unwrap_or(());
        }
        let statements = self.below(6).saturating_add(1);
        self.depth = 0;
        for _ in 0..statements {
            self.statement();
        }
        let value = self.expr(ret, 0);
        writeln!(self.out, "\treturn {value};\n}};").unwrap_or(());
        self.form("return");
        self.functions.push(Function { name, params, ret });
    }

    /// The counter of the loop about to open, by its nesting depth: `n`, `m` or `k`.
    fn counter(&self) -> &'static str {
        ["n", "m", "k"].get(self.in_loop).copied().unwrap_or("k")
    }

    /// The counter of the innermost open loop.
    fn loop_counter(&self) -> &'static str {
        ["n", "m", "k"]
            .get(self.in_loop.saturating_sub(1))
            .copied()
            .unwrap_or("k")
    }

    // -- statements ----------------------------------------------------------------------

    fn statement(&mut self) {
        let indent = "\t".repeat(self.depth.saturating_add(1));
        let choice = if self.depth >= 2 {
            self.below(6)
        } else {
            self.below(12)
        };
        match choice {
            0 | 1 => {
                let target = self.lvalue();
                let value = self.expr(target.ty, 0);
                writeln!(self.out, "{indent}{} = {value};", target.text).unwrap_or(());
                self.form("assign");
            }
            2 => {
                let target = self.float_lvalue();
                let op = self.choose(&["+=", "-=", "*=", "|=", "&="], "+=");
                let value = self.expr(Ty::Float, 1);
                writeln!(self.out, "{indent}{target} {op} {value};").unwrap_or(());
                self.form("compound-assign");
            }
            3 => {
                let entity = self.expr(Ty::Entity, 2);
                let fields = self.fields.clone();
                let field = self.pick_name(&fields);
                let value = self.expr(field.ty, 1);
                writeln!(self.out, "{indent}{entity}.{} = {value};", field.text).unwrap_or(());
                self.form("field-assign");
            }
            4 => {
                let index = self.expr(Ty::Float, 2);
                if self.chance(50) {
                    let value = self.expr(Ty::Float, 1);
                    writeln!(self.out, "{indent}arr[({index}) & 3] = {value};").unwrap_or(());
                } else {
                    let value = self.expr(Ty::Str, 1);
                    writeln!(self.out, "{indent}sarr[({index}) & 1] = {value};").unwrap_or(());
                }
                self.form("array-set");
            }
            5 => {
                let target = self.float_lvalue();
                let op = self.choose(&["++", "--"], "++");
                writeln!(self.out, "{indent}{target}{op};").unwrap_or(());
                self.form("increment");
            }
            6 => {
                let condition = self.expr(Ty::Float, 1);
                writeln!(self.out, "{indent}if ({condition}) {{").unwrap_or(());
                self.block();
                if self.chance(50) {
                    writeln!(self.out, "{indent}}} else {{").unwrap_or(());
                    self.block();
                    self.form("if-else");
                } else {
                    self.form("if");
                }
                writeln!(self.out, "{indent}}}").unwrap_or(());
            }
            7 => {
                let bound = self.below(3).saturating_add(1);
                let condition = self.expr(Ty::Float, 1);
                let n = self.counter();
                writeln!(
                    self.out,
                    "{indent}while ({n} < {bound} && ({condition})) {{\n{indent}\t{n} = {n} + 1;"
                )
                .unwrap_or(());
                self.in_loop = self.in_loop.saturating_add(1);
                self.block();
                self.in_loop = self.in_loop.saturating_sub(1);
                writeln!(self.out, "{indent}}}").unwrap_or(());
                self.form("while");
            }
            8 => {
                let bound = self.below(3).saturating_add(1);
                let n = self.counter();
                writeln!(self.out, "{indent}do {{\n{indent}\t{n} = {n} + 1;").unwrap_or(());
                self.in_loop = self.in_loop.saturating_add(1);
                self.block();
                self.in_loop = self.in_loop.saturating_sub(1);
                writeln!(self.out, "{indent}}} while ({n} < {bound});").unwrap_or(());
                self.form("do-while");
            }
            9 => {
                let bound = self.below(3).saturating_add(1);
                let n = self.counter();
                writeln!(self.out, "{indent}for ({n} = 0; {n} < {bound}; {n}++) {{").unwrap_or(());
                self.in_loop = self.in_loop.saturating_add(1);
                self.block();
                self.in_loop = self.in_loop.saturating_sub(1);
                writeln!(self.out, "{indent}}}").unwrap_or(());
                self.form("for");
            }
            10 => {
                // A comparison is a bool to FTEQCC, which a float case does not match
                // ("Implicit type mismatch. Needed bool, got float."), so the value is a name.
                let value = self.leaf(Ty::Float);
                writeln!(self.out, "{indent}switch ({value}) {{").unwrap_or(());
                let cases = self.below(3).saturating_add(1);
                for i in 0..cases {
                    writeln!(self.out, "{indent}case {i}:").unwrap_or(());
                    self.block();
                    if self.chance(70) {
                        writeln!(self.out, "{indent}\tbreak;").unwrap_or(());
                        self.form("break");
                    }
                }
                if self.chance(50) {
                    writeln!(self.out, "{indent}default:").unwrap_or(());
                    self.block();
                }
                writeln!(self.out, "{indent}}}").unwrap_or(());
                self.form("switch");
            }
            _ => {
                let callee = self.expr(Ty::Float, 1);
                writeln!(self.out, "{indent}{callee};").unwrap_or(());
            }
        }
        if self.in_loop > 0 && self.chance(10) {
            let word = self.choose(&["break", "continue"], "break");
            let n = self.loop_counter();
            writeln!(self.out, "{indent}if ({n} > 1) {word};").unwrap_or(());
            self.form(word);
        }
    }

    /// A nested block of one to three statements.
    fn block(&mut self) {
        self.depth = self.depth.saturating_add(1);
        let count = self.below(3).saturating_add(1);
        for _ in 0..count {
            self.statement();
        }
        self.depth = self.depth.saturating_sub(1);
    }

    /// A global or local of any type, as the target of an assignment.
    fn lvalue(&mut self) -> Name {
        let pool: Vec<Name> = self
            .globals
            .iter()
            .chain(self.locals.iter())
            .cloned()
            .collect();
        self.pick_name(&pool).clone()
    }

    /// A float global, local, array element or struct member, as a target.
    fn float_lvalue(&mut self) -> String {
        match self.below(4) {
            0 => {
                let index = self.expr(Ty::Float, 2);
                self.form("array-set");
                format!("arr[({index}) & 3]")
            }
            1 => {
                self.form("struct-member");
                "gst.a".to_owned()
            }
            _ => {
                let floats: Vec<Name> = self
                    .globals
                    .iter()
                    .chain(self.locals.iter())
                    .filter(|n| n.ty == Ty::Float)
                    .cloned()
                    .collect();
                self.pick_name(&floats).text.clone()
            }
        }
    }

    // -- expressions ---------------------------------------------------------------------

    /// An expression of type `ty`, nested `depth` deep so far.
    fn expr(&mut self, ty: Ty, depth: usize) -> String {
        if depth >= DEPTH {
            return self.leaf(ty);
        }
        match ty {
            Ty::Float => self.float_expr(depth),
            Ty::Vector => self.vector_expr(depth),
            Ty::Str => self.string_expr(depth),
            Ty::Entity => self.entity_expr(depth),
        }
    }

    /// A name of type `ty` in scope, a literal, or a field read.
    fn leaf(&mut self, ty: Ty) -> String {
        let names: Vec<Name> = self
            .globals
            .iter()
            .chain(self.locals.iter())
            .filter(|n| n.ty == ty)
            .cloned()
            .collect();
        match self.below(4) {
            0 => {
                let entity = if self.chance(50) { "self" } else { "world" };
                let field = self.fields.iter().find(|f| f.ty == ty).cloned();
                match field {
                    Some(field) => format!("{entity}.{}", field.text),
                    None => self.pick_name(&names).text.clone(),
                }
            }
            1 => match ty {
                Ty::Float => {
                    if self.chance(40) && !self.constants.is_empty() {
                        let constants = self.constants.clone();
                        self.pick_name(&constants).text
                    } else {
                        literal(self.rng.float())
                    }
                }
                Ty::Vector => {
                    self.form("vector-literal");
                    format!(
                        "'{} {} {}'",
                        literal(self.rng.float()),
                        literal(self.rng.float()),
                        literal(self.rng.float())
                    )
                }
                Ty::Str => {
                    self.form("string-literal");
                    format!("\"{}\"", self.choose(&["", "a", "quake", "x y"], ""))
                }
                Ty::Entity => if self.chance(50) { "self" } else { "world" }.to_owned(),
            },
            _ => self.pick_name(&names).text.clone(),
        }
    }

    fn float_expr(&mut self, depth: usize) -> String {
        match self.below(14) {
            0 => {
                let (a, b) = (
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                );
                let op = self.choose(&["+", "-", "*", "/"], "+");
                self.form("arithmetic");
                format!("({a} {op} {b})")
            }
            1 => {
                let ty = self.pick_type();
                let (a, b) = (
                    self.expr(ty, depth.saturating_add(1)),
                    self.expr(ty, depth.saturating_add(1)),
                );
                let op = if ty == Ty::Float {
                    self.choose(&["==", "!=", "<", ">", "<=", ">="], "==")
                } else {
                    self.choose(&["==", "!="], "==")
                };
                self.form("comparison");
                format!("({a} {op} {b})")
            }
            2 => {
                let (a, b) = (
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                );
                let op = self.choose(&["&&", "||"], "&&");
                self.form("logic");
                format!("({a} {op} {b})")
            }
            3 => {
                let (a, b) = (
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                );
                let op = self.choose(&["&", "|"], "&");
                self.form("bitwise");
                format!("({a} {op} {b})")
            }
            4 => {
                let ty = self.pick_type();
                let a = self.expr(ty, depth.saturating_add(1));
                self.form("not");
                format!("(!{a})")
            }
            5 => {
                let a = self.expr(Ty::Float, depth.saturating_add(1));
                self.form("negate");
                format!("-({a})")
            }
            6 => {
                let (c, a, b) = (
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                );
                self.form("ternary");
                format!("({c} ? {a} : {b})")
            }
            7 => {
                let v = self.expr(Ty::Vector, depth.saturating_add(1));
                let component = self.choose(&[".x", ".y", ".z"], ".x");
                self.form("vector-component");
                format!("({v}){component}")
            }
            8 => {
                let (a, b) = (
                    self.expr(Ty::Vector, depth.saturating_add(1)),
                    self.expr(Ty::Vector, depth.saturating_add(1)),
                );
                self.form("dot-product");
                format!("({a} * {b})")
            }
            9 => {
                let index = self.expr(Ty::Float, depth.saturating_add(1));
                self.form("array-get");
                format!("arr[({index}) & 3]")
            }
            10 => {
                self.form("struct-member");
                "gst.a".to_owned()
            }
            11 => self.builtin(Ty::Float, depth),
            12 => self.call(Ty::Float, depth),
            _ => self.leaf(Ty::Float),
        }
    }

    fn vector_expr(&mut self, depth: usize) -> String {
        match self.below(7) {
            0 => {
                let (a, b) = (
                    self.expr(Ty::Vector, depth.saturating_add(1)),
                    self.expr(Ty::Vector, depth.saturating_add(1)),
                );
                let op = self.choose(&["+", "-"], "+");
                self.form("arithmetic");
                format!("({a} {op} {b})")
            }
            1 => {
                let (v, f) = (
                    self.expr(Ty::Vector, depth.saturating_add(1)),
                    self.expr(Ty::Float, depth.saturating_add(1)),
                );
                self.form("vector-scale");
                if self.chance(50) {
                    format!("({v} * {f})")
                } else {
                    format!("({f} * {v})")
                }
            }
            2 => {
                self.form("struct-member");
                "gst.v".to_owned()
            }
            3 => self.builtin(Ty::Vector, depth),
            4 => self.call(Ty::Vector, depth),
            _ => self.leaf(Ty::Vector),
        }
    }

    fn string_expr(&mut self, depth: usize) -> String {
        match self.below(6) {
            0 => {
                let index = self.expr(Ty::Float, depth.saturating_add(1));
                self.form("array-get");
                format!("sarr[({index}) & 1]")
            }
            1 => {
                self.form("struct-member");
                "gst.s".to_owned()
            }
            2 => self.builtin(Ty::Str, depth),
            3 => self.call(Ty::Str, depth),
            _ => self.leaf(Ty::Str),
        }
    }

    fn entity_expr(&mut self, depth: usize) -> String {
        match self.below(4) {
            0 => {
                let (c, a, b) = (
                    self.expr(Ty::Float, depth.saturating_add(1)),
                    self.expr(Ty::Entity, depth.saturating_add(1)),
                    self.expr(Ty::Entity, depth.saturating_add(1)),
                );
                self.form("ternary");
                format!("({c} ? {a} : {b})")
            }
            1 => self.call(Ty::Entity, depth),
            _ => self.leaf(Ty::Entity),
        }
    }

    /// A call of one of the builtins [`BUILTINS`] declares that returns `ty`.
    fn builtin(&mut self, ty: Ty, depth: usize) -> String {
        let (name, arg) = match ty {
            Ty::Float => match self.below(6) {
                0 => ("vlen", Ty::Vector),
                1 => ("vectoyaw", Ty::Vector),
                2 => ("rint", Ty::Float),
                3 => ("floor", Ty::Float),
                4 => ("ceil", Ty::Float),
                _ => ("fabs", Ty::Float),
            },
            Ty::Vector => ("normalize", Ty::Vector),
            Ty::Str => {
                if self.chance(50) {
                    ("ftos", Ty::Float)
                } else {
                    ("vtos", Ty::Vector)
                }
            }
            Ty::Entity => return self.leaf(Ty::Entity),
        };
        let value = self.expr(arg, depth.saturating_add(1));
        self.form("builtin-call");
        format!("{name}({value})")
    }

    /// A call of an earlier function of the program that returns `ty`, with an argument of
    /// each parameter's type; a leaf when there is none.
    fn call(&mut self, ty: Ty, depth: usize) -> String {
        let candidates: Vec<Function> = self
            .functions
            .iter()
            .filter(|f| f.ret == ty)
            .cloned()
            .collect();
        let Some(function) = self.rng.pick(&candidates).cloned() else {
            return self.leaf(ty);
        };
        let args: Vec<String> = function
            .params
            .iter()
            .map(|p| self.expr(*p, depth.saturating_add(1)))
            .collect();
        self.form("call");
        format!("{}({})", function.name, args.join(", "))
    }
}

/// A float as QuakeC source writes it, whole numbers without a fraction.
fn literal(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1.0e9 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::{FORMS, program};

    // spec DIFF-14.1: a program is the same text for the same seed and number, and another
    // for another number
    #[test]
    fn a_program_is_fixed_by_its_seed_and_number() {
        assert_eq!(program(1, 0), program(1, 0));
        assert_ne!(program(1, 0).source, program(1, 1).source);
        assert_ne!(program(1, 0).source, program(2, 0).source);
    }

    // spec DIFF-14.1: over a run of programs every form appears
    #[test]
    fn every_form_appears_over_a_run() {
        let mut seen = std::collections::BTreeSet::new();
        for n in 0..300 {
            seen.extend(program(1, n).forms.keys().copied());
        }
        let missing: Vec<&str> = FORMS
            .iter()
            .copied()
            .filter(|f| !seen.contains(f))
            .collect();
        assert!(missing.is_empty(), "forms never generated: {missing:?}");
    }
}
