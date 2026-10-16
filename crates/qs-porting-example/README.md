# qs-porting-example

The worked example of the porting process in [`docs/porting.md`](../../docs/porting.md): id's
`ED_NewString` (`WinQuake/pr_edict.c`) ported to Rust as `ed_new_string`, with id's
copyright notice, a citation of the file and function, and tests that cite the spec.

- **Layer:** L1 VM. It models what the engine does to entity string values; it depends on no
  other crate.
- **Spec sections:** [EXAMPLE-1](../../docs/spec/example.md#example-1-escapes-in-entity-string-values)
  (statements EXAMPLE-1.1 to EXAMPLE-1.7).
- **Shipped:** no. Nothing depends on it; it exists to be read and copied.
