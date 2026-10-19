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
conditional dropped, includes inlined, QC-9), and each file's expansion is parsed, lowered and
resolved. Every token of an expansion keeps its origin, its own span or the outermost macro use
(QC-9.7), so every definition, reference and diagnostic of the result is at a span of a
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
- **Spec sections:** the semantic parts of QC-10 (types), QC-11 (declarations), QC-12 (scopes
  and lookup) and QC-13.6 (state-header names) in
  [`language.md`](../../docs/spec/language.md), and the diagnostics QCD-3.5, QCD-3.6, QCD-3.8,
  QCD-3.11, QCD-4.1 and QCD-7.3 in
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md). These sections are
  completed here from the syntactic parts that `qc-syntax` owns; the type rules, operator
  checks and code generation of QC-16 to QC-22 belong to the compiler (L4), so those sections
  are not declared above. Written from the spec, not ported.
- **Shipped:** yes.

Under the FTEQCC profile the full dialect of `docs/spec/dialect-fteqcc.md` resolves by FTEF-17:
a class, struct, union, typedef, enum or accessor names a type, which may be declared again; a
class's members are found through the class and its parents, in a member function's body
before the globals, and after `.` or `->` through the class of what the access is made on;
`Class::member` defines the member its class declares; `this`, `super`, `prior` and FTEQCC's
intrinsics are no names to define; equal initialisers, builtins of one number, enums declared
again and `noref` definitions are no redeclaration (FTEF-17.5, FTEF-17.8); a redeclaration
involving a type name or a pointer is no type mismatch (FTEF-17.6); `wrap`, `weak` and
`accumulate` functions may be defined again, `extern` ones elsewhere, and a `var` function is a
variable (FTEF-15.5); a function with no body is reported only where it is referred to
(FTEF-17.7); and a builtin given for a function with a body is a warning, the body kept
(FTEF-17.9). The names in state headers, sequences, case ranges and `sizeof` are resolved
(FTEF-18), and a `#pragma autoproto` before any definition, in a compiled file or a list-form
`progs.src`, reads the module with autoprototyping (FTEF-12.10), and a later one is the
warning `W0108` (QCD-7.28). FreeCS, Rad-Therapy, Nuclide's
`menu-fn`, Copper and Arcane Dimensions resolve with no error (`cargo xtask fteqcc-profile`).

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
  definition list, references and diagnostics, each diagnostic with its file.
- `ModuleIndex::definition_at`, `references`, `references_at` (`nav`): go-to-definition and
  find-references.
- `FileDecls`, `lower_file` (`lower`): the per-file raw declarations the resolver joins.
