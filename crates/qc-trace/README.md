# qc-trace

The shared VM trace format: one record per QuakeC statement executed, with the function,
statement index, opcode, the globals and entity fields it changed and the builtin it called,
plus records for runs, returns to the host, runtime errors and engine strings. It has a
compact binary form and a JSON Lines debug form, with an encoder and a strict decoder for
each, and the rules for reading entity, pointer and string values across VMs whose memory
layouts differ.

`qc-vm`'s trace hook (P1-10) writes traces through this crate; the id VM oracle (P1-11)
writes the same bytes with its own C encoder, from the spec alone; the differential runner
(P1-13) decodes both and compares them. The format is a design of this project, not a port
of id's code.

- **Layer:** L1 VM. It depends on no other workspace crate; `serde_json` parses the JSON
  form.
- **Spec sections:** [TRACE-1 to TRACE-7](../../docs/spec/trace.md), with the vectors in
  [`docs/spec/vectors/trace/`](../../docs/spec/vectors/trace/README.md).
- **Not this crate's:** the statements about what an emitter records and when, TRACE-1.3,
  TRACE-3.1, TRACE-3.4, TRACE-3.5, TRACE-4.1 to TRACE-4.7 and TRACE-4.9, which `qc-vm`'s trace
  hook tests cite; until then they are in the coverage baseline.
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs` (TEST-4 in [`docs/testing.md`](../../docs/testing.md)):

- Decoding the binary form of any trace gives the trace back (TRACE-1.2, TRACE-5.7).
- Decoding the JSON form of any trace gives the trace back (TRACE-1.2, TRACE-6.6).
- Converting binary to JSON and back gives the same bytes (TRACE-1.2).
- Any byte stream the binary decoder accepts re-encodes to exactly the same bytes, so every
  trace has one binary encoding (TRACE-5.7).
