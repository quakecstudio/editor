---
name: clean-room
description: The clean-room rules for QuakeC Studio - who may read id Software's GPL source, what a spec must contain, and how implementers work only from docs/spec/. Preloaded into the spec-author and implementer subagents.
user-invocable: false
---

# Clean room

The IDE core may be released under a permissive license only if it is not a derivative of id's GPL code. So the people (or agents) who read the GPL source write specs, and different people (or agents) implement from those specs. The full process is defined by #9 in `docs/clean-room.md`; until then, these rules apply.

## Roles

| Role | May read GPL source | Writes | Never |
| --- | --- | --- | --- |
| Spec author (`spec-author`, issues labelled `clean-room:spec` or `gpl-tools`) | Yes | `docs/spec/` prose; code in the GPL repos (`quakecstudio/oracles`, `quakecstudio/preview-host`) | Writes the clean-room implementation of anything they specified; pastes GPL code into a spec |
| Implementer (`implementer`, every other build issue) | No | Code in this repo | Opens id's engine or tools source, the oracles or preview host source, or a GPL port's source; asks a spec author for code |
| Reviewer (`reviewer`) | No | Tests, readiness and gate reports | Fixes the code it reviews |

## Writing a spec

- Describe behaviour in your own words: formats, field order and sizes, opcode semantics, edge cases, limits. Never copy code, comments or identifiers beyond the names the format itself exposes.
- Number every normative statement so tests can cite it (`docs/spec/vm.md#R12`).
- Put limits (call depth, stack sizes, runaway counts) in as named constants with a citation to the file and function you read them from.
- Give test vectors: inputs and expected outputs.
- Record every GPL file you read, with its commit, in `docs/clean-room/readers.md` under your role and issue.

## Implementing from a spec

- Work only from `docs/spec/`, the design document and public non-GPL references.
- The implementer subagent runs a guard hook that blocks fetching or reading GPL source. If the guard blocks you, you were about to look at something you must not; stop.
- If the spec is silent, ambiguous or wrong, stop and file a spec issue. Do not infer the answer from GPL code, from memory of it, or from an oracle's internals. You may run an oracle binary (downloaded from the oracles repo's releases into `target/oracle-bin/`) and compare your output to its output; that is what the differential tests do.
- Tests cite the spec section they pin.

## QuakeC game code is not engine code

The id 1.01 and rerelease QuakeC sources are the test corpus, not implementations we re-implement. Implementers may compile and run them. QuakeC 1.06 has no license: never bundle it.
