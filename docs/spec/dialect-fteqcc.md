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
- **FTEF-8.4** An `enum` or `enumflags` where a type belongs always has its body:
  `QCC_PR_ParseType` sends both keywords to `QCC_PR_ParseEnum` (`else if
  (QCC_PR_CheckKeyword(keyword_enum, "enum")) type = QCC_PR_ParseEnum(false);`), which reads an
  optional `class`, a name or legacy base type and an optional `: type`, then
  `QCC_PR_Expect("{");` on every path that has not already read the `{`
  (`qcc_pr_comp.c`, `QCC_PR_ParseEnum`). So `enum colour_e c;` and `typedef enum colour_e C;`
  are "expected {" errors; an enum is named elsewhere by its type name alone (FTE-15.4),
  `colour_e c;`.

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
- **FTEF-12.9** **Withdrawn** (P2-42, #429): it said a later `#pragma autoproto` reports
  nothing, but FTEQCC warns for it. Replaced by FTEF-12.10.
- **FTEF-12.10** `#pragma autoproto` before any definition turns on the autoprototyping of
  FTE-22.3 for the module, as `-Fautoproto` does, and `#pragma autoproto 0` turns it off:
  `QCC_PR_Precompiler` sets `autoprototype = *msg?!!atoi(msg):true;` only
  `if (numpr_globals == RESERVED_OFS)`, and otherwise warns "#pragma autoproto must appear
  before any definitions". It holds for one module: `QCC_main`, run once per `progs.src`,
  starts with `parseonly = autoprototyped = autoprototype = false;`. A list-form `progs.src`
  is read through the preprocessor (`QCC_ContinueCompile` calls `QCC_PR_LexWhitespace(false)`
  before each entry), so a `#pragma autoproto` there, as Copper's `progs.src` has, holds for the
  whole module. This profile takes a pragma in a compiled file only before the first token of
  any compiled file. A later one is ignored with the warning `W0108` "#pragma autoproto must
  appear before any definitions" (`WARN_BADPRAGMA`, on by default) at the directive, once: the
  check stands inside `if (!autoprototyped)`, so the second autoprototyping pass skips the
  pragma without a word.
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

## FTEF-14 Preprocessor directives and macros

**Source:** `qcc_pr_lex.c`, `QCC_PR_Precompiler`, `QCC_PR_FalsePreProcessorIf`,
`ParsePrecompilerIf`, `QCC_PR_SkipToEndOfLine`, `QCC_PR_PreProcessor_Define`,
`QCC_PR_SimpleGetToken`, `QCC_PR_LexWhitespace`, `QCC_PR_ExpandPreProcessorMacro`,
`QCC_PR_ExpandStrCatMarkup`, `QCC_PR_IncludeChunkEx`, `QCC_PR_CheckCompConst` and
`QCC_PR_CheckBuiltinCompConst`; `qcc_cmdlib.c`, `QCC_COM_Parse2`,
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Vanilla preprocessor:
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
- **FTEF-14.3** `#warning text` is the warning `W0103` "#warning: text"
  (`QCC_PR_ParseWarning(WARN_PRECOMPILERMESSAGE, "#warning: %s", msg);`) and `#error text` the
  error `E0118` "#Error: text" (`QCC_PR_ParseError(ERR_HASHERROR, "#Error: %s", msg);`), each on
  the side of a conditional that is taken. `#message text` prints the text and reports nothing.
  The text is the rest of the line as written (`msg[a] = pr_file_p[a];` up to the line break),
  not tokens, so a `'` in it (`#warning Toggle doesn't work`) opens no vector.
- **FTEF-14.4** `#includelist` to `#endlist` lists files that are compiled in order where the
  list stands (`QCC_FindBestInclude(pr_token, compilingfile, true);` for each word). The module
  graph reads the list and compiles each file (PRJ-5.3), so the preprocessor produces no token
  for the lines of the list. A `#endlist` with no list open is accepted.
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
- **FTEF-14.8** `#append NAME text` adds `text` to the body of macro `NAME`, or defines it when it
  is not defined (`append with no previous define is treated as just a regular define`); `#eof`
  ends the file (`pr_file_p = NULL;`). `#output`, `#forcecrc`, `#copyright`, `#pack`,
  `#package`, `#merge` and `#datafile` are accepted and produce no token: their effects are on
  the build, not on the token stream.

- **FTEF-14.9** A use of a function-like macro may give one argument more than its
  parameters, which is unused: `QCC_PR_ExpandPreProcessorMacro` checks the count only at a
  comma, after counting the argument before it, with
  `if (param == MAXCONSTANTPARAMS || param > c->numparams) QCC_PR_ParseError(ERR_TOOMANYPARAMS, "Too many parameters in macro call");`,
  so the last argument is never counted against the parameters. Two or more extra arguments are
  "macro NAME expects N arguments, found M" (QCD-8.9). The corpus relies on it
  (`EVALUATE_FIELD(mins, 0, PLAYER_SIZE)` for a two-parameter macro, `shared/game/Player.qc`).
- **FTEF-14.10** A conditional block still open at the end of the file is reported (QCD-8.3) only
  when a side of it is being skipped, as FTEQCC's "#if with no endif" comes from
  `QCC_PR_FalsePreProcessorIf`, which skips; a taken side left open is closed at the end of the
  file without a word. The corpus leaves one open (`menu-fn/m_customgame.qc`, line 18).
- **FTEF-14.11** Under this profile an `#if` or `#elif` expression is read as
  `ParsePrecompilerIf` reads it, not with the C precedence of PRJ-5.5, by the preprocessor and by
  the graph of a module of this dialect. `||` and `&&` share one level and group to the right,
  the right side read first: `eval = ParsePrecompilerIf(level)||eval;` and
  `eval = ParsePrecompilerIf(level)&&eval;` under `case PPI_LOGICAL:`, so `0 && 0 || 1` is
  `0 && (0 || 1)`, 0, where C makes it 1. The comparisons `<=`, `>=`, `<`, `>`, `!=` and `==`
  share the level below in the same way (`eval = eval <= ParsePrecompilerIf(level);` under
  `case PPI_COMPARISON:`), so `1 < 2 == 1` is `1 < (2 == 1)`, 0. A value (`PPI_VALUE`) is a
  parenthesised expression, `!` and a value, `defined` and a name with or without parentheses,
  or a word as `QCC_PR_SimpleGetToken` reads it, up to whitespace, `,`, `;`, `(`, `)`, `]` or
  `//`, whose macro's text, or else the word itself, is read with `atoi`: `1||0` is one word,
  1. The text after a whole expression is not read. The expression is the directive's bytes as
  written, a line continuation (FTEF-14.30) a space. Where FTEQCC reports an error ("unclosed
  bracket condition", "unclosed defined condition", "unexpected end-of-line") this profile
  takes the expression as false.
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
  call `M` (FTEF-14.29). `#append NAME ...` (FTEF-14.8) is read the same way and adds a space and
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
- **FTEF-14.30** A directive continues on the next line when its line ends in `\` directly
  before the break: `QCC_PR_SkipToEndOfLine` reads `else if (*pr_file_p == '\\' && pr_file_p[1] ==
  '\n') { /*linux endings*/ pr_file_p+=2;`, and `QCC_PR_PreProcessor_Define` "read[s] over a
  newline if necessary" after a `\`. A `/* */` comment that spans lines does not end the directive
  ("multi-line c style comments become part of the define itself"); a `//` comment ends its text.
  `QCC_PR_SkipToEndOfLine` goes on past a `\` and line break in that comment too, which this
  profile follows for `#define` and `#append` (FTEF-14.24) and, a known difference, not yet for
  the other directives, whose line it ends at the comment's line break (P2-94): the pinned FTEQCC
  passes over `zz zz zz` after `#undef X // c \` and after `#ifdef X // c \`.
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
- **FTEF-14.32** A built-in constant (FTEF-14.7) is the text `QCC_PR_CheckBuiltinCompConst`
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
  preprocessor each file's path as its input names it, the `progs.src` entry only where the
  caller makes it so (P2-95).

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
  break; //float(void) has no actual args`). `...` ends the list, after a type or alone, and may
  be followed by the name of the argument count.
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
- **FTEF-16.9** A clause of a `for` header may hold several expressions separated by `,`, and the
  first clause a declaration after them (`v = 0, int i = 1`).
- **FTEF-16.10** A call's result may be assigned to, as in the chain `a = f() = b` the corpus
  writes; FTEQCC reports no error there, so neither does this profile.
- **FTEF-16.11** An aggregate initialiser may name the member each value initialises,
  `{ .name = value, ... }`.
- **FTEF-16.12** A `case` or `default` label may stand in a block nested in the `switch` body,
  as C allows; the corpus does so (`client/viewmodel.qc`).

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
- **FTEF-17.2** A class's members are found through the class and its parents
  (`QCC_PR_ParseEntClass` keeps each class's members, with `newt->parentclass`). `Class::member`
  defines the member the class or a parent declares, or adds one, and its name refers to the
  member's declaration. In a member function's body a name is the function's own local or
  parameter, then a member of its class or a parent (`assumeclass`), then a global; a class's
  own name there is the class, as `spawn(Thing)` takes it, not its constructor.
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
  value (FTE-22.5), a builtin by its number and engine name as FTE-23.7 says, any other
  initialiser, a builtin FTEQCC binds no number for among them, by its text, token for token.
  An enum's names are never redeclared: `QCC_PR_ParseEnum` makes each with
  `QCC_PR_DummyDef`, which looks for no earlier definition. A function body is never equal to
  another.
- **FTEF-17.6** Under this profile a redeclaration is a type mismatch only when neither type
  involves a type name, a pointer or an unknown type, whose meaning the model does not follow
  (a typedef may name `float`); otherwise QC-11.5 holds as written.
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

## FTEF-18 Further statements, terms and frame macros

**Source:** `qcc_pr_comp.c`, `QCC_PR_ParseInitializerType_Internal`,
`QCC_PR_GenerateInitializerType`, `QCC_PR_ParseState`, `QCC_PR_ParseStatement`,
`QCC_PR_RefExpression`, `QCC_PR_RefTerm`, `QCC_PR_ParseDefs`, `QCC_PR_DiscardExpression`,
`QCC_PR_DiscardRef`, `QCC_PR_ParseImmediateStatements`, `QCC_TryEvaluateCast`, `QCC_Include`;
`qcc_pr_lex.c`, `QCC_PR_LexMacroName`, `QCC_PR_LexGrab`, `QCC_PR_CheckKeyword`,
`QCC_PR_MacroFrame`, `QCC_PR_FindMacro`, `QCC_PR_IncludeChunkEx`; `qccmain.c`, the keyword
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
- **FTEF-18.9** `until (e) s` repeats `s` while `e` is false, and `do s until (e);` is the
  `do` loop with the test reversed: `wasuntil = QCC_PR_CheckKeyword(keyword_until, "until"); if
  (wasuntil || QCC_PR_CheckKeyword(keyword_while, "while"))`, and in `do`, `until =
  QCC_PR_CheckKeyword(keyword_until, "until"); if (!until) QCC_PR_Expect ("while");`. `until` is
  a keyword only for the Hexen II targets (`nondefaultkeyword` in the keyword table, forced on
  `if (qcc_targetformat == QCF_HEXEN2 || ...)`), or when written `__until`, which
  `QCC_PR_CheckKeyword` matches whether or not the keyword is enabled (FTEF-15.1); otherwise
  `until (e)` is a call of a function named `until`.
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
- **FTEF-18.22** FTEQCC tells the readings of source apart by `s_filen`, a string it makes anew
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
    which `QCC_PR_PrintWarning` prints as an error (FTEF-18.4). The frame number still goes up
    by one for it;
  - a `$modelname` of 64 bytes or more is cut to its first 63 with the warning `W0107`
    "$modelname name too long": `if (!QC_strlcpy(pr_framemodelname, pr_token,
    sizeof(pr_framemodelname))) QCC_PR_ParseWarning (WARN_STRINGTOOLONG, ...)`, with
    `char pr_framemodelname[64];`.

  The include goes on with the frame numbering of the file that includes it, which only a
  compiled file restarts (FTEF-18.17). This profile reports a frame macro used inside a macro
  expansion at the macro body's token.
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
- **FTEF-18.24** At the end of each function, a function defined inside another included, every
  `goto` read since the last function's end must name a label read since then (FTEF-18.23):
  `for (i = 0; i < num_gotos; i++) { for (j = 0; j < num_labels; j++) if
  (!strcmp(pr_gotos[i].name, pr_labels[j].name))`, and otherwise
  `QCC_PR_ParseError(ERR_NOLABEL, "Goto statement with no matching label \"%s\"",
  pr_gotos[i].name);`, after which `num_gotos = 0;`. This profile reports each such `goto` as
  `E0308` at its label name, where FTEQCC stops at the first. A label later in the same
  function is found and one of another function is not. A function defined inside another
  checks, and clears, the enclosing function's `goto`s read before it, against the enclosing
  function's labels read before it and its own: a label after the inner function does not help
  them, and a `goto` after it does not find a label before it.
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
- **FTEF-18.26** Under this profile the nesting limit of QC-27.15 counts the FTEQCC constructs
  too: the operand of a prefix `-`, `+`, `~`, `++`, `--`, `&` or `*` (FTE-19.4, FTEF-16.7), of
  a cast (FTEF-16.2) and of `sizeof` (FTEF-18.12) is one level inside it, as the operand of `!`
  is, each `*` and each parameter list of a type (FTEF-4.1, FTEF-18.15) nests the type one
  level deeper, and so does each `*` before a declarator's name (FTEF-15.6). QC-27.16 then
  holds as for vanilla: a prefix `++` or `--` it unwinds through is read as no postfix one, and
  a type or a declarator takes no further `*` or parameter list. FTEQCC has no such limit.
- **FTEF-18.27** This profile's error recovery (QCD-1.4) treats the full dialect's bodies as a
  function body: a struct's fields, a class's members and an accessor's members end at a
  declaration line, a token at column 0 that has a declaration's shape or is one of `typedef`,
  `class`, `accessor`, `enum`, `enumflags`, `struct` and `union`, as well as at their `}`; and
  a class member's recovery stops at the `}` that closes the class. So a body that lost its
  `}` damages no declaration after the next. FTEQCC reads each body to its `}`; a field or
  member written at column 0 in the shape of a declaration therefore ends its body here, as a
  statement at column 0 ends a function body, and none of the codebases the corpus job
  resolves under this profile writes one.

  Under the vanilla profile none of this is reported.
- **FTEF-18.28** A frame cycle, `[++ first .. last]` or `[-- first .. last]`
  (`if ((isinc=QCC_PR_CheckToken("++")) || QCC_PR_CheckToken("--"))`), has `first` and `last`
  as expressions separated by `QCC_PR_Expect("..")`; `(w)` or `(W)` after the `++` or `--`
  cycles `weaponframe` instead of `frame` (`OP_CWSTATE`). Two constant frames against the
  direction, `if (first->_float > last->_float)` after `++` or `<` after `--`, are the error
  `E0212` "Forwards State Cycle with backwards range": FTEQCC's `QCC_PR_ParseWarning` with
  `ERR_STATETYPEMISMATCH`, an error code, prints an error. This profile compares two frames
  written as single numbers.

### Test vectors

Larger vectors: [`vectors/dialect-fteqcc/`](vectors/). The recorded coverage run over the
FTEQCC corpus mods is
[`vectors/dialect-fteqcc/coverage.txt`](vectors/dialect-fteqcc/coverage.txt). The vectors of
the `Codegen (later)` statements are added when L-01 (codegen) fixes them, each checked
against FTEQCC.
