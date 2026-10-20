# Trace format

**Crates:** `qc-trace` (L1). **Specs:** TRACE-1 to TRACE-7.

## Context and design philosophy

The differential harness proves `qc-vm` runs QuakeC the way id's VM does by running both on the
same scenario and comparing what each did, statement by statement (docs/spec/differential.md
DIFF-9). That needs a record of execution both sides can write identically: the trace. id never
had one, so the format is "a design of this project, not a port" (crates/qc-trace/src/lib.rs:9,
README.md:12-13), specified in docs/spec/trace.md and implemented by `qc-trace`, an L1 crate whose
only dependency is `serde_json` (crates/qc-trace/Cargo.toml:11-15).

Design rules the spec states:

- Record only what QuakeC can observe: globals and entity fields by slot, never the call stack,
  local stack or engine state (TRACE-3.4, TRACE-3.5).
- One meaning, two forms: a compact binary form for CI and a JSON Lines form for people, which
  convert losslessly (TRACE-1.2).
- Canonical encodings and strict readers, so equal traces are equal bytes and a malformed stream
  is refused, not guessed at (TRACE-5.2, TRACE-5.7, TRACE-6.6).
- Values are raw 32-bit patterns; the header carries what a reader needs to interpret entity,
  pointer and string values for either VM (TRACE-2.1, TRACE-7), since the two VMs' edict sizes
  can differ (EDICT-1.3).

## Components

### Model

`Header::new(progs_crc, edict_size, field_offset, entityfields, numstrings)` validates
TRACE-2.3 and keeps its fields private. `Changes::new` sorts global and field changes and refuses
duplicates (crates/qc-trace/src/lib.rs:281-300); the decoder uses the private
`Changes::from_sorted`, which refuses unsorted or duplicate input as `ChangesOutOfOrder`
(lib.rs:304-321). `Text` refuses NUL bytes. `Record` is one of:

| Record | Holds | Spec |
| --- | --- | --- |
| `Run` | function index, change set | TRACE-4.1 |
| `Step` | function, statement, opcode, builtin number if a CALL to a builtin, change set | TRACE-4.2, TRACE-4.3 |
| `Exit` | change set | TRACE-4.5 |
| `Error` | function, statement (-1 before the first), message, change set | TRACE-4.6 to TRACE-4.8 |
| `String` | value, text of an engine-made string | TRACE-4.10 |

`Trace { header, records }`. `Header::entity_number`, `pointer_target` and
`Trace::string_value` interpret patterns (TRACE-7.1 to TRACE-7.3); `string_value` scans records
backwards for the latest string record (lib.rs:463-476).

### Binary form

`encode`, `encode_header`, `encode_record` and `decode` (crates/qc-trace/src/binary.rs). Words
are 4-byte little-endian; uvars are minimal LEB128 of at most 5 bytes; svars are zigzag uvars;
text is a uvar length then bytes (TRACE-5.1 to TRACE-5.3). The 28-byte header is `QCTR` and six
words (TRACE-5.4). The decoder refuses an over-long or non-minimal uvar (binary.rs:287-308),
bounds a declared list length by the remaining bytes before allocating (binary.rs:326-353), and
reports `Truncated` with the record's start offset (binary.rs:257-269).

### JSON Lines form

`to_json`, `header_to_json`, `record_to_json`, `from_json` (crates/qc-trace/src/json.rs). One
object per line; keys in the fixed TRACE-6.6 order; 32-bit patterns as eight lowercase hex
digits; text with each byte outside 0x20 to 0x7E written `\xNN` (TRACE-6.4, TRACE-6.5). The
writer builds lines with `format!` and quotes text through `serde_json::Value::String`
(json.rs:29-126). The reader takes known keys out of the object and refuses leftovers
(json.rs:188-282) and refuses a printable byte written as `\xNN` (json.rs:307-333), so only the
canonical form is accepted.

### Writers and readers

- `qc_vm::trace::Trace` (crates/qc-vm/src/trace.rs) writes the trace for QuakeC Studio's VM
  through the `qc_vm::Observer` seam: the start of each run, each executed statement after its
  effects, the end of each run, and the error once, from the outermost run (TRACE-3.1, TRACE-4.1
  to TRACE-4.9).
- `qs-differential` writes the host's trace (crates/qs-differential/src/host/trace.rs) and
  compares two traces (compare.rs).
- The C oracle writes id's VM's trace (oracles/vm/trace.c); `cargo xtask oracles` decodes it and
  prints the JSON form (xtask/src/oracles.rs:176-177).

## Error model

Every decoder and constructor returns `TraceError`: `BadHeader`, `UnsupportedVersion`,
`BadMagic`, `Truncated` (with the record's start offset), `BadUvar`, `UnknownTag`,
`BadBuiltinFlag`, `OpcodeOutOfRange`, `DuplicateChange`, `ChangesOutOfOrder`, `NulInText`,
`TooLong`, and the JSON reader's errors with their line. A malformed stream is refused, never
repaired (TRACE-5.7, TRACE-6.6).

## Verification

- `crates/qc-trace/src/tests.rs`: unit tests per TRACE statement and property tests that both
  forms round-trip and convert into each other losslessly (TRACE-1.2).
- The C oracle writes the binary form independently (`oracles/vm/trace.c`), and the VM
  vectors compare its traces with this crate's decoding in CI (`cargo xtask oracles`).
- L1 readiness passed (`docs/readiness/p2-r1.md`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| A project-defined format | trace.md, `qc-trace` | Compare id's `PR_PrintStatement` text output | [inferred] text traces cannot carry change sets or survive the two VMs' differing edict sizes; trace.md states the format is the project's |
| Change sets rather than full state | Changed slots per record | Full snapshot per record | TRACE-3.1: smaller streams, and a divergence points at the slot |
| Binary plus JSON | Both, lossless | One form only | TRACE-1.2: binary for CI, JSON to read |
| Canonical encodings, strict readers | Minimal uvars, fixed key order, canonical escapes | Lenient readers | TRACE-5.2, TRACE-6.6: byte equality means trace equality |
| Raw 32-bit values plus interpreting header | `bits` with `edict_size`, `field_offset` | Typed values | TRACE-2.1, TRACE-7: the two VMs' edict layouts can differ (EDICT-1.3), so the header says how to read each |
| Separate crate with no workspace dependencies | `qc-trace` depends only on serde_json | Module of qc-vm | [inferred] the oracle tooling, qs-differential and xtask read traces without linking the VM |
| Hand-built JSON writer | `format!` per line | serde `Serialize` | [inferred] exact key order and hex formatting of TRACE-6 without derive machinery |

## Open questions

### Resolved
1. String records for engine strings builtins set are written by the differential host (P2-61,
   #455).

### Deferred
1. **Duplicate-change error variant.** Decoded duplicates report `ChangesOutOfOrder`, not
   `DuplicateChange` (lib.rs:304-321).
2. **JSON line numbers.** Model errors lose the line (json.rs:169-176); missing final newline
   reports the total line count (json.rs:138-143).
3. **Binary truncation offset** is the record's start (binary.rs:257-269).
4. **Header display** for `BadHeader` embeds "(TRACE-2.3)" (lib.rs:112).
5. **`put_length` fallback** to u32::MAX on overflow, documented unreachable (binary.rs:150-154).
6. **Emitter fallbacks in qc-vm** (crates/qc-vm/src/trace.rs:47-51, 120-122, 173) substitute 0,
   empty change sets or default text instead of surfacing an error.
7. **Two observers with the same algorithm** (qc-vm trace.rs and qs-differential host/trace.rs).
8. **Tests.** `errors_display` has no spec citation (crates/qc-trace/src/tests.rs:1019).
9. **Stale README** (crates/qc-trace/README.md:19-21); trace.md:18 names differential.md by plan
   ID P1-27.

## References

- docs/spec/trace.md, docs/spec/differential.md (DIFF-3.9, DIFF-9, DIFF-10)
- docs/adr/0013-differential-coverage-bar.md
- docs/readiness/p1-r1.md, docs/readiness/p2-r1.md; ratchets.toml `[crates.qc-trace]`
- crates/qc-trace/README.md
- Plan: P1-12 (define the trace format), P1-13 (differential runner) in plan/backlog/p1.yaml;
  P2-61 (#455)
