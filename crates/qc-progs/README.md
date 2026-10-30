# qc-progs

Reads, validates and writes `progs.dat`, the compiled QuakeC file. The reader is id's
`PR_LoadProgs` with the bounds and string checks id's engine leaves out: it returns a
zero-copy view whose tables decode from the file's bytes on access, and rejects malformed
input with a typed `ReadError`, never a panic. The writer turns an owned `ProgsFile` back into
bytes; a file read and written back is the same byte for byte. Also here: the 16-bit CRC
(`crc`), the opcode numbers (`opcode`), the `ED_Find*` lookups, and every limit in
`docs/spec/limits.md` as a named constant (`limits`). Ported from `WinQuake/pr_edict.c`,
`pr_comp.h` and `crc.c` at the commit pinned in [`docs/porting.md`](../../docs/porting.md).

- **Layer:** L0 formats. It depends on `qc-types`; the VM, the edict store and the compiler
  build on it.
- **Spec sections:** PROGS-1 to PROGS-15 and PROGS-22 in [`progs.md`](../../docs/spec/progs.md),
  PROGS-19 to PROGS-21 (FTEQCC's version 7), and LIMIT-1 in
  [`limits.md`](../../docs/spec/limits.md).
- **Not this crate's:** PROGS-16 to PROGS-18, which are in `qc-types`, and PROGS-5 (generating
  the `progdefs.h` CRC), PROGS-6.2 and PROGS-10.3, which belong to the compiler, the server and
  the VM.
- **Versions:** the header's version picks the format: 6 is id's, 7 is FTEQCC's (16-bit or
  32-bit by `secondaryversion`), and any other is `ReadError::WrongVersion`. id's engine rejects
  version 7 (PROGS-3.1); a caller that models it checks `Progs::format`.
- **Shipped:** yes.

## Invariants

Property tests in `src/read/tests.rs` and `src/write/tests.rs`, and the fuzz target
`fuzz/fuzz_targets/qc_progs.rs` (`cargo xtask fuzz qc-progs`):

- Arbitrary bytes never panic the reader (PROGS-15).
- Every record of a valid file reads back exactly as stored (PROGS-14.1).
- A file the reader accepts whose tables do not overlap writes back byte for byte
  (PROGS-22.1); one whose tables overlap writes back in qcc's layout with the same records.
- Records that pass PROGS-15 read back exactly as written (PROGS-22.5).
- The table-driven CRC equals the bitwise CRC of its polynomial (PROGS-4.1).
