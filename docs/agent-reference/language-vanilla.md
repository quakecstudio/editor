# The vanilla dialect: QuakeC as id's `qcc` reads it

Sources: [`docs/spec/language.md`](../spec/language.md) (the `QC-` statements cited below),
written from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/`, whose
`qcc.h` is quoted where it says it best. This is the dialect of a project whose `project.info`
says `vanilla`: id's own QuakeC and every mod built with id's compiler. It looks like C and is
not; most errors a model makes here are C habits. Read the files around yours first and write
what they write.

## Tokens

- A number starts with a digit, or with `-` right before a digit: no `.5`, no `1e5`, no hex,
  no suffix (QC-4.1, QC-4.4). `a-5` is the name `a` and the number `-5`, an error; write
  `a - 5` (QC-4.2).
- There is no unary minus: `a = -b;` is "not a name". id writes `0 - b` (QC-4.5).
- A vector is three numbers in single quotes, `'0 0 -16'` (QC-5.1). A string is in double
  quotes on one line; the only escapes are `\n` and `\"`, and `\\` is "Unknown escape char"
  (QC-6.2). Adjacent strings do not join (QC-6.1).
- The operators are `&&` `||` `<=` `>=` `==` `!=` `!` `*` `/` `-` `+` `=` `.` `<` `>` `&`
  `|` (QC-7.1). There is no `++`, `--`, `+=`, `%`, `^`, `~`, `?:`, shift or comma operator
  (QC-7.4): each is "Unknown punctuation" or an error at the next token.
- `//` and `/* */` are comments.

## Types and declarations

- The types are `void`, `float`, `vector`, `string` and `entity`; `.T` is a field holding a
  `T`; `T(T1 a, T2 b)` is a function returning `T` (QC-10.1 to QC-10.3). There is no `int`,
  array, struct, pointer, `const` or cast (QC-10.9, QC-19.1). A function takes at most eight
  parameters.
- A file holds only declarations: a type, names separated by `,`, each with an optional
  `= literal`, then `;` (QC-11.1). The initialiser is one literal of exactly the type; an
  `entity` or a field takes none (QC-11.3). `float x = y;` and `float x = 2 * 3;` are errors
  here, "initialiser is not a constant" (QC-27.8).
- An initialised global is a constant: `float IT_AXE = 4096;`. Do not assign to one (QC-11.6).
- `vector v;` also declares the floats `v_x`, `v_y` and `v_z`, and `.vector f;` the fields
  `f_x`, `f_y`, `f_z` (QC-11.7). They are names, not an operator: `self.origin_z`, never
  `self.origin.z`.
- A name may be declared any number of times with one type and given a value once
  (QC-11.5). That is how a function is declared ahead: `void() later;`.
- Fields and functions are declared at file level only (QC-11.9). A field is added for every
  entity in the game: `.float charge;`.

## Functions

```c
void(entity targ, float damage) hurt =
{
    local float left;

    left = targ.health - damage;
    targ.health = left;
};
```

- The type comes first, then the name, `=`, the body and a `;` after the closing brace
  (QC-11.2). `void hurt(entity targ)` is not this dialect.
- Locals are declared with `local`, and may sit anywhere in the body; they last to the end of
  the function, since a block opens no scope (QC-12.1). A local's `= literal` is stored once,
  not on each call (QC-11.10): assign in a statement instead.
- A local or a parameter cannot hide a global of the same name: it is the global, or "Type
  mismatch on redeclaration" when the types differ (QC-12.3). id's code has globals named
  `s`, `old`, `stemp` and `otemp`; a `local string s;` in a file compiled after them fails.
  Pick names the project does not use.
- `type name = #n;` binds an engine builtin (QC-13.3).
- `void() name = [$frame, next] { ... };` is a frame function: it sets `self.frame`, sets
  `self.think` to `next` and `self.nextthink` to `time + 0.1`, then runs the body (QC-13.6).
  `$frame a b c` lines name a model's frames from 0, for the file they are in only (QC-8.2,
  QC-8.3). A `$name` ends only at white space, `,` or `;`: `($b)` looks up `b)` (QC-8.6).
- The return type is not checked, and falling off the end returns (QC-13.7, QC-13.8).

## Statements

`{ ... }`, `local`, `return`, `if (...) ... else ...`, `while (...) ...`, `do ... while (...);`
and an expression followed by `;`: nothing else (QC-14.1). There is no `for`, `switch`,
`break`, `continue` or `goto` (QC-14.8). An empty block `{}` and a lone `;` are errors
(QC-14.2). Leave a loop by its condition or by `return`.

## Operators

Tightest first (QC-15.1):

| Priority | Operators |
| --- | --- |
| 1 | `.` and the call's `(` |
| 2 | `*` `/` `&` `\|` |
| 3 | `+` `-` |
| 4 | `==` `!=` `<=` `>=` `<` `>` |
| 5 | `=` (right to left) |
| 6 | `&&` `\|\|` |

- `&` and `|` bind as tightly as `*`: `a + b & c` is `a + (b & c)` (QC-15.2). `!` takes the
  whole comparison after it: `!a == b` is `!(a == b)` (QC-15.6). `=` binds tighter than `&&`:
  `a = b && c` stores `b` (QC-15.3). Use parentheses.
- `&&` and `||` evaluate both sides (QC-15.10; `qcc.h`: "The && and || operators DO NOT EARLY
  OUT like C!"). `if (e != world && e.health > 0)` reads `e.health` of the world when `e` is
  the world, which is harmless; `if (ok && Fire ())` always fires. Nest the `if`s.
- Types must match exactly, with no conversion (QC-16.1, QC-19.1): `float * vector` and
  `vector * float` scale, `vector * vector` is the dot product, and there is no `vector /
  float` (multiply by `1 / f`). `&&`, `||`, `&` and `|` take floats only: `if (self.enemy &&
  x)` is "type mismatch for &&" (QC-16.2). `==` and `!=` compare floats, vectors, strings (by
  their text), entities and functions.
- A call cannot be followed by `.` or another call: `find (world, classname, "x").health` is
  an error (QC-15.8). Store the result first.

## Calls share their storage

- The arguments of every call go through one set of slots, so a call inside a later argument
  overwrites the earlier ones: in `g (4, h (1))`, `g` receives 1 (QC-18.5). Compute the inner
  call into a local first. A call inside the first argument is safe.
- Every call returns through one slot, so `f () + g ()` adds `g ()` to itself (QC-18.6).
  Store one in a local.
- A call with no argument is checked for no count; with any, too many, too few and a wrong
  type are each an error (QC-18.8).

## Conditions

An `if` or `while` tests only the first word of its value (QC-19.2, QC-19.3, QC-19.4):

- a vector is true when its x is not 0: test `v != '0 0 0'`;
- a string is false only when it is the null string, the value of a string never set; the
  literal `""` is true, though `!""` is true as well. Test `s != ""` or `!s`, as the files
  around yours do;
- an entity is false when it is the world, a function when it is null.

## Files

- `progs.src` names the output, then each source file; the files are compiled in that order
  as one program, and a name must be declared before it is used (QC-12.5, QC-23.3). A new
  file goes after the files that declare what it uses.
- Every global function declared must be defined somewhere, or the build fails with
  "function NAME was not defined" (QC-11.8).
- This compiler also takes `#include "file"`, `#define`, `#undef`, `#ifdef`, `#ifndef`,
  `#else` and `#endif` in the vanilla dialect (QC-9.2 to QC-9.6), which id's `qcc` does not:
  use them only where the project already does.

## What a model gets wrong

| Written | What happens | Write |
| --- | --- | --- |
| `void fire(float n) { ... }` | not a declaration of this dialect | `void(float n) fire = { ... };` |
| `float n = self.health;` in a body | no `local`, and no literal | `local float n;` then `n = self.health;` |
| `for (...)`, `i++`, `x += 1`, `a ? b : c`, `%` | errors | a `while`, `i = i + 1`, an `if` |
| `-x`, `x-1` | "not a name", or an error at `-1` | `0 - x`, `x - 1` |
| `self.origin.z`, `v.x` | error | `self.origin_z`, `v_x` |
| `if (a && b ())` to guard `b` | `b` always runs | nested `if`s |
| `if (self.enemy && ...)` | "type mismatch for &&" | `if (self.enemy != world)` |
| `f (a, g (b))` | `a` is overwritten | `local` for `g (b)` first |
| `x = f () + g ();` | `g ()` twice | `local` for `f ()` first |
| `if (v)` on a vector | tests x only | `if (v != '0 0 0')` |
| `"a" "b"`, `"\t"`, `"\\"` | no joining, "Unknown escape char" | one literal, `\n` and `\"` only |
| a body with no `;` after `}` | "expected ;" | `};` |
| a local named as a global | it is the global, or a type error | another name |
