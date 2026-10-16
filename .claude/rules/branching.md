# Branching

How work gets from a branch to `main`. Read by every agent before it creates a branch or a PR.

## Branch names

Branch names carry the Conventional Commit type of the work and the issue number
([`commits-and-issues.md`](commits-and-issues.md)):

- `feat/{issue}-{slug}`, `fix/{issue}-{slug}`, and `{type}/{issue}-{slug}` for the other types.
- Examples: `feat/45-qc-types`, `fix/88-lexer-crlf`, `docs/9-porting-process`.
- Every piece of work has an issue (`plan/`), so every branch has a number. Work the plan does
  not cover starts with `/plan-change`, which creates the issue.

## Workflow

- **Never push to `main`.** It accepts changes only through the merge queue, which
  squash-merges; the PR title becomes the commit subject on `main`.
- Branch from an up-to-date `main`: `git switch main && git pull --ff-only`, then
  `git switch -c <branch>`.
- **One writer per working tree.** Agents working in parallel each use their own worktree
  (`wt switch <branch>`, or `isolation: "worktree"` for subagents). Before handing in-flight
  work to another agent, stop the first one and save its diff
  ([`development-discipline.md`](development-discipline.md)).
- Never amend or rebase commits that are already pushed to a shared branch.
- An agent may add its own PR to the merge queue once CI passes
  (`.claude/skills/work-issue/scripts/land.sh`), except for `needs:human` issues, which wait
  for Justin.

## Before opening a PR

Run what CI runs, from the repository root:

```bash
make ci        # cargo xtask ci: fmt, clippy -D warnings, tests, layering, licenses, conventions, ...
prek run       # the pre-commit hooks
python3 plan/check.py --write-coverage && (cd plan && python3 -m unittest test_check.py test_status.py)
```

Until `make ci` exists ([#2](https://github.com/quakecstudio/editor/issues/2)), run the
commands it will wrap: `cargo fmt --all --check`,
`cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo test --locked`,
`cargo xtask conventions`, `cargo xtask adr`, and the `plan/` checks above.

Then check the PR against the gates CI cannot see:

- **Every acceptance criterion** of the issue is met by a test or a recorded result, and the
  PR body says which.
- **Spec claims are quoted, not remembered.** Every "the LSP spec requires" or "id's VM does"
  in the diff or the PR body has its source
  ([`specs-are-source-of-truth.md`](specs-are-source-of-truth.md)).
- **Ported code** follows [`porting.md`](porting.md).
- **The plan stays true.** If the work showed the plan is wrong or incomplete, the PR changes
  `plan/` or the follow-up issue is filed and linked.
- **Docs follow the code.** A changed command, layout or convention updates `CLAUDE.md`,
  `docs/` or the crate README in the same PR.
