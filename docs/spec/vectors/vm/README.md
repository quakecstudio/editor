# VM test vectors

Small QuakeC programs for [`../../vm.md`](../../vm.md), for the argument checks and the recorded
rows of id's builtins in [`../../builtins.md`](../../builtins.md), and for the allocation rows of
[`../../edicts.md`](../../edicts.md) and the frame rows of
[`../../differential.md`](../../differential.md), written as statements, with the globals and
entity fields every statement changes. They are hand-written, not compiler
output, so each one isolates a single rule. `qc-vm` (P1-10) runs them as unit tests, with
`qc-builtins` (P1-16) for those marked `builtins: engine`, and the differential suite runs them
against the id VM oracle (P1-11).

| File | Statements | Vectors |
| --- | --- | --- |
| [`arith.txt`](arith.txt) | VM-4, VM-5 | 24 |
| [`compare.txt`](compare.txt) | VM-6 to VM-8 | 27 |
| [`bitwise.txt`](bitwise.txt) | VM-9 | 8 |
| [`store.txt`](store.txt) | VM-10 to VM-12 | 27 |
| [`flow.txt`](flow.txt) | VM-1 to VM-3, VM-13 to VM-19 | 56 |
| [`builtins.txt`](builtins.txt) | EDICT-1.9, BUILTIN-2.4, BUILTIN-2.5, BUILTIN-2.7 to BUILTIN-2.10, BUILTIN-7.7, BUILTIN-8.3, BUILTIN-8.8 | 26 |
| [`math.txt`](math.txt) | BUILTIN-6, BUILTIN-7.1, BUILTIN-7.2, BUILTIN-7.5, BUILTIN-7.6, BUILTIN-13.2, BUILTIN-15.4, BUILTIN-15.5: builtins.md "Math and strings" | 78 |
| [`edicts.txt`](edicts.txt) | EDICT-1.4, EDICT-1.7, EDICT-5.3, EDICT-6, EDICT-7: edicts.md "Allocation and freeing" | 17 |
| [`frames.txt`](frames.txt) | DIFF-3.6, DIFF-5.1, DIFF-5.2, DIFF-5.5, DIFF-5.6, DIFF-6.2: differential.md "Frames and thinking" and "Random numbers" | 15 |

Every vanilla opcode (PROGS-8.1) appears in at least one vector; `spec_vectors` in
`crates/qs-tests` checks that and the format below on every `cargo test`.

## Environment

Every vector runs in the same machine, so a vector only states what differs from it:

- 128 global slots, all 0 (`numglobals:` changes the count). Slots 28 to 91 are id's system
  globals from `WinQuake/progdefs.q1`: `self` is slot 28 and `time` slot 31. Vectors keep
  their own data in slots 100 to 127.
- Three edicts in use (`num_edicts` is 3), all fields 0: `e0` (the world), `e1` and `e2`; the
  array has all `LIMIT_MAX_EDICTS` edicts (EDICT-1.4). None is free, and no client slot
  (`maxclients` 0). `edicts:` and `maxclients:` change the counts; the client slots have no
  player. Each has 110 field slots: the
  105 of id's `entvars_t` (`origin` is 10 to 12, `frame` 30, `think` 44, `nextthink` 46) and
  105 to 109 for the vector's own fields.
- A string table holding only the empty string at offset 0, unless `strings:` says otherwise.
- Function 0 is the null function and function 1 starts at statement 1 with no parameters
  and no locals; the program is run by calling function 1 (`entry:` changes it). Statement 0
  is all zero and `code:` starts at statement 1.
- The server is active (`server: loading` changes it, for VM-12.2), and the clock is 0.
- The random state of DIFF-6.2 is 1, `DIFF_DEFAULT_SEED` (`seed:` changes it).
- 79 builtins, numbers 0 to 78 (`LIMIT_NUM_BUILTINS`). These are test stubs, not id's
  builtins: builtin 2 runs a nested `PR_ExecuteProgram` of the function whose index is in
  `OFS_PARM0`; every other number writes the argument count as a float to slot 1 and copies
  slot 4 to slot 2. `builtins: engine` replaces them with id's builtins as the differential
  mock has them (`differential.md` DIFF-6, DIFF-7), with no settings, empty precache lists,
  and the three message buffers of DIFF-6.5, which start empty.
- The host runs the entry function once. A `host:` block replaces that with its actions, in
  order (Host actions below).

## Format

Plain text; `#` starts a comment, except on a `strings:` line. A vector is:

```text
== <name>
spec: <statement IDs it pins, comma-separated>
globals: <slot>=<value> ...          optional, repeatable: initial globals
fields: e<n>.<field>=<value> ...     optional, repeatable: initial entity fields
strings: "<text>" ...                optional: strings after the empty one, \xNN escapes
numglobals: <count>                  optional
edicts: <count>                      optional: edicts in use, 1 to 600
maxclients: <count>                  optional: client slots, 0 to 16
seed: <number>                       optional: the random state, 0 to 4294967295
server: loading                      optional
builtins: engine                     optional: id's builtins instead of the stubs
entry: <function index>              optional
functions:                           optional: one line per function index
  <n>: first=<s> parm_start=<p> locals=<l> parms=<size>,<size>,...
  <n>: builtin=<number>
host:                                optional: one host action per line (below)
  <action>
steps: <count>                       optional: total statements executed
after: depth=<d> localstack=<n>      optional: call depth and local stack slots in use at the end
datagram: <byte> ...                 optional: the unreliable datagram's bytes at the end, decimal
oracle: no                           optional: the answer is this spec's, not id's (VM-19.4)
code:
  <OP> <a> <b> <c>                   one statement per line, from statement 1; missing operands are 0
expect:
  <statement> <OP> <slot>=<value> e<n>.<field>=<value> ...
  run <function> <slot>=<value> e<n>.<field>=<value> ...   only with host:
  done | error "<message>" | error
```

`strings:` lays the strings out after the empty string, each followed by a NUL, so the first
is at offset 1 and each next one starts after the previous one's NUL. `parms=` gives
`parm_size` and sets `numparms` to the number of sizes. An opcode may be written as its
number, for an invalid one.

`expect:` lists every statement executed, in order, each with exactly the globals and fields
whose 32-bit pattern changed during it, then how the run ended: `done` when the entry
function returned, or the error. A bare `error` accepts any message; it is used for the
divergences, whose message is the implementation's. A quoted message holds no `"` and, since
`#` starts a comment, no `#`; any other text after `error`, and an `after:` line without both
numbers in the order shown, is a malformed file, not a looser check. When `expect:` starts with `...`, only
the last statements are listed and `steps:` gives the total; the runaway and stack overflow
vectors use it.

In a vector with a `host:` block, each run the host starts (not a nested run a builtin starts)
is listed as a `run` line before its statements, with every change since the previous line:
the host's own writes, such as `time`, `self` and `nextthink` before a think function, and
anything the host changed outside a run, such as a pusher's `ltime` when its think did not run.
An error the host raises outside any run (`EDICT_NUM: bad number 600`) has no line; the vector
ends with its message. `steps:` counts statements, not `run` lines.

The changes made by a builtin are listed on its `CALL`. When the builtin starts a nested
run, its changes up to that point are on the `CALL`, and the nested run's statements follow
it.

Values:

| Form | Meaning |
| --- | --- |
| `1.5`, `-3`, `1e38`, `inf`, `-inf`, `-0` | The f32 nearest to the decimal value |
| `nan` | Any NaN as an expected value (VM-4.3); 0x7FC00000 as an initial value |
| `0x3f800000` | That exact 32-bit pattern |
| `i5` | The integer 5: a string offset, a field index or a function index |
| `e1` | The entity value of edict 1 (PROGS-17.4); its number is engine-specific |
| `es` | Any engine-made string (EDICT-13.1), as an expected value only; its number is engine-specific |
| `p1.105` | A pointer to field 105 of edict 1 (VM-12.1); its number is engine-specific |

## Host actions

A `host:` block lists what the host does, in order. The actions call the oracle's own functions
and id's, so a vector can reach the engine code that QuakeC cannot call directly:

| Action | What the host does |
| --- | --- |
| `clock <seconds>` | Sets the server clock, a `double` |
| `run <function>` | Runs the function: a run the trace records (TRACE-4.1) |
| `think e<n> <seconds>` | id's `SV_RunThink` on edict `n` with a frame of that length (DIFF-5.6) |
| `push e<n> <seconds>` | id's `SV_Physics_Pusher` on edict `n` with a frame of that length (DIFF-5.5) |
| `physics <seconds> [<count>]` | That many physics frames (DIFF-5.3), as the settle frames run |
| `frame <seconds> [<count>]` | That many server frames (DIFF-5.1), with no input |
| `edict <n> <slot>` | id's `EDICT_NUM(n)`, its entity value written to the global slot |

`<seconds>` is read to the nearest `double`. A physics or server frame runs `StartFrame`, the
function in global 83, which a vector that uses one sets.

## How they were checked

On 2026-10-16 the 128 vectors without `oracle: no` were run through id's
`PR_ExecuteProgram`, copied unchanged from `WinQuake/pr_exec.c` at
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d` together with `pr_comp.h`,
`progs.h` and `progdefs.q1` (as `progdefs.h`). The harness supplied the environment above, a
stub `quakedef.h`, and `Host_Error` and `Sys_Error` that end the run; it compared every global
and field slot before and after each statement. Built for x86-64 with GCC 14.4 at
`-O2 -fno-strict-aliasing -ffp-contract=off`, every vector matched. Built for arm64 with Apple
clang and the same flags, every vector matched except `bitand_out_of_range` and
`bitor_out_of_range`, where the ARM conversion saturates (VM-9.2). Without
`-fno-strict-aliasing`, GCC's `-O2` build gave different results for `add_v_overlap_shifted`
and `store_v_overlap_forward` (VM-5.1, VM-10.2).

The harness is not committed: it contains id's C, which the oracles fetch by commit and
checksum instead.

Since P1-11, `cargo xtask oracles` runs every vector without `oracle: no` through the VM oracle
([`oracles/README.md`](../../../../oracles/README.md)) on every CI run, on Linux, macOS and
Windows: each vector becomes a progs.dat written by `qc-progs`, the oracle traces it, and the
trace's statements, changes, builtin numbers, ending, `after:` and `datagram:` values must
match. On
2026-10-16 all 128 passed on macOS arm64 except the two VM-9.2 vectors, reported there as
expected divergences (DIFF-10.1); the x86-64 runs must pass all 128.

The 26 vectors of `builtins.txt` (`builtins: engine`) were added on 2026-10-16 for P1-31 and
checked only through the VM oracle, which runs id's `PF_` functions from the same commit with
the argument checks of BUILTIN-2.9: all 26 passed on macOS arm64. Where a builtin returns an
engine-made string (`es`), the vector compares its text with one of its own strings by `EQ_S`
(VM-7.1).

The vectors of `math.txt`, `edicts.txt` and `frames.txt` were added on 2026-10-16 for P1-32.
They carry the rows that `builtins.md`, `edicts.md` and `differential.md` had checked by hand in
local harnesses, and run through the VM oracle with the others: all 110 passed on macOS arm64.
None of them depends on the host converting an out-of-range float (the oracle's `ftos` uses
BUILTIN-2.2's conversion on every host), so they must pass on all three CI hosts. The rows of
`makevectors`, `vectoyaw` and `vectoangles` go through the C library's `sin`, `cos` and
`atan2`; should one CI host's library round one of them differently (BUILTIN-6.9), that vector
fails the run until the spec records the reference build's answer and `differential.md` DIFF-10.1
lists the difference.
