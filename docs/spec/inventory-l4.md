# The L4 path: from a definition to the progs

The functions id's `qcc` runs from a parsed declaration to the bytes of `progs.dat`, as far as
the vanilla compiler (`crates/qc-codegen`, P2-06) models them: the statements each construct
emits, the place of each definition in the global area, literals, function records,
`progdefs.h` and its CRC, and the tables `WriteData` writes. The path before it, from a token to
a definition, is [`inventory-l3.md`](inventory-l3.md), whose rows the compiler builds on: it
takes the definition every name is from the semantic model and does not resolve names again.

The source is id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`, `qcc/pr_comp.c`
and `qcc/qcc.c`, the upstream of [`../porting.md`](../porting.md). The statements cited are in
[`language.md`](language.md) (`QC`) and [`language-diagnostics.md`](language-diagnostics.md)
(`QCD`). FTEQCC's code generation (`QCC_PR_StatementFlags`, the temporaries, the array
emitters, the optimisations) is the FTEQCC subset's, P2-07, and `.lno` and constant folding are
P2-08's.

## How each row is proven

**Differential** names the programs of `cargo xtask oracles codegen`
(`xtask/src/oracles/codegen/cases.rs`, by name, and the 300 programs `program` generates in
`codegen/generate.rs`), each compiled by id's `qcc` and by `qc-codegen`: the two progs.dat must
be the same bytes, or both refused with `qcc`'s message among ours, but for the cases a QC-27
statement names. The outcomes are recorded in `xtask/fixtures/oracles/codegen-outcomes.json`,
so `cargo test -p xtask` checks the compiler against them without the oracles. `check_ours` is
`cargo xtask oracles id101`: id's QuakeC 1.01, 410,616 bytes, the same bytes from both, and the
same trace records from the VM oracle on each. The unit tests citing each statement are in
`crates/qc-codegen/src/tests/`. **Status** is `ported` (modelled and proven) or `not modelled`
(a deliberate divergence, whose statement gives the reason). The run of 2026-10-21 agrees with
`qcc` on every program but the five QC-27.10 to QC-27.13 name.

## 1. `qcc/pr_comp.c`

| Function | On the path | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- |
| `PR_CompileFile` | Copies the file's name into the string table, then compiles each declaration; an error abandons the declaration | `compile::Compiler::compile_file` | QC-25.4, QC-27.17, QC-24.2 | `two-files`, `check_ours` | ported |
| `PR_ParseDefs` | Makes or finds each declared name; stores a constant's literal, or reads a function's body or builtin number and writes its record | `compile::Compiler::declaration`, `function`, `local`; `program::Program::initialise` | QC-11.6, QC-11.10, QC-13.9, QC-21.4, QC-25.2, QC-27.12, QC-27.18 | `parameters-and-locals`, `parameters-no-locals`, `temporaries-before-a-local`, `function-order`, `local-initialiser-reused`, `string-constant`, `check_ours` | ported |
| `PR_GetDef` (its allocating half) | Places a new definition at the next free global, a vector over its components' words, a field's global holding its entity offset; resolution is the semantic model's | `program::Program::get_def`, `component`; `names::Names` | QC-11.7, QC-11.11, QC-11.12, QC-12.3, QC-21.1 to QC-21.3 | `layout`, `vector-initialiser-and-component`, `vector-field-components`, `locals-reuse-globals`, `local-vector`, `check_ours` | ported |
| `PR_ParseImmediateStatements` | A builtin's number, or the parameters, the state header, the statements and `DONE` | `compile::Compiler::function`, `parameters`; `names::Names::def` | QC-11.2, QC-13.1 to QC-13.3, QC-13.5, QC-13.7, QC-13.8, QC-13.10 | `empty-body`, `builtins`, `builtin-rounded`, `bad-parm-order`, `return-unchecked`, `two-functions-one-type`, `check_ours` | ported |
| `PR_ParseState` | `STATE frame, next`, the frame a shared literal, the next function made if new | `compile::Compiler::state` | QC-13.6, QC-12.6 | `state`, `state-declares-next`, `state-with-parameters`, `check_ours` | ported |
| `PR_ParseStatement` | Blocks, `return`, `while`, `do`, `local`, `if`, expressions, and their branches | `compile::Compiler::statement`, `patch_b`, `branch` | QC-14.3 to QC-14.6, QC-14.9, QC-19.2, QC-19.3, QC-22.1, QC-22.2 | `if-else`, `if-if-else`, `while`, `do-while`, `nested-blocks`, `expression-statement`, `vector-condition`, `conditions-of-each-type`, `program` | ported |
| `PR_Expression` | Binary operators by the operator table, field access, assignment and the `ADDRESS` of a field on the left | `compile::Compiler::binary`, `field`, `assign`; `ops::select` | QC-15.9, QC-15.10, QC-16.1, QC-16.2, QC-16.6, QC-16.8, QC-17.2 to QC-17.7, QC-27.10, QC-27.11, QCD-4.2 | `precedence`, `field-chain`, `functions-of-two-types`, `type-mismatch-div`, `type-mismatch-lt`, `type-mismatch-field-eq`, `type-mismatch-and`, `type-mismatch-dot`, `type-mismatch-dot-entity`, `assignment-chain`, `field-stores`, `field-of-another-type`, `field-of-field`, `type-mismatch-store`, `type-mismatch-store-field`, `assign-to-literal`, `earlier-load-rewritten`, `field-of-void`, `program` | ported |
| `pr_opcodes` | The opcodes' spellings and operand and result types, in id's order | `ops::ROWS`, `ops::Row` | QC-16.1, QC-16.5 | `vector-operators`, `equality-of-each-type`, `comparisons`, `program` | ported |
| `PR_Term` | `!` by its operand's type, parentheses, else a value | `compile::Compiler::expr` | QC-15.6, QC-16.4, QC-19.4 | `type-mismatch-not`, `equality-of-each-type`, `program` | ported |
| `PR_ParseValue` | A literal, or the definition a name is | `compile::Compiler::expr`, `def_of` | QC-12.5, QC-15.7 | `program`, `check_ours` | ported |
| `PR_ParseImmediate` | Reuses the first initialised definition of the literal's type and value, or makes an `IMMEDIATE` | `program::Program::immediate`, `holds` | QC-20.1 to QC-20.5 | `literal-reuses-constant`, `zero-and-negative-zero`, `vector-literal-reuse`, `string-literals`, `later-constant-not-reused`, `program` | ported |
| `PR_ParseFunctionCall` | Each argument copied to its slot with `STORE_V`, then `CALLn`; the checks of a fixed list; the return value retyped | `compile::Compiler::call`, `parm_slot`; `ops::call_opcode` | QC-18.2 to QC-18.9, QC-19.1, QCD-4.3 to QCD-4.7 | `calls-share-slots`, `variadic`, `call-through-field`, `no-arguments-to-a-fixed-list`, `return-slot-retyped`, `too-many-parameters`, `too-few-parameters`, `parm-type-mismatch`, `not-a-function`, `more-than-eight`, `field-component-argument`, `program` | ported |
| `PR_Statement` | Emits a statement; a result's temporary, never reused, but for stores and branches | `program::Program::emit`, `emit_result` | QC-21.5, QC-22.3 | `precedence`, `field-stores`, `program` | ported |
| `PrecacheSound`, `PrecacheModel`, `PrecacheFile` | Record the first argument's compile-time string of a precache call, once, in its block | `precache::Precaches::record` | QC-26.1 to QC-26.5 | none: the lists do not reach the progs (QC-26), and `qcc` prints only their lengths; `precache-calls` compares the progs | ported |

## 2. `qcc/qcc.c`

| Function | On the path | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- |
| `InitData`, `PR_BeginCompilation` | The placeholder statement, function and definitions, the empty string, the reserved globals | `program::Program::new` | QC-21.1, QC-25.1 | `empty-body`, `check_ours` | ported |
| `CopyString` | Appends a string to the table, never shared | `program::Program::copy_string` | QC-25.4 | `two-files`, `string-literals`, `check_ours` | ported |
| `PR_WriteProgdefs`, `CRC_ProcessByte` | `progdefs.h`: the globals before `end_sys_globals`, the fields before `end_sys_fields`, and the CRC of the text | `write::progdefs`; `qc_progs::crc::crc_block` | QC-25.5 to QC-25.7, QC-27.13 | `progdefs`, `vector-literal-before-end-sys-globals`, `vector-literal-then-three-definitions`, `check_ours` | ported |
| `WriteData` | The field and global definition tables, the saved-global flag, the names copied again, the padded string table, the header | `write::progs`; `qc_progs::write` | QC-21.6, QC-25.1, QC-25.3, QC-25.4 | `layout`, `check_ours` | ported |
| `main` | Compiles the files in `progs.src` order, then writes `progdefs.h` and the progs with its CRC | `compile_module`, `compile` | QC-23.3, QC-25.6 | `two-files`, `check_ours` | ported |
| `PR_FinishCompilation` | Reports every function never defined | the semantic model's, `qc_sema` `Resolver::check_undefined_functions` ([`inventory-l3.md`](inventory-l3.md)); the compiler stops on it | QC-11.8, QC-24.6 | `check_ours` | ported |

## Functions off this path

`PrintFunction`, `PR_PrintStatement`, `PR_PrintDefs`, `PR_PrintOfs`, `PR_GlobalString`,
`PR_ValueString`, `PR_String`, `PrintStrings`, `PrintFunctions`, `PrintFields` and
`PrintGlobals` print `qcc -asm` and debugging output; `CopyFiles`, `PackFile`, `CopyFile`,
`CreatePath` and `BspModels` are `qcc -copy`, `-pak` and `-bspmodels`, which read the precache
lists. None of them changes the progs. `PR_DefForFieldOfs` serves the printing.
