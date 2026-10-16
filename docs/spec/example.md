# Worked example: entity-string escapes

Prefix: `EXAMPLE` (registered in [`README.md`](README.md)).

The spec half of the worked example in [`../porting.md`](../porting.md). It describes one id
function, `ED_NewString`, in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Implemented by
[`crates/qs-porting-example`](../../crates/qs-porting-example/).

## EXAMPLE-1 Escapes in entity string values

**Source:** `WinQuake/pr_edict.c`, `ED_NewString`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

When the engine loads a map's entity lump or a saved game, `ED_ParseEpair` stores every value
of a string field or global through `ED_NewString`, which copies it into the hunk and decodes
backslash escapes on the way. A mod that puts `\n` in a `message` key relies on this.

In the C, the bounds test `i < l-1` never fails for a backslash, because `l` counts the
terminating NUL, so a trailing backslash reads the NUL as its escaped byte (EXAMPLE-1.4).
`Hunk_Alloc` returns zeroed memory, so the result is always NUL-terminated even when the NUL
was consumed that way.

### Behaviour

- **EXAMPLE-1.1** The input is the bytes before its first NUL byte; anything after it is ignored.
- **EXAMPLE-1.2** A backslash followed by `n` (0x6E) becomes one newline byte (0x0A).
- **EXAMPLE-1.3** A backslash followed by any byte other than `n` becomes one backslash, and
  that byte is dropped. In particular `\\` becomes `\`, `\t` becomes `\` and `\N` becomes `\`.
- **EXAMPLE-1.4** A backslash that is the last byte of the input stays a backslash.
- **EXAMPLE-1.5** Every byte that is not part of an escape is copied unchanged, including
  control bytes and bytes 0x80 to 0xFF.
- **EXAMPLE-1.6** Escapes do not chain: the input is scanned once, left to right, and the
  output is never re-scanned. `\\n` becomes `\n` (two bytes, backslash and `n`).
- **EXAMPLE-1.7** The output is never longer than the input.

### Constants

None. `ED_NewString` has no limits of its own; its output size is the input length plus one.

### Test vectors

Inputs are written as C string literals without the quotes. Checked on 2026-10-15 against
id's `ED_NewString`, copied unchanged from the pinned commit and built with the system C
compiler (with `Hunk_Alloc` replaced by `calloc`), over every input of up to six bytes drawn
from `\`, `n`, `a`, 0xFF and NUL: 19,531 inputs, no divergence from the port. The table is
the subset the unit tests pin.

| Input | Output | Statements |
| --- | --- | --- |
| `ab\0cd` | `ab` | EXAMPLE-1.1 |
| `\\\0n` | `\\` | EXAMPLE-1.1, EXAMPLE-1.4 |
| `Hello\\nWorld` | `Hello\nWorld` | EXAMPLE-1.2 |
| `\\\\` | `\\` | EXAMPLE-1.3 |
| `a\\tb` | `a\\b` | EXAMPLE-1.3 |
| `\\\xff` | `\\` | EXAMPLE-1.3 |
| `path\\` | `path\\` | EXAMPLE-1.4 |
| `\x01\x7f\x80\xff\n` | `\x01\x7f\x80\xff\n` | EXAMPLE-1.5 |
| `\\\\n` | `\\n` | EXAMPLE-1.6 |
| `\\\\\\n` | `\\\n` | EXAMPLE-1.6 |
