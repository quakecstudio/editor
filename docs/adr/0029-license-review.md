# ADR-0029: Release under the license review's findings, and port id's map tools as GPL

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decided by:** Justin Plock
- **Issue:** [#133](https://github.com/quakecstudio/editor/issues/133)
- **Requirements:** LIC-6

## Question

Does the first public release meet its license obligations (notices, corresponding source,
dependency licenses and corpus handling), and may id's map tools in `Quake-Tools/qutils`, whose
files carry no notice and whose `COPYING` is the GPL-2.0 text, be ported into this
GPL-3.0-or-later project?

## Context

LIC-6 asks for a license compliance review before the first public release. ADR-0002 made the
project GPL-3.0-or-later so that id's GPL source could be ported, and left two upstreams for
this review: id's `qutils` (`docs/porting.md` lists it as "Not yet portable") and FTEQW, whose
README names version 2 (ADR-0017). Justin ruled on 2026-11-16 that the project decides this
itself, with no outside approval sought: "if they are GPL-2.0 compatible, let's go with that.
we will not be seeking any approval from anyone. so make a determination on whether we can port
with attribution or pull from source as we are now."

What the review read, on 2026-11-16:

- **id-Software/Quake-Tools at `c0d1b91c74eb654365ac7755bc837e497caaca73`.** The commit that
  added `qcc/` and changed the tree is titled "Source release of qcc and the qutils, updated
  long ago to be GPL." `qutils/COPYING` is the GPL text, "Version 2, June 1991". The `qutils`
  sources carry no notice (`qutils/QBSP/QBSP.C` begins `// bsp5.c`), and `qutils/README.TXT` is
  the 1996 developer CD readme, with no license statement. The `qcc/` files of the same
  release carry id's notice: "Copyright (C) 1996-1997 Id Software, Inc. ... either version 2 of
  the License, or (at your option) any later version."
- **GPL-2.0, section 9** (the text of `qutils/COPYING`): "If the Program specifies a version
  number of this License which applies to it and "any later version", you have the option of
  following the terms and conditions either of that version or of any later version published
  by the Free Software Foundation. If the Program does not specify a version number of this
  License, you may choose any version ever published by the Free Software Foundation."
- **The GPL FAQ** (gnu.org, fetched 2026-11-16), "Is GPLv3 compatible with GPLv2?": "No. ...
  However, if code is released under GPL "version 2 or later," that is compatible with GPLv3
  because GPLv3 is one of the options it permits." And "Is it enough just to put a copy of the
  GNU GPL in my repository?": "Just putting a copy of the GNU GPL in a file in your repository
  does not explicitly state that the code in the same repository may be used under the GNU GPL.
  ... lawyers and courts might apply common sense and conclude that you must have put the copy
  of the GNU GPL there because you wanted to license the code that way."
- **FTEQW at `f937b9d88f71fc4429db5fe56c6a98d922711b2e`** (ADR-0017): its README says "FTEQW is
  supplied to you under the terms of the same license as the original Quake sources, the GNU
  General Public License Version 2."
- **The release as it is:**
  - Every package holds `LICENSE` (the GPL-3.0 text) and `packaging/NOTICE.txt`. That notice
    names the tagged commit as the corresponding source, and the release notes link the tag
    (`.github/workflows/release.yml`).
  - `cargo deny check licenses` passes: every crate's license is on `deny.toml`'s allow list,
    and each license there is GPL-3.0-compatible.
  - `cargo xtask licenses` and `cargo xtask attribution` pass: every source file carries its
    SPDX line, every ported file carries its upstream's copyright line, and every asset has its
    row in `ATTRIBUTION.md`.
  - Quake's game data and the corpus mods are never committed or bundled. They are fetched by
    checksum into a cache outside the tree (`corpus/manifest.toml`), and a mod whose license
    allows only unmodified redistribution is recorded and not used (Quoth).
  - What the packages did not hold: the license texts of the third-party crates the binaries
    link, which MIT, BSD, Apache-2.0 and the other permissive licenses ask a binary copy to
    carry, and the OFL texts of the fonts the app embeds. These texts were only in the
    repository.

## Options

| Option | Consequences |
| --- | --- |
| A. `qutils` stays read-only, used as an oracle only | P6's map compilers stay ports of ericw-tools and our own code. A behaviour of id's tools, which the `id` dialect and the id-exact differentials follow, is found by running them, not by reading their source. |
| B. `qutils` is GPL with no version stated, so any version may be chosen: port it under GPL-3.0-or-later with id's copyright line (chosen) | id's map tools join the upstream table in `docs/porting.md`. A port keeps `Copyright (C) 1996-1997 Id Software, Inc.` and cites the file and function, as the `qcc/` ports do. The four-part porting process applies unchanged. |
| C. `qutils` is GPL-2.0-only | Incompatible with GPL-3.0-or-later (the FAQ above): it could only be read, as in A, and could never be linked. |

## Decision

**`qutils` is GPL with no version specified, and is ported under GPL-3.0-or-later.** Its files
carry no notice, and id's commit releases them "to be GPL". The `COPYING` beside them is the
license's text, not a statement that the program is under version 2 only. Section 9 of that
text says a program that "does not specify a version number" may be used under "any version
ever published", GPLv3 among them. The same release licenses its `qcc/` as "version 2 ... or
(at your option) any later version", and id's other source releases are licensed the same way,
which supports reading `qutils` as GPL of any version rather than version 2 only. A port keeps
`Copyright (C) 1996-1997 Id Software, Inc.` and its `Ported from id-Software/Quake-Tools@...`
line, as every id port does. The reading that the bare `COPYING` means version 2 only (option
C) is rejected: it is the FAQ's case of a license text with no statement, and section 9 decides
that case.

**FTEQW is read as GPL-2.0-or-later.** Its README places it under "the same license as the
original Quake sources", which is version 2 or any later version. The words "the GNU General
Public License Version 2" name that license's first version, so its ports under ADR-0017 stand.

**The release meets LIC-6 with one change made here: every package now carries
`THIRD-PARTY`.** `cargo xtask notices --out` writes it from `cargo metadata` and
`ATTRIBUTION.md`, and the release build runs it before packaging. It lists each crate the
shipped binaries link, with its version, its license expression and the license files it ships
(a text shared word for word is written once), then each bundled asset of another license, with
its license text. `cargo xtask ci` runs `cargo xtask notices --check`, which fails when a linked
crate states no license.

No outside review or counsel's letter is sought. The verification of P4-22 is this ADR,
accepted by Justin.

## Consequences

- `docs/porting.md`:
  - `Quake-Tools/qutils/` joins the upstream table, with this ADR as its license note.
  - The "Not yet portable" paragraph is removed.
  - The FTEQW row cites this ADR.
- `plan/backlog/p4.yaml`: P4-22's verification is this ADR, so #133 closes with this PR.
- The release:
  - `packaging/THIRD-PARTY.txt` is written by `cargo xtask notices` in each job of
    `.github/workflows/reusable-build.yml`, is ignored by git, and is installed beside `NOTICE`
    in the app bundle, the MSI and zip, and the deb, rpm, AppImage and Flatpak.
  - `packaging/NOTICE.txt` and the release notes name it.
- `cargo xtask ci` gains the `notices` step.
- From now on, a port from `qutils` follows `docs/porting.md` like any other upstream. No port
  is planned by this ADR.

## Links

- [#133](https://github.com/quakecstudio/editor/issues/133); the pull request that merged this
  ADR.
- [ADR-0002](0002-license-model.md), [ADR-0017](0017-port-fteqcc.md),
  [ADR-0023](0023-ericw-tools-porting-upstream.md), [ADR-0025](0025-ericw-tools-vis-and-light.md).
- id-Software/Quake-Tools at `c0d1b91c74eb654365ac7755bc837e497caaca73`: `qutils/COPYING`
  (section 9), `qutils/README.TXT`, `qutils/QBSP/QBSP.C`, `qcc/pr_comp.c`, and the commit
  message.
- The GPL FAQ, <https://www.gnu.org/licenses/gpl-faq.html>, entries `#v2v3Compatibility` and
  `#LicenseCopyOnly`, fetched 2026-11-16.
- fte-team/fteqw at `f937b9d88f71fc4429db5fe56c6a98d922711b2e`, `README.md`.
