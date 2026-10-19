# qc-types

The eight QuakeC types and the slots their values occupy: the type enum (id's `etype_t`), slot
counts (id's `type_size`), the raw 4-byte slot (id's `eval_t`), typed values and their
conversion to and from slots, and the reserved global slots for a call's return value and
parameters (`OFS_NULL` to `RESERVED_OFS`). Ported from `WinQuake/pr_comp.h`, `pr_edict.c` and
`progs.h` at the commit pinned in [`docs/porting.md`](../../docs/porting.md).

- **Layer:** L0 formats. It depends on no other crate; the progs.dat loader (`qc-progs`) and
  the VM build on it.
- **Spec sections:** [PROGS-16](../../docs/spec/progs.md#progs-16-the-eight-qc-types),
  [PROGS-17](../../docs/spec/progs.md#progs-17-values-in-slots) and
  [PROGS-18](../../docs/spec/progs.md#progs-18-reserved-global-slots).
- **Spec statements:** the parameter limit LIMIT-1.4 in
  [`limits.md`](../../docs/spec/limits.md#limit-1-progsdat-format-limits).
- **Shipped:** yes.

## Invariants

Each has a property test in `src/tests.rs` (TEST-4 in [`docs/testing.md`](../../docs/testing.md)):

- Reading a slot as `f32` or `i32` and storing it back keeps every bit (PROGS-17.1).
- Writing any value to slots and reading it back gives the same slot bits, NaN payloads
  included (PROGS-17.1).
- Reading any slots as any type and writing the value back reproduces the slots (PROGS-17.1).
- A definition's type number survives classification unchanged, and only 0 to 7 are known
  types (PROGS-16.3).
- An entity number converts to its byte offset and back for any positive edict size
  (PROGS-17.4).
