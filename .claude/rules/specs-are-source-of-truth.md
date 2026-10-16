# Specifications are the source of truth

Read before implementing, reviewing or describing any behaviour that a specification or id's
code defines. QuakeC Studio has two kinds of specification, and the rule is the same for both.

## The rule

**Never state what a specification requires, or what id's code does, from memory. Open it
and quote it.**

A model reproduces the shape of a spec sentence reliably and its content unreliably. The
failure is not "I don't know"; it is a fluent, plausible sentence that is not in the
document, and at review time it looks exactly like a real citation. Quake adds a second trap:
forums, wikis and other engines describe "how QuakeC works" with confidence, and they
disagree with id's code and with each other.

This applies to reviews, PR bodies and issue comments as much as to code. "This breaks the
LSP spec" and "id's VM does not do that" are claims that need evidence.

## The two kinds

| What | Where | Source of truth |
| --- | --- | --- |
| Engine and format behaviour: progs.dat, the VM, builtins, BSP, WAD2, PAK, MDL, `.map` | [`docs/spec/`](../../docs/spec/README.md) | id's source at the pinned commit in [`docs/porting.md`](../../docs/porting.md) |
| Public protocols and formats we implement: LSP, MCP, ACP, and later JSON Schema, Steam's library files and similar | [`specs/`](../../specs/README.md) | The published specification, cached verbatim |

## Citing engine behaviour

- The statement is in `docs/spec/` with a stable ID, written with id's C open, citing the
  file, function and commit. If it is not there yet, write it there first.
- Code names the section it implements (`Implements VM-12`); tests cite the statement
  (`// spec VM-12.3: ...`); the ratchet in `qs-tests` holds every statement to a test.
- When sources disagree, [`docs/porting.md`](../../docs/porting.md#which-source-is-authoritative)
  decides: the engine over the compiler, WinQuake over QuakeWorld, the pinned commit over
  memory and other ports.
- What id's C does on the original x86 build is the answer for undefined behaviour, stated
  in the spec with its reason, not guessed in the code.

## Citing a public specification

A claim about required behaviour needs all three:

1. The document and section: "LSP 3.18, `textDocument/rename`", not "the LSP spec".
2. A **verbatim quote** of the sentence that carries the requirement.
3. A path under `specs/` (whose `manifest.tsv` row gives the origin and sha256), or the URL it
   was fetched from in this session.

`specs/` is byte-identical to the origins, so grep it before fetching from the web; it is
faster and it does not paraphrase. If a specification we implement is missing, add it to
`specs/manifest.tsv` and run `scripts/refresh-specs.sh` in the same PR.

## Verify the quote

Summarisers invent normative sentences. When a quote decides a behaviour or settles a
review, find the exact phrase again in the cached file or the fetched page. If it cannot be
found again, it is not real.

## Report the real strength

MUST, SHOULD and MAY are not interchangeable. Downgrading a MUST hides a defect; upgrading a
MAY invents work. Say which one the text uses. For id's code the equivalent is the
difference between what the code always does and what one build happened to do.

## Silence is an answer, not permission

If the specification or id's code does not cover a case, say so. The decision is then ours,
argued on its merits (compatibility with the corpus, least surprise) and recorded in the
spec statement or an ADR, not presented as conformance.

## When the source contradicts an earlier decision

The source wins, including over a merged PR, an ADR or a spec statement. Raise the
contradiction with the quote; a spec statement that turns out wrong is withdrawn and
replaced (`docs/spec/README.md`), never silently edited.
