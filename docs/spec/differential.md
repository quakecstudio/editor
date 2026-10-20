# Differential harness

Prefix: `DIFF` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how a QuakeC program is run twice, once in id's VM and once in QuakeC Studio's, so
that the two traces can be compared statement by statement: the scenario both runs read, the
deterministic mock engine both sides implement around their VM (the world and edicts at map
start, the clients, the server frames that call QuakeC, and every builtin), how a scenario
ends, when two traces count as equal, which divergences are expected, and how statement
coverage is measured from the traces.

Two programs implement this page independently. The oracle runner
([#35](https://github.com/quakecstudio/editor/issues/35), P1-11) is a C program around id's
`WinQuake/pr_exec.c`; QuakeC Studio's runner ([#37](https://github.com/quakecstudio/editor/issues/37),
P1-13) drives `qc-vm` ([#34](https://github.com/quakecstudio/editor/issues/34)) with `qc-edict`
([#33](https://github.com/quakecstudio/editor/issues/33)) and the mock engine of `qc-builtins`
([#40](https://github.com/quakecstudio/editor/issues/40), P1-16). Neither links the other's
code. Implemented by `qc-builtins` (the mock engine) and the runner (P1-13).

Most of the mock engine is a design of this project, not a port: the statements marked
**Source:** this project are decisions, made so that two independent implementations produce
the same trace and the trace exercises as much QuakeC as possible. Where the mock does what
id's server does, the section cites the function, in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, the WinQuake tree built without `QUAKE2`,
`GLQUAKE` or `IDGODS`. The trace format is [`trace.md`](trace.md); what the VM does is
[`vm.md`](vm.md); edicts, map spawning and the engine's entry points are
[`edicts.md`](edicts.md); the builtins are [`builtins.md`](builtins.md); limits are
[`limits.md`](limits.md). This page does not repeat them. The oracle's scenario tests
(`xtask/src/oracles/scenarios/`, run through the built oracle by `cargo xtask oracles`) cite
DIFF-1 to DIFF-8 and DIFF-10, and `crates/qs-tests` checks the vectors of DIFF-2.7 to
DIFF-2.9; the tests of `qs-differential` cite DIFF-9 and DIFF-11, which compare and count
traces. The tests of `qc-builtins` and `qs-differential` cite the statements they implement
as well.

"The clock" is the server clock `sv.time`, a C `double`; "(f32)x" is `x` rounded to the
nearest `f32`, as C's assignment of a `double` to a `float` rounds it.

## DIFF-1 The two runners

**Source:** this project; the VM is `WinQuake/pr_exec.c`, `PR_ExecuteProgram`.

### Behaviour

- **DIFF-1.1** A differential run executes one scenario (DIFF-2) in the oracle runner, whose
  VM is id's `PR_ExecuteProgram`, and in QuakeC Studio's runner, whose VM is `qc-vm`. Each
  writes one trace ([`trace.md`](trace.md)) of the whole scenario, and the two traces are
  compared by DIFF-9. Neither program links, calls or generates code from the other: this page
  and the specs it cites are the whole contract between them.
- **DIFF-1.2** The oracle's VM is `WinQuake/pr_exec.c` compiled unchanged, built as
  [`vm.md`](vm.md) "Reference build" requires. Everything around it is the mock engine of this
  page, written by each side on its own: the edicts, the system globals, the builtin table
  (`pr_builtins`, 79 entries), the host that calls `PR_ExecuteProgram`, and the `Host_Error`
  and `Sys_Error` that `PR_RunError` and the builtins call.
- **DIFF-1.3** Where this page or [`edicts.md`](edicts.md) and [`builtins.md`](builtins.md)
  say the mock does what an id function does, the oracle may compile that function from the
  pinned commit (for example `ED_Alloc`, `ED_Free`, `ED_LoadFromFile`, `SV_RunThink` or a
  class pure `PF_` function), and QuakeC Studio uses its own port. Where those specs fix an
  answer that id's C leaves undefined or platform-dependent (BUILTIN-2.5, BUILTIN-2.7,
  BUILTIN-2.10, BUILTIN-7.5, BUILTIN-7.6, BUILTIN-7.7, BUILTIN-8.3, BUILTIN-12.6,
  BUILTIN-16.3, EDICT-8.3), both mocks
  implement the fixed answer, so for that case the oracle does not run id's code. The float to
  integer conversion of BUILTIN-2.2 is the exception: its fixed answer is what id's code gives
  on x86, so the oracle may use C's conversion. `pr_exec.c` is never changed, so the VM's own
  undefined cases stay id's and are expected divergences (DIFF-10.1).
- **DIFF-1.4** A run is deterministic: its trace depends only on the scenario's files. It
  reads no clock, environment variable, system random source or file the scenario does not
  name, and running the same scenario twice in the same program writes the same bytes.
- **DIFF-1.5** The mock engine stands in for id's server; it does not model it. Nothing moves
  and nothing collides (DIFF-5.8, DIFF-7), so a scenario shows that the two VMs execute the
  same QuakeC the same way, not that a mod behaves as it does in Quake. The server simulation
  (`qs-sim`, [#238](https://github.com/quakecstudio/editor/issues/238)) is checked against id's
  engine by its own suite ([#239](https://github.com/quakecstudio/editor/issues/239)).

## DIFF-2 Scenarios

**Source:** this project.

A scenario is a small text file naming a progs, a map's entities and the settings of one run.
The runner of P1-13 writes scenarios for each corpus codebase; both runners read them.

### Behaviour

- **DIFF-2.1** A scenario file is lines of bytes ended by a line feed, or a carriage return and
  a line feed. A `#` and everything after it on the line is a comment. A line that is empty
  after removing the comment, spaces and tabs is skipped. Every other line is a directive: a
  key, then its arguments, separated by one or more spaces or tabs. A path is relative to the
  directory holding the scenario file and cannot contain a space, a tab or a `#`.
- **DIFF-2.2** The directives are:

  | Key | Arguments | Default | Meaning |
  | --- | --- | --- | --- |
  | `progs` | path | none: required | The progs.dat to run |
  | `entities` | path | the one entity `{ "classname" "worldspawn" }` | The map's entities (DIFF-2.5) |
  | `map` | name | `test` (`DIFF_DEFAULT_MAP`) | The map's name, 1 to 63 bytes from `a` to `z`, `0` to `9` and `_` |
  | `maxclients` | integer 1 to 16 | 1 | The number of client slots (EDICT-5.1) |
  | `clients` | integer 0 to `maxclients` | 0 | How many players enter (DIFF-4) |
  | `submodels` | integer 0 to 254 | 0 | How many brush models the map has: `*1` to `*n` |
  | `model` | name, then six numbers | | The bounds of a model: mins x y z, then maxs x y z (DIFF-6.3) |
  | `cvar` | name, text | | A setting and its text (DIFF-2.4) |
  | `frames` | integer 0 to 1000000 | 100 (`DIFF_DEFAULT_FRAMES`) | How many server frames run after the clients enter |
  | `frametime` | seconds, greater than 0 and at most 1 | 0.1 (`DIFF_DEFAULT_FRAMETIME`) | The length of each server frame |
  | `seed` | integer 0 to 4294967295 | 1 (`DIFF_DEFAULT_SEED`) | The random state (DIFF-6.2) |
  | `input` | frame, edict, field, values | | A client input (DIFF-4.3) |

  `model`, `cvar` and `input` may appear any number of times; every other key at most once.
- **DIFF-2.3** **Withdrawn** 2026-10-16: it did not say which texts are numbers, so the two
  runners could read one scenario differently: C's `strtod` and Rust's `f64` parsing both
  accept `+0.1` and `inf`, and `strtod` accepts hexadecimal; replaced by DIFF-2.7 (integers),
  DIFF-2.8 (numbers) and DIFF-2.9 (refusal).
- **DIFF-2.4** The mock's settings are `skill`, `deathmatch`, `coop` and `teamplay`, with the
  texts `1`, `0`, `0` and `0`, and each name a `cvar` line gives; a `cvar` line for one of the
  four replaces its text, and a later line for a name replaces an earlier one. A setting's
  value is its text read by `Q_atof` (BUILTIN-11.7). There are no other settings: `cvar` of
  any other name returns 0 and `cvar_set` of it changes nothing (BUILTIN-11.8).
- **DIFF-2.5** The entities file holds the text of a BSP entity lump, read as
  `ED_LoadFromFile` reads it (EDICT-8), byte for byte, with a NUL after its last byte. How the
  runner makes it, for example from a `.map` file with the brushes removed, is not part of this
  spec: both runners read the same file.
- **DIFF-2.6** An `input` line is `input <frame> <edict> <field> <values>`: `<frame>` from 1 to
  `frames`; `<edict>` from 1 to `clients`; `<field>` one of `button0` and `button2` with one
  value, 0 or 1, `impulse` with one integer from 1 to 255, or `v_angle` with three numbers.
- **DIFF-2.7** An integer (the arguments DIFF-2.2 and DIFF-2.6 call integers, and the values of
  `button0`, `button2` and `impulse`) is an optional `-` followed by one or more of the digits
  `0` to `9`, and nothing else: no `+`, no `.`, no exponent. Its value is the digits read in
  base 10, leading zeros included, and a value outside the argument's range is refused.
- **DIFF-2.8** A number (the argument of `frametime`, the six of `model` and the three of
  `v_angle`) is, in order: an optional `-`; one or more digits, with at most one `.` before,
  among or after them; and optionally an exponent, `e` or `E`, then an optional `+` or `-`,
  then one or more digits. Nothing else is a number: not a leading `+`, `inf`, `infinity` or
  `nan` in any case, hexadecimal, or a `.` without a digit. `frametime` is the `double`
  nearest the exact decimal value of the text, and a `model` or `v_angle` number the `f32`
  nearest it, ties to even, each rounded once (never through `double` on the way to `f32`); a
  value whose magnitude rounds below the smallest subnormal is a zero with the text's sign.
  A `model` or `v_angle` number whose nearest `f32` is infinite is refused, so every number a
  scenario gives is finite; `frametime` is refused unless its `double` is greater than 0 and
  at most 1.
- **DIFF-2.9** A scenario with an unknown key, a missing or extra argument, a value that is
  not an integer or a number as DIFF-2.7 and DIFF-2.8 say or is out of range, a repeated key
  that may appear once, a model name repeated, or no `progs`, is refused before anything runs,
  and no trace is written.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `DIFF_DEFAULT_MAP` | `test` | This spec |
| `DIFF_DEFAULT_FRAMES` | 100 | This spec: ten seconds at the default frame length |
| `DIFF_DEFAULT_FRAMETIME` | 0.1 (a `double`) | This spec; the length of the settle frames, `EDICT_SETTLE_FRAMETIME` |
| `DIFF_DEFAULT_SEED` | 1 | This spec |

### Example

```text
# id 1.01 on a LibreQuake start map, one player who fires and picks up every weapon.
progs     progs.dat
entities  start.ent
map       start
submodels 12
cvar      skill 2
clients   1
frames    300
input     10  1 impulse 9
input     20  1 button0 1
input     40  1 button0 0
input     50  1 v_angle 0 90 0
```

## DIFF-3 Setting up the map

**Source:** `WinQuake/sv_main.c`, `SV_SpawnServer`; `WinQuake/pr_edict.c`, `PR_LoadProgs`,
`ED_LoadFromFile`; `WinQuake/host.c`, `Host_ClearMemory`.

The mock performs the steps of `SV_SpawnServer` that QuakeC can observe, in its order, with
the map's BSP replaced by the scenario.

### Behaviour

- **DIFF-3.1** The runner loads the progs by [`progs.md`](progs.md) with the header CRC
  checked (PROGS-3). A progs that is refused, or that has fewer than 92 global slots or 105
  field slots (EDICT-2.4, EDICT-3.3), ends the scenario with that error before the trace
  header: no trace is written.
- **DIFF-3.2** The runner then writes the trace header (TRACE-1.3, TRACE-2.1), and sets the
  engine state: the setting `deathmatch` becomes `0` when the value of `coop` is non-zero; the
  skill is `(int)(skill + 0.5)` clamped to 0 to 3 and becomes the value of `skill`; the 600
  edicts are all zero with none free and `freetime` 0 (EDICT-1.8); `num_edicts` is
  `maxclients + 1`; the clock is 1.0; the server is loading; no `changelevel` has been issued;
  no client slot is active; the 64 lightstyles hold no string; the three message buffers of
  DIFF-6.5 are empty; `checkclient`'s last choice and its time are 0 (DIFF-7.3); and the random
  state is the seed (DIFF-6.2).
- **DIFF-3.3** The sound precache list holds only entry 0, the empty string. The model precache
  list holds entry 0, the empty string; entry 1, `maps/<map>.bsp`; and entries 2 to
  `submodels + 1`, the names `*1` to `*<submodels>` (BUILTIN-10.3).
- **DIFF-3.4** The world's fields are set by EDICT-4.1, its `model` an engine string with the
  text `maps/<map>.bsp`. Then the globals of EDICT-11.4: `coop` to the value of the `coop`
  setting when it is non-zero, and otherwise `deathmatch` to the value of the `deathmatch`
  setting; `mapname` to an engine string with the text `<map>`; `serverflags` to 0. These
  writes, and `time` set to 1 by EDICT-8.1, appear in the change set of the trace's first run
  record (TRACE-4.1), after the string records of their engine strings (TRACE-4.10).
- **DIFF-3.5** The map's entities are spawned from the entities file by EDICT-8, using the
  value of the `deathmatch` setting and the skill of DIFF-3.2 for EDICT-8.4. Each spawn
  function is a run. The messages EDICT-8 prints are console output and are not compared.
- **DIFF-3.6** The server then becomes active and runs the two settle frames of EDICT-8.9 as
  physics frames of 0.1 seconds (DIFF-5.3), without setting `frametime`, emptying a buffer or
  clearing muzzle flashes. Then the clients enter (DIFF-4), then `frames` server frames run
  (DIFF-5.1), and the scenario ends.
- **DIFF-3.7** A scenario that ends normally runs no more QuakeC: no `ClientDisconnect`, no
  `SetChangeParms`. What the host writes after the trace's last record, such as the last
  frame's muzzle-flash clearing, is in no change set and is not compared.
- **DIFF-3.8** Each engine string (EDICT-13.1) gets a value outside the string table, by
  EDICT-13.3, and a string record when its text is set (TRACE-4.10). Its number is each
  runner's own; DIFF-9.4 compares the text.
- **DIFF-3.9** DIFF-3.8 holds for the strings a builtin sets as for the host's: `ftos` and
  `vtos` write their one buffer (BUILTIN-7.4) and return its value, so each call whose text
  differs from that value's latest string record writes a string record before the `CALL`'s
  step record, whose change set first shows the value in `OFS_RETURN`. A call that sets the
  same text again writes none, as the oracle's `qs_emit_string_records` skips an unchanged
  text (`oracles/vm/host.c`). Without the record the value has no text in its trace
  (TRACE-7.3), and DIFF-9.4 compares the two runners' different numbers as bits.

## DIFF-4 Clients

**Source:** `WinQuake/sv_main.c`, `SV_ConnectClient`; `WinQuake/host_cmd.c`, `Host_Spawn_f`,
`Host_Begin_f`; `WinQuake/sv_user.c`, `SV_ReadClientMove`.

In id's engine a player connects over the network after the map has started. The mock lets
`clients` players enter at once, after the settle frames, so that the client code of a progs
runs; it reads no network input other than the scenario's `input` lines.

### Behaviour

- **DIFF-4.1** Client slots 0 to `clients - 1` enter in order. For each, as `SV_ConnectClient`
  does: run `SetNewParms`, with nothing set before it (EDICT-9.1), then copy the globals
  `parm1` to `parm16` into the slot's spawn parms. Then, as `Host_Spawn_f` does: set every
  field of the slot's edict to 0 without changing its free flag; set `colormap` to the edict's
  number, `team` to 1 (the colours are 0, EDICT-5.2) and `netname` to an engine string with
  the text `player` (`DIFF_CLIENT_NAME`); copy the spawn parms to `parm1` to `parm16`; set
  `time` to (f32)clock and `self` to the edict; run `ClientConnect`; then run
  `PutClientInServer` with nothing set between the two.
- **DIFF-4.2** A slot whose player has entered is active and spawned from then on: it counts
  as connected for the builtins (BUILTIN-11, BUILTIN-12.7), the physics frame (DIFF-5.4) and
  `checkclient` (DIFF-7.3). The other slots never have a player.
- **DIFF-4.3** At the start of server frame `k` (DIFF-5.1), the `input` lines for frame `k`
  are applied in file order to their edict, as `SV_ReadClientMove` stores a move: `button0` and
  `button2` set that field to the value, `impulse` sets `impulse` to the value, and `v_angle`
  sets its three slots to the three values. Nothing else of client input runs: no
  `SV_ClientThink`, so no field other than these is written.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `DIFF_CLIENT_NAME` | `player` | This spec |

## DIFF-5 Server frames

**Source:** `WinQuake/host.c`, `Host_ServerFrame` (the branch without `FPS_20`);
`WinQuake/sv_phys.c`, `SV_Physics`, `SV_Physics_Client`, `SV_Physics_Pusher`, `SV_PushMove`,
`SV_RunThink`, `SV_Physics_Toss`; `WinQuake/sv_main.c`, `SV_ClearDatagram`,
`SV_SendClientMessages`, `SV_UpdateToReliableMessages`, `SV_CleanupEnts`; `WinQuake/world.c`,
`SV_LinkEdict`.

The mock keeps every call into QuakeC that a frame of id's server makes without a map (the
order of [`edicts.md`](edicts.md) EDICT-9 and EDICT-10) and drops the movement.

### Behaviour

- **DIFF-5.1** A server frame of length `T` (the `frametime` directive): sets the global
  `frametime` to (f32)`T`; empties the unreliable datagram; applies the frame's input
  (DIFF-4.3); runs a physics frame of length `T` (DIFF-5.3), never paused; empties the reliable
  datagram; and clears `EF_MUZZLEFLASH` in `effects` on edicts 1 to `num_edicts - 1`
  (EDICT-10.7), converting `effects` to an integer and back (BUILTIN-2.1, BUILTIN-2.2).
- **DIFF-5.2** Server frames are numbered from 1. Frame `k` starts with the clock at 1.0 plus
  0.1 twice (the settle frames) plus `T` `k - 1` times, each addition made in `double` at the
  end of a physics frame (DIFF-5.3), so about 1.2 when frame 1 starts.
- **DIFF-5.3** A physics frame of length `T`, as `SV_Physics`: set `self` and `other` to the
  world and `time` to (f32)clock, and run `StartFrame`. Then for `i` from 0 while `i` is less
  than `num_edicts`, read again after each edict: skip edict `i` if it is free; link it
  (DIFF-5.7) if `force_retouch` is not 0; then give it its turn: an edict from 1 to
  `maxclients` by DIFF-5.4; otherwise one whose `movetype` equals 7 (`MOVETYPE_PUSH`) by
  DIFF-5.5; otherwise one whose `movetype` equals 0, 8, 4, 6, 10, 5 or 9 by thinking
  (DIFF-5.6) and nothing else; otherwise the fatal error `SV_Physics: bad movetype <n>`, `<n>`
  the integer conversion of `movetype`. The comparisons are of floats, so a `movetype` of 7.5
  is a bad movetype. After the last edict, when `force_retouch` is not 0 it is decreased by 1
  in `f32`; then the clock advances by `T`.
- **DIFF-5.4** A client edict's turn does nothing when its slot has no player. Otherwise, as
  `SV_Physics_Client` without the move: set `time` to (f32)clock and `self` to the edict and run
  `PlayerPreThink`. Then, by the integer conversion of `movetype`: 0, 3, 5 or 8 thinks
  (DIFF-5.6) and ends the turn if the edict is then free; 6 or 10 thinks and goes on even if
  the edict is then free, because `SV_Physics_Toss` returns into the turn; any other value is
  the fatal error `SV_Physics_client: bad movetype <n>`. Then link the edict (DIFF-5.7), set
  `time` to (f32)clock and `self` to the edict, and run `PlayerPostThink`.
- **DIFF-5.5** A `MOVETYPE_PUSH` edict's turn, the world's included, is `SV_Physics_Pusher` with
  `SV_PushMove` reduced to its branch for a pusher with zero velocity, whatever the velocity
  is: in `f32`, `oldltime` is `ltime` and `thinktime` is `nextthink`; if `thinktime` is less
  than `ltime + T` (in `double`), `movetime` is `thinktime - ltime`, or 0 if that is negative,
  and otherwise (f32)`T`; if `movetime` is not 0, `ltime` becomes `ltime + movetime`. Then if
  `thinktime` is greater than `oldltime` and not greater than `ltime`, set `nextthink` to 0,
  `time` to (f32)clock, `self` to the edict and `other` to the world, and run `think`. Nothing
  moves or is pushed, and `solid` is not changed.
- **DIFF-5.6** Thinking is `SV_RunThink` (EDICT-10.3): when `nextthink` is greater than 0 and
  not greater than `clock + T` (in `double`), set `time` to `nextthink`, or to (f32)clock if
  `nextthink` is less than the clock, then `nextthink` to 0, `self` to the edict and `other` to
  the world, and run `think`. A `nextthink` of exactly (f32)`(clock + T)` can be just above
  `clock + T` and then waits a frame.
- **DIFF-5.7** Linking an edict, from a turn or a builtin, is `SV_LinkEdict` without the area
  tree: for an edict that is not the world and not free it writes `absmin` and `absmax` by
  EDICT-12.2, and it runs no `touch` function, even where id's call would touch triggers.
- **DIFF-5.8** The mock frame leaves out everything that needs the map or another entity's
  box: client movement (`SV_ClientThink`), gravity, `SV_CheckVelocity`, water checks, the
  moves of every movetype, pushing and blocking, trigger and impact touches and `blocked`
  functions, and the client updates. So `origin`, `velocity`, `angles`, `groundentity`,
  `flags`, `waterlevel` and the other fields of EDICT-12 that physics writes change only when
  QuakeC or a builtin writes them; `touch` and `blocked` are never called; and nothing runs
  `ClientKill`, `SetChangeParms` or `ClientDisconnect` except DIFF-8.3.

## DIFF-6 Mock builtins

**Source:** this project, with the builtins of [`builtins.md`](builtins.md);
`WinQuake/pr_cmds.c`, `PF_random`; `WinQuake/sv_main.c`, `SV_StartSound`,
`SV_StartParticle`; `WinQuake/common.c`, `SZ_GetSpace`.

### Behaviour

- **DIFF-6.1** The mock's builtin table has the numbers 0 to 78 of BUILTIN-5. Builtins of
  class pure, state and output (BUILTIN-4) behave as [`builtins.md`](builtins.md) says, with
  the engine state of DIFF-2 to DIFF-5 and DIFF-6.3 to DIFF-6.5 as the fixture BUILTIN-4.2
  names; the class world builtins behave as DIFF-7 says; the numbers with no builtin fail as
  BUILTIN-1.2 says.
- **DIFF-6.2** The C library's `rand` is replaced by a 32-bit unsigned state, which starts at
  the scenario's seed: each call sets the state to `state * 1103515245 + 12345` modulo 2^32
  and returns `(state >> 16) & 0x7fff`. `random` (BUILTIN-6.6) is the only mock builtin that
  calls it, once per call.
- **DIFF-6.3** `precache_model` never fails to load a model (BUILTIN-10.4's fatal error does
  not happen). The bounds of a model list entry, for `setmodel` (BUILTIN-9.4), are: for entry
  0, `'0 0 0'` and `'0 0 0'`; for a name given by a `model` directive, its bounds; for any
  other name ending in the four bytes `.mdl`, `'-16 -16 -16'` and `'16 16 16'`; and for every
  other name, the world, the submodels and sprites among them, `'0 0 0'` and `'0 0 0'`.
- **DIFF-6.4** The command buffer that `localcmd` and `changelevel` append to (BUILTIN-11.5,
  BUILTIN-11.6) is never run: no command executes and the map never changes. `changelevel`
  still records that it was issued.
- **DIFF-6.5** The mock keeps the size in bytes of three buffers and nothing else of the
  output: the unreliable datagram (`MSG_BROADCAST`, `sound`, `particle`), the reliable datagram
  (`MSG_ALL`), each of `LIMIT_MAX_DATAGRAM` bytes, and the signon buffer (`MSG_INIT`,
  `ambientsound`, `makestatic`) of `BUILTIN_SIGNON_SIZE` bytes. It counts what BUILTIN-11 to
  BUILTIN-13 append, so it drops a sound or particle and fails with
  `SZ_GetSpace: overflow without allowoverflow set` where id's buffers would (BUILTIN-12.2,
  BUILTIN-12.5, BUILTIN-13.3). A client slot's reliable message is not kept, never overflows
  and never drops the client. No output, console text included, is compared.
- **DIFF-6.6** The errors that builtins.md fixes where id's C is undefined are runtime errors
  (VM-19.4) with these messages in the mock, so both mocks write the same error record:
  `invalid entity` (BUILTIN-2.7), `invalid string` (BUILTIN-2.5), `invalid field`
  (BUILTIN-8.3) and `invalid lightstyle` (BUILTIN-12.6).
- **DIFF-6.7** No mock builtin starts a nested run: `walkmove` and `movetogoal` touch nothing
  (DIFF-7.7, DIFF-7.8). `traceon` and `traceoff` change only the trace flag (BUILTIN-16.4),
  which does not change what a runner writes to the trace.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `DIFF_RAND_MULTIPLIER` | 1103515245 | This spec |
| `DIFF_RAND_INCREMENT` | 12345 | This spec |

## DIFF-7 Mock world builtins

**Source:** this project; the arguments, globals and fields are those of `WinQuake/pr_cmds.c`,
`PF_traceline`, `PF_checkclient`, `PF_newcheckclient`, `PF_checkbottom`, `PF_pointcontents`,
`PF_aim`, `PF_droptofloor`, `PF_walkmove`, and `WinQuake/sv_move.c`, `SV_MoveToGoal`,
`SV_StepDirection`.

The class world builtins (BUILTIN-4.4) answer as if the map were empty space with a floor
just under every entity: no trace hits anything, every point is visible from every other,
and every step succeeds. Each reads its entity arguments, and `self` where id's reads it,
with BUILTIN-2.7's check (BUILTIN-2.9), except that `checkclient` does not read `self`
(DIFF-7.3).

### Behaviour

- **DIFF-7.1** `traceline(v1, v2, nomonsters, forent)` writes `trace_allsolid` 0,
  `trace_startsolid` 0, `trace_fraction` 1, `trace_endpos` a copy of the three slots of PARM1,
  `trace_plane_normal` `'0 0 0'`, `trace_plane_dist` 0, `trace_ent` the world,
  `trace_inopen` 1 and `trace_inwater` 0, and nothing else.
- **DIFF-7.2** `pointcontents(v)` returns -1 (`CONTENTS_EMPTY`).
- **DIFF-7.3** `checkclient()` chooses as `PF_checkclient` and `PF_newcheckclient` do, with no
  PVS: when the clock minus the time of the last choice (both `double`, 0 at map start) is at
  least 0.1 (`BUILTIN_CHECKCLIENT_INTERVAL`), it takes `check`, the last choice clamped to 1
  to `maxclients`, and looks at edicts from `check + 1` (from 1 when `check` is `maxclients`),
  wrapping from `maxclients + 1` to 1 and stopping at `check`, for the first that is not
  free, has `health` above 0 and has no `FL_NOTARGET` (128) in its `flags` converted to an
  integer; the choice is that edict, or `check` when none qualifies, and its time is the
  clock. It returns the world when the chosen edict is free or its `health` is 0 or less, and
  the chosen edict otherwise. It does not read `self`.
- **DIFF-7.4** `checkbottom(e)` returns 1. `aim(e, speed)` returns a copy of the three slots of
  `v_forward`.
- **DIFF-7.5** `droptofloor()` leaves `self`'s `origin` as it is, links `self` (DIFF-5.7), sets
  `flags` to the integer conversion of `flags` with `FL_ONGROUND` (512) set, converted back
  to a float, sets `groundentity` to the world, and returns 1.
- **DIFF-7.6** The step of `walkmove` and `movetogoal` along a yaw `a` by a distance `d` is:
  `y = (f32)(a * M_PI * 2 / 360)` computed in `double`; `move_x = (f32)(cos(y) * d)` and
  `move_y = (f32)(sin(y) * d)`, in `double`; and `origin_x` and `origin_y` of `self` become
  `origin_x + move_x` and `origin_y + move_y` in `f32`. `origin_z` is not written.
- **DIFF-7.7** `walkmove(yaw, dist)` returns 0 and changes nothing when `self`'s `flags`,
  converted to an integer, has none of `FL_ONGROUND` (512), `FL_FLY` (1) and `FL_SWIM` (2).
  Otherwise it steps along `yaw` by `dist` (DIFF-7.6), links `self` (DIFF-5.7) and returns 1.
  It writes no other field.
- **DIFF-7.8** `movetogoal(step)` writes 0 to `RETURN` and changes nothing else when `self`'s
  `flags` has none of the three flags of DIFF-7.7. Otherwise it turns `self` as `ChangeYaw`
  does (BUILTIN-15.4); computes the step along `ideal_yaw` by `step` (DIFF-7.6), applied only
  when `angles_y - ideal_yaw` (`f32`) is not strictly between 45 and 315; links `self`
  (DIFF-5.7); and writes nothing to `RETURN`. It does not read `goalentity` or `enemy`, and
  does not call `rand`.

## DIFF-8 Errors and the end of a scenario

**Source:** `WinQuake/pr_exec.c`, `PR_RunError`, `PR_ExecuteProgram`; `WinQuake/host.c`,
`Host_Error`, `Host_ShutdownServer`, `SV_DropClient`, `Host_Shutdown`;
`WinQuake/sys_linux.c`, `Sys_Error`; `WinQuake/pr_cmds.c`, `PF_error`, `PF_objerror`.

### Behaviour

- **DIFF-8.1** Every error writes an error record (TRACE-4.6): a runtime error (BUILTIN-3.1), a
  host error (BUILTIN-3.2, and VM-2.1 raised by `PR_ExecuteProgram`) and a fatal error
  (BUILTIN-3.3). For an error raised by a statement or by the builtin it called, the function
  and statement are TRACE-4.7's, which for a builtin is the `CALL`. For an error the host
  raises outside any run (DIFF-5.3's and DIFF-5.4's bad movetype, EDICT-8.8's parse errors,
  `ED_ParseEdict: parse error`, `ED_Alloc: no free edicts` while spawning the map), the
  function is 0 and the statement is -1.
- **DIFF-8.2** The message is the error's text: for a runtime error, the message `PR_RunError`
  is given (TRACE-4.8); for `error` and `objerror`, the joined text of BUILTIN-7.3, not id's
  `Program error`; for VM-2.1, `PR_ExecuteProgram: NULL function`; for a fatal error, the text
  `Sys_Error` is given.
- **DIFF-8.3** After a runtime or host error, when the server is active (from the settle frames
  on), the host shuts the server down as `Host_ShutdownServer` does: for each client slot in
  order whose player has entered, it saves `self`, sets `self` to the slot's edict, runs
  `ClientDisconnect` and restores `self` (EDICT-9.2). Then the scenario ends. While the map is
  loading the server is not active, and the scenario ends after the error record.
- **DIFF-8.4** The shutdown's runs start where the error left the VM. After a runtime error the
  call depth is 0 (VM-19.1); after a host error it is still the depth at the error, since
  neither `Host_Error` nor `PF_error` resets it (BUILTIN-3.2, VM-2.1). Either way the local
  stack keeps the slots the aborted functions saved (VM-19.3). Each `ClientDisconnect` run
  takes that depth as its exit depth (VM-2.3), so after `error` called 30 calls deep its first
  QC call reaches `LIMIT_MAX_STACK_DEPTH` and fails with `stack overflow`.
- **DIFF-8.5** An error during the shutdown writes its error record and ends the scenario at
  once: id's `Host_Error` is then entered a second time and calls
  `Sys_Error ("Host_Error: recursively entered")`, so the later slots' `ClientDisconnect` does
  not run.
- **DIFF-8.6** A fatal error writes its error record and ends the scenario with no shutdown:
  `Sys_Error` prints, calls `Host_Shutdown`, which runs no QuakeC, and exits.

## DIFF-9 Comparing two traces

**Source:** this project; the rules for reading values are [`trace.md`](trace.md) TRACE-7.

[`trace.md`](trace.md) left open when two traces count as equal. Values in the trace are raw
32-bit patterns and the trace does not say which type a slot holds (PROGS-12.2), while entity
and pointer values (EDICT-1.3) and engine string values (EDICT-13.3) differ between runners.
So a pair of values is compared under every reading that could make them agree.

### Behaviour

- **DIFF-9.1** Two traces of one scenario are equal when their headers have the same version,
  `progs_crc`, `entityfields` and `numstrings` (`edict_size` and `field_offset` may differ),
  they have the same number of change-carrying records (TRACE-3), and each pair of
  corresponding change-carrying records, the `n`th of each, agrees by DIFF-9.2 and DIFF-9.3.
  String records are not paired with each other; they are used only to read values
  (TRACE-7.3).
- **DIFF-9.2** Corresponding records have the same tag, and: run records the same function;
  step records the same function, statement, opcode and builtin number, or both no builtin;
  error records the same function, statement and message, byte for byte.
- **DIFF-9.3** After each pair, every global slot and every field (edict number and field slot)
  listed in either record's change set has equivalent values in the two traces (DIFF-9.4). A
  trace's value of a slot at a record is the value in the latest change set up to and
  including that record that lists the slot; if none does, it is the progs file's value for a
  global (PROGS-12.1) and 0 for a field (TRACE-3.4).
- **DIFF-9.4** A value `a` of trace A and `b` of trace B are equivalent by the first rule that
  applies: if each is an engine string of its trace (a value from outside 0 to
  `numstrings - 1` with a string record before the record being compared, TRACE-7.3), they are
  equivalent exactly when the latest texts are the same bytes; otherwise they are equivalent
  when their bits are equal, when both are NaNs (VM-4.3), when both name the same edict `n`
  below `LIMIT_MAX_EDICTS` by TRACE-7.1, or when both name the same field slot of the same edict
  `n` below `LIMIT_MAX_EDICTS` by TRACE-7.2, each through its own trace's header.
- **DIFF-9.5** The first divergence is the first pair of corresponding records that fails
  DIFF-9.2 or DIFF-9.3, or, when one trace has fewer change-carrying records, the first record
  the shorter one lacks. Comparison stops there. A divergence in a run record's change set is a
  difference in the host's own writes, which are the mock engine's (TRACE-4.1), not the VM's.
- **DIFF-9.6** A divergence that DIFF-10.1 lists is reported as expected; any other is a
  failure.

The readings can accept a pair whose meanings differ when the numbers line up by chance: two
string offsets that happen to read as the same edict number through two different edict sizes.
That needs a defect that moves a value by exactly the difference of the two layouts, and the
cost of the other choice, reporting every string offset that is a multiple of an edict size
as a divergence, is higher. When both headers have the same `edict_size` and `field_offset`,
the entity and pointer readings agree exactly when the bits do, and only the string and NaN
rules remain.

## DIFF-10 Expected divergences

**Source:** this project; each row cites the statement that makes the divergence deliberate.

### Behaviour

- **DIFF-10.1** These are the expected divergences between the oracle and QuakeC Studio:

  | Statements | Where it shows | What the runner does |
  | --- | --- | --- |
  | VM-1.4, VM-2.2, VM-2.4, VM-3.3, VM-7.3, VM-11.4, VM-12.5, VM-14.3, VM-15.6 (VM-19.4) | QuakeC Studio's trace has an error record where id's C is undefined; the oracle may crash, write elsewhere or go on | Reports the scenario as expected when the traces agree up to that error record; nothing after it is compared |
  | PROGS-15.6, EDICT-2.4, EDICT-3.3 | QuakeC Studio refuses a progs that id's loader accepts (DIFF-3.1) | Reports the progs as refused; there is nothing to compare |
  | VM-9.2, BUILTIN-2.2 | An oracle on a host that is not x86 converts a NaN, an infinity or an out-of-range float to an integer differently (arm64 saturates) | Expected at a `BITAND` or `BITOR` step, or a `CALL` to a builtin that converts a float, only when the oracle's host is not x86; the reference oracle runs on x86-64 (P1-11) |
  | BUILTIN-6.9 | `sin`, `cos` and `atan2` differ in the last bit between C libraries | Expected in the values written by `makevectors`, `vectoyaw`, `vectoangles`, `walkmove` and `movetogoal` when the two runners use different C libraries |

- **DIFF-10.2** NaN payloads (VM-4.3), the edict header size (EDICT-1.3) and engine string
  numbers (EDICT-13.3) are not divergences: DIFF-9.4 compares through them.
- **DIFF-10.3** No other divergence is expected. One that DIFF-10.1 does not list is a defect in
  `qc-vm`, `qc-edict`, the mock engine or the oracle runner, and is fixed there. A row is added
  only with a statement elsewhere in `docs/spec/` that makes the divergence deliberate and gives
  its reason ([`../porting.md`](../porting.md)).

## DIFF-11 Statement coverage

**Source:** this project; statement 0 is reserved (PROGS-1.5, VM-3.5).

The hard bar of [`../testing.md`](../testing.md) (TEST-6) for this suite is zero divergences
on each corpus codebase; statement coverage is measured and reported, not gated against a flat
90%, because the mock engine does not drive the running world (DIFF-5.8, DIFF-1.5), so most of a
Quake progs is unreachable under it. Running-world statement coverage is `qs-sim`'s, measured by
its own suite ([#239](https://github.com/quakecstudio/editor/issues/239)). This is
[ADR-0013](../adr/0013-differential-coverage-bar.md). DIFF-11.1 to DIFF-11.3 are how the runner
measures and reports coverage.

### Behaviour

- **DIFF-11.1** A statement is executed in a trace when a step record has its index. The
  statements of a progs are 1 to `numstatements - 1`.
- **DIFF-11.2** The statement coverage of a progs over a set of scenarios is the number of
  distinct statements executed in any of their traces divided by `numstatements - 1`. It is
  measured on QuakeC Studio's traces, counting only the records before the first divergence of
  each, so a statement counts only once both VMs agree on it.
- **DIFF-11.3** For the report, a QC function's statements are those from its
  `first_statement` up to, not including, the next larger `first_statement` of a QC function
  (`first_statement` above 0), or `numstatements` for the last; a statement before the first
  QC function belongs to none.

## Test vectors

### Scenario numbers

id's code is silent here: the scenario format is this project's, so DIFF-2.7 and DIFF-2.8 are
our decision. They take the decimal forms that C's `strtod` and `strtof` and Rust's `f64`
and `f32` parsing read alike, and refuse what only one of them reads (hexadecimal, for
`strtod`) or what no runner should need: a leading `+`, `inf` and `nan`, and a value that
rounds to an infinite `f32`, so that every number in a trace's inputs is finite. `.1` and `5.`
stay numbers because both libraries read them the same way and the oracle has accepted them
since P1-11. Each runner checks the syntax first, then converts with its library.

[`vectors/scenario/numbers.txt`](vectors/scenario/numbers.txt) has 107 vectors, each a text
in the place of a `frames`, `frametime`, `model` or `v_angle` argument with the value it is
read as or `refused`; its [README](vectors/scenario/README.md) has the format. `cargo xtask
oracles` runs each through the VM oracle in CI, which must refuse exactly the `refused` ones;
`scenario_numbers` in `crates/qs-tests` checks the verdicts and the values from Rust. On
2026-10-16 all 107 passed through the oracle on macOS arm64. The edges:

| Text | Read as | Statement |
| --- | --- | --- |
| `frames 0100`, `frames -0` | 100, 0 | DIFF-2.7 |
| `frames 0000000000000000001`, `frames -0000000000000000000` | 1, 0: leading zeros, however many | DIFF-2.7 |
| `frames +1`, `frames 1.0`, `frames 1e2` | refused | DIFF-2.7 |
| `frametime .1`, `frametime 100e-3` | the `double` nearest 0.1, `0x3fb999999999999a` | DIFF-2.8 |
| `frametime 1.0000000000000001` | 1 (the nearest `double`), so accepted | DIFF-2.8 |
| `frametime 1.0000000000000002`, `frametime 1e-400` | refused: above 1, and rounds to 0 | DIFF-2.8 |
| `+0.1`, `inf`, `nan`, `0x1p-4`, `.`, `1e`, `1e1.5` | refused | DIFF-2.8 |
| `model 1.000000059604644775390625001` | `0x3f800001`: rounded once; through `double` it would be `0x3f800000` | DIFF-2.8 |
| `model 340282356779733661637539395458142568447` | `0x7f7fffff`, the largest `f32` | DIFF-2.8 |
| `model 340282356779733661637539395458142568448`, `model 1e39` | refused: the nearest `f32` is infinite | DIFF-2.8 |
| `model 1e-46`, `model -1e-46` | `0x00000000`, `0x80000000` | DIFF-2.8 |

### Scenario bytes

DIFF-2.1 makes a scenario lines of bytes, so a NUL byte is a byte of its line: it ends neither
the line nor the file, and what follows it is read. What a NUL byte in a word means follows
from the statements that word falls under. A key holding one is unknown, and an integer,
number or map name holding one is not one (DIFF-2.2, DIFF-2.7, DIFF-2.8), so each is refused
(DIFF-2.9); a path holding one names no file, so the runner cannot read it. A `model` or
`cvar` name holding one is a name, but one no QuakeC string gives, since QuakeC's strings end
at their first NUL: the bounds or setting it names are never used, and two such names differ
when their bytes do. A `cvar` text holding one has the value `Q_atof` reads from it, which
stops at the NUL (DIFF-2.4). `cargo xtask oracles` runs these cases through the VM oracle
(`xtask/src/oracles/probes.rs`).

### Frames and thinking

Checked on 2026-10-16 by a local harness holding `SV_RunThink` and `SV_Physics_Pusher` copied
unchanged from `WinQuake/sv_phys.c` at the pinned commit, with the zero-velocity branch of
`SV_PushMove` (the whole of the mock's, DIFF-5.5) and stubs for `PR_ExecuteProgram`, `sv` and
`pr_global_struct`, built with Apple clang 21 at `-O0 -ffp-contract=off -fno-strict-aliasing`
on arm64 macOS. The frame length is 0.1; floats are printed with 9 significant digits.

Since P1-32 every row of this table and the next is a vector in
[`vectors/vm/frames.txt`](vectors/vm/frames.txt), which `cargo xtask oracles` runs through the
VM oracle on Linux, macOS and Windows in CI: the host sets the clock and calls id's
`SV_RunThink` and `SV_Physics_Pusher`, compiled from the pinned commit, on one edict, or runs
the oracle's physics and server frames, and the trace shows what changed. On 2026-10-16 all 15
passed on macOS arm64.

| Input | Output | Statements |
| --- | --- | --- |
| Think, clock 1, `nextthink` 1.05 | runs; `time` 1.04999995, `nextthink` 0 | DIFF-5.6 |
| Think, clock 1, `nextthink` 1.1 (f32 1.10000002) | does not run: above `clock + 0.1` | DIFF-5.6 |
| Think, clock 1.2000000000000002, `nextthink` 1.3 | runs; `time` 1.29999995 | DIFF-5.6 |
| Think, clock 1, `nextthink` 0.5 | runs; `time` 1 | DIFF-5.6 |
| Think, clock 1, `nextthink` 0 or -1 | does not run | DIFF-5.6 |
| Pusher, clock 5, `ltime` 0, `nextthink` 0.05 | `ltime` 0.0500000007; runs with `time` 5 | DIFF-5.5 |
| Pusher, clock 5, `ltime` 0, `nextthink` 0 | `ltime` 0; does not run | DIFF-5.5 |
| Pusher, clock 5, `ltime` 0.3, `nextthink` 0.25 | `ltime` 0.300000012; does not run, now or later | DIFF-5.5 |
| Pusher, clock 5, `ltime` 0, `nextthink` 0.1 | `ltime` 0.100000001; runs | DIFF-5.5 |
| Pusher, clock 5, `ltime` 0, `nextthink` 2 | `ltime` 0.100000001; does not run | DIFF-5.5 |
| Clock after the settle frames | 1.2000000000000002; `time` 1.20000005 (`3f99999a`) | DIFF-5.2, DIFF-3.6 |
| Clock after 100 frames of 0.1 | 11.199999999999978; `time` 11.1999998 (`41333333`) | DIFF-5.2 |
| `frametime` for 0.1 | `3dcccccd` | DIFF-5.1 |

### Random numbers

Computed by the same harness from DIFF-6.2 with seed 1, and `random` as `PF_random` computes it,
`(r & 0x7fff) / ((float)0x7fff)` in `f32`:

| Call | `rand` | `random` | Statements |
| --- | --- | --- | --- |
| 1 | 16838 | 0.513870656 (`3f038d07`) | DIFF-6.2 |
| 2 | 5758 | 0.175725579 (`3e33f168`) | DIFF-6.2 |
| 3 | 10113 | 0.308633685 (`3e9e053c`) | DIFF-6.2 |
| 4 | 17515 | 0.534531713 (`3f08d712`) | DIFF-6.2 |

### Equivalence

Derived from DIFF-9.4 and TRACE-7, with 105 field slots: trace A has `edict_size` 516 and
`field_offset` 96 (the 32-bit layout), trace B 528 and 108 (the 64-bit one, EDICT-1.3), and
both have `numstrings` 1000.

| A | B | Equivalent | Why |
| --- | --- | --- | --- |
| 1032 | 1056 | yes | edict 2 in both |
| 1032 | 1032 | yes | equal bits |
| 1548 | 1056 | no | edict 3 against edict 2 |
| 1312 | 1348 | yes | `nextthink` (field 46) of edict 2 in both |
| `7fc00000` | `ffc00000` | yes | both NaNs |
| `80000000` | `00000000` | no | -0 against +0 |
| 2000 with text `1` | 70000 with text `1` | yes | engine strings, same text |
| 2000 with text `1` | 70000 with text `2` | no | engine strings, different text |
| 500 | 500 | yes | equal bits: an offset into the string table |
