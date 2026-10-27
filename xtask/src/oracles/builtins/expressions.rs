// SPDX-License-Identifier: GPL-3.0-or-later

//! The expression family of the builtin differential: programs whose outcome the semantic
//! model's check stage decides (`docs/spec/dialect-fteqcc.md` FTEF-23 to FTEF-29): the opcode
//! an operator is given and its conversions; member access on entities, classes, vectors,
//! strings, arrays, structs and pointers; constant folding; the emulation of opcodes the target
//! lacks; the comparison warnings; and assignments. Each is compiled as it stands and with every
//! warning an error, so a warning either compiler gives alone changes the outcome.

use qc_syntax::Dialect;

use super::Case;
use super::directives::BIND;

/// Locals of each basic type, opening a function the statements close.
const LOCALS: &str = "void() g = { local float a; local vector v; local string s; \
                      local entity e; local int i; ";

/// Definitions the member accesses read.
const FIELDS: &str = ".float health; .vector origin; float glob; entity self;\n";

/// Statements in a body of [`LOCALS`], after [`FIELDS`].
const STATEMENTS: &[&str] = &[
    // FTEF-23.4, FTEF-23.5: the opcode of the fewest conversions, mismatches and their casts.
    "a = a + a;",
    "v = v + v;",
    "a = v * v;",
    "v = a * v;",
    "a = a && s;",
    "a = s * a;",
    "a = e + s;",
    "a = v < v;",
    "i = i + i;",
    "a = a + i;",
    "s = s + a;",
    "a = e + s; a = s * a;",
    // FTEF-24.1 to FTEF-24.3, FTEF-24.6: entities, vectors and strings.
    "a = self.health + 1; v = self.origin * 2;",
    "a = self.origin * \"s\";",
    "a = self.glob;",
    "e.nothing = 1;",
    "a = v.x + v.y + v.z + v.r + v.b;",
    "a = v.w;",
    "a = v.q;",
    "a = \"abc\".length;",
    "a = s.length;",
    "a = \"abc\".size;",
    // FTEF-26: constant folding and its warnings.
    "a = 1 / 0;",
    "a = 3.5 | 1;",
    "a = 2 * 3 + a;",
    // FTEF-27: emulation on the standard target.
    "i = i * i;",
    "a = a % 2;",
    "a = a << 1;",
    "i = i << 1;",
    "v = v ^ v;",
    "a = a &~ 1;",
    "if (i < i) a = 1;",
    "a = !i;",
    // FTEF-28: the comparison warnings.
    "a = (s == s);",
    "a = (a && a);",
    "a = (a == 1 && 2 == 2);",
    // FTEF-29: assignments.
    "a += v;",
    "a *= v;",
    "s += s;",
    "v *= a;",
    "v |= v;",
    "a &~= 1;",
    "self.health *= 2;",
    "if (a = 1) a = 2;",
    "if ((a = 1)) a = 2;",
    "for (; a = 1;) a = 2;",
    "a++; a--; glob++;",
    "a++ = 3;",
    "(a = 2) = 3;",
    "1 = a;",
];

/// Programs that stand alone.
const PROGRAMS: &[&str] = &[
    // FTEF-23.4: the integer and string opcodes under the FTE target.
    "#pragma target fte\nvoid() g = { local float a; local string s; local int i; \
     i = i + i; a = a + i; s = s + a; };\n",
    // FTEF-17.14: a bare field in a member function is this.field.
    "entity self;\nclass C { float x; nonvirtual void() m; };\n\
     void() C::m = { local float a; a = x + 1; };\n",
    "entity self;\nclass C { float x; nonvirtual void() m; };\n\
     void() C::m = { local float a; a = x * \"s\"; };\n",
    // FTEF-24.1: a local hides a field of its name after `.`.
    ".string target;\nvoid(entity target) use = { target.target = \"\"; };\n",
    // FTEF-24.4, FTEF-24.5: structs and pointers.
    "struct pt { float px; vector pv; };\nvoid() g = { local pt p; local float a; \
     a = p.px + 1; a = p.pv * \"s\"; };\n",
    "void() g = { local float *q; local float a; a = q->x; };\n",
    // FTEF-24.6: a class's member by the base's class, and not on a plain entity.
    "entity self;\nclass W;\nclass V { float Visible; };\n\
     class W { nonvirtual float Visible(void); W m_next; virtual void Draw(void); };\n\
     float W::Visible(void) { return 1; };\n\
     void W::Draw(void) { W w = this; w = w.m_next; \
     if (w && w.Visible() && w.m_next.Visible()) { } };\n",
    "class A { float m_a; };\nentity e;\nvoid() g = { e.m_a = 1; e.nothing = 2; };\n",
    // FTEF-24.8: an array's length.
    "string styles[] = { \"a\", \"b\" };\n\
     void() g = { local float n; n = styles.length; n = n * 2; };\n",
    // FTEF-27.7, FTEF-27.10: unsigned and pointer emulations.
    "void() g = { local __uint32 u; u = u / u; if (u >= u) u = 1; };\n",
    "void() g = { local float *p; local int i; p = p + i; p = p - 1; };\n",
    "void() g = { local float *p; local float *q; local int i; i = p - q; };\n",
    "void() g = { local float *p, *q; local int i; i = p - q; };\n",
    "#pragma flag enable undefwordsize\nvoid() g = { local void *w; local int i; w = w + i; };\n",
    // FTEF-29: constants, arrays and `lax`.
    "const float k = 3;\nvoid() g = { k = 1; };\n",
    "const float k = 3;\nvoid() g = { k++; };\n",
    ".float fld; .vector fv;\nvoid() g = { fld = fv; };\n",
    "void() vf = { };\nvoid() g = { vf = vf; };\n",
    "void() g = { local float arr[4]; local float *p; arr = p; };\n",
    "#pragma flag enable lax\n.float fl; entity self;\n\
     void() g = { local float a; local vector v; a *= v; self.fl *= v; };\n",
];

/// The options under which every warning but "no references" (Q302), which the fixtures' unused
/// definitions get, is an error.
const STRICT: [&str; 3] = ["-Werror", "-Wno-Q208", "-Wno-Q302"];

/// Every case of the family, `f` bound before each program, each plain and with every warning
/// an error.
pub(super) fn cases() -> Vec<Case> {
    let mut sources: Vec<String> = STATEMENTS
        .iter()
        .map(|statement| format!("{BIND}{FIELDS}{LOCALS}{statement} }};\n"))
        .collect();
    sources.extend(PROGRAMS.iter().map(|program| format!("{BIND}{program}")));
    let mut cases = Vec::new();
    for source in sources {
        cases.push(Case::new(Dialect::Fteqcc, source.clone()));
        cases.push(Case {
            options: STRICT.map(str::to_owned).to_vec(),
            ..Case::new(Dialect::Fteqcc, source)
        });
    }
    cases
}
