# qs-git

Git in the window (P4-35, UI-19): a project's status, the hunks of each change to stage and
unstage, commits, branches, a file's history and blame, its text at `HEAD`, and its merge
conflicts resolved one side at a time.

- **Layer:** L0, beside `qs-checkpoint`. It has no dependency; it runs the `git` program.
- **Decision:** plan requirement UI-19, P4-35 (#812); not engine behaviour.
- **Spec sections:** none; git's formats and command line are git's.
- **Shipped:** yes.

## Prior art

The model is **Zed's git panel and VS Code's Source Control view**: every operation is the
git command line's, read through its machine-readable output, so the window shows what `git
status` says. A hunk is staged as `git add -p` stages one, its patch applied to the index
(`git apply --cached`), and unstaged in reverse. Conflicts are git's markers in the file
(`git help merge`, "How conflicts are presented"), kept to one side or both as VS Code's merge
editor offers.

## Public shape

- `Repo::discover` finds the repository holding a folder; `status` (`git status --porcelain`),
  `diff` (`FileDiff`, its `Hunk`s, unstaged or staged), `stage_hunk`, `unstage_hunk`, `stage`,
  `unstage`, `commit`, `branches`, `switch`, `create_branch`, `log` (following renames),
  `blame` (`git blame --porcelain`) and `head_text`.
- `conflicts` and `resolve`: the markers in a text, with and without the `diff3` base, and a
  conflict replaced by `Side::Ours`, `Theirs` or `Both`.
- Git runs in the C locale with no pager, colour, prompt or quoted paths.

## Tests

`src/tests.rs` runs git in temporary repositories: staging a hunk, committing and switching
branches leave `git status --porcelain` as `Repo::status` says (the issue's acceptance
criterion); unstaging a hunk or a file, in a repository with and without a commit; history,
blame and the text at `HEAD`; a merge conflict resolved and committed; the conflict markers,
with a property test that resolving keeps every other line; and the diff and blame parsers.
