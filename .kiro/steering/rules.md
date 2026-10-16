---
inclusion: always
---

# Project rules

The seven rules every agent follows. Each is authored once in `.claude/rules/`; the full
text lives there and must be read before the work it governs. The hard constraints among
them are also inlined in `.kiro/steering/guardrails.md`.

## Branching

How work gets from a branch to `main`: never push to `main`, branch as
`<type>/<issue>-<slug>` from an up-to-date `main`, one writer per working tree, open a PR
and run the CI checks before it. Read `.claude/rules/branching.md` before creating a branch
or a PR.

## Commits and issues

Conventional Commits (imperative, at most 72 characters, no trailing period), no
`Co-Authored-By` trailers, plain hyphens only. PRs say how each acceptance criterion is met
and end with `Closes #<issue>`. Never edit an issue body on GitHub; change `plan/` instead.
Read `.claude/rules/commits-and-issues.md` before committing, opening a PR or filing an
issue.

## Development discipline

Design, implementation and diagnostic discipline: decisions belong to a person, lead with
prior art, the issue's deliverable is the contract, scripted edits must prove they landed,
completion needs evidence, diagnostics are read-only, three strikes then re-diagnose, watch
CI instead of sleeping, one writer per working tree. Read
`.claude/rules/development-discipline.md` before design, implementation or review work.

## Layering

A crate may depend only on crates in its own layer or a lower one (L0 formats through L7 UI,
plus apps); `layers.toml` and `cargo xtask layers` enforce it. The UI is a thin top layer
over L5 commands. Read `.claude/rules/layering.md` before adding a crate or a cross-crate
dependency.

## Porting from id's source

A port is done only when four things exist together: the Rust code with id's copyright and a
`Ported from` header, a spec section in `docs/spec/`, tests citing each statement, and a
differential result against the oracle. Port only from an upstream and commit listed in
`docs/porting.md`. Read `.claude/rules/porting.md` before porting any of id's code.

## Specs are the source of truth

Never state what a specification requires, or what id's code does, from memory; open it and
quote it, with the section and the verbatim text, and report MUST, SHOULD and MAY as
written. Applies to code, reviews, PR bodies and issue comments. Read
`.claude/rules/specs-are-source-of-truth.md` before implementing, reviewing or describing
any specified behaviour.

## Testing

Where tests live (in the crate, or `crates/qs-tests/tests/it/` for cross-crate), how they
are written (cite the spec statement, factories for fixtures, no wall-clock waits, edges and
errors, property and differential tests), and the readiness bar a layer passes before work
builds on it. Read `.claude/rules/testing.md` before writing tests or building on a lower
layer.
