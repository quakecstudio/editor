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
  is cited by a line starting `// spec <ID>` in a Rust file under `crates/` or `apps/`, or
  is listed in [`coverage-baseline.tsv`](coverage-baseline.tsv).
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
   oracles once [#35](https://github.com/quakecstudio/editor/issues/35) lands). Short vectors go in a table in the
   section; larger ones go in `docs/spec/vectors/<spec>/` and the section links them.

## Registry

| Prefix | File | What it covers | Issue |
| --- | --- | --- | --- |
| `EXAMPLE` | [`example.md`](example.md) | The worked porting example: entity-string escapes | [#9](https://github.com/quakecstudio/editor/issues/9) |

Specs planned in Phase 1 (`progs.md`, `limits.md`, `vm.md`, `edicts.md`, `builtins.md`,
`trace.md`, `differential.md`) register their prefix here when they are written.
