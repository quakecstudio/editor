# qc-edict

The server's edict store: the entities the engine and QuakeC share. An edict is a fixed
engine header followed by the progs' field slots, and the engine keeps all of them in one
array that QuakeC addresses by byte offset. This crate lays that array out for a loaded
`progs.dat`, stores and clears field slots, allocates and frees edicts by id's reuse rules,
and names the system globals and fields the engine reads and writes at fixed slots.

- `EdictLayout`: the edict size from the field count (EDICT-1.1) on the 32-bit header size
  (EDICT-1.3), the entity value of an edict (EDICT-1.5), the pointer value of a field slot
  (EDICT-1.6), the number an entity value names (EDICT-1.9), and the refusal of a progs that
  cannot hold the system globals or fields (EDICT-2.4, EDICT-3.3) or whose field count would
  overflow an entity value (EDICT-3.5).
- `EdictStore`: the array of 600 edicts with `num_edicts` (EDICT-1.4, EDICT-1.8), field access
  by offset, `EDICT_NUM` and `NUM_FOR_EDICT` (EDICT-1.7), allocation (`spawn`, EDICT-6) and
  freeing (`remove`, EDICT-7) with id's spec-exact reuse rules.
- `system`: the fixed tables of the 64 system globals (EDICT-2.1) and 105 system fields
  (EDICT-3.1), with lookup by slot and by name.

The physics that moves entities, the map spawn loop, the engine entry points and which engine
code writes each system global and field (EDICT-4, EDICT-5, EDICT-8 to EDICT-13) are the
preview host's simulation (`qs-sim`, P3-24), built on this store; this crate describes only
the memory and the allocation rules.

- **Layer:** L1 VM. It depends on `qc-progs` and `qc-types` (both L0).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`:
  `WinQuake/pr_edict.c` (`ED_Alloc`, `ED_Free`, `ED_ClearEdict`, `EDICT_NUM`, `NUM_FOR_EDICT`,
  `PR_LoadProgs`), `WinQuake/progs.h` (`EDICT_TO_PROG`, `PROG_TO_EDICT`, `NEXT_EDICT`),
  `WinQuake/progdefs.q1` (`globalvars_t`, `entvars_t`), `WinQuake/sv_main.c`
  (`SV_SpawnServer`).
- **Spec sections:** [EDICT-1 to EDICT-3, EDICT-6, EDICT-7](../../docs/spec/edicts.md)
  (entity memory and numbering, system globals, system fields, allocation and freeing). The
  statements about the running server (EDICT-4, EDICT-5, EDICT-8 to EDICT-13) are implemented
  by the preview host and stay in the coverage baseline until it cites them.
- **Shipped:** yes. No panics: every index, conversion and arithmetic is checked, and id's
  behaviour on an out-of-range value is written out as the spec fixes it.

## Differential result

The spawn, remove and reuse rules are checked against id's `ED_Alloc`, `ED_Free`,
`ED_ClearEdict`, `EDICT_NUM` and `NUM_FOR_EDICT` through the VM oracle, which runs the
17 vectors of [`docs/spec/vectors/vm/edicts.txt`](../../docs/spec/vectors/vm/edicts.txt) on
every CI run once `qc-vm` (P1-10) drives this store. Until then the unit tests here pin each
statement to the recorded rows of `edicts.md` "Allocation and freeing" and "Layout".
