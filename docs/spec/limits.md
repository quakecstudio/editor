# Format and engine limits

Prefix: `LIMIT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Every fixed limit of the progs.dat format, id's VM and server, and id's qcc, as a named
constant with the file and identifier it comes from, and what happens at the limit. Sources:
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d` and the `qcc/` directory of
[id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools) at commit
`c0d1b91c74eb654365ac7755bc837e497caaca73`.

Code uses these names, never the bare numbers. Implemented by `qc-progs` (LIMIT-1), `qc-vm`
(LIMIT-2 to LIMIT-4), `qc-edict` and `qc-builtins` (LIMIT-5 and LIMIT-6) and our compiler
(LIMIT-7). Every statement is in [`coverage-baseline.tsv`](coverage-baseline.tsv) until
those crates cite it. Constants that describe the format's structure rather than a limit
(header size, record sizes, reserved slots) are in [`progs.md`](progs.md).

## LIMIT-1 progs.dat format limits

**Source:** `WinQuake/pr_comp.h`, `dstatement_t`, `ddef_t`, `dfunction_t`, `MAX_PARMS`,
`DEF_SAVEGLOBAL`; `WinQuake/pr_exec.c`, `PR_EnterFunction`.

### Behaviour

- **LIMIT-1.1** A version 6 statement operand is a signed 16-bit value, so a statement can
  name global slots 0 to 32767 only (PROGS-7.2); a program with more globals cannot reach the
  rest from code.
- **LIMIT-1.2** A definition's `ofs` is unsigned 16-bit, so a definition can name global or
  field slots 0 to 65535 (PROGS-9.2).
- **LIMIT-1.3** A definition's type has 15 bits once `DEF_SAVEGLOBAL` is removed: 0 to 32767.
- **LIMIT-1.4** A function has at most 8 parameters (`parm_size` has 8 entries and the call
  opcodes are `OP_CALL0` to `OP_CALL8`). Each parameter has 3 slots in the call area
  (PROGS-18.1), and qcc writes a `parm_size` of 1 or 3; the engine copies whatever
  `parm_size` says without checking it.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_MAX_OPERAND_SLOT` | 32767 | `WinQuake/pr_comp.h`, `dstatement_t` (`short a,b,c`) |
| `LIMIT_MAX_DEF_SLOT` | 65535 | `WinQuake/pr_comp.h`, `ddef_t` (`unsigned short ofs`) |
| `LIMIT_MAX_DEF_TYPE` | 32767 | `WinQuake/pr_comp.h`, `ddef_t` (`unsigned short type`) less `DEF_SAVEGLOBAL` |
| `LIMIT_MAX_PARMS` | 8 | `WinQuake/pr_comp.h`, `MAX_PARMS` |

## LIMIT-2 QC call depth

**Source:** `WinQuake/pr_exec.c`, `MAX_STACK_DEPTH`, `PR_EnterFunction`,
`PR_ExecuteProgram`.

### Behaviour

- **LIMIT-2.1** Entering a QC function adds one to the call depth, and the entry that makes
  the depth reach 32 fails with `stack overflow`. The function the engine calls counts, so at
  most 31 QC functions can be active at once.
- **LIMIT-2.2** The depth is shared by every QC function active in the engine, including those
  started by a nested `PR_ExecuteProgram` (for example a touch function run from inside a
  builtin).
- **LIMIT-2.3** Builtins do not count: calling a builtin does not enter a QC frame.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_MAX_STACK_DEPTH` | 32 | `WinQuake/pr_exec.c`, `MAX_STACK_DEPTH` |

## LIMIT-3 Local stack

**Source:** `WinQuake/pr_exec.c`, `LOCALSTACK_SIZE`, `PR_EnterFunction`.

### Behaviour

- **LIMIT-3.1** Entering a QC function saves its `locals` slots on a stack of 2048 slots shared
  by all active functions. An entry is allowed when the slots in use plus the new function's
  `locals` is at most 2048, and fails with `PR_ExecuteProgram: locals stack overflow` when it
  is more.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_LOCALSTACK_SIZE` | 2048 | `WinQuake/pr_exec.c`, `LOCALSTACK_SIZE` |

## LIMIT-4 Runaway loop guard

**Source:** `WinQuake/pr_exec.c`, `PR_ExecuteProgram`.

### Behaviour

- **LIMIT-4.1** Each call of `PR_ExecuteProgram` has its own counter starting at 100000. The
  counter drops by one before each statement, and the statement that brings it to 0 fails with
  `runaway loop error` without executing, so one call executes at most 99999 statements.
- **LIMIT-4.2** Statements run by a nested `PR_ExecuteProgram` count against the nested call's
  own counter, not the outer one.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_RUNAWAY` | 100000 | `WinQuake/pr_exec.c`, `runaway = 100000;` in `PR_ExecuteProgram` |

## LIMIT-5 Builtin numbers

**Source:** `WinQuake/pr_cmds.c`, `pr_builtin`, `pr_numbuiltins`; `WinQuake/pr_exec.c`,
`PR_ExecuteProgram`.

### Behaviour

- **LIMIT-5.1** id's builtin table has 79 entries, numbers 0 to 78. Calling a function whose
  builtin number is 79 or more fails with `Bad builtin call number`.
- **LIMIT-5.2** Builtin 0 cannot be called: a `first_statement` of 0 is not negative, so the
  function runs statement 0 instead (PROGS-10.2).
- **LIMIT-5.3** A `first_statement` of -2147483648 has no positive builtin number; negating it
  overflows, which is undefined in C. This spec fixes the answer as `Bad builtin call number`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_NUM_BUILTINS` | 79 | `WinQuake/pr_cmds.c`, `pr_numbuiltins = sizeof(pr_builtin)/sizeof(pr_builtin[0])`, counted without `QUAKE2` |

## LIMIT-6 Server limits QuakeC can reach

**Source:** `WinQuake/quakedef.h`; `WinQuake/pr_edict.c`, `ED_Alloc`; `WinQuake/pr_cmds.c`,
`PF_precache_sound`, `PF_precache_model`; `WinQuake/sv_main.c`, `SV_SpawnServer`.

### Behaviour

- **LIMIT-6.1** A server has at most 600 edicts, counting the world and the client edicts.
  When none can be reused and 600 are in use, allocating one (`spawn`, or an entity of the
  map being loaded) fails with `ED_Alloc: no free edicts`.
  Reuse rules are in `edicts.md` (P1-03).
- **LIMIT-6.2** The sound and model precache lists have 256 entries each, and entry 0 is
  always the empty string. Precaching a name already in the list does nothing; precaching a new
  name when all 256 entries are used fails with `PF_precache_sound: overflow` or
  `PF_precache_model: overflow`. So at most 255 sounds can be precached, and at most 255 models
  including the world model in entry 1 and its submodels after it.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_MAX_EDICTS` | 600 | `WinQuake/quakedef.h`, `MAX_EDICTS` |
| `LIMIT_MAX_MODELS` | 256 | `WinQuake/quakedef.h`, `MAX_MODELS` |
| `LIMIT_MAX_SOUNDS` | 256 | `WinQuake/quakedef.h`, `MAX_SOUNDS` |
| `LIMIT_MAX_LIGHTSTYLES` | 64 | `WinQuake/quakedef.h`, `MAX_LIGHTSTYLES`; `PF_lightstyle` does not check it (P1-04) |
| `LIMIT_MAX_QPATH` | 64 | `WinQuake/quakedef.h`, `MAX_QPATH`: the longest game path, NUL included |
| `LIMIT_MAX_MSGLEN` | 8000 | `WinQuake/quakedef.h`, `MAX_MSGLEN`: "max length of a reliable message"; which buffer each `Write*` builtin fills is in P1-04 |
| `LIMIT_MAX_DATAGRAM` | 1024 | `WinQuake/quakedef.h`, `MAX_DATAGRAM`: "max length of unreliable message" (P1-04) |
| `LIMIT_MAX_ENT_LEAFS` | 16 | `WinQuake/progs.h`, `MAX_ENT_LEAFS`: BSP leaves an edict is linked into (P1-03) |

## LIMIT-7 id qcc limits

**Source:** `qcc/qcc.h`, `MAX_STRINGS` to `MAX_DATA_PATH`, `MAX_NAME`, `MAX_REGS`,
`MAX_ERRORS`; `qcc/pr_lex.c`, `MAX_FRAMES`, `PR_ParseName`; `qcc/pr_comp.c`, `PrecacheSound`,
`PrecacheModel`, `PrecacheFile`; `qcc/qcc.c`.

These are limits of id's compiler, not of the format: a file within the format's limits may
exceed them. Our compiler is not bound by them; they are recorded so its diagnostics can say
when a program would not build with id's qcc.

### Behaviour

- **LIMIT-7.1** qcc never checks its table sizes: more than 500000 bytes of strings, 65536
  statements, 8192 functions, 16384 global definitions, 1024 field definitions or 16384 global
  slots overruns a fixed array, which is undefined in C. The result is not specified; a
  program that exceeds one does not build reliably with id's qcc.
- **LIMIT-7.2** A name of 63 or more characters fails with `name too long`, so a name has at
  most 62 characters.
- **LIMIT-7.3** qcc records the files named in `precache_sound`, `precache_model` and
  `precache_file` calls, 1024 of each. A new sound when 1024 are recorded fails with
  `PrecacheSound: numsounds == MAX_SOUNDS`, and a new file when 1024 are recorded with
  `PrecacheFile: numfiles == MAX_FILES`. For models qcc tests the sound count instead
  (`PrecacheModels: numsounds == MAX_SOUNDS`), so a new model fails only when 1024 sounds are
  recorded, and the 1025th model overruns its array.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIMIT_QCC_MAX_STRINGS` | 500000 | `qcc/qcc.h`, `MAX_STRINGS`: bytes in the string table |
| `LIMIT_QCC_MAX_STATEMENTS` | 65536 | `qcc/qcc.h`, `MAX_STATEMENTS` |
| `LIMIT_QCC_MAX_FUNCTIONS` | 8192 | `qcc/qcc.h`, `MAX_FUNCTIONS` |
| `LIMIT_QCC_MAX_GLOBALS` | 16384 | `qcc/qcc.h`, `MAX_GLOBALS`: global definitions |
| `LIMIT_QCC_MAX_FIELDS` | 1024 | `qcc/qcc.h`, `MAX_FIELDS`: field definitions |
| `LIMIT_QCC_MAX_REGS` | 16384 | `qcc/qcc.h`, `MAX_REGS`: global slots |
| `LIMIT_QCC_MAX_NAME` | 64 | `qcc/qcc.h`, `MAX_NAME`, checked by `PR_ParseName` as `strlen >= MAX_NAME-1` |
| `LIMIT_QCC_MAX_PRECACHE` | 1024 | `qcc/qcc.h`, `MAX_SOUNDS`, `MAX_MODELS`, `MAX_FILES` |
| `LIMIT_QCC_MAX_DATA_PATH` | 64 | `qcc/qcc.h`, `MAX_DATA_PATH`: a recorded precache name, unchecked |
| `LIMIT_QCC_MAX_FRAMES` | 256 | `qcc/pr_lex.c`, `MAX_FRAMES`: `$frame` names, unchecked, 16 bytes each |
| `LIMIT_QCC_MAX_ERRORS` | 10 | `qcc/qcc.h`, `MAX_ERRORS`: compilation stops at the error that takes the count past it |

## Test vectors

Checked on 2026-10-16 by reading the cited code at the pinned commits; the VM limits run
against the id VM oracle once P1-11 lands.

| Case | Result | Statements |
| --- | --- | --- |
| Statement operand bytes `ff 7f` / `00 80` | slot 32767 / -32768 | LIMIT-1.1 |
| QC recursion from one engine call, depth 31 / 32 | runs / `stack overflow` | LIMIT-2.1 |
| Entering a function with `locals` 2048 / 2049 on an empty local stack | runs / `PR_ExecuteProgram: locals stack overflow` | LIMIT-3.1 |
| An infinite `while (1)` loop of one `OP_GOTO` | `runaway loop error` on the 100000th statement | LIMIT-4.1 |
| `first_statement` -78 / -79 | builtin 78 (`setspawnparms`) / `Bad builtin call number` | LIMIT-5.1 |
| `first_statement` -2147483648 | `Bad builtin call number` | LIMIT-5.3 |
| A name of 62 / 63 characters in qcc | accepted / `name too long` | LIMIT-7.2 |
