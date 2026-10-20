# ADR-0017: Port FTEQCC's compiler behaviour from FTEQW's source

- **Status:** Accepted
- **Date:** 2026-10-20
- **Decided by:** Justin Plock
- **Issue:** [#484](https://github.com/quakecstudio/editor/issues/484)
- **Requirements:** LIC-1, LIC-2, LIC-6, LIC-8, LANG-5

## Question

May the FTEQCC dialect be ported from FTEQW's compiler source (`engine/qclib` of
`fte-team/fteqw` at `f937b9d88f71fc4429db5fe56c6a98d922711b2e`), and if so, how?

## Context

LANG-5 asks for an FTEQCC dialect profile, and LIC-8 makes "a pinned FTEQCC, built with the
oracles", the definition of expected behaviour for the rerelease. Until now FTEQW's source was
read for facts and the profile's behaviour inferred from it and from oracle cases: the
`docs/porting.md` Upstreams table listed only id's repositories, and FTEQW's QuakeC
definition files were fact-only sources (ADR-0014 to ADR-0016). The FTEQCC dialect work of
Phase 2 (P2-69 to P2-79) reads FTEQCC's lexer and preprocessor function by function, and
the macro expansion of #484 and #485 needs its behaviour exactly, so reading it only "for
facts" no longer describes the work.

What FTEQW's repository says about its licence, read at the pinned commit on 2026-10-20:

- `README.md`: "Copyright (c) 2004-2025 FTE's team and its contributors", then "FTEQW is
  supplied to you under the terms of the same license as the original Quake sources, the GNU
  General Public License Version 2."
- `engine/qclib/LICENSE` is the GPL-2.0 text (87 lines, "GNU GENERAL PUBLIC LICENSE",
  "Version 2, June 1991", ending "END OF TERMS AND CONDITIONS").
- The `engine/qclib/qcc_*.c` files carry no licence or copyright notice.

The project is GPL-3.0-or-later (ADR-0002). id's sources are GPL-2.0-or-later and are used
under version 3. FTEQW's README states version 2 and grants no "or later" in words; the
README's "the same license as the original Quake sources" may be read as that grant, but this
ADR does not settle it.

## Options

| Option | Consequences |
| --- | --- |
| A. Keep FTEQW read-only for facts | The FTEQCC profile stays a fresh design checked only by oracle cases; every behaviour must be found by a case before it is known, as the 24 divergences #484's differential found show. |
| B. Port FTEQCC's behaviour, written fresh in our design (chosen) | FTEQCC's source becomes the authority for the dialect, cited function by function; the Rust stays one implementation in our lexer, preprocessor, CST and semantic model; the oracle differentials stay as proof. |
| C. Port FTEQCC's code structure | A second compiler beside ours, with FTEQCC's globals and control flow; rejected as an amalgamation that cannot be supported. |

## Decision

FTEQW (`fte-team/fteqw`) is a porting upstream for FTEQCC's compiler, `engine/qclib`, at
`f937b9d88f71fc4429db5fe56c6a98d922711b2e`. Justin Plock decided it on 2026-10-20: "all of
this is GPL, and we are making our code available too under the GPL. i don't care if it's v2
or v3. either is fine." The FTE team is not to be contacted about it.

The port follows a clean-port policy, in Justin's words: "i don't want an amalgamation and
nasty code. we should try and create a supportable and clean code engine implementation". So:

- port FTEQCC's behaviour, not its structure;
- its source is the authority, read and cited function by function;
- the Rust is written fresh inside the existing lexer, preprocessor, CST and semantic model;
- no globals and no transliterated control flow, and CONV-17 throughout;
- one implementation with dialect profiles, never parallel compilers;
- the oracle differentials stay as the proof.

## Consequences

- `docs/porting.md` lists `fte-team/fteqw` (`engine/qclib` only) in its Upstreams table, with
  the commit, the licence as recorded here and the copyright line a ported file keeps, and
  states the clean-port policy; `.claude/rules/porting.md` summarises it. `cargo xtask
  licenses` (rules H1 to H5) accepts `Ported from fte-team/fteqw@...` headers through that row.
- A file ported from FTEQW keeps the copyright line its README carries, the only one the
  repository has: `Copyright (c) 2004-2025 FTE's team and its contributors`.
- The fact-only ADRs (ADR-0014 to ADR-0016) are unchanged: they cover FTEQW's QuakeC definition
  files for the engine-target database, not its compiler.
- #484 and #485 are the first ports under this decision.
- As ADR-0002 says, the licence compliance review before the first public release
  ([#133](https://github.com/quakecstudio/editor/issues/133), P4-22) confirms the details,
  here including how FTEQW's version 2 terms combine with the project's GPL-3.0-or-later.

## Links

- Decision: Justin Plock, 2026-10-20, in the session that orchestrates the Phase 2 work.
- Related ADRs: ADR-0002 (licence model), ADR-0014 to ADR-0016 (fact-only sources).
- Sources: `README.md` and `engine/qclib/LICENSE` of
  https://github.com/fte-team/fteqw/tree/f937b9d88f71fc4429db5fe56c6a98d922711b2e, and
  `engine/qclib/qcc_pr_lex.c` and `engine/qclib/qcc_cmdlib.c` at that commit.
