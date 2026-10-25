---
name: feedback-worktree-bash
description: [any] Worktree-isolated Bash guard rejects compound commands and paths containing "git"; keep commands plain and put scripts in files
metadata:
  type: feedback
---

[any] In a worktree-isolated session, the Bash guard refuses loops with variables, `cd` plus compound commands, heredoc python with redirects, and redirects into the scratchpad (its path contains "-git-"). Run plain single commands, write scripts to the scratchpad with Write and run `python3 <script> <args>`, and use `git -C <worktree>` for git.

**Why:** several commands were refused during the 2026-10-20 CONV-17 audit before this pattern worked.

**How to apply:** default to simple commands from the start in worktree sessions.
