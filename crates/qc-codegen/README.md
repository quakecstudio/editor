# qc-codegen

The QuakeC compiler's code generator: a module the semantic model resolved, lowered to a
version 6 `progs.dat`, the progs id's `qcc` writes for the same program, byte for byte, but
where [`docs/spec/language.md`](../../docs/spec/language.md) QC-27 says otherwise (GOAL-2,
COMP-1).

`qcc` emits code while it parses. This crate walks each compiled file's syntax tree, the
expansion `qc_sema::Db::analyze_module` read (QC-9.16), in the order `qcc`'s parser meets its
parts, and takes the definition each name is from the semantic model (QC-12). As it goes it
builds what `qcc` builds, in the same order: the definition list and the global area, each
definition placed when first met (QC-21), literals shared by value (QC-20), the statements of
each construct with a new temporary for every result (QC-14 to QC-19, QC-22), the functions in
the order their definitions end (QC-25.2) and the string table (QC-25.4); then `progdefs.h`,
whose CRC the header carries, and the progs tables (QC-25). The type rules of expressions are
the compiler's, as `qcc`'s are: an operator's opcode is the first row of its spelling whose
operand types are the operands' basic types (QC-16.8, QC-17.6), and a call's arguments must
have exactly its parameters' types (QC-18.8). Their errors have `qcc`'s texts (QCD-4.2 to
QCD-4.7). The precache lists `qcc -copy` reads are kept beside the progs (QC-26).

- **Layer:** L4 bridges. It depends on `qc-sema` (L3), `qc-syntax` and `qc-lexer` (L2), and
  `qc-progs`, `qc-lno`, `qc-types` and `qc-diag` (L0).
- **Spec sections:** QC-13 to QC-22, QC-25, QC-26 and the code-generation divergences of QC-27
  (QC-27.11 to QC-27.13, QC-27.17 to QC-27.20) of
  [`language.md`](../../docs/spec/language.md); QCD-4.2 to QCD-4.7 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md); under the FTEQCC
  profile the rerelease subset of
  [`dialect-fteqcc-rerelease.md`](../../docs/spec/dialect-fteqcc-rerelease.md) (FTE-2, FTE-3,
  FTE-5, FTE-8, FTE-9, FTE-13 to FTE-15, FTE-17, FTE-19 and FTE-22) on the vanilla opcodes.
- **Ported from:** id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`,
  `qcc/pr_comp.c` (`PR_CompileFile`, `PR_ParseDefs`, `PR_ParseImmediateStatements`,
  `PR_ParseState`, `PR_ParseStatement`, `PR_Expression`, `PR_Term`, `PR_ParseValue`,
  `PR_ParseFunctionCall`, `PR_ParseImmediate`, `PR_GetDef`'s allocation, `PR_Statement`,
  `pr_opcodes`, `PrecacheSound`, `PrecacheModel`, `PrecacheFile`) and `qcc/qcc.c` (`InitData`,
  `PR_BeginCompilation`, `CopyString`, `PR_WriteProgdefs`, `WriteData`), function by function
  as [`docs/spec/inventory-l4.md`](../../docs/spec/inventory-l4.md) lists; and
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`, `engine/qclib/qcc_pr_comp.c`
  (`QCC_LoadFromArray`, `QCC_PR_ParseRefArrayPointer`, `QCC_PR_EmitArrayGetFunction`,
  `QCC_PR_ArrayRecurseDivideRegular`), behaviour only, under ADR-0017.
- **Shipped:** yes.

## Public shape

- `compile_module(db, input, name_of, options)`: compiles a module of a `qc_sema::Db` under its
  dialect, vanilla or FTEQCC, each file named in the progs as `name_of` says (the name `progs.src` lists it by,
  QC-27.17); a preprocessor, parser, semantic or compiler error stops it, with every
  diagnostic at a span of its source file.
- `compile(units, options)`: the same for files already preprocessed, each a `Unit` of a name
  and a parse.
- `Options`: the optimisations (COMP-5), each off by default, where the output is `qcc`'s:
  `fold`, `--opt fold`, makes an arithmetic operator of two literals the literal of its value
  (QC-27.20).
- `Compiled`: the `qc_progs::ProgsFile`, the text of `progdefs.h`, its CRC, the precache lists,
  the `.lno` line map (`qc_lno::LnoFile`, QC-27.19) and the warnings. `qc_progs::write` writes
  the progs and `LnoFile::write` the line map.
- `CompileError`: the diagnostics, a dialect it does not compile (GMQCC), or an `Internal`
  construct the semantic model should have refused or the compiler does not lower.

## What it is not, yet

Under FTEQCC it lowers what the six rerelease codebases and most of the corpus mods' server
modules use, on the vanilla opcodes (FTE-1.1); FTE's extended opcodes and the rest of FTEQCC
are not lowered: among them a function defined inside another, a read through a pointer, a
compound assignment to an element its setter stores, and casts that convert between basic
types.
`cargo xtask codegen-corpus <dir>...` compiles codebases under the FTEQCC profile.

## Tests

- The unit tests in `src/tests/` cite the statement each pins (`// spec <ID>`); their expected
  statements were read from id's `qcc` built by `cargo xtask oracles build` (`qcc -asm`). The
  property tests in `src/tests/api.rs` put token soup through the compiler: every program the
  semantic model accepts is lowered.
- `cargo xtask oracles id101 <dir>` compiles id's QuakeC 1.01 with this crate: the progs.dat is
  id's `qcc`'s, byte for byte (410,616 bytes), and the scenarios the VM oracle runs on it leave
  the records `qcc`'s does.
- `cargo xtask compiler-diff` (P2-09, `docs/spec/differential.md` DIFF-12) compiles id 1.01
  with id's `qcc` and with this crate, `--opt fold` off and on, and calls every function of
  each progs in `qc-vm` from the differential scenarios' worlds: no divergence, and at least
  90% of `qcc`'s statements executed. `--full` also compiles the rerelease with the pinned
  FTEQCC and with this crate in the FTEQCC dialect, held to the same bar (DIFF-12.8, DIFF-12.11);
  the mission packs and CTF are compared when named, until they pass (P2-150).
- `cargo xtask oracles codegen` compiles the hand-written programs of
  `xtask/src/oracles/codegen/cases.rs` and 300 generated ones with both compilers: the two
  progs must be the same bytes, or both refused with `qcc`'s message among ours, but for the
  five that QC-27 says diverge. The outcomes are recorded in
  `xtask/fixtures/oracles/codegen-outcomes.json`, so `cargo test -p xtask` checks this crate
  against them on every host.
