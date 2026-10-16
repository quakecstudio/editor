# .lno line maps

Prefix: `LNO` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.lno` file: a table that gives the source line, and optionally the source
column, of every statement in a `progs.dat`. id never defined it: id's `qcc` keeps
`statement_linenums` in memory only to print statements (`qcc/qcc.c`, `PrintStatement`,
Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`) and never writes it, and id's engine
does not read one (`WinQuake/pr_edict.c`, `PR_LoadProgs`,
Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`). The format is described from the programs
that write and read it, cited for description only; none is a registered upstream for porting
([`../porting.md`](../porting.md#upstreams)):

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| FTEQCC | [fte-team/fteqw](https://github.com/fte-team/fteqw)@`f937b9d88f71fc4429db5fe56c6a98d922711b2e` (the commit [`progs.md`](progs.md) cites) | `engine/qclib/qccmain.c`, `qcc_pr_comp.c`, `qcc_pr_lex.c` (writer) |
| FTE | the same commit | `engine/qclib/pr_edict.c`, `PR_ReallyLoadProgs` (reader) |
| GMQCC | [graphitemaster/gmqcc](https://github.com/graphitemaster/gmqcc)@`b2a319efb41069b9a250e76991dbe505ff8d030d` | `code.cpp`, `ir.cpp`, `main.cpp`, `gmqcc.h`, `lexer.cpp`, `doc/gmqcc.1` (writer) |
| DarkPlaces | [DarkPlacesEngine/darkplaces](https://github.com/DarkPlacesEngine/darkplaces)@`5d80f03e26871b273da103f3b750820a3718c01a` | `prvm_edict.c`, `PRVM_LoadLNO`; `prvm_exec.c`, `PRVM_WhereAmI` (reader) |

Public documentation is one line of GMQCC's manual page (`doc/gmqcc.1`): `-flno` "Produce a
linenumber file along with the output .dat file." No other published description of the
layout was found; everything below is from the code.

Implemented by `qc-lno` (P1-28). Every statement in this file is in
[`coverage-baseline.tsv`](coverage-baseline.tsv) until that crate cites it.

`i32` and `u32` are 32-bit little-endian integers. The counts the file shares with a
`progs.dat` are the header fields of PROGS-2.1.

## LNO-1 Which programs write and read a .lno

**Source:** FTEQCC `qccmain.c`, `WriteData` (the `statement_linenums` setup and the block
after "Compile finished"), the `optimisations` table and `QCC_SetDefaultProperties`; FTE
`pr_edict.c`, `PR_ReallyLoadProgs`; GMQCC `ir.cpp`, `ir_builder::generate`, `main.cpp`,
`options_parse`; DarkPlaces `prvm_edict.c`, `PRVM_LoadLNO`.

These statements say when a `.lno` exists, so a reader knows which files to expect beside a
`progs.dat`.

### Behaviour

- **LNO-1.1** FTEQCC writes a `.lno` for every compile unless the `filenames` optimisation is
  on (`if (!opt_filenames)` builds `statement_linenums`, otherwise it is `NULL` and nothing is
  written). That optimisation is off with no `-O` option (it lacks `FLAG_ASDEFAULT`) and on
  from `-O1` (its level is 1); `-flno` turns it back off with the warning "Disabling
  -Ofilenames to satisfy -flno request".
- **LNO-1.2** FTEQCC writes no `.lno` for the `fte_debug` target (`QCF_FTEDEBUG`): that target
  stores the line table inside the progs (PROGS-21.1) and sets `statement_linenums = NULL`
  after doing so.
- **LNO-1.3** GMQCC writes a `.lno` only with `-flno`, which `-g` also turns on (`case 'g':
  opts_setflag("LNO", true)`), and only when it has exactly one line entry per statement
  (otherwise it prints `Linecounter wrong` and writes the progs without a `.lno`).
- **LNO-1.4** The `.lno` path is the output progs path with its last extension replaced by
  `.lno`: `progs.dat` gives `progs.lno`, `qwprogs.dat` gives `qwprogs.lno`. GMQCC appends
  `.lno` to a name with no `.`. FTEQCC also strips a trailing `.gz` first and appends it again
  (`progs.dat.gz` gives `progs.lno.gz`); a compressed `.lno` is outside this spec.
- **LNO-1.5** The readers look for the `.lno` beside the progs they load, by the same name
  rule (FTE `StripExtension` then `.lno`; DarkPlaces `FS_StripExtension` then `.lno`),
  through the game filesystem. A missing `.lno` is not an error: the progs runs without line
  numbers.
- **LNO-1.6** FTE reads a `.lno` only when the progs has no line table of its own: a version 7
  file with `ofslinenums` not 0 (PROGS-21.1) uses that table and the `.lno` is not opened
  (`if (!pr_linenums)`).

## LNO-2 Header

**Source:** FTEQCC `qccmain.c`, `WriteData` (`lnotype`, `version` and the six `SafeWrite`
calls); GMQCC `code.cpp`, `code_write`; FTE `pr_edict.c`, `PR_ReallyLoadProgs`; DarkPlaces
`prvm_edict.c`, `PRVM_LoadLNO`.

| Offset | Size | Field | Type | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `magic` | char[4] | `LNOF` |
| 4 | 4 | `version` | u32 | 1 |
| 8 | 4 | `numglobaldefs` | u32 | The progs header's `numglobaldefs` |
| 12 | 4 | `numglobals` | u32 | The progs header's `numglobals` |
| 16 | 4 | `numfielddefs` | u32 | The progs header's `numfielddefs` |
| 20 | 4 | `numstatements` | u32 | The progs header's `numstatements`; the length of the line table |

### Behaviour

- **LNO-2.1** A `.lno` starts with the 24-byte header above: the four bytes `LNOF` (0x4C 0x4E
  0x4F 0x46), then five 32-bit little-endian integers. FTEQCC writes the magic as the bytes of
  `*(unsigned int*)"LNOF"` and GMQCC as `fwrite("LNOF", 4, 1, fp)`; both put the same four
  bytes in that order on a little-endian machine.
- **LNO-2.2** The only version any writer produces or any reader accepts is 1. FTE rejects any
  other value (`file[1] != version`), as DarkPlaces does (`LittleLong(header[1]) == 1`). There
  is no version 2: GMQCC's column table (LNO-3.3) is added without changing the version.
- **LNO-2.3** The four counts are copied from the progs written in the same compile: FTEQCC
  writes its globals `numglobaldefs`, `numpr_globals`, `numfielddefs` and `numstatements`, the
  same values it stores in `progs.numglobaldefs`, `progs.numglobals`, `progs.numfielddefs` and
  `progs.numstatements`; GMQCC writes `code_header.defs.length`, `globals.length`,
  `fields.length` and `statements.length`, the fields of the progs header it writes next.
- **LNO-2.4** GMQCC byte-swaps every value to little-endian before writing
  (`util_endianswap`). FTEQCC writes the counts and lines in host byte order with no swap, and
  FTE compares them unswapped. This spec reads and writes little-endian only, which is what
  both produce on the x86 and ARM hosts they run on; a file FTEQCC wrote on a big-endian host
  fails LNO-5.2 or LNO-4.1.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LNO_MAGIC` | `LNOF` | FTEQCC `qccmain.c`, `lnotype` in `WriteData`; GMQCC `code.cpp`, `code_write` |
| `LNO_VERSION` | 1 | FTEQCC `qccmain.c`, `version` in `WriteData`; FTE `pr_edict.c`, `version` in `PR_ReallyLoadProgs` |
| `LNO_HEADER_SIZE` | 24 | Six 4-byte writes before the line table in `WriteData` and `code_write` |

## LNO-3 Line and column tables

**Source:** FTEQCC `qccmain.c`, `WriteData`; `qcc_pr_comp.c`, the statement builders that set
`linenum` and `QCC_PR_SimpleInitStatement`; `qcc_pr_lex.c`, `QCC_PR_Lex`
(`pr_token_line_last`) and the `#pragma line` handler. GMQCC `code.cpp`,
`code_push_statement`, `code_write`, `code_t::code_t`; `gmqcc.h`, `code_t`, `lex_ctx_t`;
`lexer.cpp`, `lex_open`, `lex_fgetc`, `lex_ungetch`, `lex_do`. DarkPlaces `prvm_edict.c`, `PRVM_LoadLNO`.

### Behaviour

- **LNO-3.1** The line table follows the header: `numstatements` i32 values, the entry at index
  `i` being the source line of statement `i`. FTEQCC writes `statement_linenums`, an `int`
  array filled from `statements[i].linenum`; GMQCC writes `code->linenums`, a
  `std::vector<int>` with one entry pushed for every statement.
- **LNO-3.2** A line number is 1-based: both lexers start a file at line 1 (FTEQCC
  `pr_source_line = 1`; GMQCC "we start counting at 1"). FTEQCC takes the line of the last
  token read when the statement was emitted (`pr_token_line_last`), so a statement can carry
  the line of the token that ended the expression rather than the one that began it; FTEQCC's
  `#pragma line(n)` makes the following line `n`. The value 0 means the writer had no line:
  GMQCC's statement 0 (pushed with `empty_ctx`, line 0) and FTEQCC's global initialiser
  statements (`linenum = 0`, "the file won't be meaningful so nor will the line").
- **LNO-3.3** GMQCC writes a column table straight after the line table: `numstatements` i32
  values from `code->columnnums`, one per statement, so its files are exactly
  `24 + 8 * numstatements` bytes. FTEQCC writes no column table, so its files are exactly
  `24 + 4 * numstatements` bytes.
- **LNO-3.4** A column is the value of GMQCC's lexer column counter when the statement's
  source context was taken: the counter is 0 at the start of each line, goes up by one for each
  byte `lex_fgetc` reads and down by one for each `lex_ungetch`, with an extra adjustment for
  multi-byte UTF-8 characters in string literals. The column of statement 0 is 0. This spec
  treats the value as GMQCC's own measure: it keeps it as written and does not convert it to a
  1-based or character-based column.
- **LNO-3.5** Line and column values are stored as signed `int`; GMQCC narrows its `size_t`
  counters into them. No writer produces a negative value for an ordinary source file, but a
  reader keeps any i32 unchanged and does not reject negative values.
- **LNO-3.6** DarkPlaces reads the column table only when the file is strictly longer than
  `24 + 8 * numstatements` bytes (`filesize > (6 + 2 * numstatements) * sizeof(int)`), so it
  ignores the columns of a file GMQCC wrote, which is exactly that long. This spec
  deliberately differs (LNO-5.4): a file of at least that length has a column table, because
  that is the layout GMQCC writes.
- **LNO-3.7** No writer puts anything after the tables. FTE does not look at the file size at
  all and DarkPlaces checks only a minimum (LNO-5.3), so both ignore bytes after the tables.

## LNO-4 Matching a progs.dat

**Source:** FTE `pr_edict.c`, `PR_ReallyLoadProgs`; DarkPlaces `prvm_edict.c`,
`PRVM_LoadLNO`; DarkPlaces `prvm_exec.c`, `PRVM_WhereAmI`, `PRVM_PrintStatement`,
`PRVM_StackTrace`.

A `.lno` is only meaningful with the progs it was written with; the counts are the check that
it is that progs.

### Behaviour

- **LNO-4.1** A `.lno` belongs to a progs when all four counts equal that progs' header fields
  `numglobaldefs`, `numglobals`, `numfielddefs` and `numstatements` (PROGS-2.1), compared as
  unsigned 32-bit values (FTE compares `unsigned int` file words with the `int` header fields;
  DarkPlaces casts both sides to `unsigned int`). Both readers check exactly these four and
  nothing else: no CRC and no checksum of the progs.
- **LNO-4.2** When the counts do not match, the readers drop the `.lno` silently and run the
  progs without line numbers (FTE: "whoops: old progs or incompatible"). A reader of this spec
  reports the mismatch as a typed error naming the first field that differs, in header order,
  and leaves the decision to drop it to the caller.
- **LNO-4.3** A stale `.lno` whose counts happen to equal the new progs' passes LNO-4.1 and
  gives wrong lines; the format cannot detect it.
- **LNO-4.4** A `.lno` holds no file names. A statement's source file is the `s_file` of the
  function whose statements contain it (PROGS-10.1), as DarkPlaces prints it
  (`PRVM_GetString(prog, func->s_file)` beside `statement_linenums[statement]`). A function
  whose body includes code from another file therefore reports that code's lines against the
  function's own file.
- **LNO-4.5** Statement 0 and any statement outside every function have no file. Their line
  entries are still present and are kept as read.

## LNO-5 Reading a .lno

**Source:** the editor's own rules, built on LNO-2 and LNO-3. FTE reads `file[0]` to
`file[5]` and the line table without checking the file size, which reads past the buffer of a
short file and is undefined; DarkPlaces checks only that the file holds the header and the
line table.

### Behaviour

- **LNO-5.1** A file shorter than 24 bytes is rejected with a typed error.
- **LNO-5.2** A file whose first four bytes are not exactly `LNOF` (case-sensitive) is
  rejected with a typed error.
- **LNO-5.3** A `version` other than 1 is rejected with a typed error carrying the version.
- **LNO-5.4** With `n` the header's `numstatements`, computed without overflow: a file shorter
  than `24 + 4n` bytes is rejected with a typed error (the line table is truncated). A file of
  at least `24 + 8n` bytes, with `n` above 0, has a column table of `n` entries after the line
  table. Otherwise there is no column table.
- **LNO-5.5** Bytes after the last table the file has (LNO-5.4) are trailing bytes. The reader
  does not reject them; it keeps them so that the writer can reproduce the file (LNO-6.2).
- **LNO-5.6** The checks run in the order LNO-5.1, LNO-5.2, LNO-5.3, LNO-5.4, and the first
  failure is reported. Reading a `.lno` does not need its progs; LNO-4.1 is a separate check
  on a file that has been read.
- **LNO-5.7** Reading never fails on the values: line and column values are kept as read
  (LNO-3.5), and the three counts other than `numstatements` are kept as read whatever they
  are.

## LNO-6 Writing a .lno

**Source:** FTEQCC `qccmain.c`, `WriteData`; GMQCC `code.cpp`, `code_write`. The checks are
the editor's own.

### Behaviour

- **LNO-6.1** The writer writes `LNOF`, version 1, `numglobaldefs`, `numglobals` and
  `numfielddefs` as given, `numstatements` as the length of the line table, the line table,
  the column table if there is one, then the trailing bytes if any. The output is
  `24 + 4n` bytes with no column table (FTEQCC's layout) and `24 + 8n` with one (GMQCC's),
  plus the trailing bytes.
- **LNO-6.2** A file the reader accepts is written back byte for byte, and reading what the
  writer wrote gives back the counts, tables and trailing bytes it was given.
- **LNO-6.3** The writer rejects, with a typed error and no output: a column table whose length
  differs from the line table's, a column table when the line table is empty (the reader
  would read it back as no column table, LNO-5.4), and a line table longer than 2147483647
  entries (the largest `numstatements` PROGS-2.1's `i32` can hold).
- **LNO-6.4** The editor's compiler writes the three counts from the progs it writes in the
  same build, so that the result passes LNO-4.1 against that progs.

## Test vectors

Derived on 2026-10-16 from the writers' code (FTEQCC `WriteData`, GMQCC `code_write`) and the
readers' checks (FTE `PR_ReallyLoadProgs`, DarkPlaces `PRVM_LoadLNO`) at the commits above.
They have not been checked against a file written by a running compiler: building FTEQCC is
the oracles' job (P1-26), and `qc-lno` (P1-28) round-trips the `.lno` files that build writes.
Bytes are hex, four per group.

The counts used throughout are `numglobaldefs` 3 (`03000000`), `numglobals` 30 (`1e000000`)
and `numfielddefs` 1 (`01000000`). `H(n)` is the header with those counts and
`numstatements` `n`: `4c4e4f46 01000000 03000000 1e000000 01000000` then `n`.

| Input | Result | Statements |
| --- | --- | --- |
| `H(02000000) 00000000 05000000` (32 bytes, FTEQCC layout) | Lines `[0, 5]`, no columns | LNO-2.1, LNO-3.1, LNO-3.3, LNO-5.4 |
| `H(02000000) 00000000 05000000 00000000 04000000` (40 bytes, GMQCC layout) | Lines `[0, 5]`, columns `[0, 4]` | LNO-3.3, LNO-3.4, LNO-5.4 |
| `H(00000000)` (24 bytes) | No lines, no columns | LNO-5.4 |
| `H(02000000) 00000000 05000000 aabbccdd` (36 bytes) | Lines `[0, 5]`, no columns, trailing `aabbccdd` | LNO-5.4, LNO-5.5 |
| `H(02000000) 00000000 05000000 00000000 04000000 aabbccdd` (44 bytes) | Lines `[0, 5]`, columns `[0, 4]`, trailing `aabbccdd` | LNO-5.4, LNO-5.5 |
| `H(02000000) ffffffff 05000000` | Lines `[-1, 5]` | LNO-3.5, LNO-5.7 |
| The first 23 bytes of `H(00000000)` | Error: too short | LNO-5.1 |
| `H(00000000)` with byte 3 `66` (`LNOf`) | Error: bad magic | LNO-5.2 |
| `H(00000000)` with version `02000000` | Error: unsupported version 2 | LNO-2.2, LNO-5.3 |
| `H(02000000) 00000000` (28 bytes) | Error: line table truncated | LNO-5.4 |
| `H(ffffffff)` (24 bytes) | Error: line table truncated, with no overflow | LNO-5.4 |
| `4c4e4f47` then 20 bytes, version `02000000` | Error: bad magic, not version | LNO-5.6 |

Matching against a progs whose header has `numglobaldefs` 3, `numglobals` 30, `numfielddefs`
1 and `numstatements` 2:

| `.lno` | Result | Statements |
| --- | --- | --- |
| The 32-byte FTEQCC-layout vector above | Matches | LNO-4.1 |
| The same with `numglobals` `1f000000` | Mismatch on `numglobals` | LNO-4.1, LNO-4.2 |
| The same with `numglobaldefs` `04000000` and `numstatements` `03000000` and a third line | Mismatch on `numglobaldefs`, the first field in header order | LNO-4.2 |

Writing:

| Input | Result | Statements |
| --- | --- | --- |
| Counts 3, 30, 1; lines `[0, 5]`; no columns | The 32-byte FTEQCC-layout vector | LNO-6.1 |
| Counts 3, 30, 1; lines `[0, 5]`; columns `[0, 4]` | The 40-byte GMQCC-layout vector | LNO-6.1 |
| Lines `[0, 5]`; columns `[0]` | Error: column table length | LNO-6.3 |
| No lines; columns `[]` | Error: column table with no lines | LNO-6.3 |
| Every accepted vector above, read then written | The same bytes | LNO-6.2 |
