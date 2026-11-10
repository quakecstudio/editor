# qs-checkpoint

Checkpoints of a project's files (P4-05, AGT-4): each one a git commit whose tree holds every
file of the project, written as git's own loose objects (zlib streams named by their SHA-1) on
a private ref `refs/checkpoints/<n>`. The store is a bare repository of its own: in a git
working tree it is `.git/quakec-studio/checkpoints`, out of the project's history and its
garbage collection; elsewhere it is the hidden `.quakec-studio/checkpoints`. Git reads it as it
reads any repository (`git --git-dir <store> log refs/checkpoints/1`).

`Store::create` records every regular file under the root but `.git`, `.quakec-studio` and
`build/`; `Store::restore` writes back every file a checkpoint holds, byte for byte, removes
every file it does not hold and the folders that leaves empty, so the project is exactly as it
was. `Store::list` lists the checkpoints with their labels.

The model is Cursor's and Zed's agent checkpoints, kept the way `git stash` keeps its commits:
as ordinary git objects on a ref outside the branches.

- **Layer:** L0, beside the other file formats: git's object format, over `sha1_smol` and
  `miniz_oxide`. The commands are `qs-workbench`'s `checkpoint.*`.
- **Spec sections:** none; git's object format is git's, not the engine's.
- **Shipped:** yes.

## Invariants

Tests in `src/tests.rs`:

- For any tree of files checkpointed and any tree after it, a restore gives the first back
  exactly (a property test).
- `git fsck --strict` passes on the store, and `git ls-tree` and `git log` read its
  checkpoints, each with the one before as its parent.
