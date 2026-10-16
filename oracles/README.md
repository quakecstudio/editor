# Oracles

The C programs that prove QuakeC Studio agrees with id's code: id's VM as a headless trace
runner, and id's qcc. They are test tools, run in CI by `cargo xtask oracles` (LIC-3, COMP-7),
and are never part of the shipped app or needed to build it: nothing in the Cargo workspace
depends on them, `cargo build` never compiles them, and only `xtask` knows they exist (ARCH-2:
the Rust side talks to them through files and their exit status).

| Oracle | What it is | Binary |
| --- | --- | --- |
| VM oracle | id's `WinQuake/pr_exec.c` in a host that implements [`docs/spec/differential.md`](../docs/spec/differential.md) and writes the trace of [`docs/spec/trace.md`](../docs/spec/trace.md) | `target/oracles/bin/vm-oracle` |
| qcc | id's QuakeC compiler, `qcc/` of Quake-Tools, built unchanged | `target/oracles/bin/qcc` |

Later oracles (a pinned FTEQCC, P1-26; id's engine; the map tools) join this directory.

## Sources

No id code is committed (LIC-7). [`sources.toml`](sources.toml) pins every file the build uses
to a commit of the upstreams in [`docs/porting.md`](../docs/porting.md#upstreams) and to its
SHA-256; `cargo xtask oracles` downloads each from `raw.githubusercontent.com` and refuses a
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
`traceline`'s collision, `checkclient`, `aim`, `droptofloor`, `walkmove` and `movetogoal`
(DIFF-7); and `dprint`, `eprint`, `coredump`, `traceon` and `traceoff`, which only print.
`ED_ParseEpair` is ours for EDICT-8.3.

## Building

```bash
cargo xtask oracles                 # build both, then run every VM vector through the VM oracle
cargo xtask oracles build           # build only; prints where the binaries are
cargo xtask oracles id101 <dir>     # also compile id's QuakeC 1.01 with qcc and trace it
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
id's with warnings off.

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
edicts, the stub builtins), with initial values from an init file that xtask writes:

```text
edicts 3
loading                       # optional: the server is loading
entry 1
global <slot> bits <hex> | entity <n> | pointer <n> <field>
field <edict> <field> bits <hex> | entity <n> | pointer <n> <field>
```

Entity and pointer values are resolved by the oracle, since they depend on its edict layout. At
the end it prints the call depth and the local stack in use (`depth=0 localstack=2`), which the
vectors' `after:` lines check. `--console` copies the console to standard error.

The exit status is 0 when the run was traced, QuakeC errors included (they are error records);
2 when the command line, scenario or progs is refused, before any trace is written; 3 when a file
cannot be written; 4 when id's code crashed on undefined behaviour, after writing the records so
far (DIFF-10.1).

### How the VM is observed

`pr_exec.c` is compiled byte for byte as fetched, and the build defines `QS_PR_EXEC`, under which
[`vm/quakedef.h`](vm/quakedef.h) redirects two of its identifiers and nothing else:

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

Calls into QuakeC from the host and from builtins go through a wrapper that writes the run and
exit records (TRACE-4.1, TRACE-4.5). id's `Host_Error` and `Sys_Error` are the oracle's: they
write the error record (TRACE-4.6 to TRACE-4.8, DIFF-8.1, DIFF-8.2) and return to the host,
which shuts the server down as DIFF-8.3 says. `PR_RunError` is id's; its message is the last
console line it prints before calling `Host_Error ("Program error")`.

### Readings the specs leave open

The oracle had to choose an answer in these places; P1-31 (`plan/backlog/p1.yaml`)
pins them in the specs so that `qc-builtins` makes the same choice:

- An entity value names an edict when it is a whole, non-negative number of edicts below
  `LIMIT_MAX_EDICTS` (EDICT-1.5); any other is BUILTIN-2.3's `invalid entity`.
- A string value is valid when it is inside the string table or is exactly the value of an
  engine string; any other is BUILTIN-2.5's `invalid string`.
- Those checks, and BUILTIN-8.3's `invalid field`, happen where id's code reads the argument
  (`G_EDICT`, `PROG_TO_EDICT`, `G_STRING`, `E_STRING`), so a builtin's earlier writes stand.
- `find` checks each edict's field value as a string argument.
- `vtos` prints a NaN or an infinity component as `ftos` does (BUILTIN-7.6).
