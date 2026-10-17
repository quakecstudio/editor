---
inclusion: always
---

# Hard guardrails

The constraints that must hold in every session, inlined so Kiro always has them in
context. Each points at the authoritative rule in `.claude/` for the full detail. These are
distilled from `CLAUDE.md` ("What not to do") and `.claude/rules/`.

1. **No panics in shipped code.** No `unwrap`, `expect`, `panic!`, `unreachable`, `todo`,
   `unimplemented`, `exit`, `[]` indexing or string slicing, unchecked arithmetic
   (use `checked_*`, `wrapping_*` or `saturating_*`), integer division or modulo on
   untrusted values, `as` casts (use `try_from` or a spelled-out conversion), or `float_cmp`.
   `unsafe` is denied everywhere. Exceptions are `#[expect(lint, reason = "...")]` at the
   narrowest scope, never `#[allow]`, and in the VM and compiler the reason cites the spec
   statement. Write out what id's C does rather than panicking.
   See `.claude/rules/development-discipline.md` and `docs/conventions.md`.

2. **Never depend upward across layers.** A crate depends only on crates in its own layer or
   a lower one; `layers.toml` maps every crate and `cargo xtask layers` enforces it. Logic
   that is not rendering does not belong in the UI (L7); it belongs in a command (L5) or
   lower. See `.claude/rules/layering.md`.

3. **Specs are the source of truth; quote, never recall.** Never state what a specification
   requires, or what id's code does, from memory. Open it and quote it: the section and the
   verbatim text, from `specs/`, `docs/spec/`, id's source at the pinned commit, or a URL
   fetched this session. Report MUST, SHOULD and MAY as written, and say so when the source
   is silent. This applies to reviews, PR bodies and issue comments too.
   See `.claude/rules/specs-are-source-of-truth.md`.

4. **A port needs all four parts together.** Rust code with id's copyright line and a
   `Ported from <owner>/<repo>@<commit>: <path>, <Function>` header; a spec section in
   `docs/spec/`; tests that cite each statement (`// spec <ID>: ...`); and a differential
   result against the oracle. Port only from an upstream and commit listed in
   `docs/porting.md`. See `.claude/rules/porting.md`.

5. **Never weaken a check to get green.** No skipped, ignored or loosened test, lint, ratchet
   or baseline entry to make CI pass, and no new baseline entry for code you wrote. Fix the
   cause, or escalate. See `.claude/rules/testing.md` and
   `.claude/rules/development-discipline.md`.

6. **`needs:human` work is prepared, not merged.** Decisions, hardware, credentials, legal
   review and the MVP release belong to Justin. Prepare them (draft ADR, options, a
   recommendation, or a comment with exactly what is needed), label the issue `escalated`
   when waiting on a person, and stop. Never decide for Justin and never merge a
   `needs:human` PR. See `.claude/rules/commits-and-issues.md` and `docs/agents/README.md`.

7. **No em or en dashes.** Use plain hyphens in all docs, commit messages and PR and issue
   text. No emoji, and no "critical", "comprehensive" or "robust": a fix is a fix.
   See `.claude/rules/commits-and-issues.md`.

8. **No `Co-Authored-By` trailers**, from people or agents. This is the project owner's rule.
   See `.claude/rules/commits-and-issues.md`.

9. **Never commit Quake game data or unlicensed sources.** No `pak0.pak`/`pak1.pak`, no
   QuakeC 1.06 (which has no license), and never commit id's C: oracles fetch third-party
   sources by commit and checksum. See `CLAUDE.md` and `.claude/rules/porting.md`.

10. **Diagnostics are read-only, and three strikes then re-diagnose.** Inspect with read
    forms (`git config --get`, not a write), never diagnose with a command that writes, and
    after three identical failures stop retrying and re-check the diagnosis from scratch.
    Watch CI with `gh pr checks <pr> --watch --fail-fast` or `gh run watch <id> --exit-status`,
    never a long `sleep`. See `.claude/rules/development-discipline.md`.

11. **Never push to `main` or edit an issue body on GitHub.** `main` takes changes only
    through the merge queue; work on a `<type>/<issue>-<slug>` branch and open a PR with
    `Closes #<issue>`. Issue bodies are generated from `plan/`; change the plan with the
    `plan-change` skill, not the GitHub UI. See `.claude/rules/branching.md` and
    `.claude/rules/commits-and-issues.md`.

12. **Run session setup before any build, test, lint, plan or git work.** Kiro does not fire
    this repo's `SessionStart` hook (it lives in a subdirectory, not the workspace root), so
    at the start of each session run the authoritative setup script once, as your first
    action: `bash .claude/hooks/session-setup.sh` (idempotent, Linux-only, always exits 0).
    It installs rustup, `prek`, `uv` and the git hooks. Where system PyYAML is unavailable,
    run `plan/` with uv: `uv run --no-project --with pyyaml python3 plan/check.py`.
    See `.claude/hooks/session-setup.sh`, `AGENTS.md` ("Environment setup") and
    `.claude/settings.json`.
