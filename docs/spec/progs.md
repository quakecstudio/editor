# progs.dat format and QC type layout

Prefix: `PROGS` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the compiled QuakeC file, `progs.dat` version 6, as the engine loads it in
`WinQuake/pr_edict.c` (`PR_LoadProgs`) with the structures of `WinQuake/pr_comp.h`, and as
id's compiler writes it in `qcc/qcc.c` (`WriteData`, `PR_WriteProgdefs`). Sources:
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d` and the `qcc/` directory of
[id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools) at commit
`c0d1b91c74eb654365ac7755bc837e497caaca73`. Where the two disagree the engine wins
([`../porting.md`](../porting.md#which-source-is-authoritative)). The FTE version 7 extension
(PROGS-19 to PROGS-21) has no id source and is described from FTEQW, which is cited here for
description only; nothing in this file is ported from it.

Implemented by `qc-types` (PROGS-16 to PROGS-18) and `qc-progs` (the rest). Limits are named in
[`limits.md`](limits.md); opcode semantics are in [`vm.md`](vm.md); the system globals and
fields and the entity layout are in [`edicts.md`](edicts.md). A statement no test cites yet
is listed in [`coverage-baseline.tsv`](coverage-baseline.tsv) ([`README.md`](README.md)).

All offsets and sizes are in bytes unless a statement says slots. A **slot** is one 4-byte
cell of the globals or of an entity's fields. `i32`, `u16` and so on are little-endian
integers of that width; `f32` is an IEEE 754 single.

## PROGS-1 File layout and byte order

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `qcc/qcc.c`, `WriteData`.

The engine reads the whole file into one buffer and points each table into it at the offset
the header gives. It never copies or moves a table, so the tables can sit anywhere in the
file, in any order.

### Behaviour

- **PROGS-1.1** Every multi-byte integer and float in the file is little-endian. The engine
  converts with `LittleLong` and `LittleShort` after loading, so a big-endian host reads the
  same values.
- **PROGS-1.2** The file starts with the 60-byte header (PROGS-2) at offset 0. Each of the six
  tables (statements, global definitions, field definitions, functions, strings, globals) is
  located only by its offset and count in the header; the engine does not require any order,
  alignment, gap or absence of overlap between them.
- **PROGS-1.3** id's qcc writes the tables in this order, each directly after the previous one:
  header, strings, statements, functions, global definitions, field definitions, globals. It
  pads the string table with NUL bytes to a multiple of 4, so every table starts 4-byte aligned.
- **PROGS-1.4** Bytes in the file that no table covers are ignored, except by the file CRC
  (PROGS-6.1).
- **PROGS-1.5** The first record of the statements, functions, global definitions and field
  definitions tables, and the first byte of the string table, are reserved: qcc writes them as
  all zero bytes (`InitData` starts every count at 1). The engine does not check them.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_STRING_ALIGN` | 4 | `qcc/qcc.c`, `WriteData`: `strofs = (strofs+3)&~3;` |

## PROGS-2 Header

**Source:** `WinQuake/pr_comp.h`, `dprograms_t`; `WinQuake/pr_edict.c`, `PR_LoadProgs`.

### Behaviour

- **PROGS-2.1** The header is 15 consecutive `i32` fields, 60 bytes:

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 0 | 4 | `version` | i32 | Format version, 6 (PROGS-3) |
  | 4 | 4 | `crc` | i32 | CRC of the `progdefs.h` qcc generated (PROGS-5) |
  | 8 | 4 | `ofs_statements` | i32 | File offset of the statements table |
  | 12 | 4 | `numstatements` | i32 | Number of 8-byte statements |
  | 16 | 4 | `ofs_globaldefs` | i32 | File offset of the global definitions table |
  | 20 | 4 | `numglobaldefs` | i32 | Number of 8-byte global definitions |
  | 24 | 4 | `ofs_fielddefs` | i32 | File offset of the field definitions table |
  | 28 | 4 | `numfielddefs` | i32 | Number of 8-byte field definitions |
  | 32 | 4 | `ofs_functions` | i32 | File offset of the functions table |
  | 36 | 4 | `numfunctions` | i32 | Number of 36-byte functions |
  | 40 | 4 | `ofs_strings` | i32 | File offset of the string table |
  | 44 | 4 | `numstrings` | i32 | Size of the string table in bytes, not a count of strings |
  | 48 | 4 | `ofs_globals` | i32 | File offset of the globals |
  | 52 | 4 | `numglobals` | i32 | Number of 4-byte global slots |
  | 56 | 4 | `entityfields` | i32 | Number of 4-byte field slots in every entity (PROGS-13) |

- **PROGS-2.2** Every offset is measured from the first byte of the file. A table occupies
  `count * record size` bytes from its offset; for the string table the count is already in
  bytes and for the globals the record size is 4.
- **PROGS-2.3** The fields are signed in id's struct. The engine never compares an offset or a
  count with the file size and never checks a sign; a reader's checks are in PROGS-15.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_HEADER_SIZE` | 60 | `WinQuake/pr_comp.h`, `sizeof(dprograms_t)` |
| `PROGS_STATEMENT_SIZE` | 8 | `WinQuake/pr_comp.h`, `sizeof(dstatement_t)` |
| `PROGS_DEF_SIZE` | 8 | `WinQuake/pr_comp.h`, `sizeof(ddef_t)` |
| `PROGS_FUNCTION_SIZE` | 36 | `WinQuake/pr_comp.h`, `sizeof(dfunction_t)` |
| `PROGS_GLOBAL_SIZE` | 4 | `WinQuake/progs.h`, `pr_globals` is `float *`; `G_INT` reads `int` |

## PROGS-3 Version and header checks

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/progdefs.q1`, `PROGHEADER_CRC`.

### Behaviour

- **PROGS-3.1** A file whose `version` is not 6 is rejected with
  `progs.dat has wrong version number (<version> should be 6)`.
- **PROGS-3.2** A file whose `crc` is not 5927 is rejected with
  `progs.dat system vars have been modified, progdefs.h is out of date`.
- **PROGS-3.3** The version is checked before the CRC: a file wrong in both reports the
  version.
- **PROGS-3.4** A field definition whose `type` has bit 15 (`DEF_SAVEGLOBAL`) set is rejected
  with `PR_LoadProgs: pr_fielddefs[i].type & DEF_SAVEGLOBAL`. The same bit in a global
  definition is accepted (PROGS-9.3).
- **PROGS-3.5** These three are the only checks the engine makes on a loaded file; it does
  not check offsets, counts, string references, opcodes or operands at load. A missing file is
  rejected with `PR_LoadProgs: couldn't load progs.dat`.
- **PROGS-3.6** The header CRC identifies the system globals and fields a mod declares (PROGS-5),
  not the file: two different mods with the same system definitions carry the same value,
  5927 for the definitions in id's `defs.qc`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_VERSION` | 6 | `WinQuake/pr_comp.h`, `PROG_VERSION` |
| `PROGS_HEADER_CRC` | 5927 | `WinQuake/progdefs.q1`, `PROGHEADER_CRC`, checked by `PR_LoadProgs` |

## PROGS-4 CRC algorithm

**Source:** `WinQuake/crc.c`, `CRC_Init`, `CRC_ProcessByte`, `CRC_Value` (the same table and
functions are in `qcc/qcc.c`).

### Behaviour

- **PROGS-4.1** The CRC is the 16-bit, non-reflected CRC with polynomial 0x1021, initial value
  0xFFFF and final XOR 0x0000 (CRC-16/CCITT-FALSE). Each byte updates it as
  `crc = (crc << 8) ^ table[(crc >> 8) ^ byte]`, truncated to 16 bits, where `table[i]` is the
  CRC of the single byte `i` with a zero initial value.
- **PROGS-4.2** The CRC of no bytes is 0xFFFF (65535), and the CRC of the ASCII bytes
  `123456789` is 0x29B1.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_CRC_POLY` | 0x1021 | `WinQuake/crc.c`, comment above `crctable` |
| `PROGS_CRC_INIT` | 0xFFFF | `WinQuake/crc.c`, `CRC_INIT_VALUE` |
| `PROGS_CRC_XOR` | 0x0000 | `WinQuake/crc.c`, `CRC_XOR_VALUE` |

### Test vectors

Checked on 2026-10-16 by computing the polynomial form in Python and comparing it with
`crctable` copied from `WinQuake/crc.c` at the pinned commit (all 256 entries equal).

| Input | CRC | Statements |
| --- | --- | --- |
| (no bytes) | 0xFFFF | PROGS-4.2 |
| `123456789` (ASCII) | 0x29B1 | PROGS-4.1, PROGS-4.2 |
| `WinQuake/progdefs.q1` up to, not including, its `#define PROGHEADER_CRC` line (2,427 bytes) | 5927 | PROGS-5.2 |

## PROGS-5 The progdefs.h CRC

**Source:** `qcc/qcc.c`, `PR_WriteProgdefs`, `main`; `WinQuake/progdefs.q1`.

qcc writes `progdefs.h`, the C view of the system globals and fields, and stores the CRC of
that text in the header so the engine can tell whether the mod's system definitions match the
structs it was compiled with.

### Behaviour

- **PROGS-5.1** The text is generated from the definitions in declaration order:
  `\n/* file generated by qcc, do not modify */\n\ntypedef struct\n{\tint\tpad[28];\n`, then one
  line per global definition before `end_sys_globals`, then `} globalvars_t;\n\n`, then
  `typedef struct\n{\n`, one line per field definition before `end_sys_fields`, and
  `} entvars_t;\n\n`. A line is `\t<ctype>\t<name>;\n` with `<ctype>` `float` for float,
  `vec3_t` for vector, `string_t` for string, `func_t` for function and `int` for every other
  type; a vector's `_x`, `_y` and `_z` definitions get no line of their own.
- **PROGS-5.2** The header `crc` is PROGS-4's CRC of exactly that text, with `\n` as one byte
  0x0A. For `WinQuake/progdefs.q1` it is 5927.
- **PROGS-5.3** qcc stores the CRC zero-extended from 16 bits, so the field is between 0 and
  65535 in any file id's qcc writes.

## PROGS-6 The file CRC

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/sv_main.c`, `SV_SendServerinfo`.

### Behaviour

- **PROGS-6.1** While loading, the engine also computes PROGS-4's CRC over every byte of the
  file as stored, before any byte swapping, into `pr_crc`.
- **PROGS-6.2** `pr_crc` is never checked. Its only use is the server info message, which
  prints it as `VERSION <version> SERVER (<pr_crc> CRC)`.

## PROGS-7 Statements

**Source:** `WinQuake/pr_comp.h`, `dstatement_t`; `WinQuake/pr_edict.c`, `PR_LoadProgs`.

### Behaviour

- **PROGS-7.1** A statement is 8 bytes:

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 0 | 2 | `op` | u16 | Opcode (PROGS-8) |
  | 2 | 2 | `a` | i16 | First operand |
  | 4 | 2 | `b` | i16 | Second operand |
  | 6 | 2 | `c` | i16 | Third operand |

- **PROGS-7.2** The operands are signed 16-bit: the bytes `ff ff` are -1, not 65535. Most
  opcodes use an operand as a global slot index; `OP_GOTO` uses `a`, and `OP_IF` and `OP_IFNOT`
  use `b`, as a jump relative to the statement. Their meaning per opcode is in `vm.md`.
- **PROGS-7.3** Statement 0 is reserved (PROGS-1.5); a function's code starts at
  `first_statement` (PROGS-10) and the table has no other structure.
- **PROGS-7.4** The engine does not check `op` or the operands at load. An opcode above
  `OP_BITOR` (65) fails only when executed, with `Bad opcode <op>`.

## PROGS-8 Opcode numbers

**Source:** `WinQuake/pr_comp.h`, the anonymous `enum` of `OP_` names.

### Behaviour

- **PROGS-8.1** The vanilla opcodes are numbered from 0 in this order, 66 in all (0 to 65):

  | Op | Name | Op | Name | Op | Name | Op | Name |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | 0 | `DONE` | 17 | `NE_S` | 34 | `STORE_ENT` | 51 | `CALL0` |
  | 1 | `MUL_F` | 18 | `NE_E` | 35 | `STORE_FLD` | 52 | `CALL1` |
  | 2 | `MUL_V` | 19 | `NE_FNC` | 36 | `STORE_FNC` | 53 | `CALL2` |
  | 3 | `MUL_FV` | 20 | `LE` | 37 | `STOREP_F` | 54 | `CALL3` |
  | 4 | `MUL_VF` | 21 | `GE` | 38 | `STOREP_V` | 55 | `CALL4` |
  | 5 | `DIV_F` | 22 | `LT` | 39 | `STOREP_S` | 56 | `CALL5` |
  | 6 | `ADD_F` | 23 | `GT` | 40 | `STOREP_ENT` | 57 | `CALL6` |
  | 7 | `ADD_V` | 24 | `LOAD_F` | 41 | `STOREP_FLD` | 58 | `CALL7` |
  | 8 | `SUB_F` | 25 | `LOAD_V` | 42 | `STOREP_FNC` | 59 | `CALL8` |
  | 9 | `SUB_V` | 26 | `LOAD_S` | 43 | `RETURN` | 60 | `STATE` |
  | 10 | `EQ_F` | 27 | `LOAD_ENT` | 44 | `NOT_F` | 61 | `GOTO` |
  | 11 | `EQ_V` | 28 | `LOAD_FLD` | 45 | `NOT_V` | 62 | `AND` |
  | 12 | `EQ_S` | 29 | `LOAD_FNC` | 46 | `NOT_S` | 63 | `OR` |
  | 13 | `EQ_E` | 30 | `ADDRESS` | 47 | `NOT_ENT` | 64 | `BITAND` |
  | 14 | `EQ_FNC` | 31 | `STORE_F` | 48 | `NOT_FNC` | 65 | `BITOR` |
  | 15 | `NE_F` | 32 | `STORE_V` | 49 | `IF` | | |
  | 16 | `NE_V` | 33 | `STORE_S` | 50 | `IFNOT` | | |

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_OPCODE_COUNT` | 66 | `WinQuake/pr_comp.h`, `OP_DONE` to `OP_BITOR` |

## PROGS-9 Global and field definitions

**Source:** `WinQuake/pr_comp.h`, `ddef_t`, `DEF_SAVEGLOBAL`; `WinQuake/pr_edict.c`,
`PR_LoadProgs`, `ED_FindGlobal`, `ED_FindField`, `ED_GlobalAtOfs`, `ED_FieldAtOfs`,
`ED_WriteGlobals`; `qcc/qcc.c`, `WriteData`.

A definition names a global slot (global definitions) or an entity field slot (field
definitions). The engine uses them to find globals and fields by name, to print values and to
save and load games; execution never reads them.

### Behaviour

- **PROGS-9.1** A definition is 8 bytes:

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 0 | 2 | `type` | u16 | Bits 0 to 14: the QC type (PROGS-16); bit 15: `DEF_SAVEGLOBAL` |
  | 2 | 2 | `ofs` | u16 | Global slot index, or for a field definition the field slot index |
  | 4 | 4 | `s_name` | i32 | Byte offset of the name in the string table |

- **PROGS-9.2** `ofs` is unsigned: a definition can name any slot from 0 to 65535, unlike a
  statement operand (PROGS-7.2).
- **PROGS-9.3** In a global definition, `DEF_SAVEGLOBAL` marks a global that a saved game
  stores. qcc sets it on every global that has no initial value, is not a function or field,
  and is not a local or parameter. The engine strips the bit before using the type.
- **PROGS-9.4** Several definitions can name the same slot: qcc gives a vector `v` the
  definitions `v` (vector) and `v_x` (float) at the same slot, and `v_y` and `v_z` at the next
  two.
- **PROGS-9.5** For a field `.T name`, qcc writes a field definition with type `T` at the
  field's slot, and a global definition with type field (5) for a global slot whose value is
  that field slot index. A vector field also gets `name_x`, `name_y` and `name_z` field
  definitions and globals for its three slots.
- **PROGS-9.6** Lookups by name compare bytes exactly (`strcmp`) and take the first matching
  definition in table order; lookups by slot take the first definition whose `ofs` matches.
- **PROGS-9.7** qcc also writes global definitions for the parameters and locals of functions,
  without `DEF_SAVEGLOBAL`, and one named `IMMEDIATE` for every constant slot it allocates
  (string, float and vector literals), so names in the table are not unique.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_DEF_SAVEGLOBAL` | 0x8000 | `WinQuake/pr_comp.h`, `DEF_SAVEGLOBAL` (`1<<15`) |

## PROGS-10 Functions

**Source:** `WinQuake/pr_comp.h`, `dfunction_t`; `WinQuake/pr_edict.c`, `PR_LoadProgs`;
`WinQuake/pr_exec.c`, `PR_ExecuteProgram`, `PR_EnterFunction`; `qcc/pr_comp.c`,
`PR_ParseDefs`.

### Behaviour

- **PROGS-10.1** A function is 36 bytes:

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 0 | 4 | `first_statement` | i32 | Index of the first statement; negative for a builtin |
  | 4 | 4 | `parm_start` | i32 | First global slot of the parameters and locals |
  | 8 | 4 | `locals` | i32 | Number of slots from `parm_start` that the call saves and restores |
  | 12 | 4 | `profile` | i32 | Runtime statement counter |
  | 16 | 4 | `s_name` | i32 | Byte offset of the function's name in the string table |
  | 20 | 4 | `s_file` | i32 | Byte offset of the source file name in the string table |
  | 24 | 4 | `numparms` | i32 | Number of parameters |
  | 28 | 8 | `parm_size` | u8[8] | Slots taken by each parameter: 1, or 3 for a vector |

- **PROGS-10.2** A function with `first_statement` below zero is builtin number
  `-first_statement`; qcc writes `#25` as -25. The engine calls the builtin without reading
  `parm_start`, `locals`, `numparms` or `parm_size`.
- **PROGS-10.3** A global of type function holds an index into this table, and index 0 is the
  null function: calling it fails with `NULL function`.
- **PROGS-10.4** `parm_size` is eight single bytes and is not byte-swapped; entries at or past
  `numparms` are ignored (qcc writes them as 0).
- **PROGS-10.5** `numparms` below zero copies no parameters; qcc writes -1 for a function
  declared with `...`. qcc does not limit the count to 8, but the engine reads only
  `parm_size[0..numparms)`, so a reader rejects more than 8 (PROGS-15.6).
- **PROGS-10.6** The engine does not interpret `profile` at load; it adds one for every
  statement the function executes, starting from the stored value. qcc writes 0.

## PROGS-11 String table

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/common.c`, `COM_LoadFile`;
`qcc/qcc.c`, `CopyString`, `WriteData`.

### Behaviour

- **PROGS-11.1** The string table is `numstrings` bytes holding NUL-terminated strings. A
  string reference (`s_name`, `s_file`, or the value of a string global) is a byte offset from
  the start of the table, and the string runs to the next NUL byte.
- **PROGS-11.2** Offset 0 is the empty string: qcc writes a NUL as the first byte.
- **PROGS-11.3** A reference may point into the middle of another string; the string is the
  bytes from that offset to the next NUL. qcc's `CopyString` appends every string it is given
  without looking for an earlier copy, so the same text can appear several times.
- **PROGS-11.4** The bytes are uninterpreted: any value from 0x01 to 0xFF can be part of a
  string, including control characters and Quake's high-bit characters.
- **PROGS-11.5** The table ends with zero to three NUL padding bytes from qcc (PROGS-1.3).
- **PROGS-11.6** At run time a string value can also point outside the table, at strings the
  engine allocates (`ED_NewString`, `sv.name`); those values are still offsets from the start
  of the table and never appear in a file as written by qcc.

## PROGS-12 Globals

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/progs.h`, `G_FLOAT`, `G_INT`;
`qcc/qcc.c`, `WriteData`; `qcc/pr_comp.c`, `PR_BeginCompilation`.

### Behaviour

- **PROGS-12.1** The globals are `numglobals` 4-byte slots holding the initial value of every
  global, constant and temporary. The engine executes in this memory directly, so the values in
  the file are the state before the first function runs.
- **PROGS-12.2** A slot holds whichever type the code using it says (PROGS-17): the file does
  not tag slots with types, and only the definitions (PROGS-9) say what a named slot holds.
- **PROGS-12.3** Slots 0 to 27 are the reserved return and parameter area (PROGS-18). qcc
  allocates every other global from slot 28 upward, so a file from qcc has at least 28 slots.

## PROGS-13 Entity fields

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `qcc/pr_comp.c`, `PR_GetDef`.

### Behaviour

- **PROGS-13.1** `entityfields` is the number of 4-byte field slots in every entity. The engine
  sizes each edict as `entityfields * 4` bytes of fields plus its own fixed header.
- **PROGS-13.2** qcc counts one slot per non-vector field and three per vector field, in
  declaration order; a field's slot index is the count before it.
- **PROGS-13.3** The engine does not check `entityfields` against the field definitions;
  [`edicts.md`](edicts.md) covers the system fields that must come first.

## PROGS-14 Load results

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`.

### Behaviour

- **PROGS-14.1** Loading a file that passes PROGS-3 yields every field of every record exactly
  as stored, after the little-endian conversion of PROGS-1.1; the loader changes no value.
- **PROGS-14.2** Loading has no other effect on the file's contents: the tables keep their
  counts, order and duplicates, and the reserved first records stay as stored.

## PROGS-15 Malformed files

**Source:** `WinQuake/pr_edict.c`, `PR_LoadProgs`; `WinQuake/common.c`, `COM_LoadFile`.

id's loader trusts the file: a table that runs past the end of the file, a negative count or a
string reference outside the table makes the engine read outside its buffer, which is
undefined behaviour in C. This spec fixes the answer: a reader rejects such a file with a
typed error instead of reading. The statements below are deliberate divergences; they reject
only files that id's qcc never writes.

### Behaviour

- **PROGS-15.1** A file shorter than the 60-byte header is rejected.
- **PROGS-15.2** A table whose offset or count is negative, or whose end
  (`offset + count * record size`) is past the end of the file, is rejected. The end is
  computed without overflow.
- **PROGS-15.3** A string reference in a definition (`s_name`) or a function (`s_name`,
  `s_file`) that is negative or not less than `numstrings` is rejected.
- **PROGS-15.4** A string reference whose bytes reach the end of the string table without a
  NUL is rejected. (id's engine would read on into the next table, or into the NUL that
  `COM_LoadFile` appends after the last byte of the file.)
- **PROGS-15.5** A file is checked in this order and the first failure is reported: size,
  then version (PROGS-3.1), then header CRC (PROGS-3.2), then table bounds, then field
  definition flags (PROGS-3.4), then string references, then function parameter counts.
- **PROGS-15.6** A function whose `numparms` is greater than 8 is rejected.
- **PROGS-15.7** Statement opcodes and operands, function `first_statement`, `parm_start` and
  `locals`, definition `ofs`, and the values of globals are not checked at load, as in id's
  engine: what an out-of-range value does when executed is in `vm.md`.
- **PROGS-15.8** A reader may skip the header CRC check (PROGS-3.2) when asked to, so the
  editor can open a mod whose system definitions differ from id's; the default is to check.
- **PROGS-15.9** Within one step of PROGS-15.5 the first failure in this order is reported:
  table bounds in header order (statements, global definitions, field definitions, functions,
  strings, globals); field definitions in table order; string references of the global
  definitions, then of the field definitions, then of the functions, each in table order, with
  a function's `s_name` before its `s_file`; parameter counts in function order. id's engine
  has no such checks, so the order is ours; it follows the header and the loader's loops.

## PROGS-16 The eight QC types

**Source:** `WinQuake/pr_comp.h`, `etype_t`; `WinQuake/pr_edict.c`, `type_size`;
`qcc/pr_lex.c`, `type_size`.

### Behaviour

- **PROGS-16.1** The type numbers used in definitions (PROGS-9.1) are:

  | Number | Name | Slots | What a slot holds |
  | --- | --- | --- | --- |
  | 0 | `ev_void` | 1 | Nothing |
  | 1 | `ev_string` | 1 | A string reference (PROGS-17.2) |
  | 2 | `ev_float` | 1 | An f32 |
  | 3 | `ev_vector` | 3 | Three f32: x, y, z |
  | 4 | `ev_entity` | 1 | An entity reference (PROGS-17.4) |
  | 5 | `ev_field` | 1 | A field slot index (PROGS-17.5) |
  | 6 | `ev_function` | 1 | A function index (PROGS-17.6) |
  | 7 | `ev_pointer` | 1 | A pointer to an entity field (PROGS-17.7) |

- **PROGS-16.2** The slot counts are id's `type_size` table. The engine computes the pointer's
  size as `sizeof(void *)/4`, which is 1 on the 32-bit x86 build id shipped and 2 on a 64-bit
  build; this spec fixes 1, the value of the shipped engine and of qcc's own table
  (`{1,1,1,3,1,1,1,1}`).
- **PROGS-16.3** A definition type above 7 (after removing `DEF_SAVEGLOBAL`) is not a vanilla
  type. id's engine has no check for it and prints such a value as `bad type <n>`; a reader
  keeps the number rather than rejecting the file.
- **PROGS-16.4** QuakeC has no way to declare a pointer: `ev_pointer` values exist only at run
  time, made by `OP_ADDRESS`, and qcc writes no definition of type 7.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_TYPE_COUNT` | 8 | `WinQuake/pr_comp.h`, `etype_t` |
| `PROGS_VECTOR_SLOTS` | 3 | `WinQuake/pr_edict.c`, `type_size[ev_vector]` |

## PROGS-17 Values in slots

**Source:** `WinQuake/progs.h`, `eval_t`, `EDICT_TO_PROG`, `PROG_TO_EDICT`, `G_STRING`;
`WinQuake/pr_edict.c`, `ED_ParseEpair`; `WinQuake/pr_exec.c`, `PR_ExecuteProgram`.

### Behaviour

- **PROGS-17.1** A slot is 4 bytes read either as an f32 or as an i32; the engine
  reinterprets the same bits (`eval_t` is a union) and never converts between them when
  copying.
- **PROGS-17.2** A string value is an i32 byte offset from the start of the string table
  (PROGS-11); 0 is the empty string.
- **PROGS-17.3** A float value is the f32 in the slot; a vector is three consecutive float
  slots, x first.
- **PROGS-17.4** An entity value is an i32 byte offset from the start of the engine's edict
  array, which is the entity number times the engine's edict size, not the entity number; 0 is
  the world. The file never holds a non-zero entity value, since QuakeC cannot initialise one.
- **PROGS-17.5** A field value is an i32 slot index into an entity's fields (PROGS-13), so
  `.origin_y` is one more than `.origin`.
- **PROGS-17.6** A function value is an i32 index into the functions table (PROGS-10); 0 is
  the null function.
- **PROGS-17.7** A pointer value is an i32 byte offset from the start of the engine's edict
  array to one field slot of one entity.

## PROGS-18 Reserved global slots

**Source:** `WinQuake/pr_comp.h`, `OFS_NULL` to `RESERVED_OFS`; `qcc/qcc.c`, `InitData`;
`qcc/qcc.c`, `PR_WriteProgdefs`.

### Behaviour

- **PROGS-18.1** Slot 0 is `OFS_NULL`; slots 1 to 3 hold a function's return value
  (`OFS_RETURN`, three slots so a vector fits); slots 4 to 27 hold the eight call parameters,
  three slots each, parameter `n` at `4 + 3n`.
- **PROGS-18.2** Slots 0 to 27 have no global definitions in a file from qcc, and the system
  globals start at slot 28, which is why `progdefs.h` opens with `int pad[28]` (PROGS-5.1).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_OFS_NULL` | 0 | `WinQuake/pr_comp.h`, `OFS_NULL` |
| `PROGS_OFS_RETURN` | 1 | `WinQuake/pr_comp.h`, `OFS_RETURN` |
| `PROGS_OFS_PARM0` | 4 | `WinQuake/pr_comp.h`, `OFS_PARM0` (`OFS_PARM1` to `OFS_PARM7` are 7, 10, 13, 16, 19, 22, 25) |
| `PROGS_PARM_SLOTS` | 3 | `WinQuake/pr_comp.h`, comment on `OFS_PARM0`: "leave 3 ofs for each parm to hold vectors" |
| `PROGS_RESERVED_OFS` | 28 | `WinQuake/pr_comp.h`, `RESERVED_OFS` |

## PROGS-19 FTE version 7 header

**Source:** `engine/qclib/pr_comp.h`, `dprograms_t`, `PROG_EXTENDEDVERSION`,
`PROG_SECONDARYVERSION16`, `PROG_SECONDARYVERSION32`; `engine/qclib/pr_edict.c`,
`PR_ReallyLoadProgs` (fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`).

FTEQCC can write version 7, an extended header used by FTE and other engines. id's engine
rejects it (PROGS-3.1). The editor reads it only behind a feature flag (FND-1).

### Behaviour

- **PROGS-19.1** A version 7 file has a 92-byte header: the 60 bytes of PROGS-2, then eight
  more 4-byte fields:

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 60 | 4 | `ofsfiles` | u32 | Offset of embedded source files, 0 if none |
  | 64 | 4 | `ofslinenums` | u32 | Offset of the line number table, 0 if none |
  | 68 | 4 | `ofsbodylessfuncs` | u32 | Offset of the names of functions without bodies |
  | 72 | 4 | `numbodylessfuncs` | u32 | Number of those names |
  | 76 | 4 | `ofs_types` | u32 | Offset of the type information table, 0 if none |
  | 80 | 4 | `numtypes` | u32 | Number of type records |
  | 84 | 4 | `blockscompressed` | u32 | Bit mask of compressed tables (PROGS-21.3) |
  | 88 | 4 | `secondaryversion` | i32 | Which version 7 layout (PROGS-19.2) |

- **PROGS-19.2** `secondaryversion` 0x021B1461 (`"1FTE" ^ "PROG"` as little-endian i32) means
  the 16-bit layout of PROGS-7 and PROGS-9; 0x65167402 (`"1FTE" ^ "32B "`) means the 32-bit
  layout of PROGS-20.
- **PROGS-19.3** FTE also accepts version 7 with `secondaryversion` `"UH27"` or `"KKQW"`, or
  any other value (treated as KKQWSV's format). Those are other engines' layouts; a reader
  rejects them as unsupported.
- **PROGS-19.4** In FTE's header the offsets and counts are unsigned, but they are bounded by
  the file size, so PROGS-15.2 applies unchanged.
- **PROGS-19.5** In the 16-bit layout FTE declares statement operands unsigned
  (`dstatement16_t`), so in a version 7 file the bytes `ff ff` are operand 65535. This differs
  from PROGS-7.2, which holds for version 6.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_VERSION_FTE` | 7 | `engine/qclib/pr_comp.h`, `PROG_EXTENDEDVERSION` |
| `PROGS_FTE_HEADER_SIZE` | 92 | `engine/qclib/pr_comp.h`, `sizeof(dprograms_t)` |
| `PROGS_FTE_SECONDARY16` | 0x021B1461 | `engine/qclib/pr_comp.h`, `PROG_SECONDARYVERSION16` |
| `PROGS_FTE_SECONDARY32` | 0x65167402 | `engine/qclib/pr_comp.h`, `PROG_SECONDARYVERSION32` |

## PROGS-20 FTE 32-bit statements and definitions

**Source:** `engine/qclib/pr_comp.h`, `dstatement32_t`, `ddef32_t`; `engine/qclib/qccmain.c`,
`WriteData` (fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`).

FTEQCC switches to the 32-bit layout when a program has more than 65530 globals or a jump too
far for 16 bits.

### Behaviour

- **PROGS-20.1** In the 32-bit layout a statement is 16 bytes: `op`, `a`, `b` and `c`, each a
  u32, at offsets 0, 4, 8 and 12.
- **PROGS-20.2** In the 32-bit layout a definition is 12 bytes: `type` u32 at 0, `ofs` u32 at
  4 and `s_name` i32 at 8. `DEF_SAVEGLOBAL` is still bit 15 of `type`.
- **PROGS-20.3** Functions, strings and globals keep the layout of PROGS-10 to PROGS-12.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PROGS_FTE32_STATEMENT_SIZE` | 16 | `engine/qclib/pr_comp.h`, `sizeof(dstatement32_t)` |
| `PROGS_FTE32_DEF_SIZE` | 12 | `engine/qclib/pr_comp.h`, `sizeof(ddef32_t)` |

## PROGS-21 FTE optional tables and compression

**Source:** `engine/qclib/pr_edict.c`, `PR_ReallyLoadProgs`; `engine/qclib/qccmain.c`,
`WriteData`, `WriteBodylessFuncs`; `engine/qclib/qcd_main.c`, `QC_decode`
(fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`).

### Behaviour

- **PROGS-21.1** When `ofslinenums` is not 0, it is the offset of `numstatements` i32 source
  line numbers, one per statement in statement order.
- **PROGS-21.2** The bodyless function table is `numbodylessfuncs` NUL-terminated names stored
  one after another from `ofsbodylessfuncs`.
- **PROGS-21.3** Bit `n` of `blockscompressed` set means a table is stored compressed: bit 0
  statements, 1 global definitions, 2 field definitions, 3 functions, 4 strings, 5 globals, 6
  line numbers, 7 types. A compressed table starts at its offset with an i32 compressed length
  followed by that many bytes of zlib data (`QC_decode` method 2).
- **PROGS-21.4** A reader rejects a file with any `blockscompressed` bit set, as FTE does when
  built without zlib; reading compressed tables would add a decompressor for a mode that
  FTEQCC's default settings do not produce (`compressoutput` is zero-initialised in
  `qccmain.c`).
- **PROGS-21.5** `ofsfiles` and the type table carry no information the engine needs to run
  the program; a reader keeps their offsets and counts and does not decode them.
- **PROGS-21.6** The version 7 header is followed by the same six tables as version 6, and
  PROGS-15 applies to them; opcodes above 65 and type numbers above 7 are FTE extensions that
  the vanilla VM does not execute.
- **PROGS-21.7** When `ofsfiles` is not 0, it is the offset of an i32 count followed by that
  many 144-byte embedded file records (`includeddatafile_t`, read by `PR_TestRecompile`):

  | Offset | Size | Field | Type | Meaning |
  | --- | --- | --- | --- | --- |
  | 0 | 128 | `filename` | char[128] | Source file name, NUL-terminated |
  | 128 | 4 | `size` | i32 | Uncompressed size |
  | 132 | 4 | `compsize` | i32 | Stored size |
  | 136 | 4 | `compmethod` | i32 | 0 stored, 1 XOR, 2 or 8 zlib (`QC_decode`) |
  | 140 | 4 | `ofs` | i32 | File offset of the stored data |

  FTEQCC at the pinned commit embeds sources as a zip archive instead and writes `ofsfiles`
  as 0 (`WriteSourceFiles`).
- **PROGS-21.8** When `ofs_types` is not 0, it is the offset of `numtypes` 28-byte type
  records (`typeinfo_t`): seven i32 fields `type`, `next`, `aux_type`, `num_parms`, `ofs`,
  `size` and `name`, the last a string reference. FTEQCC at the pinned commit never writes
  the table (`types` is always false in `WriteData`).
- **PROGS-21.9** A version 7 file is checked in this order, and the first failure is reported:
  a size of at least 60 bytes (PROGS-15.1), the version, a size of at least 92 bytes, the
  header CRC (PROGS-3.2, which PROGS-15.8 may skip), `secondaryversion` (PROGS-19.3),
  `blockscompressed` (PROGS-21.4), the bounds of the six tables (PROGS-15.9), the bounds of the
  line number table when `ofslinenums` is not 0 (`numstatements` 4-byte entries, as
  PROGS-15.2), the bodyless function names (every name's NUL inside the file, and a table of no
  names still at an offset no greater than the file size), then the remaining steps of
  PROGS-15.5. The embedded files and the type table are not checked (PROGS-21.5). FTE's loader
  checks none of the bounds, so the order is ours.

## PROGS-22 Writing a progs.dat

**Source:** `qcc/qcc.c`, `WriteData`, for the table order of PROGS-22.2. The rest is the
editor's own behaviour: id's engine never writes a progs.dat.

The writer turns the records of a progs.dat back into a file, for the editor's own tools and so
that a file read and written back can be compared byte for byte.

### Behaviour

- **PROGS-22.1** When no non-empty table overlaps another or the header, a file the reader
  accepts and the writer writes back unchanged is the same byte for byte: the writer places the
  non-empty tables in the order the file had them, with the bytes before, between and after them
  that the file had (PROGS-1.4), and gives each empty table the offset the file stored.
- **PROGS-22.2** Without a source layout (a file built in memory, or one with overlapping
  tables) the writer uses qcc's order (PROGS-1.3): header, strings, statements, functions,
  global definitions, field definitions, globals, then for version 7 the line numbers and the
  bodyless function names. It pads the string table with NUL bytes to a multiple of 4 and
  `numstrings` counts the padding. An empty table's offset is where it would start, except that
  an empty bodyless function table has offset 0.
- **PROGS-22.3** The writer computes every count and offset in the header from the tables,
  `version` and `secondaryversion` from the format, and writes `blockscompressed` as 0; `crc`,
  `entityfields`, `ofsfiles`, `ofs_types` and `numtypes` are written as given. With a source
  layout, a table the source did not have, or had empty, and that now has records goes after
  the source's last byte. A version 7 file with no line number table has `ofslinenums` 0.
- **PROGS-22.4** The writer rejects a value that does not fit its field, with a typed error and
  no output: in version 6 an opcode above 65535, an operand outside -32768 to 32767, or a
  definition type or `ofs` above 65535; in the 16-bit version 7 layout also an operand outside
  0 to 65535; a count or offset above 2147483647; a line number table whose length is not the
  number of statements; a bodyless function name containing a NUL; version 7 parts in a version
  6 file; and `ofsfiles` or `ofs_types` other than 0 without a source layout, since the bytes
  they point at would not be kept.
- **PROGS-22.5** When the records given to the writer pass the checks of PROGS-15, reading the
  written file gives back every record exactly as given.

## Test vectors

Whole files are in [`vectors/progs/`](vectors/progs/README.md), with the decoded value of
every field. Each was checked on 2026-10-16 by running id's `PR_LoadProgs`, copied unchanged
from the pinned commit into a local harness with the file system calls stubbed, over the
vector and printing every decoded field; the output matched the expected values in the
vectors' README. The FTE vectors were checked against the FTEQW structures above by reading
them, not by running FTE.

The `qc-progs` reader was checked against id's loader on 2026-10-16, before the oracle suite
exists. A local harness included `WinQuake/pr_comp.h` unchanged and `PR_LoadProgs`,
`ED_GlobalAtOfs`, `ED_FieldAtOfs`, `ED_FindField`, `ED_FindGlobal` and `ED_FindFunction` of
`WinQuake/pr_edict.c` and the CRC functions and table of `WinQuake/crc.c`, each copied verbatim
from the pinned commit, with `Sys_Error`, `Con_DPrintf` and the file loading stubbed (the file
followed by one NUL, as `COM_LoadFile` returns it), and was built with Apple clang 21 for 64-bit
macOS. For each file it printed the error, or every header field, `pr_crc`, every field of every
record with each name's bytes, the result of each lookup for every definition's name and slot
and every function's name, and every global. A Rust program printed the same from `qc-progs`
(default features). Over the four vectors and 3,000 generated files (random tables in a random
order with random gaps, valid string references, and in a fifth of them a wrong version, a
wrong CRC, a field definition with `DEF_SAVEGLOBAL` or a function with more than 8
parameters), 2,771 outputs were identical and the other 233 were the expected divergence of
PROGS-15.6: id's loader accepts more than 8 parameters and `qc-progs` rejects them. Making the
reader zero-extend the operand `a` instead of sign-extending it produced 1,568 divergences.

Statement decoding (PROGS-7):

| Bytes | `op` | `a` | `b` | `c` | Statements |
| --- | --- | --- | --- | --- | --- |
| `1f 00 29 00 2a 00 00 00` | 31 (`STORE_F`) | 41 | 42 | 0 | PROGS-7.1 |
| `3d 00 ff ff 00 00 00 00` | 61 (`GOTO`) | -1 | 0 | 0 | PROGS-7.2 |

Definition decoding (PROGS-9):

| Bytes | `type` | Save | `ofs` | `s_name` | Statements |
| --- | --- | --- | --- | --- | --- |
| `02 80 21 00 42 00 00 00` | 2 (float) | yes | 33 | 66 | PROGS-9.1, PROGS-9.3 |
| `03 00 22 00 4a 00 00 00` | 3 (vector) | no | 34 | 74 | PROGS-9.1 |
| `00 00 ff ff 00 00 00 00` | 0 (void) | no | 65535 | 0 | PROGS-9.2 |

Load errors, each a one-field change to [`minimal`](vectors/progs/README.md#minimal), run
through id's `PR_LoadProgs`:

| Change | Result | Statements |
| --- | --- | --- |
| `version` = 5 | `progs.dat has wrong version number (5 should be 6)` | PROGS-3.1 |
| `crc` = 5928 | `progs.dat system vars have been modified, progdefs.h is out of date` | PROGS-3.2 |
| `version` = 7 and `crc` = 0 | `progs.dat has wrong version number (7 should be 6)` | PROGS-3.3 |
| In [`small`](vectors/progs/README.md#small), field definition 1 `type` = 0x8002 | `PR_LoadProgs: pr_fielddefs[i].type & DEF_SAVEGLOBAL` | PROGS-3.4 |

Slot values (PROGS-17), as id's `eval_t` reads them:

| Slot bits | `_int` | `_float` | Statements |
| --- | --- | --- | --- |
| `3f800000` | 1065353216 | 1.0 | PROGS-17.1, PROGS-17.3 |
| `80000000` | -2147483648 | -0.0 | PROGS-17.1 |
| `7fc00000` | 2143289344 | NaN, bits kept | PROGS-17.1 |
| `ffffffff` | -1 | NaN, bits kept | PROGS-17.1 |

Entity values (PROGS-17.4) through `EDICT_TO_PROG`: entity 3 with a 400-byte edict is 1200;
entity 599 with a 1096-byte edict is 656504; entity 0 is 0 for every edict size.

Types and reserved slots (PROGS-16, PROGS-18), and the conversions above, were checked against
`qc-types` on 2026-10-16. A local harness included `WinQuake/pr_comp.h` unchanged and the
`type_size` table of `WinQuake/pr_edict.c`, the `eval_t` union and `EDICT_TO_PROG` and
`PROG_TO_EDICT` of `WinQuake/progs.h`, and the `type_size` table of `qcc/pr_lex.c`, each copied
verbatim from the pinned commits, and was built with Apple clang 21 for 64-bit macOS. It printed
every type number and its slot counts in both tables, `OFS_NULL`, `OFS_RETURN`, `OFS_PARM0` to
`OFS_PARM7`, `RESERVED_OFS` and `MAX_PARMS`, the `_int`, `_float`, `string`, `function` and
`edict` views and a three-component vector of ten slot bit patterns (0, 1, `3f800000`,
`80000000`, `7fc00000`, `ffffffff`, `7f800000`, `deadbeef`, `7fffffff`, and 1 again), and the
`EDICT_TO_PROG` offset and the entity number recovered from it for entities 0, 1, 3 and 599 at
edict sizes 4, 400 and 1096. A Rust program printed the same 49 lines from `qc-types`, and the two
outputs were identical. The one expected difference is PROGS-16.2: the 64-bit build's
`type_size[ev_pointer]` is 2, so the harness reported 1 for it, the value of the shipped 32-bit
engine and of qcc's table. Changing the vector's slot count to 2 in `qc-types` made the Rust
side fail on the first vector, and cargo-mutants caught all 44 viable mutants of the crate.
