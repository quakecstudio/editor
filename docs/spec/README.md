# Behaviour specs

`docs/spec/` describes what the Quake engine and id's tools actually do: file formats, VM
semantics, builtins, limits. Each spec is written with id's source open and cites the file and
function it describes. Code that implements a spec section names it, and tests that pin a
behaviour cite it, so a reader can go from a test to the rule to id's C and back.

How a spec fits into porting a piece of id's code is in [`../porting.md`](../porting.md).
Start a new spec from [`_template.md`](_template.md).

## Stable IDs

Every spec file has a short upper-case **prefix**, registered in the table below. Every
section and every behaviour statement has an ID built from it:

| Thing | Form | Example |
| --- | --- | --- |
| Section | `<PREFIX>-<n>` | `VM-12` |
| Behaviour statement | `<PREFIX>-<n>.<m>` | `VM-12.3` |
| Named constant | `<PREFIX>_<NAME>` in upper snake case | `VM_MAX_STACK_DEPTH` |

Rules:

- A section's number is its ID, not its position. New sections take the next free number in
  the file, wherever they appear on the page. Statements likewise take the next free `m` in
  their section.
- An ID is never renumbered and never reused. A section or statement that no longer applies
  stays in place, marked **Withdrawn** with the reason and the replacing ID, so old citations
  still resolve.
- Changing what a statement means is a new statement; the old one is withdrawn. Fixing its
  wording is not.
- Section headings are `## <ID> <Title>` (for example `## VM-12 Integer division`), so the
  ID is greppable and the heading anchor is stable.
- Statements are list items that begin with the bold ID: `- **VM-12.3** ...`. Only these are
  normative; prose around them explains but does not bind.

## Citing a spec

| Where | How |
| --- | --- |
| Test | A comment on the line above the test: `// spec VM-12.3: <short paraphrase>`; several IDs are separated by commas |
| Code | The rustdoc of the item that implements a section says `Implements VM-12` and names the statements it relies on |
| `#[expect]` reason | `reason = "spec VM-12.3: exact IEEE comparison"` |
| Issue or PR | `Spec: VM-12, VM-13` in the body |

A test cites the statements it pins, not just the section. Readiness requires every active
statement to be cited (TEST-3 in [`../testing.md`](../testing.md)).

## The spec-citation ratchet

`crates/qs-tests/tests/it/spec_coverage.rs` checks, on every `cargo test`:

- Every active statement in `docs/spec/*.md` (all but `README.md` and `_`-prefixed templates)
  is cited by a line starting `// spec <ID>` in the test code of a Rust file under
  `crates/`, `apps/` or `xtask/` (whose oracle module tests the oracles' C through the built
  oracle; its `fixtures/` cite statements of their own), or is listed in
  [`coverage-baseline.tsv`](coverage-baseline.tsv).
- Only a citation in test code counts, since TEST-3 asks for a test citing each statement.
  Test code is a file under a `tests` directory or named `tests.rs`; an item marked
  `#[test]` or `#[cfg(test)]`, such as an inline `mod tests`; the file of a module declared
  `#[cfg(test)] mod name;` (a test vector module such as `crates/qc-lno/src/vectors.rs`) and
  the files under its directory; and xtask's oracle scenarios in
  `xtask/src/oracles/scenarios/`, which are differential tests. A `// spec` line anywhere
  else cites nothing: shipped code names the statement it follows in a plain comment
  (`// VM-12.3: ...`) or its rustdoc. `xtask/src/test_code.rs` decides, for this ratchet and
  for the spec check of `cargo xtask readiness` alike.
- Every citation names a statement that exists and is not withdrawn. A citation names
  statements (`PREFIX-n.m`), never just a section.
- No statement in the baseline is cited: once a test cites it, it leaves the baseline in the
  same PR. The baseline only shrinks, and a crate passing readiness has none.
- [`requirements.tsv`](requirements.tsv) matches the spec files. On a mismatch the test
  prints the file's expected contents.

`requirements.tsv` is the index of every statement, one row each in file order,
tab-separated:

| Column | Meaning |
| --- | --- |
| `id` | The statement ID, `PREFIX-n.m` |
| `spec` | The spec file it is in, such as `vm.md` |
| `section` | Its section ID, `PREFIX-n` |
| `status` | `active`, or `withdrawn` when the text starts with `**Withdrawn**` |
| `statement` | The statement's text, continuation lines joined and whitespace collapsed |

`coverage-baseline.tsv` has the header `id` and one statement ID per line. A statement
written ahead of the code that implements it goes there, and the PR that adds the code
and its tests removes it.

Statements inside HTML comments are not read, which is how `_template.md` shows a withdrawn
statement without defining one.

## Writing a section

Each section holds, in this order:

1. **Source:** the id file and function it describes, with the repository and pinned commit.
2. **Behaviour:** numbered statements. Each states one observable fact, in terms a test can
   check, including the edge cases: empty input, limits, malformed data, and anything id's C
   leaves undefined. Where the C relies on undefined or platform-specific behaviour, the
   statement fixes one answer and says why (normally what the original x86 build did).
3. **Constants:** every limit or magic number as a named constant, with its value and the
   file, function and identifier it comes from. Code uses the constant's name, never a bare
   number.
4. **Test vectors:** inputs and expected outputs, each checked against id's code (through the
   oracles, [`oracles/README.md`](../../oracles/README.md)). Short vectors go in a table in the
   section; larger ones go in `docs/spec/vectors/<spec>/` and the section links them.

## Registry

| Prefix | File | What it covers | Issue |
| --- | --- | --- | --- |
| `BSP` | [`bsp.md`](bsp.md) | BSP29 maps: the header and every lump and field, what id's loader checks, visibility, lightmaps and hulls; the two BSP2 variants; `.lit` and `.ent` files; the FitzQuake lineage's loader, the default | [#142](https://github.com/quakecstudio/editor/issues/142) |
| `BUILTIN` | [`builtins.md`](builtins.md) | Every vanilla builtin, numbers 1 to 78: prototype, arguments, result, effects on edicts, globals and the engine, failures, and how faithfully each can be mocked | [#28](https://github.com/quakecstudio/editor/issues/28) |
| `DIFF` | [`differential.md`](differential.md) | The differential harness: scenarios, the deterministic mock engine both VMs run in (map setup, clients, server frames, mock builtins), errors and the end of a run, comparing two traces, expected divergences, statement coverage | [#51](https://github.com/quakecstudio/editor/issues/51) |
| `EDICT` | [`edicts.md`](edicts.md) | Edict memory and numbering, the world and client edicts, spawn and remove with reuse timing, map entity spawning, QuakeC entry points and think timing, and the system globals and fields with who writes each | [#27](https://github.com/quakecstudio/editor/issues/27) |
| `ENGT` | [`engine-targets.md`](engine-targets.md) | The engine-target database: its pinned fact sources, the facts-only extraction from them, the builtin and extension tables of the seven target engines for each module kind, and the warning for a call of a builtin the target lacks | [#60](https://github.com/quakecstudio/editor/issues/60) |
| `EXAMPLE` | [`example.md`](example.md) | The worked porting example: entity-string escapes | [#9](https://github.com/quakecstudio/editor/issues/9) |
| `FTE` | [`dialect-fteqcc-rerelease.md`](dialect-fteqcc-rerelease.md) | The FTEQCC features the 2021 rerelease QuakeC uses, each a construct vanilla `qcc` rejects: `for`, `switch`, `break`/`continue`, `++`/`--`, compound assignment, `%`, `?:`, vector members, `const`, `var`, `nosave`, arrays, `typedef struct`, `enum`, `#pragma`; and that the rerelease fits version 6 | [#57](https://github.com/quakecstudio/editor/issues/57) |
| `FTEF` | [`dialect-fteqcc.md`](dialect-fteqcc.md) | The full FTEQCC dialect beyond the rerelease subset: integer and the other C scalar types, typed numeric literals, pointers, sized and multidimensional arrays, `struct`/`union`, `typedef`, `enumflags`, classes, accessors, `__variant`, and the remaining `#pragma` directives; parse and resolve only, codegen marked later | [#81](https://github.com/quakecstudio/editor/issues/81) |
| `FS` | [`filesystem.md`](filesystem.md) | The game filesystem: search path order, `-basedir`, `-game`, `-path`, finding a file, the rerelease layout, recognising an install and its files by checksum | [#29](https://github.com/quakecstudio/editor/issues/29) |
| `LIMIT` | [`limits.md`](limits.md) | Format, VM, server and qcc limits as named constants | [#25](https://github.com/quakecstudio/editor/issues/25) |
| `LNO` | [`lno.md`](lno.md) | `.lno` line maps: which compilers write them, the header, the line and GMQCC column tables, matching a progs.dat, reading and writing | [#41](https://github.com/quakecstudio/editor/issues/41) |
| `MAP` | [`map.md`](map.md) | `.map` files: tokens, entities and key-value pairs, brushes, standard and Valve 220 texture definitions, the world and brush entities, the text both compilers read | [#142](https://github.com/quakecstudio/editor/issues/142) |
| `MD5` | [`md5.md`](md5.md) | The rerelease's MD5 models: tokens, the `.md5mesh` joints and meshes, the `.md5anim` hierarchy and frames, as QuakeSpasm-Spiked reads them | [#235](https://github.com/quakecstudio/editor/issues/235) |
| `MDL` | [`mdl.md`](mdl.md) | MDL alias models: header and the checks on it, skins and skin groups, texture coordinates, triangles, frames and frame groups, GLQuake's limits | [#142](https://github.com/quakecstudio/editor/issues/142) |
| `PAK` | [`pak.md`](pak.md) | PAK archives: header, directory, reading entries, the modified-game CRC | [#29](https://github.com/quakecstudio/editor/issues/29) |
| `PRJ` | [`project-file.md`](project-file.md) | Projects: `quakec.toml` and its modules, bare projects, output paths, the two forms of `progs.src`, the directives that decide which files are read, and each module's ordered file graph | [#47](https://github.com/quakecstudio/editor/issues/47) |
| `PROGS` | [`progs.md`](progs.md) | progs.dat version 6 and FTE version 7, and the QC type layout | [#25](https://github.com/quakecstudio/editor/issues/25) |
| `QC` | [`language.md`](language.md) | Vanilla QuakeC as id's `qcc` compiles it: lexing, `$frame`, the preprocessor, declarations, statements, expressions, types, `progs.src` and the progs each construct emits | [#30](https://github.com/quakecstudio/editor/issues/30) |
| `QCD` | [`language-diagnostics.md`](language-diagnostics.md) | The error and warning catalogue with codes | [#30](https://github.com/quakecstudio/editor/issues/30) |
| `SPR` | [`spr.md`](spr.md) | SPR sprites: header, frames and frame groups | [#235](https://github.com/quakecstudio/editor/issues/235) |
| `TRACE` | [`trace.md`](trace.md) | The shared VM trace format: per-statement records, binary and JSON forms, reading engine-specific values | [#36](https://github.com/quakecstudio/editor/issues/36) |
| `VM` | [`vm.md`](vm.md) | QC virtual machine execution: opcodes, calls, the local stack, errors | [#26](https://github.com/quakecstudio/editor/issues/26) |
| `WAD` | [`wad2.md`](wad2.md) | WAD2 archives: header, directory, lump lookup, the lumps the engine reads | [#29](https://github.com/quakecstudio/editor/issues/29) |
| `WAV` | [`wav.md`](wav.md) | WAV sounds: chunks, the header and loop points `GetWavinfo` reads, what `S_LoadSound` plays | [#235](https://github.com/quakecstudio/editor/issues/235) |

A new spec registers its prefix here in the PR that adds it.
