# Trace vectors

One trace for [`../../trace.md`](../../trace.md), in both forms, with every kind of record:
a host run, a statement, an engine string, a nested run started by a builtin with its
statements and exit, the builtin's `CALL` step after it, a runtime error, and a run that
fails before its first statement with negative numbers in it.

| File | Form |
| --- | --- |
| [`sample.jsonl`](sample.jsonl) | JSON Lines (TRACE-6), exactly as a writer produces it |
| [`sample.hex`](sample.hex) | Binary (TRACE-5), one line per header or record |

`sample.hex` holds bytes as pairs of hex digits separated by spaces; `#` starts a comment
that runs to the end of the line. The bytes, in order, are the binary trace.

The header says the emitting VM has 516-byte edicts with field slot 0 at byte 96 and 105
field slots (so the fields end exactly at the end of the edict, the limit of TRACE-2.3), and
a 16-byte string table. So `00000204` (516) in the `CALL1` step is the entity value of edict 1
(TRACE-7.1), and the string value `00000100` (256) is outside the table and names the text of
the string record (TRACE-7.3).

`qc-trace` decodes both files, checks that they hold the same trace, and checks that encoding
it gives back each file exactly (`crates/qc-trace/src/tests.rs`).

## How they were checked

On 2026-10-16 both files were written by a C program built from `trace.md` alone, without
reading `qc-trace`: it encodes words, uvars, svars, text and change sets as TRACE-5 says and
prints the JSON as TRACE-6 says, for the header and the eleven records above. Built with
Apple clang 21 (`cc -std=c11 -Wall -Wextra -O2`, no warnings), its binary output was
byte-identical to the bytes of `sample.hex`, and its JSON output is `sample.jsonl` as committed. `qc-trace`'s
encoder produces the same bytes from the same values. The program is not committed; the
VM oracle's encoder ([`oracles/vm/trace.c`](../../../../oracles/vm/trace.c)), also written from
`trace.md` alone, is checked on every oracle run: `cargo xtask oracles` decodes each trace it
writes with `qc-trace` and requires encoding the result to give back the same bytes.
