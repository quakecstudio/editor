# The rerelease's dialect: FTEQCC built for id's VM

Sources: [`docs/spec/dialect-fteqcc-rerelease.md`](../spec/dialect-fteqcc-rerelease.md) (the
`FTE-` statements cited below), written from the 2021 rerelease's QuakeC and from FTEQCC,
fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e `engine/qclib`; the vanilla rules it
keeps are [`docs/spec/language.md`](../spec/language.md)'s. Read it for any project whose
`project.info` says `fteqcc`: it is the whole dialect of the rerelease's own code, its
mission packs and mods built on them, which run on id's VM, and the part of FTEQCC every
other FTEQCC project starts from. Everything of the
`vanilla` topic still compiles here; this topic is what is added, and what stays as it was.

The program is still a version 6 `progs.dat` of id's opcodes (FTE-1.1): each construct below
is compiled down to them, so none needs a newer engine.

## What is added

- **C-style functions.** `void fire(float n) { ... }`, with the `;` after the body optional,
  beside id's `void(float n) fire = { ... };` (FTE-17.1, FTE-17.3). The rerelease writes both;
  match the file.
- **Locals with no `local`**, declared where they are used and initialised by any
  expression: `float left = targ.health - damage;` (FTE-21.1). A local or a parameter hides a
  global of the same name (FTE-22.1).
- **`for`, `switch`, `break` and `continue`.** `for (float i = 0; i < n; i++)` (FTE-2.1,
  FTE-2.2). A `switch` compares its value to each `case` constant, falls through from one
  label to the next unless a `break` leaves, and takes `default` when none matches (FTE-3.1 to
  FTE-3.3). `break` and `continue` outside a loop or switch are errors (FTE-4.2).
- **`++`, `--`, `+=`, `-=`, `*=`, `/=`** on floats and vectors, on a field too (FTE-5.1,
  FTE-6.1), and **`%`**, the remainder of two floats (FTE-7.1).
- **`cond ? a : b`**, both arms of one type (FTE-8.1, FTE-8.2).
- **Unary minus and `a-1`.** `-x` negates, and a negative number where an operator belongs is
  a subtraction (FTE-19.4, FTE-19.5).
- **`v.x`, `v.y`, `v.z`** on a vector, readable and assignable, `self.origin.z += 8` included
  (FTE-9.1, FTE-9.4). `v_x` still works.
- **`const`, `var` and `nosave`.** `const float SPEED = 320;` is a constant that may
  initialise another; assigning to it is an error (FTE-10.1 to FTE-10.3). `var float count =
  0;` is a global with a first value that may be assigned, where a bare `float count = 0;`
  is a constant (FTE-11.1, FTE-11.2). `nosave float flag;` is left out of saved games
  (FTE-12.1).
- **Arrays.** `string names[] = { "a", "b" };` or `float xs[4];`, `xs[i]` with a float
  index, and `xs.length` (FTE-13.1 to FTE-13.3). An index that is no constant is a call to a
  function the compiler writes for the array (FTE-13.12, FTE-13.11): correct, and slower than
  a field. An array is a global, not a field: for a value on each entity use a field.
- **`typedef struct { string mdl; float frame; } pose;`**, `pose p;`, `p.mdl`, and `{ a, b }`
  to initialise one or each element of an array of them (FTE-14.1 to FTE-14.3), at file
  level or in a function body (FTE-14.10). In a body a member left out is set to 0, but of an
  array member only the first element, and a vector given fewer than three numbers keeps
  whatever its other parts held: give every part you will read.
- **`enum { A, B, C };`**, float constants from 0, with `A = n` to set the next value;
  `enum Name { ... }` also names a type that is a float (FTE-15.1, FTE-15.2, FTE-15.4).
- **Parameters.** A default value, `void spawn_gib(string mdl, float speed = 200)`, used
  where the call leaves the argument out; `...` after named parameters (FTE-18.1, FTE-18.2,
  FTE-18.7).
- **Calls keep their values.** Every argument is read before any is stored, and a call's
  value is copied before a later call overwrites it: `g (4, h (1))` and `f () + g ()` mean
  what they do in C (FTE-18.4, FTE-18.5).
- **Casts**, `(float) e` (FTE-19.6), and the operators `^`, `~`, `<<`, `>>`, `&=`, `|=`
  (FTE-19.1); `vector / float` (FTE-19.9).
- **Literals.** `.5`, `1.0f`, and the escapes `\r`, `\t`, `\\`, `\'` and the rest beside `\n`
  and `\"` (FTE-20.1, FTE-20.2, FTE-20.4).
- **An empty block `{ }` and a lone `;`** are allowed (FTE-21.4, FTE-21.5).
- **Builtins by name.** `void(string s, ...) bprint = #0:ex_bprint;` binds a builtin by the
  engine's name for it, where a number alone, `#23`, binds as in vanilla (FTE-17.4,
  FTE-18.1). `engine.builtins` lists what the project's engine has.
- **`#pragma`**, as `#pragma warning disable F317`; one this compiler does not know is
  ignored with a warning (FTE-16.1, FTE-16.2).
- **Frame names carry on** from one file to the next (FTE-22.4), where vanilla forgets them
  at each file.
- With FTEQCC's `autoproto` (the mission pack Dawn of the Machine needs it, FTE-1.2), a
  function or constant may be used above its declaration (FTE-22.3). Without it, declare
  before use as in vanilla.

## What stays as in vanilla

These were checked by running this compiler on the rerelease's own code with the target
`rerelease`; the test `the_dialect_topics_hold_when_run` keeps them true.

- `&&` and `||` still evaluate both sides: `if (ok && Fire ())` always fires. Nest the `if`s.
- `&`, `|`, `^`, `<<` and `>>` still bind as tightly as `*` (FTE-19.3), `!a == b` is still
  `!(a == b)`, and `?:` binds tighter than `&&` and `||` (FTE-8.4). Use parentheses.
- An `if` on a string still tests whether it is the null string: `if ("")` is true. An `if`
  on a vector is true when any component is not 0, where vanilla tests only x.
- Numbers are floats: `7 / 2` is 3.5, and `enum : int` is accepted with its names floats
  (FTE-15.2). `int` is a type the compiler reads, but id's VM has no integer opcodes, so
  arithmetic on one is an error: count in floats.
- A file is still a list of declarations, a name is still declared before it is used unless
  `autoproto` is on, and `progs.src` still lists the files in order.
- A field is still declared at file level for every entity, `.float charge;`, though a body
  may now declare a field or a function variable as a local (FTE-21.2).

## What a model gets wrong

| Written | What happens | Write |
| --- | --- | --- |
| `if (a && b ())` to guard `b` | `b` always runs | nested `if`s |
| `if (s)` to test for an empty string | `""` is true | `if (s != "")` |
| `int i`, `i / 2` | an error: id's VM has no integer opcodes | `float i`, and `floor (i / 2)` to truncate |
| `a + b & c`, `!a == b` | `a + (b & c)`, `!(a == b)`: not C's | parentheses |
| an array as an entity's own data | one array for every entity | a field, `.float charge;` |
| `float count = 0;` at file level for a counter | FTEQCC takes an initialised global for a constant (FTE-11.2) | `var float count = 0;` or `float count;` |
| a new file not in `progs.src` | never compiled | add it after the files it uses |
