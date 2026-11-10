# qs-diff

Line diffs. `script` is the shortest edit script from one list of lines to another (Myers'
O(ND) algorithm, as git uses), giving up on a minimal script past `MAX_EDITS` edits so a
caller never waits on a huge diff. `hunks` groups a script into runs of changed lines.

The editor's inline diff (`qs-editor-view`) and the staged edits an agent proposes
(`qs-workbench`, P4-04) both read their changes from it, so a hunk the reviewer sees is the
same hunk the editor shows.

- **Layer:** L0, with no dependencies: it knows nothing of files, text encodings or the UI.
  The lines are any `PartialEq` items.
- **Spec sections:** none; a line diff is not engine or format behaviour.
- **Shipped:** yes.

## Invariants

Property tests in `src/tests.rs`:

- The script turns the first list into the second, and its equal steps are a longest common
  subsequence, so no script is shorter (within `MAX_EDITS`).
- The hunks are in order, never empty on both sides and never touching, and putting each
  hunk's new lines in place of its old lines gives the second list.
