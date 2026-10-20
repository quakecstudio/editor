# Component designs

One design document per component of the codebase. They sit between the
[design document](../design-doc.md), which says what QuakeC Studio is and why, and the
[engine behaviour specs](../../spec/README.md), which say what id's code does statement by
statement. A component design says how one part of this repository is built: its pieces, its
error model, how it is verified, the decisions behind its shape and the questions still open.

They were mapped from the code at commit `c706ef0` and are kept current from then on: a pull
request that changes a component's structure, contracts, error model or invariants updates its
design document in the same pull request (`.claude/skills/work-issue/SKILL.md`).

## How to read one

Every document has the same sections, in this order:

| Section | Holds |
| --- | --- |
| Context and design philosophy | What the component is for and the rules that shape it |
| Components | Its parts, one subsection each |
| Error model | What fails, how, and what is reported instead of failing |
| Verification | The tests, corpus checks, oracles and fuzz targets that hold it |
| Decisions and alternatives | Each design choice, what else was possible and why this one |
| Open questions | Resolved and deferred questions, with `file:line` and spec IDs |
| References | Specs, ADRs, readiness reports and plan issues |

Behaviour is not restated: a document cites the statements in [`docs/spec/`](../../spec/README.md)
it rests on, and `docs/spec/` stays the only specification. A decision marked `[inferred]` was
read from the code because no document records it; confirm it before relying on it.

## Index

| Group | Component | Crates and paths | Spec prefixes |
| --- | --- | --- | --- |
| language | [Diagnostics](language/diagnostics.md) | `qc-diag` | QCD |
| language | [Lexing](language/lexing.md) | `qc-lexer` | QC, QCD, FTEF, FTE |
| language | [Preprocessing and the project graph](language/preprocessing.md) | `qc-preproc`, `qc-project` | QC, PRJ, FTEF |
| language | [Syntax](language/syntax.md) | `qc-syntax` | QC, QCD, FTE, FTEF |
| language | [Semantics](language/semantics.md) | `qc-sema` | QC, QCD, FTE, FTEF |
| language | [Engine targets](language/engine-targets.md) | `qc-engines`, `xtask/src/engines.rs` | ENGT, QCD |
| execution | [progs.dat and .lno formats](execution/progs-format.md) | `qc-types`, `qc-progs`, `qc-lno` | PROGS, LNO, LIMIT |
| execution | [VM and edicts](execution/vm.md) | `qc-vm`, `qc-edict` | VM, EDICT, LIMIT |
| execution | [Builtins](execution/builtins.md) | `qc-builtins` | BUILTIN, DIFF |
| execution | [Trace format](execution/trace.md) | `qc-trace` | TRACE |
| game-data | [Archives and assets](game-data/archives-assets.md) | `qs-pak`, `qs-wad`, `qs-wav`, `qs-mdl` | PAK, WAD, WAV, MDL, SPR, MD5 |
| game-data | [Game filesystem](game-data/game-filesystem.md) | `qs-vfs`, `qs-install`, `qs-paths` | FS |
| game-data | [Maps](game-data/maps.md) | `qs-bsp`, `qs-map` | BSP, MAP |
| fidelity | [Oracles and the differential harness](fidelity/oracles-differential.md) | `oracles/`, `qs-differential`, `xtask/src/oracles/` | DIFF, TRACE, EDICT |
| fidelity | [Corpora, corpus checks, fuzzing and the cross-crate tests](fidelity/corpora-fuzzing.md) | `corpus/`, `fuzz/`, `qs-tests`, `xtask/src/corpus/` | none of its own |
| fidelity | [Readiness measurement and ratchets](fidelity/readiness-ratchets.md) | `xtask/src/readiness.rs`, `xtask/src/ratchet.rs`, `ratchets.toml` | none of its own |
| process | [Repository checks and CI](process/repo-checks-ci.md) | `xtask`, `qs-log`, `qs-bench`, `.github/workflows/` | none of its own |
| process | [Plan: requirements, backlog and GitHub sync](process/plan.md) | `plan/` | none of its own |
| process | [Agent harness: rules, skills, subagents and hooks](process/agents.md) | `.claude/`, `.kiro/` | none of its own |

The fidelity and process components implement requirements rather than spec statements; each
document names them (TEST, QA, CONV and PM in `plan/requirements.yaml`).
