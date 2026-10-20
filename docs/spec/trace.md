# VM trace format

Prefix: `TRACE` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the trace that the differential runner (P1-13) compares: what happened in each
QuakeC statement a VM executed, as a compact binary stream and as a JSON debug form. Two
programs write it independently from this page: `qc-vm`'s per-statement trace hook (P1-10),
through the Rust encoder in `qc-trace`, and the id VM oracle (P1-11), a C runner around
`WinQuake/pr_exec.c` with an encoder of its own. Neither links the other's code, so this page
is the whole contract between them. Implemented by `qc-trace`.

This is a format of ours, not one of id's: there is no id source to port, and the statements
below are decisions, each made so that two VMs with different memory layouts produce the same
trace for the same execution. What a VM does is in [`vm.md`](vm.md); the words "run",
"statement", "current function", "builtin" and "runtime error" mean what they mean there.
Value encodings are in [`progs.md`](progs.md) (PROGS-17). How the runner sets up a run and
compares two traces is in [`differential.md`](differential.md) and the runner itself.

## TRACE-1 The trace

**Source:** this project. The events traced are those of `WinQuake/pr_exec.c`,
`PR_ExecuteProgram` and `PR_RunError`, at id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`.

### Behaviour

- **TRACE-1.1** A trace is one header (TRACE-2) followed by zero or more records (TRACE-4), in
  the order the events they describe happened. It has no end marker: it ends after its last
  complete record.
- **TRACE-1.2** The binary form (TRACE-5) and the JSON form (TRACE-6) hold the same
  information. Decoding either form and encoding the result in the other, then decoding that,
  gives back the same header and records.
- **TRACE-1.3** The header is written after the progs is loaded (its globals hold the values
  the file gives them) and before the first run. Every run the host starts after that is
  traced, including runs nested inside builtins (VM-18).
- **TRACE-1.4** Globals are named by slot number, edicts by edict number (0 is the world) and
  entity fields by field slot number (PROGS-13), never by an address or an entity value, so
  the names do not depend on how a VM lays out its memory. Values are the raw 32-bit patterns
  of the slots (PROGS-17.1), written exactly, NaN payloads included.

## TRACE-2 Header

**Source:** this project; the values it records are those of `WinQuake/progs.h`
(`pr_edict_size`, `EDICT_TO_PROG`) and `WinQuake/pr_edict.c` (`PR_LoadProgs`, `pr_crc`).

The header carries what a reader needs to interpret entity, pointer and string values written
by this emitter (TRACE-7), since those are engine-specific (PROGS-17.4, PROGS-17.7).

### Behaviour

- **TRACE-2.1** The header holds the format version and five numbers: `progs_crc`, the CRC of the
  whole progs.dat file (PROGS-6.1, id's `pr_crc`), so a reader can check that two traces ran
  the same file; `edict_size`, the bytes between the entity
  values of consecutive edicts in this VM (id's `pr_edict_size`); `field_offset`, the bytes
  from an entity value to the pointer value of its field slot 0; `entityfields`, the number of
  field slots per edict (the progs header's `entityfields`); and `numstrings`, the size of
  the loaded string table in bytes.
- **TRACE-2.2** The version is `TRACE_VERSION`. A reader rejects any other version: a change
  to the format that an old reader would misread is a new version.
- **TRACE-2.3** `edict_size` is at least 1, `progs_crc` fits in 16 bits, and the fields fit
  inside an edict: `field_offset + 4 * entityfields` is at most `edict_size`, computed without
  overflow. A reader rejects a header that breaks any of these, and an emitter cannot write
  one.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `TRACE_VERSION` | 1 | This spec |

## TRACE-3 Change sets

**Source:** this project. The state compared is that of [`vm.md`](vm.md) VM-1.1: the globals
and the edicts' fields.

The run, step, exit and error records (TRACE-4) are the change-carrying records. Each carries
a change set: what the VM's globals and entity fields became since the previous one, which is
what the vectors in [`vectors/vm/`](vectors/vm/README.md) list after each statement.

### Behaviour

- **TRACE-3.1** A change set lists every global slot and every entity field slot whose 32-bit
  pattern, at the moment the record is written, differs from its pattern when the previous
  change-carrying record was written, or, for the first one, when the header was written. The
  patterns are compared as bits, so `-0.0` differs from `0.0` and a NaN equals itself only
  when its bits do.
- **TRACE-3.2** Each slot appears at most once, with its value at the moment the record is
  written. A slot that changed and then changed back to its earlier pattern is not listed.
- **TRACE-3.3** Global changes are in ascending order of slot. Field changes are in ascending
  order of edict number, and of field slot within one edict.
- **TRACE-3.4** The globals compared are slots 0 to `numglobals - 1`. The fields compared are
  slots 0 to `entityfields - 1` of every edict the VM has in use (in id's engine edicts 0 to
  `sv.num_edicts - 1`), free or not. An edict that was not in use at the previous
  change-carrying record is compared against all-zero fields, so a newly allocated edict
  lists only its non-zero fields.
- **TRACE-3.5** Only the globals and the fields are compared. The call stack, the local stack,
  the runaway counter, `pr_argc` and whether an edict is free are not in a change set; the
  differences they make show up in later globals and fields, or in the records' order.

## TRACE-4 Records

**Source:** this project; the events are those of `WinQuake/pr_exec.c`, `PR_ExecuteProgram`
(a run, each statement, each builtin call, the return to the caller) and `PR_RunError` (a
runtime error), and the engine strings of PROGS-11.6.

### Behaviour

- **TRACE-4.1** A run record is written when the host or a builtin calls `PR_ExecuteProgram`,
  before any of the checks of VM-2. It holds the function index as passed, a signed 32-bit
  number, and a change set: for a run started by the host, the host's own changes since the
  previous record (setting `self` and `time`, spawning edicts); for a nested run, what the
  builtin changed before starting it.
- **TRACE-4.2** A step record is written when a statement finishes without an error. It holds
  the index of the function that was current when the statement became current (VM-3.1),
  before any call or return the statement makes; the statement's index; its opcode as the
  statement stores it (PROGS-7.1); the builtin number when there is one (TRACE-4.3); and a
  change set.
- **TRACE-4.3** A step record has a builtin number exactly when its statement is a `CALL0` to
  `CALL8` that called a builtin (VM-14.4): the number `-first_statement` of the function it
  called. The step's change set includes what the builtin wrote, `OFS_RETURN` among it.
- **TRACE-4.4** When a builtin starts nested runs, each run's records (its run record, its
  statements' records and its exit or error record) are written while the builtin is running,
  so they come before the step record of the `CALL` that called the builtin. That step
  record's change set holds what the builtin changed after the last nested run returned.
- **TRACE-4.5** An exit record is written when `PR_ExecuteProgram` returns to its caller
  (VM-16.4). It holds only a change set, which is normally empty: the `DONE` or `RETURN` that
  ended the run has its own step record.
- **TRACE-4.6** An error record is written instead of a step record when a runtime error ends
  the run (VM-19.1, VM-19.4), and also for the errors a run raises before its first statement
  (VM-2.1, VM-2.2, VM-2.4). It holds a function index, a statement index, the message and a
  change set (VM-19.5 says what a failed statement may already have written).
- **TRACE-4.7** For an error raised by a statement, an error record's function and statement
  are the ones `PR_RunError` prints (VM-19.1): the current function and statement, so for
  `runaway loop error` the statement before the one that was not run. For an error raised
  before a run's first statement (VM-2.1, VM-2.2, VM-2.4), the function is the index the run
  was given and the statement is -1.
- **TRACE-4.8** An error record's message is the error's text as [`vm.md`](vm.md) gives it,
  without a trailing newline (`Bad opcode 70`, `NULL function`); for the errors of VM-19.4,
  which have no message in id's code, it is the implementation's own text. It contains no NUL
  byte.
- **TRACE-4.9** After an error record, no record follows for any run that was active when the
  error happened (VM-19.2). The host may then start a new run, which begins with its own run
  record.
- **TRACE-4.10** A string record is written whenever the VM makes a string value that lies
  outside the string table (PROGS-11.6) name some text: when a builtin or the host allocates
  an engine string, and whenever the text at such a value changes (id's `ftos`, `vtos` and
  `etos` all write `pr_string_temp` in `WinQuake/pr_cmds.c`). It holds the 32-bit string value and the text up to, not including, its
  NUL. It is written before the change-carrying record whose change set first shows the
  value, and it carries no change set: it does not reset what the next change set compares
  against (TRACE-3.1).

## TRACE-5 Binary form

**Source:** this project.

### Behaviour

- **TRACE-5.1** Numbers are written in three ways. A *word* is 4 bytes, little-endian. A
  *uvar* is an unsigned 32-bit number in LEB128: 7 bits per byte, least significant group
  first, the high bit set on every byte but the last. A *svar* is a signed 32-bit number
  `n` written as the uvar of its zigzag form, `(n << 1) ^ (n >> 31)` with an arithmetic
  shift, so 0, -1, 1, -2 become 0, 1, 2, 3.
- **TRACE-5.2** A uvar is at most 5 bytes and uses the fewest bytes that hold its value: a
  reader rejects a uvar that runs past 5 bytes, one whose fifth byte is above 0x0F (the value
  would not fit in 32 bits), and one with more than one byte whose last byte is 0x00.
- **TRACE-5.3** *Text* is a uvar byte count followed by that many bytes, none of them NUL.
- **TRACE-5.4** The header is 28 bytes: the magic `QCTR` (0x51 0x43 0x54 0x52), then six
  words: the version, `progs_crc`, `edict_size`, `field_offset`, `entityfields` and
  `numstrings`.
- **TRACE-5.5** A change set is a uvar count of global changes, each a uvar slot and a word
  holding the slot's bits, then a uvar count of field changes, each a uvar edict number, a uvar
  field slot and a word holding the bits.
- **TRACE-5.6** Each record is a tag byte followed by its fields, in this order:

  | Tag | Record | Fields |
  | --- | --- | --- |
  | 0x01 | Run | svar function, change set |
  | 0x02 | Step | svar function, svar statement, uvar opcode, builtin flag byte (0x00 none, 0x01 present), uvar builtin number if present, change set |
  | 0x03 | Exit | change set |
  | 0x04 | Error | svar function, svar statement, text message, change set |
  | 0x05 | String | word value, text |

- **TRACE-5.7** A reader rejects a stream that is shorter than the header or has the wrong
  magic, a record that ends before its last field, an unknown tag, a builtin flag byte other
  than 0x00 or 0x01, an opcode above 65535, and a change set that breaks the order of
  TRACE-3.3 or lists a slot twice. Because every value has exactly one encoding, encoding what
  a reader accepted gives back the same bytes.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `TRACE_MAGIC` | `QCTR` (0x51 0x43 0x54 0x52) | This spec |
| `TRACE_HEADER_SIZE` | 28 | This spec: the magic and six words |
| `TRACE_UVAR_MAX_BYTES` | 5 | This spec: 32 bits in groups of 7 |
| `TRACE_TAG_RUN` | 0x01 | This spec |
| `TRACE_TAG_STEP` | 0x02 | This spec |
| `TRACE_TAG_EXIT` | 0x03 | This spec |
| `TRACE_TAG_ERROR` | 0x04 | This spec |
| `TRACE_TAG_STRING` | 0x05 | This spec |

## TRACE-6 JSON debug form

**Source:** this project; JSON is [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259).

The JSON form is for reading a trace and for diffing two traces line by line; it is larger
and slower than the binary form and is not what the runner compares by default.

### Behaviour

- **TRACE-6.1** The JSON form is JSON Lines: one JSON object per line, each line ended by a
  line feed (0x0A), the header on the first line and one record on each line after it. There
  are no blank lines.
- **TRACE-6.2** The header is
  `{"trace":"qc-trace","version":1,"progs_crc":N,"edict_size":N,"field_offset":N,"entityfields":N,"numstrings":N}`.
- **TRACE-6.3** A record's `t` key names it: `run`, `step`, `exit`, `error` or `string`. A run
  has `function`; a step has `function`, `statement`, `opcode` and, only when it has one,
  `builtin`; an error has `function`, `statement` and `message`; a string has `value` and
  `text`. Every change-carrying record also has `globals`, an array of `[slot, bits]`, and
  `fields`, an array of `[edict, field, bits]`, each empty when there are no changes.
  Numbers are JSON integers.
- **TRACE-6.4** A 32-bit pattern (`bits`, and a string record's `value`) is a JSON string of
  exactly 8 lowercase hexadecimal digits: `"3f800000"` is 1.0.
- **TRACE-6.5** Text (`message`, `text`) is a JSON string in which each byte from 0x20 to 0x7E
  other than the backslash stands for itself, and every other byte, the backslash included,
  is the four characters `\x` and two lowercase hexadecimal digits. In the JSON source the
  backslash is itself escaped, so the text `a\b` followed by a line feed is
  `"a\\x5cb\\x0a"`.
- **TRACE-6.6** A writer puts the keys in the order TRACE-6.2 and TRACE-6.3 list them (`t`
  first), with no whitespace, and escapes text with JSON's own escapes only where RFC 8259
  requires them (`\"` and `\\`). A reader accepts any key order and any JSON whitespace within
  a line, and rejects a missing key, an unknown key, a value of the wrong JSON type, a number
  out of range for its field, bits or text not written as TRACE-6.4 and TRACE-6.5 say, and the
  violations of TRACE-2.3 and TRACE-5.7 that the JSON form can express.

## TRACE-7 Interpreting values

**Source:** this project; the encodings are `WinQuake/progs.h`, `EDICT_TO_PROG` and
`PROG_TO_EDICT`, and `WinQuake/pr_exec.c`, `OP_ADDRESS`, as PROGS-17.4 and PROGS-17.7
describe them.

Most values mean the same in every VM: a float's bits, a field index, a function index and a
string-table offset. Entity values, pointers and engine strings do not, so a reader comparing
two traces interprets them through each trace's header and string records. Which slots hold
such values is not in the trace (PROGS-12.2); `differential.md` and the runner decide where
to apply these rules.

### Behaviour

- **TRACE-7.1** A 32-bit pattern read as a signed number `v` is the entity value of edict `n`
  when `v` is `n * edict_size` exactly, with `n` 0 or more. 0 is the world in every trace.
- **TRACE-7.2** A pattern `v` is the pointer to field slot `f` of edict `n` when `v` is
  `n * edict_size + field_offset + 4 * f` exactly, with `n` 0 or more and `f` from 0 to
  `entityfields - 1`.
- **TRACE-7.3** A string value `v` from 0 to `numstrings - 1` is an offset into the string
  table, the same in every trace of one progs. Any other string value names the text of the
  latest string record for `v` before the point being read, and no text if there is none.

## Test vectors

One trace with every kind of record, in both forms, is in
[`vectors/trace/`](vectors/trace/README.md). `qc-trace` decodes both files, checks that they
hold the same trace and that encoding it gives back both files byte for byte.

Checked on 2026-10-16 against an encoder written in C from this page alone, without reading
`qc-trace`: built with Apple clang 21, it wrote the binary vector byte for byte from the
trace's values; see the vectors' README for the record.
