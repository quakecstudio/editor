# Development discipline

Read before design, implementation or review work. Adapted from vouch's rules; each one
came from a session that went wrong without it.

## Design

1. **Decisions belong to a person.** A choice between product or workflow models (what is
   automatic and what the modder confirms, what the agent may do unasked) goes to Justin
   with every viable option, including the plain manual one, as a `type:decision` issue and a
   draft ADR (`/prepare-decision`). Recommend and rank; never quietly drop an option.
2. **Lead with prior art.** Before designing something new, say how established tools solve
   it (id's tools, FTEQCC, TrenchBroom, ericw-tools, Zed, rust-analyzer, Roblox Studio) and
   name the model being followed.
3. **Show the concrete thing.** A UI proposal shows the screen, the labels and the exact
   text, matched to the mockups in `docs/design/`. If a question has to be asked twice,
   draw it.
4. **State the cost first.** Before starting, give the expected size and the slimmest
   alternative that still meets the issue.

## Implementation

5. **The issue's deliverable is the contract.** Build exactly what it names, at the paths it
   names. If it is wrong, change the plan (`/plan-change`) rather than delivering something
   else.
6. **Fix the class, not the instance.** When a bug shows an invariant ("every opcode that
   writes a global must ..."), find every code path that touches it and fix them all in one
   change, with a test for each.
7. **Scripted edits must prove they landed.** A `sed` or `str.replace` that misses its anchor
   changes nothing and reports success. After a scripted edit, search for the new text and
   for the absence of the old. Prefer edit tools that fail on a missed match.
8. **Completion needs evidence.** "Done, tests pass" counts only with the commands run and
   their results. Before reporting, check each claim against output from this session; say
   plainly what was not run.
9. **Never weaken a check to get green.** No skipped, ignored or loosened test, lint, ratchet
   or baseline entry to make CI pass. Fix the cause, or escalate.

## Diagnostics and retries

10. **Diagnostics are read-only.** Inspect with read forms: `git config --get`, not
    `git config <key> <value>`, which writes. The same goes for any tool where read and
    write share a verb.
11. **Three strikes, then re-diagnose.** If a command fails three times, stop retrying and
    check the diagnosis again from scratch; the cause can change while you retry.

## Agents working together

12. **Judge progress by evidence, not silence.** Before calling another agent stuck, look
    at the files: modification times, new commits, `target/` activity.
13. **One writer per working tree.** Parallel agents use separate worktrees. To hand work
    over, stop the first agent and save its diff before the next one starts.
