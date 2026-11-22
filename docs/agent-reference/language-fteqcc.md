# The FTEQCC dialect in full

Sources: [`docs/spec/dialect-fteqcc.md`](../spec/dialect-fteqcc.md) (the `FTEF-` statements
cited below), written from FTEQCC, fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e
`engine/qclib`, and from the mods that use it (Nuclide, FreeCS, FreeHL). This is the dialect
of a project whose `project.info` says `fteqcc`, with whatever target it names. It holds
everything of the `rerelease` topic (C-style functions, `for` and `switch`, `++` and `+=`,
`?:`, arrays, `typedef struct`, `enum`, locals that hide globals, calls that keep their
values), and what follows. Read the `rerelease` topic first; its "what stays as in vanilla"
holds here too unless the project turns a flag on, which its own files will show.

What the program may use is the target's, not the dialect's: no construct chooses the progs
format (FTEF-1.5). `project.info` names the target, and `engine.builtins` its builtins. A
mod for id's VM writes with the same syntax and is compiled to id's opcodes; do not assume
an FTE builtin or a class works on the rerelease's engine because the compiler read it.

## Types beyond float

- **`int`** (also `integer`) is a 32-bit signed integer (FTEF-2.1), with the other C words
  for the other widths (FTEF-2.12, FTEF-2.13). A number with a suffix has that type: `12i` is
  an integer, `1u` unsigned, `3.5d` a double, `1.0f` a float (FTEF-3.1). A plain `12` is a
  float, as ever, and `5 / 2` is 2.5. Integer arithmetic needs a target with the integer
  opcodes: for id's VM, `i + 1i` is the error `E0320`, "Opcode \"+|ADD_I\" not valid for
  target". There, count in floats.
- **Casts.** `(type) e` converts to any type (FTEF-16.2).
- **Pointers.** `T *p`, `&lvalue`, `*p` and `p->member` (FTEF-4.1 to FTEF-4.4). They need a
  target with the opcodes: for id's VM the address of a global is the error `E0331`
  (FTEF-4.6). There, use a field or an array.
- **Arrays** with a size, `float xs[8];`, and more than one dimension, `float m[4][4];`
  (FTEF-5.1, FTEF-5.2).
- **`struct` and `union`** by name: `struct Name { float a; vector b; };`, a child `struct
  Child : Parent { ... }`, members that are arrays, pointers, functions or bitfields, and
  `{ .a = 1, .b = '0 0 1' }` to initialise by member (FTEF-6.1 to FTEF-6.5, FTEF-16.11).
  Give a struct its `{ ... }` at file level; in a function body declare it and assign its
  members, since this compiler stops on a local struct with a brace initialiser.
- **`typedef OldType NewName;`** for any type (FTEF-7.1).
- **`enumflags { A, B, C };`** are 1, 2, 4: each unvalued name doubles the last (FTEF-8.1).
  `enum class Name : int { ... }` needs its base type (FTEF-8.5).
- **`__variant`** is compatible with every type, for a parameter or a return that takes
  anything (FTEF-11.1).
- **`[x, y, z]`** builds a vector from three expressions (FTEF-16.3), where `'x y z'` takes
  only numbers.

## Classes

```c
class Barrel : entity
{
    float charge;
    virtual void() Explode;
    void() Barrel;
};

void() Barrel::Explode =
{
    charge = 0;
};
```

- A class is an entity type whose members are entity fields; `class Name : Parent` inherits
  the parent's (FTEF-9.1, FTEF-9.3). `self.member` or `this.member` selects a member, and a
  member function that is not `static` or `nonvirtual` is given `this` (FTEF-9.4). A member
  named as the class is its constructor.
- A member function is `virtual`, `nonvirtual` or `static` (FTEF-9.2): a virtual one is a
  field each entity can override, the others are globals named `Class::member`; one with no
  such word is taken as non-virtual, with a warning. Its body is `Class::member = { ... }`
  (FTEF-9.4). `super::member` names the parent's and `::name` a global (FTEF-16.4).
- A class with a constructor gets a `spawnfunc_Name` function, so a map can spawn it by its
  classname (FTEF-9.5).
- A class is a mod's whole way of working or it is not used at all: write classes only in a
  project that has them.

## Statements and expressions

- A call may be followed by `.member`, `[i]` or another call (FTEF-16.6).
- `if not (e)` and `if !(e)` take the statement when `e` is false (FTEF-16.5, FTEF-18.8).
- `case a .. b:` matches a range (FTEF-18.10).
- `a ?: b` is `a` when `a` is true, else `b` (FTEF-18.11).
- `return = e;` sets what the function will return and goes on (FTEF-18.7).
- `sizeof` takes a type or an expression (FTEF-18.12).
- Adjacent string literals are one string: `"a" "b"` (FTEF-16.8).
- A `for` header may hold several expressions separated by `,` (FTEF-16.9), and so may an
  expression statement (FTEF-18.5).
- A global's initialiser may be any expression (FTEF-18.1).
- A function type may return a function (FTEF-18.15).
- `$framesave` names frames as `$frame` does (FTEF-18.14).

## The preprocessor

`#include`, `#define`, `#undef`, `#ifdef`, `#ifndef`, `#else` and `#endif`, as in every
dialect, and here also `#if expr`, `#elif`, `#elifdef` and `#elifndef` (FTEF-14.1), a macro
with `...` for the rest of its arguments (FTEF-14.5), and `a ## b` to join two tokens in a
macro's body (FTEF-14.14). A module's `defines` in `quakec.toml` are set before its first
file. Headers are usually `.qh` or `.h` files that the `.qc` files include; put a declaration
other files need in the header the project already has for it.

## What a model gets wrong

| Written | What happens | Write |
| --- | --- | --- |
| a class, a pointer or an FTE builtin in a mod for id's VM | compiles to something the engine cannot run, or an error | what the files around yours use; check `project.info` and `engine.builtins` |
| `int` counters in a mod for id's VM | `E0320`: the VM has no integer opcodes | floats |
| `float f = 5 / 2;` expected to be 2 | 2.5: plain numbers are floats | `floor (5 / 2)` |
| `struct Pair p = { 1, 2 };` in a body | this compiler stops | declare, then assign the members |
| `if (a && b ())` to guard `b` | `b` runs anyway | nested `if`s |
| a new declaration in a `.qc` file that another file needs | unknown there | the project's header, included by both |
| vanilla habits kept out of caution, `local`, `v_x`, `0 - x` | they compile, and stand out | the file's own style |
