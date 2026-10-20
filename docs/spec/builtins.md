# Vanilla builtins

Prefix: `BUILTIN` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes every builtin of id's engine, numbers 1 to 78: its QuakeC prototype, how it reads its
arguments and writes its result, what it changes in the edicts, the globals and the engine, how
it fails, and whether it can be mocked faithfully without an engine. Source:
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, the WinQuake tree built without `QUAKE2`, `GLQUAKE`
or `IDGODS`: `WinQuake/pr_cmds.c` (`pr_builtin` and the `PF_` functions) with the functions they
call in `sv_move.c`, `sv_main.c`, `world.c`, `mathlib.c`, `common.c`, `cvar.c`, `cmd.c`,
`host.c`, `console.c`, `model.c` and `pr_edict.c`. The prototypes are id's 1.01 QuakeC,
`qcc/v101qc/defs.qc` in [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)
at commit `c0d1b91c74eb654365ac7755bc837e497caaca73`, cross-checked against the 2021 rerelease's
`quakec/defs.qc` (BUILTIN-17).

Implemented by `qc-builtins` (P1-16); `differential.md` (P1-27) defines
the deterministic mock behaviour both VMs share. This spec does not repeat what other specs
state:

- how a builtin is called, the argument count and the table bound: [`vm.md`](vm.md) VM-14,
  [`limits.md`](limits.md) LIMIT-5;
- what a runtime error does: VM-19;
- the parameter and return slots: [`progs.md`](progs.md) PROGS-18; value encodings: PROGS-17;
- edict allocation, freeing and the system globals and fields, with who writes each:
  [`edicts.md`](edicts.md) EDICT-6, EDICT-7, EDICT-11, EDICT-12;
- precache and edict limits: LIMIT-6.

The collision, movement and visibility code these builtins call (`SV_Move`, `SV_movestep`,
`SV_CheckBottom`, `SV_LinkEdict`, the PVS) belongs to the server simulation (`qs-sim`, P3-24)
and its engine spec (P3); this spec states what each builtin asks of it and what it writes
back. Every statement in this file is in [`coverage-baseline.tsv`](coverage-baseline.tsv)
until `qc-builtins` cites it.

Notation: `PARM0` to `PARM7` are the parameter slots `OFS_PARM0` to `OFS_PARM7`; `RETURN` is
`OFS_RETURN`. "The float in PARM1" reads slot 7 as an f32 (PROGS-17.1). `self` is the edict the
global `self` names. A cited C function is in `WinQuake/pr_cmds.c` unless the statement names
another file.

## BUILTIN-1 The builtin table

**Source:** `WinQuake/pr_cmds.c`, `pr_builtin`, `pr_numbuiltins`, `PF_Fixme`;
`WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `OP_CALL0` to `OP_CALL8`.

A progs declares a builtin as a function whose `first_statement` is minus its number
(PROGS-10.2), as qcc writes for `= #n` in `defs.qc`. The number is the only link: the
engine never looks at the function's name or parameter list.

### Behaviour

- **BUILTIN-1.1** id's table has the 79 entries numbered 0 to 78 (LIMIT-5.1). The builtin with
  number `n` is the C function in the table of BUILTIN-5 for `n`, whatever the progs calls the
  function.
- **BUILTIN-1.2** Numbers 5, 33, 39, 42, 50, 60 to 66 and 71 have no builtin: the table holds
  `PF_Fixme` there, and calling one fails with the runtime error `unimplemented bulitin`
  (id's spelling). Number 0 cannot be called (LIMIT-5.2); 79 and above fail as LIMIT-5.1 says.
- **BUILTIN-1.3** A builtin reads its parameters from the parameter slots whatever the argument
  count of the call (VM-14.1): a parameter the call did not supply is whatever the slot held
  (VM-14.6). Only the builtins that join their string parameters (BUILTIN-7.3) read the
  argument count.
- **BUILTIN-1.4** A builtin that returns a value writes it to `RETURN` (one slot, or three for
  a vector). A builtin that returns nothing leaves the return slots unchanged unless a statement
  below says otherwise; unlike a QuakeC function (VM-16.2), it does not clear them.
- **BUILTIN-1.5** A builtin runs to completion within the call statement: no frame is pushed
  (VM-14.4). The builtins that call back into QuakeC (`walkmove` and `movetogoal`, through
  touch functions) do so as nested runs (VM-18).
- **BUILTIN-1.6** A builtin that fails may already have written globals, fields or engine
  state; where it has, the statement below says what is written before the check.

## BUILTIN-2 Reading arguments

**Source:** `WinQuake/progs.h`, `G_FLOAT`, `G_INT`, `G_EDICT`, `G_EDICTNUM`, `G_VECTOR`,
`G_STRING`, `PROG_TO_EDICT`; `WinQuake/pr_edict.c`, `NUM_FOR_EDICT`.

### Behaviour

- **BUILTIN-2.1** A float argument used as an integer (a channel, a style, a destination, a
  byte to write) is converted by C's float-to-int conversion, which truncates toward zero:
  1.9 is 1 and -1.9 is -1.
- **BUILTIN-2.2** A float outside the range of a 32-bit int, or a NaN, makes that conversion
  undefined in C. This spec fixes the answer as the x86 one (`cvttss2si`, `cvttsd2si`): the
  result is -2147483648 (0x80000000). Every conversion in this file follows this rule.
- **BUILTIN-2.3** **Withdrawn** 2026-10-16: it said the builtin fails before it writes
  anything, but the check is made where id's code reads the value (BUILTIN-2.9), after
  whatever the builtin writes first; replaced by BUILTIN-2.7.
- **BUILTIN-2.4** An entity argument read with `G_EDICTNUM` (or passed to `NUM_FOR_EDICT`)
  fails with the fatal error `NUM_FOR_EDICT: bad pointer` (BUILTIN-3.3) when its edict number
  is not less than `num_edicts` (EDICT-1.7). The statements below say which builtins do this.
- **BUILTIN-2.5** A string argument is the text at the string value (PROGS-17.2, EDICT-13.1),
  read up to its NUL. A value that is neither inside the string table nor an engine-made
  string reads other memory, which is undefined in C; this spec fixes the answer: the builtin
  fails with a runtime error (VM-19.7).
- **BUILTIN-2.6** A builtin that keeps a string argument (`setmodel`, the precache builtins,
  `lightstyle`) keeps the reference, not a copy of the text, so it sees later changes to that
  text, such as a new `ftos` result (BUILTIN-7.4).
- **BUILTIN-2.7** An entity value a builtin reads with `G_EDICT`, `G_EDICTNUM` or
  `PROG_TO_EDICT`, from a parameter, a global (`self`, `msg_entity`) or a field (`goalentity`,
  `enemy`), is not checked by id's code: `PROG_TO_EDICT` adds it to the start of the edict
  array. A value that names no edict (EDICT-1.9) makes the builtin read or write memory that is
  not an edict, which is undefined in C. This spec fixes the answer: the builtin fails with a
  runtime error (VM-19.7) where it reads the value (BUILTIN-2.9). A value that names an edict
  passes, even one at or above `num_edicts` or a free one.
- **BUILTIN-2.8** A string value is valid when it is from 0 to `numstrings - 1`, an offset into
  the string table (including one into the middle of a string, PROGS-11.3), or exactly the
  value of an engine-made string (EDICT-13.1, BUILTIN-7.4). Every other value is BUILTIN-2.5's
  invalid string: a negative one, `numstrings` or more outside the engine-made strings, and one
  that points past the start of an engine-made string's text, whose number depends on where the
  engine put that text (EDICT-13.3). A valid value's text is read up to its NUL (PROGS-11.1).
- **BUILTIN-2.9** The entity check of BUILTIN-2.7, the string check of BUILTIN-2.5 and
  BUILTIN-2.8, and the field check of BUILTIN-8.3 are made where id's code reads the value, at
  each `G_EDICT`, `G_EDICTNUM`, `PROG_TO_EDICT`, `G_STRING` and `E_STRING` and at `makestatic`'s
  read of `model` (BUILTIN-12.8), in the order id's code reads them. So a builtin that fails one
  has already done what id's code does before that read: `changelevel` has recorded that it was
  issued (BUILTIN-11.6) and `makestatic` has appended `svc_spawnstatic`. A value the builtin
  does not read is not checked: `changelevel` reads no string once one was issued, and `find`
  reads the field of only the edicts it examines (BUILTIN-8.8). Of two bad values, the one read
  first decides the error: `setmodel(e, m)` reads `e` before `m`. `G_EDICTNUM` checks the entity
  before `NUM_FOR_EDICT` (BUILTIN-2.4) does, so a value that names no edict is the runtime
  error and a value that names an edict at or above `num_edicts` is the fatal error.
- **BUILTIN-2.10** `WriteString(to, s)` and `WriteEntity(to, e)` read the destination
  (`WriteDest`, BUILTIN-13.1) and the value as two arguments of one C call
  (`MSG_WriteString (WriteDest(), G_STRING(OFS_PARM1))`,
  `MSG_WriteShort (WriteDest(), G_EDICTNUM(OFS_PARM1))`), and C leaves the order in which a
  call's arguments are evaluated unspecified, so which of two failures comes first depends on
  the compiler. This spec fixes the answer: the destination is read first, so with a bad
  destination and a bad value the error is the destination's.

## BUILTIN-3 Kinds of failure

**Source:** `WinQuake/pr_exec.c`, `PR_RunError`; `WinQuake/host.c`, `Host_Error`;
`WinQuake/sys_*.c`, `Sys_Error`; `WinQuake/pr_cmds.c`, `PF_error`, `PF_objerror`, `PF_break`.

### Behaviour

- **BUILTIN-3.1** A **runtime error** is `PR_RunError` with the message given: VM-19.1,
  VM-19.2 and VM-19.6 say what they do.
- **BUILTIN-3.2** A **host error** is a call to `Host_Error ("Program error")` without
  `PR_RunError`: the aborted runs never continue (VM-19.2) and the server shuts down, but no
  statement or call stack is printed and the call depth is not reset to 0, unlike VM-19.1.
  The shutdown runs `ClientDisconnect` for every spawned client (`Host_ShutdownServer`,
  `SV_DropClient`), and those runs start at the depth the error left. Only `error` and
  `objerror` raise it.
- **BUILTIN-3.3** A **fatal error** is `Sys_Error`: the engine prints the message and exits.
  In QuakeC Studio it ends the run with a typed error carrying the message, as a runtime error
  does, and nothing after it runs.
- **BUILTIN-3.4** A **console message** is text printed to the server console (`Con_Printf`)
  after which the builtin returns normally; the run continues.

## BUILTIN-4 Mock classes

How faithfully a builtin can be reproduced without id's engine, for `qc-builtins` (P1-16) and
the differential runner (P1-27). Each builtin's class is in the table of BUILTIN-5.

### Behaviour

- **BUILTIN-4.1** Class **pure**: the builtin reads and writes only the VM state (globals,
  edicts, the builtins' string buffers) and, for `random`, a random source. A mock that
  implements its statements is exact.
- **BUILTIN-4.2** Class **state**: the builtin also reads or writes engine state that a fixture
  can hold as plain data: the precache lists and the bounds of each precached model, the
  lightstyles, the cvars, the client slots with their spawn parms, the server's `maxclients`,
  its state (loading or active), its clock and the `changelevel` flag. A mock with that fixture
  is exact for everything QuakeC can observe.
- **BUILTIN-4.3** Class **output**: the builtin's effect outside the VM is output only: bytes
  appended to a network message buffer, console text, or text for a command buffer. A mock
  records the output, and is exact for everything QuakeC can observe, including the failures
  that depend on buffer sizes when it keeps the sizes of BUILTIN-13.
- **BUILTIN-4.4** Class **world**: the builtin needs the map's collision hulls, the area tree
  of linked edicts or the PVS. It cannot be mocked faithfully without the server simulation
  (`qs-sim`, P3-24); `qc-builtins` marks it engine-only, and `differential.md` (P1-27) says
  what the deterministic mock returns instead.

## BUILTIN-5 Numbers, names and prototypes

**Source:** `WinQuake/pr_cmds.c`, `pr_builtin`; `qcc/v101qc/defs.qc` (Quake-Tools@`c0d1b91`).

### Behaviour

- **BUILTIN-5.1** The vanilla builtins are numbers 1 to 78 in the table below. The prototype
  is the one in id's 1.01 `defs.qc`; the C function is the `pr_builtin` entry for that number;
  the class is BUILTIN-4's; the section is where its behaviour is specified. Numbers marked
  "none" are `PF_Fixme` (BUILTIN-1.2).
- **BUILTIN-5.2** Three numbers have two names in `defs.qc` and one C function each:
  `precache_model2` (75) is `PF_precache_model`, `precache_sound2` (76) is
  `PF_precache_sound` and `precache_file2` (77) is `PF_precache_file`. id's comment on 76:
  "precache_sound2 is different only for qcc".

| # | Name | Prototype (1.01 `defs.qc`) | C function | Class | Section |
| --- | --- | --- | --- | --- | --- |
| 1 | `makevectors` | `void(vector ang)` | `PF_makevectors` | pure | BUILTIN-6 |
| 2 | `setorigin` | `void(entity e, vector o)` | `PF_setorigin` | state | BUILTIN-9 |
| 3 | `setmodel` | `void(entity e, string m)` | `PF_setmodel` | state | BUILTIN-9 |
| 4 | `setsize` | `void(entity e, vector min, vector max)` | `PF_setsize` | state | BUILTIN-9 |
| 5 | none (`setabssize` in id's comment) | | `PF_Fixme` | | BUILTIN-1 |
| 6 | `break` | `void()` | `PF_break` | pure | BUILTIN-16 |
| 7 | `random` | `float()` | `PF_random` | pure | BUILTIN-6 |
| 8 | `sound` | `void(entity e, float chan, string samp, float vol, float atten)` | `PF_sound` | output | BUILTIN-12 |
| 9 | `normalize` | `vector(vector v)` | `PF_normalize` | pure | BUILTIN-6 |
| 10 | `error` | `void(string e)` | `PF_error` | pure | BUILTIN-16 |
| 11 | `objerror` | `void(string e)` | `PF_objerror` | pure | BUILTIN-16 |
| 12 | `vlen` | `float(vector v)` | `PF_vlen` | pure | BUILTIN-6 |
| 13 | `vectoyaw` | `float(vector v)` | `PF_vectoyaw` | pure | BUILTIN-6 |
| 14 | `spawn` | `entity()` | `PF_Spawn` | pure | BUILTIN-8 |
| 15 | `remove` | `void(entity e)` | `PF_Remove` | pure | BUILTIN-8 |
| 16 | `traceline` | `void(vector v1, vector v2, float nomonsters, entity forent)` | `PF_traceline` | world | BUILTIN-14 |
| 17 | `checkclient` | `entity()` | `PF_checkclient` | world | BUILTIN-14 |
| 18 | `find` | `entity(entity start, .string fld, string match)` | `PF_Find` | pure | BUILTIN-8 |
| 19 | `precache_sound` | `string(string s)` | `PF_precache_sound` | state | BUILTIN-10 |
| 20 | `precache_model` | `string(string s)` | `PF_precache_model` | state | BUILTIN-10 |
| 21 | `stuffcmd` | `void(entity client, string s)` | `PF_stuffcmd` | output | BUILTIN-11 |
| 22 | `findradius` | `entity(vector org, float rad)` | `PF_findradius` | pure | BUILTIN-8 |
| 23 | `bprint` | `void(string s)` | `PF_bprint` | output | BUILTIN-11 |
| 24 | `sprint` | `void(entity client, string s)` | `PF_sprint` | output | BUILTIN-11 |
| 25 | `dprint` | `void(string s)` | `PF_dprint` | output | BUILTIN-11 |
| 26 | `ftos` | `string(float f)` | `PF_ftos` | pure | BUILTIN-7 |
| 27 | `vtos` | `string(vector v)` | `PF_vtos` | pure | BUILTIN-7 |
| 28 | `coredump` | `void()` | `PF_coredump` | output | BUILTIN-16 |
| 29 | `traceon` | `void()` | `PF_traceon` | pure | BUILTIN-16 |
| 30 | `traceoff` | `void()` | `PF_traceoff` | pure | BUILTIN-16 |
| 31 | `eprint` | `void(entity e)` | `PF_eprint` | output | BUILTIN-16 |
| 32 | `walkmove` | `float(float yaw, float dist)` | `PF_walkmove` | world | BUILTIN-15 |
| 33 | none | | `PF_Fixme` | | BUILTIN-1 |
| 34 | `droptofloor` | `float(float yaw, float dist)` | `PF_droptofloor` | world | BUILTIN-15 |
| 35 | `lightstyle` | `void(float style, string value)` | `PF_lightstyle` | output | BUILTIN-12 |
| 36 | `rint` | `float(float v)` | `PF_rint` | pure | BUILTIN-6 |
| 37 | `floor` | `float(float v)` | `PF_floor` | pure | BUILTIN-6 |
| 38 | `ceil` | `float(float v)` | `PF_ceil` | pure | BUILTIN-6 |
| 39 | none | | `PF_Fixme` | | BUILTIN-1 |
| 40 | `checkbottom` | `float(entity e)` | `PF_checkbottom` | world | BUILTIN-14 |
| 41 | `pointcontents` | `float(vector v)` | `PF_pointcontents` | world | BUILTIN-14 |
| 42 | none | | `PF_Fixme` | | BUILTIN-1 |
| 43 | `fabs` | `float(float f)` | `PF_fabs` | pure | BUILTIN-6 |
| 44 | `aim` | `vector(entity e, float speed)` | `PF_aim` | world | BUILTIN-14 |
| 45 | `cvar` | `float(string s)` | `PF_cvar` | state | BUILTIN-11 |
| 46 | `localcmd` | `void(string s)` | `PF_localcmd` | output | BUILTIN-11 |
| 47 | `nextent` | `entity(entity e)` | `PF_nextent` | pure | BUILTIN-8 |
| 48 | `particle` | `void(vector o, vector d, float color, float count)` | `PF_particle` | output | BUILTIN-12 |
| 49 | `ChangeYaw` | `void()` | `PF_changeyaw` | pure | BUILTIN-15 |
| 50 | none | | `PF_Fixme` | | BUILTIN-1 |
| 51 | `vectoangles` | `vector(vector v)` | `PF_vectoangles` | pure | BUILTIN-6 |
| 52 | `WriteByte` | `void(float to, float f)` | `PF_WriteByte` | output | BUILTIN-13 |
| 53 | `WriteChar` | `void(float to, float f)` | `PF_WriteChar` | output | BUILTIN-13 |
| 54 | `WriteShort` | `void(float to, float f)` | `PF_WriteShort` | output | BUILTIN-13 |
| 55 | `WriteLong` | `void(float to, float f)` | `PF_WriteLong` | output | BUILTIN-13 |
| 56 | `WriteCoord` | `void(float to, float f)` | `PF_WriteCoord` | output | BUILTIN-13 |
| 57 | `WriteAngle` | `void(float to, float f)` | `PF_WriteAngle` | output | BUILTIN-13 |
| 58 | `WriteString` | `void(float to, string s)` | `PF_WriteString` | output | BUILTIN-13 |
| 59 | `WriteEntity` | `void(float to, entity s)` | `PF_WriteEntity` | output | BUILTIN-13 |
| 60 to 66 | none (the `bWrite*` lines in `defs.qc` are commented out) | | `PF_Fixme` | | BUILTIN-1 |
| 67 | `movetogoal` | `void(float step)` | `SV_MoveToGoal` (`sv_move.c`) | world | BUILTIN-15 |
| 68 | `precache_file` | `string(string s)` | `PF_precache_file` | pure | BUILTIN-10 |
| 69 | `makestatic` | `void(entity e)` | `PF_makestatic` | output | BUILTIN-12 |
| 70 | `changelevel` | `void(string s)` | `PF_changelevel` | state | BUILTIN-11 |
| 71 | none | | `PF_Fixme` | | BUILTIN-1 |
| 72 | `cvar_set` | `void(string var, string val)` | `PF_cvar_set` | state | BUILTIN-11 |
| 73 | `centerprint` | `void(entity client, string s)` | `PF_centerprint` | output | BUILTIN-11 |
| 74 | `ambientsound` | `void(vector pos, string samp, float vol, float atten)` | `PF_ambientsound` | output | BUILTIN-12 |
| 75 | `precache_model2` | `string(string s)` | `PF_precache_model` | state | BUILTIN-10 |
| 76 | `precache_sound2` | `string(string s)` | `PF_precache_sound` | state | BUILTIN-10 |
| 77 | `precache_file2` | `string(string s)` | `PF_precache_file` | pure | BUILTIN-10 |
| 78 | `setspawnparms` | `void(entity e)` | `PF_setspawnparms` | state | BUILTIN-8 |

## BUILTIN-6 Math

**Source:** `PF_makevectors`, `PF_normalize`, `PF_vlen`, `PF_vectoyaw`, `PF_vectoangles`,
`PF_random`, `PF_rint`, `PF_floor`, `PF_ceil`, `PF_fabs`; `WinQuake/mathlib.c`,
`AngleVectors`.

The arithmetic is C's: `float` operations round to f32, a `double` constant such as `M_PI`
or `0.5` makes the expression double, and the result is rounded to f32 when it is stored. The
statements give each expression as id writes it, because that decides the last bit.

### Behaviour

- **BUILTIN-6.1** `makevectors(ang)` writes the globals `v_forward`, `v_right` and `v_up` and
  nothing else. With `angle = (f32)(a * (M_PI*2/360))` computed in double for each component
  `a`, and `sy`, `cy`, `sp`, `cp`, `sr`, `cr` the f32 sine and cosine (computed in double) of
  the yaw (`ang_y`), pitch (`ang_x`) and roll (`ang_z`) angles, in f32 arithmetic:
  `v_forward = (cp*cy, cp*sy, -sp)`,
  `v_right = (-1*sr*sp*cy + -1*cr*-sy, -1*sr*sp*sy + -1*cr*cy, -1*sr*cp)`,
  `v_up = (cr*sp*cy + -sr*-sy, cr*sp*sy + -sr*cy, cr*cp)`. Zero angles give
  `v_forward = '1 0 -0'`: the sign of a zero is kept.
- **BUILTIN-6.2** `vlen(v)` returns `(f32)sqrt(x*x + y*y + z*z)`, the sum in f32 and the square
  root in double.
- **BUILTIN-6.3** `normalize(v)` computes the length as `vlen` does. If it is 0 the result is
  `'0 0 0'`; otherwise it computes `inv = (f32)(1/len)` and returns `(x*inv, y*inv, z*inv)` in
  f32. A vector whose squared components all underflow to 0 in f32 has length 0, so a very
  short vector also gives `'0 0 0'`.
- **BUILTIN-6.4** `vectoyaw(v)` returns 0 when `v_x` and `v_y` are both 0 (either sign).
  Otherwise it computes `atan2(v_y, v_x) * 180 / M_PI` in double, truncates it toward zero to an
  integer, and adds 360 if the result is negative: the result is a whole number from 0 to 359,
  and -179.4 degrees gives 181, not 180.
- **BUILTIN-6.5** `vectoangles(v)` returns `'pitch yaw 0'`. When `v_x` and `v_y` are both 0 the
  yaw is 0 and the pitch is 90 if `v_z > 0` and 270 otherwise, so `'0 0 0'` gives `'270 0 0'`.
  Otherwise the yaw is `vectoyaw`'s, and the pitch is
  `atan2(v_z, (f32)sqrt(v_x*v_x + v_y*v_y))` in degrees, truncated toward zero, plus 360 if
  negative.
- **BUILTIN-6.6** `random()` returns `(rand() & 0x7fff) / (f32)0x7fff`: one of the 32768
  values `k / 32767` for `k` from 0 to 32767, so both 0 and 1 can be returned (id's comment
  "0<= num < 1" is wrong). Each call takes one number from the C library's `rand`, which the
  engine also uses elsewhere (`movetogoal`, BUILTIN-15.6); the sequence is the C library's and
  is not part of this spec. A mock takes the 15-bit values from an injected source.
- **BUILTIN-6.7** `rint(f)` returns `(int)(f + 0.5)` when `f > 0` and `(int)(f - 0.5)`
  otherwise, the sum in double and the conversion truncating (BUILTIN-2.1, BUILTIN-2.2): halves
  round away from zero (2.5 gives 3, -2.5 gives -3), and 0 and -0 give +0.
- **BUILTIN-6.8** `floor(f)` and `ceil(f)` return C's `floor` and `ceil` of `f` (in double,
  exact) rounded to f32, so the sign of a zero is kept: `ceil(-0.5)` is -0. `fabs(f)` returns
  `f` with the sign bit cleared.
- **BUILTIN-6.9** `sin`, `cos`, `atan2` and `sqrt` are the C library's, in double. `sqrt` is
  exact; the others can differ in the last bit of the double between C libraries, which almost
  never survives the rounding to f32. This spec takes the results of the reference build (the
  oracle's C library on x86-64, P1-11); a mock uses Rust's `f64` functions, and a vector where
  they disagree after rounding is listed in `differential.md` as an expected difference.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BUILTIN_RANDOM_MASK` | 0x7fff (32767) | `PF_random`: `(rand ()&0x7fff) / ((float)0x7fff)` |

## BUILTIN-7 Strings

**Source:** `PF_ftos`, `PF_vtos`, `PF_VarString`, `pr_string_temp`.

### Behaviour

- **BUILTIN-7.1** `ftos(f)` returns `f` as decimal text: when `f == (int)f` (BUILTIN-2.2 for the
  conversion) it is `printf("%d", (int)f)`, so 1 gives `1` and -0 gives `0`; otherwise
  `printf("%5.1f", f)`: one decimal, rounded as C's `printf` rounds the exact binary value, right
  aligned in at least 5 characters, so 0.5 gives `  0.5` and 1.25 gives `  1.2`.
- **BUILTIN-7.2** `vtos(v)` returns `printf("'%5.1f %5.1f %5.1f'", v_x, v_y, v_z)`: `'0 0 0'`
  gives `'  0.0   0.0   0.0'`, with the single quotes.
- **BUILTIN-7.3** `error`, `objerror`, `bprint` and `dprint` use the text of every string
  parameter from PARM0 to the last one the call supplied, joined with nothing between; `sprint`
  and `centerprint` do the same from PARM1. The argument count decides how many (VM-14.1): a
  call with more arguments than the prototype prints them all, and one with none gives empty
  text.
- **BUILTIN-7.4** `ftos` and `vtos` write their text into one engine buffer and return a
  string value for it (an engine-made string, EDICT-13.1); every call returns the same value,
  so the text of an earlier result changes when either builtin is called again. A mock keeps
  one such buffer.
- **BUILTIN-7.5** id's buffers have fixed sizes: 128 bytes for `ftos` and `vtos`, 256 for the
  joined text of BUILTIN-7.3. Text longer than the buffer overflows it, which is undefined in C;
  `vtos` reaches it only with components of magnitude about 1e38 (130 characters for three
  `-3.4e38`). This spec fixes the answer: the text is never truncated, and the result is the
  full text.
- **BUILTIN-7.6** `ftos` of a NaN or an infinity is printed by the C library, and C libraries
  differ (MSVC's text is not C99's). This spec fixes the answer as `  nan` for every NaN,
  `  inf` and ` -inf`.
- **BUILTIN-7.7** `vtos` prints each component the way `ftos` prints a value that is not whole
  (BUILTIN-7.2), and a NaN or infinite component, which `%5.1f` leaves to the C library as
  BUILTIN-7.6 says, as `  nan` for every NaN, `  inf` and ` -inf`: `vtos` of a vector whose
  components are a NaN, an infinity and minus infinity is `'  nan   inf  -inf'`.

## BUILTIN-8 Entities

**Source:** `PF_Spawn`, `PF_Remove`, `PF_Find`, `PF_nextent`, `PF_findradius`,
`PF_setspawnparms`; `WinQuake/pr_edict.c`, `ED_Alloc`, `ED_Free`; `WinQuake/mathlib.c`,
`Length`.

### Behaviour

- **BUILTIN-8.1** `spawn()` allocates an edict by EDICT-6 and returns it, failing as EDICT-6.2
  says when none is left (a fatal error, BUILTIN-3.3).
- **BUILTIN-8.2** `remove(e)` frees `e` by EDICT-7. It refuses nothing: the world, a client
  edict and an edict that is already free are freed too (EDICT-4.6, EDICT-7.4).
- **BUILTIN-8.3** `find(start, fld, match)` looks at edicts from `start`'s number plus 1 up to
  `num_edicts - 1`, skips free ones, and returns the first whose string field `fld` has the same
  text as `match` (byte comparison, `strcmp`); it returns the world when none does. The world
  itself is never examined, and a field value of 0 is the empty string, so `match` `""` finds
  edicts whose field is unset. `start` is read with `G_EDICTNUM` (BUILTIN-2.4); `fld` is a
  field index used without a check, which is undefined in C outside `0` to `entityfields - 1`
  and fails with a runtime error in this spec (VM-11.4).
- **BUILTIN-8.4** `nextent(e)` returns the first edict after `e`, by number, that is not free,
  and the world when it reaches `num_edicts`. `nextent(world)` starts at edict 1, so client
  edicts are included. `e` is read with `G_EDICTNUM`.
- **BUILTIN-8.5** `findradius(org, rad)` looks at edicts 1 to `num_edicts - 1` in order,
  skipping those that are free or whose `solid` is `SOLID_NOT` (0). For each other edict it
  computes, per axis, `d = org - (origin + (mins + maxs) * 0.5)` with `mins + maxs` in f32 and
  the rest in double, rounded to f32, and the length of `d` as `vlen` does; the edict is
  included unless that length is greater than `rad`, so one exactly at `rad` is included and
  none is when `rad` is a NaN.
- **BUILTIN-8.6** `findradius` links the included edicts through their `chain` field in
  reverse order of number: it returns the last one included, whose `chain` is the one included
  before it, and so on, and the first one included has `chain` world. It returns the world
  when none is included. The `chain` of an edict not included is not changed.
- **BUILTIN-8.7** `setspawnparms(e)` copies the 16 saved spawn parms of `e`'s client slot to the
  globals `parm1` to `parm16` (EDICT-11.9). `e` goes through `NUM_FOR_EDICT` (BUILTIN-2.4); an
  edict number outside 1 to `maxclients` fails with the runtime error `Entity is not a client`.
  It does not check that a player is connected to that slot.
- **BUILTIN-8.8** `find` reads `start` (BUILTIN-2.4), then `match`, then, for each edict it
  examines (BUILTIN-8.3), the field `fld` with `E_STRING` before comparing: it checks that
  `fld` is a field slot (`invalid field`, BUILTIN-8.3) and that the field's value is a valid
  string (BUILTIN-2.8; `invalid string`). So it fails at the first examined edict whose field
  value is not a valid string, even when a later edict would match; it returns a match found
  before such an edict; and when it examines no edict (`start` is the last edict in use, or
  every edict after it is free) it checks neither `fld` nor any field value. id's checks for a
  null `match` (`PF_Find: bad search string`) and a null field text never fire: a string value
  never gives a null pointer.

## BUILTIN-9 Placing entities

**Source:** `PF_setorigin`, `PF_setsize`, `PF_setmodel`, `SetMinMaxSize`;
`WinQuake/world.c`, `SV_LinkEdict`; `WinQuake/model.c`, `Mod_LoadAliasModel`,
`Mod_LoadSpriteModel`, `Mod_LoadSubmodels`, `Mod_LoadBrushModel`.

### Behaviour

- **BUILTIN-9.1** `setorigin(e, o)` sets `e`'s `origin` to `o` and links `e` into the area tree
  without touching triggers, which writes `absmin` and `absmax` as EDICT-12.2 says (not for the
  world or a free edict). Linking without triggers runs no QuakeC.
- **BUILTIN-9.2** `setsize(e, min, max)` fails with the runtime error `backwards mins/maxs`
  when `min` is greater than `max` on any axis, checked x, y, z, before writing anything; a NaN
  component passes the check. Otherwise it sets `mins` to `min`, `maxs` to `max` and `size` to
  `max - min` (f32), and links `e` as `setorigin` does. The box is never rotated: id's
  `rotate` argument is forced to false ("FIXME: implement rotation properly again").
- **BUILTIN-9.3** `setmodel(e, m)` looks for `m`'s text in the model precache list from entry 0
  and fails with the runtime error `no precache: <m>` followed by a newline when it is not
  there. Entry 0 is the empty string, so `setmodel(e, "")` finds entry 0.
- **BUILTIN-9.4** It then sets `e`'s `model` to the string value `m` itself (not the list's
  copy, BUILTIN-2.6) and `modelindex` to the entry number, and sets the size as `setsize` does
  from the model's bounds: for entry 0, or an entry with no loaded model, `'0 0 0'` for both;
  for an alias model (`.mdl`) `'-16 -16 -16'` and `'16 16 16'`; for a sprite (`.spr`)
  `-w/2` to `w/2` on x and y and `-h/2` to `h/2` on z, from its largest frame width `w` and
  height `h` in integer arithmetic (an odd width loses its half); for the world (entry 1) and
  its submodels (`*1`, `*2`, ...) the bounds stored in the BSP file, widened by 1 on each side
  by `Mod_LoadSubmodels`. A model's bounds never trip `backwards mins/maxs`.

## BUILTIN-10 Precaching

**Source:** `PF_precache_sound`, `PF_precache_model`, `PF_precache_file`,
`PR_CheckEmptyString`; `WinQuake/sv_main.c`, `SV_SpawnServer`; `WinQuake/model.c`,
`Mod_ForName`, `Mod_LoadModel`.

### Behaviour

- **BUILTIN-10.1** `precache_sound(s)` and `precache_model(s)` fail with the runtime error
  `PF_Precache_*: Precache can only be done in spawn functions` unless the server is loading:
  from the start of `SV_SpawnServer` until the map's entities have spawned (EDICT-8). The
  check comes first.
- **BUILTIN-10.2** They then set `RETURN` to `s` (the same string value), and fail with the
  runtime error `Bad string` when the first byte of `s` is a space or below, read as a signed
  `char` as on x86: an empty string, one starting with a control character or a space, and one
  starting with a byte from 0x80 to 0xFF all fail.
- **BUILTIN-10.3** They look through the list from entry 0: a name already there does nothing;
  otherwise `s` (the reference, BUILTIN-2.6) goes in the first unused entry. A full list fails
  as LIMIT-6.2 says. When the map starts, the sound list holds only entry 0 (the empty string)
  and the model list holds entry 0, the world model `maps/<map>.bsp` in entry 1 and its
  submodels `*1`, `*2`, ... after it, so precaching any of those names does nothing.
- **BUILTIN-10.4** `precache_model` loads a newly added model at once; a file that cannot be
  found is the fatal error `Mod_NumForName: <s> not found`. `precache_sound` does not load
  anything.
- **BUILTIN-10.5** `precache_file(s)` sets `RETURN` to `s` and does nothing else, at any time;
  id's comment: "precache_file is only used to copy files with qcc, it does nothing".
- **BUILTIN-10.6** `precache_model2`, `precache_sound2` and `precache_file2` behave exactly as
  `precache_model`, `precache_sound` and `precache_file` (BUILTIN-5.2).

## BUILTIN-11 Printing, commands and settings

**Source:** `PF_bprint`, `PF_sprint`, `PF_centerprint`, `PF_dprint`, `PF_stuffcmd`,
`PF_localcmd`, `PF_changelevel`, `PF_cvar`, `PF_cvar_set`; `WinQuake/host.c`,
`SV_BroadcastPrintf`, `Host_ClientCommands`; `WinQuake/console.c`, `Con_DPrintf`;
`WinQuake/cmd.c`, `Cbuf_AddText`; `WinQuake/cvar.c`, `Cvar_VariableValue`, `Cvar_Set`;
`WinQuake/common.c`, `Q_atof`; `WinQuake/protocol.h`.

### Behaviour

- **BUILTIN-11.1** `bprint(s)` appends `svc_print` (8) and the joined text (BUILTIN-7.3) with
  its NUL to the reliable message of every client slot that is active and spawned.
- **BUILTIN-11.2** `sprint(client, s)` and `centerprint(client, s)` read `client` with
  `G_EDICTNUM` (BUILTIN-2.4). When its number is outside 1 to `maxclients` they print the
  console message `tried to sprint to a non-client` (both builtins) and return. Otherwise they
  append `svc_print` (8) or `svc_centerprint` (26), then the joined text from PARM1 with its
  NUL, to that slot's reliable message, whether or not a player is connected to it.
- **BUILTIN-11.3** `dprint(s)` prints the joined text to the console only when the `developer`
  setting is non-zero, and does nothing otherwise.
- **BUILTIN-11.4** `stuffcmd(client, s)` reads `client` with `G_EDICTNUM`; a number outside 1 to
  `maxclients` fails with the runtime error `Parm 0 not a client`. Otherwise it appends
  `svc_stufftext` (9) and `s` with its NUL to that slot's reliable message, connected or not;
  the client runs the text as console commands. No newline is added.
- **BUILTIN-11.5** `localcmd(s)` appends `s`, without its NUL and without adding a newline, to
  the server's own command buffer (8192 bytes), which the host runs later, not during the call.
  If the buffer's contents plus `s` would reach 8192 bytes it prints the console message
  `Cbuf_AddText: overflow` and drops `s`.
- **BUILTIN-11.6** `changelevel(s)` does nothing if a `changelevel` was already issued since
  the map started. Otherwise it records that one was, and appends `changelevel <s>` and a
  newline to the command buffer as `localcmd` does; the level changes when the host next runs
  its command buffer, not during the call.
- **BUILTIN-11.7** `cvar(s)` returns the value of the setting named `s`, or 0 when there is no
  such setting. The value is the setting's text converted by `Q_atof`: an optional `-`, then a
  hexadecimal number after `0x` or `0X`, the code of the character after a `'`, or decimal
  digits with at most one `.`, stopping at the first other character, with no exponent and no
  leading spaces (`"1e3"` is 1, `" 5"` is 0).
- **BUILTIN-11.8** `cvar_set(var, val)` sets the setting named `var` to the text `val` and its
  value to `Q_atof(val)`. An unknown name prints the console message
  `Cvar_Set: variable <var> not found` and changes nothing. When the setting is a server
  setting, the text changed and the server is active, it also broadcasts
  `"<var>" changed to "<val>"` and a newline as `bprint` does.

## BUILTIN-12 Sounds, particles, lightstyles and static entities

**Source:** `PF_sound`, `PF_ambientsound`, `PF_particle`, `PF_lightstyle`, `PF_makestatic`;
`WinQuake/sv_main.c`, `SV_StartSound`, `SV_StartParticle`, `SV_ModelIndex`;
`WinQuake/protocol.h`; `WinQuake/sound.h`.

### Behaviour

- **BUILTIN-12.1** `sound(e, chan, samp, vol, atten)` converts `chan` to an integer
  (BUILTIN-2.1) and `vol * 255` (f32) to an integer volume, and fails with the fatal errors
  `SV_StartSound: volume = <volume>` when the volume is outside 0 to 255,
  `SV_StartSound: attenuation = <atten>` when `atten` is outside 0 to 4, and
  `SV_StartSound: channel = <chan>` when the channel is outside 0 to 7, checked in that order.
  A NaN `atten` passes its check.
- **BUILTIN-12.2** Then, when the unreliable datagram (BUILTIN-13.3) holds more than
  `MAX_DATAGRAM - 16` (1008) bytes, the sound is dropped silently. When `samp` is not in the
  sound precache list, looked up from entry 1, it prints the console message
  `SV_StartSound: <samp> not precacheed` (id's spelling) and returns. `e` goes through
  `NUM_FOR_EDICT` (BUILTIN-2.4) after these checks.
- **BUILTIN-12.3** Otherwise it appends to the unreliable datagram: `svc_sound` (6); a byte of
  flags, 1 when the volume is not 255 and 2 when `atten` is not 1.0; the volume as a byte if
  flag 1; `atten * 64` as a byte if flag 2 (its low 8 bits, BUILTIN-13.2, so an `atten` of 4
  is sent as 0); a short `(edict number << 3) | channel`; the sound's precache entry as a
  byte; and three coordinates (BUILTIN-13.2), the centre of `e`,
  `origin + 0.5 * (mins + maxs)` per axis.
- **BUILTIN-12.4** `ambientsound(pos, samp, vol, atten)` looks for `samp` in the sound precache
  list from entry 0; when it is not there it prints the console message `no precache: <samp>`
  and returns. Otherwise it appends to the signon buffer: `svc_spawnstaticsound` (29), three
  coordinates of `pos`, the entry number as a byte, `vol * 255` as a byte and `atten * 64` as a
  byte. Every map's client receives the signon buffer when it connects.
- **BUILTIN-12.5** `particle(o, d, color, count)` does nothing when the unreliable datagram holds
  more than 1008 bytes. Otherwise it appends `svc_particle` (18), three coordinates of `o`, for
  each axis of `d` the integer `(int)(d * 16)` clamped to -128 to 127 as a char, then `count`
  as a byte and `color` as a byte, both converted by BUILTIN-2.1.
- **BUILTIN-12.6** `lightstyle(style, value)` converts `style` to an integer and stores `value`
  (the reference, BUILTIN-2.6) as that lightstyle. id does not check the style number against
  `LIMIT_MAX_LIGHTSTYLES` (64), so a number outside 0 to 63 writes outside the array, which is
  undefined in C; this spec fixes the answer: it fails with a runtime error (VM-19.7) before
  storing anything.
- **BUILTIN-12.7** When the server is active, `lightstyle` also appends `svc_lightstyle` (12),
  the style as a char and `value` with its NUL to the reliable message of every client slot
  that is active or spawned. While the map loads it only stores the style, which clients get
  when they connect.
- **BUILTIN-12.8** `makestatic(e)` appends to the signon buffer: `svc_spawnstatic` (20); the
  model's precache entry as a byte, found by the text of `e`'s `model` (0 for an empty model,
  and the fatal error `SV_ModelIndex: model <model> not precached` when the text is not in the
  list); `frame`, `colormap` and `skin` as bytes; then for x, y and z in turn the coordinate of
  `origin` and the angle of `angles` (BUILTIN-13.2). Then it frees `e` (EDICT-7.5).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BUILTIN_SVC_SOUND` | 6 | `WinQuake/protocol.h`, `svc_sound` |
| `BUILTIN_SVC_PRINT` | 8 | `WinQuake/protocol.h`, `svc_print` |
| `BUILTIN_SVC_STUFFTEXT` | 9 | `WinQuake/protocol.h`, `svc_stufftext` |
| `BUILTIN_SVC_LIGHTSTYLE` | 12 | `WinQuake/protocol.h`, `svc_lightstyle` |
| `BUILTIN_SVC_PARTICLE` | 18 | `WinQuake/protocol.h`, `svc_particle` |
| `BUILTIN_SVC_SPAWNSTATIC` | 20 | `WinQuake/protocol.h`, `svc_spawnstatic` |
| `BUILTIN_SVC_CENTERPRINT` | 26 | `WinQuake/protocol.h`, `svc_centerprint` |
| `BUILTIN_SVC_SPAWNSTATICSOUND` | 29 | `WinQuake/protocol.h`, `svc_spawnstaticsound` |
| `BUILTIN_SND_VOLUME` | 1 | `WinQuake/protocol.h`, `SND_VOLUME` |
| `BUILTIN_SND_ATTENUATION` | 2 | `WinQuake/protocol.h`, `SND_ATTENUATION` |
| `BUILTIN_DEFAULT_SOUND_VOLUME` | 255 | `WinQuake/sound.h`, `DEFAULT_SOUND_PACKET_VOLUME` |
| `BUILTIN_DEFAULT_SOUND_ATTENUATION` | 1.0 | `WinQuake/sound.h`, `DEFAULT_SOUND_PACKET_ATTENUATION` |
| `BUILTIN_DATAGRAM_SLACK` | 16 | `WinQuake/sv_main.c`, `SV_StartSound` and `SV_StartParticle`: `sv.datagram.cursize > MAX_DATAGRAM-16` |

## BUILTIN-13 Network messages

**Source:** `PF_WriteByte` to `PF_WriteEntity`, `WriteDest`; `WinQuake/common.c`,
`MSG_WriteChar`, `MSG_WriteByte`, `MSG_WriteShort`, `MSG_WriteLong`, `MSG_WriteCoord`,
`MSG_WriteAngle`, `MSG_WriteString`, `SZ_GetSpace`; `WinQuake/server.h`; `WinQuake/sv_main.c`,
`SV_ConnectClient`, `SV_SendClientMessages`.

### Behaviour

- **BUILTIN-13.1** The `Write*` builtins convert `to` to an integer and append to: 0
  (`MSG_BROADCAST`) the unreliable datagram; 1 (`MSG_ONE`) the reliable message of the client
  slot of the edict in the global `msg_entity`, which goes through `NUM_FOR_EDICT`
  (BUILTIN-2.4) and fails with the runtime error `WriteDest: not a client` when its number is
  outside 1 to `maxclients`; 2 (`MSG_ALL`) the reliable datagram; 3 (`MSG_INIT`) the signon
  buffer. Any other value fails with the runtime error `WriteDest: bad destination`.
- **BUILTIN-13.2** The value is written little-endian: `WriteByte` and `WriteChar` write the
  low 8 bits of the integer conversion of `f` (BUILTIN-2.1), `WriteShort` the low 16 bits,
  `WriteLong` all 32; `WriteCoord` writes the low 16 bits of `(int)(f * 8)`, the product in f32;
  `WriteAngle` writes `((int)f * 256 / 360) & 255` in integer arithmetic, so the angle is
  truncated to whole degrees first and the division truncates toward zero (-90 gives 192);
  `WriteString` writes the text and its NUL; `WriteEntity` writes the edict number of `s`
  (through `NUM_FOR_EDICT`, BUILTIN-2.4) as a short. No value is range checked: `PARANOID` is
  off.
- **BUILTIN-13.3** The unreliable datagram and the reliable datagram are `LIMIT_MAX_DATAGRAM`
  (1024) bytes and the signon buffer is 8192; they are cleared for each map (the datagrams
  also after each frame's updates are sent). A write that does not fit is the fatal error
  `SZ_GetSpace: overflow without allowoverflow set`.
- **BUILTIN-13.4** A client slot's reliable message is `LIMIT_MAX_MSGLEN` (8000) bytes and may
  overflow: a write that does not fit prints the console message `SZ_GetSpace: overflow`,
  empties the message, marks it overflowed and then writes; the server drops that client when
  it next sends updates. The run continues.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BUILTIN_MSG_BROADCAST` | 0 | `WinQuake/pr_cmds.c`, `MSG_BROADCAST`: "unreliable to all" |
| `BUILTIN_MSG_ONE` | 1 | `WinQuake/pr_cmds.c`, `MSG_ONE`: "reliable to one (msg_entity)" |
| `BUILTIN_MSG_ALL` | 2 | `WinQuake/pr_cmds.c`, `MSG_ALL`: "reliable to all" |
| `BUILTIN_MSG_INIT` | 3 | `WinQuake/pr_cmds.c`, `MSG_INIT`: "write to the init string" |
| `BUILTIN_SIGNON_SIZE` | 8192 | `WinQuake/server.h`, `server_t`, `signon_buf[8192]` |
| `BUILTIN_CMD_TEXT_SIZE` | 8192 | `WinQuake/cmd.c`, `Cbuf_Init`: `SZ_Alloc (&cmd_text, 8192)` |

## BUILTIN-14 World queries

**Source:** `PF_traceline`, `PF_checkclient`, `PF_newcheckclient`, `PF_pointcontents`,
`PF_checkbottom`, `PF_aim`; `WinQuake/world.c`, `SV_Move`, `SV_ClipToLinks`,
`SV_PointContents`; `WinQuake/world.h`; `WinQuake/sv_move.c`, `SV_CheckBottom`.

### Behaviour

- **BUILTIN-14.1** `traceline(v1, v2, nomonsters, forent)` traces a point from `v1` to `v2`
  through the world and the linked edicts whose `solid` is not `SOLID_NOT`. It ignores
  `forent`, every edict whose `owner` is `forent`, `forent`'s own `owner`, and, when `forent`'s
  `size_x` is not 0, every edict whose `size_x` is 0. `forent` is never "none": when it is the
  world, every edict whose `owner` is the world (0) is ignored, so only the map and edicts with
  an owner can be hit. `nomonsters` is converted to an integer: 1 (`MOVE_NOMONSTERS`) also
  ignores every edict whose `solid` is not `SOLID_BSP`; 2 (`MOVE_MISSILE`) traces a box of
  `'-15 -15 -15'` to `'15 15 15'` instead of a point against edicts whose `flags` have
  `FL_MONSTER` (32); any other value is a normal trace.
- **BUILTIN-14.2** `traceline` writes all nine `trace_*` globals (EDICT-11.11): `trace_allsolid`,
  `trace_startsolid`, `trace_inopen` and `trace_inwater` as 0 or 1, `trace_fraction`,
  `trace_endpos`, `trace_plane_normal`, `trace_plane_dist`, and `trace_ent`, the edict hit or
  the world when nothing was hit. It returns nothing; the 1.01 prototype `void(...)` agrees.
- **BUILTIN-14.3** `checkclient()` returns a client edict that `self` might see, or the world.
  When at least 0.1 seconds of server time have passed since it last chose, it chooses again:
  starting after the last choice, it cycles through client edicts 1 to `maxclients` and takes
  the first that is not free, has `health` above 0 and has no `FL_NOTARGET` (128) in `flags`,
  or the last choice again if no other qualifies, and stores the PVS at that client's
  `origin + view_ofs`. It returns the world when the chosen edict is free or its `health` is 0
  or less, or when the leaf at `self`'s `origin + view_ofs` is not in the stored PVS; otherwise
  the chosen edict.
- **BUILTIN-14.4** `pointcontents(v)` returns the contents of the world model's point hull at
  `v`: `CONTENTS_EMPTY` (-1), `CONTENTS_SOLID` (-2), `CONTENTS_WATER` (-3), `CONTENTS_SLIME`
  (-4), `CONTENTS_LAVA` (-5) or `CONTENTS_SKY` (-6). The current contents -9 to -14 are
  returned as `CONTENTS_WATER`. Entities are not considered.
- **BUILTIN-14.5** `checkbottom(e)` returns 1 when `SV_CheckBottom` finds `e`'s bounding box
  supported and 0 otherwise: supported when the world is solid 1 unit below each bottom corner,
  or when traces down from the bottom centre and from each bottom corner all hit within 36
  units (two steps) and no corner's floor is more than 18 units (`BUILTIN_STEPSIZE`) below the
  centre's. It writes nothing.
- **BUILTIN-14.6** `aim(e, speed)` ignores `speed` and returns a direction for `e` to shoot
  along, reading `v_forward`, the `teamplay` and `sv_aim` (0.93) settings and the `takedamage`
  and `team` fields: `v_forward` itself when a trace from `e`'s origin plus 20 units up along
  `v_forward` for 2048 units hits an edict with `takedamage` `DAMAGE_AIM` (2) that is not a
  teammate. Otherwise it picks, among the edicts with `takedamage` `DAMAGE_AIM` other than `e`
  and its teammates, the one whose centre has the largest dot product with `v_forward` from
  that start point, at least `sv_aim`, among those a trace to the centre reaches; and returns
  `v_forward` scaled by the dot product of `v_forward` with the difference of the two edicts'
  `origin`s, with its z replaced by that difference's z, normalized. With no such edict it
  returns `v_forward`. It writes nothing.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BUILTIN_MOVE_NORMAL` | 0 | `WinQuake/world.h`, `MOVE_NORMAL` |
| `BUILTIN_MOVE_NOMONSTERS` | 1 | `WinQuake/world.h`, `MOVE_NOMONSTERS` |
| `BUILTIN_MOVE_MISSILE` | 2 | `WinQuake/world.h`, `MOVE_MISSILE` |
| `BUILTIN_CHECKCLIENT_INTERVAL` | 0.1 | `PF_checkclient`: `sv.time - sv.lastchecktime >= 0.1` |
| `BUILTIN_STEPSIZE` | 18 | `WinQuake/sv_move.c`, `STEPSIZE` |
| `BUILTIN_SV_AIM_DEFAULT` | 0.93 | `PF_aim`, `cvar_t sv_aim = {"sv_aim", "0.93"}` |

## BUILTIN-15 Movement

**Source:** `PF_walkmove`, `PF_droptofloor`, `PF_changeyaw`; `WinQuake/sv_move.c`,
`SV_movestep`, `SV_MoveToGoal`, `SV_StepDirection`, `SV_NewChaseDir`, `SV_CloseEnough`;
`WinQuake/mathlib.c`, `anglemod`.

### Behaviour

- **BUILTIN-15.1** `walkmove(yaw, dist)` acts on `self`. When `self`'s `flags`, converted to an
  integer, has none of `FL_ONGROUND` (512), `FL_FLY` (1) and `FL_SWIM` (2), it returns 0 and
  changes nothing. Otherwise it tries to move `self` by `(cos(y) * dist, sin(y) * dist, 0)` with
  `y = (f32)(yaw * M_PI * 2 / 360)`, stepping up and down stairs (18 units) and refusing to walk
  off an edge, flying and swimming monsters adjusting height toward their `enemy`, and returns
  1 if it moved and 0 if not.
- **BUILTIN-15.2** A successful `walkmove` writes `origin`, may write `groundentity` and clear
  `FL_PARTIALGROUND` (EDICT-12.13, EDICT-12.18), and relinks `self` touching triggers, which
  can run their `touch` functions as nested runs (BUILTIN-1.5). After the move it restores the
  global `self` and the current function to what they were before; it does not restore
  `other` or any other global a touch function changed.
- **BUILTIN-15.3** `droptofloor()` acts on `self` and ignores its parameters (the 1.01
  prototype declares two; the rerelease's declares none). It traces `self`'s box from `origin`
  to 256 units below, as a normal move ignoring `self`. When the trace is all in solid or hits
  nothing it returns 0 and changes nothing. Otherwise it sets `origin` to the end of the trace,
  links `self` without touching triggers, sets `FL_ONGROUND` in `flags`, sets `groundentity`
  to the edict hit (the world for the map) and returns 1.
- **BUILTIN-15.4** `ChangeYaw()` turns `self`'s yaw toward `ideal_yaw` by at most `yaw_speed`,
  in f32: `current = anglemod(angles_y)`; if `current == ideal_yaw` it changes nothing.
  Otherwise `move = ideal_yaw - current`, minus 360 when `ideal_yaw > current` and
  `move >= 180`, plus 360 when `ideal_yaw <= current` and `move <= -180`; then clamped to
  `yaw_speed` when positive and to `-yaw_speed` when not; and `angles_y` becomes
  `anglemod(current + move)`.
- **BUILTIN-15.5** `anglemod(a)` is `(360.0/65536) * ((int)(a * (65536/360.0)) & 65535)` in
  double, rounded to f32: the angle is wrapped to 0 up to 360 and truncated toward zero to a
  multiple of 360/65536 degrees, so `ChangeYaw` can leave `angles_y` a little short of
  `ideal_yaw` (20 becomes 19.995117) and can change it even when it is already equal to
  `ideal_yaw` unless `ideal_yaw` is such a multiple.
- **BUILTIN-15.6** `movetogoal(step)` acts on `self` and its `goalentity`. When `self` is not on
  the ground, flying or swimming (as `walkmove` checks) it writes 0 to `RETURN` and changes
  nothing else. When `self`'s `enemy` is not the world and `self`'s `absmin` and `absmax`
  widened by `step` overlap the goal's, it does nothing. Otherwise it tries a step of `step`
  toward `ideal_yaw`, unless `rand() & 3` is 1; when it did not try or the step failed, it
  chooses a new direction toward the goal, trying the eight 45-degree headings in an order that
  also uses `rand()`, and sets `ideal_yaw` to the heading taken. Each try sets `ideal_yaw`,
  turns `self` as `ChangeYaw` does, steps as `walkmove` does without touching triggers, keeps
  the step only if `angles_y - ideal_yaw` after the turn is not strictly between 45 and 315, and
  then relinks `self` touching triggers. It writes `angles`, `ideal_yaw`, `origin`,
  `groundentity` and `flags` (EDICT-12.20).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BUILTIN_DROPTOFLOOR_DEPTH` | 256 | `PF_droptofloor`: `end[2] -= 256` |
| `BUILTIN_ANGLEMOD_STEPS` | 65536 | `WinQuake/mathlib.c`, `anglemod` |

## BUILTIN-16 Errors and debugging

**Source:** `PF_error`, `PF_objerror`, `PF_break`, `PF_coredump`, `PF_traceon`, `PF_traceoff`,
`PF_eprint`; `WinQuake/pr_edict.c`, `ED_Print`, `ED_PrintNum`, `ED_PrintEdicts`.

### Behaviour

- **BUILTIN-16.1** `error(s)` prints `======SERVER ERROR in <function>:`, a newline, the joined
  text (BUILTIN-7.3) and a newline, where `<function>` is the name of the QuakeC function that
  called it; then it prints `self` as `eprint` does and raises a host error (BUILTIN-3.2).
- **BUILTIN-16.2** `objerror(s)` prints `======OBJECT ERROR in <function>:` and the text in the
  same way, prints `self`, frees `self` (EDICT-7.5), then raises a host error. id's comment
  says "the level can continue", but the code stops the server as `error` does.
- **BUILTIN-16.3** `break()` prints the console message `break statement` and then writes to
  address -4 ("dump to debugger"), which crashes the engine. This spec fixes the answer: after
  the message it is the fatal error `break statement` (BUILTIN-3.3).
- **BUILTIN-16.4** `traceon()` and `traceoff()` set and clear the VM's statement trace flag
  (VM-2.3, VM-18.3). They change no global or field.
- **BUILTIN-16.5** `eprint(e)` prints `e`'s fields to the console as `ED_Print` formats them,
  and `coredump()` prints `num_edicts` and then every edict from 0 to
  `num_edicts - 1` the same way. `e` goes through `NUM_FOR_EDICT` (BUILTIN-2.4). The text is
  not part of this spec; they change nothing QuakeC can observe.

## BUILTIN-17 The rerelease's prototypes

**Source:** `quakec/defs.qc` in
[id-Software/quake-rerelease-qc](https://github.com/id-Software/quake-rerelease-qc) at commit
`634eefab09a77eb7b5f5ca7078ba3d8784a91142` (corpus source `quake-rerelease-qc`, sha256
`7694c006223ec43f05b923c8fe7691a6ed8298141d1562d98d6c86162de6129d`).

### Behaviour

- **BUILTIN-17.1** For every number from 1 to 78 that the rerelease's `defs.qc` declares, it
  uses the name and number of BUILTIN-5, and the same prototype except as BUILTIN-17.2 and
  BUILTIN-17.3 say. It leaves 5, 33, 39, 42, 50, 60 to 66 and 71 undeclared, as the 1.01
  `defs.qc` does.
- **BUILTIN-17.2** The rerelease declares `droptofloor` as `float()`, where 1.01 declares
  `float(float yaw, float dist)`; the builtin ignores its parameters either way (BUILTIN-15.3).
  It declares `dprint` (25) as `void(string s, ...)`, which BUILTIN-7.3 already handles.
- **BUILTIN-17.3** The rerelease declares no builtin with number 23, 24 or 73: `bprint`,
  `sprint` and `centerprint` are `#0:ex_bprint`, `#0:ex_sprint` and `#0:ex_centerprint`,
  variadic extension builtins the rerelease's engine finds by name. Its other `#0:ex_*`
  builtins and `checkextension` (`#99`) are not vanilla and are not in this spec; the engine
  target database (P2-04) covers them.

## Test vectors

### Math and strings

Checked on 2026-10-16 by a local harness holding the bodies of `PF_makevectors` (through
`AngleVectors`), `PF_normalize`, `PF_vlen`, `PF_vectoyaw`, `PF_vectoangles`, `PF_ftos`,
`PF_vtos`, `PF_rint`, `PF_floor`, `PF_ceil`, `PF_fabs`, `PF_changeyaw` and `anglemod`, copied
unchanged from `WinQuake/pr_cmds.c` and `WinQuake/mathlib.c` at the pinned commit with the
global and parameter accesses replaced by arguments and results, built with Apple clang 21 at
`-O0 -ffp-contract=off -fno-strict-aliasing` on arm64 macOS. No row converts an out-of-range
float to an integer, so they do not depend on BUILTIN-2.2. Floats are printed with 9 significant
digits.

Since P1-32 every row of this table and the next is a vector in
[`vectors/vm/math.txt`](vectors/vm/math.txt), one per input, which `cargo xtask oracles` runs
through the VM oracle on Linux, macOS and Windows in CI; on 2026-10-16 all 78 passed on macOS
arm64. The oracle runs id's `PF_` functions and `anglemod` from the pinned commit, and its own
`ftos` and `vtos` (BUILTIN-7.5 to BUILTIN-7.7). A string result is compared by `EQ_S` with the
vector's own text. `anglemod` has no builtin of its own: its rows run `ChangeYaw` with
`yaw_speed` 0 and an `ideal_yaw` the angle is not at, which stores `anglemod(anglemod(a))`, the
same value for each of these angles. The `random` row runs with the random states 2433918595 and
3889997443, whose first `rand() & 0x7fff` (DIFF-6.2) is 32767 and 1.

| Input | Output | Statements |
| --- | --- | --- |
| `ftos(0)`, `ftos(-0)`, `ftos(1)`, `ftos(-5)` | `0`, `0`, `1`, `-5` | BUILTIN-7.1 |
| `ftos(0.5)`, `ftos(1.25)`, `ftos(1.35)`, `ftos(0.05)`, `ftos(-0.04)` | `  0.5`, `  1.2`, `  1.4`, `  0.1`, ` -0.0` | BUILTIN-7.1 |
| `ftos(123456.7)`, `ftos(16777216)`, `ftos(2147483520)`, `ftos(-2147483648)` | `123456.7`, `16777216`, `2147483520`, `-2147483648` | BUILTIN-7.1 |
| `vtos('0 0 0')`, `vtos('1 -2 3.5')` | `'  0.0   0.0   0.0'`, `'  1.0  -2.0   3.5'` | BUILTIN-7.2 |
| `vtos` of three `-3.4028235e38` | 130 characters, each component `-340282346638528859811704183484516925440.0` | BUILTIN-7.5 |
| `rint` of 0, -0, 0.5, -0.5, 1.5, 2.5, -2.5, 0.49999997, -1.7, 3.2 | 0, 0, 1, -1, 2, 3, -3, 0, -2, 3 | BUILTIN-6.7 |
| `floor(-0.5)`, `ceil(-0.5)`, `ceil(1.2)`, `floor(1.8)`, `fabs(-0)` | -1, -0, 2, 1, 0 | BUILTIN-6.8 |
| `vlen('3 4 0')`, `vlen('1 1 1')`, `vlen('0 0 0')` | 5, 1.73205078, 0 | BUILTIN-6.2 |
| `normalize('3 4 0')`, `normalize('1 1 1')`, `normalize('0 0 -2')`, `normalize('0 0 0')` | `'0.600000024 0.800000012 0'`, `'0.577350259 0.577350259 0.577350259'`, `'0 0 -1'`, `'0 0 0'` | BUILTIN-6.3 |
| `vectoyaw` of `'1 0 0'`, `'0 1 0'`, `'-1 0 0'`, `'0 -1 0'`, `'1 -1 0'`, `'1 0.01 0'`, `'-1 -0.01 0'`, `'0 0 5'` | 0, 90, 180, 270, 315, 0, 181, 0 | BUILTIN-6.4 |
| `vectoangles` of `'0 0 1'`, `'0 0 -1'`, `'0 0 0'`, `'1 0 1'`, `'1 0 -1'`, `'-3 4 5'` | `'90 0 0'`, `'270 0 0'`, `'270 0 0'`, `'45 0 0'`, `'315 0 0'`, `'45 126 0'` | BUILTIN-6.5 |
| `makevectors('0 0 0')` | forward `'1 0 -0'`, right `'0 -1 -0'`, up `'0 0 1'` | BUILTIN-6.1 |
| `makevectors('0 90 0')` | forward `'-4.37113883e-08 1 -0'`, right `'1 4.37113883e-08 -0'`, up `'0 0 1'` | BUILTIN-6.1 |
| `makevectors('0 360 0')` | forward `'1 1.74845553e-07 -0'`, right `'1.74845553e-07 -1 -0'`, up `'0 0 1'` | BUILTIN-6.1 |
| `makevectors('30 45 0')` | forward `'0.612372398 0.612372398 -0.5'`, right `'0.707106769 -0.707106769 -0'`, up `'0.353553385 0.353553385 0.866025388'` | BUILTIN-6.1 |
| `makevectors('-45 180 10')` | forward `'-0.707106769 -6.18172393e-08 0.707106769'`, right `'-0.122787893 0.98480773 -0.122787803'`, up `'0.696364224 0.173648238 0.696364224'` | BUILTIN-6.1 |
| `anglemod` of 0, 20, -90, 360, 45.5, -0.001, 720.5 | 0, 19.9951172, 270, 0, 45.4998779, 0, 0.49987793 | BUILTIN-15.5 |
| `ChangeYaw`, `angles_y` 0, `ideal_yaw` 90, `yaw_speed` 20 | `angles_y` 19.9951172 | BUILTIN-15.4, BUILTIN-15.5 |
| `ChangeYaw`, `angles_y` 0, `ideal_yaw` 270, `yaw_speed` 20 | 340.004883 | BUILTIN-15.4 |
| `ChangeYaw`, `angles_y` 350, `ideal_yaw` 10, `yaw_speed` 45 | 9.99755859 | BUILTIN-15.4 |
| `ChangeYaw`, `angles_y` -30, `ideal_yaw` 0, `yaw_speed` 10 | 339.99939 | BUILTIN-15.4 |
| `ChangeYaw`, `angles_y` 90, `ideal_yaw` 90, `yaw_speed` 20 | 90, unchanged | BUILTIN-15.4 |
| `ChangeYaw`, `angles_y` 45.5, `ideal_yaw` 45.5, `yaw_speed` 0 | 45.4998779 | BUILTIN-15.5 |
| `random()` with `rand() & 0x7fff` of 32767, 1 | 1, 3.05185094e-05 | BUILTIN-6.6 |

These rows follow from BUILTIN-2.2 and BUILTIN-7.6 and were not run by the local harness,
because the local host is not x86 (an arm64 build saturates the conversion instead). The oracle
runs them on every host: its `ftos` converts with BUILTIN-2.2's rule wherever it runs, and the
`WriteAngle` and `WriteCoord` inputs are in the range of `int`. Their bytes are checked in the
unreliable datagram (`MSG_BROADCAST`, the vectors' `datagram:`).

| Input | Output | Statements |
| --- | --- | --- |
| `ftos(1e10)` | `10000000000.0` | BUILTIN-7.1, BUILTIN-2.2 |
| `ftos(2147483648)` | `2147483648.0` (an arm64 build prints `2147483647`) | BUILTIN-7.1, BUILTIN-2.2 |
| `ftos` of a NaN, an infinity, minus infinity | `  nan`, `  inf`, ` -inf` | BUILTIN-7.6 |
| `WriteAngle` of 90, 359.9, -1, -90 | bytes 64, 255, 0, 192 | BUILTIN-13.2 |
| `WriteCoord(1.06)` | short 8 (`(int)8.48`) | BUILTIN-13.2 |

### Argument checks

The vectors in [`vectors/vm/builtins.txt`](vectors/vm/builtins.txt) run id's builtins on small
programs (`builtins: engine`, format in the vectors' [README](vectors/vm/README.md)). Checked on
2026-10-16 by `cargo xtask oracles` on macOS arm64: the VM oracle, which compiles the `PF_`
functions of `WinQuake/pr_cmds.c` at the pinned commit with the checked accessors of BUILTIN-2.9
and its own `vtos`, `WriteString` and `WriteEntity`, passed all 26. In the table `e10` is edict
10 with three edicts in use, and the string table is `""` and `"ab"` (`numstrings` 4).

| Input | Output | Statements |
| --- | --- | --- |
| `setorigin(e10, '1 2 3')`, then load `e10.origin` | no error; `'1 2 3'` | EDICT-1.9, BUILTIN-2.7 |
| `setorigin` of edict 600, of the value 4, of -1 | `invalid entity` | EDICT-1.9, BUILTIN-2.7 |
| `ChangeYaw()` with `self` edict 600; `WriteByte(MSG_ONE, 0)` with `msg_entity` 4 | `invalid entity` | BUILTIN-2.7 |
| `nextent(4)`; `nextent(e10)` | `invalid entity`; `NUM_FOR_EDICT: bad pointer` | BUILTIN-2.9, BUILTIN-2.4 |
| `cvar` of offsets 0 and 3; of 4 and -1 | no error; `invalid string` | BUILTIN-2.8, BUILTIN-2.5 |
| `cvar(vtos('1.5 0 0'))` | no error | BUILTIN-2.8 |
| `precache_file(9999)` | returns 9999, no error | BUILTIN-2.9 |
| `makestatic(e1)` with `e1.model` 9999 | `invalid string` | BUILTIN-2.9 |
| `changelevel("ab")`, then `changelevel(9999)` | no error | BUILTIN-2.9 |
| `setmodel(4, 9999)` | `invalid entity` | BUILTIN-2.9 |
| `WriteString(5, 9999)`, `WriteEntity(5, 4)` | `WriteDest: bad destination` | BUILTIN-2.10 |
| `WriteString(MSG_BROADCAST, 9999)`; `WriteEntity(MSG_BROADCAST, e10)` | `invalid string`; `NUM_FOR_EDICT: bad pointer` | BUILTIN-2.10, BUILTIN-2.9 |
| `find(world, 105, "ab")`, `e1.105` 9999, `e2.105` `"ab"` | `invalid string` | BUILTIN-8.8 |
| `find(world, 105, "ab")`, `e1.105` `"ab"`, `e2.105` 9999 | `e1` | BUILTIN-8.8 |
| `find(world, 9999, "ab")` | `invalid field` | BUILTIN-8.8, BUILTIN-8.3 |
| `find(e2, 9999, "ab")`; `remove(e2)`, then `find(e1, 9999, "ab")` | the world, both times | BUILTIN-8.8 |
| `vtos` of NaN, infinity, minus infinity | `'  nan   inf  -inf'` | BUILTIN-7.7 |
| `vtos` of the NaN `ffc00000`, 1.5, -2 | `'  nan   1.5  -2.0'` | BUILTIN-7.7 |

A string value that points past the start of an engine-made string (BUILTIN-2.8) has no
vector: its number depends on where each runner puts the text.

### Numbers and prototypes

The table of BUILTIN-5 was checked on 2026-10-16 by numbering the entries of `pr_builtin` at
the pinned commit with the `QUAKE2` branch removed, which gives 79 entries with `PF_Fixme` at
0, 5, 33, 39, 42, 50, 60 to 66 and 71, and by comparing every `= #n` line of the 1.01
`defs.qc` (sha256 `6197249e...`, corpus source `quake-tools`) and the rerelease's
`quakec/defs.qc` (sha256 `7694c006...`) with it: every number matches its C function, and the
only differences between the two files are those of BUILTIN-17.2 and BUILTIN-17.3.

The rest of this spec is behaviour of the running server. It was written from the source at
the pinned commit and is checked against id's engine by the server simulation differential
suite once the engine oracle exists (P3-25); until then its vectors are the statements
themselves.
