# FTEQCC features the rerelease uses

Prefix: `FTE` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

Describes the non-vanilla QuakeC constructs the six codebases of
[id-Software/quake-rerelease-qc](https://github.com/id-Software/quake-rerelease-qc) use, at
the commit `corpus/manifest.toml` pins
(`634eefab09a77eb7b5f5ca7078ba3d8784a91142`). The rerelease's own README states the need:
"These progs also use some advanced QuakeC features that are only available in modern
compilers. We recommend using [FTEQCC](https://www.fteqcc.org) for compiling this release."
(id-Software/quake-rerelease-qc@`634eefab09a77eb7b5f5ca7078ba3d8784a91142`, `README.md`, read
on 2026-10-17.) This file is the first of COMP-3's dialect order: the FTEQCC features the
MVP must accept. The rest of FTEQCC, and GMQCC, come later.

The vanilla language is [`language.md`](language.md) (prefix `QC`); every construct here is
one vanilla `qcc` rejects, named by the vanilla rule it breaks. The syntax and semantics
below follow FTEQCC, whose source is `engine/qclib/qcc_pr_comp.c`, `qcc_pr_lex.c` and
`qccmain.c` of [fte-team/fteqw](https://github.com/fte-team/fteqw) at commit
`f937b9d88f71fc4429db5fe56c6a98d922711b2e` (the commit [`progs.md`](progs.md) and
[`lno.md`](lno.md) already cite); it was read on 2026-10-17. FTEQCC is not a registered
porting upstream ([`../porting.md`](../porting.md)), so nothing here is ported from it: these
statements are this project's own rules, written to compile the rerelease the way FTEQCC
does. Where FTEQCC's behaviour and the rerelease's use of a feature differ, the rule follows
what the rerelease relies on and says so.

To be implemented by the dialect profiles (P2-03) and `qc-codegen` (P2-06); until code cites
them the semantic and encoding statements are in
[`coverage-baseline.tsv`](coverage-baseline.tsv). The construct-detection statements (the
`.1` of each section) are cited by the coverage check in
[`xtask/src/fteqcc_coverage.rs`](../../xtask/src/fteqcc_coverage.rs), which parses every
rerelease codebase with the vanilla parser and lists the constructs it rejects (FTE-1).

## How the inventory was taken

`cargo xtask fteqcc-coverage <dir>...` fetches the six rerelease codebases
(`cargo xtask corpus fetch rerelease rerelease-hipnotic rerelease-rogue rerelease-mg1
rerelease-ctf rerelease-mg3`), parses every `.qc` and `.qh` file with the vanilla parser
(`qc-syntax`, [`language.md`](language.md)), and groups each construct the vanilla grammar
rejects into one of the classes below by its signature token. It fails if a class is found in
the source that no section here names, so the spec and the source cannot drift apart. The
recorded run is in [`vectors/dialect-fteqcc-rerelease/coverage.txt`](vectors/), and covers
FTE-2 to FTE-21. FTE-17 to FTE-22 and FTE-15.4 were found later, when the dialect profile
(P2-03) parsed and resolved every module of the six codebases with zero errors
(`cargo xtask fteqcc-profile`), and their classes were added to the check after (P2-33).
FTE-22 is name resolution, which has no syntax of its own, so it has no class. The hanging
`;` of FTE-21.5 exists only once a macro whose body ends in `;` is expanded, so the check finds
it as a use of such a macro followed by `;`, in a file the vanilla parser may accept as
written.

## FTE-1 The rerelease fits version 6

**Source:** the recorded differential-oracle build of the six codebases with FTEQCC
(`xtask/src/oracles/rerelease.rs`, P1-26), and `progs.md` PROGS-8 (the opcode set id's VM
has).

FTEQCC compiles the rerelease to a `progs.dat` id's own VM can run. It does not need FTE's
extended version 7 progs format.

### Behaviour

- **FTE-1.1** Every rerelease codebase compiles to a version 6 `progs.dat`
  (`progs.md` PROGS-2.1) whose statements all use opcodes 0 to 65, the opcodes id's VM has
  (`progs.md` PROGS-8.1). None uses an FTE extended opcode or the version 7 format. So the
  rerelease needs no extension to the progs format this project already reads; the dialect is
  a front-end concern, not a format one.
- **FTE-1.2** FTEQCC compiles the rerelease with `-Tid`, its target that refuses any opcode
  id's VM lacks. The mission-pack codebase Dawn of the Machine (`rerelease-mg3`) also needs
  `-Fautoproto`, because it uses functions and constants before it declares them, which
  FTEQCC resolves only when it collects prototypes in a first pass. Both facts are recorded
  by the oracle (`xtask/src/oracles/rerelease.rs`).

## FTE-2 `for` loops

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement` (the `for` case), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has `if`, `while` and `do` only (QC-14.1); `for` is not a statement keyword, so
`for (...)` parses `for` as a name and fails.

### Behaviour

- **FTE-2.1** `for (init; cond; step) body` is a statement. `init` and `step` are expression
  statements (either may be empty), `cond` is an expression (an empty `cond` is always true),
  and `body` is one statement or a block.
- **FTE-2.2** `init` may be a local declaration with an initialiser: `for (float i = 0; ...)`
  declares `i` for the loop. The rerelease uses this form
  (`rerelease-mg1/misc_corpses.qc`, `rerelease-mg3/mg3_upgrades.qc`).
- **FTE-2.3** The loop runs `init` once, then while `cond` is true runs `body` then `step`.
  `continue` (FTE-4) jumps to `step`; `break` (FTE-4) leaves the loop.
- **FTE-2.4** Progs encoding: a `for` lowers to the statements of `while` (QC-22, QC-14.3):
  `init`; a test with `IFNOT` to after the loop; `body`; `step`; a `GOTO` back to the test.
  No new opcode is used.

## FTE-3 `switch`, `case` and `default`

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement` (the `switch` case), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `switch`; the words `switch`, `case` and `default` are ordinary names
(QC-3.2), so `switch (x)` fails on the `(`.

### Behaviour

- **FTE-3.1** `switch (expr) { ... }` selects among `case value:` labels and an optional
  `default:` label by comparing `expr` to each `case` value. A `case` value is a
  constant expression (the rerelease uses named float constants, for example
  `rerelease/itemnames.qc`).
- **FTE-3.2** Control enters at the first matching `case`, or at `default` if none matches and
  a `default` is present, or leaves the `switch` if none matches and there is no `default`.
- **FTE-3.3** Execution falls through from one label's statements into the next unless a
  `break` (FTE-4) leaves the `switch`. A `case` whose body is a block `{ ... }` is accepted
  (`rerelease/itemnames.qc` uses `case IT_KEY1: { ... }`).
- **FTE-3.4** Progs encoding: a `switch` lowers to a chain of equality tests and `GOTO`s over
  vanilla opcodes (`EQ_F` or the comparison for the switched type, `IFNOT`, `GOTO`), with no
  new opcode; a `break` becomes a `GOTO` to after the `switch`.

## FTE-4 `break` and `continue`

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement` (the `break` and `continue` cases), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `break` or `continue` statement; both words are ordinary names
(QC-3.2). (id 1.01 does use `break` as the *name* of builtin #6, `void() break = #6;`, which
is a declaration, not a statement; that stays vanilla.)

### Behaviour

- **FTE-4.1** `break;` leaves the innermost enclosing `while`, `do`, `for` (FTE-2) or `switch`
  (FTE-3). `continue;` jumps to the next iteration of the innermost enclosing loop (its
  `step` in a `for`, or its condition test). The rerelease uses both
  (`rerelease/monsters/chthon.qc`, `rerelease-mg1/misc_fx.qc`).
- **FTE-4.2** A `break` or `continue` with no enclosing loop or switch is an error.
- **FTE-4.3** Progs encoding: each becomes a `GOTO` to the loop or switch's exit or
  continue point; no new opcode is used.

## FTE-5 Increment and decrement operators

**Source:** `qcc_pr_lex.c`, `QCC_PR_LexPunctuation` (the `++` and `--` tokens);
`qcc_pr_comp.c`, `QCC_PR_Expression`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

`qcc.h` states vanilla QuakeC has none: "There are no ++ / -- operators" (QC-7.4).

### Behaviour

- **FTE-5.1** `lvalue++` and `lvalue--` are postfix expressions that read the lvalue, then
  store it plus or minus one. The rerelease uses `i++` in `for` loop steps
  (`rerelease-mg1/misc_corpses.qc`, `rerelease-mg3/mg3_upgrades.qc`).
- **FTE-5.2** As a statement the result is unused, so postfix and prefix have the same effect;
  the rerelease uses only the postfix form in a `for` step.
- **FTE-5.3** Progs encoding: `x++` lowers to `ADD_F x, 1` into a temporary and `STORE_F` back
  to `x` (or the field store for a field), over vanilla opcodes; no new opcode is used.

## FTE-6 Compound assignment operators

**Source:** `qcc_pr_lex.c`, `QCC_PR_LexPunctuation` (the `+=`, `-=`, `*=`, `/=` tokens);
`qcc_pr_comp.c`, `QCC_PR_Expression`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

`qcc.h` states vanilla QuakeC has none: "or operate/assign operators" (QC-7.4).

### Behaviour

- **FTE-6.1** `lvalue += rhs`, `-=`, `*=` and `/=` read the lvalue, apply the operator with
  `rhs`, and store the result back. The rerelease uses all four on floats and vectors
  (`rerelease/ai.qc` `ang_x *= -1`, `rerelease-ctf/hook.qc` `start += offset`,
  `rerelease-mg1/math.qc` `num/= 2`).
- **FTE-6.2** The operand types follow the matching binary operator (QC-16): `+=` and `-=`
  on `float` or `vector`, `*=` with a `float` or vector-scale right side, `/=` by a `float`.
- **FTE-6.3** Progs encoding: `x += y` lowers to the binary op into a temporary and a store
  back to `x`, over vanilla opcodes (`ADD_F`/`ADD_V` and so on); no new opcode is used.

## FTE-7 Modulo operator

**Source:** `qcc_pr_lex.c`, `QCC_PR_LexPunctuation` (the `%` token); `qcc_pr_comp.c`,
`QCC_PR_Expression`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `%`: it is "Unknown punctuation" (QC-7.2, QC-7.4).

### Behaviour

- **FTE-7.1** `a % b` is the floating-point remainder of `a` divided by `b`. The rerelease
  uses it on floats (`rerelease-mg1/horde.qc` `self.wave % 3`).
- **FTE-7.2** Division by zero follows FTEQCC's generated code; the exact result is fixed when
  P2-06 implements the lowering and is checked against the rerelease's runtime behaviour in
  the differential suite.
- **FTE-7.3** Progs encoding: `%` has no vanilla opcode, so FTEQCC emits a short sequence of
  vanilla float opcodes (a division, truncation and multiply-subtract). The exact statements
  are fixed by P2-06 and recorded in this section's vectors then.

## FTE-8 Ternary conditional operator

**Source:** `qcc_pr_lex.c`, `QCC_PR_LexPunctuation` (the `?` and `:` tokens);
`qcc_pr_comp.c`, `QCC_PR_Expression`, `QCC_PR_RefExpression` and `QCC_PrioritiseOpcodes`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has neither `?` nor `:` as punctuation: each is "Unknown punctuation"
(QC-7.2).

### Behaviour

- **FTE-8.1** `cond ? a : b` evaluates to `a` when `cond` is true (QC-19) and to `b`
  otherwise. The rerelease uses it, including nested
  (`rerelease-mg1/ai.qc`, `rerelease-mg1/misc_fx.qc`).
- **FTE-8.2** `a` and `b` must have the same type; that is the type of the whole expression.
- **FTE-8.3** Progs encoding: lowers to an `IFNOT` over the two arms storing into a shared
  temporary, over vanilla opcodes; no new opcode is used.
- **FTE-8.4** `?:` is read at FTEQCC's `TERNARY_PRIORITY`, the assignment priority, which
  is tighter than `&&` and `||`: without `flag_cpriority`, `QCC_PrioritiseOpcodes` sets
  `priority_class[PC_LOGICAND] = 7;`, `priority_class[PC_LOGICOR] = 7;`,
  `priority_class[PC_TERNARY] = 6;` and `priority_class[PC_STORE] = 6;`, and
  `QCC_PR_RefExpression` tries `if (priority == TERNARY_PRIORITY && QCC_PR_CheckToken ("?"))`
  on the operand it read at the priority below. The right operand of `=` is read at its own
  priority and that of `&&` and `||` at the one below theirs, so each reaches the `?` first:
  `x = a ? b : c` assigns the conditional, and `a && b ? c : d` is `a && (b ? c : d)`, as is
  `a || b ? c : d` with `||`. A comparison, `a == b ? c : d`, is the condition. Both arms are
  read at `TOP_PRIORITY` (`val = QCC_PR_Expression(TOP_PRIORITY, 0);` for the middle and
  `QCC_PR_Expression(TOP_PRIORITY, EXPR_DISALLOW_COMMA)` for the last), so a conditional in
  the last arm nests to the right, `a ? b : c ? d : e` is `a ? b : (c ? d : e)`, and the last
  arm takes `&&` and `=`. This differs from C, where `?:` is looser than `||`; FTEQCC takes
  the C order only with `flag_cpriority` (`priority_class[PC_TERNARY] = 12;`), which is not
  its default.

## FTE-9 Vector member access

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseValue` (the `.x`, `.y`, `.z` member on a vector),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` reads `v.x` as the field named `x` of entity-typed `v` (QC-18, member access is
only entity-field access); a vector has no members. The rerelease applies `.z` directly to a
`vector` local.

### Behaviour

- **FTE-9.1** On an expression of type `vector`, `.x`, `.y` and `.z` name its first, second
  and third float. The rerelease uses `.z` as an lvalue (`rerelease-ctf/hook.qc`
  `offset.z *= -1`).
- **FTE-9.2** A member is a float lvalue when its base is an lvalue, so it may be assigned and
  used with a compound operator (FTE-6).
- **FTE-9.3** Progs encoding: a vector's three floats are consecutive globals (QC-21), so
  `v.x`, `v.y` and `v.z` are the global at the vector's offset, offset plus one and offset
  plus two; no new opcode is used.

## FTE-10 `const` declarations

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs` (the `const` keyword), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `const`; `const` at the start of a declaration is read as a name and is
"not a type" (QC-11).

### Behaviour

- **FTE-10.1** `const T name = value;` declares a named compile-time constant of type `T`.
  The rerelease uses it heavily for float and string constants
  (`rerelease-mg1/lights.qc`, `rerelease-mg3/defs.qc`).
- **FTE-10.2** A `const` is folded at compile time where it is used; it may initialise another
  `const` (`rerelease-mg1/lights.qc` `const float LightStyles_MAX = LightStyles.length - 1`).
- **FTE-10.3** Assigning to a `const` is an error.
- **FTE-10.4** Progs encoding: a `const` that is only folded contributes no global of its
  own; its value is emitted as an immediate (QC-20) wherever it is used.

## FTE-11 `var` declarations

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs` (the `var` keyword), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `var`; it is read as a name and is "not a type" (QC-11).

### Behaviour

- **FTE-11.1** `var T name = value;` declares a mutable global of type `T` with an initial
  value, the opposite of `const`. The rerelease uses `var float`
  (`rerelease-mg1/defs.qc` `var float num_players = 0;`,
  `rerelease-rogue/newplats.qc` `var float elvButnDir = 0;`).
- **FTE-11.2** It differs from a bare `T name = value;` global by overriding FTEQCC's default
  that an initialised global is constant, so the name may be assigned to later.
- **FTE-11.3** Progs encoding: a global definition (QC-21) with its initial value, writable at
  runtime; no new opcode is used.

## FTE-12 `nosave` storage qualifier

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs` (the `nosave` keyword), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no storage qualifiers; `nosave` is read as a name and is "not a type"
(QC-11).

### Behaviour

- **FTE-12.1** `nosave T name;` declares a global that the engine's save and load of the
  globals skips. The rerelease uses `nosave float`
  (`rerelease-mg1/defs.qc` `nosave float load_flag;`).
- **FTE-12.2** It is otherwise an ordinary global of type `T` (QC-21). The qualifier affects
  only savegames, which id's VM spec does not cover, so its only compile-time effect is to
  be accepted.
- **FTE-12.3** Progs encoding: an ordinary global definition (QC-21); the `nosave` flag lives
  in FTEQCC's extended def flags, which a version 6 progs does not carry, so it is a
  front-end-only qualifier here.

## FTE-13 Arrays

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs`, `QCC_PR_ParseArrayPointer`, and the `.length`
member in `QCC_PR_ParseValue`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no arrays: `[` after a name in a declaration is "Unknown punctuation" in
that position, and subscripting is not an expression form (QC-7, QC-15).

### Behaviour

- **FTE-13.1** `T name[N] = { ... };` and `T name[] = { ... };` (size inferred from the
  initialiser) declare an array of `T`. The rerelease declares string and struct (FTE-14)
  arrays with an inferred size (`rerelease-mg1/lights.qc` `const string LightStyles[] = {...}`,
  `rerelease-mg1/misc_corpses.qc` `poseInfo poses[] = { ... }`).
- **FTE-13.2** `name[i]` subscripts the array with a float index `i`. The rerelease uses both
  a loop variable and a field as the index (`rerelease-mg1/misc_corpses.qc`
  `poses[i].mdl`, `poses[self.style]`).
- **FTE-13.3** `name.length` is the element count as a float constant
  (`rerelease-mg1/lights.qc` `LightStyles.length`, `rerelease-mg1/misc_corpses.qc`
  `poses.length` in a `for` bound).
- **FTE-13.4** Progs encoding: FTEQCC lays an array out as consecutive globals and compiles a
  subscript to its array-access helper over vanilla opcodes (it does not use a version 7
  feature for the rerelease, FTE-1.1). The exact lowering is fixed by P2-06 and recorded in
  this section's vectors then.

## FTE-14 `typedef struct` and member access

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs`/`QCC_PR_ParseStruct` and the typedef handling,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has `void`, `float`, `vector`, `string` and `entity` only (QC-10.1): no
`struct`, no `typedef`, no user-named aggregate type.

### Behaviour

- **FTE-14.1** `typedef struct { T1 f1; T2 f2; ... } Name;` defines an aggregate type `Name`
  with the named fields, in declaration order. The rerelease defines one
  (`rerelease-mg1/misc_corpses.qc` `typedef struct { string mdl; float frame; } poseInfo;`).
- **FTE-14.2** `Name v;` declares a value of the type; `v.f` reads field `f`. The rerelease
  reads struct fields through an array element (`poses[self.style].mdl`,
  `poses[i].frame`).
- **FTE-14.3** A struct initialiser `{ a, b }` gives its fields in order, and an array of
  structs (FTE-13) is initialised with one such brace group per element.
- **FTE-14.4** Progs encoding: a struct value occupies the sum of its fields' sizes in
  consecutive globals; field access is a fixed offset. No version 7 feature is used for the
  rerelease (FTE-1.1); the exact layout is fixed by P2-06 and recorded then.

## FTE-15 `enum`

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseEnum`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `enum`; the word is read as a name and is "not a type" (QC-11).

### Behaviour

- **FTE-15.1** `enum { A, B, C };` declares named float constants starting at 0 and
  increasing by one, like a sequence of `const float` (FTE-10). The rerelease uses the bare
  form and the typed forms `enum : int { ... }` and `enum class Name : int { ... }`
  (`rerelease-mg1/lights.qc`, `rerelease-mg3/defs.qc`, `rerelease-hipnotic/defs.qc`).
- **FTE-15.2** An explicit value `A = n` sets the running value; the next unvalued name is
  `n + 1`. QuakeC floats hold these, so `: int` and `enum class` names resolve to the same
  float constants; the `: int` and `class` are accepted and otherwise ignored for a version
  6 target.
- **FTE-15.3** Progs encoding: each name is a folded float constant (FTE-10.4); an `enum`
  contributes no global of its own.
- **FTE-15.4** `enum Name { ... }` also names a type, `Name`, which declares parameters and
  variables of the enum: `QCC_PR_ParseEnum` reads the name (`name = QCC_PR_ParseName();`) and
  makes it a type (`enumtype = QCC_PR_NewType(name, basetype->type, true);`). The rerelease
  declares `enum UpgradeType { ... }` and `float GetUpgradeAmount(UpgradeType Type)`
  (`rerelease-mg3/defs.qc`, `rerelease-mg3/mg3_upgrades.qc`).

## FTE-16 `#pragma` directive

**Source:** `qcc_pr_lex.c`, `QCC_PR_Precompiler` (the `pragma` case), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

This project's vanilla preprocessor (QC-9) handles `#include`, `#define`, `#undef`, `#ifdef`,
`#ifndef`, `#else` and `#endif`, which the rerelease also uses and which stay vanilla; QC-9.9
makes any other directive an error. `#pragma` is that other directive, and QC-9's note
assigns it here.

### Behaviour

- **FTE-16.1** `#pragma name args` is a compiler directive. The rerelease uses
  `#pragma warning disable F317` and `#pragma warning enable F317`
  (`rerelease-mg1/misc_corpses.qc`), which turn one FTEQCC warning off and on around a block.
- **FTE-16.2** A `#pragma` this compiler does not recognise is accepted and ignored with a
  warning, so unknown build-time hints do not stop a compile. `#pragma warning` is recognised
  and adjusts whether the named diagnostic is reported.
- **FTE-16.3** `#pragma` produces no tokens and no progs output; it affects only diagnostics
  and the build.

## FTE-17 C-style function declarations

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs` and `QCC_PR_ParseInitializerType` (the function
case), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` declares a function only as `rettype(params) name` (QC-10.3), wants `=`, `,` or `;`
after the name (QC-11.1), and takes a builtin number greater than 0 (QC-13.3). The rerelease's
`defs.qc` and code also use C's order, `void makevectorsfixed(vector ang);`, define functions
with no `=` (`void InitLightStyles() { ... }` in `rerelease-mg3/lights.qc`), and bind engine
builtins by name, `void localsound( entity client, string sample ) = #0:ex_localsound;`.

### Behaviour

- **FTE-17.1** `rettype name(params)` declares `name` with the function type
  `rettype(params)`: after the name `QCC_PR_ParseDefs` reads
  `if (QCC_PR_CheckToken("("))` ... `type = QCC_PR_ParseFunctionType(false, type);`. The type is
  the one `rettype(params) name` declares, so a C-style definition and a vanilla prototype
  (`void() InitLightStyles;` in `rerelease-mg3/world.qc`) are the same function (QC-11.5).
- **FTE-17.2** A function declarator's body may follow it without `=`:
  `isinitialised = QCC_PR_CheckToken ("=") || ((type->type == ev_function) && (pr_token[0] ==
  '{' || pr_token[0] == '[' || pr_token[0] == ':'));`. This holds for both orders,
  `void InitLightStyles() { ... }` and `void(entity e) remove { ... }`
  (`rerelease-mg1/defs.qc`).
- **FTE-17.3** The `;` after a function declaration is optional: `QCC_PR_ParseDefs` ends with
  `if (type->type == ev_function) QCC_PR_CheckTokenComment (";", ...)`, which takes a `;` if
  there is one. A `;` before a definition is skipped (`while (QCC_PR_CheckToken(";")) ;` at the
  start of `QCC_PR_ParseDefs`), so `};` after a body is also accepted.
- **FTE-17.4** A builtin initialiser is `#number` or `#number:name`, and the number may be 0:
  `QCC_PR_ParseInitializerType` takes any integral immediate
  (`pr_immediate._float == (int)pr_immediate._float`), then
  `if (!*fname && QCC_PR_CheckToken (":")) QC_strlcpy(fname, QCC_PR_ParseName(), ...)`; the
  name is the engine's name for the builtin. The rerelease binds its `ex_` extensions this way.
- **FTE-17.5** A function's builtin initialiser may also start with `:` in place of `#`, and
  the engine name may stand in place of the number: a string literal after `#` or `:`, or a
  name after `:`, is the name, and the number is then 0. `QCC_PR_ParseInitializerType_Internal`
  (`qcc_pr_comp.c`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`), when
  `if (type->type == ev_function && pr_token_type == tt_punct)`, reads
  `if (QCC_PR_CheckToken ("#") || QCC_PR_CheckToken (":"))`, with `int binum = 0;`, then the
  number of FTE-17.4, `else if (pr_token_type == tt_immediate && pr_immediate_type == type_string) QC_strlcpy(fname, pr_immediate_string, sizeof(fname)); else if (pr_token_type == tt_name) QC_strlcpy(fname, pr_token, sizeof(fname));`,
  and a `:name` after it only `if (!*fname && QCC_PR_CheckToken (":"))`, so `#"a":b` and
  `:a:b` are errors. A name directly after `#` never reaches it: `QCC_PR_Lex` (`qcc_pr_lex.c`)
  reads `#` and a letter as a macro use, and with no such macro reports "Explicit precompiler
  usage when not defined %s"; this profile reports it as `E0209`. By FTE-17.2's
  `pr_token[0] == ':'` the `:` form may follow a function declarator with no `=`, and only a
  function takes it. The pinned FTEQCC, built by `cargo xtask oracles build`, compiles
  `void(string s, ...) bp = #"ex_bprint";`, `= :ex_bprint3;`, `= :5;`, `= :"ex_bprint4";`,
  `= :0:ex_bprint5;` and `void(string s, ...) bx :ex_bprint6;` with 0 warnings, and rejects
  `= #"ex_a":ex_b;`, `= :ex_a:ex_b;`, `= #ex_bprint2;` and `float fx = :ex_bprint7;`. The
  vanilla profile reads none of these forms (QC-13.3).
- **FTE-17.6** The number of a builtin initialiser is FTEQCC's `binum`, which depends on the
  literal's type (FTEF-3.1): a `float` literal is bound as in QC-13.10, `(int)` of its value
  once that value is whole and from -2^31 to 2^31 - 1, so `#123456789` is 123456792; an
  integer literal with the `i` suffix is its `int`, the whole number kept to 32 bits, so
  `#16777217i` is 16777217 and `#4294967313i` is 17; and a literal with a `u` or `l` suffix is
  "Bad builtin immediate" (`E0209`). `QCC_PR_ParseInitializerType_Internal` (`qcc_pr_comp.c`,
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`) reads
  `if (pr_token_type == tt_immediate && pr_immediate_type == type_float && pr_immediate._float == (int)pr_immediate._float) binum = (int)pr_immediate._float; else if (pr_token_type == tt_immediate && pr_immediate_type == type_integer) binum = pr_immediate._int;`,
  then the string and name of FTE-17.5, `else QCC_PR_ParseError (ERR_BADBUILTINIMMEDIATE, "Bad builtin immediate");`;
  `QCC_PR_LexNumber` (`qcc_pr_lex.c`) gives a number with no suffix and no fraction the type
  `float` (`pr_immediate._float = (float)(num*sign);`), an `i` suffix `type_integer` with
  `pr_immediate._int = num;`, and a `u` or `l` suffix `type_uint`, `type_int64` or
  `type_uint64`, which no branch takes. The pinned FTEQCC binds `#16777217i` as 16777217,
  `#0x1000001i` as 16777217 and `#4294967313i` as 17, and rejects `#16u`, `#16l` and `#16ll`;
  built for arm64 it binds `#2147483648` as 2147483647, where the x86 result of VM-9.2 is the
  rule.

## FTE-18 Parameter lists

**Source:** `qcc_pr_lex.c`, `QCC_PR_ParseFunctionType`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has `(...)` only as the whole list (QC-10.4), a parameter is a vanilla type and a
name (QC-10.3), and nothing follows the name.

### Behaviour

- **FTE-18.1** `...` may end a list after named parameters: each parameter begins with
  `if (QCC_PR_CheckToken ("..."))`, which, with no type after it, sets `ftype->vargs = true`
  and ends the list. The rerelease declares `void(string s, ...) bprint = #0:ex_bprint;`.
- **FTE-18.2** A parameter may have a default value, `type name = value`:
  `if (QCC_PR_CheckToken("=")) paramlist[numparms].defltvalue =
  QCC_PR_ParseDefaultInitialiser(paramlist[numparms].type);`. The rerelease writes
  `float(entity e) predicate = (float(entity e))SUB_True` (`rerelease-mg1/subs.qc`).
- **FTE-18.3** A parameter's type is read by `QCC_PR_ParseType`, so it may be a type name the
  program defined, such as the named enum (FTE-15.4) of `float GetUpgradeAmount(UpgradeType
  Type)` (`rerelease-mg3/mg3_upgrades.qc`).

## FTE-19 Further operators

**Source:** `qcc_pr_lex.c`, `pr_punctuation[]` and `pr_punctuationremap[]`; `qcc_pr_comp.c`,
`QCC_PrioritiseOpcodes`, `QCC_PR_RefTerm` and `QCC_PR_RefExpression`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc`'s punctuation is QC-7.1; `~`, `^` and the rest below are "Unknown punctuation"
(QC-7.2), and an expression term has no prefix operator but `!` (QC-15.7). The rerelease clears
and sets flags with `self.flags &~= FL_ONGROUND` and `other.flags (+) FL_INSHELTER`
(`rerelease-mg1/weapons.qc`, `rerelease-mg1/triggers.qc`), uses `self->effects`
(`rerelease/client.qc`), `~` (`rerelease-mg1/items_runes.qc`), `-fabs(...)`
(`rerelease-mg1/rotate.qc`) and a cast (FTE-18.2).

### Behaviour

- **FTE-19.1** FTEQCC's punctuation adds, among others, `->`, `~`, `^`, `<<`, `>>`, `&=`, `|=`,
  `^=`, `&~=`, `<<=`, `>>=`, `(+)` and `(-)`, tried before shorter punctuation as its table
  says: "longer symbols must be before a shorter partial match". `^` is exclusive or, `<<` and
  `>>` shift, and each `op=` stores `a op b` back to `a`; `&~=` clears the bits of its right
  side.
- **FTE-19.2** `pr_punctuationremap[]` reads three of them as others: "(+) -> |=", "-> -> ." and
  "(-) -> &~=". So `a (+) b` is `a |= b`, `a (-) b` is `a &~= b`, and `e->f` is `e.f`.
- **FTE-19.3** Without `flag_cpriority` (the default for the rerelease's `-Tid` build),
  `QCC_PrioritiseOpcodes` gives shifts, `&`, `^` and `|` the priority of `*` and `/`
  (`priority_class[PC_MULDIV] = 3; ... priority_class[PC_SHIFT] = 3; ...
  priority_class[PC_BITAND] = 3; priority_class[PC_BITXOR] = 3; priority_class[PC_BITOR] = 3;`),
  one tighter than `+` and `-` as in vanilla (QC-15.1). The compound assignments are in
  `PC_STORE`, with `=`: right-associative, at the assignment priority.
- **FTE-19.4** A term may start with `-`, `+` or `~`: `QCC_PR_RefTerm` reads
  `if (QCC_PR_CheckToken ("-")) { ... e = QCC_PR_Expression (UNARY_PRIORITY, EXPR_DISALLOW_COMMA);`
  and the same for `+`, with `#define UNARY_PRIORITY 1`, so the operand is a term with its
  postfix forms; `~` takes `QCC_PR_Expression (NOT_PRIORITY, ...)`, the operand of `!`
  (QC-15.6). `-e` negates, `+e` is `e`, and `~e` is the bitwise not.
- **FTE-19.5** A negative number where a binary operator belongs is a subtraction:
  `QCC_PR_RefExpression` "work[s] around (4-3) being parsed as 4 -3 with no operator between"
  by reading the token as `+` and the negative number after it, at the priority of `+`. The
  lexer still reads `-1` after a value as one number (QC-4.1). The rerelease writes
  `self.count -1` (`rerelease-mg3/mg3_sacrifice_triggers.qc`).
- **FTE-19.6** `(type) e` is a cast: `QCC_PR_RefTerm` reads `(`, then
  `newtype = QCC_PR_ParseType(false, true, false)`; if it is a type, `)` and
  `e = QCC_PR_Expression (UNARY_PRIORITY, EXPR_DISALLOW_COMMA); e = QCC_EvaluateCast(e,
  newtype, false);`. A parenthesis that does not open a type is a parenthesised expression,
  as in vanilla. This profile parses before names are known, so it takes a cast at a `(`
  followed by a type keyword or `.`; the rerelease casts to no type a program defines.

## FTE-20 Further literals

**Source:** `qcc_pr_lex.c`, `QCC_PR_Lex`, `QCC_PR_LexNumber` and `QCC_PR_LexEscapedCodepoint`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` starts a number only at a digit or at `-` and a digit (QC-4.1), and knows the
escapes `\n` and `\"` only (QC-6.2).

### Behaviour

- **FTE-20.1** A `.` followed by a digit starts a number: `QCC_PR_Lex` dispatches
  `(c == '.'&&pr_file_p[1]!='.'&&pr_file_p[1] >='0' && pr_file_p[1] <= '9')` to
  `QCC_PR_LexNumber`. The rerelease writes `float DRAW_AI_SINGLE_FRAME = .1;`
  (`rerelease/defs.qc`).
- **FTE-20.2** A number may end in the float suffix `f` or `F`, which is not part of its value:
  after the fraction `QCC_PR_LexNumber` has `else if (c == 'f' || c == 'F') { pr_immediate_type
  = type_float; pr_file_p++; break; }`. The rerelease writes `1.0f` (`rerelease/client.qc`).
- **FTE-20.3** **Withdrawn** (P2-26, #82): it kept FTEQCC's escapes beyond one byte "Unknown
  escape char" under the FTEQCC profile, which now also reads the full dialect. Replaced by
  FTE-20.4 for the one-byte escapes and `dialect-fteqcc.md` FTEF-13.4 for the rest.
- **FTE-20.4** Besides `\n` and `\"`, the escapes that stand for one ASCII byte are `\r`, `\t`,
  `\a`, `\v`, `\f`, `\#`, `\\`, `\?` and `\'`, as `QCC_PR_LexEscapedCodepoint` maps them
  (`else if (c == 't') c = '\t';	//tab` and the rest). The rerelease uses `\t`
  (`rerelease-mg1/misc_corpses.qc`). FTEQCC's other escapes are the full dialect's
  (`dialect-fteqcc.md` FTEF-13.4).

## FTE-21 Declarations and statements in a body

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement` and `QCC_PR_ParseDefs`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

In vanilla `qcc` a local is declared with `local` (QC-11.10), a local may not be a field or a
function (QC-11.9), every definition and statement ends with `;`, and a nested block may not be
empty (QC-14.2).

### Behaviour

- **FTE-21.1** A statement that starts with a type declares locals with no `local`:
  `QCC_PR_ParseStatement` passes a name for which `QCC_TypeForName(pr_token)` finds a type, or
  one of the words `float`, `vector`, `string`, `entity`, `var`, `const` and the other type and
  storage keywords, to `QCC_PR_ParseDefs`, and also a statement starting with `.` ("for local
  .entity without var/local"). Such a declaration may stand anywhere among the statements. The
  rerelease writes `float playerFlags = CheckPlayerEXFlags(playerEnt);` mid-body
  (`rerelease/weapons.qc`) and `poseInfo pose = poses[self.style];`
  (`rerelease-mg1/misc_corpses.qc`). This profile parses before names are known, so it takes a
  statement that starts with a name and then another name, `Type name`, for a declaration of
  a type the program defined; no expression statement starts that way.
- **FTE-21.2** A local may be a field or a function variable: FTEQCC's check is commented out,
  `//		QCC_PR_ParseError ("Fields must be global");`. The rerelease declares
  `void() tutsh;` and `local void() pain_event;` in bodies
  (`rerelease-mg1/map_specific/mge2m2.qc`, `rerelease-mg3/monsters/boss_final.qc`).
- **FTE-21.3** A definition that is not a function may lack its `;`: `QCC_PR_ParseDefs` reports
  `QCC_PR_ParseWarning(WARN_UNDESIRABLECONVENTION, "Missing semicolon at end of definition");`
  and goes on. The rerelease's `local float color = bottom + top * 16` has none
  (`rerelease-ctf/teamplay.qc`). This profile reports `W0203` (QCD-7.18).
- **FTE-21.4** A block may be empty: `QCC_PR_ParseStatement` reads
  `while (!QCC_PR_CheckToken("}")) QCC_PR_ParseStatement ();`. The rerelease has `else { }`
  holding only a comment (`rerelease-mg3/mg3_upgrades.qc`).
- **FTE-21.5** A `;` alone is an empty statement: `QCC_PR_ParseStatement` takes it with
  `QCC_PR_ParseWarning(WARN_POINTLESSSTATEMENT, "Hanging ';'");`, unless a macro expansion
  produced it. A macro written `if (CheckContentsDamage()) return;` and used with a `;` after it
  makes one (`rerelease-mg1/ai.qc`, `rerelease-mg1/monsters/tarbaby.qc`). This profile reports
  `W0204` (QCD-7.19) for every one, since the parser does not see expansions.

## FTE-22 Name resolution

**Source:** `qcc_pr_comp.c`, `QCC_PR_GetDef`, `QCC_PR_ParseInitializerType_Internal`,
`QCC_PR_ParseDefs` and `QCC_PR_CompileFile`; `qcc_pr_lex.c`, `QCC_PR_ClearGrabMacros` and
`QCC_PR_MacroFrame`; `qccmain.c`, the `compiler_flag[]` table and
`QCC_SetDefaultProperties`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

In vanilla `qcc` a local or parameter named like a global is that global (QC-12.3), a name is
initialised once (QC-11.5), a name is known only after its declaration (QC-12.5), and the frame
list starts empty in each file (QC-8.3).

### Behaviour

- **FTE-22.1** A parameter or local is a new definition that shadows a global of the same name,
  whatever its type: when `QCC_PR_GetDef` allocates in a function's scope it skips global
  definitions, `if ( (def->scope || (scope && allocate)) && def->scope != scope) { ... continue;
  // in a different function }`. A use in the body finds the function's own definition first.
  The rerelease names a parameter `light` beside the spawn function `void() light`
  (`rerelease-mg3/mg3_weapons.qc`, `rerelease-mg3/lights.qc`). A second declaration of a name
  in the same function is still that definition again, or a type mismatch (QC-11.5).
- **FTE-22.2** **Withdrawn** (P2-26, #82): it made a second initialiser that is not a literal
  always "NAME redeclared", which the full dialect's corpus contradicts. Replaced by FTE-22.5
  for literals and `dialect-fteqcc.md` FTEF-17.5 for the rest.
- **FTE-22.5** A name initialised twice with the same value is accepted: for a "dupe
  initialisation" `QCC_PR_ParseInitializerType_Internal` compares the two word by word and
  reports `"incompatible redeclaration"` only when they differ. The rerelease initialises
  `float SIGIL_E1 = 1;` in both `rerelease-mg1/defs.qc` and `rerelease-mg1/items_runes.qc`.
  This profile compares two literal initialisers by value and accepts them when equal, and a
  differing literal is still "NAME redeclared" (QCD-3.6); other initialisers compare as
  FTEF-17.10 says.
- **FTE-22.3** With `-Fautoproto` every top-level declaration of the program is visible in every
  function body: `qccmain.c` describes the flag as "Causes compilation to take two passes
  instead of one. The first pass, only the definitions are read. The second pass actually
  compiles your code." `rerelease-mg3` needs it (FTE-1.2). Between the passes
  `QCC_SetDefaultProperties` makes a new table of `#define`s
  (`Hash_InitTable(&compconstantstable, ...)`), so the second pass defines its macros again.
- **FTE-22.4** Frame macros (QC-8) carry from one file to the next: at the start of a file
  `QCC_PR_CompileFile` calls `QCC_PR_ClearGrabMacros (true)`, which with `newfile` set only
  restarts the numbering (`if (!newfile) pr_nummacros = 0; pr_macrovalue = 0;`). A `$frame`
  name another file defined takes the new value ("old file, override it"); one this file
  already defined keeps its first (QC-8.7). With `-Fautoproto` the frame macros of the first
  pass are still defined in the second, so a file may use a frame name a later file defines
  (`rerelease-mg3/misc_corpses.qc` uses `$CORPSEFRAME_PLAYER_6` from `rerelease-mg3/player.qc`).

### Test vectors

Larger vectors: [`vectors/dialect-fteqcc-rerelease/`](vectors/). The recorded coverage run
over the six codebases is
[`vectors/dialect-fteqcc-rerelease/coverage.txt`](vectors/dialect-fteqcc-rerelease/coverage.txt).
The progs-encoding vectors (the `.4`/`.3` statements that say "fixed by P2-06") are filled in
when that issue lands, each checked against FTEQCC's output and the rerelease's runtime
behaviour.

## FTE-23 Builtin initialisers, read end to end

**Source:** `qcc_pr_lex.c`, `QCC_PR_Lex`, `QCC_PR_LexNumber` and `QCC_PR_LexString`;
`qcc_pr_comp.c`, `QCC_PR_ParseInitializerType_Internal`, `QCC_PR_GenerateBuiltinFunction` and
the "dupe initialisation" comparison of `QCC_PR_GenerateInitializerType`; `qccmain.c`,
`WriteData`; fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`, the FTEQCC that
`cargo xtask oracles build` builds. id's `qcc`: `qcc/pr_lex.c`, `qcc/pr_comp.c`,
id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`.

FTE-17.4 to FTE-17.6 say which builtin initialisers FTEQCC takes. This section follows one
from the first byte to the function FTEQCC writes: how its lexer reads what follows `#` or
`:`, what each literal's type is, how strings join, what an engine name is, and when a second
initialiser of the same function is the same builtin. A builtin initialiser is
`QCC_PR_ParseInitializerType_Internal`'s function case:

```c
char fname[256];
...
if (QCC_PR_CheckToken ("#") || QCC_PR_CheckToken (":"))
{
	int binum = 0;
	if (pr_token_type == tt_immediate
	&& pr_immediate_type == type_float
	&& pr_immediate._float == (int)pr_immediate._float)
		binum = (int)pr_immediate._float;
	else if (pr_token_type == tt_immediate && pr_immediate_type == type_integer)
		binum = pr_immediate._int;
	else if (pr_token_type == tt_immediate && pr_immediate_type == type_string)
		QC_strlcpy(fname, pr_immediate_string, sizeof(fname));
	else if (pr_token_type == tt_name)
		QC_strlcpy(fname, pr_token, sizeof(fname));
	else
		QCC_PR_ParseError (ERR_BADBUILTINIMMEDIATE, "Bad builtin immediate");
	QCC_PR_Lex();

	if (!*fname && QCC_PR_CheckToken (":"))
		QC_strlcpy(fname, QCC_PR_ParseName(), sizeof(fname));
```

### Behaviour

- **FTE-23.1** Under FTEQCC a `#` opens a builtin only when the byte right after it is `"`,
  `-`, a digit, `)` or `,`. `QCC_PR_Lex` reads any other as a macro use: `if (c == '#' &&
  !(pr_file_p[1]==')' || pr_file_p[1]==',' || pr_file_p[1]=='\"' || pr_file_p[1]=='-' ||
  (pr_file_p[1]>='0' && pr_file_p[1] <='9')))	//hash and not number`, then `if
  (!QCC_PR_CheckCompConst()) { ... QCC_PR_ParseError(ERR_CONSTANTNOTDEFINED, "Explicit
  precompiler usage when not defined %s", pr_token); }`. So `= # 5;`, `= # "x";`, `= #'a';`,
  `= #.5;`, `= #+5;`, `= #(5);`, `= #/**/5;`, `= #\t5;` and `= #;` are errors, which this
  profile reports as `E0209`, and `= #5;`, `= #-5;` and `= #"x";` are builtins. `:` is plain
  punctuation, so `= : 5;`, `= :'a';` (97) and `= : "nmr";` (`nmr`) are builtins. The
  vanilla `#` is plain punctuation too (`pr_punctuation[]` in id's `qcc/pr_lex.c` lists
  `"#"`), and id's `qcc` binds `= # 5;` as 5, as the vanilla profile does.
- **FTE-23.2** An FTEQCC hex number with a fraction is C's `atof` of its text, which reads
  `0x` as a hexadecimal float: `QCC_PR_LexNumber` keeps the `0x` in `pr_token`, takes only
  decimal digits after the `.` (`if (c >= '0' && c <= '9') { pr_token[tokenlen++] = c; }`),
  and sets `pr_immediate._float = (float)atof(pr_token);`. So `0x10.0` is 16, `-0x10.0` -16,
  `0x1.8` 1.5 and `0x1.08` 1.03125; `0x1.a` is `0x1.` (1) and then a name.
- **FTE-23.3** A `d` literal is a `double`, and FTEQCC binds no builtin number for it: the
  initialiser takes only `type_float`, `type_integer` and `type_string` immediates, and
  `QCC_PR_LexNumber` gives `type_double` to a `d` or `D` after a fraction (`else if (c == 'd'
  || c == 'D') { pr_immediate_type = type_double;`) and after the digits of base 8 or 10
  (`else if (c == 'd' || c == 'D') {	//note: conflicts with hex.`). So `= #16d;`,
  `= #16.0d;`, `= #0d;`, `= #010d;`, `= #0x1.5d;` and `= :16d;` are "Bad builtin immediate"
  (`E0209`), and `qc_syntax::builtin_binum` gives them no number. In base 16 `d` and `f` are
  digits (`c < 'a'+base-10`), so `#0x1d` is 29 and `#0x10f` 271; in base 8 or 10 `f` makes a
  `float`, so `#16f` and `#16.0f` are 16.
- **FTE-23.4** A `-` is part of a number only right before a digit (`QCC_PR_Lex`: `( c=='-' &&
  pr_file_p[1]>='0' && pr_file_p[1] <='9')`), and FTEQCC binds a negative builtin number like
  any other (FTE-17.4): `#-5` and `:-5i` are -5, `#-0x10` -16. `= #- 5;`, `= #-.5;`, `= :- 5;`
  and `= :-.5;` read the `-` alone and are "Bad builtin immediate" (`E0209`).
- **FTE-23.5** Adjacent string literals are one string (FTEF-16.8): `QCC_PR_LexString` loops,
  skipping whitespace and comments with `QCC_PR_LexComment(&pr_token_precomment);` before each
  part, until `else if (first) { ... } else break;`. So `= #"ab" "cd";` binds the engine name
  `abcd`, `= :"x" "y";` `xy`, and the parts may stand on separate lines or around a comment.
  An empty string sets no name: `fname` stays empty, so the builtin is named after the
  definition (`QCC_PR_GenerateBuiltinFunction(def.sym, binum, *fname?fname:def.sym->name)`)
  with `binum` 0, and `if (!*fname && QCC_PR_CheckToken (":"))` still reads a name after it:
  `= #"";` is builtin 0 named after the function and `= #"":ex_b;` binds `ex_b`. After a
  non-empty string a `:name` is an error, as FTE-17.5 says.
- **FTE-23.6** An engine name is at most 255 bytes: `QC_strlcpy(fname, ..., sizeof(fname))`
  into `char fname[256];` keeps the first 255 of a longer string or name. A name after `:` is
  `pr_token`, which `QCC_PR_ParseName`'s "name too long" check does not see.
- **FTE-23.7** A builtin initialiser makes a builtin function, numbered `binum` and named the
  engine name or, with none, the definition's name, unless it initialises a definition already
  initialised and FTEQCC finds one to reuse: `if (basedef && basedef->initialized)` it takes
  the first function, in the order they were made, with `functions[i].code == -1 &&
  functions[i].builtin == binum` and `!strcmp(functions[i].name, fname)`, or with no `fname`
  `(!*functions[i].name || !strcmp(functions[i].name, defname))`; `if (!tmp.cast) f =
  QCC_PR_GenerateBuiltinFunction(def.sym, binum, *fname?fname:def.sym->name);`. The "dupe
  initialisation" comparison then reports `"incompatible redeclaration"` unless the function is
  the one the definition has. So a second builtin initialiser is the same value exactly when
  its number and engine name are those of the first and no function with them was made before
  the definition's: under this profile it is otherwise `E0206`. `#0:ex_a` then `#0:ex_b`,
  `#5:ex_a` then `#5:ex_b`, `#5` then `#5:ex_a` and `#0` then `:ex_a` are `E0206`; `:ex_a`
  then `:"ex_a"`, `#0:ex_a` then `:ex_a`, `#"ex_a"` then `:ex_a`, `#5` then `#5.0`, and, for a
  definition `f`, `#5` then `#5:f` and `#0` then `:f` are not. After `void() g = #5:ex_a;`,
  `void() f = #5:ex_a;` given twice is `E0206`, since FTEQCC reuses `g`'s function; a function
  made for a redeclaration it reuses none for counts as made then. FTEQCC reports the
  redeclaration only once it has read the initialiser, so a bad literal in it is that error
  instead; id's `qcc` reports `"%s redeclared"` at the `=`, before it (`PR_ParseDefs`).
- **FTE-23.8** `cargo xtask oracles builtins` (and `cargo xtask oracles`, which CI runs on
  Linux, macOS and Windows) compiles every builtin initialiser of a cross product with the
  pinned FTEQCC under the FTEQCC profile and with id's `qcc` under the vanilla profile, and
  compares each outcome with the profile's: the builtin number and engine name of `f`'s
  function in the `progs.dat` written (`first_statement` is `-binum`, `WriteData`: `if
  (functions[i].code == -1) funcs[i].first_statement = PRLittleLong (-functions[i].builtin);`),
  or that the compiler accepted it with no builtin number, refused it as a redeclaration, or
  refused it otherwise. The cross product is `#`, `:`, `# ` and `: `, before each of 86 values
  (decimal, octal and hex numbers with their signs; fractions; the `f`, `d`, `i`, `u` and `l`
  suffixes and their combinations; character constants and a vector; names; strings, joined
  across spaces, comments and lines, empty, and longer than FTE-23.6's limit), each with no
  engine name or one written five ways after it, and after each of three function
  declarators; and 29 builtin forms given twice to one function in every pairing, with and
  without another function bound by the first form before them (the vanilla dialect only the
  `#number` forms). Numbers outside -2^31 to 2^31 - 1 are left out, since C leaves their
  conversion to `int` undefined (QC-13.10). The oracles' outcomes are recorded in
  `xtask/fixtures/oracles/builtin-outcomes.json`, which `cargo test -p xtask` checks the
  profile against on every host. The run of 2026-10-20 (macOS arm64) made 7382 cases: 7314
  agree, and 68 differ as QC-27.9 says, the vanilla profile refusing `#0`, `#-0`, `#0.0` and
  `#-5`, which id's `qcc` takes (QC-13.4), alone or given twice. Before P2-69 and P2-70 the same
  check found 856 cases that differed.
- **FTE-23.9** A `#` before a macro's name is that macro's expansion with no `#`, which the
  cross product leaves out: `QCC_PR_CheckCompConst` expands it and `QCC_PR_Lex` reads on, so
  with `#define N 5`, `= #N;` is `= 5;` ("Cannot cast from float to void()") and with
  `#define N "ex_a"` an implicit cast of the string. This profile reports every `#NAME` as
  `E0209`, as FTE-17.5 says for a name that is no macro; reading the macro as FTEQCC does is
  P2-71. String literals with a prefix (`R"..."`, `Q"..."`, `u8"..."`, `u"..."`, `U"..."`),
  which `QCC_PR_Lex` reads as strings (`if (c == '\"' || ((c == 'R' || c == 'Q' || c == 'u' ||
  c == 'U') && pr_file_p[1] == '\"') || (c == 'u' && pr_file_p[1] == '8' && pr_file_p[2] ==
  '\"'))`), are left out too: this profile does not lex them (P2-72). So is a name of 63 to 254
  bytes after `#number:`, which `QCC_PR_ParseName` takes (`if (strlen(pr_token) >= MAX_NAME-1)`
  with `#define MAX_NAME 256` in `qcc.h`) and this profile, like every name, reports as
  `E0202` from 63 bytes (QC-3.3); FTEQCC's longer names are P2-73.
