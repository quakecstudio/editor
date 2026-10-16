# Risk register

The risks to QuakeC Studio, who owns each one, the issues that carry its mitigation and where it
stands. It tracks the [design document](design/design-doc.md#risks-and-open-questions)'s "Risks and
open questions" (`OPEN-5`): every row of the design document's risk table is here with the same
risk, impact and mitigation text. To add, reword or retire a risk, change the design document and
this page in the same PR.

`python3 plan/check.py` fails if the two tables differ, or if a row here has no owner, no linked
issue or an unknown status.

## How it is kept

- **Owner** is the `area:*` label whose issues carry the mitigation. Justin
  ([@jplock](https://github.com/jplock)) is accountable for every area; the agent or person
  working an owner's issues updates the row when one of them closes.
- **Linked issues** are the issues that deliver the mitigation, or that would show the risk
  happening.
- **Status** is one of:
  - **Open**: no mitigating issue has closed yet.
  - **Mitigating**: some mitigating issues have closed; others are still open.
  - **Mitigated**: every mitigating issue has closed; the risk is watched but needs no new work.
  - **Retired**: the risk can no longer happen.
- **Review at every gate.** Each phase gate review (`/gate-review`) re-reads this page, updates
  the status of every row, adds any risk the phase surfaced (to the design document first) and
  records the review in the log below. The gate report in `docs/gates/<phase>.md` links the
  result.

## Register

| ID | Risk | Impact | Mitigation | Owner | Linked issues | Status |
| --- | --- | --- | --- | --- | --- | --- |
| R-1 | Our compiler mis-handles real-world mods | Mods that build elsewhere fail or behave differently here | Differential tests against id's VM; compile the rerelease and a public mod corpus in CI; publish a compatibility list | area:compiler | [#16](https://github.com/quakecstudio/editor/issues/16) corpora, [#37](https://github.com/quakecstudio/editor/issues/37) differential VM runner, [#38](https://github.com/quakecstudio/editor/issues/38) corpus tooling, [#65](https://github.com/quakecstudio/editor/issues/65) compiler differential suite, [#66](https://github.com/quakecstudio/editor/issues/66) conformance suite, [#132](https://github.com/quakecstudio/editor/issues/132) compatibility list | Mitigating |
| R-2 | Compiler is now on the critical path | MVP slips | Vanilla first, then only the rerelease's FTEQCC features; GMQCC syntax after the MVP | area:compiler | [#62](https://github.com/quakecstudio/editor/issues/62) vanilla codegen, [#57](https://github.com/quakecstudio/editor/issues/57) rerelease FTEQCC spec, [#63](https://github.com/quakecstudio/editor/issues/63) rerelease FTEQCC codegen, [#186](https://github.com/quakecstudio/editor/issues/186) GMQCC (post-MVP) | Open |
| R-3 | Map compiler output differs from established tools | Maps leak, break or light differently than with ericw-tools | Differential checks against id's tools and ericw-tools in CI; compile a public map corpus | area:map | [#16](https://github.com/quakecstudio/editor/issues/16) map corpus, [#160](https://github.com/quakecstudio/editor/issues/160) map compiler oracles, [#164](https://github.com/quakecstudio/editor/issues/164) map compiler differential suite | Mitigating |
| R-4 | Map editor scope rivals the rest of the IDE | Effort pulled from the code and preview work | Map editor phases start only after the MVP; entity editing ships before brush editing | area:process | [#136](https://github.com/quakecstudio/editor/issues/136) MVP gate (Phase 5 depends on it), [#147](https://github.com/quakecstudio/editor/issues/147) entity editing (Phase 5), [#165](https://github.com/quakecstudio/editor/issues/165) brush editing (Phase 6) | Open |
| R-5 | License non-compliance | Distribution blocked or takedown requests | GPL-3.0-or-later throughout (ADR-0002); notices and a source link in every package; compliance review before the first public release | area:legal | [#8](https://github.com/quakecstudio/editor/issues/8) license model, [#2](https://github.com/quakecstudio/editor/issues/2) license and attribution checks, [#129](https://github.com/quakecstudio/editor/issues/129) packaging, [#207](https://github.com/quakecstudio/editor/issues/207) mod export, [#133](https://github.com/quakecstudio/editor/issues/133) compliance review | Mitigating |
| R-6 | FTEQW unavailable or weak on macOS | No live debug on Mac | Retired: the IDE runs only our own preview host, on all three OSes (ADR-0005) | area:preview | [#12](https://github.com/quakecstudio/editor/issues/12) preview host base, [#14](https://github.com/quakecstudio/editor/issues/14) default preview target | Retired |
| R-7 | Editor text and native feel fall short on egui | The editor looks or types worse than Zed | Shaping library with font fallback, IME through winit, AccessKit text, `muda` menus; P2-17, P2-18 and P2-23 acceptance; keep UI as a thin top layer | area:editor | [#10](https://github.com/quakecstudio/editor/issues/10) gpui vs egui spike, [#1](https://github.com/quakecstudio/editor/issues/1) pinned workspace, [#2](https://github.com/quakecstudio/editor/issues/2) layering check, [#73](https://github.com/quakecstudio/editor/issues/73) app shell | Open |
| R-8 | Small audience | Slow adoption | LSP + tree-sitter + MCP reach users in their existing editors and agents | area:language | [#70](https://github.com/quakecstudio/editor/issues/70) LSP server, [#71](https://github.com/quakecstudio/editor/issues/71) tree-sitter grammar, [#72](https://github.com/quakecstudio/editor/issues/72) editor integrations, [#124](https://github.com/quakecstudio/editor/issues/124) MCP server | Open |
| R-9 | Agent invents builtins or semantics | Broken mods, lost trust | Engine-target database in context; nothing is done without a clean compile and passing tests | area:agent | [#60](https://github.com/quakecstudio/editor/issues/60) engine-target database, [#119](https://github.com/quakecstudio/editor/issues/119) agent context, [#117](https://github.com/quakecstudio/editor/issues/117) done criteria, [#126](https://github.com/quakecstudio/editor/issues/126) agent evals | Open |
| R-10 | No redistributable test assets | Weak CI | Open asset set for CI; user's own data for local runs | area:legal | [#16](https://github.com/quakecstudio/editor/issues/16) corpora, [#17](https://github.com/quakecstudio/editor/issues/17) test assets for CI, [#38](https://github.com/quakecstudio/editor/issues/38) corpus tooling, [#43](https://github.com/quakecstudio/editor/issues/43) the user's game data | Mitigating |
| R-11 | Our Rust preview host is a whole engine on the MVP path | MVP slips; movement, rendering or mods behave differently than in the engines players use | Port from id's source with specs; server simulation compared frame by frame with id's engine as an oracle, rendering and sound within stated tolerances; renderer shared with the map editor | area:preview | [#12](https://github.com/quakecstudio/editor/issues/12) preview host decision, [#90](https://github.com/quakecstudio/editor/issues/90) preview host app, [#146](https://github.com/quakecstudio/editor/issues/146) renderer (moved to Phase 3), [#43](https://github.com/quakecstudio/editor/issues/43) the user's game data, [#136](https://github.com/quakecstudio/editor/issues/136) MVP gate | Open |

## Open questions

The design document's open questions that are still unanswered, and the issue that answers each.

| Question | Owner | Issue |
| --- | --- | --- |
| Product name (QuakeC Studio is unclaimed in search, worth a trademark check) | area:legal | [#15](https://github.com/quakecstudio/editor/issues/15) |

## Review log

| Date | Review | Changes |
| --- | --- | --- |
| 2026-10-15 | Register created ([#19](https://github.com/quakecstudio/editor/issues/19)) | All ten design-document risks entered; R-1, R-3 and R-10 are Mitigating because the corpora selection ([#16](https://github.com/quakecstudio/editor/issues/16)) closed, R-5 because the license decision ([#8](https://github.com/quakecstudio/editor/issues/8)) closed |
| 2026-10-15 | ADR-0004 ([#12](https://github.com/quakecstudio/editor/issues/12)) | R-11 added for the Rust preview host on the MVP path; its engine, oracle and fidelity issues get numbers when the plan is synced and are linked here then |
| 2026-10-15 | ADR-0005 ([#14](https://github.com/quakecstudio/editor/issues/14)) | R-6 retired: the IDE runs only our own preview host; R-11's oracle is id's engine instead of QuakeSpasm |
| 2026-10-15 | ADR-0003 ([#10](https://github.com/quakecstudio/editor/issues/10)) | R-7 rewritten for egui: the risk is now the editor's text and native feel, not gpui's API churn; the web build question is answered |
