# The L3 path: from a token to a definition

The functions id's `qcc` and FTEQCC run from a token the parser holds to a definition and a
diagnostic, as far as the semantic model (`crates/qc-sema`) models them: declarations, types,
resolution, initialisers and the diagnostics the profiles report (P2-117). Code generation
(P2-06 onward) is not on this path; the functions of the two files that only generate code are
listed at the end, with the issue that owns them. The path before the parser, from `progs.src`
to a token, is [`inventory-l2.md`](inventory-l2.md).

The sources are id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73` (`qcc/`) and
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e` (`engine/qclib/`), the upstreams of
[`../porting.md`](../porting.md). The statements cited are in [`language.md`](language.md)
(`QC`), [`language-diagnostics.md`](language-diagnostics.md) (`QCD`),
[`dialect-fteqcc-rerelease.md`](dialect-fteqcc-rerelease.md) (`FTE`) and
[`dialect-fteqcc.md`](dialect-fteqcc.md) (`FTEF`).

## Lineage

FTEQCC's compiler descends from id's `qcc` ([`inventory-l2.md`](inventory-l2.md) records the
lexer's and the command library's evidence), and the definitions half of `qcc_pr_comp.c` grew
from id's `pr_comp.c`. fteqw's first commit of `engine/qclib` (7c00f2b1, 2004-08-23) has, in
`qcc_pr_comp.c`:

- `void QCC_PrecacheSound (QCC_def_t *e, int ch)`, id's `PrecacheSound` with checks added, still
  carrying id's error as a comment: `//		QCC_Error ("PrecacheSound: numsounds == MAX_SOUNDS");`;
  and `QCC_PrecacheFile` and `QCC_PR_ParseImmediate` of the same descent;
- `QCC_PR_GetDef (QCC_type_t *type, char *name, QCC_def_t *scope, pbool allocate, int
  arraysize)`, id's `PR_GetDef` with a hash table and an array size, whose scope test is id's line
  for line: `if ( def->scope && def->scope != scope) { ... continue;		// in a different
  function }`, as `PR_GetDef` has `if ( def->scope && def->scope != scope) continue;		// in a
  different function`. The pinned commit keeps the comment in both branches of `QCC_PR_GetDef`;
- `QCC_PR_ParseDefs`, `QCC_PR_ParseState` and `QCC_PR_ParseImmediateStatements`, id's functions
  under FTEQCC's prefix.

So each row of the first table pairs id's function with the FTEQCC function it became, and its
content is what FTEQCC's body does differently, which is what the FTEQCC profile adds on top of
the port of id's. GMQCC was written independently ([`inventory-l2.md`](inventory-l2.md)) and is
not on this path; it is the third dialect tier, later.

## How each row is proven

The differential cases are the programs of `cargo xtask oracles builtins` (FTE-23.15,
FTEF-19.18), compiled by id's `qcc` and the pinned FTEQCC and read by the profiles, the outcome
of each recorded in `xtask/fixtures/oracles/builtin-outcomes.json`. The definitions family is
`xtask/src/oracles/builtins/declarations.rs`: `VANILLA` (id's `qcc`), `FTEQCC` and `WARNINGS`
(FTEQCC with `-Werror -Wno-Q208`, so a warning either compiler gives alone changes the outcome).
The builtin cross product of `builtins.rs` and the directive family of
`builtins/directives.rs` prove the builtin initialisers and the macros that write them. The unit
tests citing each statement are in `crates/qc-sema/src/tests/` (`definitions.rs` for this
issue's ports) and `crates/qc-syntax/src/tests/`. The run of 2026-10-21 agrees with both oracles
on every case but those the statements QC-27.8, QC-27.9, FTEF-13.8 and FTEF-14.47 name.

## 1. id's functions and their FTEQCC counterparts

| id (`qcc/`) | FTEQCC (`engine/qclib/`) | What FTEQCC does differently | Rust counterpart | Statements | Differential cases |
| --- | --- | --- | --- | --- | --- |
| `pr_comp.c`, `PR_ParseDefs` | `qcc_pr_comp.c`, `QCC_PR_ParseDefs` | The qualifiers before the type and their warnings; C-style declarators, pointers, arrays, `Class::name`, typedefs; a function initialised with a value; the constness of a definition fixed by its first declaration; `shared`, `extern`, `wrap`, `weak`, `accumulate`; "Missing semicolon at end of definition"; `typedef;` a warning; a body for an initialised function "redeclaration of function body"; an `int` when a qualifier is followed by no type (`hadmodifier`) | `qc_sema::lower::lower_declaration`, `omitted_type`, `resolve::Resolver::declare_top_level`, `check_initialisation`, `warn_qualifiers`; `qc_syntax` `Parser::declaration`, `fte_type_omitted`, `typedef_decl` | QC-11.1 to QC-11.13, FTE-10 to FTE-12, FTE-17, FTE-21.3, FTEF-15, FTEF-19.3, FTEF-19.4, FTEF-19.9, FTEF-19.11, FTEF-19.15, FTEF-19.16, FTEF-19.21, QCD-3.6, QCD-3.16, QCD-3.21 to QCD-3.25, QCD-3.30, QCD-7.18, QCD-7.37, QCD-7.39 to QCD-7.43 | `VANILLA` (QC-11), `FTEQCC` and `WARNINGS` (FTEF-19.3, 19.4, 19.9, 19.11, 19.15, 19.16, 19.21) |
| `pr_comp.c`, `PR_GetDef` | `qcc_pr_comp.c`, `QCC_PR_GetDef`, `QCC_PR_GetSRef`, `QCC_PR_DummyDef` | Locals shadow globals; a duplicate local a warning, a duplicate parameter an error, a local hiding a global of its kind a warning; type comparison by `typecmp` with default values, `typecmp_lax` optional arguments, the misdeclared-builtin, `*extensions.qc`, `noref` replacement and c89 exceptions; array sizes; `static` units; `autocvar_` promotion; vector components made with no lookup, of the vector's words; keyword names a warning; deprecation warnings at a use | `resolve::Resolver::declare_global_with`, `check_redeclared_type`, `check_array`, `declare_scoped_with`, `lookup_global`, `push_def` | QC-11.5, QC-11.7, QC-11.11, QC-11.12, QC-12, FTE-22.1, FTEF-17.12, FTEF-19.1, FTEF-19.5, FTEF-19.7, FTEF-19.8, FTEF-19.12, FTEF-19.13, FTEF-19.16, QCD-3.5, QCD-3.15, QCD-3.20, QCD-3.31, QCD-7.3, QCD-7.32 to QCD-7.36, QCD-7.38, QCD-7.44, QCD-7.47 | `VANILLA` (components, locals), `FTEQCC` (FTEF-19.1, 19.7, 19.8, 19.12), `WARNINGS` (FTEF-19.1, 19.7, 19.13, 19.16) |
| `pr_comp.c`, `PR_ParseImmediateStatements` | `qcc_pr_comp.c`, `QCC_PR_ParseImmediateStatements`, `QCC_PR_GenerateBuiltinFunction`, `QCC_PR_GenerateQCFunction` | `:` as well as `#`, integer builtin numbers, engine names; parameters made with `allocate` 2, so a duplicate is an error, and no "bad parm order", parameters being locals; unnamed parameters a warning; `wrap` and `prior`; `accumulate` | `qc_syntax::builtin_binum`, `lower::LiteralValue::of_fteqcc`, `resolve::Resolver::resolve_body`, `below` | QC-13.1 to QC-13.5, QC-13.10, FTE-17.4 to FTE-17.6, FTE-23, FTEF-17.9, FTEF-19.1, FTEF-19.4, QCD-3.9, QCD-5.4, QCD-7.23, QCD-7.48 | the builtin cross product; `VANILLA` (QC-13.5); `FTEQCC` and `WARNINGS` (FTEF-19.1, 19.4) |
| `pr_comp.c`, `PR_ParseState` | `qcc_pr_comp.c`, `QCC_PR_ParseState` | Frame cycles `[++ a .. b]`; the frame and next function any expressions, the frame read before the next function's name is declared, a missing `,` a warning, a frame of a string or vector and a next function of a literal errors | `lower::lower_body` (`state_uses`, `state_literals`), `resolve::Resolver::declare_next_think`, `check_state_literals`; `qc_syntax` `Parser::state` | QC-13.6, FTEF-18.3, FTEF-18.28, FTEF-19.14, QCD-3.10, QCD-3.12, QCD-3.27, QCD-4.9, QCD-7.21 | `VANILLA` (QC-13.6), `FTEQCC` (FTEF-19.14) |
| `pr_comp.c`, `PR_ParseValue` | `qcc_pr_comp.c`, `QCC_PR_ParseRefValue` | `::`, `Class::member`, `super::`; members of the class in a member function; intrinsics declared on demand when no definition has the name, `random` among them; `this` and `super` errors outside a member function; a field's name in a member function is `this.field`; an unknown name a non-fatal error | `resolve::Resolver::resolve_value`, `resolve_scoped`, `resolve_member`, `check_named_value` | QC-12.5, FTEF-16.4, FTEF-17.2 to FTEF-17.4, FTEF-19.2, FTEF-19.16, QCD-4.1, QCD-4.10 to QCD-4.12 | `FTEQCC` (FTEF-19.16) and the corpus resolution job (`cargo xtask fteqcc-profile`) |
| `pr_comp.c`, `PR_ParseImmediate` | `qcc_pr_comp.c`, `QCC_PR_ParseImmediate`; `qcc_pr_lex.c`, `QCC_PR_LexNumber` | The literal's type: integer, unsigned, 64-bit and double literals by suffix, a character constant a float; constants shared by value. The type and value of a literal initialiser are on this path, the sharing of constants is code generation | `cast::Literal::of_fteqcc_number`, `lower::raw_value`, `LiteralValue::of_literal` | QC-4, QC-5, QC-6, FTEF-3.1, FTEF-3.3, FTEF-19.2, FTEF-19.5 | `FTEQCC` (FTEF-19.2, 19.5) |
| `pr_comp.c`, `PR_CompileFile` | `qcc_pr_comp.c`, `QCC_PR_CompileFile` | A `;` before a definition skipped; an error in the first token an ordinary error (FTEQCC sets its recovery point first) | `qc_syntax` `Parser::source_file`; `qc_sema::Db::analyze_module` | QC-24.2, QC-24.7, QC-27.1, FTE-17.3 | the directive family (a file's first token) |
| `pr_lex.c`, `PR_ParseError` | `qcc_pr_lex.c`, `QCC_PR_ParseError`, `QCC_PR_ParseWarning`, `QCC_PR_PrintWarning` | Warnings, each with its id; a warning of an error id (`ERR_*`) is a non-fatal error; `#pragma warning` and `-W` change a warning's action | `qc_diag::Code` (`E` and `W` codes), the resolver's `report` | QC-24.1, QCD-1, FTEF-18 | `WARNINGS` (`-Werror`) |
| `pr_lex.c`, `PR_Expect` | `qcc_pr_lex.c`, `QCC_PR_Expect` | The same | `qc_syntax` `Parser::expect` | QCD-3.4 | every family |
| `pr_lex.c`, `PR_Check` | `qcc_pr_lex.c`, `QCC_PR_CheckToken`, `QCC_PR_CheckName`, `QCC_PR_CheckKeyword` | Keywords enabled or disabled by default, `__word` matched in any case | `qc_syntax` `fte::is_decl_qualifier`, `starts_local_statement`, `c_scalar::CWord::of` | FTEF-2.7, FTEF-15.1, FTEF-19.1 | `FTEQCC` (FTEF-2, FTEF-19.1) |
| `pr_lex.c`, `PR_ParseName` | `qcc_pr_lex.c`, `QCC_PR_ParseName` | 255 bytes, not 63; "unexpected EOF" | `qc_syntax` `Parser::name` | QC-3.3, FTEF-15.7, QCD-3.1, QCD-3.2 | the builtin cross product (`NAME_LENGTHS`) |
| `pr_lex.c`, `PR_FindType` | `qcc_pr_lex.c`, `QCC_PR_FindType`, `typecmp_strict`, `typecmp`, `typecmp_lax` | `typecmp` compares default values where both have one; `typecmp_lax` takes optional arguments; id's `type_floatfield` is no interned type | `ty::Type` (`PartialEq`, `compatible`), `resolve::defaults_agree`, `Resolver::lax_same`, `Facts::field_component` | QC-10.6, QC-11.12, FTEF-17.12, FTEF-19.6, FTEF-19.7 | `VANILLA` (QC-11.12), `FTEQCC` (FTEF-19.6, 19.7) |
| `pr_lex.c`, `PR_ParseType` | `qcc_pr_lex.c`, `QCC_PR_ParseType`, `QCC_PR_ParseFunctionType`, `QCC_TypeForName` | The loop over C type words, its narrowing warnings, `const` and `volatile`; registered type names; pointers, structs, classes, accessors, enums, typedefs resolved to their types; a name that is no type; parameters with `optional`, `__out`, defaults, no name; more than 8 parameters | `qc_syntax::c_scalar::CWords::read`, `ty::Type::from_ast`, `Type::fteqcc_scalar`, `lower::param_infos`, `Resolver::resolved`, `check_type_names`; `qc_syntax` `fte_type`, `fte_param_list` | QC-10, FTEF-2.1, FTEF-2.7 to FTEF-2.11, FTEF-4 to FTEF-10, FTEF-15.2 to FTEF-15.4, FTEF-19.6, FTEF-19.9, FTEF-19.20, QCD-7.52 | `FTEQCC`, `WARNINGS` (FTEF-2, FTEF-19.6, 19.9, 19.20) |
| `pr_lex.c`, `PR_SkipToSemicolon` | `qcc_pr_lex.c`, `QCC_PR_SkipToSemicolon` | Leaves a macro's expansion first | the parser's recovery (`qc_syntax::recovery`), of its own | QC-24.2, QCD-1.4 | not compared: recovery is this project's (QCD-1.4) |
| `qcc.c`, `PR_FinishCompilation` | `qccmain.c`, `QCC_PR_FinishCompilation` | `noref` functions with no body and no reference skipped; `spawnfunc_` of a class written; `extern` refused on the id target; no check once an error was reported (the profile still reports a function with no body, QCD-1.4) | `resolve::Resolver::check_undefined_functions` | QC-11.8, QC-24.6, FTEF-9.5, FTEF-17.11, FTEF-19.17, QCD-3.11 | the macro-text family (`DECLARATION_SOURCES`), `FTEQCC` |

## 2. FTEQCC's functions on the path that id's `qcc` has no counterpart of

| FTEQCC function | What it does on this path | Rust counterpart | Statements | Differential cases |
| --- | --- | --- | --- | --- |
| `qcc_pr_comp.c`, `QCC_PR_ParseInitializerType_Internal`, `QCC_PR_ParseInitializerDef` | Reads an initialiser: a builtin, a body, a value cast to the type, `_()`, an aggregate | `lower::lower_declarators`, `resolve::Resolver::check_value` | FTE-10.2, FTE-23, FTEF-15.2, FTEF-19.2, FTE-23.16, QCD-3.13, QCD-3.32 | `FTEQCC` (FTEF-19.2) and the builtin cross product |
| `qcc_pr_comp.c`, `QCC_PR_GenerateInitializerType` | The constness check of a name, the "dupe initialisation" comparison and its hacks (`parseentitydata`, a builtin after a body) | `resolve::Resolver::check_named_value`, `redeclare`, `same_value`, `parseentitydata` | FTE-22.5, FTEF-17.8 to FTEF-17.10, FTEF-19.3, FTEF-19.5, QCD-3.14, QCD-7.45, QCD-7.49 | `FTEQCC` and `WARNINGS` (FTEF-19.3, 19.5) |
| `qcc_pr_comp.c`, `QCC_TryEvaluateCast`, `QCC_EvaluateCast`, `QCC_SRef_IsNull` | The implicit cast of a value to a type, null first | `cast::Source::refused`, `Source::warns`, `Resolver::cast_value` | FTEF-19.2, QCD-3.13, QCD-7.46 | `FTEQCC`, `WARNINGS` (FTEF-19.2) |
| `qcc_pr_comp.c`, `QCC_PR_ParseDefaultInitialiser` | A default value: a constant with a value, cast | `lower::param_infos`, `resolve::Resolver::check_defaults` | FTE-18.2, FTEF-19.6, QCD-3.17, QCD-3.18 | `FTEQCC` (FTEF-19.6) |
| `qcc_pr_comp.c`, `QCC_PR_ParseEnum` | Enum values, `enumflags`, base types, `enum class`, their errors and warnings | `lower::lower_enum`, `resolve::Resolver::declare_enum_item`, `enum_value`, `warn_enumflag` | FTE-15, FTEF-8, FTEF-17.10, FTEF-19.10, QCD-3.27 to QCD-3.29, QCD-7.50, QCD-7.51 | `FTEQCC`, `WARNINGS` (FTEF-19.10) |
| `qcc_pr_comp.c`, `QCC_PR_ParseDefArray` | An array's dimensions, an unsized one sized by its initialiser | `lower::array_dim` | FTE-13, FTEF-5, FTEF-19.8 | `FTEQCC` (FTEF-19.8) |
| `qcc_pr_comp.c`, `QCC_PR_IntConstExpr` | An array size that is an expression | not modelled: FTEF-19.17 (an expression's value is the compiler's) | FTEF-19.17 | none: not compared |
| `qcc_pr_comp.c`, `QCC_PR_DummyFieldDef`, `QCC_PR_ExpandUnionToFields` | The fields of a struct or union field | not modelled: FTEF-19.17 (the members' layout is the compiler's) | FTEF-6, FTEF-19.17 | none: not compared |
| `qcc_pr_lex.c`, `QCC_PR_ParseEntClass`, `QCC_PR_ParseStruct`, `QCC_PR_ParseAccessorMember`, `QCC_PR_MakeThiscall` | Classes, structs and accessors: their names and members | `lower::lower_class`, `lower_accessor`, `resolve::Resolver::declare_class`, `define_member`, `member_in`; `qc_syntax` `class_decl`, `struct_decl`, `accessor_decl` | FTEF-6, FTEF-9, FTEF-10, FTEF-17.1 to FTEF-17.3, FTEF-19.15, QCD-3.25, QCD-3.26, QCD-7.41 | `FTEQCC` (FTEF-19.15) and the FTEQCC corpus resolution job |
| `qcc_pr_lex.c`, `TypeName` | A type in a message | `Type::fteqcc_name` (`cast.rs`) | FTEF-19.2 | the messages are compared by the unit tests, the outcomes by `FTEQCC` |
| `qcc_pr_comp.c`, `QCC_PR_ParseStatement` (its definition branch) | Which statements define locals, the C type words among them; `typedef` in a body | `qc_syntax` `Parser::statement_inner`, `fte_looks_like_local`, `c_word_starts_local` | FTEF-2.10, FTEF-16.1, FTEF-19.1, FTEF-19.9 | `FTEQCC` (FTEF-2, FTEF-19.1) |
| `qcc_pr_comp.c`, `QCC_PR_ParseStatement_For` (its `subscopestart` and `localstable` removal) | The locals a `for` statement defines leave scope at its end | `lower::lower_stmt`, `RawLocal::scope_end`, `resolve::Resolver::lookup_local` | FTEF-19.19 | `FTEQCC`, `WARNINGS` (FTEF-19.19) and the FTEQCC corpus resolution job |
| `qcc_pr_comp.c`, `QCC_PR_ParseInitializerType_Internal` (a string for an 8-bit array) | An 8-bit array initialised by a string, "initializer-string too long" | `resolve::Resolver::check_array_value`, `RawValue::string_len` | FTEF-19.8, QCD-3.33 | `FTEQCC` (FTEF-19.8) |
| `qcc_pr_comp.c`, `QCC_PR_ParseFunctionTypeReacc`, the `flag_acc` branches of `QCC_PR_ParseDefs` | Reacc syntax | not modelled: FTEF-19.17 (`-Tid` does not enable it) | FTEF-19.17 | none: not compared |
| `qcc_pr_comp.c`, `QCC_PR_ParseAsm` | `asm` blocks | not modelled: code generation (P2-07) | FTEF-19.17 | none: not compared |
| `qcc_pr_comp.c`, `QCC_PR_EmitClassFromFunction` | The body of a class's `spawnfunc_` | the definition only, `resolve::Resolver::declare_spawnfunc`; the body is code generation (P2-07) | FTEF-9.5 | the FTEQCC corpus resolution job |

## 3. id's functions FTEQCC dropped or replaced

| id function or behaviour | What FTEQCC does instead | Rust counterpart | Statements |
| --- | --- | --- | --- |
| `PR_ParseImmediateStatements`, `Error ("bad parm order")` | No such check: parameters are locals of the function, never a global reused | `resolve::Resolver::resolve_body` (vanilla only) | QC-13.5, QCD-5.4, FTEF-19.1 |
| `PR_GetDef`, the reuse of a global by a local or parameter | A local shadows the global | `resolve::Resolver::declare_scoped_with` | QC-12.3, FTE-22.1 |
| `PR_GetDef`, `type_floatfield` for a field's components | One type for every `.float`, compared by `typecmp` | `Facts::field_component` (vanilla only) | QC-11.12 |
| `PR_ParseDefs`, "wrong immediate type for NAME" | The cast of `QCC_EvaluateCast` | `resolve::Resolver::check_value` | QC-11.3, QCD-3.7, FTEF-19.2 |
| `PR_ParseDefs`, the type of the last literal lexed checked for any initialiser | The initialiser is parsed and cast; the vanilla profile refuses a non-literal (QC-27.8) | `qc_syntax` (`E0506`) | QC-11.4, QC-27.8 |
| `PR_ParseImmediate`, "weird immediate type" | Every constant type is shared | none: no input reaches it | QC-20 |
| `PR_FindType`, the `COMPLEX TYPE` definition it makes | `QCC_PR_FindType` makes none | none: no name a program sees | QC-10.6 |
| `PR_CompileFile`, the recovery point set after the first token | Set before it | `qc_syntax` (QC-27.1) | QC-24.7, QC-27.1 |

## Functions of the two files off this path

`pr_comp.c`'s `PR_Statement`, `PR_ParseFunctionCall`, `PR_Term`, `PR_Expression` and
`PR_ParseStatement` (beyond the names they use and the `local` definitions they start), and
`PrecacheSound`, `PrecacheModel` and `PrecacheFile`; and their FTEQCC counterparts
`QCC_PR_StatementFlags`, `QCC_PR_ParseFunctionCall`, `QCC_PR_GenerateFunctionCallRef`,
`QCC_PR_RefTerm`, `QCC_PR_RefExpression`, `QCC_PR_ParseStatement`, the `QCC_Precache*`
functions, the temporaries, the array emitters and the optimisations: these type expressions,
check calls and write statements, the compiler's work (QC-14 to QC-22, QC-26; P2-06, P2-07,
P2-08). The names they use are on this path, as the uses the lowering records and the resolver
resolves (QC-12.5).
