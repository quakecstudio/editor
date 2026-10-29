# Full FTEQCC syntax and semantics

Prefix: `FTEF` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

Describes the FTEQCC constructs **beyond the rerelease subset**: the type system (integers
and the other C types, pointers, arrays, structs, unions, enums, typedefs, classes,
accessors, `__variant`) and the remaining `#pragma` directives. The rerelease subset, the
first tier of COMP-3's dialect order, is [`dialect-fteqcc-rerelease.md`](dialect-fteqcc-rerelease.md)
(prefix `FTE`); this file is the second tier, the rest of FTEQCC that the wider FTEQCC corpus
mods use. GMQCC is the third tier and comes later
([`README.md`](README.md), registry; L-10).

The vanilla language is [`language.md`](language.md) (prefix `QC`); every construct here is
one vanilla `qcc` rejects, named by the vanilla rule it breaks. The syntax and semantics
below follow FTEQCC, whose source is `engine/qclib/qcc_pr_comp.c`, `qcc_pr_lex.c` and
`qccmain.c` of [fte-team/fteqw](https://github.com/fte-team/fteqw) at commit
`f937b9d88f71fc4429db5fe56c6a98d922711b2e` (the commit [`progs.md`](progs.md),
[`lno.md`](lno.md) and [`dialect-fteqcc-rerelease.md`](dialect-fteqcc-rerelease.md) already
cite); it was read on 2026-10-17. These statements describe what FTEQCC accepts, so that we
parse and resolve it. Since [ADR-0017](../adr/0017-port-fteqcc.md), `engine/qclib/` of
fte-team/fteqw is a registered porting upstream ([`../porting.md`](../porting.md#upstreams)),
and code that ports an FTEQCC function, such as `qc-preproc`'s macro expansion, cites that
function in its file header.

This file is the parse-and-resolve spec (P2-25): syntax and type rules. **Code generation is
out of scope and is marked later.** Each section's final statement is a `Codegen (later)`
note that names the issue (L-01) which fixes the progs output. Those notes are active
statements that no code implements yet, so they are listed in
[`coverage-baseline.tsv`](coverage-baseline.tsv) until L-01's tests cite them. Many of these
constructs need FTE's version 7 progs format ([`progs.md`](progs.md) PROGS-19 to PROGS-21),
unlike the rerelease subset, which fits version 6 (`dialect-fteqcc-rerelease.md` FTE-1).

Implemented by the dialect profiles and `qc-sema` (P2-26); a statement no test cites yet is
listed in [`coverage-baseline.tsv`](coverage-baseline.tsv) ([`README.md`](README.md)). The
construct-detection statements (the `.1` of each section) are cited by the coverage check in
[`xtask/src/fteqcc_full_coverage.rs`](../../xtask/src/fteqcc_full_coverage.rs), which scans
the FTEQCC corpus mods for each construct class and fails if the source holds a class no
section here names (FTEF-1).

## How the inventory was taken

`cargo xtask fteqcc-full-coverage <dir>...` scans every `.qc`, `.qh` and `.h` file under each
FTEQCC corpus mod directory and groups each full-dialect construct the vanilla grammar lacks
into one of the classes below by its signature token. It fails if a class is found that no
section here names, so the spec and the corpus cannot drift apart. The recorded run is in
[`vectors/dialect-fteqcc/coverage.txt`](vectors/dialect-fteqcc/coverage.txt).

The FTEQCC corpus mods are the `mod` codebases that use the full FTEQCC dialect rather than
only the rerelease subset: Nuclide SDK (`nuclide`), FreeCS (`freecs`) and Rad-Therapy / FreeHL
(`freehl`), fetched with `cargo xtask corpus fetch nuclide freecs freehl`
([`corpus/manifest.toml`](../../corpus/manifest.toml); each is recorded ISC there, with
"FTEQCC extended dialect" in its notes). These hold the classes below and no others. The
rerelease subset's own constructs (`FTE-2` to `FTE-16`: `for`, `switch`, `++`, `%`, `?:`,
`const`, `var`, `nosave`, bare `enum`, inferred-size arrays, `typedef struct`, `#pragma
warning`) also appear in these mods; they belong to `dialect-fteqcc-rerelease.md` and are
counted there, not duplicated here. This file counts only the constructs beyond that subset.

## FTEF-1 Scope and the version 7 target

**Source:** `qccmain.c`, the `compiler_flag[]` keyword table and `QCC_OPCodeSetTargetName`;
`qcc_pr_lex.c`, the `#pragma TARGET` case of `QCC_PR_Precompiler`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Progs format:
[`progs.md`](progs.md) PROGS-19 to PROGS-21.

FTEQCC's full dialect targets FTE's extended engine as well as id's. Its `#pragma TARGET`
chooses the output format; `QCC_OPCodeSetTargetName` lists the targets in its diagnostic as
`ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)` (`qcc_pr_lex.c`, `QCC_PR_Precompiler`, the
`TARGET` case). The constructs below are what the FTEQCC corpus mods use that the rerelease
subset does not.

### Behaviour

- **FTEF-1.1** A FTEQCC corpus mod (`nuclide`, `freecs`, `freehl`) is accepted under the
  FTEQCC profile. Every full-dialect construct it contains is covered by a section of this
  file; the coverage check (`xtask/src/fteqcc_full_coverage.rs`) fails on a class no section
  names. This statement is what that check pins.
- **FTEF-1.2** Unlike the rerelease subset, which fits version 6 (`dialect-fteqcc-rerelease.md`
  FTE-1.1), the full dialect's aggregate and pointer types (FTEF-6, FTEF-9) in general need
  FTE's version 7 progs format ([`progs.md`](progs.md) PROGS-19). Which construct needs
  version 7 and which lowers to version 6 is a code-generation question, fixed by L-01, not by
  this parse-and-resolve spec.
- **FTEF-1.3** Codegen (later): the progs format and opcode set each construct emits are fixed
  by L-01 and recorded there, against FTEQCC's output. This spec does not state them.

## FTEF-2 Integer and the other C scalar types

**Source:** `qcc_pr_lex.c`, `QCC_PR_ParseType` (the C-type branch: `signed`, `unsigned`,
`long`, `short`, `char`, `int`, `integer`, `float`, `double`, `_Bool`), `QCC_PR_CheckKeyword`
and `QCC_TypeForName`, the type globals (`type_integer`, `type_uint`, `type_int64`,
`type_uint64`, `type_sint8`, `type_uint8`, `type_sint16`, `type_uint16`); `qcc_pr_comp.c`,
`QCC_PR_ParseStatement`; `qccmain.c`, the `compiler_flag[]` keyword table and
`QCC_PR_NewType`'s calls, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Ported in
`crates/qc-syntax/src/c_scalar.rs`.

Vanilla `qcc` has `void`, `float`, `vector`, `string` and `entity` only (QC-10.1): `int`,
`integer`, `bool`, `double` and the rest are read as names and are "not a type" (QC-11). The
corpus uses `int` heavily (3392 occurrences across the three mods) and `bool` (1312).

### Behaviour

- **FTEF-2.1** `int` and `integer` both name a 32-bit signed integer type. FTEQCC defines them
  as `type_integer` (`QCC_PR_NewType("__int32", ev_integer, ...)`), and the two keywords
  `keyword_int` and `keyword_integer` both resolve to it (`QCC_PR_ParseType`, the C-type
  branch, `int`/`integer` set `bits = 32`). The corpus declares locals, globals, fields,
  parameters and return types of this type.
- **FTEF-2.2** **Withdrawn** (P2-117, #545): it said `signed`, `char`, `short` and `long` are
  types, but their keywords are disabled by default (`nondefaultkeyword` in `compiler_flag[]`),
  so bare they are "not a type". Replaced by FTEF-2.7 and FTEF-2.8.
- **FTEF-2.3** **Withdrawn** (P2-117, #545): it said `double` is a type, but `keyword_double` is
  disabled by default: the double is `__double`. Replaced by FTEF-2.7 and FTEF-2.8.
- **FTEF-2.4** **Withdrawn** (P2-117, #545): it said the C words combine in any order C allows,
  but the loop takes a word only while its test holds (`long int` is 32 bits, `int long` ends
  at `int`). Replaced by FTEF-2.8 and FTEF-2.9.
- **FTEF-2.5** **Withdrawn** (P2-117, #545): it said `volatile` is accepted, but
  `keyword_volatile` is disabled by default. Replaced by FTEF-2.11.
- **FTEF-2.6** Codegen (later): how each width is stored and which need version 7 is fixed by
  L-01.
- **FTEF-2.7** A C type word is matched as `QCC_PR_CheckKeyword(keywordenabled, string)` matches
  it: a token that starts with `__` matches its keyword in any case whether or not the keyword
  is enabled (`if (pr_token[0] == '_' && pr_token[1] == '_') { if (stricmp(string,
  pr_token+2)) return false; ...`), and a bare word only when its keyword is enabled, case
  sensitively. By default (`compiler_flag[]`) `int` and `integer` (`typekeyword`) and
  `unsigned` (`defaultkeyword`) are enabled, `float` and `_Bool` (`QCC_PR_CheckKeyword(true,
  "_Bool")`) always are, and `double`, `long`, `short`, `char`, `signed`, `register` and
  `volatile` (`nondefaultkeyword`) are not: `long l;`, `double d;`, `signed x;`, `short s;`
  and `char c;` are "\"WORD\" is not a type" (`E0203`), and `__long l;`, `__double d;` and
  `__CHAR c;` are types.
- **FTEF-2.8** `QCC_PR_ParseType` first looks the word up with `QCC_TypeForName`, which finds the
  types `QCC_PR_NewType` registers (`float`, `__double`, `__int`, `__int32`, `__uint`,
  `__uint32`, `__int64`, `__uint64`, `__int8`, `__uint8`, `__int16`, `__uint16`, `__bint`, ...):
  such a word is the type alone, so in `__int unsigned x;` the name is `unsigned`. Else its loop
  over C words takes, after an optional leading `register`, each word whose test holds and
  stops at the first that fails: `signed` and `unsigned` when neither was read; `long` while no
  width is set, a second making the width `longlongbits` (64); `int` or `integer` while no
  width is set or it is 16, setting 32 when none is; `short` (16) and `char` (8) while none is
  set; `_Bool` while none is set and no sign was read, which ends the type as `type_bint`
  (`__bint`, the `int` keyword being enabled); `float` while no width is set and no `long` was
  read (32, a float); `double` while no width is set or after `long` (64, or 128 after `long`,
  a float). A word the loop does not take is the declarator's name, so `int int i;`, `unsigned
  unsigned u;`, `int float f;` and `__long __long int x;` are errors. With no width set the
  width is `longbits`, 64, after `long` and 32 otherwise (`flag_ILP32` is set only by `-std=c`);
  then a float of 64 bits or more is `__double` and else `float`, and an integer of more than 32
  bits is `__int64`, of 8 or fewer `__int8`, of 16 or fewer `__int16` and else `__int32`, each
  unsigned (`__uint64`, `__uint8`, `__uint16`, `__uint32`) after `unsigned`. So `__long int` is
  `__int32`, `unsigned __long` is `__uint64`, `__short int` is `__int16`, `__register` alone is
  `__int32` and `unsigned float` is `float`. FTEQCC's `bool` is no keyword: it comes from the
  engine's extension headers (`fteextensions.qc`), a defined type.
- **FTEF-2.9** An `unsigned` float is the warning `W0226` "ignoring unsupported unsigned keyword,
  type will be signed", and a float of more than 64 bits (`__long __double`) "long doubles are
  not supported, using double", in that order, at the type's first word. The integer branch's
  "long longs are not supported, using long" is never given: `long long` sets 64 bits, and only
  more than 64 warns.
- **FTEF-2.10** In a function body a statement defines locals at a name `QCC_TypeForName` finds
  or at one of the bare words `QCC_PR_ParseStatement` tests with their keyword enabled
  (`integer`, `unsigned`, `int` and `_Bool` among the C words; FTEF-19.1 lists the others).
  `STRCMP` compares the bare word, so a `__` C word that names no registered type is a value
  there: `__long x;` in a body is "Unknown value \"__long\"", and `__int x;` defines a local.
- **FTEF-2.11** A type may start with `const`, which `QCC_PR_ParseType` reads and ignores with the
  warning `W0226` "ignoring unsupported const keyword", and then with `volatile`, read and
  ignored, which by default only `__volatile` spells; a `*` may be followed by `const`, with
  the same warning (`if (QCC_PR_CheckKeyword(keyword_const, "const")) QCC_PR_ParseWarning
  (WARN_IGNOREDKEYWORD, "ignoring unsupported const keyword");`). A bare `volatile` is a name,
  so `void(volatile float v) f;` is an error. At file level `__volatile float v;` is a
  definition; in a body it is a value (FTEF-2.10). How `const` makes a constant is FTE-10.
- **FTEF-2.12** An 8- or 16-bit integer, `__int8`, `__uint8`, `__int16` or `__uint16` (and the
  C words that make one, `char` and `short`), is `ev_bitfld` with the `parentclass` `int` or
  `uint` (`type_sint8 ->parentclass = type_integer;` and the others, `qccmain.c`). It is cast as
  its parent (`QCC_TryEvaluateCast` follows `parentclass`), so `c = "a"` is "Implicit cast from
  string to __int8" and `c = 3.5` "Numerical overflow. 3.500000 will be rounded to 3", and
  messages name it by its own name; a definition of it is read as a bitfield at bit 0
  (`QCC_BitfieldToDef`), extended by `OP_BITEXTEND_I`, or `OP_BITEXTEND_U` for an unsigned one,
  with the mask of its bits, to a value of its parent's type; it is no bitfield's base ("bitfields
  must be integer types"); and a string initialises an array of 8-bit integers with no cast, a
  character to each element (FTEF-19.8). An element of an array of them at a constant index is
  the bitfield `index * bits` bits in (`ret.ofs += bitofs>>5; bitofs&=31;`), read extended and
  stored packed into its word (`QCC_PR_Statement_BitCopy`, FTEF-6.15); at another index it is
  read through its address (`QCC_PR_GenerateAddressOf`, which the id target refuses) by the
  narrow load of FTEF-27.10. A local of one is initialised packed at bit 0
  (`QCC_PR_GenerateInitializerType`), and a local array of 8-bit ones from a string a character,
  then zeros, to each element. A field of one has no load: read, it is the fatal error `E0358`
  "QCC_PR_ExpandField: invalid field type __int8"; stored, its index is the error `E0359` "Unable
  to evaluate to numeric constant" (`QCC_Eval_Int` of the field), and the value is packed at bit 0
  and stored through its address (FTEF-6.18). FTEQCC prints "INTERNAL: over-freed refcount to
  NAME" for such a local's initialiser, its own temporaries' bookkeeping, which the semantic model
  does not report.

## FTEF-3 Integer, double and typed numeric literals

**Source:** `qcc_pr_lex.c`, `QCC_PR_LexNumber` (the base, sign and suffix handling) and
`QCC_PR_Lex` (the `0x` and leading-digit dispatch),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has float literals (QC-4) and hex is not among them; a trailing type suffix on a
number is not a vanilla token. The corpus writes `int` constants, hex masks and the `0i`
null.

### Behaviour

- **FTEF-3.1** A number may carry a type suffix: `f`/`F` float, `d`/`D` double, `i`/`I`
  integer, `u`/`U` unsigned, `l`/`L` long (and `ll`/`LL` long-long), in the combinations
  `QCC_PR_LexNumber` accepts (length and sign flags in either order). `12i` is an integer
  literal, `3.5d` a double, `1u` an unsigned.
- **FTEF-3.2** `0x`/`0X` begins a hexadecimal literal and a leading `0` a base-8 (octal) one;
  `QCC_PR_LexNumber` reads the digits in that base and `QCC_PR_Lex` dispatches `0x` to it.
  FTEQCC warns "A leading 0 is interpreted as base-8." on an octal literal.
- **FTEF-3.3** Without a suffix the type is chosen by the compiler flags: `flag_assume_integer`
  makes an un-suffixed whole number an integer (widening to `uint`/`int64`/`uint64` as its
  magnitude needs), otherwise it is a float (`QCC_PR_LexNumber`, the "assume base 10" tail).
  An un-suffixed number with a `.` is a float, or a double under `flag_assume_double`.
- **FTEF-3.4** The built-in macro `__NULL__` expands to `0i` (`qcc_pr_lex.c`,
  `QCC_PR_CheckBuiltinCompConst`), an integer zero used as a null pointer or entity; the corpus
  uses it.
- **FTEF-3.5** Codegen (later): how each literal type becomes an immediate is fixed by L-01.

## FTEF-4 Pointer types and pointer operators

**Source:** `qcc_pr_lex.c`, `QCC_PR_PointerType`, `QCC_PointerTypeTo` and the `*` loop of
`QCC_PR_ParseType`; the `pr_punctuation[]` table (`->`, `*^`, `&`, `*`) and
`QCC_PR_LexPunctuation`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.
Vanilla pointers: [`progs.md`](progs.md) PROGS-16.4.

Vanilla `qcc` cannot declare a pointer: "QuakeC has no way to declare a pointer" (PROGS-16.4),
and `&` and `->` are not vanilla operators (QC-7). FTEQCC declares pointer types and takes
addresses. The corpus uses `(__variant *)` casts and `&tcp_irc`-style address-of.

### Behaviour

- **FTEF-4.1** `T *` is the type pointer-to-`T`; one `*` per level, parsed by the `*` loop of
  `QCC_PR_ParseType` building `QCC_PointerTypeTo`/`QCC_PR_PointerType`. `float *`, `entity *`
  and `__variant *` all occur. A cast `(T *)expr` reinterprets `expr` as that pointer type.
- **FTEF-4.2** `&lvalue` is the address of an lvalue, of type pointer-to-its-type (the `&`
  token in `pr_punctuation[]`). The corpus takes the address of globals and of struct fields.
- **FTEF-4.3** `ptr->member` dereferences a pointer and selects a member (the `->` token,
  remapped to `.` in `pr_punctuationremap[]`): `ptr->member` is `(*ptr).member`.
- **FTEF-4.4** `*ptr` dereferences a pointer to its pointed-to lvalue. FTEQCC's internal
  dereference operator is `*^` (`QCC_PR_LexPunctuation` notes "fteqcc uses *^ internally"), and
  `**` is remapped to it under `flag_dblstarexp` for GMQCC compatibility with a warning; in
  FTEQCC source `*` before a value is the dereference.
- **FTEF-4.5** Codegen (later): pointers need FTE's version 7 progs
  ([`progs.md`](progs.md) PROGS-19, PROGS-20); the lowering is fixed by L-01.

## FTEF-5 Arrays with a declared size and multiple dimensions

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseArrayType` and the `[` handling in the type parsers,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset: inferred-size
arrays and `.length` are `dialect-fteqcc-rerelease.md` FTE-13.

Vanilla `qcc` has no arrays (QC-7, QC-15). The rerelease subset uses only inferred-size arrays
with a brace initialiser (FTE-13); the full dialect also declares a fixed size and more than
one dimension.

### Behaviour

- **FTEF-5.1** `T name[N];` declares an array of `N` elements, `N` a constant integer
  expression. `QCC_PR_ParseArrayType` reads `[N]` with `QCC_PR_IntConstExpr`, and errors
  "cannot cope with 0-sized arrays" on `N == 0`.
- **FTEF-5.2** `T name[A][B]...` declares a multidimensional array; `QCC_PR_ParseArrayType`
  loops over each `[...]` and builds nested array types with `QCC_GenArrayType`. It errors
  "too many dimensions" past 64.
- **FTEF-5.3** A sized array may be a struct (FTEF-6) or class member, a local, a global, a
  parameter (where FTEQCC may demote it to a pointer, FTEF-4) or a field; `QCC_PR_ParseStruct`
  and the function-type parser both call `QCC_PR_ParseArrayType`.
- **FTEF-5.4** Codegen (later): array layout and subscript lowering are fixed by L-01 (and for
  the inferred-size subset, FTE-13.4).

## FTEF-6 `struct` and `union`

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStruct` (shared by `struct` and `union`); `qccmain.c`,
the `keyword_struct` and `keyword_union` entries of `compiler_flag[]`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset: `typedef struct`
is `dialect-fteqcc-rerelease.md` FTE-14.

Vanilla `qcc` has no aggregate type (QC-10.1). The rerelease subset uses only
`typedef struct { ... } Name;` (FTE-14); the full dialect uses named `struct`, `union`,
inheritance, bitfields and nested members.

### Behaviour

- **FTEF-6.1** `struct Name { T1 f1; T2 f2; ... };` and the anonymous `struct { ... }` define
  an aggregate with the named members in declaration order (`QCC_PR_ParseStruct` with
  `structtype == ev_struct`). `struct Name;` with no body is a forward declaration.
- **FTEF-6.2** `union Name { ... };` is the same with `structtype == ev_union`: every member
  shares offset 0 and the union's size is its largest member (`QCC_PR_ParseStruct`, the
  `ev_union` offset branch).
- **FTEF-6.3** `struct Child : Parent { ... }` inherits: the child begins at the parent's size
  and adds its own members (`QCC_PR_ParseStruct` reads `:` then the parent name, and sets
  `newt->size = newt->parentclass->size`). The parent must be a previously defined struct.
- **FTEF-6.4** A member may itself be an array (FTEF-5), a pointer (FTEF-4), a function type,
  or a bitfield `T name : bits;` on an integer type (`QCC_PR_ParseStruct`, the `:` bitfield
  branch, which promotes to 64-bit when `bits > 32` and errors "bitfields must be integer
  types" otherwise). `v.member` selects a member.
- **FTEF-6.5** `access.member` on a struct value or through a pointer (FTEF-4.3) reads or
  writes the member; a brace initialiser `{ a, b }` gives members in order (as FTE-14.3 for the
  typedef-struct subset).
- **FTEF-6.6** Codegen (later): struct and union layout, and which need version 7, are fixed by
  L-01.

- **FTEF-6.7** `QCC_PR_ParseStruct` reads a body's members in a loop, and this profile reads them
  so: a `}` ends it, with "missing semi-colon" when a member's type is still open (`if (newparm)
  QCC_PR_ParseError(ERR_EXPECTED, "missing semi-colon");`); a `;` closes the open type (`newparm =
  NULL;`); a `,` goes on with the same type and is "element missing type" when none is open; any
  other token starts a new type, which is "missing semi-colon" while one is open, and begins with
  `public`, `private` or `protected` and `QCC_PR_Expect(":")` (`QCC_PR_CheckKeyword(1, "public")`,
  then `continue`), or `static` or `virtual`, then `QCC_PR_ParseType(false, false, true)`, which
  makes the type of a nested `struct`, `union`, `enum` or `class` whether or not a declarator
  follows, then any `*`, then either a `;` (`//annonymous structs do weird scope stuff`: a member
  of the name "", a warning "declaration does not declare anything" unless its type is a struct or
  union) or `(` and an optional `*` and a name and `)`, or a name, then `[` array dimensions, `(`
  and a parameter list (else "expected function arg list"), `:` and a width, and `=` and a default
  value. A body is `struct S { public: float a; private: float b; };`, `struct S { struct T {
  float a; }; float b; };`, `struct S { enum { A }; float a; };`, `struct S { struct { float a; };
  float b; };`, `struct S { float; };` and `struct S { float a, *b; };`. A member written at
  column 0 is a member (FTEF-18.34). A `static` member is a global, not counted among the members
  (`numparms`), and a body with none is "struct %s has no members" (`QCC_PR_ParseError
  (ERR_NOTANAME, "%s %s has no members", ...)`), as `struct S { };`, `struct S { ;; };` and `union
  U { };` are, and so is a body whose every member the parent has with the name and type (`oldparm
  = QCC_PR_FindStructMember(newt, parmname, &ofs, &bitofs)`, which is then the parent's), `struct
  P { float a; }; struct S : P { float a; };`. A member whose type takes a `(` with `*` after it,
  `float (*fp)(float);` with `qcfuncs` on, is "expected function arg list": `QCC_PR_ParseType`
  took the `(`. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_lex.c`, `QCC_PR_ParseStruct`.
- **FTEF-6.8** After `struct` or `union`, `QCC_PR_ParseStruct` reads `{` for an anonymous type
  (`<struct>`, `<union>`), else the tag with `QCC_PR_ParseName`, so `struct *p;` is "not a name";
  a struct (`structtype == ev_struct`) then reads `:` and a parent, which a union does not, so
  `union U : P { float b; };` is an error at the `:`; a parent needs the body (`if (parenttype)
  QCC_PR_Expect("{");`), so `struct S : P;` is "expected {"; with no parent a tag without `{`
  names the type and returns (`else if (!QCC_PR_CheckToken("{")) return newt;`), so `struct S x;`
  is a variable of the type, and `struct S;` declares it; the type is the one
  `QCC_TypeForName(tname)` finds, so `struct float x = 1;` is a `float`. A named union is an
  `ev_struct` type like a struct. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_lex.c`, `QCC_PR_ParseStruct`.
- **FTEF-6.9** A member's `:` is followed by `QCC_PR_IntConstExpr()`; "too many bits" for more
  than 64; the type must be `ev_integer`, `ev_uint`, `ev_int64` or `ev_uint64`, else
  `QCC_PR_ParseWarning(ERR_BADNOTTYPE, "bitfields must be integer types")`, an error, so `struct S
  { float a : 3; };`, a `char`, a `short` and a `string` field are refused and `int a : 3;`,
  `unsigned a : 3;` and `long a : 3;` and `int a : 40;` (promoted to 64 bits) are not. The width's
  names are values (`int a : b;` with no `b` is "Unknown value"), a literal with a fraction is
  "Value is not an integer constant" (`int a : 1.5;`), and `(1 + 2)` is a width. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseStruct`.
- **FTEF-6.10** `QCC_TypeForName(tname)` finds the type of a struct's name; a struct given a body
  when the name is a type with a size (`if (newt->size)`: a struct defined, or `float`, a class,
  an enum, a typedef) is a new type compared with the old after its members (`typecmp_strict(newt,
  oldtype)`), "struct %s redeclared differently" when they differ in a name, type, array size, bit
  width or order of a member, and the same members are accepted: `struct S { float a; }; struct S
  { float a; };`, but not `struct S { float b; };`, nor `struct float { float a; };`. A parent
  that is not yet a type is "Parent type %s was not yet defined", one that is no struct "Parent
  type %s is not a struct" (a typedef of a struct is the struct, `typedef struct P { float a; }
  PP; struct S : PP { float b; };`), and a struct that has a parent or a size given another is
  "Redeclaration of struct with different parent type". A direct member whose type is the struct
  itself, or a struct with no body so far, is "type %s not fully defined yet" (`if (type == newt
  || ((type->type == ev_struct || type->type == ev_union) && !type->size))`,
  `QCC_PR_ParseWarning(ERR_NOTANAME, ...)`). A struct written in a function's body is compared with the struct of its name as any is, and lasts to the body's end (`QCC_PR_ParseStruct` with `pr_scope`, dropped with the block's locals), so a struct of its name after the body is another. The struct's own name is no parent of it (`newt = QCC_TypeForName(tname)` comes after the parent is found): `struct S : S { float a; };` is "Parent type S was not yet defined", and `struct S; struct S : S { float a; };` never ends in FTEQCC. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseStruct`.
- **FTEF-6.11** The member after `.` of a struct is `QCC_PR_FindStructMember`'s: the members of
  the struct are searched, with `stricmp` under `-Fcaseinsens` and `STRCMP` otherwise, those of an
  anonymous struct member too, and more than one found is "multiple members found matching %s.%s";
  a member declared again with its name, type and size is one; none found in the struct goes on to
  its parent. `union U { float a; float A; }; U x; ... x.A = 1;` is refused under `-Fcaseinsens`
  and accepted otherwise; a name no struct of the chain has is `QCC_PR_ParseWarning(ERR_BADMEMBER, "%s is not a member of %s", ...)`, an error, `S s; s.b = 1;` after `struct S { float a; };`; a struct's name is no scope of its members or of a nested enum's values, `float y = S::A;` after `struct S { enum { A }; float a; };` is "Unknown value"; and
  an array takes `.length` and no other member, "expected length, found a" for `S x[2]; x.a = 1;`.
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_FindStructMember`.
- **FTEF-6.12** A struct or union is cast to and from nothing but itself: `QCC_TryEvaluateCast`
  takes a cast of a type to itself (`if (!typecmp(src.cast, cast))`) and a constant 0 to any type
  but a struct or union (`else if (QCC_SRef_IsNull(src) && totype != ev_struct && totype !=
  ev_union)`), and has no case for a struct, so `QCC_EvaluateCast` ends in `QCC_PR_ParseError(0,
  "Cannot cast from %s to %s", ...)`: `struct S { float a; }; S s = 1;`, `S s = 0;`, `S s; float x
  = s;` and `(struct S)0` in a body are "Cannot cast from float to struct S" and "from struct S to
  float". A struct's initialiser is an aggregate, `S s = { 1 };` (`else if ((type->type ==
  ev_struct || type->type == ev_union) && QCC_PR_CheckToken("{"))` in
  `QCC_PR_ParseInitializerType_Internal`), or a value of the same type, `S t = s;`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_TryEvaluateCast`, `QCC_EvaluateCast`, `QCC_PR_ParseInitializerType_Internal`.
- **FTEF-6.13** A type has a size in words, `size`, and in bits, `bits`, with an alignment in
  bits, `align` (`QCC_PR_NewType`: `type_size[]` words, aligned to 32): one word for a float, a
  string, an entity, a field, a function, a pointer and a 32-bit integer, two for a 64-bit
  integer and a double, three for a vector and a `variant`; `__int8` and `__uint8` take 8 bits
  aligned to 8, `__int16` and `__uint16` 16 aligned to 16. `QCC_PR_ParseStruct` lays out a
  struct's members bit by bit after its parent's words, skipping a `static` member and one the
  parent has with the same name, size and type: a member with bits is aligned to its alignment,
  a bitfield (`int x : N`, of an integer's size and alignment, an `int` wider than 32 bits made
  64) only where it would cross it, any other member to a word, and each takes its bits, or 32
  a word, times its array's elements. A union's members all start at 0 and it takes the
  largest. The tail is padded to the largest alignment, the size is the bits rounded up to
  words, and the type keeps its bits when that alignment is no whole word. `sizeof` is the bits
  over 8 when they are no whole words, else 4 bytes a word: `struct { __int8 a[3]; }` is 3,
  `struct { __int8 a; __int16 b; }` 4, `struct { int a : 30; int b : 5; }` 8. The model lays out
  a struct whose array sizes and widths are numbers. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseStruct`; `engine/qclib/qccmain.c`, `QCC_PR_NewType`, `type_size`.
- **FTEF-6.15** A bitfield member (`int a : N`, FTEF-6.13) narrower than 32 bits is `ev_bitfld`
  with its bit offset in its word, `bitofs`, as the layout places it. Read, its word is read and
  extended (`QCC_BitfieldToDef`, `OP_BITEXTEND_I` for an `int`, `OP_BITEXTEND_U` for a `uint`,
  with `QCC_MakeUIntConst((bitofs<<8)|bits)`, which on a target without them is a left and a
  right shift, FTEF-27.7). Stored, but through a pointer, the word is read and the value packed
  into it (`QCC_PR_Statement_BitCopy`): `OP_BITCOPY_I` as a plain statement when the target has
  it, else `OP_BITAND_I` of the word with `~(((1u<<bits)-1u)<<bitofs)`, `OP_BITAND_I` of the
  value with `(1u<<bits)-1u`, `OP_LSHIFT_I` by `bitofs` and `OP_BITOR_I`, each folded or
  emulated as any: on the id target `x.a = 1;` for `struct S { int a : 3; } x;` is the two
  refused opcodes `BITAND_I` and `BITOR_I`, and `x.b = i;` for a second member at bit 3 also the
  `MUL_I` of the shift. Through a pointer it is loaded by its width (`QCC_LoadFromPointer`):
  `OP_LOADP_I8` or `OP_LOADP_U8` for 8 bits, the 16-bit loads for 16, and any other width the
  fatal error `E0353` "ptr-to-bitfld has unsupported bit depth". A pointer to `__int8`,
  `__uint8`, `__int16` or `__uint16` is read by the same loads (FTEF-27.10), its index made an
  integer first and 0 when there is none.
- **FTEF-6.16** A struct's `static` member is no member but the global `STRUCT::NAME` of its
  type ("static members are technically just funny-named globals", `QCC_PR_GetDef(type,
  membername, ...)`, a constant only for a function), so `S::a = 1;` stores it, `float x = S::a;`
  is "initializer for 'x' is not constant", and `S::b` of an instance member is "Unknown value
  \"S::b\"". An array's only member is `length` (FTE-13), its elements' struct's members none, so
  `x.length` of an array of structs is its length and `x.a` "expected length, found a".
- **FTEF-6.17** A struct member's array sizes are each `QCC_PR_IntConstExpr()` (`QCC_PR_ParseArrayType`),
  an integer constant, and a 0 is the fatal error `E0356` "cannot cope with 0-sized arrays":
  `float a[0]`, `a[0x0]` and `a[2][0]` are refused, `a[2.0]` and `a[-1]` are not, `a[1.5]` is
  `E0262` "Value is not an integer constant", and a name the module does not define, there or
  as a bitfield's width (FTEF-6.9), is "Unknown value \"A\"." and then that error. A defined
  name is taken as a constant; an expression of more than a literal or a name is not folded
  (FTEF-19.63).
- **FTEF-6.18** A store through a pointer (`QCC_StoreToPointer`, the type the source's when the
  pointer points at a `variant`): to an 8- or 16-bit integer or a bitfield by `OP_STOREP_I8` or
  `OP_STOREP_I16`, any other width the fatal error `E0353` "ptr-to-bitfld has unsupported bit
  depth", a bitfield that does not start its word first the error `E0357` "left operand (NAME)
  has a bit offset" (not fatal). Otherwise, with an index on a target whose stores take no offset
  (`QCC_OPCode_StorePOffset`: FTE's formats before revision 5712 and DarkPlaces' before 20241108)
  the index is first added to the pointer by `OP_ADD_PIW`. A float, vector, entity, string,
  function or field is stored by its `OP_STOREP_*`; an integer by `OP_STOREP_I`, or else
  `OP_STOREP_FLD`; a 64-bit value by `OP_STOREP_I64`, or else two `OP_STOREP_FLD`, the second at
  the index plus 1 by `OP_ADD_I`, or with no offset the pointer plus 1 by `OP_ADD_PIW`; a pointer
  by `OP_STOREP_P`, `OP_STOREP_I` or else `OP_STOREP_FLD`. A struct or union (and any other type)
  is stored word by word: where stores take an offset, `OP_STOREP_V` for each three words when
  the target has it, then `OP_STOREP_I` (or else `OP_STOREP_F`) for each word left, each word's
  offset added to an index by `OP_ADD_I`; else the pointer offset to each by `OP_ADD_PIW` and
  `OP_STOREP_V` for each three, `OP_STOREP_I64` for each two when the target has it, and
  `OP_STOREP_F`. Each store is a `QCC_PR_SimpleStatement`, an opcode the target lacks the fatal
  error `E0320`. An 8- or 16-bit store the target lacks is emulated
  (`QCC_PR_Statement_SmallStore`): for `OP_STOREP_C` the value made a `uint`; the index (0 when
  there is none) split into the place in its word, `& 3` or `& 1`, and the word, `>> 2` or
  `>> 1`; the word read by `OP_LOADP_I`; the value masked by `0xff` or `0xffff` and shifted by
  `OP_LSHIFT_I` to its place (`OP_MUL_I` of the place by its bits), the mask shifted and
  inverted (`OP_BITNOT_I`), the word's bits there cleared by `OP_BITAND_I` and the two joined by
  `OP_BITOR_I`; and the word stored by `OP_STOREP_I`, or `OP_STOREP_F` when the target lacks it.
  Each is folded or emulated as any opcode (FTEF-27): on the id target `*pc = c;` for an
  `__int8 *pc` is the `memgetval` and `OP_LOADP_I` errors, two `OP_BITAND_I` and an `OP_BITOR_I`.
- **FTEF-6.19** A member reached through a pointer is at an index (`QCC_PR_ParseRefArrayPointer`):
  its offset in units of its alignment (a word, or a byte or two for an 8- or 16-bit integer),
  with no index before it; with one, the index made an integer and multiplied by `OP_MUL_I` into
  the member's units when it is narrower than a word, then, unless the offset is 0, `OP_ADD_I` of
  it and the offset when the target has `OP_ADD_I`, else `OP_ADD_F` of it made a float and the
  offset as a float (times `32/align`). On the id target `pt[i].v = w;` for a `struct { float f;
  vector v; } *pt` is `OP_MUL_I` of `i` by 4 (FTEF-31.7), the index made a float for `OP_ADD_F`
  ("itof emulation") and made an integer again for the store ("ftoi emulation").
- **FTEF-6.20** A member of a struct or union that is a value, not reached through a pointer
  (`st.f`, `sarr[1].f`), is the struct's own definition at the member's word (`QCC_PR_BuildRef`
  of its `sym` with the member's `ofs`), no temporary: its address is the struct's, refused on
  the id target with "Address-of operator is not supported in this form without extensions"
  and on the FTE target a pointer to the member, and `st.n == st.n` of an `int` member is the
  same operand twice, "Result of comparison is constant" on the id target (FTEF-28.1), where
  `st.n == st.m` is not.
- **FTEF-6.21** An 8- or 16-bit integer read through its address (`OP_LOADP_I8`, `OP_LOADP_U8`,
  `OP_LOADP_I16`, `OP_LOADP_U16`) is a value of the opcode's type, `int` or `uint`, as its
  extension at a constant index is (FTEF-2.12): `x_s = carr[i];`, `x_s = *pc;` and `x_s =
  ps->c;` of `__int8`s warn "Implicit cast from __int32 to string", of `__uint16`s "from
  __uint32"; a bitfield read so keeps its type.
- **FTEF-6.23** An 8- or 16-bit integer member of a struct is `ev_bitfld` (FTEF-2.12), read and
  stored as a bitfield of its width at bit 0 of the word that holds it (the member's `bitofs` is
  0): on the id target `st.h = 1;` of a `__uint16 h` is the `OP_BITAND_I` and `OP_BITOR_I` of
  `QCC_PR_Statement_BitCopy`, with no shift, a read is `OP_BITEXTEND_U` by `(0<<8)|16`, and its
  address and a pointer's index to it count its bytes (FTEF-6.19).
- **FTEF-6.24** A struct's member array named bare is its head (`REF_ARRAYHEAD`), with the
  member's first size as its length: `s.a[4]` of a `float a[3]` is "(constant) array index out of
  bounds (0 <= 4 < 3)". Its index is added to the member's word in the struct
  (`QCC_PR_ParseRefArrayPointer`'s `idx`, an `int` on a target with `OP_ADD_I` and else a float):
  `OP_ADD_F` when both are floats, else `OP_ADD_I` of both made integers, so on the FTE target
  `s.a[3.5]` warns "Numerical overflow. 3.500000 will be rounded to 3" and on the id target
  `s.a[i]` is `OP_ADD_I`'s refusal. An element at an index that is no constant is read by the
  struct's `ArrayGet` into its return, which for a struct of more than three words is returned
  through storage `OP_PUSH` makes, refused on a target without it. Through a pointer the element
  is read and stored through the pointer at that index (`ps->a[i]`), and the head is the pointer
  plus the member's offset in bytes by `OP_ADD_I`.
- **FTEF-6.25** A call through a struct's function member reads the parameters of the member's
  type (`QCC_typeparam_s`): a parameter with a default may be left out, and a call that leaves
  out one with none warns "too few parameters on call to NAME, PARAM will be UNDEFINED", NAME the
  struct's definition, the member's `sym`: `w.get()` of a `float(entity e = __NULL__) get` is no
  warning, `w.two()` of a `float(float a) two` names `w`.
- **FTEF-6.26** An error FTEQCC raises about an element of an aggregate initialiser that is
  itself braced stands where its `{` does, the token the parser has read: "inherited structs must
  use designated initialisers" and "member U.b was already ininitialised" are on the `{`'s line,
  whatever follows it on the next.
- **FTEF-6.27** A bitfield's width is the signed `int bits` `QCC_PR_IntConstExpr()` gives, kept in
  the unsigned `bitfld->bits`: a negative width is no error ("too many bits" compares the signed
  value with 64, and no promotion to 64 bits happens for it), and the struct's layout adds it in
  C's unsigned arithmetic (`unsigned int bitsize`), which wraps: `struct S { int a : -1; int b;
  };` puts `b` at bit 0 and is one word, and `struct S { int a : -1; };` is of size 0.
- **FTEF-6.28** A member of a struct or union of size 0 (`!t->size`), one named with no body yet
  or one laid out to no word, is the fatal error `E0362` "NAME was not defined yet"
  (`QCC_PR_ParseRefArrayPointer`), before any member is looked up: `struct T; T *pt;` makes
  `pt->a` that error, and `struct T2 { }; T2 t;` makes `t.a` "T2 was not defined yet", not "a is
  not a member of T2".
- **FTEF-6.22** A struct or union with no tag (`struct { ... } x;`, `typedef struct { ... } T;`)
  is a type of its own, with its members and its layout, named `<struct>` or `<union>`: `t.zz` of
  one is "zz is not a member of <struct>", `sizeof(T)` of `typedef struct { float a; vector v; }
  T;` is 16, and since `typecmp` compares a struct by its size and name, two of them with the
  same size cast to each other (`A a; B b; a = b;` for two `typedef struct { float a; }`), and of
  different sizes are "Cannot cast from struct <struct> to struct <struct>".
- **FTEF-6.14** A struct or union read through a pointer is read word by word
  (`QCC_LoadFromPointer`, `case ev_struct: case ev_union:`): `OP_LOADP_V` for each three of its
  words and `OP_LOADP_I` for each word after them, each a `QCC_PR_SimpleStatement`, which
  emulates nothing and refuses an opcode the target lacks with the fatal error `E0320` "Opcode
  \"=|LOADP_V\" not valid for target. ..." (the first refused ends the statement); with an index,
  each word's offset is added to it by `OP_ADD_I`. A member reached through a pointer, `p->a`,
  `p.a`, `(*p).a` or `p[0].a` for a `struct S *p`, is read by the load of its own type at its
  offset (FTEF-27.10), the pointer read first, so `(*pp)->a` loads `*pp` with `OP_LOADP_I` and
  then `a` with `OP_LOADP_F`. On the id target `local S y = *p;` is "Opcode \"=|LOADP_I\" not
  valid for target" for a one-word `S`, and `OP_LOADP_V` for one of four words.
- **FTEF-6.29** A struct or union member that is a type with nothing after it but `;`, other
  than a struct's or union's, whose members it nests, is the warning `W0204` "declaration does not
  declare anything (TYPE)" (`WARN_POINTLESSSTATEMENT`), and declares nothing: `struct S { float;
  };`. A `virtual` member that is no function is the error `E0369` "[non]virtual members must be
  functions", which goes on (`if ((isnonvirt || isvirt) && type->type != ev_function)`; FTEQCC
  reads no `nonvirtual` in a struct). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseStruct`.
- **FTEF-6.30** A name as a member's array size or bitfield width (`QCC_PR_IntConstExpr`) is an
  integer constant only when it names a constant: `float y; struct S { int a : y; };` and `float
  a[y];` are "Value is not an integer constant", as an unknown name is. An unknown name read there
  is defined a global `variant` (FTEF-19.71), "Unknown value \"NAME\".", so `float u = A;` after
  `struct S { float a[A]; };` is only "initializer for 'u' is not constant". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_IntConstExpr`, `QCC_PR_ParseValue`.

## FTEF-7 `typedef` of any type

**Source:** `qcc_pr_lex.c`, `QCC_TypeForName` and the typedef path (`ev_typedef`,
`QCC_PR_NewType(..., true)` adding to `typedeftable`); `qccmain.c`, the `keyword_typedef`
entry, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset:
`typedef struct` only, `dialect-fteqcc-rerelease.md` FTE-14.

Vanilla `qcc` has no `typedef` (QC-10.1). The rerelease uses it only before `struct`; the full
dialect aliases any type.

### Behaviour

- **FTEF-7.1** `typedef OldType NewName;` defines `NewName` as another name for `OldType`.
  `QCC_TypeForName` resolves a name to its real type, following an `ev_typedef`'s `aux_type`.
  The corpus typedefs function types, pointer types and enum/struct types (187 `typedef`
  occurrences across the three mods).
- **FTEF-7.2** A typedef name is usable wherever a type is (`QCC_PR_ParseType` resolves it
  through `QCC_TypeForName`): as a member type, a parameter type, a cast, or the base of a
  pointer or array.
- **FTEF-7.3** Codegen (later): a typedef is a front-end alias and emits nothing of its own
  (its underlying type's codegen applies); confirmed by L-01.

- **FTEF-7.4** `QCC_PR_ParseDefs` loops over its declarators after `typedef` as it does after any
  type (`do { ... } while (QCC_PR_CheckToken (","));`), each one defining a type, with its `*`s,
  parameter lists, `(*name)(params)` and array dimensions, and no initialiser or body (`if
  (istypedef) { ...; def = NULL; continue; }`): `typedef float t, u;`, `typedef float t, *u;`,
  `typedef void() fn, fn2;`, `typedef struct S { float a; } T, *PT;` and `typedef enum { A } E,
  F;` define two types each. `typedef` is read among the qualifiers (`istypedef=true;` in the loop
  of storage classes) before or after the others and any number of times, so `typedef typedef
  float tt;` defines `tt`, and `local typedef float t;` in a body defines `t` there. After
  `typedef` the declarators are those of any definition: `typedef class C { float a; } CC;` is
  `QCC_PR_ParseEntClass`'s type (FTEF-9.8). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseDefs`.
- **FTEF-7.5** A definition whose type is a typedef of a function type is a function:
  `QCC_PR_ParseDefs` reads the type with `QCC_PR_ParseType`, which finds the typedef's type
  (`QCC_TypeForName`), and then tests the type, not its spelling (`type->type == ev_function`), so
  `{`, `[` and `#` after the declarator are the function's body, state header and builtin, and `=
  {` its body: `typedef void() fn; fn g = { return; };`, `fn g = #6;`, `fn g : 6;`, `fn g = [1, g]
  {};`, `typedef float(float) fn; fn g = { return 1; };`, `typedef void(float a) fn; fn g = { a =
  1; };`, with `static` or `var`, in a body, as a class member or a struct's `static` or `virtual`
  member, in `extern "C"` and `extern "QC"` blocks, through a chain `typedef fn fn2;`, through a
  macro, and from a typedef a body makes, under no option, `-Fcaseinsens` and `qcfuncs` off; `fn g
  = { 1 };` is a body missing a `;`. The typedef's declarator may be C's `(*name)(params)`
  (FTEF-21.7) or `name(params)`, which make the function type. A typedef whose declarator is
  `(name)` is a function type whose parameter `name` is no type, "expected function arg list" as
  `T (name);` is (FTEF-19.35): `typedef float (t);` and `typedef float t; typedef t (u);` are
  refused. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseDefs`; `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseType`.

## FTEF-8 `enum`, `enumflags` and typed or scoped enums

**Source:** `qcc_pr_comp.c`/`qcc_pr_lex.c`, `QCC_PR_ParseEnum` (bare and `flags` forms);
`qccmain.c`, the `keyword_enum` ("kinda like in c") and `keyword_enumflags` ("like enum, but
doubles instead of adds 1") entries, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.
Rerelease subset: the bare `enum { ... }` and `enum : int` / `enum class` forms are
`dialect-fteqcc-rerelease.md` FTE-15.

Vanilla `qcc` has no `enum` (QC-11). The rerelease subset uses the plain and typed forms; the
full dialect also uses `enumflags`.

### Behaviour

- **FTEF-8.1** `enumflags { A, B, C };` declares named constants whose unvalued members double
  rather than increment: FTEQCC's `QCC_PR_ParseEnum(flags=true)`, described in `qccmain.c` as
  "like enum, but doubles instead of adds 1." So `A, B, C` are 1, 2, 4. The corpus uses it for
  bit masks.
- **FTEF-8.2** `enumflags`, like `enum`, takes an explicit value (`A = n`) that sets the
  running value, and an optional element type (`: int`); the typed and `enum class` forms
  resolve as FTE-15.2 states. A bare `enum`'s increment-by-one semantics are FTE-15.1.
- **FTEF-8.3** Codegen (later): each enum name is a folded constant and the enum contributes no
  global of its own (as FTE-15.3 for the subset); confirmed by L-01.
- **FTEF-8.4** An `enum` or `enumflags` where a type belongs always has its body:
  `QCC_PR_ParseType` sends both keywords to `QCC_PR_ParseEnum` (`else if
  (QCC_PR_CheckKeyword(keyword_enum, "enum")) type = QCC_PR_ParseEnum(false);`), which reads an
  optional `class`, a name or legacy base type and an optional `: type`, then
  `QCC_PR_Expect("{");` on every path that has not already read the `{`
  (`qcc_pr_comp.c`, `QCC_PR_ParseEnum`). So `enum colour_e c;` and `typedef enum colour_e C;`
  are "expected {" errors; an enum is named elsewhere by its type name alone (FTE-15.4),
  `colour_e c;`.
- **FTEF-8.5** `enum class` needs its base type: after `strictenum = QCC_PR_CheckName("class");`
  and the name, `QCC_PR_ParseEnum` reads `:` and the type, `else if (strictenum)
  QCC_PR_Expect(":");`, so `enum class E { A };`, `enum class { A };` and `enum class E;` are
  "expected :, found {" (or `;`), and `enum class E : float { A };` is an enum; FTE-15.2's `enum
  class` names are those of an enum with a base type. The declarators follow the body as they
  follow any type (FTEF-15.6): `enum E { A } x;` is `E x`, `enum { A } x;` a `float x` (the
  base), and `enum class E : float { A } x;` an `E x` that no float takes (FTEF-19.10). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseEnum`.
- **FTEF-8.6** A named enum that is not `enum class` makes a type of its name with its base's
  basic type, the first declaration's (`enumtype = QCC_TypeForName(name); if (!enumtype) ...
  enumtype = QCC_PR_NewType(name, basetype->type, true);`), while its names stay of the base
  type: a value of it casts as the base does and prints by its name, so `enum EN { A }; EN e;
  string s = e;` is `E0213` "Cannot cast from EN to string", `enum EN : int { A }` makes it
  `W0220` "Implicit cast from EN to string", `int i = e;` warns "ftoi emulation" as a float does,
  and `string s = A;` casts the null constant. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseEnum`.

## FTEF-9 Classes

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseEntClass` (parsed by `QCC_PR_ParseType` on the
`class` keyword, guarded by `flag_qcfuncs`); `qccmain.c`, the `keyword_class` entry,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Classes are entity subtypes
([`progs.md`](progs.md) PROGS-16, entities; [`edicts.md`](edicts.md), fields).

Vanilla `qcc` has no `class`; the word is a name and "not a type" (QC-11). The corpus is
heavily class-based (547 `class` declarations across the three mods).

### Behaviour

- **FTEF-9.1** `class Name { ... };` and `class Name : Parent { ... };` define an entity
  subtype whose members are entity fields, inheriting the parent's fields (`QCC_PR_ParseEntClass`
  reads an optional `: Parent`, requires the parent be a class/entity type, and sets
  `newt->parentclass`). `class Name;` with no body is a forward declaration.
- **FTEF-9.2** A member function is declared with `virtual`, `nonvirtual` or `static`
  (`QCC_PR_ParseEntClass`'s keyword loop). A `virtual` function becomes an entity field that
  can be overridden per instance; a `nonvirtual` or `static` function is a global named
  `Class::member`. FTEQCC warns "`%s::%s` was not qualified. Assuming non-virtual." when a
  member function has no qualifier. `nonvirtual:` and `virtual:` as a label set the default for
  the following members (the "fteqcc-specific" `assumevirtual` path).
- **FTEF-9.3** A non-function member is a field of the entity (`QCC_PR_ParseEntClass` builds an
  `ev_field` type per member and unions it into the class's field layout). `self.member` or
  `this.member` selects it. `const`, `var`, `static`, `ignore`/`strip` and the access labels
  `public`/`private`/`protected` are accepted qualifiers; access control is not enforced
  (the labels are read and ignored).
- **FTEF-9.4** A member function body `Class::member = { ... }` or `= [frame, think] { ... }`
  defines it. A `nonvirtual`/`static` member has no implicit `this`; a `virtual` or plain
  member receives `this` as a pointer to the class (`QCC_PR_MakeThiscall` prepends a `this`
  parameter). A member whose name equals the class name is a constructor; FTEQCC warns if it
  is not `void`, takes arguments, or is `static`.
- **FTEF-9.5** For each class with a constructor FTEQCC defines a `spawnfunc_Name` function so
  maps can spawn it (`QCC_PR_ParseEntClass`'s trailing `QCC_PR_GetDef("spawnfunc_%s", ...)`).
- **FTEF-9.6** Codegen (later): the field layout, vtable and `this`-call lowering, and the
  version 7 requirement, are fixed by L-01.

- **FTEF-9.7** `QCC_PR_ParseEntClass` reads a class's parent with `QCC_TypeForName`, so after
  `:` a name that is no type is the error `E0240` "Parent class NAME was not yet defined"
  (`QCC_PR_ParseError(ERR_NOTANAME, "Parent class %s was not yet defined", parentname);`), also
  for the class itself (`class C : C {`), and a type that is no entity (`if (fieldtype->type !=
  ev_entity)`), a struct, an enum or `float`, is `E0241` "Parent type NAME is not a
  class/entity"; `entity`, a class and a typedef of one are parents. A class given its members
  again with a parent other than the first is `E0242` "Parent class changed on redeclaration of
  NAME" (`if (redeclaration && fieldtype != newt->parentclass)`). Under `-Fcaseinsens` the
  parent is found in any case (FTEF-19.33), so `class D : c` after `class C` is a class of `C`.

- **FTEF-9.8** `QCC_PR_ParseType` reads `class` with `qcfuncs` on as the type
  `QCC_PR_ParseEntClass` makes (`if (flag_qcfuncs && QCC_PR_CheckKeyword (keyword_class, "class"))
  type = QCC_PR_ParseEntClass();`), wherever a type stands, and the declarators follow it as they
  follow any type. `class Name` with no `{` returns the type, a forward declaration
  (`forwarddeclaration = !QCC_PR_CheckToken("{")`), so `class C; class C x;`, `class C { float a;
  } x;`, `void() g = { class C x; };`, `class C *p;` and `void(class C x) h = #6;` define a
  variable; a parent needs the body (`QCC_PR_Expect("{")`), so `class D : C;` is "expected {"; a
  name `QCC_TypeForName` finds is that type, `class float x;` a `float` named x; and the name
  `QCC_PR_ParseName` reads must be one. A class given a body in a function's body is "Declaration
  of class %s%s%s within function". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.9** In `QCC_PR_ParseEntClass`'s loop over a member's keywords `public:` and `private:`
  read one `:`, and `protected:` reads two (`QCC_PR_Expect(":"); ... QCC_PR_Expect(":");`), so
  `protected: float b;` is "expected :, found float" and `protected: : float b;` is a label;
  `virtual` and `nonvirtual` are labels only when a `:` follows (`firstkeyword &&
  QCC_PR_CheckToken(":")`); a label is followed by the member (`basetype = QCC_PR_ParseType(false,
  false, false)` is "In class %s, expected type"), so a label at the `}` is "is not a type"; and a
  `;` where a member starts is "is not a type". A first token `,` is "member missing name".
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.10** A member given a value with `=` is read as a function's body or value (`havebody =
  true`); "Can only initialise member functions" when it is not of function type and not `static`
  (`const` makes a member static), unless the value is `0` or `0i` (`isnull`): `class C { float a
  = 1; };` is refused, `class C { float a = 0; };`, `class C { static float a = 1; };` and `class
  C { const float a = 1; };` are not, and `var` does not make one static. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.11** A member whose type is a `struct` or `union` is "Struct or union in class %s" (`if
  (basetype->type == ev_struct || basetype->type == ev_union)`), a pointer to one is not. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.12** `QCC_PR_ParseEntClass` finds the class's name with `QCC_TypeForName(classname)`: a
  type with members (`if (newt && newt->num_parms != 0) redeclaration = true;`), a struct or union
  among them, is a redeclaration, "Parent class changed on redeclaration of %s" when its parent is
  not the one given (`if (redeclaration && fieldtype != newt->parentclass)`): `struct S { float a;
  }; class S { float b; };`. Another type with a size, `float`, a typedef or an enum, is taken as
  the class (`if (!newt)` makes none) and given the members, and its `spawnfunc_NAME` is declared
  (`QCC_PR_GetDef(type_function, membername, NULL, true, 0, GDF_CONST)`) and, the type being no
  entity, never given a body, "function spawnfunc_float has no body" (FTEF-17.11): `class float {
  float a; };`, `typedef float T; class T { float b; };` and `enum E { A }; class E { float b; };`
  are refused, which this profile reports at the class's name as "Parent class changed on
  redeclaration of NAME" (`E0242`), the program being refused either way. A class given its
  members again in another case under `-Fcaseinsens` is the class (FTEF-19.33), whose members are
  compared (FTEF-9.7): `class C { float a; }; class c { float b; };` is "Incompatible redeclaration
  of class c. b differs." with it and two classes without. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.13** A class given its body again is compared with the first, member by member in order
  (`if (newt->params[i].arraysize != parms[i].arraysize || typecmp(newt->params[i].type,
  parms[i].type) || strcmp(newt->params[i].paramname, parms[i].paramname))`), "Incompatible
  redeclaration of class %s. %s differs." at the first that differs in name, type or array size,
  and "Incompatible redeclaration of class %s." when the count differs (`if (newt->num_parms !=
  numparms)`): `class C { float a; }; class C { float b; };` and `class C { float a; }; class C {
  };` are refused, `class C { float a; }; class C { float a; };` and a forward `class C;` before
  either are not; under `-Fcaseinsens` `class c { float b; }` after `class C { float a; }` is the
  class given again (FTEF-9.12). A member's type is a type (`basetype = QCC_PR_ParseType(false,
  false, false)`, "is not a type"), and a struct or union by its name is "Struct or union in
  class" as by its keyword (FTEF-9.11): `class O { T x; };` with no type `T`, and `struct S {
  float a; }; class O { S x; };`, are refused. A `nonvirtual`, `static` or `ignore` member is a global of the class's or nothing, not among the members compared (`continue` before `parms[numparms]` is filled), and a member's array size is compared (`newt->params[i].arraysize != parms[i].arraysize`): `class C { float a; }; class C { float a[2]; };` and `class C { float a; }; class C { ignore float a; };` are refused. The
  constructor is the member named exactly as the class (`!strcmp(parmname, classname)`), under
  `-Fcaseinsens` too: `class c { C(); };` is "function c::c has no body" with it, the name `C`
  being the class's type there and no constructor. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseEntClass`.
- **FTEF-9.14** A class is an entity type (`QCC_PR_NewType(classname, ev_entity, ...)`), and an
  implicit cast between two entity types, a class either side, is made
  (`QCC_TryEvaluateCast`, `totype == ev_entity && src.cast->type == ev_entity`): with the warning
  `W0220` "Implicit cast from A to B" when neither the value's type nor a class it derives from is
  the target to `typecmp_lax` (`for (t = src.cast; t; t = t->parentclass) if
  (!typecmp_lax(t, cast)) break;`), a class with no parent class deriving from the entity.
  `typecmp_lax` compares no class names but a class's members, its `params`, as a function's
  parameters: as many on each side (`if (!a->params[t].optional) return 1;` past the shorter),
  each of one basic type, an entity or struct by its name and anything else by `typecmp_lax`; the
  members are those a redeclaration compares (FTEF-9.13), the class's own, no parent's, no
  `nonvirtual`, `static` or `ignore` one, and an entity has none. A type is itself (`if (a == b)
  return 0;`), so a class with a member of its own type compares. So `C c = e;` for an entity `e`
  warns "Implicit cast from entity to class C" when `C` has a member and not when it has none; a
  class to `entity` never warns, its chain ending at the entity; `D d = c;` for a `C c` and `class
  D : C` does not warn (`D`'s own members, `float b` against `C`'s `float a`, are one), and two
  unrelated classes warn when their members differ, `class C { float a; }` from `class D { string
  b; }` or from `class D { float a; nonvirtual void() m; }` with `C`'s `m` virtual. A cast
  written out, `(C)e`, never warns. A global initialised with one of a variable, `C c = (C)e;`, is
  no constant (FTEF-34.9).
- **FTEF-9.15** A member function made `virtual` that a parent class has as a nonvirtual
  function is the error `E0350` "NAME defined as virtual in CLASS, but nonvirtual in PARENT"
  (`QCC_PR_ParseEntClass`, the first parent up the chain with a global `PARENT::NAME`), and the
  member is not defined: `class C { nonvirtual void() m = {}; }; class D : C { virtual void() m =
  {}; };` is refused.
- **FTEF-9.16** A member function's qualifiers (`QCC_PR_ParseEntClass`): the member named exactly
  as the class, the constructor, warns `W0265` "Constructor CLASS::TOKEN should not be static."
  under `static` or `const`, "Constructor CLASS::TOKEN does not return void." when it returns a
  value and "Constructor arguments are not supported at this time CLASS::TOKEN. Use named fields
  instead." when it has parameters or `...`, TOKEN being the token after its name and array
  suffix (`pr_token`, as `=`, `;` or `,`). Any other member function with none of `virtual`,
  `nonvirtual`, `static` and `const`, and no `virtual:` or `nonvirtual:` label before it in the
  class (`assumevirtual == 0`), warns `W0265` "CLASS::NAME was not qualified. Assuming
  non-virtual." and is nonvirtual; one with more than one of `virtual`, `nonvirtual` and
  `static` or `const` is the error `E0351` "Multiple conflicting qualifiers on CLASS::TOKEN."
  (`if (isvirt+isnonvirt+isstatic != 1)`). A member function, virtual or not, that nothing
  defines is "function CLASS::NAME has no body" (FTEF-19.41), by the name it is defined as; an
  array of member functions is an array of fields and is not reported.
- **FTEF-9.19** A class's `vector` data member, no array, is a `.vector` field, which has its
  components' fields too, `NAME_x`, `NAME_y` and `NAME_z` (`QCC_PR_GetDef` of a vector field
  makes them), so `c.v_x = 1;` stores one and `c.v_w` is "Unknown field \"v_w\" in class \"C\"";
  a `vector v[2]` member has none.
- **FTEF-9.20** A class member that is no function written `virtual` or `nonvirtual` is "virtual
  keyword on member that is not a function", and one written `inline` "inline keyword on member
  that is not a function"; an `inline` member function with no body, or a virtual one, the class's
  default included, is "inline keyword on function prototype or virtual function". Each is a
  `QCC_Error`, which ends FTEQCC, the fatal error `E0379`, and a typedef of a function type makes
  a function. The words are those written, not the class's `virtual:` or `nonvirtual:` default.
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseClass`.
- **FTEF-9.18** A `static` or `const` data member is the global `CLASS::NAME` of its type, no
  field, a `const` one a constant (`GDF_CONST`), and `CLASS::NAME` names a member through the
  class (`QCC_PR_GetDef` of `CLASS::NAME`): a data member that is no global is the field, of
  type `.TYPE`, and a function, `virtual` or not, the function `CLASS::NAME`, a constant. So
  `float x = C::a;` is "Cannot cast from .float to float" for `float a`, "initializer for 'x' is
  not constant" for `static float a`, and accepted for `const float a = 1`; `void() x = C::m;` is
  accepted for any member function `m`.
- **FTEF-9.17** The `0` or `0i` a member that is no function may be given (FTEF-9.10) is the
  token's text (`QCC_PR_CheckImmediate("0") || QCC_PR_CheckImmediate("0i")`, a `STRCMP` with
  `pr_token`), not its value: `0x0`, `00`, `0.0` and `-0` are "Can only initialise member
  functions". A vector immediate is lexed component by component into `pr_token`, which keeps
  the last component's text, so `'1 0 0'` and `'0 0 0'` are taken and `'0 0 1'` is refused.

## FTEF-10 Accessors

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseAccessorMember` and the `accessor` branch of
`QCC_PR_ParseType`; `qccmain.c`, the `keyword_accessor` entry,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `accessor`, `get` or `set`; the words are names (QC-11). The corpus's
`fteextensions.qc` declares engine handle types as accessors (`accessor strbuf : float;` and
others).

### Behaviour

- **FTEF-10.1** `accessor Name : Base { get ...; set ...; };` defines a type `Name` that is
  stored as `Base` (its parent type) but whose reads and writes go through get and set
  functions (`QCC_PR_ParseType`'s `accessor` branch builds an `ev_accessor` type whose
  `parentclass` is `Base` and whose size matches `Base`). `accessor Name : Base;` with no body
  declares the handle type alone, as `fteextensions.qc` does.
- **FTEF-10.2** A `get` member defines the read function and a `set` the write
  (`QCC_PR_ParseAccessorMember`, keyed by `setnotget`); an optional `[IndexType]` makes it an
  indexed accessor (`name[i]` calls the getter or setter with the index). `inline` and `&`/`*`
  on the member select by-reference or by-pointer forms. The functions are named
  `Name::get_field` and `Name::set_field`.
- **FTEF-10.3** Inside a `class` (FTEF-9), `get` and `set` members declare accessor members of
  that class (`QCC_PR_ParseEntClass` dispatches `isget`/`isset` to
  `QCC_PR_ParseAccessorMember`). Virtual and static accessors are rejected ("virtual accessors
  are not supported", "static accessors are not supported").
- **FTEF-10.4** **Withdrawn** (P2-136, #587): it left an accessor's reads and writes to code
  generation; the check stage reads them. Replaced by FTEF-10.7.

- **FTEF-10.5** `QCC_PR_ParseType` reads `accessor` among the types (`QCC_PR_CheckKeyword
  (keyword_accessor, "accessor")`), wherever a type stands: `typedef accessor B : float;`, `static
  accessor A : float;` and `accessor A : float x;` define or declare with the declarators that
  follow. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_lex.c`, `QCC_PR_ParseType`.
- **FTEF-10.6** `accessor Name : Base` reads the base with `QCC_PR_ParseType(false, false,
  false)`, "is not a type" when it is none, and `QCC_PR_ParseError(ERR_NOTANAME, "Accessor %s
  cannot be based upon %s", ...)` when it is a struct or union (`if (!type || type->type ==
  ev_struct || type->type == ev_union)`): `accessor A : struct S { float a; };` and `struct S {
  float a; }; accessor A : S;` are refused, `accessor A : float *;` is not. A name that is another
  type is "Type %s cannot be redefined as an accessor" (`if (newt && newt->type != ev_accessor)`),
  `typedef float A; accessor A : float;`, while a variable's name is free, `float A; accessor A :
  float;`. A later `accessor A : Base` with another base is "Accessor %s basic type mismatch (%s,
  expected %s)" (`else if (type != newt->parentclass)`), `accessor A : float; accessor A :
  vector;`, and with the same base is accepted; under `-Fcaseinsens` `a x;` after it is a variable
  of the accessor (FTEF-19.33). An accessor declared with its base where a class member's type stands, `class O { accessor A : float x; };`, ends FTEQCC too, which this profile reports as "Accessor A declared with its base in class O" (`E0258`). An accessor with no `:`, `accessor A;` or `accessor A : float; accessor A;`, reads the null `type` (`newt->size=type->size;`, `TypeName(type, ...)`) and ends
  FTEQCC, which the oracle records as refused; this profile reports "Accessor A cannot be based
  upon ??", the message of the path where `parentname` is "??". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseType`.

- **FTEF-10.7** A member of a value whose type has accessor members, by name
  (`QCC_PR_ParseField`), is a reference to it (`QCC_PR_BuildAccessorRef`, `REF_ACCESSOR`), of the
  type of the member declared first, read-only as the value is; a member that takes an index is
  given one by `[...]`, and an accessor's nameless indexer by `value[...]`. A read calls its `get`
  function with the value and the index, made the indexer's type (`QCC_SupplyConversion`,
  "Implicit type mismatch. Needed float, got string.") or an integer 0 with none; with no `get` it
  is the fatal error `E0348` "Accessor NAME has no get function". A store calls its `set`
  function with the value, the index and the stored value, which is cast to the `set` member's
  type as an argument is ("Cannot cast from float to string"); with no `set` it is the fatal error
  `E0349` "Accessor has no set function". A name no member of the type has, on a type that is no
  entity, is `E0261` "NAME is not a member of TYPE" (`QCC_PR_ParseWarning(ERR_BADMEMBER, ...)`,
  so the read goes on), and the member is a `void` 0: `a = h.nope;` is then "Cannot cast from
  void to float".
- **FTEF-10.8** An accessor type and its base cast to each other with the warning `W0264`
  "Implicit cast from A to B" (`QCC_EvaluateCast`, `cast->parentclass == src.cast`, the warning of
  code 0), a literal of the base's own type to the accessor too (`a = 1;` for an `accessor A :
  float`, not `a = 1i;`); a value of an accessor type is otherwise cast as its base's, and a cast
  to one is checked against its base, "Cannot cast from string to float" for an `accessor H :
  string`. No opcode takes an accessor (`(*op->type_a)->type != lhsd.cast->type`), so an
  operator's operand of one is cast to the opcode's type, `a * f` warning "Implicit cast from A
  to float", and `-a` is the fatal error "type mismatch for -" (FTEF-30.4). A member that takes
  an index is followed by its `[` (`QCC_PR_Expect("[")`), `a.z = f` the fatal error "expected [,
  found =". A
  member's body is a function (`QCC_PR_GenFunctionType`) whose `this` is of the accessor's type,
  whose index is of its indexer's and whose `value`, for `set`, is of the member's, and is
  checked as any function's body is: `bufstr_get(this, idx)` with a `float` parameter warns
  "Implicit cast from strbuf to float".
- **FTEF-10.9** `QCC_PR_ParseAccessorMember` reads a member's type as a type's name and its `*`s,
  so a parameter list after it stands where the `=` is expected: `get float() x = {};` and `set
  void(float v) y = {};` are the fatal error `E0204` "expected =, found (".
- **FTEF-10.10** In a class, `get` and `set` are among the qualifiers `QCC_PR_ParseClass` reads
  before a member's type, in any order with `nonvirtual`, `virtual`, `static`, `const`, `var`,
  `ignore`, `strip` and `inline`, and either makes the member an accessor's, a `set` one when
  `set` is among them: `virtual get float x = { ... };` and `get virtual float x`. `virtual` among
  them is the error `E0377` "virtual accessors are not supported at this time", which reads on,
  and `static` or `const` the fatal error `E0378` "static accessors are not supported". A name
  after the type is the member's, `get` and `set` included. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseClass`.
- **FTEF-10.11** An accessor member whose value is a name, `get float x = g;`, declares that
  function with the member's function type (`QCC_PR_GetSRef(functype, funcname, NULL, true, 0,
  GDF_CONST)`): `this` of the accessor's or class's type, then the index and for `set` the value,
  returning the member's type or for `set` nothing. So a definition of the name with another type,
  before the member or after it, is "Type mismatch on redeclaration of g. float(A this), should be
  float()", in the order the two are read, and a name nothing defines is a function with no body.
  The name stays a reference to the function. The `=` is required (`QCC_PR_Expect("=")`): a
  member with neither a body nor a function is "expected =, found ;". This profile compares an
  accessor's or class's type with any other as `fteqcc_same` does, so a function that differs
  from the member's only in such a parameter is not reported. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseAccessorMember`.
- **FTEF-10.12** A second `get` or `set` member of one name defines its function again:
  `QCC_PR_ParseAccessorMember` parses the body into the definition with no test that it has one
  (`f = QCC_PR_ParseImmediateStatements(...); def.sym->initialized = 1;`), so `get float x = {
  return 1; }; get float x = { return 2; };` in an accessor or a class is no "redeclaration of
  function body". A function defined twice outside an accessor still is. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseAccessorMember`.

## FTEF-11 The `__variant` type

**Source:** `qcc_pr_lex.c`/`qccmain.c`, `type_variant` (`QCC_PR_NewType("__variant",
ev_variant, ...)`) and the `ev_variant` cases of `typecmp_lax`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has no `__variant`; the word is a name and "not a type" (QC-11). The corpus uses
`__variant` and `__variant *` for generic values and allocations (119 occurrences).

### Behaviour

- **FTEF-11.1** `__variant` is a type that any other type is compatible with: `typecmp_lax`
  returns "equivalent" whenever either side is `ev_variant` ("variants and args don't make
  sense, and are considered equivelent(ish)"). It is used for generic parameters and return
  values, and `__variant *` (FTEF-4) for untyped memory.
- **FTEF-11.2** An assignment or call that mixes `__variant` with a concrete type resolves
  without a type error, because of FTEF-11.1; the concrete type governs the operation.
- **FTEF-11.3** Codegen (later): how a `__variant` value is stored and dispatched is fixed by
  L-01.
- **FTEF-11.4** FTEF-11.1 holds only after `typecmp_lax` compares the two types' `aux_type`s
  (`if (typecmp_lax(a->aux_type, b->aux_type)) return 1;` comes before the `ev_variant` return),
  so a `__variant` differs from a pointer or a function, whose `aux_type` it lacks: `float **p;
  __variant *q; q = p;` and `void() *r; q = r;` are `W0220` "Implicit cast from float** to
  __variant*", as is `p = q;`, while `__variant **t; t = p;` and `string *s; q = s;` cast with
  no warning.

## FTEF-12 The remaining `#pragma` directives

**Source:** `qcc_pr_lex.c`, `QCC_PR_Precompiler` (the `pragma` case and its sub-cases),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset: `#pragma warning`
is `dialect-fteqcc-rerelease.md` FTE-16.

This project's vanilla preprocessor (QC-9) handles `#include`, `#define` and the conditionals,
which stay vanilla, and QC-9.9 makes any other directive an error; `dialect-fteqcc-rerelease.md`
FTE-16 covers `#pragma` in general and `#pragma warning` in particular. The corpus uses more
`#pragma` names, listed by their recorded counts across the three mods below.

### Behaviour

- **FTEF-12.1** `#pragma TARGET name` chooses the output target (`QCC_PR_Precompiler`, the
  `TARGET` case, calling `QCC_OPCodeSetTargetName`); it warns "Unknown target ... Ignored." and
  lists `ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)` for an unknown one. The corpus uses it 28
  times.
- **FTEF-12.2** `#pragma PROGS_DAT path` (and the lowercase `progs_dat`, and `#pragma output`)
  sets the output file unless the command line named one (`QCC_PR_Precompiler`, the `PROGS_DAT`
  and `output` cases, calling `QCC_JoinPaths` into `destfile`). `#pragma PROGS_SRC` is accepted
  and silently ignored. The corpus uses `PROGS_DAT`/`progs_dat` 26 times.
- **FTEF-12.3** `#pragma sourcefile path` registers an additional source file to compile
  (`QCC_PR_Precompiler`, the `sourcefile` case, calling `QCC_RegisterSourceFile` for each
  parsed name). The corpus uses it 9 times.
- **FTEF-12.4** **Withdrawn** (P2-116, #544): it gave the form as `#pragma flag name on|off`, but
  `QCC_PR_Precompiler` reads the state first and then the names. Replaced by FTEF-12.11.
- **FTEF-12.5** `#pragma forcecrc n` forces the progs definitions CRC
  ([`progs.md`](progs.md) PROGS-5) to `n` (`QCC_PR_Precompiler`, both the `#pragma forcecrc`
  sub-case and the top-level `#forcecrc` directive, setting `ForcedCRC`). The corpus uses it 4
  times.
- **FTEF-12.6** `#pragma noref n` and `#pragma nosave n` set the file-wide defaults for the
  `noref` and `nosave` qualifiers (`QCC_PR_Precompiler`, setting `defaultnoref` and
  `defaultnosave`); `nosave` is FTE-12's qualifier applied by default. The corpus uses `noref`
  2 times.
- **FTEF-12.7** FTEQCC recognises further `#pragma` names the corpus does not currently use but
  which the FTEQCC profile must accept without error (`QCC_PR_Precompiler`'s remaining
  sub-cases): `optimise`/`optimize`, `opcode`, `autoproto`, `once`, `file`, `line`,
  `includedir`, `defaultstatic`, `framerate`, `compress`, `copyright`, `wrasm`,
  `DONT_COMPILE_THIS_FILE`. An unrecognised `#pragma` is accepted with the warning "Unknown
  pragma '%s'" (as FTE-16.2), not an error.
- **FTEF-12.9** **Withdrawn** (P2-42, #429): it said a later `#pragma autoproto` reports
  nothing, but FTEQCC warns for it. Replaced by FTEF-12.10.
- **FTEF-12.10** **Withdrawn** (P2-132, #569): it said a `#pragma autoproto` is taken only before
  any definition and otherwise warns, but `numpr_globals` is `RESERVED_OFS` for the whole parse,
  so FTEQCC takes it wherever it stands and never warns. Replaced by FTEF-12.18.
- **FTEF-12.8** **Withdrawn** (P2-116, #544): it said a `#pragma` does not affect the token
  stream, but `#pragma flag` turns on and off flags `QCC_PR_Lex` reads. Replaced by FTEF-12.12.
- **FTEF-12.11** The build pragmas are read as `QCC_PR_Precompiler` reads them, against FTEQCC's
  tables (`qc_preproc`'s `fte_tables.rs`, written out of `qccmain.c` and `qcc_pr_comp.c`), each
  warning FTEQCC gives being its Q120 (`WARN_BADPRAGMA`, `W0113`) unless said otherwise.
  `#pragma flag STATE NAME ...`, and `keyword`, reads its text with `QCC_COM_Parse`: STATE
  `enable` or `on` turns each NAME on and `disable` or `off` off, in any case; any other STATE is
  "compiler flag state not recognised" and sets nothing. Each NAME is the first row of
  `compiler_flag[]` whose `abbrev` it is in any case (`QC_strcasecmp`), "keyword/flag NAME not
  recognised" when none is, and "Cannot enable/disable keyword/flag via a pragma" when the row
  lacks `FLAG_MIDCOMPILE` (`acc`, `parms`, `autoproto`, `wasm`, `caseinsens`, `int`,
  `integer`). The flags start as `QCC_main` sets them, `FLAG_ASDEFAULT` on, and `QCC_SetDefaultProperties`
  does not set them again, so a pragma's setting holds into the second autoprototyping pass
  (`qc_preproc::FlagSettings`). `#pragma optimise` (or `optimize`) with a level `0` to `3`, or a
  text starting `addon` or `mutator` in any case, sets the optimisations; any other text, after
  an optional `no-`, is matched in any case against `optimisations[]`' `abbrev` and `fullname`,
  "pragma NAME: TEXT unsupported" when it matches none (NAME the pragma's name as written).
  `#pragma opcode STATE NAME ...` takes `enable`, `on`, `disable` or `off` ("opcode state not
  recognised" for any other) and names `pr_opcodes[]`' `opname`s in any case, "opcode NAME is
  internal" for one past `OP_NUMREALOPS` and "opcode NAME not recognised" for none. `#pragma
  TARGET NAME` takes its first token, a `targets[]` name in any case with an optional `_` and
  revision (`fte_5768`): an unknown one is "Unknown target 'NAME'. Ignored." and a line "Valid
  targets are: ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)", and a revision past the format's highest
  "target revision N is unknown, assuming revision M", both warnings with no name (`W0117`).
  Any pragma name `QCC_PR_Precompiler` does not know is "Unknown pragma 'NAME'". The pinned FTEQCC
  (`fteqcc -Tid`) gives each of these warnings with this text for `#pragma foo`, `#pragma flag
  enable foo`, `#pragma flag maybe hashonly`, `#pragma flag enable acc`, `#pragma
  optimise foo`, `#pragma opcode foo`, `#pragma opcode on STORE_F foo`, `#pragma TARGET XYZ` and
  `#pragma TARGET fte_99999`. Two warnings depend on what the compiler has made so far, which the
  preprocessor does not know, and are not modelled: `optimise` inside a function ("unable to
  change optimisation options mid-function") and a `TARGET` that switches to or from Hexen II
  after the first statement. The pragmas' effects on the build are the compiler's (FTEF-12.17).
- **FTEF-12.12** Seven of the flags `#pragma flag` sets (FTEF-12.11) change how FTEQCC lexes,
  and hold from the token after the directive: `QCC_PR_Lex` reads each token as the parser asks
  for it, with the flags of that moment (`qc_lexer::LexFlags`, `qc_lexer::lex_flagged`). The
  pinned FTEQCC (`fteqcc -Tid`) gives each result below.
  `qcfuncs` (`flag_qcfuncs`, on by default): with it off, `\0` to `\7` in a string are C's octal
  escape, the digit and up to two more octal digits (`if (flag_qcfuncs) c = 0xe012 + c - '0';
  ... else //C compat`), and `\8` or `\9` is the error "Bad character code \8" (`E0103`).
  `qccx` (`flag_qccx`): a `:` after a string part, comments before it passed over, writes a NUL
  into the string and reading goes on (`if (flag_qccx && *pr_file_p == ':') { pr_file_p++;
  pr_token[len++] = 0; continue; }`), so `"a":"b"` is the string `a`, NUL, `b`; a character
  constant is a float whose bits are its code, with "char constant: denormal" (F313,
  `WARN_DENORMAL`, an error by default, `W0121`); `%` before a digit or `-` is "denormalized
  immediate" (F313) and a float whose bits are the `int` `QCC_PR_LexInteger` reads after it,
  `atoi` of `0x` and the digits, `a` to `f` and `.` that follow, so a hex `%0x5` is 0; and a hex
  number with no suffix goes to `qccxhex`: a float whose bits are the number's low 32 bits, with
  "numerical overflow" (`W0111`) where those bits are not the number and are no negative `int`.
  `utf8` (`flag_utf8strings`): a plain string is read and written as `u8"..."` is
  (`stringtype = flag_utf8strings?2:0;`), so a byte that starts no UTF-8 sequence is "Input
  string is not valid utf-8". `ILP32` (`flag_ILP32`): a number needs `ll` to be 64 bits
  (`islong >= (flag_ILP32?2:1)`), so `0x100000000l` is an `int` with "numerical overflow".
  `ssp` (`flag_dblstarexp`): `**` is the token `*^`, with "** is unsafe around pointers, use *^
  instead." (F321, `WARN_GMQCC_SPECIFIC`, `W0118`). `assumeint` (`flag_assume_integer`): a number
  with no suffix and no `.` is an integer, with no float's "numerical overflow ... will be
  rounded", and a character constant is an integer, `qccx` aside; its value stays a float
  (FTEF-13.5), and "Numerical truncation" when such an integer becomes a float is the
  compiler's. `macroinstrings` (`flag_macroinstrings`): a `#` in a string starts a name that runs
  to white space or one of `)(+-*/\|&=^~[]"{};:,.#`; `QCC_PR_CheckCompConstString` looks it up and
  then looks up each macro's text as a name again for as long as it names a macro, and the text
  it ends on is written into the string as it is, trailing spaces and tabs dropped, with "Macro
  NAME expansion in string" (`WARN_MACROINSTRING`, `W0122`); a name of no macro writes the `#` as
  a byte and is "Unable to expand string macro NAME" unless empty, and a string the text would
  take to 8191 bytes or more is "String length exceeds 8191" (`E0502`). So with `#define FOO
  bar` and two spaces and `#define BAR FOO`, `"a#BAR.c"` is `abar.c`. A deliberate divergence:
  the lookup stops after 64 macros, as FTEQCC's recursion has no bound and never returns for a
  macro whose text names itself. The preprocessor lexes the rest of a text again when the flags
  change, or under `macroinstrings` when a macro is defined or undefined, and marks each change
  by token in `FileOutput::flags`, which `qc_syntax::parse_preprocessed_flagged` reads to
  lex the preprocessed text again as FTEQCC lexed it.
- **FTEF-12.13** Two flags change the preprocessor's reading. `hashonly` (`flag_hashonly`): a name
  is a macro's or a built-in constant's only after a `#` (`if (flag_hashonly ||
  !QCC_PR_CheckCompConst()) QCC_PR_LexName ();`), so with `#define FOO 5`, `FOO` is the name
  `FOO` and `#FOO` is `5`. `nofileline` (`flag_nopragmafileline`): `#pragma file` and `#pragma
  line` change nothing (`if (!flag_nopragmafileline)`).
- **FTEF-12.14** **Withdrawn** (P2-118, #549): it said `caseinsensitive` and `assumedouble`
  are not modelled, but the semantic model reads them (FTEF-19.24, FTEF-19.25), and the command
  line that sets `acc` and `caseinsens` is read (FTEF-14.71). Replaced by FTEF-12.15, itself
  replaced by FTEF-12.16.
- **FTEF-12.15** **Withdrawn** (P2-119, #554): it said the keyword flags are not read, but the
  parser reads each keyword with the flags in force at its token (FTEF-20). Replaced by
  FTEF-12.16.
- **FTEF-12.16** **Withdrawn** (P2-121, #556): it said `cpriority` is not modelled, but the
  parser reads the priorities it sets (FTEF-21.4). Replaced by FTEF-12.17.
- **FTEF-12.17** The other flags `qcc_pr_lex.c` reads are not modelled, as they belong to the
  compiler or its console output rather than to the tokens or the definitions: `acc` (its `|`
  in a string as a line feed, and reacc's type names and keywords, FTEF-19.63, FTEF-20.13); and
  `msvcstyle` and `debugmacros`, which change only what FTEQCC prints. The keyword flags, which
  `#pragma keyword` and `#pragma flag` set with the others, are FTEF-20's, and the flags the
  parser and the semantic model read, `cpriority` among them, FTEF-21's. A pragma emits no
  progs output of its own (as FTE-16.3); its effects on the output (the target format, the CRC,
  the flags) are realised by codegen (L-01).

- **FTEF-12.18** `#pragma autoproto [N]` is taken wherever it stands in the first pass.
  `QCC_PR_Precompiler` reads it as `if (!autoprototyped) { if (numpr_globals != RESERVED_OFS)
  QCC_PR_ParseWarning(WARN_BADPRAGMA, "#pragma autoproto must appear before any definitions");
  else autoprototype = *msg?!!atoi(msg):true; }`, and no definition changes `numpr_globals`: only
  `QCC_FinaliseDef`, which `QCC_UnmarshalLocals` runs when `QCC_FinishCompile` ends the compile,
  and `QCC_FinaliseTemps` add to it (`numpr_globals += def->symbolsize;`), `#merge` itself
  calling it "not normally changed until after compiling". So `void() f = #5;`, `float x;`,
  `typedef float t;` and a function with a body before the pragma leave it as it was, and the
  warning is never given while FTEQCC parses (this profile gives none; `W0108` is only the class
  `#pragma warning` names, FTEF-14.63). `autoprototype` starts false (`parseonly =
  autoprototyped = autoprototype = false;` in `QCC_main`, once per `progs.src`) or as `-Fautoproto`
  sets it (FTEF-14.72), and as a list-form `progs.src`'s pragma sets it: that file is read through
  the preprocessor (`QCC_ContinueCompile` calls `QCC_PR_LexWhitespace(false)` before each
  entry), so its pragma, as Copper's has, holds for the whole module. Each `#pragma autoproto`
  of the first pass then sets it from the token after it, across the module's files and
  `#include`s in compilation order, `autoproto 0` off and any other number or none on. FTEQCC
  makes the second pass if the flag is on when the first ends (`if (autoprototype) { qccmsrc =
  originalqccmsrc; autoprototyped = autoprototype; QCC_SetDefaultProperties(); autoprototype =
  false; ...`), and in the second pass the pragma is skipped without a word (`if
  (!autoprototyped)`). So `float x; #pragma autoproto` is a two-pass compile, `#pragma autoproto`
  and then `#pragma autoproto 0` is a one-pass compile, and `-Fautoproto` with a last `#pragma
  autoproto 0` is a one-pass compile whose functions are all skipped (FTEF-12.19).
- **FTEF-12.19** The first pass reads each declaration with `autoprototype` as it is at its
  `=`. While it is on, `QCC_PR_ParseDefs` makes the definition and skips what initialises it (`if
  (autoprototype || dostrip || ...) { //ignore the code and stuff`): a frame header `[ ... ]`, a
  body `{ ... }` balanced, a string's `_("...")`, or the tokens to the next `,` or `;`, a
  builtin's `#` and number among them, and then `QCC_FreeDef`s it. The definition is made as the
  written `=` decides its constness (`isinitialised = QCC_PR_CheckToken ("=") || ...`, then
  `GDF_CONST` unless `assumevar`), and is not initialised, so `float x = 5; float y = x;` is read
  as it is without the pragma, and the value is not checked: a name used in a skipped body or
  value is not looked up, and the lexical errors of its tokens are still the lexer's. While it is
  off, the declaration is compiled in full, so an error in it, such as `Unknown value "h"` for a
  name defined after it, is the first pass's, and a function body in it is compiled. The second
  pass compiles everything again with every name the first pass made defined: a function with a
  body from the first pass that is given a body again is "redeclaration of function body" and
  "incompatible redeclaration" (a builtin, a prototype or a body the first pass skipped is not),
  and a value is compared with the first pass's as FTE-22.6 says. When no second pass follows, a
  function whose body was skipped is "function NAME has no body" (`ERR_NOFUNC`), which this
  profile reports as QC-11.8's "NAME was not defined" (`E0211`). This profile reads the first
  pass as its own file (its flags, keywords and warning settings, FTEF-12.20), declares what it
  skips uninitialised, and reports once what both passes report, but for the parse diagnostics of
  the skipped initialisers.
- **FTEF-12.20** The second pass starts with what the first pass ended with, but for what
  `QCC_SetDefaultProperties` sets again: the compiler flags, which `QCC_main` set once
  (FTEF-12.11), the keywords (FTEF-20.1), the operator priorities (FTEF-21.4), the pragma defaults
  `noref` and `nosave` (`defaultstatic` is 0, FTEF-21.14) and the definitions the first pass
  made carry; the macros (FTE-22.3), the warning settings (FTEF-14.63) and the target
  (FTEF-20.3) are the command line's again. The preprocessor marks the second pass's first token
  with the flags the first pass ended with, and the second pass is lexed, parsed and its
  definitions made with them: `#pragma autoproto` and `float x = 5; float y = x;` followed by `#pragma
  flag enable assumevar` is the redeclaration of x as non-constant (F329) and not "is not
  constant", `string s = B;` after an enum of `float` names, with `assumeint` on at the end,
  reads `B` as an `__int32` (F314), and `[[eraseable]] float x;` before a last `#pragma flag
  enable attributes` is refused by the first pass, where `attributes` is off.
- **FTEF-12.21** The first pass gives a parameter no default that names a definition:
  `QCC_PR_ParseDefaultInitialiser` reads it, and of a value not initialised `if (autoprototype)
  return nullsref;`, and the pass initialises no definition (FTEF-12.19). The definition keeps
  the first pass's type, which the second pass's, with the default, matches (FTEF-19.7), so a
  call that leaves the parameter out warns "too few parameters" (FTEF-32.3). A literal default
  is initialised and stays. Copper's `T_RadiusDamage` takes `float dmgtype = DMGTYPE_EXPLOSION`.

## FTEF-13 Further tokens

**Source:** `qcc_pr_lex.c`, the `pr_punctuation[]` table, `QCC_PR_LexPunctuation`,
`QCC_PR_LexVector`, `QCC_PR_LexEscapedCodepoint` and `QCC_PR_LexNumber`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset: the punctuation of
`dialect-fteqcc-rerelease.md` FTE-19.1, the literals of FTE-20.

Vanilla `qcc` has no `::` (QC-7.1), reads a `'` only as the start of a three-component vector
(QC-5.1), and knows the escapes `\n` and `\"` only (QC-6.2). The corpus names class members as
`Class::member` (4953 lines), joins long lines with a `\` at their end, writes character
constants and two-component vectors (`'8 8'` in `fteextensions.qc`), and uses `\x` escapes.

### Behaviour

- **FTEF-13.1** `::` is one token. `pr_punctuation[]` lists `"::"` before `":"`, as its rule
  says: "longer symbols must be before a shorter partial match".
- **FTEF-13.2** A `\` directly before a line break joins the lines: the token is skipped with
  the break, in `QCC_PR_LexPunctuation`'s
  `if ((unsigned char)*pr_file_p == (unsigned char)'\\' && pr_file_p[1] == '\r' && pr_file_p[2] == '\n') pr_file_p+=3;`
  and the same for a lone `\r` or `\n` (`pr_file_p+=2`). The corpus continues declarations
  this way (`const string g_spray_mat_1 = \`, `shared/environment/Spraylogo.h`).
- **FTEF-13.3** A `'` opens a character constant when the next byte is `\` (an escape,
  FTEF-13.13), a byte of 0x80 or more (a UTF-8 sequence, FTEF-13.14), or a byte followed by `'`:
  `QCC_PR_LexVector` reads `if (*pr_file_p == '\\') {//extended character constant`, the
  `utf8_check` branch, and `else if (pr_file_p[1] == '\'') {//character constant`. Its value is
  the character's code, a float unless `flag_assume_integer`. Otherwise the `'` opens a vector,
  which may stop after two components, the third 0, with FTEQCC's warning "2d vector"
  (`if (*pr_file_p == '\'' && i == 1)`).
- **FTEF-13.4** **Withdrawn** (P2-72, #475): it said every code above one byte is written to
  a string as UTF-8, which only a UTF-8 string does, and that `\{` stops quietly at a byte that
  is no digit. Replaced by FTEF-13.8 to FTEF-13.11 for strings and FTEF-13.12 for character
  constants.
- **FTEF-13.5** This parse-and-resolve profile keeps every number token of FTEF-3 as a float
  holding its value (an integer rounded to the nearest float), and every character constant as
  a float holding its code: the literal's FTEQCC type (FTEF-3.1, FTEF-3.3) matters only to type
  checking and code generation, which are later (L-01).
- **FTEF-13.6** A vector's components are read by `QCC_PR_LexFloat`, which takes the bytes of
  a number and then skips one lowercase `f` only: `if (*pr_file_p == 'f') pr_file_p++;`
  (`qcc_pr_lex.c`, `QCC_PR_LexFloat`). So `'1f 2f 3f'` is the vector `'1 2 3'`, and an `F`
  after a component is no suffix: it is read as the next component, and the vector is bad
  (QC-5.2).
- **FTEF-13.7** A string literal starts at `"`, and at `R"`, `Q"`, `u"`, `U"` and `u8"`:
  `QCC_PR_Lex` has `if (c == '\"' || ((c == 'R' || c == 'Q' || c == 'u' || c == 'U') &&
  pr_file_p[1] == '\"') || (c == 'u' && pr_file_p[1] == '8' && pr_file_p[2] == '\"'))
  QCC_PR_LexString ();`. `QCC_PR_LexString` reads parts in a loop, skipping white space and
  comments before each (`QCC_PR_LexComment`), and takes those prefixes and `L"` too
  (`(*pr_file_p == 'U' || *pr_file_p == 'u' || *pr_file_p == 'L') && pr_file_p[1] == '\"'`),
  so `L"..."` is a string only after another part: `"ex_" L"a"` is `ex_a`, and a lone
  `L"a"` is the name `L` and a string. The prefix belongs to the part's token, and the parts
  join as FTEF-16.8 says. The vanilla dialect reads each prefix as a name.
- **FTEF-13.8** A part's prefix sets its `stringtype`: a plain string, with no `-Futf8`
  (`flag_utf8strings` is off by default), and `R"..."` are 0, "quake output, input is 8bit";
  `Q"..."` is 1, "quake output, input is utf-8"; `u8"..."`, `u"..."`, `U"..."` and `L"..."` are
  2, "utf-8 output, input is utf-8" (`u`, `U` and `L` with the warning "char32_t/char16_t/wchar_t
  strings are not supported, treating as u8 prefix (as utf-8)"). A byte of 0x80 or more starts
  a UTF-8 sequence only in types 1 and 2, read by `utf8_check`: two bytes for 0 (modified
  UTF-8) or 0x80 to 0x7FF, three for 0x800 to 0xFFFF (surrogates included), four for 0x10000 to
  0x10FFFF; any other byte is itself. Type 2 writes each code as UTF-8 (`forceutf8`), in as
  many bytes as its size needs with no check that it is a Unicode scalar; types 0 and 1 write
  it as one Quake byte (`forcequake`): a code from 0xE000 to 0xE0FF as its low byte, Quake's
  glyph, any other as its low byte too (`pr_token[len] = c;`), above 0x7F with the warning
  "Cannot convert codepoint %#x to quake's charset". So `"\["` is the byte 0x10 and `u8"\["`
  the three bytes of U+E010, `Q"é"` is 0xE9 and `u8"é"` C3 A9, `"\{256}"` is a NUL and
  `u8"\xc0\x80"` written as bytes is one NUL. A code of 0x80000000 or more given to
  `forceutf8` (`\U80000000` or more, in any string, FTEF-13.11) makes FTEQCC loop for ever:
  `t <<= 5` reaches 0 and `while (c >= t)` never ends; this profile writes it in 7 bytes, and
  `cargo xtask oracles builtins` counts the pinned FTEQCC, killed after 20 seconds, as
  differing as this statement says.
- **FTEF-13.9** A raw string, `R"delim(...)delim"`, takes the bytes up to `(` as its delimiter,
  at most 62 (`char rawdelim[64]` from index 1, "EOF while parsing raw string delimiter.
  Expected: R\"delim(string)delim\"" at the end of the file or the 63rd byte, `E0101`), skips a
  `\` and line break right after the `(` ("generally part of the C preprocessor"), and then
  keeps every byte as it is, line breaks, backslashes and `"` included, up to the first `)`
  followed by the delimiter and `"`; the end of the file first is "EOF inside quote".
- **FTEF-13.10** In a string that is not raw, `\s` and `\b` stand for no character and toggle
  bronze text for the characters after them (`texttype ^= 0xe080`). A byte read as itself
  (FTEF-13.8) from a space up gets the high bit (`if (c >= ' ') c |= texttype;`), so a tab
  stays a tab and a byte of 0x80 or more is unchanged; in types 1 and 2 an ASCII character from
  a space up becomes the code `0xE080` with it (`cp|0xe080`), a Quake glyph, and a character
  above ASCII keeps its bytes ("Unable to mask non-ascii chars. Attempting to mask bytes"). So
  `"\ba"` is 0xE1 and `u8"\ba"` the UTF-8 of U+E0E1. `\.` is `0xe01c | texttype`; the codes
  of the other escapes take no bronze.
- **FTEF-13.11** An escape in a string is read by `QCC_PR_LexEscapedCodepoint`: `\n`, `\r`,
  `\t`, `\a`, `\v`, `\f` and `\#`, `\"`, `\\`, `\?`, `\'` (FTE-20.4); `\[` and `\]` 0xE010 and
  0xE011; `\.`, `\<`, `\-` and `\>` 0xE01C to 0xE01F; `\(`, `\=`, `\)` and `\+` 0xE080 to 0xE083
  ("quake specific"); a digit 0xE012 plus the digit under `flag_qcfuncs`, on by default
  ("yellow numbers"); `\{n}` and `\{xN}` the decimal or hex code up to the `}`, with "Bad
  character code" at any other byte, the end of the file included; `\u` and `\U` up to four or
  eight hex digits, then `if (nibbles) QCC_PR_ParseWarning(ERR_BADCHARACTERCODE, "Unicode
  character terminated unexpectedly");`, which the pinned FTEQCC reports as an error; since
  `while (nibbles --> 0)` leaves `nibbles` at `UINT_MAX` when every digit is read, the only
  form with no error stops one digit short (`\u041` and a byte that is no hex digit); `\x`
  one or two hex digits, "Bad character code" with none; a `\` before a line break that break;
  a `\` before CR the byte after the CR; anything else "Unknown escape char". This profile
  reports "Bad character code" as `E0103`, the code of "Unknown escape char" (QCD-2.3). The
  code of `\u` and `\U` is written as UTF-8 in any string ("\u is a utf-8 code regardless of
  output encoding") and that of `\x` as one byte in any ("an explicit byte regardless of
  encoding"); any other as FTEF-13.8 says for the string's type.
- **FTEF-13.12** **Withdrawn** (P2-75, #480): it said this profile takes a character
  constant's escape with no code of its own as its byte, with no error, and reports no "Bad
  character code" there. Replaced by FTEF-13.13.
- **FTEF-13.13** A character constant's escape, `'\c'`, is read as a string's is (FTEF-13.11),
  errors included: `QCC_PR_LexVector` reads `if (*pr_file_p == '\\') {//extended character
  constant` and then `pr_immediate._float = QCC_PR_LexEscapedCodepoint();` (`_int` under
  `flag_assume_integer`), the function a string's escapes go through. The constant's value is the
  code: `'\n'` is 10, `'\['` 0xE010, `'\.'` 0xE01C and `'\x4'` 4. "Unknown escape char" (`'\s'`,
  `'\b'` and `'\q'`: only a string reads `\s` and `\b`, as bronze, FTEF-13.10), "Bad character
  code" (`'\{6z}'`, `'\xg'`) and "Unicode character terminated unexpectedly" (`'\u0041'`;
  `'\u041'` is 65) are errors, `E0103` as in a string. The end of the file right after the `\` is
  "EOF inside quote" (`E0101`), and no other error; after a `\` and CR FTEQCC takes the NUL
  as the byte after the CR (`c = *pr_file_p++;`) and reads on past it, and this profile reports `E0101` there too. Any other byte than
  `'` after the escape is "Bad character constant" (`if (*pr_file_p != '\'') QCC_PR_ParseError
  (ERR_INVALIDVECTORIMMEDIATE, "Bad character constant");`, `E0104`). FTEQCC stops at its first
  error; this profile goes on after the escape, and so may also report "Bad character constant"
  after a bad one.
- **FTEF-13.14** A character constant at a byte of 0x80 or more is read by `utf8_check`
  (FTEF-13.8): `int b = utf8_check(pr_file_p, &pr_immediate._int);` then `pr_file_p+=b+1;`.
  Its value is the sequence's code, a surrogate or the modified UTF-8 NUL included, and the
  byte after the sequence is skipped unread, as the closing `'` it should be, so `'éx` is 233.
  A byte that starts no sequence `utf8_check` takes (`*value = 0xFFFD; return 0;`) is the code
  0xFFFD and is skipped alone, so a `'` after it opens a vector. Recorded with the pinned
  FTEQCC (`fteqcc -Tid` on `void() f = :'...';`, reading `f`'s `first_statement`): `'é'` (C3
  A9) and `'éx` bind 233, `'\xed\xa0\x80'` 55296, `'\xc0\x80'` 0 and `'\xf0\x90\x80\x80'` 65536;
  `'\xff;` binds 65533; `'\xff'`, `'\xe9'` and `'\xc1\x81'` are "Bad vector" at the `'` after
  the byte. The bytes are written here as `\x` escapes.
- **FTEF-13.15** White space between tokens is only what FTEQCC's `qcc_iswhite` takes, `#define
  qcc_iswhite(c) ((c) == ' ' || (c) == '\r' || (c) == '\n' || (c) == '\t' || (c) == '\v')`
  (`cmdlib.h`), where id's lexer takes every byte up to the space as a signed `char` (QC-2.1). So
  under this profile any other control byte, a form feed or 0x01, and DEL, starts no token and is
  "Unknown punctuation: '\x%x'" (`QCC_PR_LexPunctuation`, `QCC_PR_ParseWarning
  (ERR_UNKNOWNPUCTUATION, "Unknown punctuation: '\\x%x'", *pr_file_p);`), `E0105` with FTEQCC's
  text, the byte in lowercase hex: the pinned FTEQCC refuses `float` and a form feed before `x;`
  ("Unknown punctuation: '\xc'"), and accepts two vertical tabs between definitions. Reading goes on
  after the byte, as FTEQCC's `pr_file_p++; QCC_PR_Lex();` does. A byte of 0x80 or more is a
  name's (FTEF-13.16), never white space.
- **FTEF-13.16** A name starts at an ASCII letter, `_` or a byte of 0x80 or more (`QCC_PR_Lex`,
  `if ( (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || (c & 0x80))`) and
  `QCC_PR_LexName` reads it a character at a time, each a UTF-8 sequence `utf8_check` reads
  (FTEF-13.8, ASCII bytes one each), while the byte after is a letter, a digit, `_` or of 0x80 or
  more. So `float é;` and `float xéy;` define the names, written as UTF-8. A byte that starts no
  sequence (`if (!b)`) is the error "Invalid UTF-8 code sequence before WORD. Lead byte was 0xNN"
  (`ERR_NOTANAME`, `E0123`): NN the byte in lowercase hex, WORD the bytes up to white space from
  the first byte after it that starts a sequence, or, when only white space but a line feed stands
  between that byte and the line's end, "Invalid UTF-8 code sequence at end of line. Lead byte was
  0xNN". The pinned FTEQCC refuses `float x` 0xC3 `;` ("... before ;. Lead byte was 0xc3") and
  `float` 0xA0 `x;`, the non-breaking space of Latin-1. FTEQCC stops at the error; this profile
  makes no token of the name and reads on at that byte, where FTEQCC leaves `pr_file_p`. The
  vanilla dialect keeps QC-1.3: such bytes are white space.
- **FTEF-13.17** After a number `QCC_PR_LexNumber` reports "bad suffix on number TEXT" when a
  letter, a digit, `_` or a byte of 0x80 or more follows it (`checkjunk:`, `if ( (c >= 'a' && c
  <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || (c >= '0' && c <= '9') || (c & 0x80))
  QCC_PR_ParseWarning(ERR_NOTANUMBER, "bad suffix on number %s", pr_token);`), an error as its code
  is an `ERR_` one, `E0124`. TEXT is `pr_token`: the number's bytes, a `-` first included, `0X`
  written `0x`, and a type suffix included except an `f` or `d` after a fraction, which is passed
  over unwritten: `5abc` is "bad suffix on number 5", `5fx` "... 5f", `1.5e3` "... 1.5" and `08`
  "... 0" (after FTEF-13.19's warning). The letters after it are then read as a name of their own,
  as FTEQCC reads on. A vector's components, which `QCC_PR_LexFloat` reads, and the numbers of
  directives (`QCC_PR_LexInteger`) are not checked.
- **FTEF-13.18** White space and comments before a token are read as `QCC_PR_Lex` reads them:
  `QCC_PR_LexComment` first, which reads white space up to and including the first line ending
  (`qcc_islineending`: a LF, or a CR not before a LF), or a `//` comment up to and including its
  LF (`while (*pr_file_p && *pr_file_p != '\n')`), after which `QCC_PR_NewLine` passes over the
  next line's spaces and tabs and only a `//` after other white space there, a VT, is read in the
  same way, or the first `/* */` comment; then `QCC_PR_LexWhitespace`, whose `//` comment ends at
  any line ending (`while (*pr_file_p && !qcc_islineending(pr_file_p[0], pr_file_p[1]))`). So a
  lone CR ends a `//` comment only after a line ending since the token before it: the pinned
  FTEQCC refuses `// c`, a CR and `float y = ;` on the line after a definition, and `float x; //
  c`, a LF, `// e`, a CR and `float y = ;`, and accepts `float x; // c`, a CR and `float y = ;`,
  which is all comment, and the same with a VT before `// e`. A `/* */` comment's `*/` is looked for from the byte after
  the `*` (`pr_file_p+=1; do { pr_file_p++; ... } while (pr_file_p[0] != '*' || pr_file_p[1] !=
  '/');`), so `/*/` opens a comment that runs on, where id's closes it (QC-2.3): FTEQCC accepts
  `/*/ float x; */`. A `/*` inside one is the warning "\"/*\" inside comment" (`WARN_NESTEDCOMMENT`,
  F325, `W0112`, at the `/*`), and a comment with no `*/` is "EOF inside comment" (`E0501` with
  FTEQCC's text). A `\` before a line break, `\` CR LF taken whole, joins the lines and the reading
  starts again after it (FTEF-13.2).
- **FTEF-13.19** `QCC_PR_LexNumber` accumulates the digits in a `long long num` (`num*=base; num
  += c-'0';`), which wraps on the two's complement builds FTEQCC runs on, and writes `(float)(num*
  sign)`: the pinned FTEQCC binds `#18446744073709551621` to builtin 5. It warns "A leading 0 is
  interpreted as base-8." (`WARN_OCTAL_IMMEDIATE`, named `octal`, `W0110`, at the `0`) for a `0`
  and then a digit, and "numerical overflow" (`WARN_OVERFLOW`, F312, `W0111`, at the number) where
  the value it stores is not the number: after an `f` or `d` suffix when `(longlong)` of the float
  or double differs from `num`; after an `i` or `u` suffix with no `l`, when the low 32 bits it
  keeps (`pr_immediate._int = num`) differ from `num` and are not negative, so `4294967313i` warns
  and `0xffffffffi` does not; and with no suffix in base 16 only, "numerical overflow %lld will be
  rounded to %f", `0x100000001` giving "numerical overflow 4294967297 will be rounded to
  4294967296.000000". A float past `long long`'s range converts with C's undefined behaviour; this
  profile saturates, as arm64 does.
- **FTEF-13.20** FTEQCC's token buffer is `char pr_token[8192];` (`FTEQCC_MAX_TOKEN`): a string,
  its parts joined (FTEF-16.8), of more than 8191 bytes is "String length exceeds 8191"
  (`QCC_Error(ERR_INVALIDSTRINGIMMEDIATE, "String length exceeds %u", ...)`, which ends the
  compile; `E0502` with FTEQCC's text), and a name or number that long runs past the buffer, which
  FTEQCC does not check, and is `E0502` "token too long" here, a deliberate divergence from that
  undefined behaviour. A name of 256 bytes or more is the parser's "name too long" (FTEF-15.7).
  So a string of 3000 bytes, which id's `qcc` cannot hold (QC-27.4), is accepted.
- **FTEF-13.21** In a string whose input is UTF-8 (`Q"`, `u8"`, `u"`, `U"`, `L"` and, with
  `-Futf8`, `"`, FTEF-13.8) a byte that starts no sequence `utf8_check` reads is the error "Input
  string is not valid utf-8", and a byte of 0x80 or more starting a sequence inside bronze text
  (FTEF-13.10) "Unable to mask non-ascii chars. Attempting to mask bytes", each
  `QCC_PR_ParseWarning(ERR_BADCHARACTERCODE, ...)` and so an error, `E0103` with FTEQCC's text;
  FTEQCC reads on, writing the byte as FTEF-13.8 says. An unknown escape is "Unknown escape char
  %c" with the byte after the `\`.
- **FTEF-13.22** FTEQCC's F301 (`WARN_NOTUTF8`) and F311 (`WARN_FTE_SPECIFIC`), off by default
  (FTEF-14.63), are reported once a `#pragma warning` turns them on: F301 for a `u"`, `U"` or `L"`
  string, "char32_t/char16_t/wchar_t strings are not supported, treating as u8 prefix (as utf-8)",
  for a code a Quake string cannot hold, "Cannot convert codepoint %#x to quake's charset", and
  for a UTF-8 one past U+10FFFF, "Bad unicode character code - codepoint is above 0x10FFFFu"
  (`W0119`); F311 for a vector of two components, "2d vector" (`W0120`).
- **FTEF-13.23** A vector's components are read with `QCC_PR_LexWhitespace` before the first and
  after each (`QCC_PR_LexVector`), not with `QCC_PR_LexComment`, so a comment before the first
  component is passed over and a `//` comment between components ends at a lone CR: the pinned
  FTEQCC accepts `'/*c*/1 2 3'` and `'1 // c`, a CR and `2 3'`.
- **FTEF-13.24** Under FTEQCC the punctuation is `pr_punctuation[]`, tried in its order, the first
  that matches the input winning ("longer symbols must be before a shorter partial match"),
  which has, besides the vanilla tokens and those of FTE-19.1, `<=>`, `*^`, `><`, `@` and `^^`
  (`qc_lexer::Punct::FTEQCC_TABLE`); a byte it does not list is FTEF-13.15's "Unknown
  punctuation". So `a<=>b` is `a`, `<=>`, `b` and `a><b` is `a`, `><`, `b`.
  `QCC_PrioritiseOpcodes` gives `*^` (`POW_F`) and `><` (`CROSS_V`) the priority of `*` (`PC_MULDIV`) and `<=>` (`SPACESHIP_F`) that
  of `==` (`PC_EQUALITY`), as this parser does. `@` (a `qccx` unary in `QCC_PR_Term`) and `^^`
  (which no opcode has) are tokens the parser of this profile reads as no operator.
- **FTEF-13.25** In a string, `\` before a CR is the byte after the CR (`c = *pr_file_p++;` in
  `QCC_PR_LexEscapedCodepoint`), and when that byte is no LF FTEQCC warns `"Hanging \\\\\r"`
  (`WARN_HANGINGSLASHR`, `W0123`), the text `Hanging \\` and a CR: the pinned FTEQCC gives it for
  `"a`, `\`, a CR and `b"`. A `\` before a CR and an LF is the LF, with no warning.

## FTEF-14 Preprocessor directives and macros

**Source:** `qcc_pr_lex.c`, `QCC_PR_Precompiler`, `QCC_PR_FalsePreProcessorIf`,
`ParsePrecompilerIf`, `QCC_PR_SkipToEndOfLine`, `QCC_PR_PreProcessor_Define`,
`QCC_PR_DefineName`, `QCC_PR_UndefineName`, `QCC_PR_SimpleGetToken`, `QCC_PR_SimpleGetString`,
`QCC_PR_LexInteger`, `QCC_PR_NewLine`, `QCC_PR_LexWhitespace`, `QCC_FindBestInclude`,
`QCC_JoinPaths`, `QCC_PR_AddIncludePath`, `QCC_PR_ExpandPreProcessorMacro`,
`QCC_PR_ExpandStrCatMarkup`, `QCC_PR_IncludeChunkEx`, `QCC_PR_CheckCompConst` and
`QCC_PR_CheckBuiltinCompConst`; `qcc_cmdlib.c`, `QCC_COM_Parse`, `QCC_COM_Parse2`,
`QCC_LoadFile`, `QCC_SanitizeCharSet` and `decodeUTF`;
`qccmain.c`, `QCC_SetDefaultProperties`; `qcc_pr_comp.c`, `QCC_Include` and
`QCC_PR_CompileFile`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Vanilla preprocessor:
[`language.md`](language.md) QC-9; the project graph's reading of the same directives:
[`project-file.md`](project-file.md) PRJ-5.

This project's vanilla preprocessor (QC-9) knows `#include`, `#define`, `#undef`, `#ifdef`,
`#ifndef`, `#else` and `#endif`, and QC-9.9 makes any other directive an error. The corpus also
writes `#if` (256 lines), `#elif`, `#warning` (16), `#includelist` inside the files a list
names, macros continued over several lines, variadic macros (`#define ncLog(...)`), `#` and
`##` in macro bodies, and FTEQCC's built-in `__NULL__` and `__FUNC__`. Under the FTEQCC
profile the preprocessor reads them as FTEQCC does.

### Behaviour

- **FTEF-14.1** `#if expr`, `#elif expr`, `#elifdef NAME` and `#elifndef NAME` choose a side as
  the project graph does (PRJ-5.5): `QCC_PR_Precompiler` evaluates `#if` with
  `eval = ParsePrecompilerIf(PPI_TOPLEVEL);`, and `QCC_PR_FalsePreProcessorIf` takes the first
  `#elif`, `#elifdef` or `#elifndef` that is true, or the `#else`, only while no earlier side was
  (`if (!hadtrue && !strncmp(pr_file_p, "elif", 4) && level == 1)`). An `#if` inside a skipped
  side only nests (`if (!strncmp(pr_file_p, "if", 2)) level++;`). The expression is evaluated
  by the one evaluator the graph uses, so the graph and the preprocessor take the same sides.
- **FTEF-14.2** **Withdrawn** (P2-81, #492): it said a `//` comment ends a directive, which
  holds for the directive's text but not for the line `QCC_PR_SkipToEndOfLine` skips after it.
  Replaced by FTEF-14.30 and FTEF-14.24.
- **FTEF-14.3** **Withdrawn** (P2-94 to P2-99, #507 to #512): it said a `#warning` or `#error`
  text is the rest of the line, where `#error`'s stops at a tab too and both start right after
  the directive's name. Replaced by FTEF-14.38.
- **FTEF-14.4** **Withdrawn** (P2-94 to P2-99, #507 to #512): it said a `#endlist` with no list
  open is accepted, where FTEQCC reads it as no directive. Replaced by FTEF-14.41.
- **FTEF-14.5** A macro parameter list whose last parameter is `...` makes the macro variadic:
  `if (nl == 3 && s[0] == '.' && s[1] == '.' && s[2] == '.') { cnst->varg = true;`. The arguments
  after the named ones, with the commas between them, replace `__VA_ARGS__` in the body
  (`if (*pr_file_p == ',' && c->varg && param >= c->numparams) extraparam++;`); a use may give
  no variadic argument. A use with fewer arguments than named parameters is "macro NAME
  expects N arguments, found M" (QCD-8.9), as FTEQCC's "Not enough macro parameters" is.
- **FTEF-14.6** **Withdrawn** (P2-78, #484): it said every macro body reads `#param` as a string
  and `a ## b` as one token, which only a function-like macro's does, and that a paste is always
  one token. Replaced by FTEF-14.14 to FTEF-14.16.
- **FTEF-14.7** When no macro of the name is defined, `QCC_PR_CheckBuiltinCompConst` expands
  `__NULL__` to `0i`, `__FILE__` to the file's name as a string, `__LINE__` to the line number
  and `__LINESTR__` to it as a string, `__FUNC__` and `__func__` to the function's name as a
  string, `__TIME__` and `__DATE__` to strings, `__RAND__` to a number and `__QCCVER__` to a
  string. The preprocessor does not know the function a use is in, so this profile expands
  `__FUNC__` to an empty string; the value is a compile-time string and does not change
  resolution.
- **FTEF-14.8** **Withdrawn** (P2-94 to P2-99, #507 to #512): it said the build directives
  produce no token and read only their line, where `#package` ends the text and `#pack` and
  `#forcecrc` may read into the next line. Replaced by FTEF-14.39.

- **FTEF-14.9** A use of a function-like macro may give one argument more than its
  parameters, which is unused: `QCC_PR_ExpandPreProcessorMacro` checks the count only at a
  comma, after counting the argument before it, with
  `if (param == MAXCONSTANTPARAMS || param > c->numparams) QCC_PR_ParseError(ERR_TOOMANYPARAMS, "Too many parameters in macro call");`,
  so the last argument is never counted against the parameters. Two or more extra arguments are
  "macro NAME expects N arguments, found M" (QCD-8.9). The corpus relies on it
  (`EVALUATE_FIELD(mins, 0, PLAYER_SIZE)` for a two-parameter macro, `shared/game/Player.qc`).
- **FTEF-14.10** **Withdrawn** (P2-94 to P2-99, #507 to #512): it said a taken side left open is
  closed at the end of its file, where FTEQCC's count of open sides runs on into the next file.
  Replaced by FTEF-14.37.
- **FTEF-14.11** **Withdrawn** (P2-94 to P2-99, #507 to #512): it said a line continuation in an
  `#if` expression reads as a space and that an expression FTEQCC reports as an error is only
  false, where FTEQCC stops at the `\` and the preprocessor reports the error. Replaced by
  FTEF-14.33.
- **FTEF-14.12** A deliberate divergence: this profile's `#if` reader nests at most 64 levels
  (`MAX_DEPTH` in `qc-preproc`'s `cond.rs`), each parenthesis, each `!` and each right side of
  an operator one level deeper (`ParsePrecompilerIf` calls itself for each), and an expression
  nested deeper is false, as one FTEQCC reports as an error is. FTEQCC's
  `ParsePrecompilerIf` recurses with no limit; this profile bounds it so that no input can
  exhaust the stack, as PRJ-5.10 bounds the vanilla reader and QC-27.15 the parser.
- **FTEF-14.13** A name is a macro use only when the word `QCC_PR_CheckCompConst` reads at it ends
  with the name: `QCC_PR_Lex` calls it for every name (`if (flag_hashonly ||
  !QCC_PR_CheckCompConst()) QCC_PR_LexName ();`), and it takes the word up to the first NUL,
  `qcc_iswhite` byte (space, CR, LF, tab, VT) or one of `)(+-*/|&=^~[]"{};:<>,.#` (`for (end =
  pr_file_p; ; end++)`) and looks that word up (`pHash_Get(&compconstantstable, pr_token)`, then
  `QCC_PR_CheckBuiltinCompConst`). So a macro's name, or a built-in constant's (FTEF-14.7),
  followed directly by any other byte, such as `!`, `?`, `%`, `@`, `$`, `'`, `` ` ``, `\`, a
  control byte, DEL or a byte of 0x80 or more, is a name: with `#define N 5`, `x = N!=5;` is
  "Unknown value \"N\"" (`E0301`) and `x = N==5;` is `x = 5==5;`. The byte is the one FTEQCC reads
  after the name: in a macro's body the body's next byte as stored (QC-9.16), and after an
  argument the body's byte after the parameter, so `#define A(a) a!=5` used as `A(N)` leaves `N` a
  name; at the end of an expansion the word ends. FTE-23.10 reads the word after a `#` in the same
  way, in an expansion as in the source: `#define K #/**/N` then `K` is `5`, as FTEQCC stored the
  body `#N` (the pinned FTEQCC: "Cannot cast from float to void()" for `void() f = K;`,
  FTE-23.11). The vanilla preprocessor expands every name that is a macro (QC-9.4).
- **FTEF-14.14** In a function-like macro's body `a ## b` writes `b`'s text right after `a`'s,
  with the white space before the `##` dropped (`*bufferlen = whitestart;`), and the result is
  lexed again with the rest of the expansion: a text that lexes as one token is that token (`x
  ##_net` with `x` given `health` is the name `health_net`, `#1##6` the builtin 16), and one that
  lexes as more is the tokens of that text, each against the one before (`J(a, +)` with `#define
  J(a, b) a ## b` is `a+`, the name and `+`). Either side may be empty, so `##` at the start or
  end of the body, or beside an empty argument, leaves the other side as it is, and an empty
  argument after `##` joins nothing to what follows it: `#define P(a, b) a##b` used as `P(#5, )`
  or `P(, #5)` is `#5`. `##` is two `#` that touch as the body was stored (`if (pr_file_p[1] ==
  '#')`); `# #` is a `#` before a word (FTEF-14.31). An argument beside `##` is copied whole: one
  after it by `noargexpand`, one before it because `start[0] == '#' && start[1] == '#'`, each with
  what stands before its `,` or `)`. So the right argument's later tokens touch the joined one
  (`P(l, bl:)` writes the label `lbl:`), and white space or a comment after the left argument
  stays between the sides, as `*bufferlen = whitestart` drops only the body's white space: `P(x ,
  y)` is `x y`. A paste whose text opens a comment (`/` and `/`, or `/` and `*`) writes nothing
  more: the comment runs to the end of the chunk.
- **FTEF-14.15** **Withdrawn** (P2-80, #491): it said an argument that comes from another
  expansion's text is written into a `#` string as its tokens touch, one space where they do
  not, where FTEQCC keeps that text's white space; this profile now keeps it too. Replaced by
  FTEF-14.31.
- **FTEF-14.16** An object-like macro's body is included as it was stored
  (`QCC_PR_IncludeChunkEx(c->value, ...)`), and `QCC_PR_ExpandPreProcessorMacro`, which reads `#`
  and `##`, runs only for a macro with a parameter list (`if (c->numparams>=0)`). So in an
  object-like macro `#` and `##` are tokens like any other: `#define P #5:ex_##a` is refused, the
  pinned FTEQCC reporting it at the `##`.
- **FTEF-14.17** A macro is not expanded in the chunk of its own expansion (`if (c &&
  (!currentchunk || currentchunk->cnst != c))`, "macros don't expand themselves"), and otherwise
  is, while fewer than 8 chunks of it are open for an object-like macro (`if (c->inside >= 8)`) or
  at most 8 for a function-like one (`if (c->inside>8) return false;`). The open chunks are the
  expansion a token comes from and those that expansion was used in, back to the source: a chunk
  stays open until its last token is read. So a macro used again through another is expanded
  again: with `#define A #5 B` and `#define B :A`, `A` is `#5 :#5 :...` eight times over, which
  the pinned FTEQCC refuses, where QC-9.4's rule leaves `#5 :A`. A built-in constant's value
  (FTEF-14.7) is a chunk of its own (`QCC_PR_IncludeChunkEx(tok, true, NULL, NULL)`), so it
  touches neither token beside it. This profile reads FTEQCC macros by this rule and keeps no hide
  set for them; the vanilla dialect keeps QC-9.4's.
- **FTEF-14.18** **Withdrawn** (P2-81, #492): it said a comment before a function-like macro's
  `(` leaves the name as it is, which holds for the source and for an argument's text but not
  for a macro's stored body, where `QCC_PR_PreProcessor_Define` dropped the comment. Replaced by
  FTEF-14.29.
- **FTEF-14.19** **Withdrawn** (P2-83, #494): it said this profile's expansions write at most
  2^22 tokens, which it checked before each expansion and counted after it, so one expansion
  could write far more. Replaced by FTEF-14.28.
- **FTEF-14.20** In an argument that is not copied whole (FTEF-14.14),
  `QCC_PR_ExpandPreProcessorMacro` reads each token with `QCC_COM_Parse2` and writes a built-in
  constant's value in its place (`end = QCC_PR_CheckBuiltinCompConst(qcc_token, retbuf,
  sizeof(retbuf));`, "we should be testing all defines instead of just built-in ones"), in the
  macro's own chunk: so with `#define K(x) x`, `K(#__LINE__)` is `#` and the line number touching,
  a builtin, where the line number of a built-in constant read anywhere else is a chunk of its own
  (FTEF-14.17). Other macros in an argument are expanded only when the expansion is read.
- **FTEF-14.21** **Withdrawn** (P2-87, #504): it said this profile keeps a file's name whole in
  `__FILE__`, escaping it, a deliberate divergence while the name was the host's path. The name
  is now FTEQCC's, written as FTEQCC writes it. Replaced by FTEF-14.32.
- **FTEF-14.22** A function-like macro's arguments are found in the text after its `(`, which
  `QCC_PR_ExpandPreProcessorMacro` reads a byte at a time: after the white space and comments
  that follow the `(` (`QCC_PR_LexWhitespace(false)`), a `"` string is passed over (`do {
  pr_file_p++; } while( (pr_file_p[-1] == '\\' || pr_file_p[0] != '\"') && *pr_file_p &&
  *pr_file_p != '\n' );`), `(` and `)` nest (`plevel`), and a `,` or `)` outside them ends an
  argument (`paramoffset[param++] = start;`), the white space and comments after a `,` being
  skipped before the next. Every other byte, a character constant's or a comment's, is part of
  the argument, so a `,`, `(` or `)` in one ends an argument or nests as any other does: with
  `#define E(n) :n`, `E(',')` gives `E` the arguments `'` and `'`, and FTEQCC refuses `void() f
  = E(',');` ("Bad vector"); with `#define M(x) x`, `M(#5 /* ) */)` ends at the `)` in the
  comment, so its argument `#5 /* ` opens a comment the expansion ends inside, and `M(#5 // )`, a
  line break and `)` ends at the `)` in the `//` comment; a comment right after the `(` or a `,` is
  skipped first, so `M(/* ) */ #5)` is `#5`. With `#define S(a) #a`, `:S(e'x)` binds the name
  `e'x`. The string skip ends only at a `"` with no `\` right before it, a `\\` before it
  included, so a string whose last byte is a `\` runs on past its `"` to the next `"` or the line's
  end, and the argument with it: with `#define E(n) n`, `E(:"ex\\")` runs to the end of the file,
  which FTEQCC reports as "EOF on macro call" (FTEF-14.29), and `E(:"ex\\") ")"` takes `:"ex\\")
  "` as its argument. A line break, or a `\` before one, is part of an argument like any byte.
  Reading of the text the use stands in goes on after its `)`, read again from there where a
  token of the text as first read stood across it. The arguments' text is read as code only where
  the expansion writes it, so its lexical errors are reported there and nowhere else.
- **FTEF-14.23** `QCC_PR_PreProcessor_Define` reads a macro's name with `QCC_PR_SimpleGetToken`,
  up to white space, `,`, `;`, `(`, `)`, `]` or `//`, and a parameter list from a `(` right after
  it as bytes: each parameter runs from after the spaces and tabs that follow the `(` or a `,` to
  the next `,` or `)`, its trailing spaces and tabs dropped, and the first byte after a `,` and
  its spaces is passed over before a delimiter is looked for (`if(!*pr_file_p++)`), so `(a,)`
  reads on past its `)`. A parameter given twice is refused ("duplicate macro paramter name
  '%s'"), as are a 33rd parameter (`MAXCONSTANTPARAMS` = 32, "May not have more than %i
  parameters to a macro"), a name of 32 bytes or more (`MAXCONSTANTPARAMLENGTH` = 32, "parameter
  name is too long"), a `...` that is not last ("varadic argument must be last") and a list the
  text ends in ("missing ) in macro parameter list"): this profile reports each as "bad parameter
  list for macro NAME" (`E0115`) and defines nothing. So `#define E(n, n) n` and `#define E(n,n)
  n` are refused, and `#define E( n ) n` names `n`. `()` is one parameter with no name, so `#define
  E() #5` takes one argument, and a second (FTEF-14.9). A parameter is its bytes: one that is no
  word, such as `a b`, is never matched in the body.
- **FTEF-14.24** `QCC_PR_PreProcessor_Define` stores a body from after the spaces and tabs that
  follow the name or the `)` to a line break, the end of the file, or a `//` outside a quote; it
  drops each `/* */` comment outside a quote ("multi-line c style comments become part of the
  define itself. this also negates the need for \ at the end of lines. although we don't bother
  embedding.") and each `\` before a line break (LF, CR or CR LF) with the line break ("read over
  a newline if necessary"), storing the byte after it whatever it is, and drops the spaces and
  tabs at the end. Its quote flag flips at every `"`, an escaped one included (`if (*s == '\"')
  quote=!quote;`), so `#define N :"ex\"_a//"` stores `:"ex\"_a`, whose string the expansion
  leaves open, and FTEQCC refuses `void() f = N;`. After a `\` and line break, a line whose first
  byte after spaces and tabs is `#` keeps its line break, and so does the next continued line
  ("#define hacks however, require that it becomes foo\nbar\nmoo"). `QCC_PR_SkipToEndOfLine`
  then reads to the end of the directive's line, passing over `/* */` comments, strings before
  any `//`, and each `\` and line break, in a `//` comment too, so after `#define H 1 // c \` the
  next line is part of the directive and never read. The directive's text is read as code only
  where an expansion writes it. A function-like macro's name and a `(` that only dropped comments
  and continuations stand between in a body are apart only by the spaces and tabs around them, so
  `#define K M /**/ (#5)`, `#define K M/**/ (#5)` and `#define K M \`, a line break and `(#5)`
  call `M` (FTEF-14.29). `#append NAME ...` (FTEF-14.39) is read the same way and adds a space and
  its body to the macro's stored body when the parameters are the macro's; with others
  ("different number of macro arguments in macro append", "arguments differ in macro append") it
  defines the macro again, with the warning `W0102`.
- **FTEF-14.25** In a function-like macro's body, and in an argument copied a token at a time
  (FTEF-14.20), `QCC_COM_Parse2` reads each token after white space and `//` comments: a number
  (digits and `.`, or `0x` and hex digits and `.`), a word (a letter or `_`, then letters, digits
  and `_`), or one byte, each kept to its first 1023 bytes (`char qcc_token[1024]`, `if (len >=
  sizeof(qcc_token)-1) ;`). A parameter is a word that is its name (`!STRCMP(qcc_token,
  c->params[p])`), so in `#define E(a) #5a` and `#define E(a) #5.a` the `a` is the parameter, and
  in `#define E(a) #0x5a` it is a hex digit (the builtin 90). A string right after `#` or `##` is
  read by `QCC_COM_Parse2` too, which writes its text with the escapes `\"`, `\\` and `\n` read
  and no quotes, and goes on one byte past the closing `"`: so `#define S(a) #"5"` writes `#5`,
  `#"5"x a` writes `#5` and the argument, the `x` passed over, and `a##"5"` given `#` writes
  `#5`.
- **FTEF-14.26** In a variadic macro's body `__VA_COUNT__` is FTEQCC's count of the variadic
  arguments, `QC_snprintfz(tmp, sizeof(tmp), "%i", param-1+extraparam);`: the arguments found
  less one, and the commas after the named ones. So with `#define V(...) :__VA_COUNT__`, `V()`
  and `V(a)` are `:0` and `V(a, b)` is `:1`, and with `#define V(a, ...) #5:__VA_COUNT__`, `V(a,
  b)` is `#5:1`, which FTEQCC refuses ("not a name"); its "__VA_COUNT__ without any variable
  args" is never reached, since fewer arguments than named parameters are "Not enough macro
  parameters" first (FTEF-14.5). `__VA_ARGS__` is the variadic argument when one was given (`if
  (param-1 == c->numparams)`); when none was, after `##` it takes away a `,` written right before
  it (`if(*bufferlen>0 && (*buffer)[*bufferlen-1] == ',') *bufferlen-=1;`), the white space the
  `##` drops gone first, so `#define V(a, ...) a,##__VA_ARGS__` and `a , ## __VA_ARGS__` used as
  `V(#5)` are `#5`, while `a,__VA_ARGS__` keeps the `,`. An empty variadic argument is its text,
  commas and all: `#define E(...) #5 __VA_ARGS__` used as `E(, )` writes `#5 , `, the
  declaration `#5` and a `,` before the `;` (FTEF-15.8).
- **FTEF-14.27** A built-in constant an argument copies as its value (FTEF-14.20) is written in
  place of the white space and `//` comments `QCC_COM_Parse2` passed over before it too (`end` is
  written, not `argstart` to `argend`), so with `#define E(n) n`, `E(# __LINE__)` is `#2`, the
  builtin 2, as `E(#//`, a line break and `__LINE__)` is the builtin 3. The line a built-in
  constant has is `pr_source_line`, or in a macro's chunk `currentchunk->currentlinenumber`, the
  line FTEQCC was reading when `QCC_PR_IncludeChunkEx` included the chunk (`chunk->currentlinenumber
  = pr_source_line;`). A use in a file includes its expansion at the line of its `)`, or of its
  name for an object-like macro, which is also the line of a constant its arguments copy; reading
  a chunk counts its lines from 1 (`pr_source_line = 1;`), so a use in a chunk includes its
  expansion at 1 and a line for each line break of the chunk before the use's end, and a constant
  its arguments copy has the line of the chunk the use is in. So `#define L :__LINESTR__` and
  `#define M L` used as `M` on line 3 bind the name `1`, and `#define M(x) :__LINESTR__ x` used
  as `M(`, a line break and `)` from line 2 binds `3`.
- **FTEF-14.28** A deliberate divergence: FTEF-14.17 lets an expansion grow exponentially
  (`#define A B B` and `#define B A A` expand to 2^16 names), and one expansion of a body that
  names its parameter `n` times, given an argument of `m` tokens, writes `n` times `m` (a body of
  8000 `x` given 8000 tokens is 64000000), and FTEQCC builds it all. So this profile makes at most
  `FTE_MAX_EXPANSIONS` = 2^20 macro expansions in one file and writes at most
  `FTE_MAX_EXPANDED_TOKENS` = 2^22 tokens with them: an expansion's text is written only while it
  holds no more bytes than the tokens left under the bound, a token being at least one byte, and
  its tokens are counted when it is lexed. The first use past either bound is reported once, as
  "macro expansion too large" (`E0120`), and it and later uses are left as written. No program of
  the corpus comes near it. The vanilla dialect has the same bound (`language.md` QC-9.18).
- **FTEF-14.29** A function-like macro takes its arguments only from the text of the chunk its
  name is in: `QCC_PR_CheckCompConst` skips spaces and tabs after the name (`while(*pr_file_p == '
  ' || *pr_file_p == '\t')`) and expands it only `if (*pr_file_p == '(')`, so a line break or a
  comment before the `(`, in the source or in an argument's text copied into an expansion, or the
  end of the expansion the name is in, leaves the name as it is (`M\n(#5)` is the name `M`; with
  `#define N M`, `N(#5)` is too), while in a macro's stored body the comments and continuations
  between a name and its `(` are gone (FTEF-14.24); and `QCC_PR_ExpandPreProcessorMacro` reads the
  arguments up to the `)` within that text (FTEF-14.22), reporting "EOF on macro call" (`if
  (!*pr_file_p) QCC_PR_ParseError(ERR_EOF, "EOF on macro call");`) when it ends first. This
  profile reports that as "unterminated argument list for macro NAME" (`E0117`): with `#define
  L(x) x:` and `#define LL L(lbl`, `LL) goto lbl;` is `E0117`, as is the end of the file inside a
  use's arguments.
- **FTEF-14.30** **Withdrawn** (P2-94, #507): it said only `#define` and `#append` read on past a
  `\` and line break in a `//` comment, a known difference for the other directives. Every
  directive now ends its line where `QCC_PR_SkipToEndOfLine` ends it. Replaced by FTEF-14.35.
- **FTEF-14.31** In a function-like macro's body `#param`, `param` a named parameter, is a string
  of the argument's text as written, from after the white space and comments that follow its `(`
  or `,` (`QCC_PR_LexWhitespace(false)`) to its `,` or `)`: `QCC_PR_ExpandStrCatMarkup` copies
  `paramoffset[p]`, white space and comments included, with `\` before each line break, `\`, NUL
  and `"`. So `#define S(a) #a` used as `S( ex_a )` is `"ex_a "`, `S(ex a)` is `"ex a"`,
  `S(ex/**/a)` is `"ex/**/a"`, `S(ex\a)` is `"ex\\a"` and `S()` is `""`; every byte is kept, one
  of 0x80 or more included; the argument's text is a string's, so no lexical error in it is
  reported unless the body also writes the argument as code. `...` has no name (the loop compares
  `c->params[p]`), so `#__VA_ARGS__` is a `#` before the word `__VA_ARGS__`. A `#` before a word
  that is no parameter writes `#` and that word with nothing between (`QCC_PR_ExpandStrCat(...,
  "#", 1);` then `qcc_token`), so `#define S(a) # 5` is `#5`, a builtin; a `#` at the end of the
  body writes nothing (`if (!pr_file_p) break;`). An argument that comes from another expansion's
  text is that text as the expansion wrote it, white space and all.
- **FTEF-14.32** **Withdrawn** (P2-95, #508): it said the semantic model gives the
  preprocessor each file's path as its input names it, the `progs.src` entry only where the
  caller makes it so, but the semantic model now gives an FTEQCC graph's files by their names
  (FTEF-14.75). Replaced by FTEF-14.74.
- **FTEF-14.74** A built-in constant (FTEF-14.7) is the text `QCC_PR_CheckBuiltinCompConst`
  writes for it, read as a chunk of its own (FTEF-14.17): `0i` for `__NULL__`, the line (`%i`)
  for `__LINE__` and in quotes for `__LINESTR__` (FTEF-14.27), `""` for the constants this
  profile does not know, and for `__FILE__` `QC_snprintfz(retbuf, retbufsize, "\"%s\"",
  s_filen);` in `char retbuf[256]`, so at most 255 bytes, ended at its first `:` (`erk =
  strchr(retbuf, ':'); if (erk) { erk[0] = '\"'; erk[1] = 0; }`) with nothing in the name
  escaped: a `\` in the name starts an escape, a `"` ends the string, a line break is "newline
  inside quote" (`E0102`) and a name of 254 bytes or more leaves the string open. Its lexical
  errors are reported at the constant's name. `s_filen` is the name of the file being read: for
  a compiled file the name it is compiled by (`QCC_PR_CompileFile (char *string, char
  *filename)`), which the preprocessor takes as its caller gives the path, so a caller gives it
  as `progs.src` wrote it; for an `#include`d file the bytes between its quotes joined by
  `QCC_JoinPaths` to the compiled file's name, after its last `/` or `\`, or alone when they
  start with one (`QCC_FindBestInclude(msg, compilingfile, false);`), so a file another included
  file includes is named beside the compiled file; and from a `#pragma file(NAME)` or `#pragma
  file NAME` (any case of `file`) to the end of that file, NAME (`s_filen = e =
  qccHunkAlloc(strlen(m)+1);`), the pragma's text being bytes to the end of its line, read from
  after a space or tab or from a `(`, its trailing white space dropped and, from a `(`, ended at
  the first `)`, and never lexed as code. In a macro's chunk `s_filen` is the file's name, the
  line and the macro's name joined by `:` (`QC_snprintfz(p, b, "%s:%i:%s", ...)`), which the
  `:` cut ends at the file's name. So with `progs.src` listing `t.qc`, `void() f = :__FILE__;`
  binds the name `t.qc`, after `#pragma file(ex_a:b)` `ex_a`, and after `#pragma file(e\\x)`
  `e\x`; with `progs.src` listing `src/a.qc`, which includes `x/b.qh`, which includes `c.qh`, the
  pinned FTEQCC names them `src/a.qc`, `src/x/b.qh` and `src/c.qh`. The semantic model gives the
  preprocessor each file's name as FTEF-14.73 says.
- **FTEF-14.33** An `#if` or `#elif` expression is read as `ParsePrecompilerIf` reads it, not
  with the C precedence of PRJ-5.5, by the preprocessor and by the graph of a module of this
  dialect. `||` and `&&` share one level and group to the right, the right side read first:
  `eval = ParsePrecompilerIf(level)||eval;` and `eval = ParsePrecompilerIf(level)&&eval;` under
  `case PPI_LOGICAL:`, so `0 && 0 || 1` is `0 && (0 || 1)`, 0, where C makes it 1. The
  comparisons `<=`, `>=`, `<`, `>`, `!=` and `==` share the level below in the same way (`eval =
  eval <= ParsePrecompilerIf(level);` under `case PPI_COMPARISON:`), so `1 < 2 == 1` is `1 < (2
  == 1)`, 0. A value (`PPI_VALUE`) is a parenthesised expression, `!` and a value, `defined` and a
  name with or without parentheses, or a word as `QCC_PR_SimpleGetToken` reads it, up to white
  space, `,`, `;`, `(`, `)`, `]` or `//`, whose macro's text, or else the word itself, is read
  with `atoi`: `1||0` is one word, 1. The reader starts after `#if`'s name (`pr_file_p =
  directive+2`) or `#elif`'s, reads within the line, as its white space is all but a line break
  (`qcc_iswhite(*pr_file_p) && *pr_file_p != '\n'`), and stops after the whole expression; a `\`
  is a byte like any other, so `#if 0 \`, a line break and `|| 1` is 0, and the line's end is then
  read as FTEF-14.35 says, which makes the `\` there "unexpected tokens at end of line". Where
  FTEQCC reports an error, "unclosed bracket condition", "unclosed defined condition" or
  "unexpected end-of-line" (`#if` with nothing after it), the preprocessor reports it (`E0121`)
  and takes the expression as false; the graph takes it as false.
- **FTEF-14.34** **Withdrawn** (P2-103, #520): it said the preprocessor reports FTEQCC's "Explicit
  precompiler usage when not defined NAME" as `E0121` for a `#` that stands first on its line of the
  output, where the semantic model would drop the line as a directive's. The semantic model now
  parses the output with no directive lines (QC-9.19). Replaced by FTEF-14.52.
- **FTEF-14.35** After a directive's operand `QCC_PR_SkipToEndOfLine` reads to its line's break,
  passing over `/* */` comments, which may span lines, strings before any `//`, and each `\`
  and line break (LF or CR LF), in a `//` comment too, so after `#undef X // c \` or `#ifdef X
  // c \` the next line is part of the directive, never read as code. For `#define`, `#append`,
  `#undef`, `#if`, `#ifdef`, `#ifndef`, `#else`, `#endif`, `#package`, `#pack`, `#forcecrc`,
  `#merge`, `#includelist`, `#include`, `#output`, an `#elif` that is taken (FTEF-14.37) and
  `#endlist` (FTEF-14.41) it is called with `errorifnonwhite`, so a byte outside a comment and
  before any `//` that is no white space, a string's `"` included, is the error "unexpected
  tokens at end of line" (`E0121`; `QCC_PR_ParseWarning(ERR_UNKNOWNPUCTUATION, ...)` reports an
  error code as an error): `#ifdef X Y`, `#endif X`, `#else X` and `#include "b.qh" X` are
  refused. A `#pragma`'s line ends at its break (FTEF-14.43). Reading goes on at the break, which
  ends the line, so the next line may start a directive.
- **FTEF-14.36** **Withdrawn** (P2-116, #544): it said this profile does not report FTEQCC's
  warnings for `#undef` of no macro and for an `#if` form with no white space after its name,
  which are on by default. Replaced by FTEF-14.57.
- **FTEF-14.37** FTEQCC counts the sides that are open and taken in one `static int ifs`, kept
  from file to file of a module, included files and the second autoprototyping pass included: a
  true `#if`, `#ifdef` or `#ifndef` adds one; a false one is skipped by
  `QCC_PR_FalsePreProcessorIf(false, ...)`, which adds one if it stops at a side it takes. That
  function reads a line at a time: after spaces and tabs a `#` and spaces and tabs before a word
  starting `endif` closes a level and one starting `if` opens one (`if (!strncmp(pr_file_p, "if",
  2)) level++;`); at the block's own level, while no side was taken, a word starting `else`
  takes the next side, its line read with `errorifnonwhite`, and one starting `elif`, `elifdef`
  or `elifndef` is tested, which needs a space or a tab after `elif`, `elifdef` or `elifndef`
  ("malformed #elif", `E0121`), and is taken when true, its line read with `errorifnonwhite`
  after the test; every line ends where `QCC_PR_SkipToEndOfLine` ends it, so a `//` comment
  ending in `\` hides the next line's `#endif`, and the end of the text is "#if with no endif"
  (`E0110`) at the conditional. On a taken side `#else` and `#elif` (and `#elsewhere`) take one
  from the count, read their line (`#else`'s with `errorifnonwhite`) and skip to the block's end
  with `QCC_PR_FalsePreProcessorIf(true, ...)`, which takes no side, so a second `#else` there is
  skipped too; `#endif` takes one. Either with nothing open is "#else outside of #if" or
  "unmatched #endif" (`E0109`). A taken side left open at the end of a file is closed by an
  `#endif` in a file read after it, and is not reported.
- **FTEF-14.38** `#error` is the error `E0118` "#Error: TEXT" (`QCC_PR_ParseError(ERR_HASHERROR,
  "#Error: %s", msg);`), `#warning` the warning `W0103` "#warning: TEXT" and `#message` reports
  nothing. TEXT is the bytes from right after the directive's name, a space before the text
  included, to the first NUL or line break, at most 1023, and for `#error` to a tab too
  (`pr_file_p[a] != '\t' && pr_file_p[a] != '\n'`), for `#warning` and `#message` to a CR too;
  the line is then read without `errorifnonwhite`. So `#warning Toggle doesn't work` is
  "#warning:  Toggle doesn't work", and no `'` in it opens a vector.
- **FTEF-14.39** The build directives change the build, not the text read, but each reads its
  line as FTEQCC does: `#copyright` and `#message` read it without `errorifnonwhite`; `#forcecrc`
  reads a number with `QCC_PR_LexInteger`, which takes the byte after the name whatever it is
  and then digits, `.` and `a` to `f`, so `#forcecrc` at a line's end takes the line break and
  reads the next line as its own; `#pack id` passes over `id` and one byte more, and `#pack N`
  reads `N` with `QCC_PR_LexInteger` and one byte more, a number above 5 being "No more than 5
  packs are allowed" (`E0121`); `#output` and `#merge` read a string with
  `QCC_PR_SimpleGetString`; and each then reads its line with `errorifnonwhite`. `#package`
  reads its line so and then ends the text (`pr_file_p += strlen(pr_file_p);`), as `#eof` ends it
  (`pr_file_p = NULL;`). `#datafile` reads a string, one byte more, and the bytes to the next
  line break. `#append NAME text` adds `text` to the body of macro `NAME`, or defines it when it
  is not defined (`append with no previous define is treated as just a regular define`).
- **FTEF-14.40** `#include` takes the bytes between `"` and `"`, or `<` and `>`, after the spaces
  and tabs after its name ("Not a string literal (on a #include)" for another byte, "#include
  continued over line boundry" for a line break first, each `E0113`), reads its line with
  `errorifnonwhite`, and `QCC_FindBestInclude(msg, compilingfile, false)` finds the file: the
  bytes joined with `QCC_JoinPaths` to the name of the file being compiled, then to each include
  directory in order, the first that names a file being read (`if (FileSize(fullname) == -1) {
  includepath++; continue; }`), none being "Couldn't open file" (`E0111`). The file being
  compiled is the one `progs.src` or an `#includelist` lists, never an included one, so a file
  an included file includes is found beside the compiled file: with `progs.src` listing
  `src/a.qc`, which includes `x/b.qh`, which includes `c.qh`, the pinned FTEQCC reads `src/c.qh`
  and refuses the program when only `src/x/c.qh` exists. An empty name includes nothing, and a
  name `#pragma once` marked is not read again (FTEF-14.70). The file is read next, as text of
  its own, after the directive's line. The vanilla dialect keeps QC-9.2.
- **FTEF-14.41** `#includelist` reads its line with `errorifnonwhite`, then words: before each,
  white space and comments are read as `QCC_PR_LexWhitespace(false)` reads them, which runs the
  directives at the starts of their lines (FTEF-14.45); a word is what `QCC_PR_SimpleGetToken`
  reads, a byte it stops at with no word is passed over, and the word `#endlist`, wherever it
  stands, ends the list, its line read with `errorifnonwhite`. Every other word names a file the
  graph compiles (PRJ-5.3); the preprocessor produces no token for the list. The end of the text
  before `#endlist`, reached or made by a directive in the list (`#eof`, `#package`,
  `DONT_COMPILE_THIS_FILE`), is FTEQCC's "eof in includelist", which the preprocessor reports as
  `E0406` "#includelist without #endlist" at the `#includelist`. `#endlist` outside a list is no
  directive (FTEF-14.52).
- **FTEF-14.42** `QCC_PR_DefineName` refuses a macro name of 64 bytes or more (`if (strlen(name)
  >= MAXCONSTANTNAMELENGTH || !*name) QCC_PR_ParseError(ERR_NAMETOOLONG, "Compiler constant name
  length is too long or short");`, `#define MAXCONSTANTNAMELENGTH 64` in `qcc.h`), for `#define`
  and `#append`: this profile reports it (`E0121`) and defines nothing. A name of 63 bytes is
  defined. `QCC_PR_PreProcessor_Define` reads the directive's own word first with
  `QCC_PR_SimpleGetToken` and the name after it, so `#defined N 5` defines `N`, `#defineN 5`
  defines `5` and `#define/**/N 5` defines `5`, and a refused definition's line is read with no
  error of its own, as FTEQCC's compile stops at the first.
- **FTEF-14.43** `#pragma`'s name is the bytes after its spaces and tabs up to the first space,
  tab or `(` met while the name read so far is empty, and its text the bytes after (from the
  `(`, or after the space or tab) to the line's break, its white space at the end dropped; a
  pragma's name is matched in any case (`QC_strcasecmp`). `#pragma line(N)` and `#pragma line N`
  number the line after the pragma `N`: `pr_source_line = strtoul(m, &m, 0)-1;`, `m` the text
  after a `(` it starts with, and the pragma's line break adds one. `strtoul` reads a sign, `0x`
  and hex digits, `0` and octal digits, or decimal digits, nothing being 0, a value too large
  `ULONG_MAX` and a `-` negating it, on a 64-bit `unsigned long`, and FTEQCC's `int` keeps its
  low 32 bits: so `#pragma line 0x10` numbers the next line 16, `#pragma line 010` 8, `#pragma
  line -1` -1 and `#pragma line 4294967297` 1. The lines after it count on from there to the end
  of the file or chunk the pragma is in, which `__LINE__` and `__LINESTR__` write (FTEF-14.27)
  outside a macro's expansion (in one, FTEF-14.46),
  and an `#include`d file's numbering ends with it (`QCC_PR_UnInclude` restores `pr_source_line`).
  FTEQCC's diagnostics report the line too; this profile's diagnostics keep the place of their
  text, a deliberate divergence, as a span in the file says more than a line FTEQCC was told.
- **FTEF-14.44** **Withdrawn** (P2-116, #544): it said this profile keeps only F303's warning
  setting and does not report "Unknown pragma". Replaced by FTEF-14.70.
- **FTEF-14.45** `QCC_PR_LexWhitespace(false)`, which reads the white space and comments after a
  function-like macro use's `(` and after each `,` of its arguments (FTEF-14.22), and before
  each word of an `#includelist` and each byte `DONT_COMPILE_THIS_FILE` passes over, calls
  `QCC_PR_NewLine(false)` at each line ending, which runs a directive at the next line's start
  (FTEF-14.52): so with `#define M(x) x`, `M(`, a line break, `#define Y #5`, a line break and
  `Y)` defines `Y` and expands to `#5`, and `M(`, a line break, `#ifdef Q`, `zz`, `#endif` and
  `#5)` is `#5`. The directive's line is no part of the argument, which starts after the white
  space. A line break in the middle of an argument is part of it (FTEF-14.22), so a directive on
  the next line is the argument's text, run where the expansion holds it (FTEF-14.46). A
  directive that ends the text, `#eof` or `#package`, ends the arguments, which is "EOF on macro
  call" (`E0117`), and the pinned FTEQCC crashes on `#eof` there. A use refused with its
  arguments read, for "EOF on macro call", too many parameters (`E0116`) or the bound of
  FTEF-14.28, is an error where FTEQCC's compile stops; this profile then reads the use again
  as it is written, unless a directive ran among its arguments, when reading goes on after the
  text the arguments took (after the `)`, or at the end of the text), so that no directive
  runs twice: with `#define M(x) x`, `M(`, a line break, `#append A 2`, a line break and `1, 2,
  3) A` appends to `A` once.
- **FTEF-14.46** A macro's expansion is read as a chunk of text (FTEF-14.17), and a `#` that
  starts a line of it, after a line break in it and spaces and tabs, is a directive there,
  where its text's line breaks are those `QCC_PR_PreProcessor_Define` kept in the body before a
  continued line starting with `#` (FTEF-14.24) or an argument held; a `#` at the chunk's start
  is none. Such a body is FTEQCC's warning F303 "preprocessor directive within preprocessor
  macro NAME" at the first such `#`, once for the definition (`if (!cnst->evil)`), an error by
  default (`qccwarningaction[WARN_EVILPREPROCESSOR] = WA_ERROR;`, `E0122`), a warning after
  `#pragma warning enable F303` (`W0109`) and nothing after `#pragma warning disable F303`. With
  F303 off, `#define D \`, a line break, `#define Y #5 \`, a line break and `Y` makes `D` define
  `Y` and expand to `#5`. In the chunk a `#pragma file` names, and a `#pragma line` numbers, the
  rest of that chunk only, and `#eof` and `#package` end it; the pinned FTEQCC crashes on `#eof`
  in a chunk, where this profile ends the chunk. The chunk's own `__LINE__` and `__LINESTR__`
  still write the line of its use (`if (currentchunk && currentchunk->cnst) line =
  currentchunk->currentlinenumber;`), so the lines a `#pragma line` numbers there are those of
  the uses after it in the chunk, whose expansions write them: with F303 off, `#define L
  __LINE__`, a `D` whose body is `#pragma line(30)` and `L __LINE__` on lines of their own, and
  `D __LINE__` on line 6, are `30 6 6`.
- **FTEF-14.47** Deliberate divergences of this profile's directives: an `#include` met among a
  use's arguments, in a macro's expansion, in an `#includelist` or in the text
  `DONT_COMPILE_THIS_FILE` passes over is refused, as "#include where this profile does not
  follow it" (`E0121`), where FTEQCC reads the file as the text it goes on reading there, which
  ends the arguments or list it is in; an `#eof` run anywhere but at the start of a file, in a
  chunk too, ends the text it is in, where the pinned FTEQCC crashes, as
  `QCC_PR_LexComment` and `QCC_PR_LexWhitespace` read on from the null `pr_file_p` it leaves
  (`cargo xtask oracles builtins` counts those cases as differing as this statement says: an
  FTEQCC case that holds `#eof` after its first byte, refused by FTEQCC and accepted here with
  no error); an `#include` of a file already being read is "recursive
  #include of PATH" (`E0112`), where FTEQCC reads it again until it runs out of memory; a macro
  name in an `#includelist` is read as a file's name, where FTEQCC expands it
  (`QCC_PR_CheckCompConst()`), as the graph reads the list (PRJ-5.3); `#merge` and `#datafile`
  open no file, where FTEQCC refuses a missing one, as the files are the build's; and a
  directive met while 64 others are being run inside the readings they started, an
  `#includelist` or `DONT_COMPILE_THIS_FILE` inside another, is "directives nested too deeply"
  (`E0121`) and ends the reading, where FTEQCC's recursion has no bound, so that no input can
  exhaust the stack.
- **FTEF-14.48** Before the first file FTEQCC's `QCC_SetDefaultProperties` defines `FTEQCC` and
  `__FTEQCC__` with empty bodies (`QCC_PR_DefineName("FTEQCC", NULL);
  QCC_PR_DefineName("__FTEQCC__", NULL);`), so `#ifdef FTEQCC` is true and `#if FTEQCC` 0, and
  `#undef` removes them. It runs again before the second autoprototyping pass (FTE-22.3), which so
  starts with those two macros only, no include directory but the module's (FTEF-14.49), no `#pragma
  once` mark (`QCC_PR_CloseProcessor`), and F303 an error again; the count of open sides
  (FTEF-14.37) is kept, being `QCC_PR_Precompiler`'s own.
- **FTEF-14.49** A module's include directories (PRJ-5.2) are FTEQCC's `-I` options:
  `QCC_PR_CommandLinePrecompilerOptions` gives each to `QCC_PR_AddIncludePath(name)` (`else if (
  !strncmp(myargv[i], "-I", 2) )`), which keeps it as `#pragma includedir` does (FTEF-14.70): a `/`
  after it unless it ends in one, at most 8. `QCC_SetDefaultProperties` calls it after
  `QCC_PR_CloseProcessor`, so they are the first include directories, before any `#pragma
  includedir` adds one, and they are there again for the second autoprototyping pass (FTEF-14.48).
  `QCC_FindBestInclude` tries them after the file being compiled (FTEF-14.40), so with `progs.src`
  listing `src/a.qc`, which includes `x/b.qh`, which includes `c.qh`, and only `src/x/c.qh` there,
  `-Isrc/x` reads it, as Rad-Therapy's `client/Makefile` gives `-I../shared` for `shared/defs.h`'s
  `#include "Weapon.h"`.
- **FTEF-14.50** FTEF-14.43's `strtoul` is the host C library's, and the pinned FTEQCC reads a
  `#pragma line` number otherwise where `unsigned long` is 32 bits (LLP64, 64-bit Windows): there a
  value of 2^32 or more is `ULONG_MAX` = 4294967295, so `#pragma line 4294967297` numbers the next
  line -1, where an LP64 host (64-bit Linux and macOS) numbers it 1. This profile reads the number
  as an LP64 build does on every host, a deliberate divergence from the LLP64 build, so that a
  project reads the same wherever it is opened; the builtin differential records the LP64 outcomes
  and lists the cases a 32-bit `strtoul` reads otherwise as expected where the oracle ran on an
  LLP64 host.
- **FTEF-14.51** Under this profile the syntax tree, which shows a file as written and runs no
  directive (both sides of a conditional are parsed), makes a directive node of the text
  the preprocessor reads as one directive (`qc_preproc::fte_directive_end`). It starts at a `#`
  that `QCC_PR_NewLine` reaches (FTEF-14.52), the file read as one being compiled, so a `#`
  after spaces and tabs on its first line starts one too, and it ends where the preprocessor
  reads on after it with nothing run: at the end of its line as `QCC_PR_SkipToEndOfLine` ends it
  (FTEF-14.35), after the operand the directive reads, a `#define`'s body (FTEF-14.24) or an
  `#if` expression (FTEF-14.33) among them, and for an `#elif`, `#elifdef` or `#elifndef` after
  its test, as `QCC_PR_FalsePreProcessorIf` reads one after the side before it was skipped
  (FTEF-14.37), so `#elif (1/*)` ends at its `)`; at a `#pragma`'s line break (FTEF-14.43);
  after the word of an `#if`, `#ifdef` or `#ifndef` with no white space after its name
  (`#if(1)`, FTEF-14.57); and at an `#eof`'s line break, as FTEQCC reads nothing after it. So
  `#undef X // c \`, a line break and `float y;` is one node of two lines, and `#pragma p // c
  \` ends at its line break. A `#` after a `/* */` comment on its line or in the middle of a
  line, and a `#` whose name matches no directive (`#foo`), are read as code (FTE-23.10).
  Unless only white space stands between the end and the token before it, the text from the end
  is lexed again as the preprocessor lexes it where it reads on (`qc_preproc::relex_from`,
  FTEF-14.22), so `#if"a b"` is a node `#if"a` and the code ` b"`. A lexical error the first
  lexing found in a node's text, as the unclosed string of `#define X "a`, is reported by
  neither, as the preprocessor reads that text as bytes. The text an `#if` or `#else` skips, an
  `#includelist`'s list (FTEF-14.41) and what `#eof`, `#package` and `DONT_COMPILE_THIS_FILE`
  leave unread are parsed as code. The vanilla dialect keeps QC-9.1's node: a `#` first on its
  line before a name on that line, to the first line break after it. A rendering of the
  preprocessor's output has no directive lines in either dialect (QC-9.19).
- **FTEF-14.52** `QCC_PR_Precompiler` reads a directive at a `#` that `QCC_PR_NewLine` reaches:
  the first byte after a line ending (LF, or a CR not before a LF, `qcc_islineending`) that the
  white-space reader passes, a `//` comment's included, and the spaces and tabs after it (`while
  (*pr_file_p==' ' || *pr_file_p == '\t') pr_file_p++;`), or the start of a file being compiled
  after its spaces and tabs (`QCC_PR_CompileFile` calls `QCC_PR_NewLine` first). An `#include`d
  file is read with `QCC_PR_Precompiler()` at its first byte and no spaces skipped, so `  #define
  N 5` on its first line is no directive. A `#` after a `/* */` comment on its line, or after one
  that spans lines, or in the middle of a line, is none. After the `#` every byte up to `' '` but
  a line break is passed over (`for (directive = pr_file_p+1; *directive; directive++)`, `if
  (*directive > ' ') break;`, a byte of 0x80 or more too, as FTEQCC's `char` is signed), so `#
  define` and `#` and a tab or a vertical tab before `define` are directives; a line break first
  is "Hanging # with no directive" (`E0121`). The name is matched by its first bytes (`if
  (!strncmp(directive, "define", 6))`), in FTEQCC's order: `define`, `append`, `undef`, `if`,
  `else` and `elif`, `endif`, `eof`, `error`, `warning`, `message`, `copyright`, `package`,
  `pack`, `forcecrc`, `merge`, `includelist`, `include`, `datafile`, `output` and `pragma`. So
  `#defined`, `#iffy`, `#elsewhere`, `#endiff`, `#eofx`, `#errors`, `#includes` and `#pragmas`
  are those directives. A name that matches none, `#endlist` and `#foo` among them, leaves the
  `#` to be read as code (FTE-23.10), `#NAME` a macro's use, and FTEQCC's "Explicit precompiler
  usage when not defined NAME" when `NAME` is no macro, which this profile refuses where the
  parser reads the `#`, first on its line of the output too (QC-9.19): `E0209` in a builtin
  initialiser (FTE-23.10), the parser's other errors elsewhere. Each directive's operand is read
  from its name's end as its own name's length gives it (`pr_file_p = directive+5;` for
  `undef`), so `#undefine N` undefines `ine`.
- **FTEF-14.53** In code a `\` directly before a line break joins the lines (FTEF-13.2) with no
  `QCC_PR_NewLine`: `QCC_PR_LexPunctuation` passes over both (`else if ((unsigned char)*pr_file_p ==
  (unsigned char)'\\' && (pr_file_p[1] == '\r' || pr_file_p[1] == '\n')) pr_file_p+=2;`, and
  `pr_file_p+=3;` for a CR LF) and calls `QCC_PR_Lex()`, whose white-space reader meets no line
  ending before the next line's first byte, so a `#` there, after spaces and tabs too, is no
  directive (FTEF-14.52) but code: the pinned FTEQCC refuses `float g; \`, a line break and `#define
  N 1`, `#undef N` or `#include "b.qh"` ("Explicit precompiler usage when not defined define", and
  `undef` or `include`), and with `#define undef 6;` and `#define N`, `float g = \`, a line break,
  `#undef N` and `#ifdef N` takes the `#ifdef`'s side. A `\` in a `//` comment joins nothing, as the
  white-space reader ends the comment at its line break and calls `QCC_PR_NewLine`; nor does a `\`
  that another line ending follows before the `#`, which the white-space reader passes, nor one in a
  directive's text, after which reading goes on at or after the line break (FTEF-14.35, FTEF-14.43).
  The rule holds wherever FTEQCC lexes code: a file the preprocessor reads, a macro's expansion,
  whose text holds a `\` and a line break only from an argument (FTEF-14.22, FTEF-14.24), so with
  `#define M(x) x`, `void() f = M(#5 \`, a line break, `#define Y 6` and `);` is refused, and the
  project graph's code (PRJ-5.13); the syntax tree makes no directive node of such a `#`
  (FTEF-14.51).
- **FTEF-14.54** **Withdrawn** (P2-116, #544): it said this profile does not add the line feed
  `QCC_LoadFile` writes after a file's text, and the end of `#if`, `#ifdef` and `#` alone at a
  file's end showed that its readers do not take the text's end for a line's. Replaced by
  FTEF-14.64.
- **FTEF-14.55** FTEQCC's `atoi` is the host C library's, and this profile reads it as an LP64 host
  (64-bit Linux and macOS) does, on every host: `(int)strtol(s, NULL, 10)` (glibc `stdlib/atoi.c`,
  `return (int) strtol (nptr, (char **) NULL, 10);`; Apple Libc `stdlib/FreeBSD/atoi.c`, `return
  (int)strtol_l(str, (char **)NULL, 10, __current_locale());`, FTEQCC setting no locale), which
  passes over the bytes the C locale's `isspace` takes (space, tab, LF, VT, FF and CR), reads a
  sign and decimal digits, holds a value past a 64-bit `long` at `LONG_MAX` or `LONG_MIN`, and keeps
  the low 32 bits in the `int`. It is the `atoi` of each FTEQCC reading that names one: an `#if`
  value (FTEF-14.33), whose `int eval` (`eval = atoi(pr_token);`, `eval = atoi(c->value);`) is
  compared and negated as an `int`; `#pack`'s number (`QCC_PR_LexInteger`, `return atoi
  (pr_token);`, FTEF-14.39); the `noref` and `autoproto` pragmas (FTEF-12.6, FTEF-12.10); and
  `$framevalue` (`pr_macrovalue = atoi(pr_token);`, FTEF-18.16). So, as the pinned FTEQCC reads them
  on macOS arm64, `#if 4294967296`, `#if -99999999999999999999` and `#define X 4294967296` then `#if
  X` are false; `#if 4294967297 == 1`, `#if 2147483648 < 0`, `#if -2147483649 > 0` and `#if
  99999999999999999999 == -1` are true; `#if` or a `#define X` body of U+00A0, U+3000 or U+0085
  before `1` is false, as `strtol` passes over none of their bytes; `#pack 4294967296` is one pack;
  and `$framevalue 4294967301` numbers the next frame 5, `$framevalue 99999999999` 1215752191. Where
  `long` is 32 bits (LLP64, 64-bit Windows) `strtol` holds a value past an `int` at `INT_MAX` or
  `INT_MIN`, so there `#if 4294967296` is true: a deliberate divergence from that build, as
  FTEF-14.50's, so that a project reads the same wherever it is opened, and the builtin
  differential lists a case whose directive holds a number the two read otherwise as expected where
  the oracle ran on an LLP64 host, a project graph case among them (PRJ-5.13). The vanilla
  dialect's expressions keep PRJ-5.5.
- **FTEF-14.56** Under this profile the syntax tree reads its directive nodes (FTEF-14.51) in the
  text `QCC_LoadFile` gives (FTEF-14.64), at the file's own offsets
  (`qc_preproc::qcc_load_in_place`), so that it makes a node of
  each directive the preprocessor runs and stays the file's bytes: each NUL byte is read as a
  space, so the text goes on past it and a directive on a later line is a node, and a directive's
  start is found in the text after a UTF-8 byte order mark at the file's start, so `#define N 1`
  on the first line after one is a node, after spaces and tabs too. Only that first mark is
  skipped: after a second one the `#` starts no line, as FTEQCC reads it. The NUL and the mark
  stay in the tree as white space, and a value read from a token's text, an engine name's string
  among them (`ast::Builtin::engine_name`, FTE-17.5), reads a NUL as that space. Text a
  preprocessor wrote (`qc_syntax::parse_preprocessed`) is no file the loader read and still ends
  at its first NUL. A file that starts with a UTF-16 or UTF-32 byte order mark is
  read by the tree as its bytes to the first NUL, as under the vanilla dialect (QC-1.1): the
  preprocessor reads its decoded text (FTEF-14.64), whose offsets are not the file's, so how such
  a file is shown is the semantic model's choice, not the tree's. The vanilla dialect keeps
  QC-1.1.

- **FTEF-14.57** `#undef` reads a name with `QCC_PR_SimpleGetToken` after the spaces and tabs
  after its name and removes the macro; a name of no macro, the empty one included, is FTEQCC's
  warning "Precompiler constant NAME was not defined" (`WARN_UNDEFNOTDEFINED`, with no name,
  `W0114`), on by default, which comes before any "unexpected tokens at end of line" of its line.
  `#ifdef` and `#ifndef` read a name in the same way, after a white-space byte that must follow
  their name; a missing name is no macro's, so `#ifdef` alone is false and `#ifndef` alone true.
  `#if`, `#ifdef` or `#ifndef` with a byte that is no white space right after the name (`if
  (!qcc_iswhite(*pr_file_p))`), as in `#ifdefX`, `#iffy` and `#if(1)`, is the Q120 warning
  "Unknown pragma" (`W0113`) and opens no block: reading goes on as code after the word
  `QCC_PR_SimpleGetToken` reads from the name, so `#if(1)` leaves `(1)`. FTEQCC's warning names
  `qcc_token`, the last word any `QCC_COM_Parse` or `QCC_COM_Parse2` read, a macro's expansion
  or a `progs.src` entry included (the pinned FTEQCC prints "Unknown pragma 't.qc'" for `#iffy`
  in `t.qc`); this profile names the word, `'iffy'`, a deliberate divergence, as the word FTEQCC
  prints stands for nothing in the directive. At the end of a file `#if`, `#ifdef` and `#ifndef`
  read FTEQCC's line feed after the text (FTEF-14.64).
- **FTEF-14.58** `QCC_PR_PreProcessor_Define`'s errors have FTEQCC's texts: "No name defined for
  compiler constant" (`E0114`), and for a parameter list it refuses (FTEF-14.23) "May not have more
  than 32 parameters to a macro", "parameter name is too long (max 32)", "varadic argument must be
  last", "duplicate macro paramter name 'NAME'" and "missing ) in macro parameter list"
  (`E0115`). Its warnings are F315's (`WARN_DUPLICATEPRECOMPILER`, `W0115`) and F316's
  (`WARN_IDENTICALPRECOMPILER`, `W0116`, off by default), at the macro's name: an `#append` of a
  macro whose parameter count or `...` differs is "different number of macro arguments in macro
  append", and one whose names differ "arguments differ in macro append", each then defining the
  macro anew; a definition of a macro that already has one, an `#append` defining anew included, is
  "Alternate precompiler definition of NAME (OLD -> NEW)", OLD and NEW the stored bodies, when they
  differ, and "Identical precompiler definition of NAME" when they do not, the parameters not
  compared (`if (strcmp(oldcnst->value, cnst->value))`); and a body's `/*` with no `*/` before the
  text's end is "EOF inside quote in define NAME". So `#define A 1` then `#define A() 1` is
  F316's, and `#define F(a) a` then `#append F(b) 2` gives "arguments differ in macro append" and
  "Alternate precompiler definition of F (a -> 2)", as the pinned FTEQCC prints them.
- **FTEF-14.59** **Withdrawn** (P2-116, #544): it gave "Include path too long." for a `#pragma
  includedir` of 2048 bytes, which FTEQCC's `char msg[1024]` cannot hold (FTEF-14.66). Replaced
  by FTEF-14.68.
- **FTEF-14.61** In a function-like macro's body a `#` before a token that names no parameter, read
  with `QCC_COM_Parse2`, is written as `#` and the token, and is FTEQCC's warning "'#TOKEN' is not
  a macro parameter in NAME" (`QCC_PR_ParseWarning(0, ...)`, the warning of code 0, with no name,
  `W0117`), at the `#`, for each expansion, unless the body holds a directive (`if (!c->evil)`,
  FTEF-14.46). So `#define E(x) #5:x` used as `E(ex_a)` warns "'#5' is not a macro parameter in
  E", as the pinned FTEQCC does.
- **FTEF-14.62** FTEQCC's `QCC_PR_DefineName` warns "Duplicate definition for Precompiler constant
  NAME" (Q207) when a `-D` option gives a macro defined before, `FTEQCC` and `__FTEQCC__` included,
  another body; it prints it against no file, "cmdline", and this profile, which defines a module's
  `defines` as FTEQCC's `-D` options (FTEF-14.48), does not report it. `#define` removes the macro
  before it defines it, so it never gives this warning.
- **FTEF-14.63** FTEQCC's warnings are reported as its `qccwarningaction` sets each, `WA_IGNORE`
  not at all, `WA_WARN` as the warning and `WA_ERROR` as an error, which FTEQCC prints as "werror"
  (`QCC_PR_PrintWarning`, `else if (qccwarningaction[type] == 2)`): `E0125` with the warning's
  text, F303's `E0122` (FTEF-14.46). The settings start as `QCC_SetDefaultProperties` makes them,
  every warning `WA_WARN` but F208, F209, F300, F311, F330, F322, F301, F302, F331, `localptr` and
  F316 `WA_IGNORE` and F303 and F313 `WA_ERROR` (`qccwarningaction[WARN_FTE_SPECIFIC] =
  WA_IGNORE;` and the rest), and again before the second autoprototyping pass (FTEF-14.48). `#pragma
  warning STATE NAME ...` reads its text with `QCC_COM_Parse`: STATE `enable` or `on` warns,
  `disable`, `off` or `ignore` ignores, `error` errs and `toggle` sets `!!` of the setting, so a
  warning stays one and an error becomes one (`qccwarningaction[wn] = !!qccwarningaction[wn];`), in
  any case (`stricmp`); any other STATE is the Q120 warning "warning state not recognised"
  (`W0113`) and changes nothing. Each NAME is looked up in `warningnames[]` in any case
  (`QCC_WarningForName`, `stricmp(name, warningnames[i].name+1)`): `Q100` to `Q302`, `F129`,
  `F207` to `F211`, `F300` to `F335`, `field-redeclared`, `deprecated`, `bounds`, `octal`,
  `unimplemented`, `localptr` and `largereturn`; one it does not know is "warning id not
  recognised" (`W0113`, as the settings stand when FTEQCC reads it). A setting holds for the rest of
  the module, the files read after it included, and a warning is reported as the settings stand
  where FTEQCC reads its bytes: so `#pragma warning disable F325` before a `/* /* */` comment
  drops `W0112`, and `#pragma warning error octal` makes `float y = 017;` after it `E0125`, which
  the pinned FTEQCC refuses ("werror octal: A leading 0 is interpreted as base-8."). Warnings
  FTEQCC gives no name are always reported. The settings in force from each token on are in the
  preprocessor's output (`qc_preproc::FileOutput::warnings`), for the warnings the parser and the
  semantic model report.
- **FTEF-14.64** A source file is read as the text FTEQCC's `QCC_LoadFile` (`qcc_cmdlib.c`) hands
  its lexer, by the preprocessor and by the project graph (PRJ-5.13), a compiled file and an
  `#include`d one alike. `QCC_SanitizeCharSet` decodes a file that starts with the byte order mark
  of UTF-32LE (`ff fe 00 00`), UTF-32BE (`00 00 fe ff`), UTF-16LE (`ff fe`) or UTF-16BE (`fe ff`),
  tested in that order, to UTF-8 with `decodeUTF`, skips a UTF-8 one (`mem += 3;`), and leaves any
  other file as it is. `decodeUTF` reads whole code units only (`chars = inbytes / w;`), joins a
  high surrogate to a low one after it (`if (inc >= 0xd800u && inc < 0xdc00u && i+1 < chars)`),
  writes any other surrogate in three bytes as any value of U+0800 to U+FFFF, and makes a value
  past U+10FFFF U+FFFD (`if (inc > 0x10FFFF) inc = 0xFFFD;`). Then every NUL byte of the text is a
  space (`else if (!mem[check])`, `mem[check] = ' ';`), so the text goes on past a NUL where QC-1.1
  ends it: the pinned FTEQCC refuses `void() f = #5;`, a line break, a NUL and `zz zz zz` ("\"zz\"
  is not a type"), refuses `#define N #5`, a NUL, `zz`, a line break and `void() f = N;`, and binds
  `f` to builtin 5 after `#ifdef Q`, a line break, `zz`, a NUL, a line break and `#endif`. A NUL
  first in a compiled file is a space `QCC_PR_NewLine` passes over, and first in an included file a
  byte before its `#`, so no directive starts there (FTEF-14.52). After a UTF-8 byte order mark a
  directive starts a compiled file's first line, after spaces and tabs too, and an included file's
  first byte; FTEQCC binds `f` to builtin 5 from `#define N #5` and `void() f = N;` after one, with
  and without two spaces before the `#`, and from `void() f = #5;` written as UTF-16LE after its
  mark. FTEQCC warns "file contains null bytes N/LEN" once for a file with a NUL, N the first NUL's
  offset in the text and LEN the text's length (`QCC_PR_Warning(WARN_UNEXPECTEDPUNCT, filename,
  line, "file contains null bytes %u/%"pPRIuSIZE, check, len)`), a warning with no name, `W0117`.
  `QCC_LoadFile` then writes a line feed after the text (`mem[len] = '\n';`), and this profile's
  readers read it too, so every directive and token at a file's end meets a line's end before the
  text's: the pinned FTEQCC refuses a file that ends in `#if` ("unexpected end-of-line"), in
  `#ifdef` ("#if with no endif") or in `#` ("Hanging # with no directive"), and calls a string that
  the file's end leaves open "newline inside quote" (`E0102`), as this profile does. A diagnostic's
  place in the file is the bytes its text came from: 3 bytes on after a UTF-8 mark, in a decoded file
  the code unit each byte came from, and the file's end for the line feed
  (`qc_preproc::SourceFile::file_span`). The vanilla dialect keeps QC-1.1: a file's bytes, its text
  ending at the first NUL.
- **FTEF-14.65** **Withdrawn** (P2-116, #544): it left a `#pragma` line of 1024 bytes or more
  unmodelled, which this profile now reports as an error (FTEF-14.66). Replaced by FTEF-14.70.
- **FTEF-14.66** `QCC_PR_SimpleGetToken`, `QCC_PR_SimpleGetString` and `QCC_PR_LexMacroName`
  read into `pr_token` and stop the compile with `QCC_Error (ERR_INTERNAL, "token exceeds %i
  chars", i)` at a 1024th byte (`if (i == sizeof(qcc_token)-1)`), and `QCC_PR_SimpleGetString`
  with "new line inside string" at a line break in its string. So a name of 1024 bytes after
  `#define`, `#undef`, `#ifdef`, `#ifndef`, `#elifdef` or `#elifndef`, a word of an `#if` or
  `#elif` expression, an `#includelist` word or a list's output (PRJ-4.9), and a string of 1024
  bytes or with a line break after `#output`, `#merge` or `#datafile`, are each an error, `E0121`
  with FTEQCC's text, and a frame macro word of 1024 bytes is `E0502` with it; 1023 bytes are
  read. A `#pragma`'s name, or its text after the name, of 1024 bytes runs past `char
  msg[1024]`, which `msg[a++] = *pr_file_p;` fills with no bound. That is undefined behaviour,
  and the pinned builds differ. On macOS the build stops at the overrun (exit -6, for `#pragma
  foo` with 1024 bytes of text too, so it is `msg` and no path limit), and on Windows too; the
  Linux build of CI reads on and compiles the program for texts of 1024 and 1025 bytes. This
  profile reports `E0121` "#pragma name or text longer than FTEQCC's 1023 bytes" for each, a
  deliberate divergence. The differential takes such a case's other outcome on Linux as this
  statement's (`Overrun::Unseen`), and a recording there keeps the stopping build's outcome. `QCC_COM_Parse` keeps 1023 bytes of a longer word and drops the rest
  (`if (len >= sizeof(qcc_token)-1) ;`), which a pragma's words and a list's files are read
  with. After such an error FTEQCC's compile stops: the preprocessor reads no more of the file
  and the graph no more of it, but the lexer reads on after a frame word, so diagnostics FTEQCC
  would not reach may follow it. The pinned FTEQCC (`fteqcc -Tid`) gives each error at 1024 bytes
  and none at 1023.
- **FTEF-14.67** `QCC_PR_LexWhitespace` reports a `/* */` comment the text ends in with
  `QCC_PR_ParseError(0, "EOF inside comment\n");` where it reads white space outside the lexer's
  tokens: after a use's `(` and each `,` (FTEF-14.22), before each `#includelist` word
  (FTEF-14.41) and before each word of a list-form `progs.src` (PRJ-4.9). This profile reports it
  as `E0501` "EOF inside comment" at the comment's `/*`, and the reading ends at the end of the
  text; the pinned FTEQCC gives it for a list `progs.dat`, `t.qc /* open`.
- **FTEF-14.68** `QCC_PR_AddIncludePath`'s warnings have no name (`WARN_STRINGTOOLONG`, `W0117`):
  "Too many include dirs. Ignoring and hoping the stars align." for a ninth directory a `#pragma
  includedir` adds, and "Include path too long." for a directory that with its `/` has 2048 bytes
  or more, as the copy is bounded by the whole table (`sizeof(qccincludedir)`, `char
  qccincludedir[MAXINCLUDEDIRS][256]`), the directory then not kept. A `#pragma includedir` cannot
  name one so long: its text past 1023 bytes runs past FTEQCC's `msg` (FTEF-14.66), and
  `QCC_COM_Parse` and `char newinc[1024]` hold 1023 bytes of it. Only a module's `-I` option
  (FTEF-14.49) can, and FTEQCC prints its warnings against no file (`s_filen` "cmdline"), which
  this profile does not report. A name of 256 to 1023 bytes runs on into the next slots of
  FTEQCC's table, which this profile does not model: it keeps the name whole, as a later directory
  FTEQCC adds overwrites the bytes.
- **FTEF-14.69** `#copyright` copies into `msg` only the bytes after its name that are white
  space (`for (a = 0; a < sizeof(msg)-1 && qcc_iswhite(pr_file_p[a]) && pr_file_p[a] != '\0';
  a++)`), at most 1023, so its test `if (strlen(msg) >= sizeof(QCC_copyright))` against `char
  QCC_copyright[1024]` never holds and "Copyright message is too long" is never given; this
  profile gives no diagnostic for a `#copyright` of any length. The pinned FTEQCC gives none for
  2000 bytes of text or of spaces.
- **FTEF-14.70** Of the other pragmas, these change what the preprocessor reads:
  `DONT_COMPILE_THIS_FILE` passes over the rest of the text a byte at a time after white space
  read as `QCC_PR_LexWhitespace(false)` reads it, so the directives at the starts of its lines
  still run, an `#error` refusing the program; `once` marks the file being compiled
  (`strcpy(onced->name, compilingfile);`), not the file it is in, so a header's `#pragma once`
  keeps nothing from being read twice; `includedir` adds the directory its first token
  (`QCC_COM_Parse`) names, a `/` after it unless it ends in one, joined to the file being
  compiled, after the others, at most 8 (`MAXINCLUDEDIRS`, FTEF-14.68); `noref` and `autoproto`
  take their text with `atoi` (FTEF-12.6, FTEF-12.10), so `#pragma noref(1)` is 0; `warning STATE
  NAME ...` sets FTEQCC's warning settings (FTEF-14.63); and `flag` and `keyword` set the compiler
  flags (FTEF-12.11). The rest, `COPYRIGHT`, `compress`, `forcecrc`, `framerate`, `file`
  (FTEF-14.74), `line` (FTEF-14.43), `nosave`, `defaultstatic`, `wrasm`, `optimise` and
  `optimize`, `sourcefile`, `TARGET`, `PROGS_SRC`, `PROGS_DAT` and `opcode`, change the build,
  with the warnings of FTEF-12.11, and an unknown one is the Q120 warning "Unknown pragma 'NAME'"
  (`W0113`). `#pragma COPYRIGHT`'s warning "Copyright message is too long" needs a text of 1024
  bytes or more, which overruns FTEQCC's `char msg[1024]` first, as `#copyright`'s, which copies
  only the white space after its name (`qcc_iswhite(pr_file_p[a])`), cannot reach it; a pragma line
  of 1024 bytes or more is that undefined behaviour, which this profile reports as an error
  (FTEF-14.66).
- **FTEF-14.71** **Withdrawn** (P2-119, #554): it said `-std=` is not read, but `-std=` sets
  the keywords the parser reads (FTEF-20.1). Replaced by FTEF-14.72.
- **FTEF-14.72** FTEQCC's command line sets the warnings and compiler flags each time
  `QCC_SetDefaultProperties` resets them, before the first pass and the second autoprototyping
  pass, as it ends by calling `QCC_PR_CommandLinePrecompilerOptions`, which reads every
  argument in order, but the value `-srcfile`, `-src`, `-o`, the nine `-max_` options and a bare
  `-I` take, the next argument (`if (++i == myargc)`, `i++;`). First `QCC_CheckParm ("-Fqccx")`,
  in any case (`QC_strcasecmp`), ignores F313 and F314. An argument that
  starts with `-W` in any case (`strnicmp(myargv[i], "-W", 2)`) is a warning option, its text
  after `-W` compared in any case: `all` makes each ignored warning a warning but F208, F209,
  F311, F330 and F332 ("these warnings require -Wextra to enable"), and F319, Q205, F312, F307,
  F320, F322, F321, Q208 and F211 only while F321 is not ignored; `extra` makes every ignored
  warning a warning; `none` ignores every warning; `error` sets `werror`; `no-mundane` ignores
  F306, Q207, F319, Q201, F300, F208, Q302, Q205, Q120, F316, the warning of an `#undef` of no
  macro, F209, F301 and F331; `error-NAME`, `no-NAME` and `NAME` set the warning
  `QCC_WarningForName` finds for NAME (FTEF-14.63) to an error, ignored or a warning, and a
  NAME it does not find changes nothing. `-Farithmetic-exceptions` makes F332 an error and
  `-Fno-arithmetic-exceptions` ignores it. After every argument, with `werror` set, each warning
  not ignored is an error (`if (qccwarningaction[j]) qccwarningaction[j] = WA_ERROR;`). An
  argument that starts with `-F` or `-K` in any case turns on, or after `no-` off, the first row
  of `compiler_flag[]` whose abbreviation it names in any case, whether or not the row has
  `FLAG_MIDCOMPILE`, so only the command line sets `caseinsens`. The command line's warnings for
  arguments it does not recognise ("Unrecognised warning parameter", printed against "cmdline")
  are not reported: they belong to no file of the module. A `-F` NAME no row of `compiler_flag[]`
  has is a compatibility flag: `short-logic` sets the `lo` optimisation, `correct-logic`
  `ifvector` and `vectorlogic`, and `false-empty-strings` `ifstring`, each turned off after
  `no-`. An argument that starts with `-std=` in any case sets every flag and keyword to its
  default (`*compiler_flag[p].enabled = !!(compiler_flag[p].flags & FLAG_ASDEFAULT)`) and then
  a dialect's, in order with the `-K` and `-F` options: `C`, `c89`, `c90`, `c99`, `c11` and
  `c17` in any case "set up for greatest C compatibility" (`for`, `goto`, `extern`, `static`,
  `auto`, `switch`, `case`, `default`, `int`, `typedef`, `struct`, `union`, `enum`, `float`,
  `double`, `long`, `short`, `char`, `signed`, `unsigned`, `register` and `volatile` on; `asm`,
  `accessor`, `class`, `var`, `inout`, `optional`, `state`, `inline`, `nosave`, `shared`,
  `noref`, `unused`, `used`, `local`, `nonstatic`, `ignore`, `strip`, `vector`, `entity`,
  `string`, `integer`, `enumflags`, `thinktime`, `until` and `loop` off; `ILP32`, `lo`,
  `assumevar`, `subscope`, `cpriority`, `assumeint`, `assumedouble` and `iffloat` on and
  `qcfuncs`, `macroinstrings` and `boundchecks` off; and F302 a warning, Q101, Q105 and Q106
  errors, and F306, `octal` and Q205 ignored), `c++` the same with `class` and `inline` then
  on; `c23` none of them, being missing from the test of the C names; and, compared exactly,
  `qccx` (`qccx` on, F313 and F314 ignored), `preqcc` (`hashonly` and `macroinstrings` on),
  `reacc` (`acc` on, `macroinstrings` off), `frikqcc` (`state` on, `macroinstrings` off),
  `fteqcc` (none), `qcc` and `id` (`ifvector`, `vectorlogic`, `asm`, `break`, `continue`, `for`,
  `goto`, `const`, `var`, `inout`, `optional`, `state`, `inline`, `nosave`, `extern`, `shared`,
  `noref`, `unused`, `used`, `static`, `nonstatic`, `ignore`, `strip`, `switch`, `case`,
  `default`, `accessor`, `class`, `vector`, `entity`, `float`, `string`, `int`, `integer`,
  `typedef`, `struct`, `union`, `enum`, `enumflags`, `thinktime`, `until`, `loop`, `wrap` and
  `weak` off, and F320 an error), `hcc` and `hexenc` (the same off but `break`, `switch`,
  `case` and `default`, with `macroinstrings` off and `thinktime`, `until`, `loop`, `switch`,
  `case`, `default` and `break` on), and `gmqcc` (`ifvector`, `vectorlogic`, `ssp`,
  `attributes`, `assumevar`, `subscope`, `cpriority`, `allowuninit`, `reciprocal-math`,
  `undefwordsize` and `lo` on, `boundchecks` and `macroinstrings` off, `asm`, `inout`,
  `optional`, `state`, `inline`, `nosave`, `extern`, `shared`, `unused`, `used`, `nonstatic`,
  `ignore`, `strip`, `accessor`, `class`, `vector`, `entity`, `float`, `string`, `int`,
  `integer`, `struct`, `union`, `enumflags`, `thinktime`, `until`, `loop`, `wrap` and `weak`
  off and `enum`, `break`, `continue`, `for`, `goto`, `typedef`, `switch`, `case`, `default`,
  `const`, `var`, `static` and `noref` on; and F208, F319, Q205, F312, F307, F320, F322, F302,
  F321, `octal`, Q208, F211 and F333 ignored and Q106 an error); any other is "Unrecognised std
  parameter", the flags already at their defaults. The macros `-std=` defines for the C dialects
  (`__STDC_HOSTED__`, `__STDC_VERSION__` and the rest) are not read, as the profile reads no
  `-D` of the command line: a module's defines are the project graph's (PRJ-5.4). The module is
  compiled as `fteqcc -Tid` with these options after it (PRJ-3.4), so its target is the
  standard one, which the `-h2`, `-fte`, `-fteh2` and `-dp` that `QCC_SetDefaultProperties`
  reads before the options cannot change, and then that of each `-TNAME` that names one of
  `targets[]` (`QCC_OPCodeSetTargetName`, FTEF-12.11), in order, `-Tparse` none. The `/W`, `/F`
  and `/K` spellings FTEQCC reads only on Windows (`WINDOWSARG`) are not read: the pinned
  oracle is not built for Windows.
- **FTEF-14.73** Under `-Fcaseinsens`, which only the command line sets (FTEF-14.72), FTEQCC
  searches its macro table in any case: `pHash_Get = &Hash_GetInsensitive;` and
  `QCC_PR_CheckCompConst`, `QCC_PR_DefineName` and `QCC_PR_UndefineName`
  look a name up with `pHash_Get(&compconstantstable, name)`. So after `#define Foo 5` a use of
  `foo` or `FOO`, `#ifdef foo`, `#ifndef foo`, `#if defined(foo)`, `#if foo == 5` and a macro
  whose body names `foo` find it, and without `-Fcaseinsens` they do not; a definition in another
  case finds the old macro (`oldcnst = pHash_Get(&compconstantstable, pr_token);`) and replaces
  it. `#undef` finds the macro the same way, with no "was not defined" warning, but removes it
  with `Hash_Remove(&compconstantstable, name)`, which hashes the name as written and compares it
  exactly: `#undef foo` of a macro defined as `Foo` leaves it defined, and `#undef` of a name
  with an upper case letter, or a redefinition of a macro that was defined with one, looks in a
  bucket the macro was not added to, which the pinned FTEQCC reads when it is empty and ends
  with a segmentation fault (measured on macOS arm64, the differential's host). This profile
  removes a macro only when the `#undef` names its spelling, finds it in any case, and never
  ends; the differential holds no program that makes FTEQCC read an empty bucket. The macros
  FTEQCC defines itself, `FTEQCC` and `__FTEQCC__`, are added before the search is switched, to
  the buckets of the name as written, so under `-Fcaseinsens` `#ifdef FTEQCC` is false, as is
  `#ifdef fteqcc` (measured); this profile finds no built-in macro there.
- **FTEF-14.75** The semantic model (`Db::analyze_module`) gives the preprocessor each file it
  compiles at the path the database holds it by and named by the name its input holds for it,
  which for an FTEQCC module's graph (`ModuleInput::from_graph`) is the graph's name (PRJ-6.7),
  and the preprocessor opens each name an `#include` is joined to (FTEF-14.40) as the file of
  that name in the input's directory, the graph's (PRJ-6.8). So
  `__FILE__` (FTEF-14.74) in a listed file and in each file it includes is the name FTEQCC gives
  it, compiled in the source's directory: with `progs.src` listing `src/a.qc` it binds
  `void() f = :__FILE__;` to `src/a.qc` in that file, to `src/x/b.qh` in the `x/b.qh` it
  includes, to `src/c.qh` in a `c.qh` that one includes, to `src/y/c.qh` after `#pragma
  includedir y`, to `src/./b.qh` in a `b.qh` that `src/./a.qc` includes, and to `inc/c.qh` in a
  `c.qh` found with `-Iinc` or `-Iinc/`; with it listing `sub\top.qc`, to `sub`, a tab and
  `op.qc`, the `\t` an escape (FTEF-14.74), and in a `tab.qh` that file includes, to `sub`, a
  tab and `ab.qh`. A file with no name in the input is named by its path. The
  named projects of `cargo xtask oracles builtins` (`NAMED_PROJECTS` and `NAMED_DIR_PROJECTS` in
  `xtask/src/oracles/builtins/directives.rs`) hold these cases, the profile reading every
  FTEQCC case with no `progs.src` of its own through its graph in a directory of its own
  (`graph_input`), and the run of 2026-10-22 (macOS arm64) bound each as the pinned FTEQCC does.

## FTEF-15 Declarations

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs`, `QCC_PR_ParseInitializerType_Internal` and
`QCC_PR_GenerateInitializerType`; `qcc_pr_lex.c`, `QCC_PR_CheckKeyword`, `QCC_PR_ParseType` and
`QCC_PR_ParseFunctionType`; `qcc.h`, `MAX_EXTRA_PARMS`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset: the qualifiers of
`dialect-fteqcc-rerelease.md` FTE-10 to FTE-12, the C-style declarations of FTE-17, the
parameters of FTE-18.

FTEQCC knows every type name when it reads a declaration; this parse-and-resolve profile reads
the file before names are known, so it decides by the shape of the tokens. The corpus starts
declarations with qualifiers (`noref`, `__wrap`, `__deprecated("...")`, `static`), returns
user types and pointers from functions (`jsonnode(string) json_parse`, `entity*(...)`), and
declares parameters without names or with `optional` and `__out`.

### Behaviour

- **FTEF-15.1** A declaration may start with qualifiers, read by the loop at the start of
  `QCC_PR_ParseDefs`: `extern`, `static`, `shared`, `const`, `var`, `nonstatic`, `noref`,
  `nosave`, `strip`, `inline`, `wrap` and `weak`, and also, written with a leading `__`,
  `auto`, `register`, `unused`, `used`, `ignore`, `accumulate` and `deprecated`, which may take
  a `("reason")`. `QCC_PR_CheckKeyword` matches any keyword written with `__` whether or not it
  is enabled ("lets just always go insensitive with a leading underscore pair"). Then comes a
  type. At file level a name that is no statement word starts a declaration when a name, a `*`
  or a parameter list follows it, since a type name is followed by its declarator; a name alone
  is still "not a type" (FTE-21.1).
- **FTEF-15.2** A function's return type may be any type: `QCC_PR_ParseType` ends with
  `if (flag_qcfuncs && QCC_PR_CheckToken ("("))` after any type, a user type or a pointer
  included. A function may be initialised with a value as well as a body or a builtin, as
  `QCC_PR_ParseInitializerType_Internal` does with
  `tmp = QCC_PR_Expression(TOP_PRIORITY, EXPR_DISALLOW_COMMA);`: another function, or `0`.
- **FTEF-15.3** A parameter (`QCC_PR_ParseFunctionType`) has optional qualifiers `optional`,
  `__in`, `__out` and `__inout`; a type; an optional `&` ("accept c++ syntax"); then a name, a
  function pointer `(*name)(params)`, or nothing, since `else if (pr_token_type == tt_name) name =
  QCC_PR_ParseName ();` takes a name only when there is one; array suffixes; and a default value.
  A lone unnamed `void` is no parameter (`else if (paramlist[numparms].type->type == ev_void)
  break; //float(void) has no actual args`). `...` ends the list, after a type or alone, and may be followed by the name of the argument count. A `*` after a function type makes a pointer to it, `void(void() *a) h = #6;`.
- **FTEF-15.4** A function type has at most `MAX_PARMS+MAX_EXTRA_PARMS` parameters ("Too many
  parameters. Sorry. (limit is %i)"), with `#define MAX_EXTRA_PARMS 128` and id's
  `MAX_PARMS` of 8 (`LIMIT_MAX_PARMS`, QC-27.7): 136 under this profile, not 8.
- **FTEF-15.5** `wrap`, `weak` and `accumulate` let a function be defined again: a `wrap`
  definition replaces the one before it and may call it as `prior` (`PIF_WRAP`, "wrapper function
  does not wrap anything" when there is none), a `weak` one gives way to a later definition, and
  an `accumulate` one is extended by later ones. `extern` says the body is elsewhere ("function
  is external, don't error or warn if the body was not found"). A `var` function-typed name is a
  variable that holds a function, not a function to define: `QCC_PR_ParseDefs` makes
  "functions and fields ... always ... consts even when not initialised", unless `var`.
- **FTEF-15.6** `T a, *b` declares `b` a pointer, and `struct { ... } a[4], *p;` declares
  variables of an unnamed struct, as C does; each `*` before a declarator's name is a level of
  pointer (FTEF-4.1).
- **FTEF-15.7** A name is "name too long" (`E0202`) from 255 bytes, not id's 63 (QC-3.3):
  `QCC_PR_ParseName` (`qcc_pr_lex.c`) reports `if (strlen(pr_token) >= MAX_NAME-1)
  QCC_PR_ParseError (ERR_NAMETOOLONG, "name too long");` with `#define MAX_NAME 256` in
  `qcc.h`. So a name of 254 bytes is accepted in a declaration, after `#number:`, in an
  expression and after `::`, and one of 255 is the error; the pinned FTEQCC, built by
  `cargo xtask oracles build`, gives the same for `float n...;`, `void() f = #5:n...;` and
  `x = n...;` in a body, at 254 and 255 bytes. The name after a lone `:` is no
  `QCC_PR_ParseName` (FTE-23.6), so it has no limit. The vanilla dialect keeps QC-3.3.
- **FTEF-15.8** A declaration's declarators may end with a `,` before its `;`:
  `QCC_PR_ParseDefs` (`qcc_pr_comp.c`) reads them in `do { ... } while (QCC_PR_CheckToken
  (","));`, each starting with `if (QCC_PR_CheckToken (";"))`, which ends the declaration. So
  `void() f = #5, ;` and `void() g, f = #5,;` declare `f` the builtin 5, as the pinned FTEQCC
  compiles them, and so does `#define E(...) #5 __VA_ARGS__` used as `E(, )` (FTEF-14.26);
  `void() f = #5, , ;` is refused, a `,` being no name. The vanilla parser is unchanged.
- **FTEF-15.9** With `qcfuncs` on, after a function type, `(name)` followed by `;`, `=`, `,` or
  `[`, `name` no type, is C's parenthesised name: `QCC_PR_ParseDefs` tries
  `QCC_PR_ParseFunctionType` after the `(`, which finds no type there, and its C-style branch
  reads the name with no `*` (`isvar |= isptr>0`, so no variable). So `typedef void() (fn);` and
  `typedef float() (fn);` define `fn` a function type, `void() (g) = {};` defines `g`, and
  `void() (g), (h);` declares two functions with no body. With `qcfuncs` off the list is a QC
  parameter list and the declarator has no name, `";" - not a name`, as it has for a type's name
  (`typedef float t; void() (t);`).

- **FTEF-15.10** A parameter whose type writes a struct, union or enum (`void(union { float a;
  vector v; } u) f`, `void(enum { EA } e) f`) has that type, read with the type
  (`QCC_PR_ParseType`), so the function's type prints it (`void(union <union> u)`) and the
  parameter is a value of it in the body.
## FTEF-16 Statements and expressions

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement`, `QCC_PR_RefTerm`, `QCC_PR_ParseRefValue`
(the `[` vector and `::` branches), `QCC_PR_RefExpression`; `qcc_pr_lex.c`,
`QCC_PR_LexString`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset:
`dialect-fteqcc-rerelease.md` FTE-19 to FTE-21.

The corpus writes locals of class and pointer types, casts to classes (`(ncPlayer)self`),
`[x, y, z]` vectors, scoped names (`ncTimer::_TimerThink`, `super::Save`, `::classname`),
`if not (...)`, members of what a call returns, and `case` labels inside nested blocks.

### Behaviour

- **FTEF-16.1** A statement declares locals when it starts with a qualifier (FTEF-15.1), a C
  type word (FTEF-2), a `.`, or a name followed by a name, by `*`s, a name and a `;`, `=`, `,` or
  `[`, or by a parameter list and a name. FTEQCC decides by `QCC_TypeForName`; no expression
  statement has these shapes, except a product `a * b` alone, which this profile reads as an
  expression unless a declarator's end follows.
- **FTEF-16.2** `(type) e` is a cast to any type: `QCC_PR_RefTerm` reads `(`, then
  `newtype = QCC_PR_ParseType(false, true, false)`, and, after `)`,
  `e = QCC_PR_Expression (UNARY_PRIORITY, EXPR_DISALLOW_COMMA);` or, before `{`, an initialiser
  of the type. This profile takes a cast where `(` holds a type word, a `.` or a C type word; or
  a name and `*`s or a function type's parameter list, closed by `)` and followed by a term; or a
  lone name in parentheses followed by a term. `(visible(head))` and `(sin(x))` stay
  parenthesised calls.
- **FTEF-16.3** `[x, y, z]` builds a vector of three expressions, the missing ones 0 ("and to not
  need the 2nd/3rd parts if you're lazy - [5] or [5,6]"); a fourth is an error, since
  `QCC_PR_Expect("]")` follows the third.
- **FTEF-16.4** `Class::member` names a member of a class or its parents, `super::member` one of
  the parent of the class whose member function it is, and `::name` a name of the global scope
  (`if (QCC_PR_CheckToken("::")) { assumeclass = NULL;`).
- **FTEF-16.5** `if not (e)` takes the statement when `e` is false:
  `int negate = QCC_PR_CheckKeyword(keyword_not, "not")/*hexenc*/;`.
- **FTEF-16.6** A call may be followed by a subscript, a member or another call; in vanilla a
  call ends the term (QC-15.8).
- **FTEF-16.7** A term may start with `++` or `--` (`QCC_PR_RefTerm`'s `preinc`), `&` (an
  address, FTEF-4.2) or `*` (a dereference, FTEF-4.4); the operand is a term with its postfix
  forms.
- **FTEF-16.8** Adjacent string literals are one string: `QCC_PR_LexString` reads strings in a
  loop that skips whitespace and comments and ends only where no `"` follows
  (`else if (first) ... else break;`).
- **FTEF-16.9** A clause of a `for` header may hold several expressions separated by `,`, and the first clause a declaration after them (`v = 0, int i = 1`). `QCC_PR_ParseStatement_For` reads the first clause with `type = QCC_PR_ParseType (false, true, false)`, so any type starts a declaration there, `for (accessor A : float i; ; )` and `for (enumflags E { A } i; ; )` among them, which a statement's `accessor` and `enumflags` do not (FTEF-20.14).
- **FTEF-16.10** A call's result may be assigned to, as in the chain `a = f() = b` the corpus
  writes; FTEQCC reports no error there, so neither does this profile.
- **FTEF-16.11** An aggregate initialiser may name the member each value initialises,
  `{ .name = value, ... }`.
- **FTEF-16.12** A `case` or `default` label may stand in a block nested in the `switch` body,
  as C allows; the corpus does so (`client/viewmodel.qc`).
- **FTEF-16.13** Any expression may stand left of an assignment operator: `QCC_PR_RefExpression`
  reads the left side as a term whatever it is, and an assignment to a value that is no
  variable is checked, not refused by the parser (FTEF-29.1). `~a = 1`, `1 = a` and
  `(a = 2) = 3` each give the two "Assignment to const" warnings, not a syntax error (the
  pinned FTEQCC, 2026-10-27), so this profile's parser reports none; vanilla's E0508
  (QC-27.10) stays.

- **FTEF-16.14** `local` starts no declaration in a `for` header: `QCC_PR_ParseStatement_For`
  reads the first clause with `QCC_PR_ParseType (false, true, false)`, which takes no `local`, so
  the word is a name there, made a `variant` by `QCC_PR_GetDef` with the warning `W0221` "\"local\"
  keyword used as variable name - coexisting" and then "Unknown value \"local\"." (FTEF-19.45),
  and the type word after it is "expected ;, found float". The warning is given for any unknown
  name that is a keyword in force (FTEF-20.4).
- **FTEF-16.15** `(NAME)` where `QCC_PR_ParseType` finds no type of the name is no cast but the
  value in parentheses, an unknown name a `variant` (FTEF-19.69), and what this profile's parser
  read as the cast's operand follows it (FTEF-16.2): `- e`, and a negative number, which the
  lexer splits after a value (`QCC_PR_IncludeChunk`), is a subtraction, `(x) -1` as `x - 1`;
  `(args)` calls the value, "not a function" of a `float`; `[i]` is an index the value does not
  take (FTEF-18.37, FTEF-18.38); and a number or a name is "Expected punctuation"
  (`WARN_UNEXPECTEDPUNCT`), then "expected ;, found TOKEN", or in a definition's value the end
  of the value, "Missing semicolon at end of definition" and at global scope "\"TOKEN\" is not a
  type". So `(x)[1] = 2;` of a `float x` is "Statement does not do anything" and "expected ;,
  found [", `y = (x) 1;` "Expected punctuation" and "expected ;, found 1", and no `(x)` is
  "\"x\" is not a type". Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_RefTerm`, `QCC_PR_RefExpression`.
## FTEF-17 Name resolution

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseRefValue` (`assumeclass`, `this`, `super`, the
intrinsics), `QCC_PR_ParseInitializerType_Internal`, `QCC_PR_GenerateInitializerType` (the "dupe
initialisation" comparison), `QCC_PR_ParseEnum`; `qcc_pr_lex.c`, `QCC_PR_ParseEntClass`;
`qccmain.c`, the bodyless-function check of `QCC_WriteData`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset:
`dialect-fteqcc-rerelease.md` FTE-22.

### Behaviour

- **FTEF-17.1** A class, struct, union, typedef, enum or accessor names a type, and its name may
  be used where a value is (`spawn(ncTimer)`, `sizeof(graphNode_t)`). A type's name may be
  declared again: a class forward and then with its body, a typedef in two headers.
- **FTEF-17.2** **Withdrawn** (P2-133, #574): it said a name in a member function's body is found among the members of its class, but `QCC_PR_ParseValue` reaches a field as `this.field` through the global `self`, without which FTEQCC ends; replaced by FTEF-17.14.
- **FTEF-17.3** The name after `.` or `->` is a member of the class of what the access is made
  on, when that is known from a name's declared type, `this`, a cast or a call's return type;
  else an entity field the name defines; else a member of any class with the name. A vector's
  `x`, `y` and `z`, `.length`, and a field of a struct or accessor name no definition, and a
  name that is a value but no field is not checked; any other name is "Unknown value" (QCD-4.1).
- **FTEF-17.4** `this`, `super` and, in a `wrap` function, `prior` are no names a program
  defines, and neither are the intrinsics `QCC_PR_ParseRefValue` declares on demand ("intrinsics,
  any old function with no args will do"): `nil`, `randomv`, `alloca`, `entnum`, `autocvar`,
  `used_model`, `used_sound`, `va_arg` and `_`, with `sizeof` and `_length`. `autocvar(name,
  default)` takes the cvar's name, which is no value, and `sizeof` may take a type word.
- **FTEF-17.5** **Withdrawn** 2026-10-20 (P2-70): it compared a builtin by its number alone,
  but FTEQCC reuses a builtin function only for the same number and engine name; replaced by
  FTEF-17.10.
- **FTEF-17.10** A name initialised again is accepted when the "dupe initialisation"
  comparison finds the two values equal word by word: this profile compares two literals by
  value (FTE-22.6), a builtin by its number and engine name as FTE-23.17 says, any other
  initialiser, a builtin FTEQCC binds no number for among them, by its text, token for token.
  An enum's names are never redeclared: `QCC_PR_ParseEnum` makes each with
  `QCC_PR_DummyDef`, which looks for no earlier definition. A function body is never equal to
  another.
- **FTEF-17.6** **Withdrawn** (P2-117, #545): it took every type name for one whose meaning the
  model does not follow, but a typedef's name is the type it names (`QCC_TypeForName`,
  FTEF-19.9). Replaced by FTEF-17.12.
- **FTEF-17.7** **Withdrawn** (P2-84, #495): it said a function with no body is an error only
  where the module refers to it, but the check's `d->unused` is the `noref` mark, so FTEQCC
  reports every bodyless function that is not both `noref` and unreferenced. Replaced by
  FTEF-17.11.
- **FTEF-17.8** A definition marked `noref` or `unused`, by its qualifier or a `#pragma noref 1`
  before it (FTEF-12.6), may be initialised again with another value: the "dupe
  initialisation" comparison runs only `if (basedef->initialized && !basedef->unused ...)`.
  The corpus's `fteextensions.qc` sets `#pragma noref 1` and a later file binds
  `getgamedirinfo = #0` again.
- **FTEF-17.9** A function that has a body, given a builtin number again, keeps its body, with
  the warning `W0206` "incompatible redeclaration. Ignoring replacement of qc function with
  builtin.": the "dupe initialisation" comparison of `QCC_PR_GenerateInitializerType` has
  `else if (!def.sym->arraysize && def.cast->type == ev_function &&
  functions[QCC_SRef_DataWord(def, i)->_int].code>-1 &&
  functions[QCC_SRef_DataWord(tmp, i)->_int].code==-1)` before its "incompatible
  redeclaration" error. As FTEF-17.8, the comparison is not made for a definition marked
  `noref`. Copper's client module defines `sin` in `maths.qc` and then reads
  `float(float angle) sin = #60;` in `csdefs.qc`.
- **FTEF-17.11** A function with no body is an error ("function %s has no body", `E0211` here)
  unless it is marked `noref`, by its qualifier or a `#pragma noref 1` (FTEF-12.6), and the module
  does not refer to it: FTEQCC's check (`qccmain.c`) skips a definition only `if (d->unused &&
  !d->used)`, `unused` being the `noref` mark, before `QCC_PR_Warning(ERR_NOFUNC, d->filen,
  d->s_line, "function %s has no body",d->name);`. So `void() f = #5;` then `void() g;` is an
  error, as are `void() f = #5, g;` and `#define V(a, ...) a,##__VA_ARGS__` used as `V(#5, g)`,
  and the pinned FTEQCC refuses each; `noref void() g;` compiles, and a `noref` function that is
  called is an error. A member function is not checked, and a class's `spawnfunc_Name`
  (FTEF-9.5), whose body FTEQCC writes when the program does not, is never undefined.
- **FTEF-17.12** **Withdrawn** (P2-132, #569): it said a pointer is a type the model does not
  follow, but FTEQCC compares pointers by what they point to, and `float *a; float a;` is a
  mismatch. Replaced by FTEF-17.13.
- **FTEF-17.13** **Withdrawn** (#84): it said a redeclaration involving a class, struct, union,
  accessor or enum type was never a mismatch; FTEQCC's `typecmp` compares those types too, and
  FTEF-17.21 replaces it.

- **FTEF-17.14** A class's members are found through the class and its parents
  (`QCC_PR_ParseEntClass` keeps each class's members, with `newt->parentclass`). `Class::member`
  defines the member the class or a parent declares, or adds one, and its name refers to the
  member's declaration. In a member function's body a name is the function's own local or parameter, then a member of its class or a parent, then a global. A field, a `virtual` function among them, is read as `this.field` (`if (pr_classtype && expandmemberfields && d.cast->type == ev_field)`), `this` being made from the global `self` (`QCC_PR_DummyDef(pr_classtype, "this", pr_scope, 0, QCC_PR_GetDef(NULL, "self", NULL, true, 0, false), ...)`), so `entity self; class C { float a; void() m = { a = 1; }; };` and `void() C::m = { a = 1; };` after it compile; with no `self` defined FTEQCC ends in that `QCC_PR_GetDef` (the oracle records the program as refused), which this profile reports as "Unknown value" at the name. `this.a = 1;` needs no `self`, and `m()` calls a nonvirtual `m`. A
  class's own name there is the class, as `spawn(Thing)` takes it, not its constructor. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseValue`.
- **FTEF-17.16** A member function written with its body inside its class is compiled where it
  stands, so a member the class declares after it is not there yet: `n();` is "Unknown value
  \"n\"." and `this.n` or `this.a` "Unknown field \"n\" in class \"C\"" for a later `n` or `a`,
  which a body written after the class sees. `this` there is the entity as the class's type
  (FTEF-17.14), so `local float f = this;` is "Cannot cast from class C to float".
- **FTEF-17.15** `TYPE Class::member` of a member the class or a parent declares is
  `QCC_PR_GetDef` of `Class::member` with TYPE, so a TYPE that `typecmp` tells from the
  member's is the error `E0205` "Type mismatch on redeclaration of Class::member. TYPE, should
  be MEMBERTYPE", and the definition is not read on. Parameter names do not count, a class is
  told from the entity and another class, and a typedef is its type: `void() C::m` and
  `float(float x) C::m` for `void(float x) m` are refused, `void(float y) C::m` is not. A
  constructor declared `C();` is of type `void()` (`QCC_PR_GenFunctionType(type_void, ...)`),
  so `C() C::C = {};` is "C(), should be void()" and `void() C::C = {};` defines it. The
  types are named as `TypeName` names them, a class as `class NAME`, inside a function's
  parameters too.
- **FTEF-17.17** `autocvar(name, default[, "description"])` is the global `autocvar_name`,
  made by the call with the default's type and value (`QCC_PR_GetSRef(e.cast, autocvarname, NULL,
  true, 0, GDF_USED)`), so the call is a value of that type: `string s = autocvar(sv_x, 1);` is
  "Cannot cast from float to string". A default that is no constant is the error `E0367`
  "autocvar default value is not constant", and one that differs from the value the global was
  given before, by its definition or an earlier call, is the fatal error `E0368` "autocvar NAME
  was already initialised with another value". The global is a definition as any other: the
  rest of the module refers to it, a global `autocvar_name` of another type is "Type mismatch on
  redeclaration", before the call or after it, and a later one given another value is
  "incompatible redeclaration". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseFunctionCall`.
- **FTEF-17.18** In a member function, `self` not after `::` is `this`, the function's class:
  `QCC_PR_ParseValue` warns `WARN_SELFNOTTHIS` (`F331`, off by default) "'self' used inside OO
  function, use 'this'." and reads `this` in its place, so `self.x` is the class's member `x`.
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseValue`.
- **FTEF-17.19** FTEQCC keeps a list of the sounds, models, textures and files the program
  names: the first argument of a `precache_sound`, `precache_model`, `precache_texture` or
  `precache_file` function (any name with that prefix), the second of `setmodel` and
  `localsound`, the third of `sound`, and the argument of `used_sound` and `used_model`, each a
  string immediate or a constant string. A path that is new to its list and has a backslash
  warns "backslashes in path names are non-portable - PATH", with the path's escapes read, and
  no command-line flag turns it off; a path the list has already warns no more. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PrecacheSound`, `QCC_PrecacheModel`, `QCC_SetModel`, `QCC_SoundUsed`,
  `QCC_PrecacheTexture`, `QCC_PrecacheFile`, `QCC_PR_ParseFunctionCall`.
- **FTEF-17.20** A call of a function named `spawn` that takes no parameters is
  `spawn(CLASS, field: value, ...)` or `spawn(e, CLASS, ...)`, which is a value of the class
  (`result.cast = rettype`); `spawn()` is a call as any other. `QCC_PR_ParseType` reads the class,
  else an expression, then a `,` ("expected ,, found )" when the call ends) and the class; a name
  there that is no type is the fatal error `E0373` "Spawn operator with undefined class: NAME",
  and a type that is no entity's ends at the token after it, `")" - not a name`. Each argument
  after the class is a field, `QCC_PR_ParseValue(rettype, ...)`: a member of the class or its
  parents, else a definition of the name; a name the class has no member of and nothing defines is
  "Unknown field NAME in class CLASS", and anything that is no field is the fatal error `E0374`
  "Named field is not a field.", a positional value among them. The field's name is followed by
  `:`, or by `=` with the warning `W0318` "That = should be a :", and then its value, an
  expression of any type. The parser reads such an argument, a name and `:` or `=` after
  `spawn`'s first argument, as a `NamedArg` node. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseFunctionCall`.
- **FTEF-17.21** A redeclaration's type is the earlier one's by `typecmp`, with each typedef's
  name read as the type it names (`QCC_TypeForName`, FTEF-19.9) and a named enum that is no
  `enum class` as its base type: a class, struct, union or accessor is its own type, the same as
  another only by name, so `C c; entity c;`, `void(entity e) f; void(C e) f;` and two classes'
  parameters are "Type mismatch on redeclaration", and an `enum E` and a `float` are one type.
  `__bint` and `__bfloat` are one type, and no other (FTEF-23.8). Failing that, `typecmp_lax`
  decides between a warning and the error (FTEF-19.7): an accessor is its base there, so
  `accessor box : float; box b; float b;` is "Optional arguments differ on redeclaration of b.
  float, should be box", but a parameter's kind is compared before its contents, `ev_accessor`
  being no other kind, so `float(float h) g` redeclared as `float(box this)` is a mismatch. A
  pointer is compared with a pointer by what each points to and is never the type of a
  non-pointer: `float *a; float a;` is "Type mismatch on redeclaration of a. float, should be
  float*" (`E0205`), `float *a; float *a;` none, and `float *a; string *a;` one. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `typecmp`, `typecmp_lax`; `engine/qclib/qcc_pr_comp.c`, `QCC_PR_GetDef`.

## FTEF-18 Further statements, terms and frame macros

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseInitializerType_Internal`,
`QCC_PR_GenerateInitializerType`, `QCC_PR_ParseState`, `QCC_PR_ParseStatement`,
`QCC_PR_RefExpression`, `QCC_PR_RefTerm`, `QCC_PR_ParseDefs`, `QCC_PR_DiscardExpression`,
`QCC_PR_DiscardRef`, `QCC_PR_ParseImmediateStatements`, `QCC_TryEvaluateCast`, `QCC_Include`;
`qcc_pr_lex.c`, `QCC_PR_LexMacroName`, `QCC_PR_LexGrab`, `QCC_PR_CheckKeyword`,
`QCC_PR_MacroFrame`, `QCC_PR_FindMacro`, `QCC_PR_ExpandMacro`, `QCC_PR_IncludeChunkEx`; `qccmain.c`, the keyword
table and `QCC_SetDefaultProperties` (every warning on, `qccwarningaction[i] = WA_WARN;`, but
those it lists),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Rerelease subset:
`dialect-fteqcc-rerelease.md` FTE-8, FTE-10 and FTE-21.

The rest of what the corpus and FTEQCC's own tests write: initialisers that are expressions,
functions defined inside functions, state frames computed from expressions, the statement forms
FTEQCC takes from FrikQCC and HexenC, `sizeof`, and the frame-macro forms of `QCC_PR_LexGrab`.
Copper and Arcane Dimensions, which build with FTEQCC from list-form `progs.src` files, use the
frame macro forms of FTEF-18.13, FTEF-18.16 and FTEF-18.17 and the statement sequences of
FTEF-18.5.

### Behaviour

- **FTEF-18.1** A definition's initialiser may be any expression: past its special cases
  `QCC_PR_ParseInitializerType_Internal` reads
  `tmp = QCC_PR_Expression(TOP_PRIORITY, EXPR_DISALLOW_COMMA); tmp = QCC_EvaluateCast(tmp, type,
  true);`, so `float IT_ALL = IT_AXE | IT_SHOTGUN;` is a global with a folded value. A global's
  value must still be constant: `QCC_PR_GenerateInitializerType` reports "initializer for '%s'
  is not constant" when `!tmp.sym->constant`. This parse-and-resolve profile parses any
  expression there and checks only the names in it (FTE-10.2, P2-34); whether it folds is
  checked with constant folding (P2-06).
- **FTEF-18.2** A function-typed local may be given a body inside a function body: the `if
  (pr_scope)` branch of `QCC_PR_ParseInitializerType_Internal` saves the outer function's
  locals, calls `QCC_PR_ParseImmediateStatements` for the inner body ("generate a goto statement
  around the nested function, so that nothing is hurt"), and restores them, so the inner
  function is a function of its own, `void() g = { ... };` with or without `local`.
- **FTEF-18.3** A state's frame and next function are expressions: `QCC_PR_ParseState` reads
  `s1 = QCC_PR_Expression (TOP_PRIORITY, EXPR_DISALLOW_COMMA);` for the frame, converted to
  float, and then `def = QCC_PR_Expression (TOP_PRIORITY, EXPR_DISALLOW_COMMA);` for the next
  function, which is "Type mismatch" unless it is a function or null. A missing `,` between
  them is the warning "missing comma in state definition" (`WARN_UNEXPECTEDPUNCT`), not an
  error.
- **FTEF-18.4** **Withdrawn** (P2-39, #423): it called a constant range against its
  direction a warning, but FTEQCC raises it with `QCC_PR_ParseWarning(ERR_STATETYPEMISMATCH,
  ...)`, and `QCC_PR_PrintWarning` prints any code `if (type >= ERR_PARSEERRORS)` as an error
  and counts it in `pr_error_count`. Replaced by FTEF-18.19.
- **FTEF-18.5** An expression statement may hold several expressions separated by `,`, each
  evaluated and the last one kept: `QCC_PR_ParseStatement` ends with
  `QCC_PR_DiscardExpression (TOP_PRIORITY, 0);`, and `QCC_PR_RefExpression` continues
  `if (!(exprflags&EXPR_DISALLOW_COMMA) && priority == TOP_PRIORITY && QCC_PR_CheckToken
  (","))`, warning "Statement does not do anything" when the expression before the `,` has no
  effect. Initialisers, arguments, state parts and `case` values pass `EXPR_DISALLOW_COMMA`, so
  a `,` there separates rather than sequences.
- **FTEF-18.6** **Withdrawn** (P2-42, #429): it gave a function defined inside another labels
  of its own, but FTEQCC shares one list between them and clears it at the end of either.
  Replaced by FTEF-18.23 and FTEF-18.24.
- **FTEF-18.7** `return = e;` sets the value the function returns without returning: "return =
  foo; sets the value that will be returned when the function finally exits, without returning
  now. return; returns that value now". A return type larger than a vector is the error "'%s'
  not supported when returning %s".
- **FTEF-18.8** `if !(e)` takes the statement when `e` is false, like `if not (e)`
  (FTEF-16.5): `if (!negate && QCC_PR_CheckToken("!"))` sets `negate = 2`, with the warning
  "if !() is specific to fteqcc" (`WARN_FTE_SPECIFIC`).
- **FTEF-18.9** **Withdrawn** (P2-119, #554): it said `until` is a keyword only for the Hexen II
  targets, but a `#pragma keyword` or a `-K` option turns it on too. Replaced by FTEF-18.31.
- **FTEF-18.10** A `case` label may give a range, `case a .. b:`, matched when the value is from
  `a` to `b` inclusive: after the first value `QCC_PR_ParseStatement` tries `if (QCC_PR_CheckToken(".."))`
  and reads a second with `QCC_PR_RefExpression (&pr_casesref2[num_cases], TOP_PRIORITY,
  EXPR_DISALLOW_COMMA);`, and for a float the switch tests `OP_GE_F` against `a` and `OP_LE_F`
  against `b`.
- **FTEF-18.11** `a ?: b` is `a` when `a` is true and `b` otherwise, `a` evaluated once: after
  `if (priority == TERNARY_PRIORITY && QCC_PR_CheckToken ("?"))` the conditional tries
  `if (QCC_PR_CheckToken(":"))` before reading a middle operand.
- **FTEF-18.12** `sizeof` takes a type or an expression, with or without parentheses:
  `QCC_PR_RefTerm` reads `pbool bracket = QCC_PR_CheckToken("("); t =
  QCC_PR_ParseType(false, true, false);`, and when no type follows,
  `r = QCC_PR_RefExpression(&refbuf, TOP_PRIORITY, 0); //formulas are accepted weirdly enough,
  its not just terms.` The value is the size in bytes, an unsigned integer constant; for a
  string literal it counts the terminating NUL.
- **FTEF-18.13** A frame macro reference (`$name`, QC-8.5) ends at whitespace or at the first
  `,`, `;`, `&`, `|`, `(`, `)`, `]` or `..`: `QCC_PR_LexMacroName` copies bytes `while ( (c =
  *pr_file_p) > ' ' && c != '\n' && c != ',' && c != ';' && c != '&' && c != '|' && c != ')' &&
  c != '(' && c != ']' && !(pr_file_p[0] == '.' && pr_file_p[1] == '.'))`, where id's qcc stops
  only at whitespace, `,` and `;` (QC-8.6). So `$a||x`, `($b)` and `[$a, th]` name the frames `a` and
  `b`.
- **FTEF-18.14** `$framesave` defines frame names exactly as `$frame` does (QC-8.2):
  `QCC_PR_LexGrab` takes `if (!STRCMP (pr_token, "frame") || !STRCMP (pr_token, "framesave"))`
  to `QCC_PR_ParseFrame`, which numbers each name on the line from the running frame count.
- **FTEF-18.15** A function type may return a function: after the type, `QCC_PR_ParseDefs`
  loops `while(QCC_PR_CheckToken ("("))` over parameter lists ("qc-style function... possibly
  returning a function."), so `float(float)(string) f;` declares `f` as a function taking a
  string and returning a `float(float)` function.
- **FTEF-18.16** Besides `$frame` and `$framesave`, `QCC_PR_LexGrab` knows these `$` commands,
  each followed by words read as frame macro names are (FTEF-18.13). `$cd`, `$origin`, `$base`,
  `$flags`, `$scale` and `$skin` are ignored with their words: `while (QCC_PR_LexMacroName ())
  ;`, which stops at the end of the line or at the first word that ends at once, where id's qcc
  drops the rest of the line (QC-27.14). `$flush` numbers the next frame name 0
  (`QCC_PR_ClearGrabMacros(true)`); `$frame_reset` also forgets every frame macro
  (`QCC_PR_ClearGrabMacros(false)`, "full reset of all frame macros"); `$framevalue N` numbers
  the next one `N` (`pr_macrovalue = atoi(pr_token);`); `$framerestore NAME` numbers it as the
  macro `NAME` is, which must exist ("Unknown frame macro"); and `$modelname NAME` first makes
  the previous model name a frame macro of the current number
  (`QCC_PR_MacroFrame(pr_framemodelname, pr_macrovalue, true)`), then takes `NAME` as the model
  name and, when `i = QCC_PR_FindMacro(pr_framemodelname)` is not 0, numbers the next frame
  `i`, which is -1 for a name with no macro. Under the vanilla profile each of these is a frame
  macro reference (QC-8.5). Copper's `m_knight.qc` begins with `$flush`.
- **FTEF-18.17** Frame macros are kept from file to file (FTE-22.4), and the number of the next
  frame name, `pr_macrovalue`, starts at 0 in each file (`QCC_PR_CompileFile` calls
  `QCC_PR_ClearGrabMacros (true);`) and goes up by one for each name (`QCC_PR_MacroFrame(pr_token,
  pr_macrovalue++, false)`), a name already defined in the same file keeping its first number.
  The limit is on the macros of the whole program, not of one file: with
  `#define MAX_FRAMES 8192`, `QCC_PR_MacroFrame` reports `if (pr_nummacros >= MAX_FRAMES)
  QCC_PR_ParseError(ERR_TOOMANYFRAMEMACROS, "Too many frame macros defined");`, which this
  profile reports as `E0504` (QCD-6.4) at the name that reaches it. Under the vanilla profile
  the limit stays 256 frames in one file (QC-27.5). Arcane Dimensions' `mon_bossmammoth.qc`
  defines more than 256 frames.
- **FTEF-18.18** `..` is one token: `pr_punctuation[]` lists `"...", "..", "."`, longest first.
  So `..T` is a field of a field, which `QCC_PR_ParseType` reads `if (QCC_PR_CheckToken
  (".."))` as two field levels ("so we don't end up with the user specifying '. .vector
  blah'"), and a number ends before `..`: `QCC_PR_LexNumber` takes a fraction only
  `else if (c == '.' && pr_file_p[1]!='.')`, so `1..5` is `1`, `..` and `5`.
- **FTEF-18.19** **Withdrawn** 2026-10-20 (P2-93): it named its form after the withdrawn
  FTEF-18.4; replaced by FTEF-18.28.
- **FTEF-18.20** The sequences of FTEF-18.5 are read wherever FTEQCC reads an expression with
  no `EXPR_DISALLOW_COMMA` at `TOP_PRIORITY`: a returned value and `return =`
  (`e = QCC_PR_Expression (TOP_PRIORITY, 0);`), the conditions of `if`, `while`, `until`, `do`
  and `switch`, a parenthesised expression (`r = QCC_PR_RefExpression(retbuf, TOP_PRIORITY,
  0);` before `QCC_PR_Expect (")")`), a subscript (`tmp = QCC_PR_Expression (TOP_PRIORITY, 0);`
  after `QCC_PR_CheckToken("[")`), the middle of `?:` (`val = QCC_PR_Expression(TOP_PRIORITY,
  0);`) and the expression of `sizeof` (FTEF-18.12). The last arm of `?:` passes
  `EXPR_DISALLOW_COMMA`.
- **FTEF-18.21** A frame macro is looked up by its name and then, when no macro has it, by the
  name in any case: `QCC_PR_FindMacro` searches `for (i=pr_nummacros-1 ; i>=0 ; i--)` with
  `STRCMP` and then again with `stricmp`, the second time with the warning `W0104` "Case
  insensitive frame macro (using %s)" (`WARN_CASEINSENSITIVEFRAMEMACRO`), so the macro defined
  last of those that match is used. The lookups of a reference, `$framerestore` and
  `$modelname` (FTEF-18.16) are all `QCC_PR_FindMacro`'s.
- **FTEF-18.22** **Withdrawn** (P2-94 to P2-99, #507 to #512): it cited the withdrawn FTEF-18.4
  for an error code `QCC_PR_PrintWarning` prints as an error. Replaced by FTEF-18.29, which
  cites FTEF-18.28.
- **FTEF-18.23** A statement may be a label, written `name:` (the C style, taken `if
  (pr_token_type == tt_name && pr_file_p[0] == ':' && pr_file_p[1] != ':')`) or `:name` (the
  FrikQCC style, `if (QCC_PR_CheckToken(":"))`), and `goto name;` jumps to one (`goto` is a
  default keyword in the `qccmain.c` keyword table); a label name that is not a name is
  "invalid label name". FTEQCC keeps one list of labels, `pr_labels`, which the end of every
  function clears (`if (num_labels) num_labels = 0;` in `QCC_PR_ParseImmediateStatements`),
  a function defined inside another (FTEF-18.2) included: that one is read by the same
  `QCC_PR_ParseImmediateStatements` in the middle of the enclosing body, with the note
  "FIXME: make sure gotos/labels/cases/continues/breaks are not broken by this." So a label
  read since the end of the last function is the warning `W0312` "Duplicate label NAME"
  (`QCC_PR_ParseWarning(WARN_DUPLICATELABEL, "Duplicate label %s", pr_token);`) at its name,
  and is not added again: a label of the enclosing function read before an inner function is
  a duplicate for a label of that name in the inner one, and is forgotten after it. The
  preprocessed text this profile parses keeps a name and the `:` after it together when the
  source does, so the C style is found there too.
- **FTEF-18.24** **Withdrawn** (#84): it had the parser check each `goto`'s label at its label
  name; FTEQCC checks the statements generated, so a `goto` a stripped `if` arm took back is not
  checked, and the error is fatal at the function's end. FTEF-34.12 replaces it.
- **FTEF-18.25** FTEQCC's flag `qcc_usefulstatement` says whether the expression being read
  does something: a call sets it (`if (priority == FUNC_PRIORITY && QCC_PR_CheckToken ("(") )
  { qcc_usefulstatement=true;`, before the arguments are read), and so do an assignment of
  any of the operators of FTE-6 and FTEF-16, once its right side is read, a prefix `++` or `--`
  (`QCC_PR_RefTerm`), and a postfix `++` or `--` still pending when the expression is discarded
  (`QCC_PR_DiscardRef`), that is, the expression itself, in parentheses or as the last of a
  sequence. A discarded expression with none of these is the warning `W0313` "Statement does
  not do anything" (`WARN_POINTLESSSTATEMENT`, on by default) at that expression:
  - an expression statement, and the first and last clauses of a `for`: `QCC_PR_DiscardExpression`
    saves the flag, clears it, reads the expression, and warns `if (ref->cast->type != ev_void
    && !qcc_usefulstatement)`, then restores the flag. The first clause's expressions, separated
    by `,`, are each discarded so (`QCC_PR_DiscardExpression(TOP_PRIORITY,
    EXPR_DISALLOW_COMMA);` in a `do ... while(QCC_PR_CheckToken(","))` loop); the last clause is
    one sequence. This profile takes an expression as `void` only when it is a cast to `void`
    (`QCC_TryEvaluateCast`: "anything can be cast to void, but only do it explicitly."), or
    one in parentheses;
  - each expression before the `,` of a sequence (FTEF-18.5, FTEF-18.20), wherever the
    sequence is: `QCC_PR_DiscardRef(lhsr); if (!qcc_usefulstatement)
    QCC_PR_ParseWarning(WARN_POINTLESSSTATEMENT, "Statement does not do anything");
    qcc_usefulstatement = false;`, with no exception for `void`. Outside an expression
    statement and a discarded `for` clause the flag then carries what came before, a call in an
    earlier condition included, since only the discarding and the `,` clear it; this profile
    keeps the flag over one file.
- **FTEF-18.26** **Withdrawn** (P2-130, #567): it counted the operators, casts, `sizeof`, `*`s
  and parameter lists but not an `extern` block, whose `QCC_PR_ParseDefs` recursion nested
  without a limit. Replaced by FTEF-18.32.
- **FTEF-18.27** **Withdrawn** (P2-133, #574): it ended a struct's, class's or accessor's body at
  any declaration line, but FTEQCC reads each body to its `}`, so a member at column 0 was
  refused; replaced by FTEF-18.34.
- **FTEF-18.28** A frame cycle, `[++ first .. last]` or `[-- first .. last]`
  (`if ((isinc=QCC_PR_CheckToken("++")) || QCC_PR_CheckToken("--"))`), has `first` and `last`
  as expressions separated by `QCC_PR_Expect("..")`; `(w)` or `(W)` after the `++` or `--`
  cycles `weaponframe` instead of `frame` (`OP_CWSTATE`). Two constant frames against the
  direction, `if (first->_float > last->_float)` after `++` or `<` after `--`, are the error
  `E0212` "Forwards State Cycle with backwards range": FTEQCC's `QCC_PR_ParseWarning` with
  `ERR_STATETYPEMISMATCH`, an error code, prints an error. This profile compares two frames
  written as single numbers.
- **FTEF-18.29** FTEQCC tells the readings of source apart by `s_filen`, a string it makes anew
  (`qccHunkAlloc`) for each compiled file (`QCC_PR_CompileFile`), each `#include` of a file,
  and each macro expansion (`QCC_PR_IncludeChunkEx`, given a `CompilerConstant_t`), and it
  compares the pointers, so each of these is a reading of its own; the file that includes
  another is the same reading again after the include. A frame macro keeps the reading that
  defined it (`pr_framemacro[].file`), and:
  - a use of a frame macro that another reading defined is the warning `W0105` "Stale macro
    used (NAME, defined in FILE)" (`WARN_STALEMACRO`, on by default), where FILE is the
    defining file's path: `QCC_PR_FindMacro` checks
    `if (pr_framemacro[i].file != s_filen)` after each of its two lookups (FTEF-18.21), for a
    reference, `$framerestore` and `$modelname` alike;
  - a frame name the same reading already defined keeps its first number with the warning
    `W0106` "Duplicate macro defined (NAME). Rename it." (`WARN_DUPLICATEMACRO`): in
    `QCC_PR_MacroFrame`, `if (pr_framemacro[i].file == s_filen && !force)`; a name another
    reading defined takes the new number and this reading silently ("old file, override it"),
    and so does the model name `$modelname` defines, with `force` set (FTEF-18.16). This is
    what "the same file" of FTEF-18.17 and FTE-22.4 means;
  - a new frame name of 64 bytes or more is the error `E0119` "Name for frame macro NAME is
    too long" and is not defined: `if (strlen(name)+1 > sizeof(pr_framemacro[0].name))
    QCC_PR_ParseWarning(ERR_TOOMANYFRAMEMACROS, ...)`, with `char name[64];`, an error code,
    which `QCC_PR_PrintWarning` prints as an error (FTEF-18.28). The frame number still goes
    up by one for it;
  - a `$modelname` of 64 bytes or more is cut to its first 63 with the warning `W0107`
    "$modelname name too long": `if (!QC_strlcpy(pr_framemodelname, pr_token,
    sizeof(pr_framemodelname))) QCC_PR_ParseWarning (WARN_STRINGTOOLONG, ...)`, with
    `char pr_framemodelname[64];`.

  The include goes on with the frame numbering of the file that includes it, which only a
  compiled file restarts (FTEF-18.17). This profile reports a frame macro used inside a macro
  expansion at the macro body's token.
- **FTEF-18.30** A frame macro whose number is below 0 is unknown where it is expanded: a
  reference and `$framerestore` take their number from `QCC_PR_ExpandMacro`, which refuses any
  negative one as it refuses a name with no macro, `QCC_PR_FindMacro`'s `-1`: `int i =
  QCC_PR_FindMacro(pr_token); if (i < 0) QCC_PR_ParseError (ERR_BADFRAMEMACRO, "Unknown frame
  macro $%s", pr_token);`. So `$framevalue -5`, `$frame a` and `x = $a;` is the error `E0106` "Unknown frame macro $a"
  at `$a` (QCD-2.6), as is a `$framevalue` whose `atoi` is negative (FTEF-14.55):
  `-99999999999` is -1215752191 and `2147483648` is -2147483648 on an LP64 host. The lookup's
  warnings (FTEF-18.21, FTEF-18.29) come first, as `QCC_PR_FindMacro` gives them. A refused
  `$framerestore` leaves the number of the next frame name as it was. `$modelname` calls
  `QCC_PR_FindMacro` itself (FTEF-18.16), so a negative model name macro still numbers the
  next frame. Under the vanilla profile frames are numbered from 0 (QC-8.5) and none is
  negative. The cases are in the directive family of `cargo xtask oracles builtins`
  (`FRAME_VALUES` in `xtask/src/oracles/builtins/directives.rs`).

### Test vectors

Larger vectors: [`vectors/dialect-fteqcc/`](vectors/). The recorded coverage run over the
FTEQCC corpus mods is
[`vectors/dialect-fteqcc/coverage.txt`](vectors/dialect-fteqcc/coverage.txt). The vectors of
the `Codegen (later)` statements are added when L-01 (codegen) fixes them, each checked
against FTEQCC.
- **FTEF-18.31** `until (e) s` repeats `s` while `e` is false, and `do s until (e);` is the
  `do` loop with the test reversed: `wasuntil = QCC_PR_CheckKeyword(keyword_until, "until"); if
  (wasuntil || QCC_PR_CheckKeyword(keyword_while, "while"))`, and in `do`, `until =
  QCC_PR_CheckKeyword(keyword_until, "until"); if (!until) QCC_PR_Expect ("while");`. `until` is
  a keyword when its flag is on: by default only for the Hexen II targets (`nondefaultkeyword`
  in the keyword table, forced on `if (qcc_targetformat == QCF_HEXEN2 || ...)`, FTEF-20.3), and
  after `#pragma keyword enable until`, `-Kuntil` or a `-std=` that sets it (FTEF-20.1); or when
  written `__until`, which `QCC_PR_CheckKeyword` matches whether or not the keyword is enabled
  (FTEF-20.2); otherwise `until (e)` is a call of a function named `until`.
- **FTEF-18.32** Under this profile the nesting limit of QC-27.15 counts the FTEQCC constructs
  too: the operand of a prefix `-`, `+`, `~`, `++`, `--`, `&` or `*` (FTE-19.4, FTEF-16.7), of
  a cast (FTEF-16.2) and of `sizeof` (FTEF-18.12) is one level inside it, as the operand of `!`
  is, each `*` and each parameter list of a type (FTEF-4.1, FTEF-18.15) nests the type one
  level deeper, and so does each `*` before a declarator's name (FTEF-15.6); and each `extern`
  block (FTEF-21.17) is one level for the definitions it holds, braced or not, at file level,
  in a body and behind `local`, since `QCC_PR_ParseDefs(NULL, false)` calls itself for them. So
  `extern "C" { extern "C" { ... float y; } }` and `extern "C" extern "C" ... float y;` nest to
  the limit and no further, whether the braces close or not. QC-27.16 then holds as for
  vanilla: a prefix `++` or `--` it unwinds through is read as no postfix one, a type or a
  declarator takes no further `*` or parameter list, and a block past the limit is one error token, its `extern`. FTEQCC has no such limit. The tests parse every construct that nests, repeated,
  under both profiles, so that no recursive entry of the parser is without its level.

- **FTEF-18.33** The body of a `switch` is any statement: after `QCC_PR_Expect (")")`, `oldst =
  numstatements; QCC_PR_ParseStatement ();` reads one, so `switch (1) ;` and `switch (1) break;`
  are read, and the labels of its block are those of the statement. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseStatement`.
- **FTEF-18.36** With `qcfuncs` on, `QCC_PR_ParseType` takes one parameter list after a type;
  a further one is `QCC_PR_ParseDefs`'s (`while(QCC_PR_CheckToken ("("))`), and the declarator it
  starts then needs a name (`name = QCC_PR_ParseName ()`): `void()();` and `void() (float);` are
  the error `E0201` `";" - not a name`, and `void();` declares nothing.
- **FTEF-18.37** Where FTEQCC's expression ends at a `[` or `.` after a value that takes
  neither (FTEF-31.2, FTEF-24.1), the statement it has read is that value alone when the value
  starts the statement, so `x[1] = 2;`, `x.y = 2;` and `3[1] = 2;` warn "Statement does not do
  anything" before "expected ;, found [", as a value that calls nothing and stores nothing
  does (FTEF-18.25); `g()[1] = 2;` does not, nor does `y = x[1];`, whose statement starts
  elsewhere. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_DiscardExpression`, `QCC_PR_ParseStatement`.
- **FTEF-18.38** In a definition's value the same `[` ends the value before it, which the
  definition then reads whole: cast to its type, "initializer for 'NAME' is not constant" at
  global scope when the value's definition is no constant (`tmp.sym->constant`, which a field
  is), and a refused cast's fatal error ends it there. The `[` is then where the definition
  wants its `;`: "Missing semicolon at end of definition", and at global scope `"[" is not a
  type` as it reads the next definition, in a function "Statement does not do anything" of the
  statement the `[` starts, which this profile does not check further. So `float q = x[1];` of
  a global `float x` gives all three, `.float q = ff[1];` of a field the last two, `local float
  q = x[1];` the warnings, and `float q = e[1];` of an entity only "Cannot cast from entity to
  float". Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseDefs`, `QCC_PR_GenerateInitializerType`.
- **FTEF-18.34** This profile's error recovery (QCD-1.4) ends the body of a struct, class,
  accessor, `switch` or function at a declaration line, a token at column 0 that has a
  declaration's shape or is one of `typedef`, `class`, `accessor`, `enum`, `enumflags`, `struct`
  and `union`, only when no `}` stands ahead to close it, since FTEQCC reads each body to its `}`
  and a member or a local at column 0 is one. So a body that lost its `}` damages no
  declaration after the next, and a body that has its `}` is read whole whatever its columns.
  Under the vanilla profile a function's statements end at a declaration line as before, where a
  declaration needs `local`.
- **FTEF-18.35** A duplicate C-style label (FTEF-18.23) is warned of and its name read, but not
  its `:` (`QCC_PR_ParseWarning(WARN_DUPLICATELABEL, ...); QCC_PR_Lex(); return;`), so the `:`
  starts a FrikQCC-style label: `l: ; l: x;` defines `x`, and `l: ; l: ;` is the error `E0201`
  `invalid label name ";"` (`QCC_PR_ParseError(ERR_BADLABELNAME, "invalid label name \"%s\"",
  pr_token);`), which a `:` before any token that is no name is.

## FTEF-19 Definitions as FTEQCC makes them

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs`, `QCC_PR_GetDef`, `QCC_PR_DummyDef`,
`QCC_PR_ParseInitializerType_Internal`, `QCC_PR_GenerateInitializerType`,
`QCC_PR_ParseDefaultInitialiser`, `QCC_PR_ParseEnum`, `QCC_PR_ParseDefArray`,
`QCC_PR_ParseImmediateStatements`, `QCC_PR_ParseState`, `QCC_PR_ParseRefValue`,
`QCC_TryEvaluateCast`, `QCC_EvaluateCast` and `QCC_PR_ParseStatement`; `qcc_pr_lex.c`,
`QCC_PR_LexNumber`, `QCC_PR_ParseFunctionType`, `QCC_TypeForName`, `typecmp` and `typecmp_lax`;
`qccmain.c`, the keyword and warning tables, `QCC_SetDefaultProperties` and
`QCC_PR_FinishCompilation`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. The
function-by-function inventory is [`inventory-l3.md`](inventory-l3.md).

FTEQCC's compiler descends from id's `qcc` (`inventory-l2.md`), and its `QCC_PR_ParseDefs` and
`QCC_PR_GetDef` grew from id's `PR_ParseDefs` and `PR_GetDef` (QC-11, QC-12). These statements
are what the FTEQCC profile adds on top of them, ported into `qc-sema`'s resolver
(`crates/qc-sema/src/resolve/`, `crates/qc-sema/src/cast.rs`) and proven by the definitions
family of `cargo xtask oracles builtins` (FTEF-19.18). Where a statement names a warning, the
pinned FTEQCC gives it with every warning on but those `QCC_SetDefaultProperties` turns off
(`qccwarningaction[i] = WA_WARN;`, FTEF-18).

### Behaviour

- **FTEF-19.1** Parameters and locals are found in their function first (`QCC_PR_GetDef` scans
  `localstable` when `scope` is set). A parameter given twice, or named as a component of an
  earlier vector parameter, is the error `E0215` "Duplicate definition of NAME.": the parameters
  are made with `QCC_PR_GetSRef (type->params[u].type, pr_parm_names[u], pr_scope, 2, 0, 0)`, and
  `if (allocate == 2) QCC_PR_ParseErrorPrintDef (ERR_TYPEMISMATCHREDEC, def, "Duplicate
  definition of %s%s%s.", ...)`. A vector parameter after a parameter named as its component is
  no error: `QCC_PR_DummyDef` makes the components with no lookup. A local declared again with
  its type is the warning `W0207` "NAME duplicate definition ignored", or "nonstatic redeclaration
  of NAME ignored" after a `static` one, and the same definition; a `static` local declared again
  is neither; another type is `E0205`. A new parameter or local named as a global of the same
  kind of type (`def->type->type == type->type`) is the warning `W0208` "Local \"NAME\" hides
  global with same name and type"; a global of another kind gives none. A parameter of a
  function with a body that has no name is the warning `W0222` "Parameter N of NAME is not
  named", N counted from 1; a lone `void` is no parameter (FTEF-15.3). In a function body a
  statement defines locals when it starts with `local`, a type, a type name, `.`, or one of the
  words `QCC_PR_ParseStatement` tests with their keyword enabled: `var`, `noref`, `static`,
  `extern`, `const`, `typedef`, `class`, `struct`, `union` and `enum` (`register`, `volatile` and
  `auto` are disabled keywords, `nondefaultkeyword`); `shared`, `nosave`, `inline` and the `__`
  words start no definition there, so `shared float x;` is "Unknown value \"shared\"".
- **FTEF-19.2** **Withdrawn** (P2-122, #558): it gave every cast among pointers, strings and
  functions the warning `W0220`, but `QCC_TryEvaluateCast` makes those to and from `void*` and
  between a string and a pointer to an 8-bit integer with none, and a pointer to another with
  none when they point to one type; and it read the flags that change the casts as off.
  Replaced by FTEF-19.31.
- **FTEF-19.3** A definition is a constant when its first declaration makes it one, `GDF_CONST`:
  `const`, or at file level and not `var`, initialised, a function or a field (`if (isconstant
  || (!isvar && !pr_scope && ((isinitialised && !flag_assumevar) || type->type == ev_function ||
  type->type == ev_field)))`); an enum's value and a `const` local are constants, a `static`
  local and an `autocvar_` are not. A later declaration does not change it (`//
  def->constant = isconst;` is commented out) and warns `W0212` "Redeclaration of NAME would
  change to const." (or "non-const.") when initialised, "Redeclaration of NAME changes const."
  when not; a new definition is compared with the constness it gets, which differs only for an
  initialised `autocvar_` (FTEF-19.16), so `float autocvar_x = 1;` gives the warning too. A
  name initialising a global or a `static` that is no constant is the error `E0214`
  "initializer for 'NAME' is not constant" (`QCC_PR_GenerateInitializerType`, `if
  (!tmp.sym->constant)`); a plain or `const` local's initialiser is not checked. A constant that
  has no value yet gives the warning `W0219` "initializer for NAME is not initialised yet, NAME
  will be treated as 0", and the value 0; a field always has its value (fields are "initialised
  behind the scenes") and a function not yet defined is filled in once it is (`$relocf`), so
  neither warns.
- **FTEF-19.4** **Withdrawn** 2026-10-28: it gave the "incompatible redeclaration" after "redeclaration of function body" as `E0206`, which is `E0363` (FTE-22.6); replaced by FTEF-19.57.
- **FTEF-19.57** A body given to a definition already initialised, by a builtin, a body, `extern`
  or `shared`, is the error `E0216` "redeclaration of function body" (`else if
  (basedef->initialized == 1 && !(flags & PIF_STRONGER))`), reported before the "incompatible
  redeclaration" the two different functions then give (`E0363`); a body after a `weak` one, or
  for an `accumulate` function, is none, and a body after a `wrap` one is. A body replacing the
  builtin of `anglemod` or `crossproduct` is the warning `W0217` "NAME already declared as a
  builtin" instead, and the builtin is marked unused. A `wrap` body with nothing initialised to
  wrap is the error `E0222` "wrapper function does not wrap anything", and one that does not use
  `prior` the error `E0223` "Wrapper function \"NAME\" does not refer to its prior function". An
  `accumulate` declaration of a function initialised before without it is the warning `W0218`
  "NAME redeclared to accumulate after initial declaration".
- **FTEF-19.5** **Withdrawn** 2026-10-28: it gave a differing word `E0206`, which is "incompatible redeclaration", `E0363` (FTE-22.6); replaced by FTEF-19.58.
- **FTEF-19.58** The "dupe initialisation" comparison of FTE-22.6 and FTEF-17.10 compares the
  words the two initialisers give: a number literal's value as FTEQCC's lexer reads it, so
  `0x10` and `16` are equal; a constant's name, the constant's value (`float y = 2; float x = y;
  float x = 2;` compiles); a vector's components, which `QCC_PR_DummyDef` gives the vector's
  initialised state and words (`d->initialized = def->initialized;`), so `vector v = '1 2 3';
  float v_x = 1;` compiles and `float v_x = 2;` is `E0363`; and a `const` or `static` local's, as
  a global's. `parseentitydata` given another builtin after `#608` or `#613` is accepted with the
  warning `W0223` "incompatible redeclaration. Please validate builtin numbers. parseentitydata
  is #613" ("dpextensions is WRONG").
- **FTEF-19.6** A default value must be a constant (`if (!ref.sym->constant)
  QCC_PR_ParseError(0, "Default value not a constant\n");`, `E0217`) with a value
  (`QCC_PR_ParseError(0, "Default value not initialized yet\n");`, `E0218`, but with
  autoprototyping), cast to the parameter's type (FTEF-19.31). Two function types are the same
  only if, where both give a default value, the values are equal (`typecmp`, `(a->type ==
  ev_function && (a->params[i].defltvalue.cast && b->params[i].defltvalue.cast))`), so `void(float
  a = 1) g;` and `void(float a = 2) g = #6;` are `E0205`; a value given on one only is no
  difference. A parameter with a default value is not `optional`.
- **FTEF-19.7** At file level a redeclaration whose type differs (`typecmp`) but agrees under
  `typecmp_lax`, the return types and the shared parameters agreeing and every parameter one
  list has beyond the other `optional`, whether either is variadic aside, is the warning `W0209`
  "Optional arguments differ on redeclaration of NAME. A, should be B", unless the two have the
  same parameters and only the first is variadic, and the definition takes the type with more
  parameters. A redeclaration of `droptofloor`, `callfunction` or `trailparticles` with another
  type is the warning `W0210` "NAME builtin was wrongly redefined as TYPE. ignoring later
  definition", and in a file named `*extensions.qc` (any case) of a function "NAME builtin was
  redefined as TYPE. ignoring alternative definition": the first type stays and the
  declaration goes on. A global marked unused that nothing refers to, `def->unused &&
  !def->referenced`, is replaced by a declaration of another type, with the warning `W0211`
  "Type redeclaration of TYPE NAME replaces existing variable": a builtin a body replaced
  (FTEF-19.57) is one. A `noref` mark is a reference too (`if (noref) { def->unused = true;
  def->referenced = true; }`), so a `noref` global declared again with another type is
  `E0205`.
  A function declared again as `T(...)` with no parameters is no difference ("c89-style dumb
  redeclarations"). Any other difference is `E0205`, with FTEQCC's text "Type mismatch on
  redeclaration of NAME. A, should be B".
- **FTEF-19.8** A redeclaration's array size, where it gives one, is the definition's: `if
  (def->arraysize != arraysize && arraysize>=0)` is the error `E0220` "Array sizes for
  redecleration of NAME do not match (A->B)" (in a function body "(A -> B)"), a declaration that
  is no array giving 0, so `float x[2]; float x;` is the error. `[]` gives no size, and an
  unsized definition takes the size a later one gives; an unsized array's initialiser sizes it
  (`QCC_PR_ParseDefArray` counts the elements of `{ ... }`, and a string's bytes and its
  terminator, `dim[0] = pr_immediate_strlen+1;`). A size that is no number literal is not
  compared (FTEF-19.63). An array is initialised with `{ ... }`, or an array of an 8-bit
  integer (`__int8`, `__uint8`, `__char`: `def.cast->type == ev_bitfld && def.cast->bits == 8`)
  with a string: anything else is the error `E0204` "expected {, found X"
  (`QCC_PR_ParseInitializerType_Internal`, `QCC_PR_Expect("{");`). A string of more bytes than
  the array's size is the error, not fatal, `E0233` "initializer-string too long" (`if
  (pr_immediate_strlen > arraysize) QCC_PR_ParseWarning (ERR_BADARRAYSIZE, ...)`), so `__char
  c[2] = "ab";` is no error.
- **FTEF-19.9** A `typedef` of a name a typedef already names must name the same type, as
  `QCC_TypeForName` resolves a typedef to its type (`if (t && t->type == ev_typedef) return
  t->aux_type;`): else the error `E0219` "Cannot redeclare typedef NAME from A to B", so
  `typedef float t; typedef int t;` is the error and `typedef float t; typedef t t;` is not. A
  redeclaration compares a typedef's name as the type it names (`typedef float t; t x; vector
  x;` is `E0205`). A typedef with a storage qualifier is the error `E0224` "bad combination of
  modifiers with typedef (defining NAME)", `static`, `noref` and `nosave` among them when no
  `#pragma` makes them the default (FTEF-21.14); `typedef;` and `typedef float;` define nothing, with the
  warning `W0215` "typedef defines no types". A `typedef` in a function body is a definition
  statement (FTEF-19.1), checked as one at file level, whose type name this profile reads as
  any other type name.
- **FTEF-19.10** An enum's names are constants of the enum's base type, `float` unless `: type`
  gives another, made with `QCC_PR_DummyDef`. A name's value is the one after its `=`, a number
  or a numeric constant converted to the base type, or the one before it plus 1; under
  `enumflags` times 2, starting at 1, and 1 after a 0 (`if (!next_i) next_f = next_i = 1;`).
  Each enum starts its count again. After a value this profile does not compute (FTEF-19.63)
  the names that follow have no value it compares. A
  string or vector after `=` is the error `E0227` "Implicit type mismatch. Needed float, got
  string." (`QCC_SupplyConversion`), a name that is no constant with a value the error `E0228`
  "enum - NAME is not a compile-time constant", and a base type that is no number with no value
  the error `E0229` "values for enums of this type must be initialised". An `enumflags` value
  that is no whole number is the warning `W0225` "enumflags - VALUE not an integer value", and
  one with more than one bit set "enumflags - 0xV(V) has multiple bits set". A base type before
  the `{` with no `:` is the warning `W0224` "legacy enum base type. Use \"enum [class]
  [name_e]:type\" instead", and so is a named enum declared again, whose name `QCC_PR_ParseType`
  then reads as that base type. A later declaration of an enum's name compares its initialiser with
  the name's value (FTEF-19.58). The names of an `enum class` are not referable alone
  (`QCC_PR_DummyDef(..., !strictenum, ...)`), only as `Name::value`, and are of the enum's own
  type, which casts to no other: `float x = E::A;` is "Cannot cast from enum E:float to float".
- **FTEF-19.11** A `shared` definition is initialised by its declaration (`if
  (!def->initialized && shared) ... def->initialized = true;`), and giving it a value, then or
  later, is the error `E0221` "shared values may not be assigned an initial value". An `extern`
  function is initialised by its declaration too (FTEF-19.57); `extern` on what is no function is
  ignored, with the warning `W0214` "extern keyword may only apply to qc functions.".
- **FTEF-19.12** A `static` global is visible only in its unit, the compiled file and the files
  it includes (`if ((def->isstatic||(flags&GDF_STATIC)) && strcmp(def->unitn,
  scope?scope->unitn:s_unitn))`), so another unit's lookup finds the next definition of the
  name, or none; `static` on a name another file defined is the error `E0231` "can't redefine
  non-static as static".
- **FTEF-19.13** A use of a definition marked `__deprecated` is the warning `W0314` "Variable
  \"NAME\" is deprecated", followed by ": REASON" when `__deprecated("REASON")` gives one.
- **FTEF-19.14** `QCC_PR_ParseState` reads a state header's frame before its next function, so
  the frame sees no name the header declares (`[h, h]` with `h` not declared before is "Unknown
  value \"h\""). The frame is converted to a float, a string or vector being `E0227` "Implicit
  type mismatch. Needed float, got string."; a next function that is a literal but 0 is the
  error `E0309` "Type mismatch: string, should be void()".
- **FTEF-19.15** `Scope::name` defines a member of the class `Scope`, or, with the warning
  `W0216` "Scope::name is not part of class Scope", a new one (`WARN_MEMBERNOTDEFINED`); of a
  struct, the global `Scope::name` an earlier definition made, else the error `E0226`
  "Scope::name is not part of class Scope" (`ERR_NOTDEFINED`); and a scope that is neither, a
  typedef or an enum included, is the error `E0225` "Scope is not a class". After either error
  this profile still defines the global `Scope::name`, for navigation (QCD-1.4).
- **FTEF-19.16** A name is looked up as a definition first and only then as an intrinsic
  (FTEF-17.4), `random` among them ("returning a float just in case people declare things in the
  wrong order"). Outside a member function `this` is the error `E0310` "Cannot use 'this'
  outside of an OO function" and `super` `E0311` "Cannot use 'super' outside of an OO function";
  in a member function of a class with no parent `super` is `E0312` "class NAME has no super",
  where the pinned FTEQCC crashes (signal 11), refusing the program as the profile does. A local
  named `autocvar_...` is a global, with the warning `W0213` "Autocvar \"NAME\" defined with
  local scope. promoting to global."; a constant or static one is made neither, with "defined as
  constant. attempting to correct that for you." or "defined as static. ...". A definition named
  as an enabled keyword (`var`, `for`, `switch`, `case`, `local`, `default`, `goto`,
  `continue`, `string`, `float`, `entity`, `vector`, `const`, `asm`, and `break` for what is no
  function) is the warning `W0221` "\"NAME\" keyword used as variable name - coexisting", the
  keyword keeping its meaning. `QCC_PR_ParseDefs` warns `W0214` "Conflicting storage classes."
  for more than one of `extern`, `static`, `shared` and `auto`, "Conflicting constness." for
  `const` and `var`, and "function modifier on non-function." for `inline`, `wrap`, `weak` or
  `accumulate` on what is no function, which `accumulate` makes the error `E0230` "accumulate
  applies only to functions, not NAME".
- **FTEF-19.17** **Withdrawn** (P2-121, #556): it said attributes, `assumeint` and `assumevar` are
  not modelled, but the parser and the semantic model read them (FTEF-21.9, FTEF-19.24,
  FTEF-21.11). Replaced by FTEF-19.30.
- **FTEF-19.18** The definitions family of `cargo xtask oracles builtins`
  (`xtask/src/oracles/builtins/declarations.rs`) compiles programs that bind `f` and then make
  the definitions these statements and QC-11 to QC-13 describe, under both profiles, and the
  FTEQCC warning programs with `-Werror -Wno-Q208`, for which every warning of the profile is an
  error, so a warning one compiler gives and the other does not changes the outcome (Q208, "no
  end_sys_fields defined", is what every program without system definitions gets). Its run of
  2026-10-21 (macOS arm64) agrees on every case but the vanilla `float k = 7; float y; float x
  = y;`, which differs as QC-27.8 says.
- **FTEF-19.19** The locals a `for` statement defines, in its first clause or anywhere in its
  body, leave scope at its end: `QCC_PR_ParseStatement_For` keeps `subscopestart =
  pr.local_tail` and `subscopestop = pr_subscopedlocals?NULL:pr.local_tail->nextlocal`, which
  is the end of the list whether `subscope` is on or off (FTEF-21.12), and after the statement removes
  every local after `subscopestart` from `localstable` ("remove any new locals from the
  hashtable. typically this is just the stuff inside the for(here;;)"). So after the loop the
  name is the global again, or "Unknown value", and a local of the name is a new definition of
  any type: `for (float i = 0;;) {} for (string i = "";;) {}` is two locals. A local the
  statement found again (`float i; for (float i = 0;;) {}`, FTEF-19.1) was not made there and
  stays. Only `for` does this: a block, `while`, `do`, `if` and `switch` keep their locals.
- **FTEF-19.20** A name that is a definition's type, or part of it (a field's, a pointer's, a
  function's return or parameter type), must be a type at that point: C type words
  (FTEF-2.8), a name `QCC_PR_NewType` registers (`variant`, `__variant`, `__string`,
  `__vector`, `__entity`, `__bfloat` and the scalars of FTEF-2.8), or a class, struct, union,
  typedef, enum or accessor declared before it, at file level or, for a local, by a `typedef`
  earlier in the same body (`QCC_TypeForName` finds a typedef whose `scope` is `pr_scope`).
  Else `QCC_PR_ParseType` gives the error `E0203` "\"NAME\" is not a type", which ends the
  declaration, so `foo x;`, `.foo x;`, `foo *p;` and `t x; typedef float t;` are errors and
  `class C; C c;` is not. In a body FTEQCC reads such a statement as an expression ("Unknown
  value \"NAME\"", FTEF-2.10); this profile, whose parser cannot tell a type name from a value,
  reports the same `E0203` there and makes no local, an error either way. A type of more than
  one word that is no run of C words (`struct tag`, an inline `enum` or `struct`) and the types
  of a class's or struct's members are not checked (FTEF-19.63).
- **FTEF-19.21** After a qualifier the type may be left out: `QCC_PR_ParseDefs` sets
  `hadmodifier` for each storage class or modifier it reads and calls `QCC_PR_ParseType (false,
  hadmodifier, ...)`, whose `silentfail` returns no type instead of "not a type", and then
  `if (hadmodifier) { //banned since c99... basetype = type_integer; }`. So `const x = 5;`,
  `static y;`, `noref z;`, `var v;`, `__register r;` and, in a body, `static y;` define
  `__int32`s, checked as any definition is (`const x = 5; float x;` is a type mismatch, `const
  k = "a";` the warning "Implicit cast from string to __int32"). The parser reads the type as
  left out when a qualifier is followed by a name and then `;`, `,`, `=` or `[`; a name that is
  a type there (`static t;` after `typedef float t;`) is not told apart, and is read as the
  name.
- **FTEF-19.22** The parser's and the semantic model's warnings are reported as FTEQCC's
  warning settings stand where it gives each (FTEF-14.63, FTEF-14.72): ignored, a warning, or
  an error, `E0125`. Each is the warning `QCC_PR_ParseWarning` is given its text with:
  `W0203` `WARN_UNDESIRABLECONVENTION`; `W0204` and `W0313` Q205 (`WARN_POINTLESSSTATEMENT`);
  `W0205` and `W0215` `WARN_UNEXPECTEDPUNCT`; `W0206`, `W0210`, `W0211` and `W0223` F129
  (`WARN_COMPATIBILITYHACK`); `W0207` Q207; `W0208` F306; `W0209` F308; `W0212` F329; `W0213`
  F310; `W0214` and `W0226` `unimplemented` (`WARN_IGNOREDKEYWORD`); `W0216` F335; `W0217`
  `WARN_NOTSTANDARDBEHAVIOUR`; `W0218` F321; `W0219` `WARN_NOTCONSTANT`; `W0220` F314; `W0221`
  `WARN_KEYWORDDISABLED`; `W0222` F320; `W0224` F328; `W0225` F327; `W0312` Q111; `W0314` F326.
  The five that `warningnames[]` does not name are warnings by default, and no `#pragma
  warning` names them, but `-Wnone`, `-Wextra` and `-Werror` reach them. A warning is read with
  the settings at the token it stands at: a definition's at its first token, a local's, a
  parameter's and a name's use at theirs. `W0201` (vanilla's) and `W0311` (this project's own,
  QCD-7.17) are no FTEQCC warning and are reported as they are.
- **FTEF-19.23** A builtin's engine name written as strings (`#"NAME"`, `:"NAME"`, FTE-17.5) is
  their value as FTEQCC's lexer read them, with the compiler flags in force at the initialiser
  (FTEF-12.12): with `qcfuncs` off `\101` is C's octal escape, `A`, and with it on Quake's
  yellow `1` and the digits after it; under `utf8` a `\u` escape writes UTF-8.
- **FTEF-19.24** `QCC_PR_LexNumber` types a number by the flags in force where it stands
  (FTEF-12.12). Under `assumeint` a number with no suffix and no `.` is an integer by its
  magnitude (`if (num > UINT64_C(0x7fffffffffffffff)) pr_immediate_type = type_uint64; else if
  (num > 0xffffffffu) ... type_int64; else if (num > 0x7fffffffu) ... type_uint; else ...
  type_integer;`), a number with an `f` suffix staying a float (its branch sets `type_float` first),
  and a character constant is an `__int32` (`QCC_PR_LexVector`, `if
  (flag_assume_integer) { pr_immediate_type = type_integer; ...`); under `assumedouble` a number
  with a `.` is a `__double` (`pr_immediate_type = flag_assume_double?type_double:type_float;`),
  but after an `f` suffix (`else if (c == 'f' || c == 'F') { pr_immediate_type = type_float;`). A
  single `l` suffix is 64 bits, and under `ILP32` only two are (`if (islong >=
  (flag_ILP32?2:1))`).
  The literal's type is then cast as FTEF-19.31 says: `string s = 5;` under `assumeint` is
  "Implicit cast from __int32 to string", and `string s = 1.5;` under `assumedouble` "Cannot
  cast from __double to string". The value is kept as the lexer keeps it, a float's bits
  (FTEF-13.5).
- **FTEF-19.25** **Withdrawn** (P2-119, #554): it said the parser reads keywords as written
  under `-Fcaseinsens`, but it matches them in any case as FTEQCC does (FTEF-20.2). Replaced by
  FTEF-19.29.
- **FTEF-19.26** Under `qccx`, `@` before a term is that term read as a string (`QCC_PR_RefTerm`,
  `if (flag_qccx && QCC_PR_CheckToken ("@"))`, `QCC_PR_RefExpression (retbuf,
  UNARY_PRIORITY, EXPR_DISALLOW_COMMA)`), a reference that may be assigned; that it applies only
  to a float or an integer, "'@' operator only functions on floats and ints. go figure.", is the
  type checker's (FTEF-19.63). Without `qccx`, `@` is no term. `^^` is a token of
  `pr_punctuation` that no operator of `pr_opcodes` names, so `a ^^ b` ends the expression
  after `a`.
- **FTEF-19.27** A module's source in source form, the `progs.src` FTEQCC compiles as code
  (PRJ-4.4), is read by `Db::parse` past a first line that starts with `#!` (PRJ-4.10), as
  `qc_syntax::parse_module_source` reads it: the database takes a module's prelude
  (`ModuleInput::prelude`) to be that file.
- **FTEF-19.28** FTEQCC names its core types once, before the first pass, with the flags the
  command line set: `type_string = QCC_PR_NewType(flag_qcfuncs?"string":"__string", ev_string,
  true);`, and so `vector` and `entity`, then `__string`, `__vector` and `__entity` whatever the
  flag. So under `-Fno-qcfuncs` or `-Kno-qcfuncs` the words `string`, `vector` and `entity` are
  no type, "\"WORD\" is not a type" (`E0203`, FTEF-19.20), unless a typedef names the word
  again, while a `#pragma flag disable qcfuncs`, read after the types are named, keeps them.
- **FTEF-19.29** **Withdrawn** (P2-132, #569): it said a type name is found in any case, but
  the type table also holds a scope's, a typedef's redeclaration, a class's members and an
  enum's names, each searched or compared a way of its own. Replaced by FTEF-19.33.
- **FTEF-19.30** **Withdrawn** (P2-122, #558): it said the output target's `extern` check and
  the casts `qccx`, `lax` and `typeexplicit` allow are not modelled, but the semantic model reads
  them (FTEF-22). Replaced by FTEF-19.32.
- **FTEF-19.31** A value initialiser, a parameter's default value and a function initialised
  with a value are cast to the definition's type by `QCC_EvaluateCast(tmp, type, true)`, which
  refuses with the error `E0213` "Cannot cast from A to B", A and B as `TypeName` writes them
  (`__int32`, `__double`, `.float`, `void()`, `entity*`; parameter names, which the model does
  not keep, are left out). A literal's type is `QCC_PR_LexNumber`'s (FTEF-3.1; a character
  constant is a float); a name's is its definition's, and in a member function a field's name
  is `this.field`, of the field's type. The C type words name the type FTEF-2.8 makes, and a
  registered type name its type (`__int32`, `__double`); an 8- or 16-bit integer and `__bint`
  are types this profile does not follow but as what a pointer points to. `QCC_TryEvaluateCast`
  with `implicit` set casts, its branches in order: a type to itself; a null, an integer,
  unsigned or float constant whose word is 0 (`QCC_SRef_IsNull`), to any type but a struct or
  union; numbers among the float, integer, unsigned, 64-bit and double types; a float or an
  integer to a vector, with the warning `W0220` "Implicit cast from A to B"; a string to a
  pointer to an 8-bit integer (`__int8*`, `__uint8*`) and back ("string and char* are the same
  type"), and a pointer, a function or a string to `void*` and `void*` to them ("can cast any
  pointer type to void*"), with no warning; a pointer to another, with `W0220` unless they point
  to one type (`typecmp_lax`, which follows a pointer to what it points to), a type this profile
  does not follow agreeing with any; pointers,
  strings, integers, unsigned and functions among themselves, and an integer to an entity and
  back, the casts "acceptable but probably an error", with `W0220` but between the two integer
  types. Nothing else casts: `float x = "a";`, `entity e = 1;`, `void() g = 5;`, `string s =
  5l;`, `vector v = 5u;` and `entity e = 0d;` are `E0213`, and `entity e = 0;`, `string s =
  -0;` and `void() g = 0;` cast; `void *p = "a";` and `__uint8 *p = "a";` cast with no word,
  `float *p = "a";`, `__int16 *p = "a";` and a `float*` initialising a `vector*` with `W0220`.
  The types are compared with every typedef in them resolved, as `QCC_PR_ParseType` resolves
  them, and two functions the same part by part (FTEF-17.12) are one type: `typedef float t;`
  makes a `void(t x)` initialise a `void(float x)` with no word, and `typedef __uint8 byte;` a
  string a `byte*`.
  These are the casts with `lax`, `qccx` and `flag_undefwordsize` off, their `-Tid` defaults;
  FTEF-22 gives what each changes. A string initialised with `_(...)` takes one string
  literal, else the error `E0232` "_() intrinsic accepts only a string immediate". An
  initialiser that is no literal and no name is read by the check stage (FTEF-34.9).
- **FTEF-19.32** **Withdrawn** (P2-136, #587): it said an initialiser of a global that is an
  expression is checked for no type, and that a function returning a large type is not warned
  of; the check stage reads such an initialiser (FTEF-34.9) and warns of a large return
  (FTEF-34.8). Replaced by FTEF-19.63.

- **FTEF-19.33** Under `-Fcaseinsens`, which only the command line sets (FTEF-14.72), FTEQCC's
  name tables are searched in any case (`pHash_Get = &Hash_GetInsensitive;`), and
  `QCC_PR_GetDef` makes two passes, the first taking only a definition whose name is the one
  written (`if (ins == 0 && strcmp(def->name, name))`), the second, which only a lookup makes
  (`insmax = allocate?1:2;`), any whose name it is in any case, the newest first (the table's
  chain). So a use of `foo` finds `Foo` when nothing is named `foo`, the function's locals
  before the globals, and of `float Foo = 1; string FOO = "a";` the string, the one defined
  last (`float x = foo;` is "Cannot cast from string to float"); a definition named as another
  in another case is a definition of its own. `QCC_TypeForName` searches the type table the
  same way, so a type is found in any case wherever a declaration, a typedef, a parameter or a
  `Scope::` reads one: the names `QCC_main` registers (`void`, `float`, `string`, `vector`,
  `entity` when `qcfuncs` was on, `__string`, `__int32`, `__variant`, `variant`, `__bfloat` and
  the rest of its list) and the typedefs, structs, unions, enums and classes the program names,
  so `Void() h = #6;`, `VECTOR v;`, `typedef Entity e;` and `enum E {A}; float y = e::A;` are
  read as their lower case, and `Long x;` is "\"Long\" is not a type" as `long` is, the C words
  of `QCC_PR_ParseType` being the keywords of FTEF-20.2. A typedef of a name differing in case
  from one made, with another type, is "Cannot redeclare typedef"; the members of a class found
  by its name or a parent's are found in any case, `C::F` and `c::f` naming `C::f`; and an enum's
  value after `Scope::` is the exact one (`strcmp (acc->fieldname, name)`), so `E::a` is
  `Unknown value "E::a"` while `a` finds `A`. `QCC_PR_ParseEnum` makes each value a new definition
  with `QCC_PR_DummyDef`, never a redeclaration, which the lookups find before an older one of
  its name, so `enum {A, B}; #pragma flag enable assumeint` and another `enum {A, B}` make `B` an
  `__int32`. The keywords `QCC_PR_CheckKeyword` and `QCC_PR_CheckName` then also match in any case
  (FTEF-20.2); a statement that starts with a type word in another case is the parser's
  (FTEF-20.6). This profile does not search the preprocessor's macro table in any case, nor
  follow a class named as a parent in another case or report one not defined.
- **FTEF-19.34** The scope of `Scope::name` is read as `QCC_TypeForName` reads a type: a class,
  struct, union, enum or typedef the program defined, or a name `QCC_main` registers, else the
  error `"Scope" is not a type` (`ERR_NOTATYPE`), as for `Nope::thing()`, `Other::g` after
  `float Other::g = 1;` ("Other is not a class", FTEF-19.15) and `e::A` after `enum E {A, B};`.
  `super` is a class's parent and no type name.

- **FTEF-19.35** A definition's type is read by `QCC_PR_ParseType` even when no declarator follows
  (`if (QCC_PR_CheckToken (";")) ... return;` comes after the type), so `float (x);` with
  `qcfuncs` on is `ParseType`'s function type `float(x)` whose parameter `x` is no type, "expected
  function arg list" (`if (!t) { if (!flag_qcfuncs) t = type_integer; else return NULL; }`), and
  `S2 (x);` and `void() g = { local float (x); };` likewise; with `qcfuncs` off the parameter is
  an `int` named x. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseDefs`.
- **FTEF-19.36** An enumerator given again in a named enum's body with another value is "%s::%s
  already declared": `enum E { A, A };` (the second is 1), while `enum E { A = 1, A = 1 };` and an
  anonymous `enum { A, A };` are not, a name given to the enum in an earlier `enum E {` being the
  base type there (FTEF-19.10) and keeping no names under it. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseEnum`.
- **FTEF-19.37** An enum's body holds at least one enumerator: `QCC_PR_ParseEnum` reads a name
  with `QCC_PR_ParseName` before it looks for the `}` (`while(1) { name = QCC_PR_ParseName(); ...
  if (QCC_PR_CheckToken("}")) break; QCC_PR_Expect(","); if (QCC_PR_CheckToken("}")) break; }`),
  so `enum E { };` and `enum { , };` are "not a name", and `enum { A, };` is not. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseEnum`.
- **FTEF-19.38** A type's name is no value: `QCC_PR_ParseValue` finds no definition by the name of
  a struct, union, class, enum, typedef or accessor, "Unknown value" for `S(1)`, `x = S;` and
  `local float q = C;`; the operand of `sizeof` is tried as a type first and `spawn(Thing)` takes
  a class by its name (FTEF-17.1), and `S` in `sizeof(S)` is the type. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefTerm`, `QCC_PR_ParseValue`.
- **FTEF-19.39** An element of a variable, `x[1]`, is no constant initialiser: "initializer for
  'y' is not constant" for `float x[2]; float y = x[1];`, with `qcfuncs` on or off. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseInitializerDef`.
- **FTEF-19.40** A call of a name whose type is a float, vector, string, entity or struct is "not
  a function". Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseFunctionCall`.
- **FTEF-19.41** A function with no body is an error (FTEF-17.11) whether it is a global or a
  member function a class declares and no `Class::member` defines, a `virtual` one included:
  `class C { void() m; };` and `class C { virtual void() m; };` are "function %s has no body",
  `class C { void() m; }; void C::m(void) { };` is not. A function whose type a typedef names is
  one, `typedef float fn(float); fn x;`, and so is C's function pointer typedef (FTEF-21.7)
  `extern "C" { typedef float (*fp)(float); fp x; }`, a function type with no `*`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_FinaliseFunction`, `QCC_PR_ParseDefs`.
- **FTEF-19.42** A state header in a member function names its next function among the members
  declared before it, and one that is no `void()` is "Type mismatch: %s, should be void()": `class
  C { float() m = [1, m] { return 1; }; };` and `class C { void() m = [1, n] {}; void() n = {};
  };` (`n` a function the header declares) are refused, `class C { void() m = [1, m] {}; };` is
  not. A next function in another case is the function under `-Fcaseinsens` (FTEF-19.33): `$frame
  run`, `void() g = [$RUN, G] {};`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseState`.
- **FTEF-19.43** A cast's type and `sizeof`'s are read with `QCC_PR_ParseType(false, true,
  false)`, `silentfail`, so a name that is no type makes no cast and no type: `(T *)0`, `(T)0`,
  `sizeof(T *)` and `sizeof(T)` with no type `T` are a parenthesised expression and an operand,
  "Unknown value \"T\""; `sizeof(s)` after `struct S` is the type under `-Fcaseinsens`
  (FTEF-19.33) and "Unknown value" without, and `(struct s *)0` is a pointer to the struct `s`, a
  tag, in either case (FTEF-6.8). This profile's parser reads the shapes `(name)` and `(name *)`
  before an operand, and `sizeof(name *)`, as a type, and the semantic model reports the name as
  no type, "\"T\" is not a type" (`E0203`): the program is refused either way, and `sizeof(name)`
  tries the name as a type first (FTEF-19.38). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefTerm`.
- **FTEF-19.44** `QCC_PR_ParseEnum`'s legacy form reads any type's name before the `{` as the base
  type (`type = QCC_PR_ParseType(false, true, false)`), a class's or struct's among them, and a
  base after `:` may be one too; an enumerator of such a base has no number to take (`else
  QCC_PR_ParseError(ERR_NOTANUMBER, "values for enums of this type must be initialised");` after
  the `ev_integer`, `ev_uint`, `ev_int64`, `ev_uint64`, `ev_float` and `ev_double` cases) and one
  given a value is an implicit cast to the class, an error: `class E { float e; }; enum E { A };`,
  `class E; enum E { A = 1 };`, `struct E { float a; }; enum E { A };` and `struct S { float a; };
  enum E : S { A };` are refused, which this profile reports as "values for enums of this type
  must be initialised" (`E0229`) at the first enumerator. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseEnum`.

- **FTEF-19.45** A name no definition has, in a body or an initialiser, is made a `variant` and
  reported `QCC_PR_ParseWarning (ERR_UNKNOWNVALUE, "Unknown value \"%s\".", name)`, an error
  ending with a full stop (`E0301`, QCD-4.1's message with it), after `QCC_PR_GetDef`'s
  warning `W0213` "Autocvar \"NAME\" defined with local scope. promoting to global." for an
  `autocvar_` name in a body; a scoped name `S::n` is
  `QCC_PR_ParseError (ERR_UNKNOWNVALUE, "Unknown value \"%s::%s\"", ...)` with none. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseRefValue`.
- **FTEF-19.46** **Withdrawn** 2026-10-28: it counted array sizes among the parts not modelled, which FTEF-19.62 now folds; replaced by FTEF-19.63.
- **FTEF-19.63** These parts of the path are not modelled by this profile, each for its reason:
  the values of default values and enum values that are expressions other than a
  literal, a name or `_()`, which FTEQCC folds as it compiles them, the compiler's work (P2-06,
  constant folding P2-08); an initialiser of a global that is such an expression is compared by
  its text (FTEF-17.10), and typed by the check stage (FTEF-34.9); the warnings FTEQCC gives as
  it writes the program rather than as it defines a name, a definition never referred to ("no
  references", `WARN_NOTREFERENCED`), a field allocated twice ("Field def already has a value",
  "Inconsistant field def"), the precache checks ("Sound \"NAME\" was used but not directly
  precached" and the others of `QCC_WriteData`, from the names `precache_*`, `setmodel`,
  `sound`, `localsound`, `used_sound` and `used_model` are given) and the end_sys checks, which
  belong to the compiler's output (QC-25); designated initialisers, `{ .a = 1 }`, whose members are read as written (FTEF-19.47
  reads the others), and a definition of a struct type not yet defined ("redeclared with
  different storage type"), the compiler's (QC-21); and FTEQCC's reacc and hexenc syntaxes (`flag_acc`, `var name
  : type`, a field `alias`), which `-Tid` does not enable.
- **FTEF-19.47** A braced initialiser is read against the type element by element
  (`QCC_PR_ParseInitializerType_Internal`): an array's elements up to its size ("arrays go
  recursive", each an element of the next dimension or the element type), a vector's as three
  floats, a struct's one per member in order, but `static` and `virtual` members, and each
  value as a definition's value is (FTEF-19.31, FTEF-19.3): `float x[2] = {"a", 1};` is "Cannot
  cast from string to float", `vector x[2] = {1, 2};` warns twice "Implicit cast from float to
  vector", and `float x[2] = {y, 1};` for a variable `y` is "initializer for 'x' is not
  constant". A member or element that is an array needs its own braces, else the fatal error
  "expected {, found X"; a struct member given a value is cast to the struct, "Cannot cast from
  float to struct S". A struct with a parent takes only `{}` or designated members, else the
  fatal error `E0354` "inherited structs must use designated initialisers"; a union's members all
  start at 0, so a second element is the fatal error `E0355` "member U.v was already
  ininitialised". An element past the size or the members is left to what reads on: inside
  braces "expected }, found X"; at a global's top level the warning "Missing semicolon at end of
  definition" and the fatal error `"X" is not a type`; at a local's the warning, "Statement does
  not do anything" and "expected ;, found }". So `float x[2] = {1, 2, 3};`, `S x = {1, 2};` for a
  one-member `S` and `float x[2][2] = {{1, 2, 3}, {3, 4}};` are refused, and `float x[2] = {1,
  2,};` and fewer elements than members are not.
- **FTEF-19.48** An initialiser of a global or `static` naming no definition is "Unknown value
  \"NAME\"." where it is used and then `E0214` "initializer for 'X' is not constant"; an unknown
  `Scope::name` is the first alone.
- **FTEF-19.49** A default value is read with the parameter's type, before the declaration's name
  (FTEF-19.6), so a fatal error in it ends the declaration there: the name is neither defined nor
  compared with an earlier definition, and a body after it is not read. `void(float a, optional
  float b = '1 2 3') g;` then `g(1);` is "Cannot cast from vector to float" and "Unknown value
  \"g\"."; a redeclaration with such a default is that error alone, no "Type mismatch on
  redeclaration".
- **FTEF-19.50** A field of a struct, union or class type casts only to a field of the same type
  or of a `variant` (`typecmp`), any other field or value being the fatal error `E0213` "Cannot
  cast from A to B": `.S fs; .float fa; fs = fa;` is "Cannot cast from .float to .S", and a field
  of a class to a field of a class it derives from "Cannot cast from class Other to class
  Player".
- **FTEF-19.51** An attribute list is a modifier too: `QCC_PR_ParseDefs` sets `hadmodifier = true;`
  at the end of each pass of its modifier loop, the `[[...]]` branch included, so after an
  attribute list alone the type may be left out and is `__int32` (FTEF-19.21): with `attributes`
  on, `[[eraseable]] x = 5; string s = x;` warns "Implicit cast from __int32 to string".
- **FTEF-19.52** An item of an enum of an integer base (`int`, `uint`, `__int64`, `__uint64`) is the
  constant `QCC_MakeIntConst(next_i)` and its kin make of its value, a constant of that type the
  folding reads: `enum E : int { C = 2, D };` makes `arr[D]` the index 3. A float literal given
  to one is converted by `QCC_SupplyConversion`, whose `OP_CONV_FTOI` (or `OP_CONV_FU` and the
  64-bit ones) folds with its warnings: `enum E : int { C = 1.5 };` is "Numerical overflow.
  1.500000 will be rounded to 1" and the value 1.
- **FTEF-19.53** A braced initialiser for a scalar, a float, string, entity, field or integer,
  global, local or an array's element, is the error `E0201` "\"{\" - not a name" at the `{`:
  `QCC_PR_ParseInitializerType` reads the scalar's value as an expression, which `{` cannot
  start (`float x = { 1 };`, `float a[2] = { {1}, 2 };`); a vector takes braces as three floats.
- **FTEF-19.54** `QCC_main` makes the core types again under names of their own:
  `QCC_PR_NewType("__int", ev_integer, ...)`, `"__uint"`, and `"__string"`, `"__vector"` and
  `"__entity"` when the core type's name has no `_`. Each is the basic type it names and prints
  by its own name: `__int a; string s = a;` is "Implicit cast from __int to string", `__vector
  cv; float f = cv;` "Cannot cast from __vector to float", and an argument of `__uint` is named
  `__uint` in the builtin checks' messages (FTEF-33).
- **FTEF-19.55** A float or an integer cast to a vector (`QCC_EvaluateCast`, `totype == ev_vector`)
  is multiplied by `'1 1 1'`, `OP_MUL_FV` or `OP_MUL_IV`, folded, emitted or emulated in turn: on
  the id target `int i; vector v; v = i;` is "Implicit cast from __int32 to vector" and then
  `OP_MUL_IV`'s "itof emulation: denormals are unsafe i", in a store, a compound assignment and
  a local's initialiser alike.
- **FTEF-19.56** A pointer's braced initialiser (`else if (type->type == ev_pointer &&
  QCC_PR_CheckToken("{"))`) is a nameless array of what the pointer points to, made by
  `QCC_PR_DummyDef` with `GDF_STATIC` inside a function, whose elements are read as values of a
  definition named `''`: `float x; float *p = { x };` is `E0214` "initializer for '' is not
  constant", a local's included, and `{ 1, "a" }` "Cannot cast from string to float". The
  pointer then takes its address with `QCC_PR_GenerateAddressOf`: a target without
  `OP_GLOBALADDRESS` refuses it, the fatal `E0331` "Address-of operator is not supported in this
  form without extensions. Consider the use of: #pragma target fte"; at global scope it is
  `E0345` "Unable to generate statements at global scope (GLOBALADDRESS)."; and a global's or a
  `static`'s value is then `E0214` "initializer for 'p' is not constant". A plain or `const`
  local takes it with no message.
- **FTEF-19.59** A `static` local whose function already has a local of the name that is not
  `static` is the fatal error `E0364` "static redefinition of NAME follows non-static definition."
  (`QCC_PR_GetDef`: `else if (allocate && (flags & GDF_STATIC) && !def->isstatic)`): `void() f = {
  local float x; static float x; };`. A second `static` is the same definition, an `extern
  static` one a lookup of it and no error, and a `static` local of a global's name hides the
  global (FTEF-19.1). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_GetDef`.
- **FTEF-19.60** A lookup that passes over a `static` global of another unit (`if
  ((def->isstatic||(flags&GDF_STATIC)) && strcmp(def->unitn, ...)) { if (!foundstatic)
  foundstatic = def; ...}`) and finds nothing warns `W0315` "NAME defined static" (`if
  (foundstatic && !allocate && !(flags & GDF_SILENT))`, `WARN_SAMENAMEASGLOBAL`, `F306`) before
  FTEF-19.45's "Unknown value": `a.qc` `static float x;` and `b.qc` `void() f = { x = 1; };`.
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_GetDef`.
- **FTEF-19.61** A global or `static` local `__auto` (`isauto`, not in a body unless `static`)
  is allocated at start-up by `memalloc`, or `malloc`, which `QCC_PR_EmulationFunc` looks up by
  name; with neither defined it is the fatal error `E0365` "memalloc not yet defined, cannot
  define __auto NAME at global scope". A local's qualifiers are checked as a global's
  (FTEF-21.14): two storage classes are `W0214` "Conflicting storage classes." (`static __auto`
  is), and `const` with `var` "Conflicting constness.". `auto` itself is a keyword only once
  enabled; `__auto` always is. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseDefs`.
- **FTEF-19.62** An array's size that is an expression other than a number literal is folded as
  it is read (`QCC_PR_ParseDefArray`: `QCC_SRef_EvalConst` of `QCC_PR_Expression`, an integer by
  `QCC_Eval_Int`), a global's at global scope before its definition is made, a local's where it
  is declared; one that is no constant is the error `E0366` "Array length is not a constant", and
  the definition's size is the one it folds to, for its bounds (FTEF-31.6), its initialiser and a
  redeclaration's size (FTEF-19.8). So `const float N = 3; float c[N];` makes `c[3]` "(constant)
  array index out of bounds (0 <= 3 < 3)" and `float d[N] = {1,2,3,4};` reads the fourth value as
  the next definition, and `float n; float a[n];`, `float a[n2];` and `float a[n+1];` (with
  "Unable to generate statements at global scope (ADD_F).") are the error. A local's first
  dimension is left for the function to allocate (`dynlength`) only on a target with `OP_PUSH`
  under `qcfuncs` off, when the local is not `static` and has no initialiser. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseDefArray`.
- **FTEF-19.64** Designated braces, whose first element starts with `.`, name a member in every
  element (`QCC_PR_ParseInitializerType_Internal`: `if (QCC_PR_CheckToken(".")) ... else
  QCC_PR_ParseError (ERR_EXPECTED, "designated initialisers require all members to be initialised
  the same way")`), a `}` after a trailing `,` among them: `S st = { .a = 1, 2 };` and `{ .a = 1, .b =
  2, }` are the fatal error `E0372`, at the element. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseInitializerType_Internal`.
- **FTEF-19.65** Positional struct braces are read member by member, each value followed by `,`
  or by the closing `}`, and the loop ends at the last member (`for (partnum = 0; partnum <
  (type)->num_parms; partnum++) { if (QCC_PR_CheckToken("}")) break; ... if (isunion ||
  !QCC_PR_CheckToken(",")) { QCC_PR_Expect("}"); break; } }`). After a value for every member
  and a trailing `,`, the closing `}` is left to what reads on: a global's or a local's definition
  is the warning "Missing semicolon at end of definition" and the fatal error `"}" is not a type`,
  at the `}`; `S st = { 1, 2, };` for a struct of two members. Braces with fewer values end at
  their `}`. This profile does not follow the `}` of nested braces, which closes the enclosing
  braces, nor what a local's definition is when more statements follow it. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseInitializerType_Internal`.
- **FTEF-19.66** A definition may write `CLASS::NAME` with no type: `QCC_PR_ParseDefs` reads the
  class's name as the type and, with `::` after it, takes it as the scope (`classname =
  type->name; type = type_invalid;`), and the type is then `void`, so `C::m() = {}` defines a
  `void()`. Unless the name is the class's, a constructor's, it is the error `E0376` "no type
  specified for CLASS::NAME. this is only allowed for constructors", which does not end the
  definition: `C::m = {};` is then also "Type mismatch on redeclaration of C::m. void, should be
  void()". A scope that names no type is `"D" is not a type`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseDefs`.
- **FTEF-19.67** A `(` after a definition's name, and its array suffixes, is a parameter list
  whatever the type (`if (QCC_PR_CheckToken("("))`), so on a function type the definition is a
  function returning a function: `float() g()`. When that function type was written inline
  rather than by a typedef (`type_inlinefunction`), it warns `W0320` "Function returning
  function. Is this what you meant? (suggestion: use typedefs)" (`WARN_UNSAFEFUNCTIONRETURNTYPE`,
  which no flag names), a typedef's as a definition's. Such a type prints its return type by the
  function type's name, `__function()`. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseDefs`.
- **FTEF-19.68** A typedef's name is looked up among the types FTEQCC defines itself as among
  the program's (`old = QCC_TypeForName(name)`): `void`, `float`, `vector`, `string`, `entity`
  and the registered C scalars such as `__int`. A typedef naming another type by one of them is
  the error `E0219` "Cannot redeclare typedef NAME from NAME to TYPE": `typedef vector float;`;
  one naming the same type is none, and, being a type, no definition named as a keyword. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseDefs`.
- **FTEF-19.69** An unknown name in a body (FTEF-19.45) is defined there as a `variant`
  (`QCC_PR_GetSRef(type_variant, name, pr_scope, true, ...)`): its later uses in the function are
  that definition and report nothing, another function's report it again, and a later global of
  the name is no conflict. The checks after the name read it as a `variant`: `nope[1]` and
  `nope.y` are "expected ;, found [" and "expected ;, found .", `if (nope)` is "conditional type
  mismatch: variant", `-nope` "type mismatch for -", and a `variant` stores to a struct, as
  `QCC_SupplyConversionForAssignment` refuses no conversion to or from one. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseValue`, `QCC_PR_GetSRef`, `QCC_SupplyConversionForAssignment`.
- **FTEF-19.70** A `static` local's value is a constant, as a global's is
  (`QCC_PR_GenerateInitializerType`: `basedef->isstatic` and `!tmp.sym->constant`), whatever
  expression gives it: `static float y = x + 1;`, `-x`, `[x, 0, 0]` and `h(1)` of a variable `x`
  and a function `h` are "initializer for 'y' is not constant"; `1 + 2` and `k * 2` of a
  constant `k` fold and are accepted. A plain or `const` local is never refused (`basedef->scope
  && !basedef->isstatic && !basedef->initialized`). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_GenerateInitializerType`.
- **FTEF-19.71** At global scope an unknown name in a definition's value is a global `variant`,
  as in a function it is a local one (FTEF-19.69): `QCC_PR_GetSRef(type_variant, name, pr_scope,
  true, ...)` with `pr_scope` null defines it, and "Unknown value \"NAME\"." is a warning. The
  value is then no constant, "initializer for 'u' is not constant", and the checks after the
  name read a `variant`: `float u = -nope;` is "type mismatch for -", `nope + 1` "Unable to
  generate statements at global scope (ADD_F).", and `float u = nope[1];` ends at the `[`
  (FTEF-18.38). Later uses find the global with no message, and `float nope;` after it
  redeclares it. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseValue`, `QCC_PR_GetSRef`.
- **FTEF-19.72** A global's value that names a global variable whose type converts to the
  global's by opcodes (FTEF-22) is read at global scope as any expression is (FTEF-34.9): `int
  x; float q = x;` on the id target is "itof emulation: denormals are unsafe x", "Unable to
  generate statements at global scope (DIV_F).", then "initializer for 'q' is not constant";
  `float x; int q = x;` the `ftoi` emulation and `MUL_F`; under `#pragma target fte` `CONV_IF`.
  A constant converts by folding, with no message. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseInitializerType`, `QCC_SupplyConversionForAssignment`.
- **FTEF-19.73** After a qualifier, `hadmodifier`, `QCC_PR_ParseType` with `silentfail` finds
  no type at any token that is no name and no field's `.` or `..`, and `QCC_PR_ParseDefs` takes
  `type_integer` (FTEF-19.21); the token is then the definition's name, which
  `QCC_PR_ParseName` refuses: `var = 1;`, `const 5;` and `static = 2;` in a body are "\"=\" -
  not a name" and "\"5\" - not a name", never "is not a type". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseDefs`; `engine/qclib/qcc_pr_lex.c`, `QCC_PR_ParseName`.
- **FTEF-19.74** In a body a statement `T x;`, with no `local` or qualifier and `T` a lone name
  that names no type there, is no definition: `QCC_PR_ParseStatement` finds no type and reads
  the expression `T`, an unknown name being a `variant` (FTEF-19.69), "Unknown value \"T\".",
  then at `x` "Expected punctuation", the statement "Statement does not do anything", and the
  fatal "expected ;, found x"; a name that is a value gives all but the first. So a typedef out
  of its block, a struct of an inner block, or a type named before its typedef is not "is not a
  type", which `local T x;` still is (FTEF-19.20). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseStatement`, `QCC_PR_RefExpression`.
- **FTEF-19.75** A strict enum, `enum class E : T`, casts to and from no other type wherever a
  value is cast, stored, passed, or cast explicitly (FTEF-19.10): its item's null constant too,
  which `QCC_SRef_IsNull` would let any other type take, so `f = E::A;` of a float `f` is
  "Cannot cast from enum E:float to float", and `e = 1;`, `e = (E)1;` and the global `E e =
  (E)1;` are "Cannot cast from float to enum E:float"; under `lax` "Implicit lax cast". A plain
  enum's item is its base type's constant, and its 0 casts to a string with no message. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_TryEvaluateCast`, `QCC_PR_ParseEnum`.

## FTEF-20 Keywords

**Source:** `qcc_pr_lex.c`, `QCC_PR_CheckKeyword`, `QCC_PR_CheckName`, `QCC_PR_ParseType` and
`QCC_PR_ParseFunctionType`; `qcc_pr_comp.c`, the `keyword_*` defines, `QCC_PR_ParseStatement`,
`QCC_PR_ParseStatement_Using`, `QCC_PR_ParseAsm`, `QCC_PR_ParseDefs`, `QCC_PR_DummyDef`,
`QCC_PR_ParseImmediateStatements` and `QCC_PR_ParseRefValue`; `qccmain.c`, `compiler_flag[]`,
`QCC_PR_CommandLinePrecompilerOptions` and `QCC_SetDefaultProperties`;
`qcc_pr_lex.c`, `QCC_PR_ParseEntClass`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.
The places FTEQCC reads and sets its keyword flags are rows of [`inventory-l2.md`](inventory-l2.md)
and [`inventory-l3.md`](inventory-l3.md).

FTEQCC's keywords are flags of `compiler_flag[]` (`keyword_for`, `keyword_long` and the rest of
the `keyword_*` rows), each on or off as the compile goes, and its parser asks for each with
`QCC_PR_CheckKeyword(keyword_X, "X")`. The statements below say how the flags are set and where
they are read; FTEF-2.7, FTEF-2.10, FTEF-15.1, FTEF-15.3, FTEF-16.1, FTEF-19.1 and FTEF-19.16
describe the same places with the flags at their defaults. The keyword family of
`cargo xtask oracles builtins` (`xtask/src/oracles/builtins/keywords.rs`) turns each on and off
and compares the outcome with the pinned FTEQCC's.

### Behaviour

- **FTEF-20.1** The parser reads each keyword with the flags in force at its token. They start
  as `compiler_flag[]` sets them (`FLAG_ASDEFAULT`, `qc_preproc::Keywords::default`), then as the
  command line sets them (`-K`, `-F` and `-std=`, FTEF-14.72) and a Hexen II target (FTEF-20.3),
  then as each `#pragma keyword` or `#pragma flag` sets them (FTEF-12.11) from the token after
  the directive, which FTEQCC lexes, and so reads the directive, before the parser asks for it;
  and a definition named as a keyword may turn that keyword off (FTEF-20.4). The preprocessor
  marks the flags by token (`qc_preproc::FileOutput::keywords`): the first token holds the
  whole state, and a pragma's mark sets only the flags it names, `kce` and `caseinsens` among
  them, so a keyword a definition turned off stays off unless a pragma names it again. The
  flags carry from file to file and into the second autoprototyping pass, as the pragmas'
  flags do (FTEF-12.11). `QCC_ImportProgs`, which `#merge` runs, turns `wrap` and `weak` on
  ("these keywords are implicitly enabled by #merge"); this profile opens no file for `#merge`
  (FTEF-14.47), so it turns none on. Under the vanilla dialect, which has no keyword flags, the
  parser reads id's words as `qcc` does (QC-14.1).
- **FTEF-20.2** `QCC_PR_CheckKeyword(enabled, word)` takes a token that starts with `__` when
  the rest is `word` in any case, whether or not the keyword is on ("lets just always go
  insensitive with a leading underscore pair": `if (pr_token[0] == '_' && pr_token[1] == '_')
  { if (stricmp(string, pr_token+2)) return false; ...`), and any other token only when the
  keyword is on and the token is `word`, in any case under `caseinsens` (`if
  (flag_caseinsensitive) { if (stricmp (string, pr_token)) return false; }`) and exactly
  otherwise. `return`, `if`, `else`, `while`, `do` and `not` are `#define`d as 1, always on;
  `_Bool`, `sizeof`, `_length` and a class's member words (FTEF-20.11) are asked for with `1`
  or `true`; `deprecated` with `false`, so only `__deprecated` is it. `QCC_PR_CheckName(word)`,
  which reads an accessor's `get`, `set` and `inline` and an enum's `class`, takes `word`, in
  any case under `caseinsens`, and never a `__` spelling. So `__FOR` is `for` with the keyword
  off, `FOR` is `for` only under `-Fcaseinsens`, and `__while` is `while`.
- **FTEF-20.3** After the command line `QCC_SetDefaultProperties` turns on `thinktime`, `until`
  and `loop` for a Hexen II target: `if (qcc_targetformat == QCF_HEXEN2 || qcc_targetformat ==
  QCF_UHEXEN2 || qcc_targetformat == QCF_FTEH2) { keyword_thinktime = true; keyword_until =
  true; keyword_loop = true; }`. The target is the command line's (FTEF-14.72), which
  `QCC_SetDefaultProperties` sets again before each pass, so a `#pragma TARGET` turns none of
  them on.
- **FTEF-20.4** `QCC_PR_DummyDef` warns of a new definition named exactly as a keyword that is
  on, `W0221` "\"NAME\" keyword used as variable name - coexisting" (`WARN_KEYWORDDISABLED`,
  which has no name) with `kce` on, and with it off "- disabling" and turns the keyword off
  (`#define KEYWORD(x) if (!STRCMP(name, #x) && keyword_##x) do{ ... keyword_##x =
  keywords_coexist;}while(0)`): `var`, `thinktime`, `for`, `switch`, `case`, `local`,
  `default`, `goto`, `break` for a definition that is no function, `continue`, `state`,
  `string`, `float` unless the target is `QCF_HEXEN2` or `QCF_UHEXEN2` ("hexen2 requires
  this..."), `entity`, `vector`, `const` and `asm`. The keyword is off from the token after the
  name. `QCC_PR_DummyDef` makes every definition: a global, a local, a field, the parameters of
  a function with a body as `QCC_PR_ParseImmediateStatements` reads it (a prototype's and a
  builtin's make none), an enum's names, a state header's or `state` statement's next think
  function when it is new, and a `using` alias; a typedef's name, a struct's members and a
  class's members are none, and `Class::member` is named so, no keyword. So with `kce` off `float for;` makes `for (;;) {}` a call of the
  float `for`, the error "not a function", and `for = 1;` an assignment. Two deliberate
  divergences, both only with `kce` off, as the parser turns the keyword off and the semantic
  model makes the definitions: a definition named as a keyword turns it off even when it is a
  redeclaration, which `QCC_PR_GetDef` finds with no new definition; and the keyword is off to
  the end of the file the definition is in, and on again in the next file and the second
  autoprototyping pass unless a definition there turns it off, as each file is parsed on its own;
  and a body whose function type a typedef names defines no parameter, as the parser knows no
  typedef's parameters.
- **FTEF-20.5** A statement of `QCC_PR_ParseStatement` is read by its first token, the keywords
  tried in its order: `{`; `return`; `exit` (`keyword_exit` is `flag_acc`, reacc's, FTEF-20.13,
  so only `__exit`), `OP_DONE` and a `;`; `loop` and a statement, run again until a `break`;
  `until` or `while`, a parenthesised condition and a statement; `for`; `do`, a statement, then
  `until` with its keyword or `while` exactly (`QCC_PR_Expect ("while")`, so `__while` there is
  "expected while"); `local` and a definition; the definitions of FTEF-20.14; `state`, a state
  header `[frame, next]` as a function's (FTEF-18.3) and a `;`; `if`, with `not` (FTEF-16.5);
  `switch`; `using`; `asm`; a `:` label (FTEF-18.23); `goto`; `break`, which before a `(` is a
  call of the function `break` (`if (!STRCMP ("(", pr_token))`), and else a `;`; `continue`;
  `case`; `default`; `thinktime`, an expression, a `:`, an expression and a `;`; `;`; a `name:`
  label; and else an expression. Each keyword starts its statement whatever follows it, so with
  `for` on `for = 1;` is "expected (, found =". `using (a, b = c) s` finds each name as a value is
  found (`QCC_PR_GetDef (NULL, name, pr_scope, false, 0, 0)`), "NAME is not defined"
  (`E0313`) when none is, and makes an alias of its type named as it is or, after `=`, as the
  name after it, from its name until the statement `s` ends; `using a, b = c;` makes them to
  the end of the function. `asm` takes an instruction, or several in `{ }`: `local` and a definition, or a
  token that is an `opname` of `pr_opcodes[]` exactly and its operands up to a `;`, which are
  code generation's (FTEF-19.63); any other token is `E0511` "Bad op code name NAME"
  (`ERR_BADOPCODE`). `thinktime e : t;` looks up `nextthink` and `time` by name when the target
  has no `OP_THINKTIME` (`QCC_PR_GetSRef(NULL, "nextthink", ...)`), "Unknown value
  \"nextthink\"" when there is none: on the standard target, and on every target but
  `QCF_HEXEN2` and `QCF_UHEXEN2` in this profile, which takes the other targets' opcode sets for
  code generation's (L-01); that `e` must be an entity and `t` a float ("thinktime type
  mismatch") is the type checker's (FTEF-19.63). By default `loop`, `until`, `state`,
  `thinktime` and `using` are off, so only their `__` spellings start these statements.
- **FTEF-20.6** **Withdrawn** (P2-131, #568): it found a type by its exact name only, and left
  the types the program defines and a `for` initialiser's types out. Replaced by FTEF-20.14.
- **FTEF-20.7** `QCC_PR_ParseDefs` reads qualifiers before a type in a loop, each with its
  keyword: `typedef`, `extern`, `auto`, `register`, `static`, `shared`, `const`, `var`,
  `nonstatic` (only outside a function: `!pr_scope && QCC_PR_CheckKeyword(keyword_nonstatic,
  "nonstatic")`), `unused` or `noref`, `used`, `nosave`, `strip` or `ignore`, `inline`, `wrap`,
  `weak`, `accumulate` and `deprecated` (`QCC_PR_CheckKeyword(false, "deprecated")`). So with
  its keyword off a qualifier is a type's name, `#pragma keyword disable const` making `const
  float x = 1;` "\"const\" is not a type", and with it on a qualifier off by default is one,
  `#pragma keyword enable unused` making `unused float x;` a definition.
- **FTEF-20.8** `QCC_PR_ParseType` reads, each with its keyword, `const` (with "ignoring
  unsupported const keyword") and `volatile` before a type and `const` after a `*`
  (FTEF-2.11); then `accessor` (FTEF-10.1); `class` only with `qcfuncs` on (`if (flag_qcfuncs &&
  QCC_PR_CheckKeyword (keyword_class, "class"))`, at its token, FTEF-12.12); `enum`, `enumflags`,
  `union` and `struct`; and then a name `QCC_TypeForName` finds or the C type words
  (FTEF-20.9). With its keyword off each is a type's name, so `#pragma keyword disable struct`
  makes `struct s { float a; };` "\"struct\" is not a type" while `__struct s { ... };` defines
  the struct.
- **FTEF-20.9** The loop over C type words (FTEF-2.8) takes each word with its keyword in force
  at the type: `register`, `signed`, `unsigned`, `long`, `int` or `integer`, `short`, `char`,
  `_Bool` (always), `float` and `double`. `_Bool` is `__bint` with `keyword_int` on and
  `__bfloat` with it off (`if (keyword_int) type = type_bint; else type = type_bfloat;`), and
  `longbits` is 32 under `ILP32` and 64 otherwise (`#define longbits (flag_ILP32?32:64)`,
  FTEF-12.12), so with `#pragma flag enable ILP32` `__long l;` is `__int32`. The type the words
  make is the one FTEQCC registers, named as `QCC_PR_NewType` names it (`__int32`, `__uint64`,
  `__int16`, `__bfloat`), which the semantic model compares and writes in messages (TypeName).
  So `#pragma keyword enable long` makes `long l;` an `__int64` and `long int l;` an `__int32`.
- **FTEF-20.10** `QCC_PR_ParseFunctionType` reads a parameter's qualifiers with their keywords:
  `optional` (`keyword_optional`, on by default) and `inout`, `out` and `in`
  (`keyword_inout`, off by default, so only `__inout`, `__out` and `__in`).
- **FTEF-20.11** `QCC_PR_ParseEntClass` reads a class member's words with
  `QCC_PR_CheckKeyword(1, ...)`, always on: `nonvirtual`, `static`, `const`, `var`, `virtual`,
  `ignore`, `strip`, `inline`, `get` and `set`, and `public`, `private` and `protected` before a
  `:`, so `__STATIC` is `static` there and `STATIC` under `caseinsens`. `QCC_PR_ParseRefValue`
  takes `this` and `super` as the intrinsics of FTEF-19.16 only with `keyword_class` on
  (`else if (keyword_class && !strcmp(name, "this"))`), and with it off they are names like any
  other, "Unknown value" when no definition has them.
- **FTEF-20.12** `QCC_PR_ParseDefs` takes a `;` right after a declaration's type and defines
  nothing (`if (QCC_PR_CheckToken (";")) { if (istypedef) ...; return; }`, "type (%s) with no
  name" commented out), so `float;`, `const float;` and, in a body, `enum { A, B };` and `struct
  s { float a; };` are definitions of no name; `typedef;` and `typedef float;` are FTEF-19.9's.
- **FTEF-20.13** The reacc keywords `CodeSys`, `function`, `objdata`, `object`, `pfunc`,
  `system`, `real`, `external` and `exit` are `#define`d as `flag_acc` (`#define keyword_codesys
  flag_acc //reacc needs this`), which only `-Facc` and `-std=reacc` set. Their syntax is
  reacc's, which this profile does not read (FTEF-19.63): it reads the reacc keywords as off,
  so after a type at file level `__function` and in a function's initialiser `__external`,
  which `QCC_PR_CheckKeyword` would take, are names, where the pinned FTEQCC reads reacc's
  function and builtin syntax; `__exit` is FTEF-20.5's.
- **FTEF-20.14** In a body a statement defines locals when its first token is a name
  `QCC_TypeForName` finds, not followed by `::` (`strncmp(pr_file_p, "::", 2)`, so with the `::`
  touching the name), or is exactly one of the words `QCC_PR_ParseStatement` tests with
  `STRCMP` with its keyword on: `var`, `noref`, `string`, `float`, `double`, `entity`, `vector`,
  `integer`, `unsigned`, `signed`, `long`, `int`, `short`, `char`, `_Bool` (always),
  `register`, `volatile`, `static`, `class`, `struct`, `union`, `enum`, `extern`, `auto`,
  `typedef` and `const`; or is a `.`. No `__` spelling and no other case is among the `STRCMP`
  words, so `__typedef float t;` and `__long x;` in a body are values, "Unknown value", and
  `typedef` there starts a typedef only as written. `QCC_TypeForName` searches the table of the
  names `QCC_PR_NewType` registers (`void`, `float`, `vector`, `string`, `entity`, `__double`,
  `__int`, `__int32`, `__uint`, `__uint32`, `__int64`, `__uint64`, `__int16`, `__uint16`,
  `__int8`, `__uint8`, `__bint`, `__bfloat`, `__variant`, `variant`, `__string`, `__vector`
  and `__entity`) and of the types the program made, a `typedef`'s name and the tag of a
  `struct`, `union`, `enum`, `class` or `accessor`, a local `typedef`'s in its function only
  (`t->scope == pr_scope`), and under `-Fcaseinsens` it searches in any case
  (`pHash_Get = &Hash_GetInsensitive`, FTEF-19.29): `Float x;`, `VECTOR v;`, `Entity e;`,
  `String s;`, `FLOAT x; x = 1;`, `__Int x;`, `Float;` and `ENTITY:;` are definitions, the last
  `":" - not a name`, and `float Float; ... Float = 1;` is `"=" - not a name`, and a
  `typedef float Foo;` makes `Foo = 1;` that error. A name that is
  such a type starts a definition whatever follows it, `void;` and `__variant;` among them
  (FTEF-20.12), and a name that is none is a value. The `for` initialiser is no statement:
  `QCC_PR_ParseStatement_For` reads each of its items, separated by `,`, with
  `QCC_PR_ParseType (false, true, false)`, which finds those names, the C type words with
  `QCC_PR_CheckKeyword` (FTEF-20.9, in any case under `caseinsens`), so `for
  (Integer i; ; )` defines `i` under `-Fcaseinsens` and `for (Float i = 0; ...)` defines it,
  while a name it finds nothing for is the first item of an expression. In `sizeof (NAME)` and
  a cast's parentheses such a name is a type, and with `caseinsens` off the same name in
  another case is a value, "Unknown value".
- **FTEF-20.15** A `typedef` and a struct's or union's tag a function's body declares last to
  the end of its block, and are no type after it: `{ { typedef float t; } t x; }` reads `t` as
  no type, `{ struct S { float a; }; S x; }` defines `x`, and `S y;` after the body is "\"S\" is
  not a type". An enum a body declares makes its values constants of the function in FTEQCC, not
  seen after the body (`float x = A;` there is "Unknown value \"A\"."); this profile divergently
  declares a body's enum as one at file level, its values and name seen after the body too,
  since the file's enum checks (the legacy base form, a name another type has, FTEF-19.10) read
  it there.

## FTEF-21 Flags that change in the middle of a file

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseDefs` (the `extern` string branch, `ignoreptr`, the
`flag_qcfuncs`, `flag_assumevar`, `flag_attributes` and `defaultstatic` reads),
`QCC_PrioritiseOpcodes`, `QCC_PR_RefExpression`, `QCC_PR_RefTerm` (`!` and `~`),
`QCC_PR_ParseStatement` (a block's end), `QCC_PR_GetDef` (`pr_subscopedlocals`),
`QCC_PR_ParseEnum`; `qcc_pr_lex.c`, `QCC_PR_Precompiler` (`flag`, `keyword`, `noref`, `nosave`,
`defaultstatic`), `QCC_PR_ParseType` and `QCC_PR_ParseFunctionType` (their `flag_qcfuncs`
reads); `qccmain.c`, `compiler_flag[]`, `QCC_PR_BeginCompilation` and
`QCC_SetDefaultProperties`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Every
place the pinned FTEQCC changes a flag the lexer, the parser or the definitions read is a row of
the table "Where the flags change" of [`inventory-l2.md`](inventory-l2.md).

FTEQCC keeps its compiler flags in globals that `QCC_PR_Lex` and the parser read as each token
is lexed and parsed: `#pragma flag` changes them between two tokens (FTEF-12.11, FTEF-12.12), and
so does `QCC_PR_ParseDefs`, which sets some of them for an `extern "C"` block and restores them
after it. The preprocessor cannot know the second: the block is the parser's.

### Behaviour

- **FTEF-21.1** Each token is lexed, parsed and its definitions made with the flags in force when
  FTEQCC looks at it: a file starts with the preprocessor's (the command line's, then each
  pragma's before it, from file to file), each `#pragma flag` or `keyword` sets only the flags it
  names from the token after it (`*compiler_flag[f].enabled = st;`), and the parser replays them
  with the changes its `extern` blocks make, at one token in FTEQCC's order: a block's restore
  (before the token is lexed), a block's start (before `QCC_PR_Lex` lexes the token after its
  string), then the pragmas in the white space before the token, which `QCC_PR_Lex` runs as it
  lexes it. The flags kept are those the lexer reads (FTEF-12.12), `cpriority`, `assumevar`,
  `subscope`, `attributes`, `ifvector` and `vectorlogic` (FTEF-30.2), with the operator
  priorities (FTEF-21.4) and the defaults of FTEF-21.14; a pragma that names none of them marks nothing, and the others are FTEF-21.20's.
  A flag read while the parser looks at a token, as FTEQCC reads one after it has lexed the token
  after what it read, is the flag at that token: `subscope` at a block's end is read at the token
  after its `}`. The token after an `extern` block is lexed with the block's flags and read with
  them restored, the pragmas before it applied first and the restore after them, so a pragma
  there that names one of the six saved flags is undone for the reading and holds for the
  lexing.
- **FTEF-21.2** **Withdrawn** (P2-130, #567): it joined every string beside the block's, compared
  the whole of it, took a `;` before the end of the text or a `}` for a definition, and gave
  the blocks no nesting level. Replaced by FTEF-21.17.
- **FTEF-21.3** At a block's end FTEQCC restores the six flags it saved where it started
  (`flag_qcfuncs = old_qcfuncs; flag_cpriority = old_cpriority; flag_assume_integer =
  old_assumeint; flag_assume_double = old_assumef64; flag_assumevar = old_assumevar;
  pr_subscopedlocals = old_subscope;`, "FIXME: reset all flags+keywords?"), so a `#pragma flag`
  in the block that names one of them holds only to its end, and one that names any other flag
  holds after it: in `extern "C" { #pragma flag enable qcfuncs }` `qcfuncs` is on to the `}` and
  off again before it as before the block. Blocks nest, each restoring what it saved, the inner
  first.
- **FTEF-21.4** The operator priorities are `QCC_PrioritiseOpcodes`', which reads `flag_cpriority`
  when it runs: once a compile starts, from `QCC_PR_BeginCompilation` after the command line is
  read (`-Fcpriority`, `-std=c`), and at each `#pragma flag` that names `cpriority`, at once
  (`if (compiler_flag[f].enabled == &flag_cpriority) QCC_PrioritiseOpcodes();`). Nothing else runs
  it: an `extern "C"` block sets `flag_cpriority` and leaves the priorities as they are ("we don't
  currently change operator precedence"), a pragma in the block that names `cpriority` sets them
  for the rest of the compile though the block restores the flag, and the priorities hold into
  the second autoprototyping pass, which `QCC_SetDefaultProperties` starts without
  `QCC_PR_BeginCompilation`.
- **FTEF-21.5** With `cpriority` on `QCC_PrioritiseOpcodes` gives each class of `pr_opcodes` C's
  priority: `priority_class[PC_UNARYNOT] = 1; priority_class[PC_MULDIV] = 2;
  priority_class[PC_ADDSUB] = 3; priority_class[PC_SHIFT] = 4; priority_class[PC_RELATION] = 5;
  priority_class[PC_EQUALITY] = 6; priority_class[PC_BITAND] = 7; priority_class[PC_BITXOR] = 8;
  priority_class[PC_BITOR] = 9; priority_class[PC_LOGICAND] = 10; priority_class[PC_LOGICOR] =
  11; priority_class[PC_TERNARY] = 12; priority_class[PC_STORE] = 13;`. The classes are the
  opcodes' (`*`, `/`, `%`, `*^` and `><` `PC_MULDIV`; `+` and `-` `PC_ADDSUB`; `<<` and `>>`
  `PC_SHIFT`; `<`, `<=`, `>` and `>=` `PC_RELATION`; `==`, `!=` and `<=>` `PC_EQUALITY`; `&`, `^`
  and `|`; `&&`; `||`; the assignments `PC_STORE`), so `a == b & c` is `(a == b) & c`, `!a == b`
  is `(!a) == b` (`NOT_PRIORITY` is 1, the term and its calls), `a || b ? c : d` is `(a || b) ? c
  : d`, and `x = a ? b : c` assigns the conditional. With it off they are QuakeC's (FTE-19.3,
  FTE-8.4).
- **FTEF-21.6** `!` and `~` read their operand with `EXPR_WARN_ABOVE_1`, which every right operand
  of a binary operator in it is read with too, and an expression in parentheses, an argument and
  a subscript are not, so each binary operator the operand takes is the warning F304 "suggest
  parenthesis for unary operator that applies to multiple terms" (`if (priority > 1 && exprflags &
  EXPR_WARN_ABOVE_1)`, `WARN_UNARYNOTSCOPE`, on by default, `W0227`), given at the token after its
  right operand: with QuakeC's priorities `!a == b`, `!a * b`, `~a & b` and `!(a) == b` warn, and
  `!a && b`, `!(a == b)` and `-a == b` do not; with C's none does, the operand being a term.
- **FTEF-21.7** With `qcfuncs` off `QCC_PR_ParseType` takes no parameter list (`if (flag_qcfuncs &&
  QCC_PR_CheckToken ("("))`), so `void()` is no type where a type is read alone, a parameter's,
  a member's or a cast's, and `QCC_PR_ParseDefs` reads a declaration's type without its `*`s
  (`QCC_PR_ParseType (false, hadmodifier, !flag_qcfuncs)`, `ignoreptr`), which each declarator
  reads: `float *a, b;` makes `a` a pointer and `b` a `float`, where with it on both are pointers.
  Each declarator then reads `*`s, and parameter lists before its name in a loop (`while(
  QCC_PR_CheckToken ("("))`), each a function returning the type so far, which the type took
  before with `qcfuncs` on, so `.void() think;` declares a function returning `.void`, not a
  field, and a list may stand before any declarator's name whatever the flag (`float a, (float
  x) g;`); and C's function pointer `(*name)` or `(*name[N])` and a parameter list, a function
  type with a pointer to it for each `*` past the first, a variable (`isvar |= isptr>0;`) as
  every declarator after it in the declaration, but in a `typedef`. Where the type took a list,
  its first declarator's `(*` is a list `QCC_PR_ParseFunctionType` refuses ("expected function arg
  list"), so `float (*fp)(float);` is an error with `qcfuncs` on.
- **FTEF-21.8** With `qcfuncs` off `QCC_PR_ParseFunctionType` reads `()` as variadic (`if
  (!flag_qcfuncs) ftype->vargs = true; //'void name()' is vargs/undefined in C89`), so `void h();`
  is `void(...)`; a first parameter that is no type, as an `int` named by the word (`if
  (!flag_qcfuncs) t = type_integer; //c89 assumes int.`), so `void h(x);` is `void(__int32 x)`,
  where a later one is "\"y\" is not a type", the parser reading a lone name there as such a
  name, as it cannot tell one a typedef names, which FTEQCC reads as an unnamed parameter of that
  type, a deliberate divergence; and an array parameter `[N]` as a pointer ("just
  turn it into a pointer and ditch the top-level size. C style."), as `[]` is whatever the flag
  ("length omitted. just treat it as a pointer"), where with it on `float a[2]` is a `float`.
- **FTEF-21.9** `QCC_PR_ParseDefs` reads `[[` among the qualifiers after one of them, `typedef`
  among them, or with `attributes` on outside a function (`else if
  ((hadmodifier||(flag_attributes && !pr_scope)) && QCC_PR_CheckToken("["))`), then
  `QCC_PR_Expect("[")`, attributes and `,`s up to a `]`, and `QCC_PR_Expect("]")`, and counts it a
  qualifier. Its attributes, each name read with `QCC_PR_CheckName`: `alias(NAME)` or
  `alias("NAME")`, `accumulate`, `eraseable`, which is `noref`, and `__deprecated` with an
  optional `("reason")` (`QCC_PR_CheckKeyword(false, "deprecated")`, so a bare `deprecated` is
  none), whose argument is a string or the error "expected string". Any other token is F321
  "Unknown attribute \"NAME\"" (`WARN_GMQCC_SPECIFIC`, `W0228`), and the tokens up to the next
  `]` or `,` are passed over, so `[[foo(1, 2)]]` warns of `foo` and of `2`; an `accumulate` after
  one is F321 "accumulating an accumulating accumulation". A definition given `alias(TARGET)` is
  made with no lookup of its name (`QCC_PR_DummyDef`) and refers to the definition `TARGET` names
  in its scope, "TARGET not yet defined, cannot create NAME as an alias" (`E0234`) when none does;
  it may not be initialised, "alias NAME may not be initialised" (`E0235`), and its type may be
  another's; `alias` counts among the storage classes of FTEF-19.16. With `attributes` off a
  file-level `[[` is "\"[\" is not a type", and in a body `[[` is an expression's.
- **FTEF-21.10** `QCC_PR_ParseDefs` tests `extern` against the declaration's type, once it is read
  (`if (externfnc && !pr_scope && basetype->type != ev_function) { if (flag_qcfuncs)
  QCC_PR_ParseWarning(WARN_IGNOREDKEYWORD, "extern keyword may only apply to qc functions.");
  externfnc=false; }`): on a type that is no function's, a C-style declarator's `void
  h(float x)`, a left out type's and a typedef's of what is no function included, the `extern`
  is dropped, so the function is no
  external one and needs a body ("function NAME has no body"), with the warning only when
  `qcfuncs` is on. With `qcfuncs` off a C function declared in a body is a global constant
  whose body is elsewhere (`if (!flag_qcfuncs && pr_scope) isconstant = externfnc = true; //in C,
  a locally defined function refers to an external one.`), made in the global scope
  (`externfnc?NULL:pr_scope`), so `void h();` in a body declares the global `h`.
- **FTEF-21.11** With `assumevar` on an initialised global is no constant (`(isinitialised &&
  !flag_assumevar)`, "initialised things are assumed to be consts, unless assumevar is
  specified"), read once the `=` is: so a name that initialises another global is
  "initializer for 'NAME' is not constant" (`E0214`), and `const` still makes a constant.
- **FTEF-21.12** With `subscope` on at a block's end FTEQCC removes the locals made in the block
  from `localstable` (`if (pr_subscopedlocals) { //remove any new locals from the hashtable.`), so
  after it their names are the global again or "Unknown value", and a local whose name a local
  already has is a new one: `QCC_PR_GetDef` passes the old one over for a type mismatch ("assume
  it was defined in a different subscope") and, but for a `static`, for any other ("subscopes mean
  that the later one replaces the first, hopefully."), with no "duplicate definition ignored", and
  from there the name is the new one, the newest in the table first. A local is found from where
  it is made. A `for` statement's locals leave scope whether the flag is on or off (FTEF-19.19).
- **FTEF-21.13** With `assumeint` on where `QCC_PR_ParseEnum` starts an enum's base type is
  `__int32` (`basetype = (flag_assume_integer?type_integer:type_float);`), so its names are
  integers, which a string may be initialised with, with the warning of FTEF-19.31.
- **FTEF-21.14** `#pragma noref N`, `#pragma nosave N` and `#pragma defaultstatic N` set the
  defaults `QCC_PR_ParseDefs` starts each definition with (`pbool isstatic=defaultstatic; ...
  pbool noref = defaultnoref; pbool nosave = defaultnosave;`, `!!atoi(msg)`), from the token
  after them, as their marks hold them from file to file; `QCC_SetDefaultProperties` sets
  `defaultstatic = 0;` before the second autoprototyping pass and leaves the others. Under
  `defaultstatic` every global is `static` (FTEF-19.12) but one marked `nonstatic`, which only
  outside a function undoes it, and every local is a `static` local. A `typedef`'s `static`,
  `noref` and `nosave` are no "bad combination of modifiers with typedef" when the default is on
  (`||(isstatic && !defaultstatic) ||(noref && !defaultnoref) ||(nosave && !defaultnosave)`),
  `auto`, `register` and `nonstatic` never are, and under `defaultstatic` a typedef has two storage
  classes, "Conflicting storage classes.". `QCC_PR_ParseDefs` reads `typedef` among its other
  qualifiers, before or after them, and after it, a modifier, a type may be left out
  (FTEF-19.21): `typedef static x;` names an `__int32` and is the error.
- **FTEF-21.15** **Withdrawn** (P2-122, #558): it said the target's `flag_undefwordsize` and the
  casts `lax`, `typeexplicit` and `qccx` allow are not modelled, but the semantic model's casts
  and conversions read them (FTEF-22). Replaced by FTEF-21.18.
- **FTEF-21.16** The flag family of `cargo xtask oracles builtins`
  (`xtask/src/oracles/builtins/flags.rs`) compiles programs that bind `f` and then use each of
  these statements, under the FTEQCC profile, with no option, with `-Werror -Wno-Q208` for those
  a warning decides, and with the command-line options that set the flags. Its run of
  2026-10-21 (macOS arm64) agrees with the pinned FTEQCC on every case.
- **FTEF-21.17** `QCC_PR_ParseDefs` reads `extern` as its first qualifier, with its keyword, then a
  string (`if (!hadmodifier && pr_token_type == tt_immediate && pr_immediate_type==type_string)`),
  as a block: a `{`, definitions up to a `}` (`QCC_PR_ParseDefs(NULL, false)` each), or with no
  `{` one definition. The string is the one token the lexer read, whose parts
  `QCC_PR_LexString` joined as any string's (`"" "C"` is `C`); a string beside it that comes
  from another text is another token, a macro's and the source's or two macros', so with `#define
  C "C"` the program `extern C C { string s = "\9"; }` is the block `"C"` and a definition that
  starts with the string `"C"`, `"C" is not a type`, where `#define C "C" "x"` and `extern C {
  ... }` is the block `"Cx"`. It is compared with `strcasecmp(pr_token, "C")` and
  `strcasecmp(pr_token, "QC")`, so in any case, and only up to its first NUL, where `pr_token`
  ends: `"C\x00x"` is `"C"`, and under `qccx` so is `"C":"x"`, whose `:` the lexer writes as a
  NUL; `"C"` turns `qcfuncs` off and
  `cpriority`, `assumeint`, `assumevar` and `subscope` on (`flag_qcfuncs = false; flag_cpriority
  = true; flag_assume_integer = true; ... flag_assumevar = true; pr_subscopedlocals = true;`),
  `assumedouble` left as it is ("not forced, too annoying"); `"QC"` turns `qcfuncs` on, the others
  "refrain from changing it"; any other string changes nothing, and the block is read all the
  same. Each of its definitions is a `QCC_PR_ParseDefs` call, whose first act is `while
  (QCC_PR_CheckToken(";"))` and whose second is to read a type, so the `;`s may stand before
  each definition and after none: a block or an `extern "STRING"` that reaches the end of the text
  with no definition is the error `"" is not a type` (`extern "C"`, `extern "C";`, `extern "C"
  ;;;`, `extern "C" "QC"`), and one that reaches a `}` is `"}" is not a type` (`extern "C" { ; }`,
  `extern "C" { float y; ;; }`); a `;` after a definition at the end of the text is the same
  (`float x;;` and `void() g = {};;` at file level, where the file's loop tests for the end of
  the text only before a call). The flags hold from the token after the string, which
  `QCC_PR_Lex()` lexes once they are set, to the token after the block's last, which
  `QCC_PR_CheckToken("}")` or the definition's end lexes before they are restored: so in
  `extern "C" { float y; } string s = "\9";` the word `string` is lexed with `qcfuncs` off and
  `"\9"` with it on, and the definition `string` starts is read with the flags restored
  (FTEF-21.1), so `extern "C" int a; enum { A };` names a `float` enum. A file that ends in the
  block is "expected }" (`if (pr_token_type == tt_eof) { QCC_PR_Expect ("}"); break; }`). After
  another qualifier the string is no block (`const extern "C" {` is "\"C\" - not a name" after
  the warning of FTEF-21.10). In a body `QCC_PR_ParseStatement` reads `extern` exactly, or
  `local` and then `extern`, as a definition, so the block's definitions are locals, and `x = 1;`
  in it is no definition; a `typedef` in the block is a definition of it as at file level. A
  qualifier alone, `static;`, `const;` or `const *x;`, is a definition of an `int` of no name:
  the type is left out where the token after the qualifiers is no name (FTEF-19.21). The block is
  one level of nesting (FTEF-18.32). The parser lexes the text again with the blocks it read, and
  reports the lexical errors and warnings of the tokens they lex otherwise, in place of the
  preprocessor's.
- **FTEF-21.18** **Withdrawn** (P2-06, #587): it listed `ifvector` and `vectorlogic` among the
  flags the semantic model does not read, but a condition's test reads them (FTEF-30.2).
  Replaced by FTEF-21.20.
- **FTEF-21.19** A `break` that no loop or `switch` holds, a `continue` that no loop holds, and a
  `case` or `default` label that no `switch` holds, are counted (`num_breaks`, `num_continues`,
  `num_cases`) and left over at the function's end: "%s: function contains illegal breaks", "%s:
  function contains illegal continues" and "%s: function contains illegal cases"
  (`QCC_PR_ParseImmediateStatements`): `void() g = { break; };`, `{ continue; }`, `{ switch (1) {
  case 1: continue; } }`, `{ while (1) { } break; }`, `BREAK;` under `-Fcaseinsens`, `{ case 1: ;
  }`, `{ default: ; }`, `{ if (1) { case 1: ; } }` and `{ switch (1) { case 1: ; } case 2: ; }`
  are errors, a `break` in a `switch`, `loop` or `until`, a `continue` in a loop and a label in a
  `switch` are not. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseImmediateStatements`, `QCC_PR_ParseStatement`.
- **FTEF-21.20** The flags a pragma may set mid-compile that no token, tree, definition or cast
  of the semantic model reads are not modelled, each for its reason: `nosave`'s default but for
  a typedef, and `optimise`, `opcode`, `iffloat`, `ifstring`, `nullemptystr`, `boundchecks`,
  `fastarrays`, `lo`, `reciprocal-math`, `allowuninit`, `omitinternals`, `embedsrc`, `dumpfilenames`, `annotate`, `brokenarray`, `rootconstructor`
  and `pythonlogic`, which the compiler's output reads (P2-06 onward); `msvcstyle`,
  `debugmacros` and `verbose`, which change only what FTEQCC prints; the target's
  `flag_pointerrelocs`, which `#pragma TARGET` sets with `QCC_OPCodeSetTarget` and only an
  initialiser that takes an address reads ("preinitialised pointer variables is disabled for
  this target", `QCC_PR_GenerateInitializerType`), an expression FTEF-19.63 leaves untyped; the
  dereference of a function `*f`, which `qcfuncs` makes "attempt to dereference function type."
  in `QCC_PR_RefTerm`, `vargtodouble`, and the `__int32` that `&&` and `||` of constants fold to
  under `assumeint`, the expression typing's and the compiler's; under `qccx` the field access
  `e^[f]` and `e[f]` of `QCC_PR_ParseField`, which only the expression's type, an entity's, tells
  from `^` and a vector or an index, so the parser reads the first as `^` and the second as an
  index, both accepted as FTEQCC accepts them; and `lax`, `typeexplicit`, `qccx` and
  `flag_undefwordsize` where an expression's operators, assignments, conditions and pointer
  arithmetic read them (`QCC_PR_Statement`'s "type mismatch for OP",
  `QCC_SupplyConversionForAssignment`, `QCC_PR_GenerateLogicalTruth`, `sizeof`), the expression
  typing's (P2-06).
- **FTEF-21.21** With `qcfuncs` off a C function a body declares (FTEF-21.10) is defined by a
  body written with it, `void x() {}`, and needs none elsewhere; C's `(*name)(params)` there is
  a variable (`isvar |= isptr>0`), no function, so `float (*x)(float); x(1);` and `entity
  (*x)();` are accepted. After the `)` of `(*name)` the declarator's loop reads further parameter
  lists (`float (*x)(float)(float);`). A `const` C function pointer is both, "Conflicting
  constness." (`isconstant+isvar>1`), and a constant, a function needing a body. A function's
  parameters are those of the last list before its name when the type takes none, `void(float
  x) h = { x = 1; };`, and so for a struct's or a typedef's parameter type.

- **FTEF-21.22** A function type whose first parameter's type is a name that is no type, through
  `*`s, makes `QCC_PR_ParseFunctionType` return none, and its caller the fatal error `E0204`
  "expected function arg list" (`if (!type) QCC_PR_ParseError(ERR_BADNOTTYPE, "expected function
  arg list");`): `void(foo a) g;`, `float (x);` and `typedef float (t);`; a later parameter's is
  `E0203` "\"foo\" is not a type", as a return type's is.
- **FTEF-21.23** The illegal `break`, `continue` and `case` errors of FTEF-21.19 name the
  function, `QCC_PR_ParseError(ERR_ILLEGALCASES, "%s: function contains illegal cases",
  f->name)`, a member function's name being `CLASS::NAME`: `void() g = { break; };` is "g:
  function contains illegal breaks", once for the function however many it holds.
- **FTEF-21.24** The `break`s and `continue`s FTEF-21.19 counts are those left in `num_breaks`
  and `num_continues` when the function ends, after an `if` arm of a constant condition is
  stripped (`QCC_UngenerateStatements(oldnumst)` forgets the `pr_breaks` and `pr_continues` at
  or after it): `if (0) { break; }` leaves none and is no error. The continues are checked before
  the breaks, each a fatal error. The function's name in its end-of-function messages, these and
  FTEF-34.6's "not all control paths return a value" and "contains unreachable code", is
  `pr_scope->name`, a member function's `CLASS::NAME`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseImmediateStatements`, `QCC_UngenerateStatements`.
## FTEF-22 The flags and the target the casts read

**Source:** `qcc_pr_comp.c`, `QCC_TryEvaluateCast`, `QCC_EvaluateCast`, `QCC_SupplyConversion`,
`QCC_ShouldConvert`, `QCC_OPCodeSetTarget`, `QCC_OPCodeSetTargetName`, `QCC_PR_ParseEnum` and
`QCC_PR_ParseState`; `qcc_pr_lex.c`, `QCC_PR_Precompiler` (`TARGET`, `flag`); `qccmain.c`,
`compiler_flag[]`, `QCC_PR_CommandLinePrecompilerOptions` (`-T`, `-F`, `-std=`),
`QCC_SetDefaultProperties` and `QCC_PR_FinishCompilation`; `qcc_cmdlib.c`, `basictypenames`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. The rows of
[`inventory-l3.md`](inventory-l3.md) name each function and its Rust counterpart.

The casts of FTEF-19.31 and the conversions of an enum's value and a state frame (FTEF-19.10,
FTEF-19.14) read three of the flags `#pragma flag` and the command line set in the middle of a
file, `lax`, `qccx` and `typeexplicit`, and one the output target sets, `flag_undefwordsize`;
`QCC_PR_FinishCompilation` reads the target itself. The preprocessor keeps the four flags with
the others (FTEF-21.1) and the target as it reads the files (FTEF-20.4).

### Behaviour

- **FTEF-22.1** With `lax` on (`flag_laxcasts`, `#pragma flag enable lax`, `-Flax`) a cast
  `QCC_TryEvaluateCast` refuses is made by `QCC_EvaluateCast` with the warning `W0229` "Implicit
  lax cast from A to B" (`QCC_PR_ParseWarning(0, ...)`, warning 0, `WARN_DEBUGGING`, which no
  name sets and `-Werror` makes an error), in place of "Cannot cast" (`r.cast = cast;
  //decompilers suck`): `float x = "a";`, `void() g = 5;`, `.float fl = 1;`, `string s = '1 2
  3';`, a default value `float a = "a"` and a local's initialiser cast so, and a strict enum's
  value too, `float x = E::A;` being "Implicit lax cast from enum E:float to float".
- **FTEF-22.2** With `qccx` on `QCC_TryEvaluateCast` adds a float to an entity or a string, and
  an entity or a string to a float, to the casts "acceptable but probably an error" (`||
  (totype == ev_entity && src.cast->type == ev_float && flag_qccx)` and the three like it),
  with `W0220`: `entity e = 1;`, `entity e = 1.5;`, `string s = 1;`, `float x = "a";` and a
  local entity or string initialising a float warn, where without it they are "Cannot cast".
  An integer to an entity warns either way, and a double to an entity and a string to a vector
  are refused either way. `-Fqccx` and `-std=qccx` also ignore F314 (FTEF-14.72), a `#pragma
  flag enable qccx` not.
- **FTEF-22.3** An enum's value and a state frame are converted by `QCC_SupplyConversion(var,
  wanted, true)` to the enum's base type and to a float. `QCC_ShouldConvert` needs no
  conversion for a value of the kind, among the 32-bit and among the 64-bit integers, and for
  an integer to a function or a pointer; it converts among the numbers but between a double and
  a 32-bit integer, a float to a vector, and a pointer to a float or an integer to an integer or
  a float by what it points to; anything else none converts. With `typeexplicit` on any value
  that is not of the kind already is the error `E0236` "Automatic type conversions disabled.
  Needed A, got B." (`if (flag_typeexplicit)`, before the test for none, so `enum { A = 0i };`,
  `enum { A = '1 2 3' };` and `void() g = [1i, g] {};` are errors, and `enum { A = 1 };` is
  not); `QCC_EvaluateCast` reads no `typeexplicit`, so `float x = 1i;` compiles. A value none
  converts is the error `E0227` "Implicit type mismatch. Needed A, got B.", or with `lax` on the
  warning `W0230` of that text (`WARN_LAXCAST`, F314), the value kept as it is: an enum's name
  then has the value it would have had with no `=`, as `QCC_PR_ParseEnum` reads the value of a
  number only (`enum { A = "a", B };` makes `B` 1), and a frame of a cycle `[++ a .. b]` is then
  the error `E0238` "state type mismatch", once, after both frames are converted (`if
  (s1.cast->type != ev_float || def.cast->type != ev_float)`). `E0236` and `E0227` are fatal,
  so a state header's first one is its last word. A and B are `basictypenames`' (`integer`, `uint`, `long`, `ulong`, `double`), so
  under `assumeint` `enum { A = "a" };` is "Implicit type mismatch. Needed integer, got
  string.". A name's value is converted by its definition's type, before its constness is read,
  so a `string` constant initialising an enum's name is `E0227` and a variable `float` "is not a
  compile-time constant".
- **FTEF-22.4** `QCC_OPCodeSetTarget` sets `flag_undefwordsize` to false, then to true for
  `QCF_DARKPLACES` only (`flag_undefwordsize = true; //DP insists on using word-aligned
  pointers`). It runs for each `#pragma TARGET` that names a target (`dp`, `darkplaces`, `dp_1`),
  at the token after it, and for each `-TNAME` among the command-line options, in their order,
  so `-Fundefwordsize -Tid` leaves it off and `-Tdp -Tfte` too, and `QCC_SetDefaultProperties`
  runs it for the standard target before the command line, also before the second
  autoprototyping pass, which so starts with it off; `-std=gmqcc` sets it, and
  `#pragma flag` and `-F` name it `undefwordsize`. With it on a string cast to a pointer is the
  non-fatal error `E0237` "pointer<->string casts were disabled." (`ERR_BADEXTENSION` given as a
  warning, FTEF-18) and the cast made, before the `char*` and `void*` casts of FTEF-19.31, so
  `void *p = "a";` is the error too; a pointer cast to a string is not, the C testing `totype ==
  ev_pointer && src.cast->type == ev_string` twice. A `#pragma TARGET` that would switch to or
  from a Hexen II target after the first statement is ignored with a warning (`numstatements >
  1`), which this profile does not model, statements being code generation's: it takes the
  target.
- **FTEF-22.5** `QCC_PR_FinishCompilation` reads the target once every file is compiled: unless
  it is `QCF_FTE`, `QCF_FTEDEBUG` or `QCF_FTEH2`, each function an `extern` made at file level
  (`def->isextern`, an `extern` `QCC_PR_ParseDefs` kept, FTEF-21.10) is the error `E0239`
  "extern is not supported with this target format", at its definition. So `extern void(float
  x) h;` is the error under `-Tid`, and no error under `-Tfte` or after `#pragma target fte`
  anywhere in the module, the last target holding. It is reported definition by definition with
  the functions that have no body (QCD-3.11), and, as those are, also when the module has other errors, which
  FTEQCC's check (`if (pr_error_count) return false;`) never reaches (QCD-1.4); the outcome is
  the same. A module the semantic model reads without its preprocessing (`Db::module_index`) has
  no target and is not checked.
- **FTEF-22.6** Each value is cast or converted with the flags at the token after it: FTEQCC
  casts it once `QCC_PR_Expression` has read it, which has lexed that token, and the pragmas
  before it with it (FTEF-21.1). So in `float x = "a"` then `#pragma flag enable lax` then `;`
  the cast is lax, and a `#pragma flag disable lax` before the next definition holds for it.
- **FTEF-22.7** The cast family of `cargo xtask oracles builtins`
  (`xtask/src/oracles/builtins/casts.rs`) compiles programs that bind `f` and then make the casts
  and conversions of these statements and FTEF-19.31 under the FTEQCC profile, the flags set by
  `#pragma flag`, `#pragma target` and the command line, and those a warning decides with
  `-Werror -Wno-Q208`. Its run of 2026-10-22 (macOS arm64) agrees with the pinned FTEQCC on
  every case.
- **FTEF-22.8** An implicit cast of a pointer to another pointer (`else if (totype ==
  ev_pointer && src.cast->type == ev_pointer)`), but to or from `void*`, is silent when what the
  source points to, or a type it derives from (`t = t->parentclass`), is the target's pointee by
  `typecmp_lax`, and else `W0220` "Implicit cast": a struct is no float, so `S *ps; float *pf;
  pf = ps;` is "Implicit cast from struct S* to float*", and `float **` to `struct S*` warns too.
  Two functions are among the casts "acceptable but probably an error", which warn only when
  `typecmp_lax` tells them apart (`if (implicit && typecmp_lax(src.cast, cast))`): by their return
  types and the parameters both have, a parameter only one has unless it is `optional`, not
  by `...` (`typecmp_lax` reads `vargcount`, not `vargs`), so `void(float) f2; void(float, ...) f4; f2 = f4;` is silent and `f2 = f3;` of a
  `void(int)` is "Implicit cast from void(__int32) to void(float)". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_TryEvaluateCast`.

## FTEF-23 Operators as FTEQCC types them

**Source:** `qcc_pr_comp.c`, `pr_opcodes[]`, `QCC_PrioritiseOpcodes`, `QCC_NumericTypeRanking`,
`QCC_canConv`, `QCC_PR_ChooseOpcode` and `QCC_PR_RefExpression`; `qccmain.c`, `QCC_main` (the
basic types); `progslib.h`, `etype_t`; `progsint.h`, `QCC_opcode_t`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

FTEQCC has no separate type rules for its operators: an operator's type is the type of the
opcode it picks for its operands. The semantic model keeps the table (`crates/qc-sema/src/check/
opcodes.rs`, the rows of `pr_opcodes[]` in order) and picks as FTEQCC picks
(`crates/qc-sema/src/check/convert.rs`).

### Behaviour

- **FTEF-23.1** The opcodes an operator may be given are the rows of `pr_opcodes[]` whose
  `name` is its spelling and whose priority class has the operator's priority, in the table's
  order: `QCC_PrioritiseOpcodes` lists each priority's rows in table order and sorts them by
  name with `qsort`, and `QCC_PR_RefExpression` starts at the first row of the spelling. The
  table has 413 rows before its `NULL` name, the opcode numbering, and every row's `opname`
  is the name `#pragma opcode` takes (FTEF-12). `sort_opcodenames` compares names only, so the
  order of a spelling's rows is the C library's `qsort`'s, which C leaves unspecified: glibc's
  keeps table order, and macOS's does not (the pinned FTEQCC on macOS arm64 takes `MUL_FI` as
  the first `*` row). The order decides only a tie of conversions (FTEF-23.4) and the row used
  after a mismatch; the semantic model takes table order, and the differential has no case
  whose outcome the order decides.
- **FTEF-23.2** `QCC_NumericTypeRanking` ranks the numeric types `double` 15, `float` 10,
  `__uint64` 6, `__int64` 3, `__uint32` 1 and `__int32` 0 ("large gap, to try to de-prioritize
  the opcodes that mix float and int types"); every other type is unranked.
- **FTEF-23.3** `QCC_canConv` gives the cost of converting a value to a basic type, after an
  accessor or a truth value is taken for its `parentclass`: 0 for the same type; 1 for a field
  holding the type, in a member function's body (`pr_classtype`); 2 for a constant 0 of an
  integer, float or pointer type (`QCC_SRef_IsNull`); between two ranked types `3 + (to - from)`
  up the ranking and `150 + (from - to)` down it; 9 to `variant` and 10 from it; 200 from a
  float to a vector and 201 from a vector to a float; else no conversion.
- **FTEF-23.4** `QCC_PR_ChooseOpcode` takes, among the candidates of FTEF-23.1, the first
  needing no conversion, else the first needing fewest: for an ordinary operator the larger of
  the two operands' costs, and for an assignment the value's cost, the target's type having to
  be the row's `type_a` exactly, and for a pointer row what it points at too (`type_pointer`
  points at `__pointeraux`, a float). A member operator's row must also hold what the field
  holds. Before choosing, an operand that is a truth value is taken for its `parentclass`, and
  two operands of one enum type for the enum's base type. When no candidate can take the
  operands it is the error `E0302` "type mismatch for OP (A and B)", a warning `W0231`
  (`WARN_LAXCAST`) with `lax` on (FTEF-22.1), and the first candidate is used.
- **FTEF-23.5** Once an opcode is chosen, an operand whose basic type is not the row's (and the
  row's is not `variant`) is converted to it, with the warning `W0232` "Implicit conversion
  from A to B" when the conversion costs 100 or more (FTEF-23.3), the left operand's before the
  right's: `v < v` for two vectors is `LT_F` with the warning twice, "Implicit conversion from
  vector to float". A, B are the types' names (`name`, not `TypeName`). The conversion is
  `QCC_EvaluateCast(operand, row_type, true)`, the implicit cast of FTEF-19.31 and FTEF-22 with
  its diagnostics: so a vector compared with `<` is then "Cannot cast from vector to float",
  and after a mismatch (FTEF-23.4) the first candidate's cast of each operand is made or
  refused, `s * a` for a string `s` being "Cannot cast from string to float" too, or under
  `lax` "Implicit lax cast".
- **FTEF-23.6** A refused cast is `QCC_PR_ParseError`: FTEQCC stops the function, skips to the
  next `;` at file level and reads on from there as declarations. The semantic model, which
  recovers on its own (QCD-1.4), stops checking the statement and checks the next one; the
  module fails to compile either way.
- **FTEF-23.7** A comparison's and a logical operator's value is of the opcode's result type,
  `type_bfloat` or `type_bint` (`EQ_F`, `NOT_F`, `AND_F`: `&type_bfloat`; `EQ_I`, `NOT_I`:
  `&type_bint`): a float of the name `__bfloat`, printed so (`string s = (f == 1);` is "Cannot
  cast from __bfloat to string"), and an `int` printed `__int32`; `_Bool` is `type_bint`, an
  `int` (`_Bool b; string s = b;` is "Implicit cast from __int32 to string").
- **FTEF-23.8** `__bint` and `__bfloat` are both `ev_boolean` (`QCC_PR_NewType("__bint",
  ev_boolean, true)`), one type to `typecmp` and `typecmp_lax` and no other type's:
  `__bint b; __bfloat b;` redeclares nothing, `_Bool b; int b;` is "Type mismatch on
  redeclaration of b. __int32, should be __bint", `TypeName` printing `__bint`. A cast to either
  is `QCC_PR_GenerateLogicalTruth`'s `!=` with zero (`NE_F`, `NE_S`, `NE_E`, `NE_V`, `NE_FNC`,
  `NE_I` for integers and pointers, `NE_D`, `NE_I64`, `NE_U64`) then that value cast to the
  boolean's `parentclass`, with no message, implicit or explicit (`else if (totype ==
  ev_boolean)`), and the value is of the parent type; `QCC_StoreSRefToRef` makes the truth again
  of a number stored into one. So on the id target `__bint c = 1.5;` folds with no overflow
  warning, `c = f;` and `c = s;` are two "ftoi emulation" warnings, `c = (_Bool)f;` three, and a
  local's initialiser `_Bool c = f;` one.
- **FTEF-23.9** An operand converted to the chosen row's type is the cast's result
  (`QCC_EvaluateCast(lhsd, (*op->type_a), true)`), of the row's type, so the opcode's own checks
  read that type: a null constant, which converts to any type (FTEF-23.3), compared with a
  string, an entity or a field is no "'==' type mismatch" (FTEF-28.2): `s == __NULL__`, `e ==
  0i` and `fld == 0` warn of nothing. An operand of the row's basic type keeps its own type, a
  class's among them. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_RefExpression`, `QCC_EvaluateCast`.
- **FTEF-23.10** A value made a boolean, cast to one or stored to one (`QCC_StoreSRefToRef`'s
  `dest->cast->type == ev_boolean`), is always `QCC_PR_GenerateLogicalTruth`'s comparison, a
  boolean value too, which it reads as its parent type: on the id target `b = b;` of a `_Bool b`
  is `OP_NE_I`'s emulation and two "ftoi emulation: denormals are unsafe", and `b = (_Bool)i;`
  three. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_GenerateLogicalTruth`, `QCC_TryEvaluateCast`,
  `QCC_StoreSRefToRef`.
- **FTEF-23.11** Under `lax` a `void` value made a boolean warns "cast to boolean"
  (`QCC_PR_GenerateLogicalTruth`'s `t == ev_void && flag_laxcasts`, its message given whole) and
  is `OP_NE_F` against 0: `bb = (_Bool)vf();` of a `void() vf` is that warning, then on the id
  target three "ftoi emulation". Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_GenerateLogicalTruth`.

## FTEF-24 Member access as FTEQCC types it

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseField` and `QCC_PR_ParseRefArrayPointer`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

FTEQCC reads `a.b` and `a->b` by the type of `a`: `QCC_PR_ParseField` for an entity (or a
type with accessors), `QCC_PR_ParseRefArrayPointer` for the rest. The semantic model types each
(`crates/qc-sema/src/check/member.rs`). Each error here is `QCC_PR_ParseError`, which ends the
statement's checks (FTEF-23.6).

### Behaviour

- **FTEF-24.1** On an entity the selector is read as a value with no member expansion
  (`QCC_PR_ParseRefValue(..., expandmemberfields false)`), or the expression in parentheses
  after `.`: a field's member is the value it holds, a `variant`'s a `variant`; any other
  definition is the error `E0318` "NAME is not a field", or "NAME is not a field of class CLASS"
  when the entity's type is a class (`t->parentclass`).
- **FTEF-24.2** On a vector `x`, `y` and `z`, or `r`, `g` and `b`, are its components, floats;
  on a `.vector` field they are `.float` fields (`type_floatfield`). `w` and `a` need a fourth
  component a vector does not have (`t->size >= 4`), and any other name is the error `E0314`
  "unsupported vector swizzle '.NAME'".
- **FTEF-24.3** On a string, `length` is the string's length, an `int`, when the string is a
  constant, else the error `E0316` "not a constant"; any other name is `E0315` "unsupported
  string method NAME".
- **FTEF-24.4** On a struct or union, and on a field of one, the member found in it or its
  parents (`QCC_PR_FindStructMember`, FTEF-6.11); on a field the member is a field of the
  member's type (`QCC_PR_FieldType`).
- **FTEF-24.5** Through a pointer, the member of the struct or union it points at; a pointer to
  any other type is the error `E0317` "indirection in TYPE - not a struct or union".
- **FTEF-24.6** On an entity in a body, a member no definition has is made by
  `QCC_PR_GetSRef(type_variant, name, pr_scope, true, ...)`, a `variant`, and reported with
  `QCC_PR_ParseWarning(ERR_UNKNOWNVALUE, ...)`, an error that does not end the statement:
  `E0319` "Unknown field \"NAME\" in class \"CLASS\"", CLASS `entity` for a plain entity and the
  class for a class's value. The member's value is a `variant`. A class's member is looked up
  as `CLASS::NAME` in the base's class and its parents, so it is a field of the member's type
  on a value of the class or one derived from it, and unknown on any other entity: `e.m_a` for
  a plain `entity e` and a member `m_a` of a class is "Unknown field \"m_a\" in class
  \"entity\"", whatever the resolver's name lookup (FTEF-17.3) offers for navigation.
- **FTEF-24.7** Messages print a type as `TypeName` (`qcc_pr_lex.c`) does: a class `class NAME`
  (an entity type with a `parentclass`), and a field of a class too, the class branch writing
  over the field's `.`; a struct or union `struct NAME` or `union NAME`; a strict enum `enum
  NAME:BASE`; a pointer `T*`; a field `.T`; any other type by its name. A function type prints
  each parameter as `[inout ][optional ]TYPE[ NAME][ = DEFAULT]`, then `...` when it is
  variadic: the default as `QCC_VarAtOffset` prints the constant, a float `%gf` (`2f`, `0.5f`), an
  integer `%#xi` (`0x3i`), a vector `'%g %g %g'`, a string quoted. The names are kept beside the
  types for messages and never tell two types apart (QC-10.10).
- **FTEF-24.8** An array's only member is `length`, an `int`, the array's size (`QCC_PR_Expect
  ("length")` after an array's `.`, FTE-13); `LightStyles.length` for an array of strings is the
  array's length, not the string's.

- **FTEF-24.9** A member no definition has that names a value FTEQCC declares as it reads it,
  an intrinsic (`randomv`, `alloca`, `entnum`, `autocvar`, `used_model`, `used_sound`, `va_arg`,
  `_`, `random`) or `nil`, is that value and no field: the fatal error `E0318` "NAME is not a
  field", `nil` printed as its constant's name, "IMMEDIATE is not a field".
- **FTEF-24.10** `QCC_PR_ParseField` reads `.` only after an entity, a value of a type with
  accessor members, a vector, a `.vector` field, a string, a struct or union, a pointer or a class;
  after a value of any other type the expression ends before the `.`. The statement goes on with
  the value it has, `b = a` of `b = a.x + 1` stored with its casts and conversions, and then the
  `.` is the fatal error `E0204` "expected ;, found ."; inside parentheses, a call's arguments or
  an index the construct expects its own token at once, "expected ), found .". A fatal error
  before it, as "Cannot cast from .float to float" for `b = fl.x`, is reported instead.
- **FTEF-24.11** Some messages print a type by its own name, `type->name`, not by `TypeName`:
  a pointer a declaration makes is `ptr` (`QCC_PointerTypeTo`), and an array's address `T*`
  (`QCC_PR_PointerType`); a function `__function` (`type_function->name`); a field `.` and its
  type's own name (`QCC_PR_FieldType`); a struct, union, class or enum its name with no keyword
  or base (`s` for `struct s`); any other type by its name. `float *p; void() g = { return p; };`
  is "'g' returned ptr, expected void".
- **FTEF-24.12** A named union is made `ev_struct` (`QCC_PR_ParseStruct`, `newt =
  QCC_PR_NewType(tname, ev_struct, true)`), so `TypeName` prints it as `struct NAME`; a struct or
  union with no tag is `struct <struct>` or `union <union>` (`QCC_PR_NewType(structtype==ev_union?
  "<union>":"<struct>", structtype, false)`); and a field of a struct or union prints its own name,
  `.NAME`.
- **FTEF-24.13** A typedef of a class is the class (`QCC_TypeForName` gives the typedef's real
  type), so a value of it has the class's members, and a member it lacks is "Unknown field
  \"NAME\" in class \"CLASS\"", CLASS the class's own name: `class C { float f; }; typedef C T;
  T t;` reads `t.f`, and `t.zz` names `C`. A class a typedef writes, `typedef class C2 { float
  f; } T;`, is declared with it.
- **FTEF-24.14** A parameter's default prints as `QCC_VarAtOffset` prints a constant of the
  parameter's type: a float `%gf`, a double `%gd`, an `int` `%#xi`, a `uint` `%#xu`, an
  `__int64` `%#xill` and an `__uint64` `%#xull` of its bits (C writes `0` for zero, so `0u`), a
  vector `'%g %g %g'` whatever form it was written in (`[1, 2, 3]` is `'1 2 3'`), a string
  quoted.
- **FTEF-24.15** A member of a class declared (`class C;`) but never defined, which has no
  `parentclass`, is the error `E0371` "Class \"C\" is not defined, cannot access member \"NAME\""
  (`QCC_PR_ParseRefValue`: `else if (!assumeclass->parentclass && assumeclass != type_entity)`)
  in place of FTEF-24.6's "Unknown field"; once the class is defined its unknown member is that
  error again. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseRefValue`.
- **FTEF-24.16** After an entity's `.`, a selector that names a class's member through its class,
  `e.CLASS::name` or `e.(CLASS::name)`, is the global `CLASS::name`, the member function's own
  body even for a `virtual` one, called with the entity as `self` (`REF_NONVIRTUAL`, for a
  `REF_GLOBAL` whose name has `::`): "too many parameters on call to C::random" names it, and no
  "is not a field of class" is given. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseField`.
## FTEF-25 The opcodes the target has

**Source:** `qcc_pr_comp.c`, `QCC_OPCodeValidForTarget`, `QCC_OPCodeSetTarget` and
`QCC_OPCodeSetTargetName`; `qcc_pr_lex.c`, `QCC_PR_Precompiler` (`opcode`),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Each opcode of `pr_opcodes[]` is valid for the output or not (`OPF_VALID`). The preprocessor
keeps the validity where each token stands (`crates/qc-preproc/src/fte_opcodes.rs`), and the
check stage reads it where it types an operator, an opcode that is not valid being emulated or
refused (`QCC_PR_StatementFlags`).

### Behaviour

- **FTEF-25.1** `QCC_OPCodeSetTarget(format, revision)` holds the revision to the format's
  highest (6614 for the FTE formats, 20250104 for DarkPlaces, 0 for QSS and the rest) and sets
  every opcode valid or not by `QCC_OPCodeValidForTarget`. It runs for the command line's last
  `-T` option, else for the standard format at revision 0, and again at each `#pragma TARGET`
  that names a target, with the revision after `_` or the row's default (5768 for `fte`, 12901
  for `dp`). So a `#pragma TARGET` undoes every `#pragma opcode` before it.
- **FTEF-25.2** `QCC_OPCodeValidForTarget`: no opcode from `OP_NUMREALOPS` (282) on is valid.
  The standard, KK7 and QTest formats have the opcodes before `OP_MULSTORE_F`; Hexen II's have
  those up to `OP_CALL8H` but `OP_SWITCH_V` to `OP_SWITCH_FNC`; the FTE formats have all of
  them by revision: `OP_PUSH` and those from `OP_STOREP_I16` from 6614, those from `OP_LT_U`
  from 5768, from `OP_STOREP_I8` from 5744, from `OP_STOREF_V` from 5698, from `OP_IF_F` from
  3349 and the rest always; QSS has id's and a list of integer, conditional, field-store, array
  and Hexen II call opcodes; DarkPlaces has id's, its shifts and unsigned opcodes from revision
  20250104, its pointer loads from 20241108, eight opcodes it once got wrong from 12901 and a
  list of integer and global-array opcodes always.
- **FTEF-25.3** `#pragma opcode enable NAME...` (or `on`) makes each named opcode valid, and
  `disable` (or `off`) not, in order, the names an `opname` in any case; an internal one is "opcode
  NAME is internal" and changes nothing, and an unknown one "opcode NAME not recognised"
  (FTEF-12). The change holds from the token after the pragma.

## FTEF-26 Constant arithmetic

**Source:** `qcc_pr_comp.c`, `QCC_PR_StatementFlags` (its folding before it emits),
`QCC_SRef_EvalConst`, `QCC_SRef_IsNull`, `QCC_Eval_Truth` and `QCC_PR_RoundFloatConst`;
`qcc_pr_lex.c`, `QCC_PR_LexNumber`; `qccmain.c`, `optimisations[]` (`constantarithmatic`),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Before it emits an opcode, `QCC_PR_StatementFlags` folds what it can of its constant operands,
with `opt_constantarithmatic` on (`FLAG_ASDEFAULT`, so unless the command line turns it off) or
outside a function. What it folds is never emitted, so an opcode the target lacks is not
emulated or refused there (FTEF-25). The semantic model folds the same opcodes
(`crates/qc-sema/src/check/fold.rs`), its constants FTEQCC's `QCC_eval_t` words with the type
they were made with (`crates/qc-sema/src/check/constant.rs`). C's arithmetic is written out:
integers wrap, shift counts are the low bits of the count, and a float out of an integer's range
saturates, as on the arm64 build the differential is recorded on.

### Behaviour

- **FTEF-26.1** A number literal's value is `QCC_PR_LexNumber`'s: its digits accumulated in a
  64-bit integer that wraps, the sign applied, then held to the literal's type (FTEF-3); a
  fraction read with `atof`.
- **FTEF-26.2** With both operands constant, the logic, bitwise, shift, arithmetic, comparison
  and 64-bit opcodes fold to a constant of the opcode's result, reading each operand's words as
  the opcode's operand type (a float's word read as an `int` is its bits). A float division by 0
  is "Division of %g by 0" (an `int` dividend "%d"), an integer one "Division by constant 0" and
  0, `INT_MIN / -1` "Signed overflow on division by -1" and `INT_MAX`; a remainder by 0 is not
  folded. A float operand of a bitwise or shift opcode that is not a whole number is "Constant
  float operand %f will be truncated to %i".
- **FTEF-26.3** With only the left operand constant: the unary opcodes and conversions fold, a
  conversion that loses the value being "Numerical overflow. %f will be rounded to %i", for a
  float with a zero exponent "Denormalized float %g will be rounded to %i. This is probably not
  what you want.", or "Numerical truncation of %#x to %#x."; the 64-bit and double conversions
  (`OP_CONV_FI64`, `OP_CONV_DI64`, `OP_CONV_FU64`, `OP_CONV_DU64` and back) fold the same way,
  with "Numerical overflow" for any fraction lost and "Numerical truncation" as `%#llx`, so
  `__int64 x; x = 1.5;` warns and `x = 1;` folds; `0 + b`, `0 | b` and `1 * b` are
  `b`; `0 & b` and a shift of 0 are 0; a false left operand of `&&` is 0 and a true one of `||`
  1, as floats.
- **FTEF-26.4** With only the right operand constant: `a + 0`, `a - 0`, `a | 0`, `a * 1` and
  `a / 1` are `a`, and `a & ~0` for integers; a division by a constant 0 is "Division by 0"; a
  false right operand of `&&` is 0. `||` never folds here: `QCC_Eval_Truth(eval_a, ...)` reads
  the absent left constant, which is false.
- **FTEF-26.5** A value is a constant when it is a literal, a folded result, or a definition
  that is constant and initialised (`QCC_SRef_EvalConst`), an enum's item and a `length` among
  them. A constant whose value the semantic model does not keep (an integer constant's
  initialiser, kept as text, an enum's item, a `length`) decides nothing it checks: no folding,
  no emulation.

- **FTEF-26.6** An unsigned division folds with no check of its divisor (`eval_a->_uint /
  eval_b->_uint`), a 0 divisor giving 0 as arm64's `udiv` does. A remainder by a constant 0,
  which is not folded (FTEF-26.2), is emulated as `a - b * (a / b)` in that order (FTEF-27.4), its
  steps folding in turn, so `13i % 0i` is 13 (`13 / 0` is 0 with "Division by constant 0", ignored
  by default) and `13.5 % 0.0` is 13 with the truncation warnings of `inf | inf`.
- **FTEF-26.7** A folded `OP_CONV_FTOI` is `QCC_MakeIntConst(eval_a->_float)`, the float made a
  `long long` (saturating on arm64) and then an `int`: when the two differ, after the overflow
  warning, it is `W0234` "Constant int operand %llu will be truncated to %i", the `long long`
  printed unsigned and the value its low 32 bits. So `i = 1i / 0;` folds `1 / 0.0` to `inf`, warns
  "Numerical overflow. inf will be rounded to 2147483647" and then "Constant int operand
  9223372036854775807 will be truncated to -1", and stores -1.
- **FTEF-26.8** A folded `OP_CONV_I64I` keeps the low 32 bits (`pint_t d = eval_a->i64;`) and,
  when they are not the value, warns `W0234` "Numerical truncation of %llx to %#x.", the 64-bit
  value in hex with no prefix and the 32 bits with C's `%#x` (`0` for 0): `i = 5000000000l;` is
  "Numerical truncation of 12a05f200 to 0x2a05f200.". Under `ILP32` the `l` literal is 32 bits
  (FTEF-12.12) and there is nothing to convert.
- **FTEF-26.9** Two constant doubles fold as C computes them: `OP_ADD_D`, `OP_SUB_D`, `OP_MUL_D`
  and `OP_DIV_D` to a double with no check of a 0 divisor; `OP_LE_D`, `OP_LT_D`, `OP_EQ_D` and
  `OP_NE_D` to an `int`; `OP_LSHIFT_DI` and `OP_RSHIFT_DI` shift the double made a `long long`
  (`(pint64_t)eval_a->_double << eval_b->_int`), and `OP_BITAND_D`, `OP_BITOR_D` and
  `OP_BITXOR_D` combine both made `long long`s, each result made a double again. `OP_RSHIFT_U64I`
  shifts the unsigned 64-bit value. `OP_BITEXTEND_I` and `OP_BITEXTEND_U` of two constants are
  `(a << (32 - bits - ofs)) >> (32 - bits)`, signed and unsigned, `bits` and `ofs` the low byte
  and the rest of the second. So `arr[(__double)2.5 * 2]` is index 5.
- **FTEF-26.11** A logical not of a constant folds (`QCC_PR_StatementFlags`, `a is const, b is
  not`): `OP_NOT_F` and `OP_NOT_V` to the float 1 when the value, or every component, is 0,
  `OP_NOT_ENT` and `OP_NOT_FNC` of the word, and `OP_NOT_I` to the `int` 1 or 0, before any
  emulation: on the id target `!1i` is 0 with no "Result of comparison is constant" of an
  emulated `OP_EQ_FNC`.
- **FTEF-26.12** A function's definition is a constant whose value no opcode's folding reads, so
  an opcode of it is emitted, or emulated where the target lacks it, as a variable's is: on the
  id target `b = (_Bool)fn;` of a `void() fn` is the `OP_NE_FNC` truth and three "ftoi
  emulation", as of an entity. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_StatementFlags`, `QCC_SRef_EvalConst`.
- **FTEF-26.10** A constant operand that an opcode's row converts is the constant its conversion
  folds to (`QCC_SupplyConversion` emits the conversion opcode, folded as FTEF-26.3 says), so the
  opcode then folds too: in `(__double)2.5 * 2` the float 2 is converted by `OP_CONV_FD` to the
  double 2, and `OP_MUL_D` folds.
## FTEF-27 Opcodes the target lacks

**Source:** `qcc_pr_comp.c`, `QCC_PR_StatementFlags` (its `if (!QCC_OPCodeValid(op))` switch and
`badopcode`), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

When the opcode an operator is given (FTEF-23) is not folded (FTEF-26) and the target lacks it
(FTEF-25), `QCC_PR_StatementFlags` emulates it. Each opcode an emulation emits goes through
`QCC_PR_StatementFlags` in turn, so it is folded, emitted or emulated as well. The semantic
model follows each emulation as far as its diagnostics (`crates/qc-sema/src/check/emulate.rs`);
the statements it would emit are code generation's.

### Behaviour

- **FTEF-27.1** An opcode the target has is emitted with no diagnostic: on the FTE target the
  integer and string opcodes, on the standard target id's.
- **FTEF-27.2** An integer, power or vector remainder opcode calls a helper the program defines
  before the function, found by name in the function's scope or the globals (`AddInt`, `SubInt`,
  `MulInt`, `DivInt`, `BitandInt`, `BitorInt`, `ModVec`, `powf` then `pow`, and for `~int`
  `SubInt`); with none it is the warning `W0235` "NAME function not defined: cannot emulate WHAT"
  and refused (FTEF-27.5), a power's naming `pow` ("pow function not defined: cannot emulate
  int*^int"). A mixed integer and float opcode converts its integer operand
  (`OP_CONV_ITOF`) and emits the float opcode; an integer comparison converts its left operand
  and emits the mixed one; `==` and `!=` of integers and fields are `OP_EQ_FNC` and `OP_NE_FNC`.
  The unsigned opcodes are the signed ones, the 64-bit unsigned the 64-bit signed; `OP_EQ_I64`
  and `OP_NE_I64` compare the two words; the 64-bit bitwise opcodes and loads are emitted as
  statements of the 32-bit ones with no check of the target.
- **FTEF-27.3** `OP_ADD_SF` calls `AddStringFloat` when defined; else, for a string that is no
  constant, it is the warning `W0236` "OP_ADD_SF: string+float may be unsafe", then the float is
  converted to an integer and added with `OP_ADD_I`. `OP_ADD_SI` and `OP_ADD_IS` warn "OP_ADD_SI:
  string+int may be unsafe" and add with `OP_ADD_I`. An entity plus a float, unless `qccx` is on,
  converts the float and adds with `OP_ADD_EI`, which needs the `nextent` builtin ("the nextent
  builtin is not defined" and refused) and then subtracts with `OP_SUB_I` (`(op ==
  &pr_opcodes[OP_ADD_EF]) ? OP_ADD_I : OP_SUB_I`, never `OP_ADD_EF` there). `<=>` of strings needs
  `strcmp`, else the fatal error `E0321`.
- **FTEF-27.4** A conversion folds a constant (FTEF-26.3); else it calls `itof`, `ftoi`, or for
  `OP_CONV_UF` `utof`, `quake_utof`, `itof` or `quake_itof`, when defined; else it is the warning
  `W0237` "itof emulation: denormals are unsafe NAME" (NAME the symbol's, `var_a.sym->name`: the
  value's definition, `IMMEDIATE` for an immediate, and for a temporary `temp_N`, N its slot in
  `QCC_GetTemp`'s temporaries, which `opt_overlaptemps` reuses; the semantic model does not follow
  the slots and prints the temporary's type name, `__int32`, where FTEQCC prints `temp_0`) or
  "ftoi emulation: denormals are unsafe",
  F313, an error by default, or for `OP_CONV_UF` `W0238` "utof emulation: will break if NAME is
  big". `%` with no `mod` (or `ModInt`) function is expanded into division, `|`, multiplication
  and subtraction; `^` into `~`, `&` and `|`; `v / f` into `1 / f` and `OP_MUL_VF`. An integer
  shift with no `LShiftInt`, `RShiftInt` or `RShiftUInt` is a product or quotient by the constant
  `QCC_MakeIntConst(1<<n)`, so a shift by 0 folds away, or by `2 *^ n` for a variable `n`; `>>`
  of an integer tests the sign (`OP_LT_I` against `QCC_MakeFloatConst(0)`), divides the
  complement (`OP_BITNOT_I`, the division, `OP_BITNOT_I`) and then divides the value itself as a
  `QCC_PR_SimpleStatement`, which refuses a division the target lacks with the fatal error
  `E0320` (FTEF-6.14): `i >> 2` ends with "Opcode \"/|DIV_I\" not valid for target". A float shift is the integer one on converted operands when the
  target has it, else `floor(a * pow(2, b))` when `powf` or `pow` and `floor` are defined, else
  `bitshift`, else "bitshift function not defined: cannot emulate OP_LSHIFT_F*" and refused.
- **FTEF-27.5** An opcode FTEQCC does not emulate is refused, `badopcode`: the error `E0320`
  "Opcode \"OP|NAME\" not valid for target. Consider the use of: #pragma target fte" when the FTE
  target at its default revision (5768) has it, "... #pragma target fte_6614" when only the
  newest revision has it, else "Opcode \"OP|NAME\" is not supported.", OP the operator and NAME
  the opcode's `opname`. It is reported with `QCC_PR_ParseWarning(ERR_BADEXTENSION, ...)` and does
  not end the statement.
- **FTEF-27.6** `&&` and `||` are `OP_AND_F` and `OP_OR_F` when the target has no other logic
  opcode for their operands.
- **FTEF-27.7** The unsigned, 64-bit and double opcodes the target lacks: `<` and `<=` of
  unsigned values are the float comparisons of `OP_CONV_UF`'s conversions when the target has no
  `OP_LT_I`, else the signed ones of the values less `0x80000000`; `>` and `>=` of unsigned, 64-bit
  and double values are `<` and `<=` of the operands swapped; unsigned division warns `W0235`
  "OP_DIV_U emulation: assuming the high bit is not set..." and divides as signed; a sign or zero
  extension of a constant width and offset is two shifts, of any other refused; `&` and `|` of
  doubles are the 64-bit integer ones between conversions (`OP_CONV_DI64`, `OP_CONV_I64D`); `^` of
  vectors and doubles is `(a & ~b) | (b & ~a)`; a shift of a double converts it to a 64-bit
  integer first; `!` of an integer is `OP_EQ_FNC` with 0.
- **FTEF-27.8** A cast of an operand to its opcode's type (FTEF-23.5) emits the conversion
  opcodes `QCC_TryEvaluateCast` names for the pair of types, in order: `OP_CONV_UF` for an
  unsigned value made a float, `OP_CONV_ITOF` for an integer, `OP_CONV_FTOI` back, the double and
  64-bit conversions, two of them for a double and a 32-bit integer; each is folded, emitted or
  emulated in turn. With no `utof`, `quake_utof`, `itof` or `quake_itof`, `OP_CONV_UF` is
  `OP_CONV_ITOF`, with its warning, then "utof emulation: will break if NAME is big".
- **FTEF-27.9** Some opcodes are replaced by another, `op = OTHER; break;`, which is emitted with
  no check of the target, no folding and no helper: the compound stores (`OP_ADDSTORE_F` and the
  rest, their operands swapped), `OP_STORE_P` and `OP_STORE_I` as `OP_STORE_F`, `OP_LOAD_P` and
  `OP_LOAD_I` as `OP_LOAD_F`, `OP_STOREP_P` as `OP_STOREP_I`, `~` of a float, vector or 64-bit value
  as a subtraction from -1, and the pointer comparisons as the entity or integer ones, so `p > q`
  of pointers on the standard target warns of nothing. `&` and `|` of vectors are three float
  statements emitted the same way. A bit clear, `a &~ b`, is the target's clearing opcode when it
  has one, else `a & b` emitted in turn and a subtraction replacing it. A float or string tested
  by an `if` is compared with 0 (`OP_NE_F`, `OP_NE_S`) and tested as an integer. The commented-out
  mixed stores (`OP_ADDSTORE_IF` and the like) and the pointer stores under `IAMNOTLAZY`, which the
  pinned source does not define, are refused (FTEF-27.5).
- **FTEF-27.10** Pointer arithmetic scales the offset by what the pointer points at: a float
  offset is first converted to an integer; for `void` it is bytes (`OP_ADD_I`, `OP_SUB_I`), for an
  8- or 16-bit type bytes times its width, and for any other type the offset times its size in
  words is added by `OP_ADD_PIW`, which, emulated, multiplies by `VMWORDSIZE` (4) and adds. A
  subtraction's size is negated as an unsigned value, `QCC_MakeIntConst(-aux_type->size)`, so it is
  always the warning `W0234` "Constant int operand 4294967295 will be truncated to -1" (for a size
  of 1). The difference of two pointers of different types by `typecmp` is the fatal error `E0327`
  "incompatible pointer types"; of one type it is their byte difference divided by the size.
  Under `undefwordsize` each assumption is the error `E0326` ("void* maths was disabled.",
  "pointer* maths was disabled on this type.", "pointer sizes were marked as undefined.", "Making
  assumptions about pointer sizes was blocked."). A load of 8 or 16 bits reads the word that holds
  it and shifts; a word load calls `memgetval`, else `W0235` "memgetval function not defined:
  cannot emulate OP_LOADP_*" and refused. A struct or union pointed at is sized by its layout
  (FTEF-6.13), so `p + 1` for a `struct S { float a; vector v; } *p` scales by 4 and `p - 1`
  warns "Constant int operand 4294967292 will be truncated to -4"; a struct whose layout the
  model does not follow decides no folding and gives no truncation warning.

The statements an emulation emits are code generation's; the semantic model follows them only
as far as they report.

- **FTEF-27.11** Some opcodes the target lacks are emitted as plain statements
  (`QCC_PR_SimpleStatement`), which `QCC_PR_StatementFlags`'s global-scope check never sees: a
  64-bit `&`, `|` and `^` as two `OP_BITAND_I`, `OP_BITOR_I` or `OP_BITXOR_I`, a 64-bit field
  load as two `OP_LOAD_FLD`, each refused on a target without it by the fatal error `E0320`
  (`force` off); a vector's `&` and `|` as three float ones with `force` on, never refused. So
  `d & e` of doubles on the id target is the two `OP_CONV_DI64` refusals and then `OP_BITAND_I`'s,
  which ends the statement before `OP_CONV_I64D`.
- **FTEF-27.12** An opcode emulated as another (`return QCC_PR_StatementFlags(OTHER, ...)`)
  gives the other's result, of the other's type: `OP_EQ_I` is `OP_EQ_FNC` and `OP_AND_I` is
  `OP_AND_F`, so `b = (i == j);` and `b = (i && j);` of ints store a float with no conversion.
  An unsigned operator emulated by its signed one then casts the result back
  (`var_c.cast = type_uint;` or `type_uint64`), a comparison's included: `b = !ul;` of a
  `__uint64` on the id target needs `OP_CONV_U64F`, refused with `E0320`.
- **FTEF-27.13** `OP_EQ_I64` and `OP_NE_I64` compare word by word: the operands cast to `int`,
  `OP_EQ_I` (or `OP_NE_I`) of the low words, then of the high words, joined by `OP_AND_I` (or
  `OP_OR_I`), each emitted or emulated in turn. Each word's comparison gives FTEF-28.1's
  warnings, so `l == l` of an `__int64` warns "Result of comparison is constant" twice.
- **FTEF-27.14** A prefix `++` or `--` of a pointer adds `QCC_MakeIntConst(preinc *
  aux_type->size)` by `OP_ADD_PIW`, the size in words of what it points at, 1 for `void`. The size
  is unsigned, so `--` adds its negation and warns as FTEF-27.10's subtraction: `--p` of a `vector
  *` is `W0234` "Constant int operand 4294967293 will be truncated to -3", and on the id target
  both steps are then `OP_ADD_I`'s helper warning and refusal.
- **FTEF-27.15** The emulated signed right shift tests its value's sign and jumps:
  `QCC_Generate_OP_IFNOT` of the `OP_LT_I` against 0 skips the negative form, which
  `QCC_Generate_OP_GOTO` ends before the positive one. At global scope each jump is a statement
  too, "Unable to generate statements at global scope (IFNOT)." after the comparison's and
  "(GOTO)." after the negative form's, as `float q = c;` of an `__int8 c` on the id target gives
  in the extension of its bits (FTEF-2.12). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_StatementFlags` (`OP_RSHIFT_I`).
- **FTEF-27.16** A statement's result is a temporary, named `temp_N` by its word offset among
  the temporaries (`QCC_GetTemp`): under `opt_overlaptemps`, on by default, the first one no
  reference holds of the result's size (1 word, 3 for a vector, 2 for a 64-bit value), else a new
  one after the last; a statement frees its operands' first (`QCC_FreeTemp`), a refused opcode's
  too (`badopcode`), and a store, a jump or a `void` opcode takes none. The emulation warnings
  name an operand by `sym->name`, so a temporary's is `temp_N`, as `QCC_VarAtOffset` prints it;
  the other messages name it by `QCC_GetSRefName`, its type's name (FTEF-27.4). So on the id
  target `x = c8;` of an `__int8 c8` is "itof emulation: denormals are unsafe temp_0", the
  `OP_MUL_I` of its shift's result, and `s[i] += x;` of a string's character `temp_1`. FTEQCC
  keeps the temporaries across the module and those a statement leaves referenced; this profile
  frees them all at each statement's end, so in a later function a number can differ. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_GetTemp`, `QCC_FreeTemp`, `QCC_PR_StatementFlags`, `QCC_VarAtOffset`.
## FTEF-28 The warnings of an emitted comparison

**Source:** `qcc_pr_comp.c`, `QCC_PR_StatementFlags` (the switch after the emulation),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Once an opcode is emitted (not folded, FTEF-26, and not emulated, FTEF-27),
`QCC_PR_StatementFlags` warns of what its operands show: the symbol (`sym`) that holds each, and
whether it is constant (`sym->constant`, an immediate or a constant definition, not a
temporary). The semantic model keeps each value's symbol (`crates/qc-sema/src/check/compare.rs`).

### Behaviour

- **FTEF-28.1** **Withdrawn** (#84): it said a function's definition is no constant symbol to the
  comparison warnings; it is one, and a comparison of types `typecmp_lax` tells apart gives no
  warning, which FTEF-28.4 says, replacing it.
- **FTEF-28.2** **Withdrawn** (#84): it said a comparison of operands of different basic types is
  "'OP' type mismatch: A with B"; the warning is never given, and FTEF-28.3 replaces it.
- **FTEF-28.3** A comparison never warns "'OP' type mismatch: A with B", even with F307 on. When
  `typecmp_lax(var_a.cast, var_b.cast)` tells the operands apart, the check walks `var_a`'s
  parents, `for (t = var_a.cast; t; t = t->parentclass) if (typecmp_lax(t, var_b.cast))
  break;`, which stops at the first type that differs, `var_a.cast` itself, and `if (t) break;`
  then leaves before the warning, as the walk of `var_b`'s parents would. So `!u` of a `__uint`
  and `!p` of a pointer, emulated on the id target as `OP_EQ_FNC` with an `int` 0, warn of
  nothing, nor do `.float == .vector` or `float() != void()`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_StatementFlags` (the `OP_EQ_F` to `OP_GT_F` cases).
- **FTEF-28.4** `OP_AND_F` with one symbol on both sides is "Parameter offsets for && are the
  same", and with both constant "Result of comparison is constant"; `OP_OR_F` with one symbol is
  "Parameters for || are the same", and with either constant "Result of comparison is constant".
  A comparison whose operands' types `typecmp_lax` tells apart leaves the switch before its
  constant check (FTEF-28.3) and warns of nothing. Any other with both operands constant symbols
  (`sym->constant && !sym->temp`), or, unless it is a float or vector equality, `<=` or `>=`,
  with one symbol on both sides, is "Result of comparison is constant": `s == s`, `e == e`, `a <
  a` and `"x" == "y"` are, `a == a` is not, and two constants of a float comparison fold first
  (FTEF-26.2). A function's and a field's definitions are constant symbols, so `fn == fn2` of two
  `void()` and `fl == fl2` of two `.float` warn, where `ff != fn` of a `float()` and a `void()`
  and `fl == fv` of a `.float` and a `.vector` do not. A null constant converted for the opcode is
  a constant of the row's type with no statement, `type_function` a `void()` and `type_field` a
  `.void`: `fn == 0` warns, `ff == 0` and `fl == 0` do not. A `case` value supplied for a
  function or pointer switch keeps its `int` type (`QCC_ShouldConvert` gives 0), so `switch (ff)
  { case 1i: }` warns of nothing; any other operand converted for the opcode is a temporary.
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_StatementFlags`, `QCC_TryEvaluateCast`, `QCC_SupplyConversion`.

## FTEF-29 Assignments

**Source:** `qcc_pr_comp.c`, `QCC_PR_RefExpression` (the assignment operators),
`QCC_StoreSRefToRef`, `QCC_SupplyConversionForAssignment`, `QCC_PR_ParseRefArrayPointer` (`++` and
`--` after a term), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

An assignment's left side is read as a term (FTEF-16.13) into a reference that knows whether it
is read-only (a constant, an immediate, or what an operator or another assignment gave) and
whether it is the result of a postfix `++` or `--`. The semantic model checks assignments in
`crates/qc-sema/src/check/assign.rs`.

### Behaviour

- **FTEF-29.1** Before the right side is read, an assignment to the result of a postfix `++` or
  `--` is the fatal error `E0322` "Assignment to post-inc result", and one to a read-only
  reference the warning `W0241` "Assignment to const". `1 = a`, `~a = 1`, `(a + 1) = 2` and
  `(a = 2) = 3` are each warned, not refused.
- **FTEF-29.2** `=` casts the value to the left side's type (`QCC_EvaluateCast(rhs, lhs, true)`,
  FTEF-19.31), so a value of another type is "Cannot cast from A to B" (`E0213`), a fatal error,
  and an array named bare is a pointer to its elements: `arr = 1` is "Cannot cast from float to
  float*". A compound assignment takes the first opcode of its list (`opcodes_addstore` and the
  others) whose operands are the left side's and the value's basic types, an array named bare its
  address (`arr += 1` is `ADD_PF`, then FTEF-29.3's "left operand must be an l-value"); with none,
  the value is cast to the left side's type and the list searched again; with none still it is the
  fatal error `E0323` "Type mismatch on assignment. A OP B is not supported" (`s += s`), A and B
  the types' own names (FTEF-24.11), B the left side's when the cast changed the value's type
  (`ptr *= ptr` for `p *= v` under `lax`, `float* += ptr` for `arr += p`). A field or pointer
  updated by an in-place opcode the target has (`opcodes_addstorep` and the others) is checked no
  further.
- **FTEF-29.3** The store (`QCC_StoreSRefToRef`) to a read-only reference warns `W0241`
  "Assignment to constant NAME", NAME as `QCC_GetSRefName` gives it (FTEF-27.4), and to an array
  named bare is "left operand must be an l-value (did you mean NAME[0]?)" (`E0325`). An
  assignment's value is the value stored, read-only, so `(a = 2) = 3` warns "Assignment to
  constant IMMEDIATE".
- **FTEF-29.4** A compound assignment's opcode is emitted (FTEF-27), and its result converted for
  the store by `QCC_SupplyConversionForAssignment`: an explicit cast (`QCC_TryEvaluateCast` with
  `implicit` off, which also casts between an entity and a float), else `QCC_ShouldConvert`'s
  conversion; with none it is the fatal error `E0324` "Implicit type mismatch on assignment to
  NAME. Needed A, got B." (under `typeexplicit` for any conversion), or under `lax` the warning
  `W0243`. `a *= v` of a float and a vector is the error: `OP_MUL_FV` gives a vector.
- **FTEF-29.5** A postfix `++` or `--` of a read-only value is the fatal error `E0322`
  "postincrement: lhs is readonly" or "postdecrement: lhs is readonly". Otherwise the result is
  marked as a postfix's, a temporary that messages name by its type (`QCC_GetSRefName`), and the operand is read and
  updated by adding 1 or -1 (`--` adds -1; `QCC_RefToDef`, or `QCC_PR_DiscardRef` for a value
  discarded: a statement's expression, a `for` header's first or last clause, a comma's left
  side), by its basic type: `OP_ADD_F` of a float, `OP_ADD_D` of a double, `OP_ADD_I` of an int
  for an integer and for a string, `OP_ADD_U` for a uint, `OP_ADD_I64` and `OP_ADD_U64` of an
  int64; a pointer by `OP_ADD_PIW` of the step times the size in words of what it points at (a
  word for `void`), or for `__int8` and `__int16` (`aux_type->bits`) by `OP_ADD_I` of the step in
  bytes, which under `undefwordsize` a value not discarded first makes the fatal error `E0322`
  "not allowed to make assumptions about pointer sizes. sorry."; any other type (a vector, an
  entity) is the fatal error `E0322` "post increment operator not supported with this type". A
  bitfield that does not start its word is first "post increment operator on bitfield"; one that
  does, and an 8- or 16-bit integer (FTEF-2.12), is read, then "not supported" when its value is
  used (`ref->cast` is `ev_bitfld`) and stepped as the integer it is read as when discarded. Each
  opcode is emitted as FTEF-27 says, and the sum stored back as an assignment's (FTEF-29.7,
  FTEF-6.15): `s++` warns "type mismatch: __int32 __int32 to string s".
- **FTEF-29.6** An assignment that is the whole condition of an `if`, `while`, `do` or `for`,
  not parenthesised (`a = 1 && a` is one, `a = (1 && a)`), warns `W0242` "suggest parenthesis for assignment used as truth value" after its
  right side is read; `if ((a = 1))` does not.
- **FTEF-29.7** The store checks the value's type against the left side's: unless the value is
  null, or its type or a parent's is the left side's by `typecmp_lax`, or either is a `variant`,
  or the value is a `void*` or `variant*` stored into a pointer, a number is converted to the left
  side's type (`QCC_SupplyConversion`) and anything else is the warning `W0240` "type mismatch: A
  NAME to B TARGET", TARGET `base.field` for a field. A cast, `lax` included, gives the value the
  left side's type first, so only a compound assignment's result kept under `lax` (FTEF-29.4)
  reaches the warning: `a *= v` is "type mismatch: vector vector to float a".
- **FTEF-29.8** Two names are given to a target. A compound assignment's "Implicit type mismatch
  on assignment to NAME" and a call's "argument NAME will be ignored" name it by
  `QCC_GetRefName`: `base.field`, `base[index]` for an element or a string's character, the
  index `IMMEDIATE` for a constant (`arr[IMMEDIATE]`), `base->index` through a pointer, `TEMP`
  for no index (`p->TEMP` for `*p`), and the base's name for anything else, an accessor's member
  included (its branch writes the member's name, then `break`s and returns the base's). The
  store's "type mismatch" names it as `QCC_StoreSRefToRef` does (FTEF-29.7): `base.field`,
  `base[index]` but for an `IMMEDIATE` index, else the base.
- **FTEF-29.9** A constant that is null (`QCC_SRef_IsNull`, an integer, float or pointer 0) casts
  as a null whatever its definition's type, a `typedef` or `int` one included: `const int zi = 0;
  string s = zi;` is no cast warning.
- **FTEF-29.11** So is a global's initialiser that names a constant whose word is 0
  (`QCC_SRef_IsNull`): `enum { H }; string s = H;` and `const float z = 0; string s2 = z;` are
  no error, where a constant 1 is "Cannot cast from float to string"; a struct or union takes no
  null ("Cannot cast from float to struct S"), and a strict enum's value casts to no other type
  whatever it is (FTEF-19.10).
- **FTEF-29.12** A compound assignment or an increment of an array named bare, after its
  "left operand must be an l-value (did you mean NAME[0]?)" (FTEF-29.3), frees the head a
  second time: `QCC_StoreSRefToRef` has discarded the reference it read (`QCC_PR_DiscardRef`),
  and the operator frees it again, `W0319` "INTERNAL: over-freed refcount to NAME"
  (`QCC_FreeTemp`). So `farr += 1;`, `farr++`, `++farr` and `farr--` of a `float farr[3]`
  under `#pragma target fte` give both, a local array's too, and the plain store `farr = p;`
  only the first. A struct's member array frees its offset constants as well, which this
  profile does not follow. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_StoreSRefToRef`, `QCC_FreeTemp`.
- **FTEF-29.13** A store to a definition is its `OP_STORE_*` statement, unless the value is the
  temporary the statement before made, which `opt_assignments` folds the store into; and a postfix
  `++` or `--` whose value is read copies the old value to a temporary by `OP_STORE_*` first. At
  global scope each is "Unable to generate statements": `float q = (x = y);` is `STORE_F`, `(x =
  y + 1)` only `ADD_F`, `float q = x++;` `STORE_F` and `ADD_F`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_StoreSRefToRef`, `QCC_PR_RefExpression` (`ref->postinc`), `QCC_RefToDef`.
- **FTEF-29.10** An array named bare, its head, is the pointer to its first element to a
  prefix or postfix `++` and `--` (`REF_ARRAYHEAD`): read by its address, which the id target
  refuses (FTEF-31.4), stepped by `OP_ADD_PIW` of its element's size, `--` warning of the
  truncated negation (FTEF-27.14), and stored as `QCC_StoreSRefToRef` stores to the head, "left
  operand must be an l-value (did you mean NAME[0]?)" (`E0325`): on the FTE target `--varr;` of a
  `vector varr[2]` is "Constant int operand 4294967293 will be truncated to -3" then the l-value
  error, and the value of `farr++` is a `float*`.

## FTEF-30 Terms

**Source:** `qcc_pr_comp.c`, `QCC_PR_RefTerm`, `QCC_PR_GenerateLogicalTruth`,
`QCC_PR_GenerateLogicalNot`, `QCC_PR_GenerateBitwiseNot`, `QCC_Generate_OP_IFNOT`,
`QCC_Generate_OP_IF`, `QCC_PR_GenerateVector`, `QCC_EvaluateCast`, `QCC_PR_RefExpression` (the
conditional operator), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

A term is what FTEQCC reads before an operator: a prefix operator and its operand, a cast, a
vector constructor, `sizeof`, `_length`, and the conditional operator, which `QCC_PR_RefExpression`
reads at its priority. The semantic model checks them in `crates/qc-sema/src/check/term.rs` and
`crates/qc-sema/src/check/truth.rs`; the address-of operator, indexing and the read of a
reference are FTEF-31's.

### Behaviour

- **FTEF-30.1** `!`, `~` and a value made a truth value emit the opcode of the value's basic type
  (a truth value taken for its parent's): `!` `OP_NOT_F`, `OP_NOT_S`, `OP_NOT_ENT`, `OP_NOT_V`,
  `OP_NOT_FNC` and `OP_NOT_I` (for integers and pointers), and `== 0` for a double or 64-bit
  value; `~` `OP_BITNOT_F`, `OP_NOT_S`, `OP_BITNOT_V`, `OP_BITNOT_I` and the 64-bit and double
  ones, but nothing for an entity or a function; the truth `!= 0` of each. Each is folded,
  emitted or emulated (FTEF-27). A `void` value is under `lax` the warning `W0248` with the
  message and `OP_NOT_F`, else, like any type with no opcode, the fatal error `E0328` "Type
  mismatch: !TYPE" or "Type mismatch: ~TYPE": `~e` of an entity and `!(void)a`.
- **FTEF-30.2** A condition is tested by `QCC_Generate_OP_IFNOT`, or `QCC_Generate_OP_IF` for a
  jump taken when it is true: integers, entities, fields, functions, pointers and truth values
  as integers; a string warns `W0245` "if (string) tests for null, not empty" (with a full stop
  for `QCC_Generate_OP_IF`), F322, off by default; a vector is `!v` under `ifvector`, on by
  default, else `W0246` "if (vector) tests only the first element with the current compiler
  flags"; a double or 64-bit value is compared with 0; a `variant`, struct, union, strict enum
  or `void` warns `W0247` "conditional type mismatch: TYPE", TYPE from `basictypenames[]`. The flags
  `ifvector` and `vectorlogic` are kept for it (FTEF-21.1). The conditional operator's test is
  checked; the statements' (FTEF-18) are the statement checks'.
- **FTEF-30.3** `c ? a : b` tests `c` (FTEF-30.2); `c ?: b` takes `c` as the first arm. An
  integer arm is made a float when the target has no `OP_STORE_IF`. An integer and a float are a
  float, the integer converted; an integer and an unsigned value unsigned, and so for 64 bits.
  Arms whose types `typecmp` tells apart, a truth value taken for its parent's, are then
  reconciled: a null arm takes the other's type when they are of one size; else arms
  `typecmp_lax` tells apart (it compares what fields, pointers and functions hold, and
  parameters and a class's members, but no class names, FTEF-9.14) warn `W0244` "Type mismatch
  on ternary operator: A vs B", warning 0, and arms it takes as one, two classes with the same
  members, are the fatal error `E0332` "Ternary operator with mismatching types". `a ? s : 2` warns and the store then refuses the string.
- **FTEF-30.4** `-e` is `0 - e` of the operand's type (`OP_SUB_F`, `OP_SUB_D`, `OP_SUB_V`,
  `OP_SUB_I`, `OP_SUB_U` and the 64-bit ones), `+e` the operand; any other type is the fatal
  error `E0302` "type mismatch for -" or "type mismatch for +". `++e` and `--e` of a read-only
  value are the fatal error `E0329` "++ operator on read-only value" (or `--`), and of a type
  with no `+` `E0329` "++ operator on unsupported type"; otherwise the `+` of 1 or -1 of the
  operand's type, a pointer's by `OP_ADD_PIW`, is emitted and stored, and the value is the
  operand stored to, read-only, so `itof` of `--i` names `i`.
- **FTEF-30.5** `*e` of a pointer is what it points at, which may be assigned to; of a string
  its first character, an integer; under `qccx` of a float or integer an entity; of a function
  the function, or under `qcfuncs`, on by default, the fatal error `E0330` "attempt to
  dereference function type."; of anything else `E0330` "Unable to dereference non-pointer
  type.". Under `qccx`, `@e` of a float or integer is a string, of anything else `E0330`
  "'@' operator only functions on floats and ints. go figure.".
- **FTEF-30.6** A cast the program writes, `(TYPE)e`, is `QCC_EvaluateCast(e, TYPE, false)`:
  it warns of nothing, takes what an implicit cast takes (FTEF-19.31) and also an entity and a
  float either way (`OP_CONV_ITOF`, `OP_CONV_FTOI`), two types of one basic type and any type to
  `void`, and under `lax` anything; else it is "Cannot cast from A to B" (`E0213`), a fatal
  error. Its conversion opcodes are emitted (FTEF-27.8): `(entity)a` is "ftoi emulation:
  denormals are unsafe" on the standard target.
- **FTEF-30.7** The vector constructor `[x, y, z]`, and `(vector){x, y, z}`, casts each
  component that is no float or integer to a float (`QCC_EvaluateCast(x, type_float, true)`,
  "Cannot cast" for a string or an entity); a component left out, `[x]` or `[x, y]`, is a float
  0; three constants are a vector constant; two null components make the vector the third
  times its axis, `'1 0 0' * x` by `OP_MUL_VF`; else each component is stored, a float with
  `OP_STORE_F` and an integer with `OP_STORE_IF`, which on the standard target is "itof
  emulation". Its "Argument not a single numeric value in vector constructor" is not reachable:
  the cast before it gives every component a float's type or fails.
- **FTEF-30.8** **Withdrawn** (#84): it said the model keeps no array's length or struct's
  layout, so their sizes were no known constants; it keeps both, and FTEF-30.11 replaces it.

- **FTEF-30.9** `?:` compares its arms' types (FTEF-30.3) before it stores the last arm, so
  "Type mismatch on ternary operator: A vs B" comes before that store's statement.
- **FTEF-30.10** A conditional's first arm is stored by its size (`switch(r.cast->size)`: three
  words by `OP_STORE_V`, two by `OP_STORE_I64`, one by `OP_STORE_F`), and one of more than
  three words is the fatal error `E0370` "oversized ternary result": `struct S { float a, b, c, d;
  }; S r = c ? s1 : s2;`. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_RefExpression`.
- **FTEF-30.11** `sizeof(TYPE)` and `sizeof e` are unsigned constants: a string immediate's length
  with its NUL, a type's bytes, `bits / 8` for an 8- or 16-bit type and 4 per word for any other,
  and an array's, a member array's included, its element's times its length: `sizeof(farr)` is 16
  for `float farr[4]`. Under `undefwordsize` `sizeof` of a type is the error `E0326` "sizeof was
  disabled, use array.length instead." and of an expression "sizeof was disabled.". `_length(e)`
  (or `_length e`) is an integer constant: a defined array's length, a constant string's without
  its NUL, a vector's 3; a struct's member array's length is no constant. A string that is no
  constant is the fatal error `E0334` "_length(string) requires an initialised constant", any
  other type "_length() unsupported argument type for intrinsic". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefTerm`.
- **FTEF-30.12** A truth value as an arm of `?:` is `ev_boolean`: it is not the `int` arm a
  target with no `OP_STORE_IF` makes a float, nor the `int` promoted to the other arm's `float`,
  so `x = q ? bb : x;` of a `_Bool bb` warns "Type mismatch on ternary operator: __int32 vs
  float", and the `?:` is the arms' store, `QCC_PR_DummyDef(type, "ternary", ...)`, which the
  emulation warnings name `ternary`: "itof emulation: denormals are unsafe ternary" as it is
  stored to the float. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_RefExpression` (`?`).
- **FTEF-30.13** A call's value is an alias of `OFS_RETURN` (`QCC_GetAliasTemp`), no temporary,
  so an arm's store of it is no folded one; and the first arm's store, the condition's under `?:`
  or the middle arm's, asks for its statement (`&truthstore`), which first copies the aliases to
  temporaries (`QCC_ClobberDef(NULL)`): at global scope `float q = c ? ff() : y;` is "Unable to
  generate statements" of `IFNOT`, `CALL0`, two `STORE_F`, `GOTO` and a last `STORE_F`, where
  `c ? y : ff()` stores the call once. A `?:` with a vector condition under `ifvector`, its
  default, tests `!v` by `OP_IF_I`, "(IF)". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefExpression` (`?`), `QCC_ClobberDef`, `QCC_Generate_OP_IFNOT`.
- **FTEF-30.14** `typecmp_lax` compares two function types by their return types and the
  parameters both have (`minargs`), any more one of them has being `optional`; whether either
  takes `...` is not compared. So `void(float a, ...)` and `void(float b)` are one type to it,
  and as `?:` arms, which `typecmp` tells apart, are "Ternary operator with mismatching types";
  `void(float a, float b)` and `void(float b)` are not, "Type mismatch on ternary operator"; and
  `void(float a)` stores to a `void(float a, optional float b)` with no "Implicit cast" (FTEF-22.8).
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_lex.c`,
  `typecmp_lax`.
- **FTEF-30.15** At global scope (`pr_scope` null) a conditional whose condition is a constant
  (`QCC_SRef_EvalConst`) generates nothing: `c ?: b` is `c` when `QCC_Eval_Truth` holds of it,
  else `b`; `c ? a : b` is `a` when the constant's first word (`eval->_int`) is not 0, so `-0.0`
  picks `a`, else `b`. The other arm is read and dropped, and the value is the arm's, constant or
  not ("initializer for 'q' is not constant" only when that arm is not). In a body the jumps are
  always generated (`if (pr_scope) eval = NULL;`). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefExpression`.
## FTEF-31 References

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseRefArrayPointer`, `QCC_RefToDef`,
`QCC_PR_GenerateAddressOf`, `QCC_LoadFromArray`, `QCC_LoadFromPointer`, `QCC_StoreToArray`,
`QCC_StoreToPointer`, `QCC_PR_Statement_SmallStore`, `QCC_StoreSRefToRef`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

FTEQCC parses a term into a reference (`QCC_ref_t`): a definition (`REF_GLOBAL`), an entity's
field (`REF_FIELD`), an array named bare (`REF_ARRAYHEAD`), a word of a definition at an index
(`REF_ARRAY`), what a pointer points at (`REF_POINTER`) and a string's character
(`REF_STRING`). Where it needs the value it reads the reference (`QCC_RefToDef`), which emits the
loads; an assignment stores to it. The semantic model keeps the kind of each value's reference
(`crates/qc-sema/src/check/reference.rs`) and reads it where FTEQCC does: operator operands,
conditions, `?:`'s arms, casts, vector components, an assignment's value and a compound one's
left side, the operand of `!`, `~`, `-`, `+`, `*`, `++` and `--`, an index, an entity before
its field, and an expression statement; the left side of `=`, the operand of `&` and `@`,
`sizeof`'s, an index's base and a parenthesised expression are references.

### Behaviour

- **FTEF-31.1** A reference read: a definition or field is the value; an array named bare is
  its address (FTEF-31.4); an element at an index that is no constant is loaded (FTEF-31.2);
  what a pointer points at by `OP_LOADP_*` (FTEF-31.3); a string's character by `OP_LOADP_C` for
  a float index, else `OP_LOADP_U8`.
- **FTEF-31.2** `base[index]`: of an array, its element, a constant index past its end the
  warning `W0249` "(constant) array index out of bounds (0 <= I < N)", an index into an element
  of more than one word scaled by its size (`OP_MUL_F` for a float index, else `OP_MUL_I`); of a
  class's array member or an array of fields, the field the index selects, the index read as an
  element's once, when it is parsed, so `arr[i] = 0i` converts `i` once; of a pointer, what it
  points at; of a string, its character, under `qccx` the string offset (`OP_ADD_I`, or `OP_ADD_F`
  with "string offsetting emulation: denormals are unsafe" for a float index); of a vector or an
  entity's `.vector` field, its component, a constant index past 2 the fatal error `E0333`
  "(vector) array index out of bounds" or "(.vector) array index out of bounds"; anything else
  is no array, where FTEQCC's parser stops at the `[`: `E0204` "expected ;, found [", `)` inside
  parentheses or a call's arguments and `]` inside an index. An element at a variable index is
  read by `OP_LOADA_*` or `OP_GLOAD_*` with the index made an integer when the target has them,
  by `OP_FETCH_GBL_*` with it made a float for a global array when the target has that, and else
  by the generated `ArrayGet` function, the index made a float (a vector's divided by 3): on the
  standard target `arr[i]` of an integer `i` is "itof emulation: denormals are unsafe i".
- **FTEF-31.3** What a pointer points at is read with the index made an integer, by
  `OP_LOADP_F`, `OP_LOADP_S`, `OP_LOADP_V`, `OP_LOADP_ENT`, `OP_LOADP_FLD`, `OP_LOADP_FNC`,
  `OP_LOADP_I` (integers, and pointers when the target has no `OP_LOADP_P`) or `OP_LOADP_I64`,
  each emulated or refused (FTEF-27.10); a struct, union or double word by word with no check;
  a `void` or `variant` is the fatal error `E0336` "ptr-to-variants must be cast to a different
  type before being read". A store through a pointer is by offset, which needs no emulation on
  the standard target.
- **FTEF-31.6** An array of fields named after an entity's `.` with no `[` after it,
  `e.a` for `.float a[2]` or a class's `float a[2]` member, is the array's head, whose address
  is taken (FTEF-31.4): on the id target the fatal error `E0331` "... Consider the use of either
  '#pragma target fte' or '#pragma flag enable brokenarray'", stored to or read; `e.a[1]` is the
  element.
- **FTEF-31.4** `&e`: under `qccx` a float is a pointer to a float, and a string, field, entity
  or function a float; a temporary not reached through a pointer is the error `E0335` "Address-of
  temp on line FILE:LINE", FILE the file's path as the module names it and LINE its line, and
  stays the value it is; else its address: a field's by `OP_ADDRESS`; a pointer's element's by
  `OP_ADD_PIW`; a definition's, an element's or an array's by `OP_GLOBALADDRESS`, which a target
  without it refuses, the fatal error `E0331` "Address-of operator is not supported in this form
  without extensions. Consider the use of: #pragma target fte", and for an array "Consider the
  use of either '#pragma target fte' or '#pragma flag enable brokenarray'"; reading an array
  named bare takes its address, so `arr = arr` and `arr += 1` are that error on the standard
  target. The address of a local warns `W0250` "Address-of operator on local TYPE NAME is unsafe
  if recursing (and still bloated when not). Consider use of 'static' or 'auto'.", once per
  local, off by default; the model counts parameters as locals and does not tell a `static`
  local. FILE is the file's name as the compiler was given it, `s_filen` (FTEF-14.32), and with
  LINE follows `#pragma file` and `#pragma line` (FTEF-14.43): `Address-of temp on line
  foo.qc:40` after `#pragma file foo.qc` and `#pragma line 40`.
- **FTEF-31.5** A store to an element at an index that is no constant is by its address when the
  target has `OP_GLOBALADDRESS` or `OP_GSTOREP_F`, the index made an integer, else by the
  generated `ArraySet` function, the index made a float (a vector's divided by 3); a value of
  another type than the element's, which only a compound assignment's result under `lax`
  leaves, is the fatal error `E0337` "Type mismatch on indexed assignment of NAME: A VALUE,
  needed B.". An element is named `base[index]` in the store's messages, but for an immediate
  index. A string's character is stored by `OP_STOREP_C` for a float when the target has it,
  else `OP_STOREP_I8` when it has that, else `OP_STOREP_C` or `OP_STOREP_I8`, the value made a
  float or an integer for it and the index an integer, added to the string by `OP_ADD_I` on a
  target whose stores take no offset; the store is emulated as FTEF-6.18 says.
- **FTEF-31.7** An index on a pointer is scaled as an array's is (`QCC_PR_ParseRefArrayPointer`,
  `t->size != 1`): by the size in words of what the pointer points at, a vector's 3, a 64-bit
  value's 2, a struct's or union's its own, by `OP_MUL_F` for a float index, else the index
  made an integer and multiplied by `OP_MUL_I`; an index of an 8- or 16-bit integer
  (`t->bits`) is made an integer and multiplied by one, which emits nothing. `pv[i]` of a
  `vector *pv` is `OP_MUL_I` of `i` by 3, which the id target emulates.

- **FTEF-31.8** A constant index is read by `QCC_Eval_Int`, a 64-bit integer: C's conversion of a
  float or double (saturating, as on arm64), an integer, or a 64-bit one, and the bounds check
  compares its low word as unsigned, so `arr[13.5 / 0.0]` is "(constant) array index out of bounds
  (0 <= -1 < 1)". A vector or string constant has no value there, `E0360` "Unable to evaluate to
  numeric constant", twice for an array's, a vector's and a `.vector` field's index (the bounds
  and the element's read) and 0 for the index; a pointer's index is made an integer instead, a
  string "Implicit type mismatch. Needed integer, got string.".
- **FTEF-31.9** A constant vector's component at a constant index is a constant, the component's
  value: `('13 5 2' + '1 2 3')[1]` is 7, and `vc[1]` of a `const vector vc = '4 9 2'` is 9.
- **FTEF-31.10** An element of an array, or a vector's component, at a constant index is the
  array's definition at that word (`ret.ofs += index`), so `iarr[1] == iarr[1]` of `int`s is
  the same operand twice ("Result of comparison is constant" on the id target) and `iarr[1] ==
  iarr[2]` is not; at an index that is no constant it is loaded into a temporary.
- **FTEF-31.11** The address of a string's character is no address `QCC_PR_GenerateAddressOf`
  makes: on any target it is the fatal error `E0361` "Cannot use addressof operator ('&') on a
  global. Please use the FTE target." (`pw = &s[0];`).
- **FTEF-31.12** The address of an element through a pointer to an 8- or 16-bit integer
  (`basetype->align != 32`, "index is bytes or shorts or something small") is the pointer plus
  the element's byte offset by `OP_ADD_I`, not `OP_ADD_PIW`: a constant index's
  `index*(align/8)` folded, unless `undefwordsize`, else `OP_MUL_I` of `align/8` and the index
  made an integer, which folds away for a byte. So on the id target `__int8 *pc; &pc[i]` is only
  `ADD_I`'s "AddInt function not defined" and "not valid for target" errors, `__int16 *pc;
  &pc[i]` `MUL_I`'s and then `ADD_I`'s, and `&pc[0]` none. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_GenerateAddressOf`.
- **FTEF-31.13** Under `qccx`, `&` of a float "casts it to a (pre-dereferenced) pointer"
  (`QCC_PR_BuildRef(retbuf, REF_POINTER, QCC_RefToDef(r, true), nullsref, type_float, false,
  0)`): a float read through the value as an address, by `OP_LOADP_F`, and stored to likewise,
  so on the standard target `float f; string s; s = &f;` is `OP_LOADP_F`'s "memgetval function not
  defined" and "not valid for target" errors and then "Implicit cast from float to string".
  Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefTerm`.
- **FTEF-31.14** A string's character is read and stored through `QCC_SupplyConversion(..., true)`:
  read at an index that is no float, the index is made an integer, and stored, the value is made
  a float or an integer for the opcode FTEF-31.5 chooses, then the index an integer. A type that
  does not convert is the error "Implicit type mismatch. Needed integer, got string." for an index
  (a warning under `lax`), and the value's cast is the definition's (FTEF-19.31): `s["a"]`, `s[e]
  = 1` and `s[1] = "b"`, "Cannot cast from string to float". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_RefToDef`, `QCC_StoreSRefToRef`, `QCC_SupplyConversion`.
- **FTEF-31.15** An array head has a length prefix for `OP_FETCH_GBL_*` when the target has it,
  unless it is embedded in another symbol: `def->arraylengthprefix = !rootsymbol &&
  !def->localscope && ...` reads `localscope` before `QCC_PR_GetDef` sets it, so a local array
  has the prefix too. Read there with an index that is no float (`QCC_LoadFromArray`, `accel ==
  1`), the index is converted with `QCC_SupplyConversion(index, ev_float, true)`, whose result is
  not kept: the fetch takes the index as it is and frees it a second time, which for an index
  that is a named definition warns `W0319` "INTERNAL: over-freed refcount to NAME", as under
  `#pragma target hexen2` `farr[i]` does for an `int i`. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_LoadFromArray`, `QCC_PR_GetDef`, `QCC_FreeTemp`.
- **FTEF-31.16** An array of fields, an entity's `e.fa[i]` of a `.float fa[4]` or a class's
  member array `this.arr[i]`, is indexed in elements of the field's type: an index that is no
  constant is scaled as an array head's is (FTEF-31.2), so a `.vector` array's `int` index is
  `OP_MUL_I` by 3, on the id target "MulInt function not defined: cannot emulate int*int" and
  its refusal, and the scaled index is a temporary. A class's member array is a
  `QCC_PR_DummyDef` with a root symbol, the class's field, so it has no length prefix
  (FTEF-31.15): under `#pragma target hexen2` `this.arr[ii]` of an `int ii` warns only "itof
  emulation", where `e.fa[ii]` of a global array of fields also warns "INTERNAL: over-freed
  refcount to ii". Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseRefArrayPointer`, `QCC_LoadFromArray`,
  `QCC_PR_DummyDef`; `engine/qclib/qcc_pr_lex.c`, `QCC_PR_ParseType` (the class's fields).
- **FTEF-31.17** Under `qccx` the address of a field is the field cast to a float (`r->cast =
  type_float`), the same symbol, whose word is the field's offset: the module's first field, at
  offset 0, is then a null constant (`QCC_SRef_IsNull`), which `QCC_TryEvaluateCast` casts to
  any type with no warning, so `s = &fl;`, `e = &fl;` and `p = &fl;` of the first `.float fl`
  give nothing, where any later field is "Implicit cast from float to string", "... to entity"
  and "Cannot cast from float to float*". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_RefTerm`, `QCC_TryEvaluateCast`, `QCC_SRef_IsNull`.
- **FTEF-31.18** A string's character read at an index that is no float is the value
  `OP_LOADP_U8` gives: an `int` on a target that has it, and on one that does not the
  `__uint32` of the last shift of its emulation (`OP_RSHIFT_U`), so under `#pragma target dp`
  `x = s[ii];` of a float `x` adds "utof emulation: will break if ... is big". A compound
  assignment to the character reads it so, and a float value cast to an unsigned type keeps the
  type its conversion gives (`QCC_TryEvaluateCast` sets no cast on the float to `uint` branch):
  `s[ii] += x;` there and `u += x;` of a `__uint u` on any target are "Type mismatch on
  assignment. __uint32 += __int32 is not supported" and "__uint += __int32". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_StatementFlags` (`OP_LOADP_U8`, `OP_CONV_FU`), `QCC_TryEvaluateCast`,
  `QCC_PR_RefExpression`.

## FTEF-32 Calls

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseFunctionCall`, `QCC_PR_GenerateFunctionCallRef`,
`QCC_PR_ParseDefs` (a local's initialiser), `QCC_PR_ParseRefValue` (`expandmemberfields`),
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

The semantic model checks calls in `crates/qc-sema/src/check/call.rs`.

### Behaviour

- **FTEF-32.1** The callee is read (FTEF-31.1); a value of a type that is no function or
  `variant` is the fatal error `E0256` "not a function" (FTEF-19.40), whatever names it. A
  `variant` takes any arguments.
- **FTEF-32.2** **Withdrawn** (#84): it said a variadic function is no intrinsic and did not say
  which calls are a method's; FTEQCC's test, `!newself.cast && !t->num_parms`, counts no `...`,
  and FTEF-32.10 replaces it.
- **FTEF-32.3** Each argument is read and, when `typecmp` tells its type from its parameter's,
  cast to it as an assignment's value is (FTEF-29.2), its conversion emitted: `f1(i)` of an
  integer is "itof emulation". An argument past the last parameter of a function that is not
  variadic warns `W0252` "too many parameters on call to NAME, argument ARG will be ignored", ARG
  as `QCC_GetRefName` names it; past the eighth of a variadic one `W0251` "More than 8 parameters
  on varargs function"; past the 136th (`MAX_PARMS + MAX_EXTRA_PARMS`) the fatal error `E0338`
  "More than 136 parameters". Missing arguments take their parameters' defaults; an `optional`
  parameter ends the count; else the warning `W0253` "too few parameters on call to NAME, PARAM
  will be UNDEFINED" (Q105), "too few parameters on call to NAME" for a parameter with no name.
  The value is the function's return type.
- **FTEF-32.4** A local's initialiser is the local's first store: read, converted to the local's
  type, and its cast reported here for an expression the resolver's initialiser check does not
  cast itself (a literal, `_()`, a name, a cast of a literal or an element, FTEF-19.31).
- **FTEF-32.5** In a member function a field named bare is `this.field` (FTEF-17.14), and so is
  a class's member field, which holds its own type; `::name` and `Scope::name` are the field
  itself (`expandmemberfields = false;`, "::classname is always usable for eg: the find
  builtin"), so `find(world, ::targetname, s)` passes the field and `find(world, targetname, s)`
  is "Cannot cast from string to .string". A field held as a `variant` casts to and from any
  field type silently.
- **FTEF-32.6** An `out` argument is stored back after the call (FTEF-29.3); a read-only one,
  a constant or an immediate, is the error `E0339` "Unable to write to read-only out argument",
  not fatal.

- **FTEF-32.7** `va_arg(index, TYPE)` (and `...(index, TYPE)`) reads its index, the `,` ("expected
  ,, found X" without it) and its second argument as a type (`QCC_PR_ParseType`, `"X" is not a
  type` for another token; any type, `string`, `vector`, `float*`, `.float`); the parser reads
  any call of that name so, a function of the name with parameters included. Its index is read
  as a float times 3 (`OP_MUL_F`), or else made an integer
  (`QCC_SupplyConversion`, "Implicit type mismatch. Needed integer, got TYPE.") times 3; in a
  function with no `...`, which has no `__va_list`, it is the fatal error `E0346` "va_arg()
  intrinsic only works inside varadic functions". Its value is of TYPE.
- **FTEF-32.8** A call's value is called no further: `QCC_PR_ParseFunctionCall` is reached from
  a name, and after a call's `)` the expression is read on as a value, so the `(` after it ends
  the expression. The construct around it then expects its own token: "expected ;, found (" at
  the end of a statement, and "expected ), found (" in parentheses or a call's arguments, the
  fatal error `E0204` (FTEF-24.10). `k(g()());` is "expected ), found (". An initialiser
  `float x = g()();` in a body is FTEQCC's "Missing semicolon at end of definition" and then a
  parse error; this profile reports "expected ;, found (" there, the same refusal.
- **FTEF-32.9** An argument may be left empty, a `,` or the `)` where it would start (`else if
  (arg < t->num_parms && (QCC_PR_PeekToken (",") || QCC_PR_PeekToken (")")))`): `f(1, , 3)`,
  `f(1, )`. It passes its parameter's default value; a parameter with none, an `optional` one
  among them, is the fatal error `E0375` "Default value not specified for implicit argument N",
  counted from 1. Past the parameters, and for a `variant`, which has none, the expression read
  there is missing, `"," - not a name` or `")" - not a name`. Each default passed so frees its
  reference once more than it holds it, so FTEQCC warns `W0319` "INTERNAL: over-freed refcount
  to NAME" (`QCC_FreeTemp`, `WARN_DEBUGGING`), the default as `QCC_VarAtOffset` prints it: `2f`
  for a float immediate, a constant's name for a constant. Under the vanilla dialect an empty
  argument is a missing expression. Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseFunctionCall`, `QCC_FreeTemp`, `QCC_VarAtOffset`.
- **FTEF-32.10** A call of a function with no named parameters, `...` or not
  (`!t->num_parms`), that is no method's is an intrinsic when its name is one, and a name no definition has is called as one (FTEQCC declares
  them on demand): `_` takes only a string immediate, else the fatal error `E0340` "_() intrinsic
  accepts only a string immediate"; `random` makes its arguments floats (`QCC_SupplyConversion`,
  the fatal error `E0227` "Implicit type mismatch. Needed float, got TYPE." for a type with no
  conversion); `randomv` takes vectors as they are, any other type the fatal error `E0340` "type
  mismatch on parm N"; `alloca` makes its size an integer, rounds it up to words (`OP_ADD_I` of 3,
  `OP_DIV_I` by 4) and pushes them by `OP_PUSH`, a plain statement, which only FTE's highest
  revision has ("Opcode \"<PUSH>|PUSH\" not valid for target. Consider the use of: #pragma
  target fte_6614"); `alloca`, `used_sound` and `used_model` read their first argument whatever
  follows the `(` (`QCC_PR_Expression`), so with none they are the fatal error `E0201` "\")\" -
  not a name"; `entnum(e)` is `(e / 1i) /
  (nextent(world) / 1i)` by `OP_DIV_I`, and with no `nextent` the fatal error `E0347` "the nextent
  builtin is not defined"; `used_sound` and `used_model` take a string immediate, else the error
  `E0340` "Argument to used_sound intrinsic was not a string immediate.".
  A method's call is one made through an object's member, `c.m()`, or of a member function named
  bare in another, which passes `this` (`newself`). Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseFunctionCall`.
- **FTEF-32.11** A call names its callee as `QCC_GetSRefName` does: a definition by its name, a
  non-virtual member function's `CLASS::name` however it is reached (`QCC_RefToDef` of the
  member), and a temporary, a field's read and so a virtual method's, by its type's name, which
  for a function type FTEQCC parses is `__function`: "too many parameters on call to
  __function, argument IMMEDIATE will be ignored". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_GetSRefName`, `QCC_PR_ParseFunctionCall`; `engine/qclib/qcc_pr_lex.c`,
  `QCC_PR_ParseFunctionType`.
- **FTEF-32.12** `entnum` and `autocvar` are intrinsics only when an argument follows the `(`:
  `!QCC_PR_CheckToken(")")` takes the `)` of `entnum()` and `autocvar()`, and the call that
  follows reads its argument from the token after it, so `entnum();` is the fatal error `E0201`
  "\";\" - not a name". `autocvar` reads its name, a `,`, its default and at most a description:
  with no `,` after the name it is the fatal error `E0204` "expected ,, found )", and with a
  fourth argument "expected ), found ,". Source:
  fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `engine/qclib/qcc_pr_comp.c`,
  `QCC_PR_ParseFunctionCall`.
## FTEF-33 Builtin argument checks

**Source:** `qcc_pr_comp.c`, `QCC_VerifyFormatString`, `QCC_VerifyArgs_sendevent`,
`QCC_VerifyArgs_setviewprop`, `QCC_VerifyArgs_cvar`, `QCC_VerifyArgs_MatchingFieldType`, called
from `QCC_PR_GenerateFunctionCallRef` by the callee's name,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Each argument is checked as it is passed: of its parameter's type when it was cast to one
(FTEF-32.3), else of its own. A string's text is known for a literal and a constant initialised
with one. The semantic model checks them in `crates/qc-sema/src/check/format.rs`; FTEQCC's notes
after the warnings ("use %s or %S for strings") are not reported.

### Behaviour

- **FTEF-33.1** `sprintf` with a constant format string reads it directive by directive (`%%`
  is none): an argument position `N$`, flags, a width (`*` takes an argument, `*N$` the Nth, which
  must be a float, "width modifier requires float at arg N"), a precision (the same, "precision
  modifier ..."; a `.` with neither is "invalid format string: REST"), length modifiers (`h` a
  float, `l` and `L` an integer, `q` 64 bits; `j`, `z` and `t` warn "length modifier C is a
  placeholder"), and a conversion. A conversion assumes a float unless `%i`, `%p` or a modifier
  says otherwise: `d i c o u x p e f g` and their capitals require a float (a double with `q`),
  or with `l` an integer ("DIRECTIVE requires int at arg N (got TYPE)", warned twice, an entity
  taken for `%i`), `%p` a pointer; `%v` a vector, or with `l` "intvector"; `%s` a string; a
  `variant` is taken by any; another conversion is "invalid format string: REST", which ends the
  check. Each mismatch warns `W0254` "NAME: DIRECTIVE requires TYPE at arg N (got TYPE)", an
  argument past the last a `void`. Arguments past the last taken warn "NAME: surplus trailing
  TYPE argument(s) for format \"FORMAT\"". F324, `WARN_FORMATSTRING`.
- **FTEF-33.2** `cvar("vid_conwidth")` and `cvar("vid_conheight")` warn that they are
  deprecated; `setviewprop` and `setproperty` with a constant property check its values' basic
  types against FTEQCC's table ("NAME(PROPERTY, ...): expected TYPE, got TYPE", then `X, ` and
  `X, Y, ` for the second and third) and warn "NAME: unknown argument N" for a number not in it;
  `addstat` and `clientstat` with three arguments compare a constant type number with the
  field's basic type ("indicated type ev_TYPE does not match passed field type .TYPE", or "is
  not a basic type"), a field of a `variant` taking any; `sendevent` with a constant type list
  checks each argument after the second against its letter (`s f F i I u U v e`), "arg type list
  longer than args", "'C' is not a recognised arg type", "'C' specified, but was passed TYPE",
  and "cannot pass more than 8 args to a builtin.". Each warns `W0255`, F333,
  `WARN_ARGUMENTCHECK`.


## FTEF-34 Statements

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseStatement`, `QCC_PR_ParseStatement_For`,
`QCC_PR_FinaliseFunction`, `QCC_CheckForDeadAndMissingReturns`; `qcc_pr_lex.c`
(`QCC_PR_ParseDefaultInitialiser`, in `qcc_pr_comp.c`), fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

The semantic model checks statements in `crates/qc-sema/src/check/statement.rs`, and keeps the
layout of the jumps they generate in `crates/qc-sema/src/check/flow.rs`: each statement as
`OP_RETURN`, `OP_DONE`, `OP_GOTO`, a conditional jump or any other, with where each jump goes.
An expression stands for the statements it generates by one, when it generates any: any but a
name, a literal or a constant. The layout is FTEQCC's at its default optimisation level, without
`compound_jumps` and `return_only`; `-O2` and `#pragma optimise` change only the build here
(FTEF-12.7), and so do not change it.

### Behaviour

- **FTEF-34.1** `return;` in a function whose type is not `void` warns `W0256` "'NAME' returned
  nothing, expected TYPE" (`WARN_MISSINGRETURNVALUE`, Q203), unless the function is marked
  `accumulate` or `return =` or an earlier `return;` gave it its return value
  (`pr_scope->returndef`), so it warns once. `return = e;` reads `e` and stores it with no type
  check (`QCC_StoreSRefToRef`). `return e;` of `__NULL__` or a null constant returns from any
  function; a value of another basic type than the function's is cast to it implicitly
  (`QCC_EvaluateCast(e, type, true)`, FTEF-19.31), and from a `void` function warns `W0257`
  "'NAME' returned TYPE, expected void" (`WARN_WRONGRETURNTYPE`, Q204) instead.
- **FTEF-34.2** `if (e)` tests `e` with `QCC_Generate_OP_IFNOT` (FTEF-30.2), `if not (e)` and
  `if !(e)` with `QCC_Generate_OP_IF`; `if !(e)` of a string or a float tests `!e`
  (FTEF-30.1) with `QCC_Generate_OP_IFNOT` instead. `if !()` warns `W0261` "if !() is specific
  to fteqcc", and `if not()` "if not() is specific to fteqcc or hexen2" but on the Hexen II
  targets (`WARN_FTE_SPECIFIC`, F311, off by default). A constant condition generates no test,
  and the branch it never takes generates nothing unless it defines a label.
- **FTEF-34.3** `while (e)` tests `e` as `if` does, `until (e)` with `QCC_Generate_OP_IF`; a
  constant float condition generates no test, or a `goto` past the loop when it never holds.
  `do s while (e);` tests `e` with `QCC_Generate_OP_IF`, `do s until (e);` with
  `QCC_Generate_OP_IFNOT`; a constant condition of any type generates no test, or a `goto` back
  when it holds.
- **FTEF-34.4** `for (init; e; step)` tests `e` with `OP_IFNOT_I` and no test of its type; the
  step is checked before the body and generated after it.
- **FTEF-34.5** `switch (e)` uses `OP_SWITCH_F`, `_E`, `_V`, `_S` or `_FNC` by `e`'s type when the
  target has it. A `case` value may generate no code, else the fatal error `E0342` "Case
  statements may not use formulas"; a read is not generated until the end of the switch, so a
  field of an entity is a value. After the body each case value of another basic type is made
  the switch's (`QCC_SupplyConversion`, FTEF-22.3), and without the opcode compared with `e`: by
  `OP_EQ_F`, `_E`, `_V`, `_S`, `_FNC` (a function or a field), `_P`, `_I` (an integer or
  unsigned), `_I64` or `_D`, which warn as any comparison does (FTEF-28), and a constant 0 by
  testing `e` itself; any other type is the fatal error `E0343` "Bad switch type". A range
  `case a .. b` is `e >= a && e <= b` of a float, a double or an integer of any width, and of
  any other type warns `W0260` "switch caserange MUST be a float or integer"
  (`WARN_SWITCHTYPEMISMATCH`, which has no name). A strict enum's opcode and equality are its
  base type's, its range not, and a value of another type is no enum ("Implicit type mismatch.
  Needed enum, got TYPE."). Before the comparisons each item of a strict enum switched on that
  no case covers warns `W0263` "ENUM::ITEM not part of switch" (the warning of code 0), the
  last item first (each is prepended to the type's `accessors`); a `default`, a range, or a case
  whose value is no constant of the enum's own type stops the search, covering every item.
- **FTEF-34.6** When a function is finished (`QCC_PR_FinaliseFunction`), a `return` of the value
  `return =` set is added, then `OP_DONE`, and `QCC_CheckForDeadAndMissingReturns` reads the
  layout: a function whose type is not `void` warns `W0258` "NAME: not all control paths return
  a value" (`WARN_MISSINGRETURN`, Q206) when its last statement before `OP_DONE` is no `return`
  and no backward `goto`, or when a conditional jump goes to that `OP_DONE`; a statement after a
  `return` or a `goto` that is no `return`, `goto` or `OP_DONE` and that no jump reaches warns
  `W0259` "NAME: contains unreachable code (line N)" (`WARN_UNREACHABLECODE`, F323), N its line
  as `pr_source_line` has it, which `#pragma line` changes.
  The statement after a `return` or `goto` is not itself tested for a jump to the end. A function
  whose body had an error, which ends FTEQCC's parse of it (`QCC_PR_ParseError`), is not checked,
  nor is a function marked `accumulate`, whose parts are finished together. So `float() f = {
  if (a) return 1; };` warns, and `float() f = { if (a < 0) return 1; if (a) return 1; };` does
  not: the second test follows the first `return`.
- **FTEF-34.7** `thinktime e : t;` reads `e`, makes `t` a float (`QCC_SupplyConversion`), and is
  the fatal error `E0344` "thinktime type mismatch" unless `e` is an entity and `t` a float.
- **FTEF-34.8** A function returning a type larger than a vector (FTEF-6.13) is passed a pointer
  to the value by its caller: its body starts by storing it with `OP_STORE_P`, which a target
  without it refuses ("Opcode \"=|STORE_P\" not valid for target", FTEF-27.5, a fatal error that
  ends the function's checks), so `return;` "'NAME' returned nothing, expected TYPE" and
  `return =` "'NAME' not supported when returning TYPE" are the fatal error `E0341`, and the
  finished function warns `W0262` "Function NAME returns (large) TYPE. This is inefficient."
  (`WARN_SLOW_LARGERETURN`). The return messages of FTEF-34.1 name a type by its own name
  (FTEF-24.11), and this one by `TypeName`.
- **FTEF-34.9** An initialiser of a global that is an expression, no literal, name, cast of a
  literal or `_()`, is read as a body's expression is at global scope (`pr_scope` null): each
  statement it generates is the error `E0345` "Unable to generate statements at global scope
  (OPNAME)." (`QCC_PR_StatementFlags`, `ERR_BADEXTENSION` as a warning, so the read goes on),
  OPNAME the opcode's: an operator's that is not folded, `LOADF_F` and the like for an entity's
  field, `STORE_F` for each component of a vector built, the stores of a call's arguments (by
  their words, `STORE_V`, `STORE_I64` or `STORE_F`; an argument of more than three words copied
  as it is read, three words a parameter, by the widest store the target has for the words left,
  and the others stored after every argument is read; on a target with `OP_CALL1H` the first two
  parameters passed with no store) and `CALLn`, or `CALLnH` with `OP_CALL1H` (`CALL0` with no
  argument), `IFNOT`, `STORE_*`, `GOTO` and
  `STORE_*` for `?:` (the first arm stored by its size, `STORE_V`, `STORE_I64` or `STORE_F`,
  the last by its type as `QCC_StoreToSRef` stores it, a double's with plain statements that
  report nothing, an `OP_STORE_I64` two `STORE_FLD`s on a target without it, and a store of a
  temporary the arm's last statement made folded into it, `opt_assignments`), and the `DIV_F` or `MUL_F` of an int conversion's emulation. The value is
  then cast to the global's type, and a value that is no constant is `E0214` "initializer for
  'NAME' is not constant". So `float b = a + 1;` gives both, `float b = 1 + 2;` neither, and
  `const vector v = [19, 20];` is a constant.
- **FTEF-34.10** A `switch`'s cases are compared in order after its body is read, and a second
  `default` is the fatal error `E0352` "Duplicated default case" there (`if (defaultcase >= 0)
  QCC_PR_ParseError(ERR_MULTIPLEDEFAULTS, "Duplicated default case");`), after the errors of the
  cases before it: `switch (1) { default: case 1: default: ; }` is refused, and `switch (1) {
  case "a": default: default: ; }` reports the string case's mismatch instead.

- **FTEF-34.11** An opcode `QCC_PR_StatementFlags` refuses (FTEF-27.5, `badopcode`, an error but
  no fatal one) is still emitted, so at global scope it is also "Unable to generate statements at
  global scope (OPNAME).": `string t = s + 1;` on the id target is "OP_ADD_SF: string+float may
  be unsafe", `AddInt`'s and `ADD_I`'s refusals, and "Unable to generate statements at global
  scope (ADD_I).".
- **FTEF-34.12** At the end of each function, before the illegal `case`, `continue` and `break`
  checks, every `goto` read since the last function's end must name a label read since then
  (FTEF-18.23): `for (i = 0; i < num_gotos; i++) { for (j = 0; j < num_labels; j++) if
  (!strcmp(pr_gotos[i].name, pr_labels[j].name))`, and otherwise
  `QCC_PR_ParseError(ERR_NOLABEL, "Goto statement with no matching label \"%s\"",
  pr_gotos[i].name);`, the fatal error `E0308` at the function's closing `}` for the first such
  `goto`, which ends the function's checks. A `goto` in a stripped `if` arm is no longer among
  them (`QCC_UngenerateStatements` takes back `num_gotos`), where a label there stays. A
  function defined inside another ends the same way, checking the enclosing function's `goto`s
  read before it and its own against the enclosing function's labels read before it and its
  own, and then forgets both: a label after the inner function does not help them, and a
  `goto` after it does not find a label before it; its error ends the enclosing function's
  checks. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_PR_ParseImmediateStatements`, `QCC_UngenerateStatements`.
- **FTEF-34.13** "NAME: contains unreachable code (line N)" (FTEF-34.6) is given at the line of
  the unreachable statement (`statements[st].linenum`), and "not all control paths return a
  value" at the function's end. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`, `QCC_CheckForDeadAndMissingReturns`.
- **FTEF-34.14** These differences from FTEQCC's messages are not modelled by this profile, each
  for its reason. Code generation's, which the compiler (QC-25) owns, as the checks report
  nothing a program's meaning depends on: the slots `QCC_GetTemp` keeps across functions, so a
  later function's `temp_N` may be numbered higher (FTEF-27.16); a narrow integer read again in
  a function, whose extension FTEQCC emulates only the first time; a `__double` stored to a
  `_Bool`, which FTEQCC follows with one more "ftoi emulation"; and the offset constants a
  struct's member array frees again (FTEF-29.12). The text of an error in a program both
  refuse: `typedef ;` warned at the line of the token after the `;`; `(T *)0` and `typedef for
  x;` with `T` and `for` no type; `[x, 0, 0]_x`; and `spawn(float)`, whose type keyword the
  semantic model reads as a value. Source: fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c`.
