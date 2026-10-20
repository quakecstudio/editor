# Commits and issues

Read by every agent before it commits, opens a PR or files an issue.

## Commit messages

[Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/#specification):

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

1. Every commit has a type, then a colon and a space.
2. The description is imperative and present tense, has no trailing period and is at most
   72 characters.
3. The body explains why, not what; it is separated from the description by a blank line.
4. A breaking change is marked with `!` after the type or scope, or a `BREAKING CHANGE:`
   footer.
5. **No `Co-Authored-By` trailers**, from people or agents, and no "Generated with" footer
   or any other attribution in a commit message or a PR body: PRs are squash-merged, so the
   body becomes the commit (project owner's rule; differs from vouch).
6. Plain hyphens, never em or en dashes. No emoji. No "critical", "comprehensive" or
   "robust": a fix is a fix.

PRs are squash-merged, so the PR title is the commit on `main`: it follows the same rules.

| Type | Use for |
| --- | --- |
| `feat` | A new capability |
| `fix` | A bug fix |
| `docs` | Documentation only: `docs/`, READMEs, `CLAUDE.md`, ADRs, specs in `docs/spec/` |
| `test` | Adding or correcting tests, test harnesses, corpora |
| `refactor` | Restructuring without a change in behaviour |
| `perf` | A performance improvement |
| `build` | Cargo, the Makefile, dependencies |
| `ci` | GitHub Actions |
| `chore` | Repository and process setup |
| `style` | Formatting only |

Scopes are optional; use a crate without its `qs-` prefix (`vm`, `progs`, `paths`), or one
of `xtask`, `plan`, `adr`, `spec`, `agents`, `deps`.

## Pull requests

- The body says what the code does now and how each acceptance criterion is met, then
  `Closes #<issue>`. It describes only what is in the diff, not earlier attempts.
- A port lists `Spec:` IDs and the upstream commit ([`porting.md`](porting.md)).
- The checklist in `.github/pull_request_template.md` is the definition of done; tick what
  is met and say why any item does not apply.

## Issues

Work is planned in [`plan/`](../../plan/README.md), and GitHub issues are generated from it.

- **Never edit an issue body on GitHub**: `plan/apply.py` overwrites it. To add or change an
  issue, use `/plan-change`: edit `plan/backlog/*.yaml`, run the checks and sync after the
  merge.
- Every issue has one `type:*` label, its layer and area labels, a size, the requirement IDs
  it serves, its dependencies, a deliverable with paths, acceptance criteria a test or
  recorded result can prove, and verification.
- Follow-up work found while doing an issue becomes a new issue, linked from the PR. It is
  never left in a comment.
- A bug found by hand, outside the plan, is filed with `.github/ISSUE_TEMPLATE/bug.yml`
  and then added to `plan/` so the next sync keeps it.
- Titles describe the outcome, in sentence case with no trailing period:
  `Port the progs.dat loader`, `Lexer drops a trailing CR in string literals`.
- Security problems never go in a public issue; follow [`SECURITY.md`](../../SECURITY.md).

## Labels

From `plan/backlog/_meta.yaml`, created by `plan/apply.py`:

| Kind | Labels |
| --- | --- |
| Type (one) | `type:feature`, `type:spec`, `type:decision`, `type:spike`, `type:test`, `type:chore`, `type:gate`, `type:readiness` |
| Layer | `L0-formats` to `L7-ui`, `apps`, `c-tools`, `repo` |
| Area | `area:compiler`, `area:vm`, `area:language`, `area:preview`, `area:agent`, `area:editor`, `area:map`, `area:release`, `area:docs`, `area:legal`, `area:process` |
| Size | `size:S` (up to 2 days), `size:M` (up to a week), `size:L` (up to two weeks) |
| Owner | `needs:human` (a person must act), `escalated` (an agent is waiting on a person; see the latest comment) |
