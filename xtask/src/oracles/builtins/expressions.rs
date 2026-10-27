// SPDX-License-Identifier: GPL-3.0-or-later

//! The expression family of the builtin differential: programs whose outcome the semantic
//! model's check stage decides (`docs/spec/dialect-fteqcc.md` FTEF-6.13, FTEF-10.7, FTEF-10.8, FTEF-23 to FTEF-34): the opcode
//! an operator is given and its conversions; member access on entities, classes, vectors,
//! strings, arrays, structs and pointers; constant folding; the emulation of opcodes the target
//! lacks; the comparison warnings; assignments, terms, references and calls; and statements.
//! Each is compiled as it stands and with every warning an error, so a warning either compiler
//! gives alone changes the outcome.

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
    // FTEF-30: terms.
    "a = !e; a = !s; a = !v; a = ~s;",
    "a = ~e;",
    "a = -s;",
    "a = +s;",
    "a = -i;",
    "a = ~i;",
    "a = ++a; a = --a;",
    "a = --glob;",
    "a = ++s;",
    "e = *e;",
    "a = s ? 1 : 2; a = v ? 1 : 2; a = a ?: 2;",
    "a = a ? s : 2;",
    "a = a ? i : 2;",
    "a = (float)s;",
    "e = (entity)a;",
    "a = (float)i; i = (int)a;",
    "v = [a, s, 1];",
    "v = [i, a, 1]; v = [1, 2, 3];",
    "a = sizeof(float); a = sizeof(v); a = _length(\"ab\");",
    "a = _length(s);",
    "a = _length(a);",
    "a = !(void)a;",
    "a = (void)a ? 1 : 2;",
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
    // FTEF-32: calls.
    "float() random = #7; void(float x) f1 = {}; void(float x, float y = 2) d = {};\n\
     void(float x, optional float y) o = {}; void(...) va = {}; void() vf = {};\n\
     void() g = { local float a; local string s; local int i; d(1); o(1); a = random(1, 2); };\n",
    "void() g = { local float a; a = a(); };\n",
    "void(float x) f1 = {};\nvoid() g = { f1(1, 2); };\n",
    "void(float x) f1 = {};\nvoid() g = { f1(); };\n",
    "void(float x) f1 = {};\nvoid() g = { local string s; f1(s); };\n",
    "void(float x) f1 = {};\nvoid() g = { local int i; f1(i); };\n",
    "void(...) va = {};\nvoid() g = { va(1,2,3,4,5,6,7,8,9); };\n",
    "float() random = #7;\nvoid() g = { local float a; local string s; a = random(s); };\n",
    "void() g = { local string s; s = _(\"x\"); s = _(s); };\n",
    "void() vf = {};\nvoid() g = { local float a; a = vf(); };\n",
    "void() g = { local float a; local int i; local float y = i; };\n",
    "void() g = { local float a; local string s; local float y = a + s; };\n",
    "const float k = 3; void(__out float x) of = { x = 1; };\n\
     void() g = { local float a; of(a); of(1); };\n",
    "entity self, world; .float entnum; .string targetname;\n\
     entity(entity s, .__variant f, __variant m) findfloat = #98;\n\
     entity(entity s, .string f, string m) find = #18;\n\
     class C { nonvirtual void() m; };\n\
     void() C::m = { local entity x; x = find(world, ::targetname, \"a\"); \
     x = findfloat(world, ::entnum, 1); };\n",
    "entity self, world; .string targetname;\n\
     entity(entity s, .string f, string m) find = #18;\n\
     class C { nonvirtual void() m; };\n\
     void() C::m = { local entity x; x = find(world, targetname, \"a\"); };\n",
    // FTEF-31: references.
    "void() g = { local float a, arr[4]; local vector v, varr[4]; local int i; \
     a = arr[1]; a = v[1]; v = varr[1]; a = v[a]; };\n",
    "void() g = { local float a, arr[4]; a = arr[5]; };\n",
    "void() g = { local float a; local vector v; a = v[3]; };\n",
    "void() g = { local float a, arr[4]; local int i; a = arr[i]; arr[i] = 1; };\n",
    "void() g = { local float arr[4]; local int i; arr[i] += 1; arr[i]++; };\n",
    "void() g = { local vector v, varr[4]; local int i; v = varr[i]; };\n",
    "void() g = { local float a; a = a[1]; };\n",
    "void() g = { local float a; a = (a[1]); };\n",
    "void() g = { local float a; local float *p; a = *p; };\n",
    "void() g = { local float *p; local int *q; local int i; p[1] = 2; *q = 1; p[i] = 2; };\n",
    "void() g = { local float a; local string s; a = s[1]; };\n",
    "void() g = { local string s; s[1] = 65; };\n",
    "void() g = { local float a; local float *p; p = &a; };\n",
    "void() g = { local float arr[4]; local float *p; p = &arr; };\n",
    "void() g = { local float arr[4]; arr = arr; };\n",
    "entity e; .float fl;\nvoid() g = { local float *p; p = &e.fl; p = &p[1]; };\n",
    "#pragma target fte\n#pragma warning enable localptr\n\
     void() g = { local float a, arr[4]; local float *p; p = &a; p = &arr; p = &(a+a); };\n",
    "void() g = { local void *w; local float a; a = *w; };\n",
    "entity self;\nclass C { int arr[4]; nonvirtual void() m; };\n\
     void() C::m = { local int i; arr[i] = 0i; arr[1] = 0i; };\n",
    "#pragma flag enable lax\n\
     void() g = { local float arr[4]; local vector v; local int i; arr[i] *= v; };\n",
    // FTEF-30: terms under flags and types.
    "#pragma flag enable lax\nvoid() g = { local float a; a = !(void)a; };\n",
    "#pragma flag disable ifvector\nvoid() g = { local float a; local vector v; a = v ? 1 : 2; };\n",
    "#pragma warning enable F322\nvoid() g = { local float a; local string s; a = s ? 1 : 2; };\n",
    "#pragma flag enable undefwordsize\nvoid() g = { local float a; a = sizeof(float); };\n",
    "void() f1 = {}; void(float) f2 = {};\n\
     void() g = { local float a; local void() h; h = a ? f1 : f2; };\n",
    "class A {}; class B {};\n\
     void() g = { local float a; local A x; local B y; local entity e; e = a ? x : y; };\n",
    "const float k = 3;\nvoid() g = { local float a; a = --k; };\n",
    "#pragma flag enable lax\n.float fl; entity self;\n\
     void() g = { local float a; local vector v; a *= v; self.fl *= v; };\n",
    // FTEF-34: statements.
    "float a;\nfloat() g = { return; };\n",
    "float a;\nfloat() g = { return = 1; return; };\n",
    "float a;\nvoid() g = { return 1; };\n",
    "string s;\nfloat() g = { return s; };\n",
    "float a;\nfloat() g = { if (a) return 1; };\n",
    "float a;\nfloat() g = { if (a) return 1; else return 2; };\n",
    "float a;\nfloat() g = { if (a < 0) return 1; if (a) return 1; };\n",
    "float a;\nfloat() g = { while (a) { return 1; } };\n",
    "float a;\nfloat() g = { do { return 1; } while (a); };\n",
    "float a;\nfloat() g = { switch (a) { case 1: return 1; default: return 2; } };\n",
    "float a;\nvoid() g = { return; a = 2; };\n",
    "float a;\nvoid() g = { for (a = 1; ; a = 2) break; };\n",
    "float a;\nvoid() g = { goto l; a = 1; l: a = 2; };\n",
    "float a; string s;\nvoid() g = { switch (a) { case 1: break; case s: break; } };\n",
    "float a;\nvoid() g = { switch (a) { case a+1: break; } };\n",
    "string s;\nvoid() g = { switch (s) { case \"a\" .. \"b\": break; } };\n",
    "__variant w;\nvoid() g = { switch (w) { case 1: break; } };\n",
    "entity self; float a;\nvoid() g = { __thinktime a : 1; __thinktime self : 1; };\n",
    "#pragma warning enable F311\nfloat a;\nvoid() g = { if !(a) a = 1; if not(a) a = 2; };\n",
    "__variant w; float a;\nvoid() g = { if (w) a = 1; while (w) a = 2; };\n",
    // FTEF-6.13, FTEF-34.8: struct layouts and large returns.
    "struct s {__int8 a; __int16 b;}; float arr[1];\n\
     void() g = { local float n; n = arr[sizeof(s)]; };\n",
    "struct s {int a:30; int b:5;}; float arr[2];\n\
     void() g = { local float n; n = arr[sizeof(s)]; };\n",
    "struct s {__int8 a[3];}; float arr[3];\nvoid() g = { local float n; n = arr[sizeof(s)]; };\n",
    "struct s {float a, b, c, d;};\ns() g = { local s x; return x; };\n",
    "#pragma target fte\nstruct s {float a, b, c, d;};\ns() g = { local s x; return x; };\n",
    "#pragma target fte\nstruct s {float a, b, c, d;};\ns() g = { return; };\n",
    "enum class E : float {A, B, C}; E e;\nvoid() g = { switch (e) { case E::A: break; } };\n",
    "enum class E : float {A, B}; E e;\n\
     void() g = { switch (e) { case E::A: break; case E::B: break; } };\n",
    "enum class E : float {A, B}; E e;\nvoid() g = { switch (e) { case 0: break; } };\n",
    // FTEF-6.12: struct values in bodies.
    "float a; struct st {float x, y;}; st t;\nvoid() g = { a = t + a; };\n",
    "float a; struct st {float x, y;}; st t;\nvoid() g = { a = (float)t; };\n",
    "float a; struct st {float x, y;}; st t;\nvoid() g = { if (t) a = 1; };\n",
    "float a; struct st {float x, y;}; st t;\nvoid() g = { local st u = t; u = t; };\n",
    // FTEF-34.9: global initialisers.
    "float a; float b = a + 1;\n",
    "float b = 1 + 2;\n",
    "const vector v = [19, 20];\n",
    "float a; vector w = [a];\n",
    "string(float x) f = #1; string b = f(1);\n",
    "string s = 1 + 1;\n",
    "entity self; .float fl; float b = self.fl;\n",
    // FTEF-32.2, FTEF-32.7, FTEF-24.9, FTEF-16.14: intrinsics and keyword names.
    "void(...) v = { local float x = va_arg(0, float); };\n",
    "void() v = { local float x = va_arg(0, float); };\n",
    "float a; void() g = { a = randomv(1, 2); };\n",
    "entity e; float a; void() g = { a = entnum(e); };\n",
    "entity e; float a; void() g = { a = e.entnum; };\n",
    "float y; void() g = { y = autocvar_x; };\n",
    "float c; void() g = { c = local; };\n",
    // FTEF-10.7, FTEF-10.8: accessors.
    "string(float b, float i) bufstr_get = #0; void(float b, float i, string s) bufstr_set = #0;\n\
     accessor strbuf : float { inline get string at[float idx] = { return bufstr_get(this, idx); }; \
     inline set string at[float idx] = { bufstr_set(this, idx, value); }; \
     get float size = { return 1; }; };\n\
     strbuf h; float a; string s;\n\
     void() g = { s = h.at[1]; h.at[1] = s; a = h.size; };\n",
    "accessor strbuf : float { get float size = { return 1; }; }; strbuf h;\n\
     void() g = { h.size = 2; };\n",
    "accessor strbuf : float { get float size = { return 1; }; }; strbuf h; float a;\n\
     void() g = { a = h.nope; };\n",
    "accessor H : string; H h; float b;\nvoid() g = { b = h; };\n",
    "accessor H : float; H h; float b;\nvoid() g = { b = h; h = b; };\n",
    // FTEF-24.10: a `.` after a value with no members.
    "float a, b; void() g = { b = a.nope; };\n",
    "float a, b; void(float x) f = {}; void() g = { f(a.x); };\n",
    ".float fl; float b; void() g = { b = fl.x; };\n",
    // FTEF-24.7: function types in messages.
    "void(float x, optional float y = 2) h = {}; float b; void() g = { b = h; };\n",
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
