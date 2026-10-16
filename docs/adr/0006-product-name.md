# ADR-0006: Keep the name QuakeC Studio

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, choosing option A over the recommended option C
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

Decided: **option A. The product stays QuakeC Studio**, with a non-affiliation notice, and the name
is not registered as a trademark. The agent's recommendation was option C (a neutral name such as
Progsmith, for the reasons under Options); Justin chose to keep the name, accepting the risk that a
rename request after release would reach the packages, bundle identifiers, crates and domain at
once (risk R-5). "QuakeC" is used as the name of the language the tool works with.

| Item | Name |
| --- | --- |
| Product (display) | QuakeC Studio |
| Desktop app binary | `quakec-studio` |
| CLI and MCP server | `quakec-ide` (subcommands `build`, `check`, `run`, `mcp`), as in the design document |
| LSP server | `quakec-lsp` |
| Crate prefix | `qs-` for project crates and `qc-` for the QuakeC formats, VM and builtins, unchanged |
| Bundle identifier | `com.quakecstudio.QuakeCStudio` (macOS); Linux app id the same |
| Application directories | as [ADR-0011](0011-app-directories.md): `QuakeC Studio` on macOS and Windows, `quakec-studio` on Linux |

## Consequences

- No rename: the GitHub organization, crates, binaries, `quakecstudio.com` and ADR-0011 stay as
  they are.
- `docs/design/design-doc.md`: OPEN-1 is checked off; `plan/requirements.yaml` is unchanged.
- P0-23 ([#23](https://github.com/quakecstudio/editor/issues/23)) signs with the bundle identifier
  above; P2-13 and P2-14 keep `quakec-ide` and `quakec-lsp`.
- Every package, the README and the About box carry the notice "Quake is a trademark of ZeniMax
  Media Inc.; this project is not affiliated with or endorsed by ZeniMax or id Software" (checked
  in the license review, P4-22). No logo, artwork or trade dress from id or ZeniMax is used.
- If ZeniMax ever asks for a rename, the names to change are the ones in the table above; a new ADR
  supersedes this one.

## Links

- Issue: [#15](https://github.com/quakecstudio/editor/issues/15); pull request: this ADR's draft PR.
- Related: [ADR-0011](0011-app-directories.md) (application directories), [ADR-0002](0002-license-model.md).
- [ZeniMax Trademark and Brand Guidelines](https://www.zenimax.com/en/legal/legal-information), fetched 2026-10-15.
- [dE/Quake editing legalese](https://www.gamers.org/dEngine/license/quakeedit.html) (Jay Wilbur on names, 1996).
- USPTO record mirrors: [QUAKE 74545680](https://uspto.report/TM/74545680), [QUAKE 99680430](https://www.trademarkelite.com/trademark/trademark-detail/99680430/QUAKE), [ZeniMax marks on Justia](https://trademarks.justia.com/owners/zenimax-media-inc-696870/); Swiss mark [832080](https://www.wirtschaft.ch/trademarks/QUAKE/ZeniMax+Media+Inc+Rockville+MD+20850/06344/2025/).
- Official search tools to use: [USPTO Trademark Search](https://tmsearch.uspto.gov/), [EUIPO eSearch](https://euipo.europa.eu/eSearch/).
- [crates.io API](https://crates.io/api/v1/crates/quakec), [rdap.org](https://rdap.org/), [QCStudio](https://qcstudio.com/), [EDICTOR](https://edictor.org/).
