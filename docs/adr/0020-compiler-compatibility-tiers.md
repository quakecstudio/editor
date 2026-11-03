# ADR-0020: Hold the compiler to the oracles in compatibility tiers and generate code through an intermediate layer

- **Status:** Accepted
- **Date:** 2026-11-03
- **Decided by:** Justin Plock, 2026-11-03, choosing option B: "option 2 sounds like teh best option"
- **Issue:** [#649](https://github.com/quakecstudio/editor/issues/649)
- **Requirements:** COMP-3, COMP-4, COMP-5, COMP-6, COMP-8

## Question

When qc-codegen and FTEQCC disagree on a program, which differences must the compiler remove,
and what code-generation design serves that and the requirements still to come?

## Context

The generated-program differential of #649 (DIFF-13) compiles 2000 programs from seed 1 with
the pinned FTEQCC (fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e, `-Tid`) and with
qc-codegen, and runs both. On main at 54cb061b, 3 programs behave differently: 555, 1108 and
1230.

Program 555 is understood. FTEQCC compiles `arr[i] |= self.ff ? self.fv.x : (gf1 || gf1);` with
its `?:` result in a one-word dummy local named `ternary` (`QCC_PR_DummyDef(..., "ternary",
pr_scope, ...)` in `QCC_PR_RefExpression`, `qcc_pr_comp.c`). It loads the first arm with
`LOAD_V`, which writes three words. The two words after the local are the temporaries holding
the array index, which FTEQCC has made locals because they are live across the `ArrayGet` call
(`QCC_LockActiveTemps`, `QCC_RemapLockedTemps`). The store then indexes with a clobbered
value: FTEQCC's progs computes a different element, or fails the bounds check. This is a
defect of FTEQCC, not a rule of the language. The causes of 1108 and 1230 are not yet known.

Matching 555 exactly needs FTEQCC's temporary allocator. `QCC_GetTemp` reuses the first
temporary no reference holds. The reference counts change at the `QCC_FreeTemp`,
`QCC_UnFreeTemp` and `QCC_ForceUnFreeDef` calls throughout the compiler, 494 of
`QCC_FreeTemp` alone in `qcc_pr_comp.c`. So which temporary a value gets follows from where
FTEQCC's code frees values, not from what the program means. Porting that is porting
FTEQCC's structure, which ADR-0017's clean-port policy rules out ("port FTEQCC's behaviour,
not its structure").

The design document ("Compiler") defines correctness as the same behaviour, says byte-identical
files are not a goal (COMP-4), and takes the extended dialects' conformance from compiling the
rerelease and public mods and checking runtime behaviour (COMP-8). Optimizations behind flags
(COMP-5, L-04), incremental recompile under 200 ms (COMP-6, P2-11) and the GMQCC dialect
(COMP-3) all need the compiler to choose its own temporaries and layout.

The front end already follows rust-analyzer's design (`docs/design/semantic-model.md`): a
lossless tree, lowering, resolution, checking into a `TypedBody` per function, and a query
database. The back end does not. `qc-codegen` walks each file's syntax tree and emits
statements as it goes, in the order `qcc`'s parser meets them; it reads none of the typed
bodies and works out types again itself. FTEQCC's behaviour is spread through it as 52 dialect
branches in `compile.rs`.

## Options

| Option | Consequences |
| --- | --- |
| A. Port FTEQCC's temporary allocator into qc-codegen | Exact matches on generated programs, 555 included. More than a week of work in the 4,200 lines of `compile.rs`, each construct freeing where FTEQCC frees. It ties the code generator to FTEQCC's internals, ships FTEQCC's miscompiles, and every new seed can find another internal to copy. |
| B. Compatibility tiers, and code generation through an intermediate layer | Defined behaviour and the quirks real mods rely on match the oracles; an oracle's miscompile is recorded only with a mechanical proof. A new intermediate layer gives the back end explicit values and lifetimes, and its passes allocate temporaries and lay out memory per dialect. #649 records 555 as an FTEQCC defect once the harness proves it; a new issue builds the layer. |
| C. Compatibility tiers only, keeping direct emission | The cheapest now: #649 changes as in B and nothing else does. The optimization, incremental-recompile and GMQCC work later meets the same direct-emission design. |

Option A is not chosen because it ports structure and copies defects. Option C is not chosen
because it leaves the back end without a place for the passes the later requirements need.

## Decision

The compiler is held to the oracles in three tiers. Tier 1: the language's defined behaviour
matches id's `qcc` and the pinned FTEQCC exactly. Tier 2: the oracles' quirks that real code
depends on are ported, not excused, as FTEQCC's aliasing of the return global already is
(FTE-18.13); the corpus differentials find them. Tier 3: when
the oracle miscompiles a program, the divergence is recorded as an oracle defect only when the
harness proves it mechanically. The proof shows the oracle's progs reading a global that a
statement unrelated to the value wrote, such as a `LOAD_V` into a one-word local overwriting
the next two. A divergence with no such proof is a defect of qc-codegen, as before. Code
generation moves to an intermediate layer between the semantic model's typed bodies and the
progs: its values and their lifetimes are explicit, and back-end passes allocate temporaries
and lay out memory under each dialect's profile. That is where an FTEQCC-exact allocation mode
could be added later if a mod is ever found to depend on it. This keeps the oracles as the
authority on behaviour while leaving the compiler's structure its own, as ADR-0017 requires,
and gives the later requirements somewhere to live.

## Consequences

- #649 (P2-153): an acceptance criterion allows an oracle defect proved by the harness, and the
  deliverable adds the proof to `cargo xtask compiler-diff` and a spec statement in
  `docs/spec/differential.md`. Program 555 is the first case. 1108 and 1230 are diagnosed and
  fixed, or proved, under the same rule.
- A new issue, L-14 ([#685](https://github.com/quakecstudio/editor/issues/685)), builds the
  intermediate layer and moves temporary allocation and layout into its passes. Justin placed
  it in Phase 2, to be worked next; L-04 (dead-code removal and deduplication) depends on it.
- Since decided: 1108 is the same defect as 555 and 1230 a second one, a vector multiply
  written over its own operand (`docs/spec/differential.md`, DIFF-14.6 and DIFF-14.7); Justin
  chose a patched FTEQCC build as the proof (DIFF-14.4, DIFF-14.5).
- The rule of 2026-10-30, zero divergences with quirks ported rather than excused, keeps its
  force for tiers 1 and 2. Tier 3 is the only exception, and only with proof.
- FTE-21.7 and FTEF-27.16 describe this compiler's temporaries; where they state FTEQCC's
  allocation, they are corrected against `QCC_GetTemp`, `QCC_LockActiveTemps`,
  `QCC_RemapLockedTemps` and `QCC_FinaliseTemps` when #649 or L-14 touches them.

## Links

- Decision: Justin Plock's message of 2026-11-03 choosing option B, recorded in the pull
  request that adds this ADR; issue #649.
- Related: ADR-0017 (FTEQCC's clean-port policy).
- Sources: `docs/design/design-doc.md` ("Compiler"); `docs/design/semantic-model.md`;
  `plan/requirements.yaml` (COMP-3 to COMP-8); fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e,
  `engine/qclib/qcc_pr_comp.c` (`QCC_GetTemp`, `QCC_FreeTemp`, `QCC_LockActiveTemps`,
  `QCC_RemapLockedTemps`, `QCC_PR_RefExpression`) and `engine/qclib/qccmain.c`
  (`QCC_FinaliseTemps` after `QCC_UnmarshalLocals`); the generated programs of seed 1 under
  `target/compiler-diff/generated/1/`, with both compilers' statements for 555's `fn0`.
