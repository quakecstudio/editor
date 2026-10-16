# progs.dat test vectors

Hand-written `progs.dat` files for [`../../progs.md`](../../progs.md), stored as `xxd` hex
dumps: `xxd -r <name>.txt > <name>.dat` rebuilds the file byte for byte. Each section below
gives the file's expected decoded values. They are not compiler output; they follow the
layout id's qcc writes (PROGS-1.3) so a reader sees what a real file looks like.

**Checked:** on 2026-10-16 the version 6 files were loaded by id's `PR_LoadProgs`, copied
unchanged from `WinQuake/pr_edict.c` at id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`
into a local harness that stubs only the file system, `Sys_Error` and the edict structs, and
built with id's `crc.c` from the same commit. The harness printed every decoded field and
`pr_crc`; the output matched the values below. id's loader rejects both FTE files with
`progs.dat has wrong version number (7 should be 6)` (PROGS-3.1); their version 7 values were
checked against FTEQW's `dprograms_t` by reading, not by running FTE. The vectors join the
oracle suite when P1-11 lands.

| File | Size | SHA-256 of the rebuilt file | File CRC (PROGS-6) |
| --- | --- | --- | --- |
| [`minimal.txt`](minimal.txt) | 236 | `edf2a29784963484407aec800ba0ec6ac0dd864a769047c75c0b8c83ac8731ba` | 25104 |
| [`small.txt`](small.txt) | 744 | `7be3d319e48aad1bccd9c685ee5d2a45e2afc2c77989156204f75d241628d94b` | 36518 |
| [`fte16-minimal.txt`](fte16-minimal.txt) | 268 | `4b9ce4b1f452bebf8bd586dc9ab816a000f856c43ff2307c52636cf6d1539a43` | 62395 |
| [`fte32-minimal.txt`](fte32-minimal.txt) | 284 | `909317f8ef623240d9cdf65a10d251c45ced014cc09dabca7dc385e5d5058824` | 3494 |

## minimal

What id's qcc writes for a program with no definitions: every table holds only its reserved
zero record (PROGS-1.5), the string table is one NUL padded to 4 bytes, and the globals are
the 28 reserved slots (PROGS-12.3). Statements: PROGS-1.3, PROGS-1.5, PROGS-2.1, PROGS-3.1,
PROGS-3.2, PROGS-11.2, PROGS-11.5, PROGS-12.3, PROGS-18.2.

| Header field | Value |
| --- | --- |
| `version` | 6 |
| `crc` | 5927 |
| `ofs_statements`, `numstatements` | 64, 1 |
| `ofs_globaldefs`, `numglobaldefs` | 108, 1 |
| `ofs_fielddefs`, `numfielddefs` | 116, 1 |
| `ofs_functions`, `numfunctions` | 72, 1 |
| `ofs_strings`, `numstrings` | 60, 4 |
| `ofs_globals`, `numglobals` | 124, 28 |
| `entityfields` | 0 |

Statement 0, function 0, global definition 0 and field definition 0 are all zero; every
global is 0.

## small

A builtin, a QC function with a parameter and a local, two fields (one a vector), and a global
of every type QuakeC can initialise. As QuakeC it would read:

```c
.float health;
.vector origin;
float counter;
vector v = '1 2 3';
string greeting = "hi\n";
entity self;
void(string s) dprint = #25;
void(float f) bump = { local float t; t = f; counter = counter + t; dprint(greeting); };
```

Statements: PROGS-1.3, PROGS-2.1, PROGS-2.2, PROGS-7.1, PROGS-9.1, PROGS-9.3, PROGS-9.4,
PROGS-9.5, PROGS-10.1, PROGS-10.2, PROGS-10.4, PROGS-11.1, PROGS-11.4, PROGS-12.1,
PROGS-13.1, PROGS-13.2, PROGS-16.1, PROGS-17.2, PROGS-17.3, PROGS-17.5, PROGS-17.6.

| Header field | Value |
| --- | --- |
| `version` | 6 |
| `crc` | 5927 |
| `ofs_statements`, `numstatements` | 220, 7 |
| `ofs_globaldefs`, `numglobaldefs` | 384, 17 |
| `ofs_fielddefs`, `numfielddefs` | 520, 6 |
| `ofs_functions`, `numfunctions` | 276, 3 |
| `ofs_strings`, `numstrings` | 60, 160 |
| `ofs_globals`, `numglobals` | 568, 44 |
| `entityfields` | 4 |

Strings, by offset: 0 `""`, 1 `test.qc`, 9 `dprint`, 16 `bump`, 21 `hi\n` (with a 0x0A byte),
25 `health`, 32 `origin`, 39 `origin_x`, 48 `origin_y`, 57 `origin_z`, 66 `counter`, 74 `v`,
76 `v_x`, 80 `v_y`, 84 `v_z`, 88 `greeting`, 97 `self`, 102 `dprint`, 109 `bump`, 114 `f`,
116 `t`, 118 `health`, 125 `origin`, 132 `origin_x`, 141 `origin_y`, 150 `origin_z`; byte
159 is the one padding NUL. `health`, `origin` and the others appear twice because qcc does
not merge strings (PROGS-11.3).

Statements:

| # | `op` | `a` | `b` | `c` |
| --- | --- | --- | --- | --- |
| 0 | 0 `DONE` | 0 | 0 | 0 |
| 1 | 31 `STORE_F` | 41 | 42 | 0 |
| 2 | 6 `ADD_F` | 33 | 42 | 43 |
| 3 | 31 `STORE_F` | 43 | 33 | 0 |
| 4 | 33 `STORE_S` | 37 | 4 | 0 |
| 5 | 52 `CALL1` | 39 | 0 | 0 |
| 6 | 0 `DONE` | 0 | 0 | 0 |

Functions (`profile` is 0 and `parm_size[1..8)` is 0 in every row):

| # | `first_statement` | `parm_start` | `locals` | `s_name` | `s_file` | `numparms` | `parm_size[0]` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 0 | 0 | 0 | 0 `""` | 0 `""` | 0 | 0 |
| 1 | -25 | 0 | 0 | 9 `dprint` | 1 `test.qc` | 1 | 1 |
| 2 | 1 | 41 | 3 | 16 `bump` | 1 `test.qc` | 1 | 1 |

Global definitions (`type` as stored; 0x8000 is `DEF_SAVEGLOBAL`):

| # | `type` | `ofs` | `s_name` |
| --- | --- | --- | --- |
| 0 | 0 | 0 | 0 `""` |
| 1 | 5 field | 28 | 25 `health` |
| 2 | 5 field | 29 | 32 `origin` |
| 3 | 5 field | 30 | 39 `origin_x` |
| 4 | 5 field | 31 | 48 `origin_y` |
| 5 | 5 field | 32 | 57 `origin_z` |
| 6 | 0x8002 float, saved | 33 | 66 `counter` |
| 7 | 3 vector | 34 | 74 `v` |
| 8 | 2 float | 34 | 76 `v_x` |
| 9 | 2 float | 35 | 80 `v_y` |
| 10 | 2 float | 36 | 84 `v_z` |
| 11 | 1 string | 37 | 88 `greeting` |
| 12 | 0x8004 entity, saved | 38 | 97 `self` |
| 13 | 6 function | 39 | 102 `dprint` |
| 14 | 6 function | 40 | 109 `bump` |
| 15 | 2 float | 41 | 114 `f` |
| 16 | 2 float | 42 | 116 `t` |

Field definitions:

| # | `type` | `ofs` | `s_name` |
| --- | --- | --- | --- |
| 0 | 0 | 0 | 0 `""` |
| 1 | 2 float | 0 | 118 `health` |
| 2 | 3 vector | 1 | 125 `origin` |
| 3 | 2 float | 1 | 132 `origin_x` |
| 4 | 2 float | 2 | 141 `origin_y` |
| 5 | 2 float | 3 | 150 `origin_z` |

Globals: slots 0 to 27, 33, 38 and 41 to 43 are 0. The others, read as the type their
definition gives:

| Slot | Value | Bits |
| --- | --- | --- |
| 28 | field 0 | 0x00000000 |
| 29 | field 1 | 0x00000001 |
| 30 | field 1 | 0x00000001 |
| 31 | field 2 | 0x00000002 |
| 32 | field 3 | 0x00000003 |
| 34 | 1.0 | 0x3F800000 |
| 35 | 2.0 | 0x40000000 |
| 36 | 3.0 | 0x40400000 |
| 37 | string 21 (`hi\n`) | 0x00000015 |
| 39 | function 1 (`dprint`) | 0x00000001 |
| 40 | function 2 (`bump`) | 0x00000002 |

## fte16-minimal

[`minimal`](#minimal) as FTE version 7 with the 16-bit layout: a 92-byte header and the same
tables. Statements: PROGS-19.1, PROGS-19.2, PROGS-21.6.

| Header field | Value |
| --- | --- |
| `version` | 7 |
| `crc` | 5927 |
| `ofs_statements`, `numstatements` | 96, 1 |
| `ofs_globaldefs`, `numglobaldefs` | 140, 1 |
| `ofs_fielddefs`, `numfielddefs` | 148, 1 |
| `ofs_functions`, `numfunctions` | 104, 1 |
| `ofs_strings`, `numstrings` | 92, 4 |
| `ofs_globals`, `numglobals` | 156, 28 |
| `entityfields` | 0 |
| `ofsfiles`, `ofslinenums`, `ofsbodylessfuncs`, `numbodylessfuncs`, `ofs_types`, `numtypes`, `blockscompressed` | all 0 |
| `secondaryversion` | 0x021B1461 |

## fte32-minimal

[`minimal`](#minimal) as FTE version 7 with the 32-bit layout: the statement is 16 bytes and
each definition 12. Statements: PROGS-19.1, PROGS-19.2, PROGS-20.1, PROGS-20.2, PROGS-20.3.

| Header field | Value |
| --- | --- |
| `version` | 7 |
| `crc` | 5927 |
| `ofs_statements`, `numstatements` | 96, 1 |
| `ofs_globaldefs`, `numglobaldefs` | 148, 1 |
| `ofs_fielddefs`, `numfielddefs` | 160, 1 |
| `ofs_functions`, `numfunctions` | 112, 1 |
| `ofs_strings`, `numstrings` | 92, 4 |
| `ofs_globals`, `numglobals` | 172, 28 |
| `entityfields` | 0 |
| `ofsfiles`, `ofslinenums`, `ofsbodylessfuncs`, `numbodylessfuncs`, `ofs_types`, `numtypes`, `blockscompressed` | all 0 |
| `secondaryversion` | 0x65167402 |
