# ADR-0006: Name the product without the Quake trademark

- **Status:** Proposed
- **Date:** 2026-10-15
- **Decided by:** pending
- **Issue:** [#15](https://github.com/quakecstudio/editor/issues/15)
- **Requirements:** OPEN-1

## Question

What is the product called, and what names do its binaries, crates and bundle identifier use, given
that "Quake" is a registered trademark of ZeniMax?

## Context

The design document lists the product name as an open question: "QuakeC Studio is unclaimed in
search, worth a trademark check" (OPEN-1). The name is about to be fixed in places that are costly
to change later: the CLI and LSP binaries (P2-13 [#69](https://github.com/quakecstudio/editor/issues/69),
P2-14 [#70](https://github.com/quakecstudio/editor/issues/70)), the app shell (P2-17
[#73](https://github.com/quakecstudio/editor/issues/73)), the signing credentials and their bundle
identifier (P0-23 [#23](https://github.com/quakecstudio/editor/issues/23)), the release packages
(P4-18 [#129](https://github.com/quakecstudio/editor/issues/129)) and the application directories
already set by [ADR-0011](0011-app-directories.md) (`QuakeC Studio`, `quakec-studio`). Today the
repository uses "QuakeC Studio", the `quakecstudio` GitHub organization, the crate prefixes `qs-`
(project crates) and `qc-` (QuakeC formats and VM), and the CLI name `quakec-ide` from the design
document. Nothing has been published or signed yet.

### Search results, 2026-10-15

Trademarks. The official search tools (USPTO Trademark Search, EUIPO eSearch) are browser
applications an agent could not query; the US records below come from third-party mirrors of USPTO
data and are labelled as such. **A person must repeat the searches in the official tools and
attach the records to the issue before this ADR is accepted.**

| Mark | Office | Owner | Goods | Status | Source |
| --- | --- | --- | --- | --- | --- |
| QUAKE, reg. 2032587 (serial 74545680) | USPTO | id Software LLC (ZeniMax) | Computer software games and computer game programs | Registered 1997-01-21, renewed 2017 | uspto.report mirror |
| QUAKE, serial 99680430 | USPTO | ZeniMax Media Inc. | Class 9: recorded and downloadable computer game software | Application filed 2026-03-03 | TrademarkElite mirror |
| QUAKE, reg. 2254868 and others | USPTO | id Software / ZeniMax | Class 16 books, class 25 clothing, toys | Registered | Justia, Markinton mirrors |
| QUAKE, no. 832080 | Swiss register | ZeniMax Media Inc. | Classes 16, 25, 28, 41 | Registered 2025, to 2035 | wirtschaft.ch mirror |
| QUAKEC, QUAKEC STUDIO | USPTO, EUIPO | none found | - | No record found in the mirrors searched; official search not run | - |
| EU marks for QUAKE | EUIPO | not checked | - | Unverified: eSearch could not be queried | - |

ZeniMax's Trademark and Brand Guidelines (last updated 2024-05-02) allow its word marks "to
truthfully convey information about your product", but say, without a written license: "Don't use
ZeniMax's Brand Assets in the name of your business, product, service, app, domain name, social
media account" and "Don't alter ... for example, by combining ZeniMax's Brand Assets with other
terms". id took the same line in 1996, in Jay Wilbur's guidance on Quake utilities: "QuakeEd is a
nono ('qed', 'an editor that works with Quake' is OK)". Against that, community projects have used
"Quake" in their names for decades (QuakeSpasm, vkQuake, ezQuake) without visible action; that is
tolerance, not permission, and a signed, packaged IDE with its own domain is more visible than a
source port.

Names and domains (crates.io API, RDAP and whois, 2026-10-15):

| Name | crates.io | Domains | GitHub |
| --- | --- | --- | --- |
| QuakeC Studio | `quakec-studio`, `quakecstudio`, `quakec-ide`, `quakec-lsp` free; `quakec` taken (an embeddable QuakeC runtime, 0.0.0) | `quakecstudio.com` registered 2026-10-15 at Amazon Registrar (presumably Justin's; not verified); `.org`, `.io`, `.dev`, `.app` free | Organization `quakecstudio` exists (ours) |
| QC Studio | - | `qcstudio.com` is a live product, "QCStudio - Easy C++ Games Debugging" | - |
| Progsmith (candidate) | `progsmith` free | `progsmith.dev`, `progsmith.app` free; `progsmith.com` registered | User `progsmith` taken; an organization needs another name such as `progsmith-ide` |
| Edictor (candidate, dropped) | `edictor` free | `.dev`, `.app` free; `.com` taken | Taken; EDICTOR and eDictor are existing linguistics tools |

The project crates `qs-*` and `qc-*` checked (`qs-paths`, `qc-vm`, `qc-progs`, `qc-types`,
`qc-edict`, `qc-builtins`, `qc-lno`, `qc-codegen`) are all free on crates.io.

## Options

| Option | Consequences |
| --- | --- |
| A. Keep "QuakeC Studio", never register it, add a non-affiliation notice | No rename work. Rests on "QuakeC" being read as the language's name rather than ZeniMax's mark, which the guidelines' "combining with other terms" clause argues against, for the product name and the domain alike. A takedown or rename request after release would hit signed packages, bundle identifiers, crates and the domain at once (risk R-5) |
| B. Keep "QuakeC Studio" and ask ZeniMax for written permission | The only way to keep the name safely. Adds an outside dependency with no deadline: P0-23 and P2-13 wait, or proceed and rename if refused. Microsoft/ZeniMax may not answer a small open-source project |
| C. A neutral name with a descriptive tagline, such as "Progsmith, an IDE for QuakeC" (recommended) | Follows both ZeniMax's guidelines and id's 1996 example: "QuakeC" appears only to say what the tool works with. Cost now: rename the GitHub organization (GitHub redirects the old URLs), three `qs-` crates, ADR-0011's directory names and the docs, before anything is published. Loses the instantly searchable name; the tagline and keywords carry search instead |

## Decision

Recommendation: **option C, a neutral name with "an IDE for QuakeC" as its tagline**, with
Progsmith as the proposed name (a progs smith: it builds `progs.dat`) if it passes the official
USPTO and EUIPO searches. ZeniMax's published guidelines name exactly this case, a product and a
domain carrying its mark, as needing a license, and this is the cheapest moment to avoid it:
nothing is signed, published or packaged yet, and every place the name will be fixed is still an
open issue. Option A is a bet that tolerance shown to source ports extends to a signed IDE; option
B is right only if Justin wants to keep the name enough to wait for an answer. The names to fix:

| Item | Name |
| --- | --- |
| Product (display) | Progsmith, tagline "an IDE for QuakeC" |
| Desktop app binary | `progsmith` |
| CLI and MCP server | `progsmith` (subcommands `build`, `check`, `run`, `mcp`; the design document's `quakec-ide` is retired) |
| LSP server | `progsmith-lsp` |
| Crate prefix | `ps-` for project crates (replacing `qs-`); `qc-` stays for the QuakeC formats, VM and builtins, which describe the language and contain no mark |
| Bundle identifier | `dev.progsmith.Progsmith` (macOS), with `progsmith.dev` registered first; Linux app id the same |
| Application directories | `Progsmith` on macOS and Windows, `progsmith` on Linux (ADR-0011's layout unchanged) |

If Justin prefers another neutral name, the same table applies with that name, after the same
checks. If he chooses A or B instead, the table becomes `QuakeC Studio`, `quakec-studio`,
`quakec-ide`, `quakec-lsp`, `qs-` and `com.quakecstudio.QuakeCStudio`.

## Consequences

If accepted as recommended:

- **Before acceptance:** official USPTO and EUIPO searches for the chosen name (word mark, classes 9
  and 42) attached to #15; register `progsmith.dev` and an organization name.
- `docs/design/design-doc.md`: the title, the open question checked off, `quakec-ide` replaced in the
  Architecture diagram and Agent harness; `plan/requirements.yaml` unchanged (OPEN-1 is answered,
  not reworded).
- `plan/backlog/p2.yaml`: P2-13 and P2-14 name `progsmith` and `progsmith-lsp`; P2-16's extensions
  use the new name. `plan/backlog/p0.yaml`: P0-23's bundle identifier. These are wording changes, no
  new issues or dependencies.
- A follow-up chore issue: rename `crates/qs-*` to `ps-*`, update ADR-0011's directory names and
  `qs-paths`, and update README, CONTRIBUTING, SECURITY, CLAUDE.md and docs. ADR-0011's decision
  (native per-OS directories) is unchanged, so its names are corrected in place.
- The GitHub organization `quakecstudio` is renamed by Justin (agents cannot), and
  `quakecstudio.com` either kept as a redirect or let lapse.
- Every package and the README carry the notice "Quake is a trademark of ZeniMax Media Inc.;
  this project is not affiliated with or endorsed by ZeniMax or id Software" (checked in P4-22).

## Links

- Issue: [#15](https://github.com/quakecstudio/editor/issues/15); pull request: this ADR's draft PR.
- Related: [ADR-0011](0011-app-directories.md) (application directories), [ADR-0002](0002-license-model.md).
- [ZeniMax Trademark and Brand Guidelines](https://www.zenimax.com/en/legal/legal-information), fetched 2026-10-15.
- [dE/Quake editing legalese](https://www.gamers.org/dEngine/license/quakeedit.html) (Jay Wilbur on names, 1996).
- USPTO record mirrors: [QUAKE 74545680](https://uspto.report/TM/74545680), [QUAKE 99680430](https://www.trademarkelite.com/trademark/trademark-detail/99680430/QUAKE), [ZeniMax marks on Justia](https://trademarks.justia.com/owners/zenimax-media-inc-696870/); Swiss mark [832080](https://www.wirtschaft.ch/trademarks/QUAKE/ZeniMax+Media+Inc+Rockville+MD+20850/06344/2025/).
- Official search tools to use: [USPTO Trademark Search](https://tmsearch.uspto.gov/), [EUIPO eSearch](https://euipo.europa.eu/eSearch/).
- [crates.io API](https://crates.io/api/v1/crates/quakec), [rdap.org](https://rdap.org/), [QCStudio](https://qcstudio.com/), [EDICTOR](https://edictor.org/).
