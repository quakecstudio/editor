# qc-builtins

The vanilla QuakeC builtins and a deterministic mock engine for them. id's engine exposes 78
builtins to QuakeC, numbers 1 to 78, dispatched by number from `pr_builtins`; this crate holds
their table, the pure math and string builtins that need no engine, a deterministic mock
engine implementing the fixture of `docs/spec/differential.md`, which the unit and differential
tests and `qs-sim` run on.

- `table`: the number, name, prototype and mock class of builtins 1 to 78, with the `PF_Fixme`
  gaps (5, 33, 39, 42, 50, 60 to 66, 71) absent (BUILTIN-1, BUILTIN-4, BUILTIN-5).
- `math`: the pure math and string builtins, exact without an engine: `makevectors`,
  `normalize`, `vlen`, `vectoyaw`, `vectoangles`, `random`, `rint`, `floor`, `ceil`, `fabs`,
  `ftos`, `vtos`, and `anglemod`, with id's float-to-int rule from `qc_vm::convert`
  (BUILTIN-2.1, BUILTIN-2.2, BUILTIN-6, BUILTIN-7, BUILTIN-15.5).
- `mock`: the deterministic mock engine fixture (the precache lists and model bounds, the
  lightstyles, the cvars, the `changelevel` flag, the command buffer, the message buffer sizes
  and the DIFF-6.2 random source) that two independent runners share (DIFF-6, DIFF-7).
- `MockBuiltins`: implements `qc_vm::Builtins` over the mock engine, so a
  `qc_vm::Machine` can run a progs that calls builtins; it reads each builtin's arguments from
  the parameter slots and writes its result to `OFS_RETURN`, the system globals or the edicts
  (BUILTIN-1.3, BUILTIN-1.4, PROGS-18). A number with no builtin fails with id's
  `unimplemented bulitin` (BUILTIN-1.2). Every vanilla builtin has a body: the pure math and
  strings (BUILTIN-6, BUILTIN-7), the entity, placing, precaching, printing, command, setting,
  sound, particle, lightstyle, static-entity, network-message and error builtins against the
  mock fixture (BUILTIN-8 to BUILTIN-13, BUILTIN-16), the class world builtins by DIFF-7
  (BUILTIN-14, BUILTIN-15), and the argument checks of BUILTIN-2 with the runtime errors of
  DIFF-6.6 (`invalid entity`, `invalid string`, `invalid field`, `invalid lightstyle`).
- `BuiltinError`, `RuntimeError` and `FatalError`: the typed errors a builtin raises
  (BUILTIN-3), one variant per message, each displayed with id's bytes (DIFF-8.2). The class
  world builtins (BUILTIN-4.4) are marked in `table` and answered by DIFF-7 over the mock.
- `localization`: the rerelease's keyed strings of `localization/loc_english.txt`, looked up
  by the text the print builtins and `WriteString` send (RRE-1).
- vkQuake's extension builtins (BUILTIN-18): `tokenize`, the token list `tokenize`,
  `tokenize_console` and `tokenizebyseparator` fill and `argv` reads; `printf`, `sprintf`'s
  directive reader and the C conversions it hands each directive to; `atof`, C's `strtod` of the
  longest numeric prefix; `strconv`, a string's case and Quake's coloured-character offsets;
  `cstring`, the C string comparisons the string builtins call; and `strftime`, C's `strftime`
  of the mock's one instant.

## Layer

L1 VM. It depends on `qc-vm`, `qc-edict` (both L1) and `qc-progs`, `qc-types` (both L0), and
nothing higher.

## Spec sections

- **Spec sections:** [BUILTIN-1, BUILTIN-2, BUILTIN-3, BUILTIN-4, BUILTIN-5, BUILTIN-6,
  BUILTIN-7, BUILTIN-8, BUILTIN-9, BUILTIN-10, BUILTIN-11, BUILTIN-12, BUILTIN-13, BUILTIN-14,
  BUILTIN-15, BUILTIN-16, BUILTIN-18](../../docs/spec/builtins.md), the rerelease's
  localization [RRE-1](../../docs/spec/engine/rerelease.md) (`localization`), and the mock
  engine of [DIFF-6, DIFF-7](../../docs/spec/differential.md). BUILTIN-17, the rerelease's
  prototypes, is not implemented here: no code or test cites it, and its statements are in the
  coverage baseline.
- **Not this crate's:** RRE-1.10, the localization file's lookup on the search path, which
  `qs-vfs` implements and cites.

The table, the mock classes, the pure math and string builtins and the class state, output and
world builtin bodies against the mock fixture are complete.

## Properties

`src/properties.rs` holds the proptest properties of the readers of outside text, the
invariants the `qc-builtins` fuzz target (`fuzz/fuzz_targets/qc_builtins.rs`) checks on
arbitrary bytes:

- `Localization::loc_load_file` reads a file only up to its first zero, skips a UTF-8 byte
  order mark, and reads `key = "value"` entries back by `$key`, the first of a repeated key
  kept (RRE-1.7, RRE-1.2).
- `pf_var_string` fits its 1024-byte buffer with its terminator, and with no file loaded is
  the parameters joined and cut to 1023 bytes (RRE-1.9).
- `Tokens::tokenize` and `Tokens::split` keep at most 64 tokens, each inside the text after
  the one before, `argv` at most 1023 bytes and the same from a negative index, and -1 or the
  null string outside the list; a split token is the text from its start to its end
  (BUILTIN-18.23, BUILTIN-18.24).
- `sprintf`'s text stops at 1023 bytes for any format and arguments, and a format with no
  directive is itself (BUILTIN-18.8).
- `atof` reads back every finite double Rust writes, after white space and a sign, ignoring
  what follows the number, and every finite double written exactly in hexadecimal, padded with
  zero digits, bit for bit: a hexadecimal number is correctly rounded at any length
  (BUILTIN-18.25).
