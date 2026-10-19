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
cite); it was read on 2026-10-17. FTEQCC is not a registered porting upstream
([`../porting.md`](../porting.md)), so nothing here is ported from it: these statements are
this project's own rules, written to parse and resolve what FTEQCC accepts.

This file is the parse-and-resolve spec (P2-25): syntax and type rules. **Code generation is
out of scope and is marked later.** Each section's final statement is a `Codegen (later)`
note that names the issue (L-01) which fills in the progs output; until then those statements
do not exist here and are not cited. Many of these constructs need FTE's version 7 progs
format ([`progs.md`](progs.md) PROGS-19 to PROGS-21), unlike the rerelease subset, which fits
version 6 (`dialect-fteqcc-rerelease.md` FTE-1).

To be implemented by the dialect profiles and `qc-sema` (P2-26); until code cites them the
resolution statements are in [`coverage-baseline.tsv`](coverage-baseline.tsv). The
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
`long`, `short`, `char`, `int`, `integer`, `float`, `double`, `_Bool`), the type globals
(`type_integer`, `type_uint`, `type_int64`, `type_uint64`, `type_sint8`, `type_uint8`,
`type_sint16`, `type_uint16`); `qccmain.c`, the `compiler_flag[]` keyword table,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`.

Vanilla `qcc` has `void`, `float`, `vector`, `string` and `entity` only (QC-10.1): `int`,
`integer`, `bool`, `double` and the rest are read as names and are "not a type" (QC-11). The
corpus uses `int` heavily (3392 occurrences across the three mods) and `bool` (1312).

### Behaviour

- **FTEF-2.1** `int` and `integer` both name a 32-bit signed integer type. FTEQCC defines them
  as `type_integer` (`QCC_PR_NewType("__int32", ev_integer, ...)`), and the two keywords
  `keyword_int` and `keyword_integer` both resolve to it (`QCC_PR_ParseType`, the C-type
  branch, `int`/`integer` set `bits = 32`). The corpus declares locals, globals, fields,
  parameters and return types of this type.
- **FTEF-2.2** `unsigned` makes an integer type unsigned (`type_uint`); `signed` is the
  default and accepted. The C width keywords resolve as FTEQCC's `QCC_PR_ParseType` sets them:
  `char` is 8-bit (`type_sint8`/`type_uint8`), `short` 16-bit (`type_sint16`/`type_uint16`),
  `int`/`integer` 32-bit (`type_integer`/`type_uint`), `long` 64-bit
  (`type_int64`/`type_uint64`). `long long` is also 64-bit, with FTEQCC's warning "long longs
  are not supported, using long".
- **FTEF-2.3** `double` is a 64-bit float type (`type_double`); `float` stays the 32-bit QC
  float (QC-10). `_Bool` resolves to `type_bint` or `type_bfloat` by the `int` keyword flag
  (`QCC_PR_ParseType`, the `_Bool` case). The corpus's `bool` is this project's rendering of a
  boolean scalar; FTEQCC's own `bool` comes from the engine's extension headers
  (`fteextensions.qc`), so it is a defined type rather than a built-in keyword, and resolves
  as that definition says.
- **FTEF-2.4** FTEQCC accepts combinations in any order that C allows (`signed`/`unsigned`
  with `long`/`short`/`int`/`char`, `long int`, `short int`, `long long`), parsed by the
  keyword loop in `QCC_PR_ParseType`. It warns and narrows where a width is unsupported
  ("long doubles are not supported, using double"); the narrowing is FTEQCC's and this spec
  follows it.
- **FTEF-2.5** `const` and `volatile` are accepted type qualifiers; FTEQCC's `QCC_PR_ParseType`
  reads and warns "ignoring unsupported const keyword" and treats everything as volatile. The
  qualifier changes whether the name may be assigned, not its type (see also
  `dialect-fteqcc-rerelease.md` FTE-10 for `const`).
- **FTEF-2.6** Codegen (later): how each width is stored and which need version 7 is fixed by
  L-01.

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
- **FTEF-10.4** Codegen (later): an accessor read or write lowers to its getter or setter call;
  fixed by L-01.

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
- **FTEF-12.4** `#pragma flag name on|off` (the alias `keyword`) enables or disables a compiler
  flag or keyword mid-compile (`QCC_PR_Precompiler`, the `keyword`/`flag` case, over the
  `compiler_flag[]` table, honouring `FLAG_MIDCOMPILE`); it warns "compiler flag state not
  recognised", "keyword/flag %s not recognised", or "Cannot enable/disable keyword/flag via a
  pragma". The corpus uses it 11 times.
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
- **FTEF-12.8** A `#pragma` affects the build and diagnostics, not the token stream; it emits
  no progs output of its own (as FTE-16.3). Codegen (later): any effect a pragma has on the
  output (the target format, the CRC, the flags) is realised by L-01.

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
  FTEF-13.4), a byte of 0x80 or more (a UTF-8 sequence), or a byte followed by `'`:
  `QCC_PR_LexVector` reads `if (*pr_file_p == '\\') {//extended character constant`, the
  `utf8_check` branch, and `else if (pr_file_p[1] == '\'') {//character constant`. Its value is
  the character's code, a float unless `flag_assume_integer`. Otherwise the `'` opens a vector,
  which may stop after two components, the third 0, with FTEQCC's warning "2d vector"
  (`if (*pr_file_p == '\'' && i == 1)`).
- **FTEF-13.4** Besides the one-byte escapes of `dialect-fteqcc-rerelease.md` FTE-20.4,
  `QCC_PR_LexEscapedCodepoint` maps `\[` and `\]` to `0xe010` and `0xe011`, `\.`, `\<`, `\-` and
  `\>` to `0xe01c` to `0xe01f`, `\(`, `\=`, `\)` and `\+` to `0xe080` to `0xe083` ("quake
  specific"), `\{n}` and `\{xN}` to the decimal or hex code `n`, `\x` and one or two hex digits
  to that byte, `\u` and `\U` and four or eight hex digits to that code point, and a digit to
  `0xe012` plus the digit under `flag_qcfuncs` ("yellow numbers"). A code above one byte is
  written to the string as its UTF-8 bytes. Any other escape stays "Unknown escape char"
  (QC-6.2).
- **FTEF-13.5** This parse-and-resolve profile keeps every number token of FTEF-3 as a float
  holding its value (an integer rounded to the nearest float), and every character constant as
  a float holding its code: the literal's FTEQCC type (FTEF-3.1, FTEF-3.3) matters only to type
  checking and code generation, which are later (L-01).

### Test vectors

Larger vectors: [`vectors/dialect-fteqcc/`](vectors/). The recorded coverage run over the
FTEQCC corpus mods is
[`vectors/dialect-fteqcc/coverage.txt`](vectors/dialect-fteqcc/coverage.txt). The
resolution statements (the `.2`/`.3` and later statements that say "Codegen (later)") are
filled in when P2-26 (parse and resolve) and L-01 (codegen) land, each checked against FTEQCC.
