# Oracles

The C programs that prove QuakeC Studio agrees with id's code: id's VM as a headless trace
runner, id's qcc, and a pinned FTEQCC for the rerelease QuakeC, which id's qcc cannot compile.
They are test tools, run in CI and by `make oracles` through `cargo xtask oracles` (LIC-3,
LIC-8, COMP-7), and are never part of the shipped app or needed to build it: nothing in the
Cargo workspace depends on them, `cargo build` never compiles them, and only `xtask` knows they
exist (ARCH-2: the Rust side talks to them through files and their exit status).

| Oracle | What it is | Binary |
| --- | --- | --- |
| VM oracle | id's `WinQuake/pr_exec.c` in a host that implements [`docs/spec/differential.md`](../docs/spec/differential.md) and writes the trace of [`docs/spec/trace.md`](../docs/spec/trace.md) | `target/oracles/bin/vm-oracle` |
| qcc | id's QuakeC compiler, `qcc/` of Quake-Tools, built unchanged | `target/oracles/bin/qcc` |
| FTEQCC | The command-line compiler in `engine/qclib/` of fte-team/fteqw, built unchanged without zlib | `target/oracles/bin/fteqcc` |

Later oracles (id's engine; the map tools) join this directory.

## Sources

No third-party code is committed (LIC-7). [`sources.toml`](sources.toml) pins every file the
build uses to a commit and to its SHA-256: id's files to the upstreams in
[`docs/porting.md`](../docs/porting.md#upstreams), FTEQCC's to the fteqw commit that
[`progs.md`](../docs/spec/progs.md) and [`lno.md`](../docs/spec/lno.md) cite; `cargo xtask oracles` downloads each from `raw.githubusercontent.com` and refuses a
file whose SHA-256 differs. The files are kept in `target/oracles/src/`. id's `quakedef.h` is
never fetched: [`vm/quakedef.h`](vm/quakedef.h) stands in for it.

The VM oracle compiles `WinQuake/pr_exec.c` whole, and the id functions listed under
`[[extract]]` in `sources.toml`, which the build copies unchanged out of the fetched files into
generated files under `target/oracles/build/`. Those are the functions the mock engine runs
exactly as id's server does (DIFF-1.3): spawning a map (`ED_LoadFromFile`, `ED_ParseEdict`,
`ED_Alloc`, `ED_Free`), thinking (`SV_RunThink`, `SV_Physics_Pusher`), message writing, and the
builtins whose behaviour [`builtins.md`](../docs/spec/builtins.md) takes from id unchanged.

This directory holds only our code, all GPL-3.0-or-later:

| File | What it does |
| --- | --- |
| [`vm/quakedef.h`](vm/quakedef.h) | The declarations id's code needs, with id's names, and the build's redirections below |
| [`vm/host.c`](vm/host.c) | The command line, memory, loading a progs, the run wrapper and statement hook, errors |
| [`vm/trace.c`](vm/trace.c) | The binary trace encoder (TRACE-5) and change sets (TRACE-3), from the spec alone |
| [`vm/scenario.c`](vm/scenario.c) | Scenarios (DIFF-2), the map (DIFF-3), clients (DIFF-4), frames (DIFF-5), the end (DIFF-8) |
| [`vm/engine.c`](vm/engine.c) | The mock engine: settings, buffers, model bounds, linking, the empty world, `rand`, map keys |
| [`vm/builtins.c`](vm/builtins.c) | The builtin table, the builtins that are ours, and the vector stubs |
| [`qcc/compat.h`](qcc/compat.h), [`qcc/win32/`](qcc/win32) | What qcc needs to compile on a C99 compiler and on Windows, included around its files |

The builtins that are ours rather than id's, and why: `ftos`, `vtos` and the joined text of
`bprint` and the others (BUILTIN-7.5, BUILTIN-7.6, where id's buffers overflow or C libraries
differ); `error` and `objerror` (the error record carries the joined text, DIFF-8.2); `break`
(BUILTIN-16.3); `lightstyle`'s range check (BUILTIN-12.6), before id's `PF_lightstyle`;
`WriteString` and `WriteEntity`, which read the destination first (BUILTIN-2.10);
`traceline`'s collision, `checkclient`, `aim`, `droptofloor`, `walkmove` and `movetogoal`
(DIFF-7); and `dprint`, `eprint`, `coredump`, `traceon` and `traceoff`, which only print.
`ED_ParseEpair` is ours for EDICT-8.3.

## Building

```bash
cargo xtask oracles                 # build all three, then run every VM vector, scenario number vector, probe and scenario test through the VM oracle, and the builtin differential
cargo xtask oracles build           # build only; prints where the binaries are
cargo xtask oracles builtins [--record] # build, then run only the builtin differential (FTEQCC and the rerelease, below); --record writes its fixture
cargo xtask oracles scenarios [<name>...] # build, then run only the scenario tests, or the ones named
cargo xtask oracles fuzz <target | all> [--time <s>] # build and run the libFuzzer targets (Fuzzing, below)
cargo xtask oracles id101 <dir>     # also compile id's QuakeC 1.01 with qcc and trace it
cargo xtask oracles rerelease <dir> # also compile the six rerelease codebases with FTEQCC
cargo xtask oracles json <trace>    # print a binary trace as JSON Lines (TRACE-6)
```

It needs `curl` and the system C compiler, which xtask finds with the `cc` crate: Xcode's
command line tools on macOS, `cc` (GCC or Clang) on Linux, Visual Studio's C compiler on Windows
(found without a developer prompt). `CC` overrides the choice. A stamp of every input
(`sources.toml`, this directory, the compiler and its flags) lets an unchanged build skip the
compile; CI caches `target/oracles/src` and `target/oracles/bin` on that basis.

Every file is compiled as [`vm.md`](../docs/spec/vm.md) "Reference build" requires:
`-fno-strict-aliasing` (VM-5.1, VM-10.2) and `-ffp-contract=off` (VM-5.2), at `-O2`, as C99 with
GNU extensions. `-fsigned-char` makes `char` signed on every host, as on x86, which BUILTIN-10.2
and id's `COM_Parse` assume. MSVC's `char` is signed, it does not optimise on strict aliasing,
and under `/fp:precise` it does not contract. Our files are compiled with every warning an error;
id's and FTEQCC's with warnings off.

The VM-9.2 vectors convert out-of-range floats to integers, which only x86 does as the spec
fixes; on another host (macOS on Apple silicon) their failures are reported as expected
divergences (DIFF-10.1). CI runs the oracle on x86-64 Linux and Windows, where they must pass.

## The VM oracle

```text
vm-oracle [--console] scenario <scenario> <trace>
vm-oracle [--console] vector <progs.dat> <init> <trace>
```

`scenario` runs a scenario file of DIFF-2 and writes its trace. `vector` runs one program of
[`docs/spec/vectors/vm/`](../docs/spec/vectors/vm/README.md) in the vectors' environment (three
edicts in use out of `LIMIT_MAX_EDICTS`, the stub builtins), with initial values from an init
file that xtask writes:

```text
edicts 3
maxclients <n>                # optional: client slots with no player, 0 to 16
seed <n>                      # optional: the random state of DIFF-6.2, 1 when absent
loading                       # optional: the server is loading
builtins engine               # optional: the scenario table of id's builtins, not the stubs
entry 1
global <slot> bits <hex> | entity <n> | pointer <n> <field>
field <edict> <field> bits <hex> | entity <n> | pointer <n> <field>
host clock <seconds> | run <function> | think <edict> <seconds> | push <edict> <seconds>
host physics <seconds> <count> | frame <seconds> <count> | edict <n> <slot>
```

Every number is the one its line takes, or the file is refused: an integer is an optional `-`
and decimal digits (DIFF-2.7), in range (`edicts` 1 to 600, `seed` 0 to 4294967295, a slot,
field or edict within the progs and the edicts in use, a count at least 1, an entity or
pointer whose value fits in an `int`); `<hex>` is one to eight hexadecimal digits; `<seconds>`
is a number as DIFF-2.8 writes it. A NUL byte, a line of 512 bytes or more, or a line of more
than eight words is refused too.

Entity and pointer values are resolved by the oracle, since they depend on its edict layout.
Without `host` lines the oracle runs the entry function once; with them it does what they say,
in order, as the vectors' README describes: set the clock, run a function, run id's
`SV_RunThink` or `SV_Physics_Pusher` on one edict, run physics or server frames as scenario mode
does, or write the entity value of id's `EDICT_NUM(n)` to a global. An error ends the actions. At
the end it prints the call depth and the local stack in use (`depth=0 localstack=2`), which the
vectors' `after:` lines check, and on a second line the bytes of the unreliable datagram in hex
(`datagram=40ff00c0`), which their `datagram:` lines check. `--console` copies the console to
standard error.

The exit status is 0 when the run was traced, QuakeC errors included (they are error records);
2 when the command line, scenario or progs is refused, before any trace is written; 3 when a file
cannot be written; 4 when id's code crashed on undefined behaviour, after writing the records so
far (DIFF-10.1).

### Scenario tests

The scenario tests in [`xtask/src/oracles/scenarios/`](../xtask/src/oracles/scenarios.rs) hold
the oracle's own C to the specs it implements. Each builds a small progs (with `qc-progs`) and
the scenario, map or init file around it, runs the VM oracle, and checks what
[`differential.md`](../docs/spec/differential.md) and [`trace.md`](../docs/spec/trace.md) say
the run does: the exit status, and records and values of the trace, which `qc-trace`'s strict
decoder must read and encode back to the same bytes. Each cites the statements it pins
(`// spec DIFF-5.6: ...`). They cover every refusal of DIFF-2.9 and of the progs (DIFF-3.1),
every exit status above, the setup of the map, clients and frames, every mock builtin of
DIFF-6 and DIFF-7, the errors and shutdown of DIFF-8, and the records and binary form of the
trace. `cargo xtask oracles` runs them after the vectors and probes, so CI runs them on Linux,
macOS and Windows; the cases that write a trace to `/dev/full` run on Linux only.

xtask's own tests replay what the oracle gave, without building it:
[`xtask/fixtures/oracles/`](../xtask/fixtures/oracles/record.py) records each vector's trace
and each scenario test's exit status, output and traces, except the few traces over 8 KiB, and
`cargo test -p xtask` checks every vector and case against them. After changing a vector or a
case, run `cargo xtask oracles`, then `python3 xtask/fixtures/oracles/record.py`.

### How the VM is observed

`pr_exec.c` is compiled byte for byte as fetched, and the build defines `QS_PR_EXEC`, under which
[`vm/quakedef.h`](vm/quakedef.h) redirects three of its identifiers and nothing else:

- `pr_trace`, the statement trace flag, becomes a call to the oracle's hook. `PR_ExecuteProgram`
  tests the flag after each statement becomes current and before it executes, so the hook sees
  every statement, with `pr_xfunction` and `pr_xstatement` set; it reads as false, so id's
  statement printing never runs (the flag changes nothing the trace records, DIFF-6.7). The
  hook writes each statement's step record when the next one becomes current, when the run
  returns, or before a runaway loop's error record (TRACE-4.2, TRACE-4.7).
- `pr_stack`, the call stack, becomes storage the oracle owns with one spare frame. On a stack
  overflow `PR_StackTrace` writes the frame one past the end; in id's 32-bit build that lands
  in a local stack slot nothing reads again, and here it would land on whatever the linker
  placed next.
- `strcmp` returns the difference of the first differing bytes, read as unsigned, which is
  what glibc and macOS return and what VM-7.2 fixes for `NE_S`. C fixes only the sign, and the
  Windows C runtime returns -1 and 1.

Calls into QuakeC from the host and from builtins go through a wrapper that writes the run and
exit records (TRACE-4.1, TRACE-4.5). id's `Host_Error` and `Sys_Error` are the oracle's: they
write the error record (TRACE-4.6 to TRACE-4.8, DIFF-8.1, DIFF-8.2) and return to the host,
which shuts the server down as DIFF-8.3 says. `PR_RunError` is id's; its message is the last
console line it prints before calling `Host_Error ("Program error")`.

### Argument checks

id's builtins read entity, string and field values without checking them. The specs fix what a
bad one does, and the oracle makes the same checks as `qc-builtins`, with the messages of
DIFF-6.6:

- An entity value must name an edict (EDICT-1.9): a whole, non-negative number of edicts below
  `LIMIT_MAX_EDICTS`, in use or not; any other is `invalid entity` (BUILTIN-2.7).
- A string value must be inside the string table or exactly the value of an engine string
  (BUILTIN-2.8); any other is `invalid string` (BUILTIN-2.5). A field index outside the
  progs's fields is `invalid field` (BUILTIN-8.3).
- Each check is made where id's code reads the value (BUILTIN-2.9): `quakedef.h` replaces
  `G_EDICT`, `PROG_TO_EDICT`, `G_STRING` and `E_STRING` (and so `G_EDICTNUM`) with checked forms
  in every file but `pr_exec.c`, and in the file of id's `pr_cmds.c` functions, which the build
  marks with `QS_EXTRACTED_PR_CMDS`, it checks the `model` text `PF_makestatic` passes to
  `SV_ModelIndex`. A builtin's earlier writes stand, and `find` checks the field of each edict
  it examines and no other (BUILTIN-8.8).
- `WriteString` and `WriteEntity` are ours: id's pass `WriteDest()` and the value as arguments
  of one call, whose order C leaves to the compiler, and BUILTIN-2.10 reads the destination
  first.
- `vtos` prints a NaN or an infinity component as `ftos` does (BUILTIN-7.7).

The vectors in [`builtins.txt`](../docs/spec/vectors/vm/builtins.txt) run id's builtins
(`builtins: engine`) and pin each of these; `cargo xtask oracles` runs them with the others.

## FTEQCC and the rerelease

FTEQCC is built from the files of the `qcc` target of `engine/qclib/Makefile` (`QCC_OBJS`,
`COMMON_OBJS` and `TUI_OBJS`), with `NO_ZLIB` defined so it needs no library beyond the C
runtime; without zlib it cannot read or write compressed archives, which compiling a
`progs.src` never needs. On Windows the build also defines `strcasecmp` as `_stricmp`, the
Windows C runtime's name for the POSIX function `qcc_pr_comp.c` calls. Its `engine/qclib/LICENSE` is the GPL version 2 text and its files
carry no "or later" grant, so it is only ever run as a separate program, never linked with
anything of ours.

`cargo xtask oracles rerelease <dir>` compiles the six codebases of id's
`quake-rerelease-qc` (corpus ids `rerelease`, `rerelease-hipnotic`, `rerelease-rogue`,
`rerelease-mg1`, `rerelease-ctf` and `rerelease-mg3`), each a directory of `<dir>` as
`cargo xtask corpus fetch <id>` writes it. Each is copied to
`target/oracles/work/rerelease/<id>/src` and compiled there with

```text
fteqcc -Tid -flno -o ../progs.dat
```

- `-Tid` selects FTEQCC's `QCF_STANDARD` target: version 6 progs, where FTEQCC refuses every
  opcode from `OP_MULSTORE_F` on (`qcc_pr_comp.c`, `QCC_OPCodeValidForTarget`), so only id's
  opcodes 0 to 65 can be written. All six compile under it, so P1-02 and P1-10 need not cover
  any extra opcode. The check also reads every statement and fails on any opcode above 65.
- `-flno` writes `progs.lno` beside `progs.dat` ([`lno.md`](../docs/spec/lno.md) LNO-1.1,
  LNO-1.4). With no `-O` option FTEQCC already writes it, since the `filenames` optimisation
  that suppresses it is off; the flag keeps it written should that default change. These are
  the FTEQCC `.lno` files the `qc-lno` corpus (P1-28) reads.
- `-o ../progs.dat` overrides the output each `progs.src` names (`../qsrc/ctf/progs.dat`,
  `progs.dat`, `#output "../progs.dat"`), so every codebase writes to the same place.
- `rerelease-mg3` alone adds `-Fautoproto`: it uses functions and constants before declaring
  them (`ai.qc` uses `CORPSE_FLAG_A` from `monsters/mg3_hknight_infected.qc`), which FTEQCC
  accepts only when it collects prototypes first. The flag changes the order of definitions in
  the output, so the other codebases are compiled without it.

`progs.dat` and `progs.lno` are copied to `target/oracles/rerelease/<id>/`, the oracle
artifacts later differential tests read. The check requires each progs.dat to load with
`qc-progs` as version 6 with vanilla opcodes only, each `.lno` to have LNO-2's header with the
progs's four counts and exactly one line per statement (LNO-3.1, LNO-3.3), and both to have the
SHA-256 recorded in `xtask/src/oracles/rerelease.rs`. The progs.dat's digest is taken with the
bytes between the header and the first table zeroed, because FTEQCC writes its copyright text
there (`qccmain.c`, `WriteData`) and, unless the source sets `#copyright`, that text holds the
date of the compile and of the FTEQCC build. When the artifacts already have the recorded
digests, as after a CI cache hit, they are checked without compiling again. CI runs this in the
Linux `Test Assets` job and caches `target/oracles/rerelease` on `oracles/`, the xtask oracle
code and `corpus/manifest.toml`.

### The builtin differential

[`xtask/src/oracles/builtins.rs`](../xtask/src/oracles/builtins.rs) compiles every builtin
initialiser of a cross product (`#` and `:`; every number, string and name form; engine names;
each function declarator; every pairing of two initialisers of one function) with FTEQCC under
the FTEQCC profile and with id's qcc under the vanilla profile, each as a one-file
`progs.src` in `target/oracles/work/builtins`, and compares what each compiler did (the
builtin number and engine name it wrote, a redeclaration error, another error) with what the
profile reads ([`dialect-fteqcc-rerelease.md`](../docs/spec/dialect-fteqcc-rerelease.md)
FTE-23.12). The oracles' outcomes are recorded in
[`xtask/fixtures/oracles/builtin-outcomes.json`](../xtask/fixtures/oracles/builtin-outcomes.json):
`cargo xtask oracles` fails when an oracle gives another outcome than the recording, and
`cargo test -p xtask` checks the profiles against the recording on every host without the
oracles. After changing the cross product, run `cargo xtask oracles builtins --record`.

## Readiness

The oracles' C is the C tools layer's own code, and the readiness bar of
[`docs/testing.md`](../docs/testing.md) applies to it as to a crate. `cargo xtask readiness
<check> oracles` measures it, and the `Readiness` workflow takes `oracles` in its list of crates:

- **Spec sections:** DIFF-1, DIFF-2, DIFF-3, DIFF-4, DIFF-5, DIFF-6, DIFF-7, DIFF-8, DIFF-10,
  TRACE-1, TRACE-2, TRACE-3, TRACE-4 and TRACE-5: the statements the VM oracle and its driver
  in `xtask/src/oracles/` implement. Comparing two traces and counting their statements is
  the differential runner's work, not the oracle's.
- **Lines and branches:** the VM oracle built with clang's source-based coverage
  (`QS_ORACLE_COVERAGE`), run on every vector, number vector and probe (`cargo xtask oracles`)
  and on the id 1.01 scenarios (`cargo xtask oracles id101`, from the corpus under
  `QS_TEST_ASSETS`), counting the files of this directory and not id's. It needs clang and the
  `llvm-profdata` and `llvm-cov` of the same LLVM (`CC`, `QS_ORACLE_LLVM_PROFDATA` and
  `QS_ORACLE_LLVM_COV` choose them; on macOS `xcrun` finds Xcode's). They are not
  `LLVM_PROFDATA` and `LLVM_COV`, which cargo-llvm-cov reads for the Rust crates measured in the
  same run.
- **Flakes:** `cargo xtask oracles`, 20 times on each OS after one build.
- **Mutation:** an exception, recorded in [`ratchets.toml`](../ratchets.toml). The bar's
  mutation tool, cargo-mutants (TEST-2), mutates Rust only. The C is held instead by tests
  whose expectations are written from the spec without it: every vector, scenario number,
  probe and scenario test states the trace or exit status the spec gives, so a fault in the C
  fails one of them. The Rust that drives the oracles is in `xtask`, which cargo-mutants
  measures.
- **Fuzzing:** every reader of a file has a target (Fuzzing, below); TEST-5's day of fuzzing is
  `cargo xtask oracles fuzz all --time 86400`, whose result the readiness review records.

## Fuzzing

[`vm/fuzz.c`](vm/fuzz.c) holds a libFuzzer target for each of the VM oracle's readers of a
file: `scenario` (the scenario file, DIFF-2), `init` (vector mode's init file, read against a
small progs) and `progs` (the progs loader of DIFF-3.1 and EDICT-3.5, with the header CRC checked
and without). `cargo xtask oracles fuzz <target | all> --time <seconds>` builds each, the whole
oracle compiled with the address and undefined-behaviour sanitizers (not the alignment check:
id's edicts sit at multiples of the edict size, which a 64-bit host does not align to 8) and
linked with libFuzzer, so it needs a clang that has libFuzzer (`CC`: Linux's clang with its
runtime package, or Homebrew's LLVM on macOS, whose own clang has none). Each target's corpus is
`target/oracles/fuzz/<target>/corpus`, seeded from the inputs the scenario tests and the VM
vectors write, and an input that fails is kept in `artifacts/`. The nightly `Fuzz` workflow runs
each for 10 minutes. The readers set their state back to its defaults before each file, which is
what lets one process read file after file.

A finding becomes a scenario test or probe that runs through the built oracle: the first runs
found a progs whose edict size overflowed an `int`, which EDICT-3.5 now refuses
(`progs-too-many-fields`).

The vectors' own parser, in `xtask`, has a cargo-fuzz target, `xtask-vectors`, in
[`fuzz/`](../fuzz), which `cargo xtask fuzz` runs with the others.
