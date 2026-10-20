# progs.dat and .lno formats

**Crates:** `qc-types`, `qc-progs`, `qc-lno` (L0). **Specs:** PROGS-1 to PROGS-22, LNO-1 to LNO-6, LIMIT-1 to LIMIT-7.

## Context and design philosophy

The VM, the compiler, the language server and the differential tools all read and write the
same compiled program. This leaf owns those bytes: the QC type model (`qc-types`), the
`progs.dat` format (`qc-progs`) and the `.lno` line map (`qc-lno`). All three are L0 crates
(layers.toml) with no dependency above `qc-types`.

The guiding rule is the project's: the format is what id's `PR_LoadProgs` accepts at
id-Software/Quake@bf4ac42 (docs/porting.md, Upstreams), written up in docs/spec/progs.md, and
the code is a port of it with a differential result (progs.md, Test vectors: 3,000 generated
files against id's loader). Where id's C reads out of bounds or trusts the file, the reader
refuses instead, and each refusal is a spec statement (PROGS-15) listed as an expected
divergence in docs/spec/differential.md DIFF-10.1.

`.lno` is not id's format. lno.md describes it from FTEQCC, GMQCC and DarkPlaces for
description only, so qc-lno carries no id port header (crates/qc-lno/README.md:5-8).

## Components

### Types and slots (qc-types)

`Etype` is id's `etype_t` (8 values), each with a slot count (vector 3, every other type 1,
pointer fixed at 1 as on the 32-bit engine, crates/qc-types/src/lib.rs:144-157). `DefType`
keeps numbers above 7 rather than rejecting them (lib.rs:196-203, PROGS-16.3). `Slot(u32)` is the
raw 4-byte global or field word; `Value` reads and writes a typed value across one or three
slots, failing with `TooFewSlots` rather than panicking. Reference newtypes wrap the i32 the
VM stores: `EntityRef::number(edict_size)` returns `None` for a zero edict size or an offset
that is not a whole multiple (lib.rs:316-322). Reserved offsets (`PROGS_OFS_NULL`,
`PROGS_OFS_RETURN`, `PROGS_OFS_PARM0`, `ofs_parm(n)`, `PROGS_RESERVED_OFS` = 28) implement
PROGS-18.

### Reading a progs.dat (qc-progs)

`Progs::read(&[u8])` (or `read_with(ReadOptions { check_header_crc })`) borrows the bytes and
validates, in the order PROGS-15.5 / PROGS-15.9 / PROGS-21.9 fix (crates/qc-progs/src/read.rs:331-415):

1. Header length, version (6, or 7 with `fte`), header CRC 5927 (optional check).
2. FTE layout and compression (v7 only).
3. Every table inside the file.
4. No field def has `DEF_SAVEGLOBAL`; every string reference is inside the string table and
   NUL-terminated; no function declares more than 8 parameters (negative counts are accepted,
   PROGS-10.5, read.rs:456-467).

The result is a set of `Table<'a, T>` views. Records (`Statement`, `Def`, `Function`, `Slot`,
`i32`) are decoded lazily on each access through the sealed `Record` trait
(crates/qc-progs/src/records.rs:374-388), widened so one struct serves v6, Fte16 and Fte32
layouts: v6 operands sign-extend, Fte16 zero-extend, Fte32 are raw (records.rs:396-416).
`ed_global_at_ofs`, `ed_field_at_ofs`, `ed_find_field`, `ed_find_global` and `ed_find_function`
port id's `ED_*` lookups. `file_crc()` is PROGS-6.1. `layout()` captures each table's offset and
the lead bytes for byte-exact rewriting, or `None` if any tables overlap (read.rs:632-672).

Errors are one `ReadError` enum with id-style messages: `TooShort`, `WrongVersion`,
`HeaderCrc`, (fte) `UnsupportedFteLayout`, `Compressed`, `TableOutOfBounds`,
`FieldDefSaveGlobal`, `StringOutOfRange`, `UnterminatedString`, `TooManyParms`.

### Writing a progs.dat (qc-progs)

`Progs::to_file()` produces an owned `ProgsFile`; `write(&ProgsFile)` encodes it. With a
`Layout` from the source, tables go back to their stored offsets, empty tables keep their
stored offsets and tables absent from the source are appended (crates/qc-progs/src/write.rs:312-320);
without one, tables go in qcc's `WriteData` order with the string table padded to 4
(write.rs:339-343). `blockscompressed` is always written 0 (write.rs:282). The writer checks only
what it must to encode (operand and def ranges, sizes, FTE part consistency); it does not repeat
the reader's PROGS-15 semantic checks, and PROGS-22.5 says a reader rejects instead.

### CRC, opcodes and limits (qc-progs)

`crc` ports id's `CRC_Init` / `CRC_ProcessByte` / `CRC_Value` (CRC-16/CCITT-FALSE) with the
table computed by a `const fn` (crates/qc-progs/src/crc.rs:29-46); `qs-pak` reuses it.
`Opcode` is the 66-value vanilla enum with number and name lookups (PROGS-8). `limits` holds
every LIMIT constant (VM, server, qcc) in L0 so higher layers share one definition
(limits.rs:5-7); `qc-vm` re-exports four of them.

### The .lno line map (qc-lno)

`LnoFile::read` checks length, magic `LNOF`, version 1 and the line-table length in LNO-5.6
order and returns an owned `LnoFile { numglobaldefs, numglobals, numfielddefs, lines,
columns, trailing }`; trailing bytes are kept so `write` round-trips byte for byte (LNO-6.2).
A column table is assumed when the file has at least 24 + 8n bytes with n > 0
(crates/qc-lno/src/read.rs:119-131, LNO-5.4). `matches` / `matches_header` compare the counts
with a progs (LNO-4.1). `write` refuses a column count that differs from the line count,
columns without lines, and more than `LNO_MAX_STATEMENTS` (= i32::MAX) lines (LNO-6.3).

## Error model

- `Progs::read` returns one `ReadError` with id-style messages: `TooShort`, `WrongVersion`,
  `HeaderCrc`, (fte) `UnsupportedFteLayout`, `Compressed`, `TableOutOfBounds`,
  `FieldDefSaveGlobal`, `StringOutOfRange`, `UnterminatedString`, `TooManyParms`. Each refusal
  of a file id's C would read past is a PROGS-15 statement and an expected divergence in
  DIFF-10.1.
- `write` returns `WriteError` (`StatementOutOfRange`, `DefOutOfRange`, `TooLarge`,
  `LineNumberCount`, `NulInBodylessName`, `OpaqueTableWithoutLayout`) for what it cannot
  encode.
- `LnoFile::read` and `write` have their own `ReadError` and `WriteError`
  (`ColumnCountMismatch`, `ColumnsWithoutLines`, `TooManyStatements`).
- `Value` reads and writes fail with `TooFewSlots`; nothing panics.

## Verification

- Unit and property tests in each crate (round trips for every encoder and decoder,
  PROGS-22.1, LNO-6.2), with fixtures from the xxd dumps in `docs/spec/vectors/progs/`.
- Differential: 3,000 generated files against id's loader (progs.md, Test vectors).
- `cargo xtask corpus build` round-trips the progs.dat and `.lno` the corpus builds.
- Fuzz target `qc_progs`. L0 readiness passed (`docs/readiness/p1-r0.md`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Port id's loader rather than write one from a description | Direct port of `PR_LoadProgs` and `ED_*` | Clean-room reader | ADR-0002 (GPL-3.0-or-later) permits porting; docs/porting.md requires the four parts |
| Refuse malformed files id's C would read past | Typed `ReadError`, recorded as divergences | Reproduce id's out-of-bounds reads | PROGS-15 and DIFF-10.1; no-panic rule in docs/conventions.md |
| Zero-copy reader with lazy record decode | `Progs<'a>` over `&[u8]` | Decode every table into owned Vecs at load | [inferred] keeps load cheap for the LSP and VM; validation loops still walk tables once |
| Byte-exact round trip through an opaque `Layout` | `Layout` captured by `Progs::layout`, private fields | Always rewrite in qcc order | [inferred] lets the editor rewrite a progs without reordering it; qcc order is the fallback (PROGS-22) |
| FTE v7 behind a cargo feature | `fte = []` | Always on | Plan requirement FND-1 (progs.md:518); CI builds with `--all-features` (xtask/src/ci.rs:65) |
| Limits as L0 constants | `qc_progs::limits` | Each crate defines its own | limits.rs:5-7: one definition for every layer |
| `.lno` support in its own crate | `qc-lno` depending only on `qc-progs::Header` | Inside qc-progs | [inferred] not an id format and no port header; keeps qc-progs a pure port |
| Column-table detection by length | Length >= 24 + 8n | DarkPlaces' rule | LNO-3.6 / LNO-5.4 record it as a deliberate divergence (lno.md:144-148) |
| Waive, not lower, qc-lno's mutation bar | Exception in ratchets.toml at 89% | Lower the ratchet | P1-57; four equivalent mutants listed in ratchets.toml `[crates.qc-lno]` |

## Open questions

### Resolved
1. FTE v7 read and write is implemented behind `fte` (P1-08 and follow-ups; tests in
   crates/qc-progs/src/read/tests/fte.rs).

### Deferred
1. **Fte32 type bits above 15.** `Def::def_type` keeps 15 bits of the low 16
   (crates/qc-progs/src/records.rs:330-331); fte.rs:139-163 pins the drop. PROGS-20 does not say
   whether the upper bits carry meaning. `PROGS_FTE_TYPE_SIZE` is defined but the FTE types
   table is never decoded.
2. **WrongVersion text** says "should be 6" even when v7 is accepted
   (crates/qc-progs/src/read.rs:148-151).
3. **v7 with a bad CRC reports CRC** because the format is provisionally Fte16 at that point
   (read.rs:340-346); progs.md PROGS-3.3 records it as a deliberate divergence.
4. **Port header coverage.** opcode.rs:5 names only `OP_DONE`; limits.rs has no port header.
5. **Duplicate LIMIT constants** in crates/qc-builtins/src/mock.rs:120-203.
6. **qc-lno test quality.** tests.rs:195-197 rereads the input instead of the output;
   tests.rs:214 is a tautology.
7. **`.lno` writer asymmetry.** The reader accepts any u32 `numstatements` but the writer
   refuses more than i32::MAX lines (crates/qc-lno/src/lib.rs:52); unreachable in practice
   (over 8 GiB).
8. **Fixture coupling.** crates/qc-progs/src/fixtures.rs:38-46 `include_str!`s the xxd dumps in
   docs/spec/vectors/progs/, so the crate's tests depend on files outside it.
9. **Duplicated little-endian readers** (`le_i32` in qs-pak, qs-wad, qs-wav, records.rs
   `read_i32`, qc-lno `read_u32`).
10. **Stale spec prose** "every statement is in the baseline" in progs.md:17-20, lno.md:26-27,
    limits.md:13-16; progs.md:18-19 and limits.md:132, 146-150 name sibling specs by plan ID.

## References

- docs/spec/progs.md, docs/spec/lno.md, docs/spec/limits.md
- docs/porting.md (upstreams, process); docs/adr/0002-license-model.md
- docs/readiness/p1-r0.md; ratchets.toml `[crates.qc-types]`, `[crates.qc-progs]`,
  `[crates.qc-lno]`
- crates/qc-types/README.md, crates/qc-progs/README.md, crates/qc-lno/README.md
- Plan: P1-07 (qc-types), P1-08 (qc-progs), P1-28 (qc-lno), P1-56 (corpus .lno round trip),
  P1-57 (L0 surviving mutants) in plan/backlog/p1.yaml
