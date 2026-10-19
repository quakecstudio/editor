# ADR-0015: Also use DarkPlaces' client and menu QuakeC definitions as fact-only sources

- **Status:** Accepted
- **Date:** 2026-10-19
- **Decided by:** Justin Plock
- **Issue:** plan key P2-35 (its issue is created when the plan is synced)
- **Requirements:** LANG-6, PRIN-8, AGT-9, LIC-6, LIC-7

## Question

May the engine-target database also take builtin and extension facts from DarkPlaces'
client and menu QuakeC definition files, `csprogsdefs.qc` and `menudefs.qc`, on the basis
ADR-0014 gave its extension list?

## Context

[ADR-0014](0014-engine-fact-sources.md) made exactly three files fact-only sources: FTEQW's
`fteextensions.qc`, DarkPlaces' `dpextensions.qc` and the rerelease `defs.qc`, and says that
"Taking facts from a further file, such as another engine's builtin list, needs the same
decision for that file." P2-31 (#409) gave FTEQW client (CSQC) and menu QuakeC tables, read
from `fteextensions.qc` with `CSQC` or `MENU` defined, but DarkPlaces' `dpextensions.qc`
holds only its server QuakeC builtins, so DarkPlaces had no client or menu table and a
client or menu module targeting it was not checked (ENGT-4.8). DarkPlaces declares its
client and menu builtins in two further files of the same directory.

What the two files and their repository say, read at the commit `crates/qc-engines/sources.toml`
already pins for `dpextensions.qc`, on 2026-10-19:

- `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a`,
  `dpdefs/csprogsdefs.qc` (SHA-256
  `f9b3562ccc1298cc4794b44fb07071b5458e9919b6e674f911d666e3c54b76ab`, 1512 lines). The file
  has no license header. It opens, verbatim:

  ```
  /*
  ==============================================================================

  			SOURCE FOR GLOBALVARS_T C STRUCTURE
  			MUST NOT BE MODIFIED, OR CRC ERRORS WILL APPEAR

  ==============================================================================
  */
  ```

  No line of it contains "license", "copyright" or "GPL" (any case).
- `DarkPlacesEngine/darkplaces@5d80f03e26871b273da103f3b750820a3718c01a`,
  `dpdefs/menudefs.qc` (SHA-256
  `1bfe701c54ffece1ba6a362920c9f9ec6738e45aae31413e0ab14ff8fa6f96bd`, 605 lines). The file has
  no license header. It opens, verbatim:

  ```
  //////////////////////////////////////////////////////////
  // sys globals
  ```

  No line of it contains "license", "copyright" or "GPL" (any case).
- The repository's `COPYING` is the one ADR-0014 recorded (SHA-256
  `ba9ed2269151ad63b922598f3c5c19ddf781b2bbb6cb843ed9f085ecd1679c5d`), the GNU General Public
  License, version 2; GitHub reports the repository's license as `GPL-2.0`.

As for `dpextensions.qc`, no header and no license file forbids taking names, numbers or
signatures from these files, or redistributing such facts, and the repository's grant is
GPL version 2 with no "or any later version", so no text or code of the files is copied into
this GPL-3.0-or-later project.

## Options

| Option | Consequences |
| --- | --- |
| A. Fact-only sources now (chosen) | P2-35 gives DarkPlaces client and menu QuakeC tables, generated from the two files pinned by commit and SHA-256 in `crates/qc-engines/sources.toml`; only facts are extracted, by the same rules (ENGT-2), and a client or menu module targeting DarkPlaces is checked for `W0311` |
| B. Wait for P4-22 | DarkPlaces keeps only a server QuakeC table until the license compliance review (P4-22, #133) rules on the files; client and menu modules targeting DarkPlaces stay unchecked |

B was not chosen for the reason ADR-0014 gave for its sources: the facts are what the
database needs, and taking facts alone needs no license to the files' expression.

## Decision

Justin Plock decided on 2026-10-19 to extend the fact-only approval of ADR-0014 to DarkPlaces'
`dpdefs/csprogsdefs.qc` and `dpdefs/menudefs.qc`. The engine-target database now has five
**fact-only sources**: FTEQW's `fteextensions.qc`, DarkPlaces' `dpextensions.qc`,
`csprogsdefs.qc` and `menudefs.qc`, and the rerelease `defs.qc`. From them it takes only
facts: builtin numbers and engine names, QuakeC names, signatures and extension names. No
comment text and no code of any of them is copied into the repository, and none of them nor
its repository is a porting upstream: none is added to the upstream table of
`docs/porting.md`, and nothing in this repository is ported from them. Each file is pinned by
commit and SHA-256, with its repository's license file, in `crates/qc-engines/sources.toml`;
`cargo xtask engines` fetches the pinned files, checks the checksums and generates
`crates/qc-engines/src/tables.rs`, and the extraction rules are ENGT-2 in
`docs/spec/engine-targets.md`. Everything else ADR-0014 decided stands, and this ADR
supersedes it so that one record lists every fact source.

This is not legal advice; the license compliance review before the first public release
(P4-22, #133) confirms the details.

## Consequences

- `crates/qc-engines` (L3) gives DarkPlaces a client QuakeC table from `csprogsdefs.qc` and a
  menu QuakeC table from `menudefs.qc` (ENGT-3.8), and `W0311` checks client and menu modules
  targeting DarkPlaces (ENGT-4.8), in P2-35.
- Both files are pinned at the DarkPlaces commit already pinned for `dpextensions.qc`, so the
  three DarkPlaces tables describe one revision of the engine.
- Taking facts from a further file still needs the same decision for that file.
- CI regenerates the tables from the pinned files and fails on any difference
  (`cargo xtask engines --check`).
- P4-22 (#133) reviews these sources with the rest of the release's license compliance.

## Links

- Plan key P2-35 and the pull request that merged this ADR.
- Related ADRs: [ADR-0014](0014-engine-fact-sources.md), which this supersedes;
  [ADR-0002](0002-license-model.md) (the project license and the P4-22 review).
- Sources, all read on 2026-10-19:
  - <https://github.com/DarkPlacesEngine/darkplaces/blob/5d80f03e26871b273da103f3b750820a3718c01a/dpdefs/csprogsdefs.qc>
  - <https://github.com/DarkPlacesEngine/darkplaces/blob/5d80f03e26871b273da103f3b750820a3718c01a/dpdefs/menudefs.qc>
  - <https://github.com/DarkPlacesEngine/darkplaces/blob/5d80f03e26871b273da103f3b750820a3718c01a/COPYING>
  - `crates/qc-engines/sources.toml`, `docs/spec/engine-targets.md`, `docs/porting.md`
    (upstreams).
