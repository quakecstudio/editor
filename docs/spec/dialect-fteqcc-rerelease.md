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
recorded run is in [`vectors/dialect-fteqcc-rerelease/coverage.txt`](vectors/). The six
codebases hold these classes and no others.

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
`qcc_pr_comp.c`, `QCC_PR_Expression`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has neither `?` nor `:` as punctuation: each is "Unknown punctuation"
(QC-7.2).

### Behaviour

- **FTE-8.1** `cond ? a : b` evaluates to `a` when `cond` is true (QC-19) and to `b`
  otherwise. The rerelease uses it, including nested
  (`rerelease-mg1/ai.qc`, `rerelease-mg1/misc_fx.qc`).
- **FTE-8.2** `a` and `b` must have the same type; that is the type of the whole expression.
- **FTE-8.3** Progs encoding: lowers to an `IFNOT` over the two arms storing into a shared
  temporary, over vanilla opcodes; no new opcode is used.

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

### Test vectors

Larger vectors: [`vectors/dialect-fteqcc-rerelease/`](vectors/). The recorded coverage run
over the six codebases is
[`vectors/dialect-fteqcc-rerelease/coverage.txt`](vectors/dialect-fteqcc-rerelease/coverage.txt).
The progs-encoding vectors (the `.4`/`.3` statements that say "fixed by P2-06") are filled in
when that issue lands, each checked against FTEQCC's output and the rerelease's runtime
behaviour.
