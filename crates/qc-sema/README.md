# qc-sema

The incremental QuakeC semantic model. It turns the lossless syntax trees of
[`qc-syntax`](../qc-syntax/README.md) and the ordered file graph of
[`qc-project`](../qc-project/README.md) into a resolved view of a QuakeC module: what each
name means, its type, whether it is a field or a global, a prototype or a body, and where it
is defined and used. It is the engine behind go-to-definition, find-references and the
semantic diagnostics (GOAL-1, LANG-4).

The model is incremental, in the style of rust-analyzer's salsa. A `Db` holds each file's
text as an input and memoizes the derived queries built on it: `parse` and `lower` per file,
and `module_index` per module. A keystroke in one file re-parses and re-lowers only that
file, then re-resolves the module from the mix of fresh and cached per-file results.
`Db::counts` exposes how many times each query actually ran, so an edit's effect on the query
graph is observable (and is what the incremental tests assert). The engine is written here,
with no dependency on the `salsa` crate: it is small, and the project builds its foundations
itself.

`Db::analyze_module` reads a module as the compiler reads it. Each file of the module is put
in the `Db` with its path and its text as written (`Db::set_file`); the module is preprocessed
in compilation order through `qc-preproc` (macros expanded, the untaken side of each
conditional dropped, includes inlined, QC-9), and each file's expansion is parsed as text with
no directive lines (QC-9.19), lowered and resolved. Every token of an expansion keeps its
origin, its own span or the outermost macro use (QC-9.7), so every definition, reference and diagnostic of the result is at a span of a
source file, an included file's own where it came from one (QC-9.2). The per-file parse and
lowering are memoized on the expanded text, so editing a macro's definition re-parses the files
whose expansion it changes and no others. `Db::parse` stays the lossless tree of the file as
written, the editing view.

`Db::set_dialect` chooses the dialect the model parses and resolves in (PRJ-2.8). Under an
FTEQCC dialect the binding forms of `docs/spec/dialect-fteqcc-rerelease.md` resolve: `enum`
names and a `typedef` type name become definitions (FTE-14, FTE-15), `const`/`var`/`nosave`
globals and `for`-loop locals are ordinary definitions (FTE-2, FTE-10 to FTE-12), and the
selector of a member access is a reference to the entity field it names, while a vector member,
`.length` or a struct field names no definition (FTE-9, FTE-13, FTE-14), so the rerelease
resolves with no spurious "unknown value" and a selector that is none of these is reported.
Names in a file-level initialiser (FTE-10.2) and in a parameter's default value (FTE-18.2) are
references too, resolved in the global scope where the declaration is. A named enum's type
name is a definition (FTE-15.4), a C-style declarator has the function type of its own
parameter list (FTE-17.1), and FTEQCC's resolution rules apply (FTE-22): a local or parameter
shadows a global of the same name, a constant may be initialised twice with the same
literal, and with `Db::set_autoproto` (FTEQCC's `-Fautoproto`) every top-level declaration is
visible to every function body.

Resolution follows id's `qcc` as [`docs/spec/language.md`](../../docs/spec/language.md)
records it, not as other QuakeC tools describe it: two scopes, global and the function being
defined (QC-12.1); one definition list in declaration order, a name being the first global or
current-function definition with that name (QC-12.2); a local or parameter cannot shadow an
earlier global, it reuses it (QC-12.3); names resolve at their position in compilation order
(QC-12.5, QC-23.3); a name may be declared repeatedly and initialised once (QC-11.5); a
vector, or a vector field, also declares its `_x`, `_y` and `_z` components (QC-11.7); every
global function must be defined (QC-11.8); fields and functions are global only (QC-11.9); and
a state header's next-think name is declared as a global `void()` if new (QC-13.6).

- **Layer:** L3 semantics. It depends on `qc-syntax`, `qc-lexer` (for `atof` and the token
  kinds), `qc-preproc` and `qc-project` (L2) and `qc-diag` (L0, for the one `Diagnostic`, its
  `Code` and `Span`).
- **Spec sections:** QC-12 (scopes and lookup) in
  [`language.md`](../../docs/spec/language.md), FTE-22 (the rerelease profile's resolution)
  in [`dialect-fteqcc-rerelease.md`](../../docs/spec/dialect-fteqcc-rerelease.md) and FTEF-17
  (the full dialect's name resolution) in
  [`dialect-fteqcc.md`](../../docs/spec/dialect-fteqcc.md).
- **Spec statements:** of [`language.md`](../../docs/spec/language.md), the semantic parts of
  types, QC-10.1, QC-10.2, QC-10.3, QC-10.4, QC-10.6 and QC-10.7; of declarations, QC-11.1,
  QC-11.3, QC-11.5, QC-11.7, QC-11.8, QC-11.9 and QC-11.11 to QC-11.13; the parameter order of
  QC-13.5; the state-header name, QC-13.6; and the spacing of an expansion as the parser reads
  it, QC-9.16. Of
  [`dialect-fteqcc-rerelease.md`](../../docs/spec/dialect-fteqcc-rerelease.md), the builtin
  functions of FTE-23.7 and the cast of FTE-23.16. Of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md), the diagnostics
  QCD-3.5 to QCD-3.8, QCD-3.11, QCD-3.13 to QCD-3.33, QCD-4.1, QCD-4.9 to QCD-4.12, QCD-5.4,
  QCD-7.3, QCD-7.23, QCD-7.28 and QCD-7.32 to QCD-7.51. Of
  [`dialect-fteqcc.md`](../../docs/spec/dialect-fteqcc.md), the classes' members, FTEF-9.1,
  FTEF-9.2, FTEF-9.3, FTEF-9.4 and FTEF-9.5, the accessor
  members FTEF-10.2 and FTEF-10.3, the scoped names of FTEF-16.4, the redeclarations of
  FTEF-17.12, the C scalar types FTEF-2.8 names, and the definitions of FTEF-19.1 to
  FTEF-19.16, FTEF-19.19 and FTEF-19.20. The other statements of those
  sections are the parser's (`qc-syntax`) or the compiler's (L4): the type rules of
  expressions, operator checks and code generation. The definitions are ported from id's
  `qcc/pr_comp.c` and FTEQCC's `engine/qclib/qcc_pr_comp.c` and `qcc_pr_lex.c`, each function
  cited in the headers of `src/resolve.rs` and `src/cast.rs`
  ([`inventory-l3.md`](../../docs/spec/inventory-l3.md)).
- **Shipped:** yes.

Under the FTEQCC profile the full dialect of `docs/spec/dialect-fteqcc.md` resolves by FTEF-17:
a class, struct, union, typedef, enum or accessor names a type, which may be declared again; a
class's members are found through the class and its parents, in a member function's body
before the globals, and after `.` or `->` through the class of what the access is made on;
`Class::member` defines the member its class declares; `this`, `super`, `prior` and FTEQCC's
intrinsics are no names to define; equal initialisers, builtins of one number and engine name
that FTEQCC would bind to one function (FTE-23.7), enums declared again and `noref`
definitions are no redeclaration (FTEF-17.10, FTEF-17.8); a redeclaration
involving a type name that is no typedef's or a pointer is no type mismatch (FTEF-17.12); `wrap`, `weak` and
`accumulate` functions may be defined again, `extern` ones elsewhere, and a `var` function is a
variable (FTEF-15.5); a function with no body is reported unless it is `noref` and not
referred to (FTEF-17.11); and a builtin given for a function with a body is a warning, the body kept
(FTEF-17.9). The names in state headers, sequences, case ranges and `sizeof` are resolved
(FTEF-18), and a `#pragma autoproto` before any definition, in a compiled file or a list-form
`progs.src`, reads the module with autoprototyping (FTEF-12.10), and a later one is the
warning `W0108` (QCD-7.28). FreeCS, Rad-Therapy, Nuclide's
`menu-fn`, Copper and Arcane Dimensions resolve with no error (`cargo xtask fteqcc-profile`).

The definitions themselves follow id's `PR_ParseDefs` and `PR_GetDef` and FTEQCC's
`QCC_PR_ParseDefs` and `QCC_PR_GetDef`, ported function by function as
[`docs/spec/inventory-l3.md`](../../docs/spec/inventory-l3.md) lists (P2-117). Under the vanilla
profile a literal initialiser has the definition's type (QC-11.3), parameters keep their order
in the global area (QC-13.5), a vector's components are not initialised with it (QC-11.11), a
field's components are of a type of their own (QC-11.12) and a local is initialised once
(QC-11.13). Under FTEQCC (FTEF-19, in `src/cast.rs` and `src/resolve.rs`) an initialiser or a
default value is cast to its definition's type as `QCC_TryEvaluateCast` casts it, a name that
initialises a global must be a constant, whose value the redeclaration comparison then reads,
a parameter given twice is an error, a duplicate local or a local hiding a global a warning,
redeclarations follow `typecmp` and `typecmp_lax` with FTEQCC's exceptions, array sizes and
typedefs must agree, enums have their values, `static` globals are their unit's, a type name
must be a type declared before it, a `for` statement's locals leave scope at its end, and the
warnings of those functions are reported with FTEQCC's texts. The definitions family of
`cargo xtask oracles builtins` checks each against id's `qcc` and the pinned FTEQCC (FTEF-19.18).

## What it is not, yet

This crate resolves names and types and answers navigation and the resolution diagnostics. It
does not yet type-check expressions and operators (QC-16 to QC-19), fold constants or lay out
globals and fields (QC-20, QC-21), or emit progs statements (QC-22): those are the compiler's,
built on this model. Rename builds on it in a later issue. Under FTEQCC it does not type
expressions, so a member access whose base is not a name, `this`, a cast or a call resolves by
the member's name, and the values of `enumflags` and other constants are not computed.

## Public shape

- `Db`, `QueryCounts` and `ModuleAnalysis` (`db`): the incremental query database, its
  counters, and a module read through the preprocessor.
- `ModuleInput` and `OriginMap` (`expand`): which files a module preprocesses and compiles,
  and where each span of an expansion came from.
- `Type`, `SimpleType`, `FunctionType` (`ty`): QC-10 types with the structural equality
  `qcc` gives them (QC-10.6).
- `Definition`, `Reference`, `DefId`, `DefKind`, `Location`, `FileId` (`symbol`): the symbols
  a module declares and the uses of them.
- `ModuleIndex`, `FileDiagnostic` and `resolve_module` (`resolve`): a module's resolved
  definition list, references, later declarations of a name (QC-11.5) and diagnostics, each
  diagnostic with its file.
- `ModuleIndex::definition_at`, `references`, `references_at` (`nav`): go-to-definition and
  find-references, a later declaration naming the definition it declares again.
- `FileDecls`, `lower_file` (`lower`): the per-file raw declarations the resolver joins.

## Tests

- The unit tests in `src/tests/` cite the statement each pins (`// spec <ID>`); the property
  tests in `src/tests/properties.rs` hold the two invariants above for token soup and any
  bytes: every definition, reference and diagnostic is at a span of its source file, and
  re-resolving after an edit equals resolving the edited files from scratch.
- `cargo xtask sema-corpus` (the corpus job) resolves id 1.01 under the vanilla profile and
  the rerelease under the FTEQCC profile and compares each index with its golden file in
  [`docs/spec/vectors/sema/`](../../docs/spec/vectors/sema/), and every global definition
  with the progs.dat id's `qcc` and FTEQCC built from the same source (TEST-6).
- The `qc-sema` fuzz target ([`fuzz/`](../../fuzz/README.md)) puts arbitrary bytes, as one to
  three files, through `Db` and `qc_engines::missing_builtins` under both profiles.
