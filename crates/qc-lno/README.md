# qc-lno

Reads and writes the `.lno` line map that a QuakeC compiler writes beside a `progs.dat`. A
`.lno` gives the source line, and optionally the source column, of every statement in the
compiled progs. id never defined the format; it is described in
[`docs/spec/lno.md`](../../docs/spec/lno.md) from the programs that write and read it (FTEQCC
and FTE, GMQCC, DarkPlaces), cited for description only. No file here is ported from a
registered upstream, so no file carries an id copyright header.

`LnoFile::read` checks a file in the order of LNO-5.6 and returns an owned `LnoFile`, or a
typed `ReadError`, never a panic, for any input. `LnoFile::write` turns an `LnoFile` back into
bytes; a file the reader accepts is written back byte for byte (LNO-6.2), keeping the line
table, the optional column table and any trailing bytes. `LnoFile::matches` (and
`matches_header` for a `qc_progs::Header`) is the LNO-4.1 check that a `.lno` belongs to a
given `progs.dat`, reporting the first differing count in header order.

- **Layer:** L0 formats. It depends on `qc-progs` only for the `Header` the matcher reads; the
  compiler and the preview host build on it.
- **Spec sections:** LNO-2 to LNO-6 in [`lno.md`](../../docs/spec/lno.md). LNO-1 (when a
  compiler writes a `.lno`) belongs to the compiler, and LNO-4.4 and LNO-4.5 (a statement's
  source file) to the line-number reader that pairs a `.lno` with a `progs.dat`'s functions.
- **Shipped:** yes.

## Invariants

Unit tests of every `docs/spec/lno.md` test vector in `src/vectors.rs`, and property tests in
`src/tests.rs`:

- Arbitrary bytes never panic the reader (LNO-5.7).
- A file the reader accepts is written back byte for byte, and reading what the writer wrote
  gives back the same counts, tables and trailing bytes (LNO-6.2).
- The checks run in order and the first failure is reported (LNO-5.6); the counts and line and
  column values are kept exactly as read (LNO-3.5, LNO-5.7).

The `.lno` files the pinned FTEQCC writes beside the progs it builds are round-tripped by
`cargo xtask corpus build`, which compiles each codebase in the oracle flow (network and a C
compiler, CI only).
