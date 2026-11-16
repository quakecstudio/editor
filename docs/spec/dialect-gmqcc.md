# GMQCC dialect

Prefix: `GMQ` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

Describes the dialect GMQCC compiles, the third tier of COMP-3's dialect order after the
rerelease subset ([`dialect-fteqcc-rerelease.md`](dialect-fteqcc-rerelease.md), prefix `FTE`)
and the rest of FTEQCC ([`dialect-fteqcc.md`](dialect-fteqcc.md), prefix `FTEF`). The GMQCC
corpus codebase is Xonotic (`xonotic` in [`corpus/manifest.toml`](../../corpus/manifest.toml),
its project file [`corpus/projects/xonotic/quakec.toml`](../../corpus/projects/xonotic/quakec.toml)).

The source is GMQCC's public documentation: its manual page `doc/gmqcc.1` in
[graphitemaster/gmqcc](https://github.com/graphitemaster/gmqcc) at commit
`888ad563c7ff825ef049105adc9142e46a7cfb83` (2026-10-29), read on 2026-11-16. GMQCC is under
the MIT license (`LICENSE` at that commit: "Copyright (C) 2012, 2013, 2014, 2015 Dale Weiler,
Wolfgang Bumiller", "Permission is hereby granted, free of charge, to any person obtaining a
copy of this software ... to deal in the Software without restriction"), which allows use under
GPL-3.0-or-later. It is not a registered porting upstream ([`../porting.md`](../porting.md#upstreams));
these statements describe what GMQCC accepts, so that we parse and resolve it, and no code is
ported from it. Each statement quotes the manual's text where the manual has it.

Implemented by the GMQCC dialect profile (L-02, #186). No code implements a statement here
yet, so every statement is listed in [`coverage-baseline.tsv`](coverage-baseline.tsv) until
L-02's tests cite it.

## GMQ-1 The options Xonotic compiles with

**Source:** `doc/gmqcc.1`, OPTIONS, `-std=standard`; Xonotic's `qcsrc/tools/qcc.sh`, the
`QCCFLAGS` array (xonotic-data.pk3dir@`7068eb6bb838b97064a4e55646f3df9c3bd633e6`).

GMQCC's dialect is chosen by `-std` and then adjusted by `-f` flags and `-O` optimisations.
Xonotic's build script passes `-std=gmqcc -O3 -Werror -Wall -Wno-field-redeclared -futf8
-freturn-assignments -frelaxed-switch -Ooverlap-locals`, so the dialect this spec describes is
`-std=gmqcc` with those flags.

### Behaviour

- **GMQ-1.1** `-std=gmqcc` turns on the flags the manual lists for it: "`-fadjust-vector-fields`,
  `-fcorrect-logic`, `-ftrue-empty-strings`, `-floop-labels`, `-finitialized-nonconstants`,
  `-ftranslatable-strings`, `-fno-false-empty-strings`, `-Winvalid-parameter-count`,
  `-Wmissing-returnvalues`, `-fcorrect-ternary` (cannot be turned off)". Each is a section
  below.
- **GMQ-1.2** The manual: "Selecting a standard also implies some -f options and behaves as if
  those options have been written right after the -std option, meaning if you changed them
  before the -std option, you're now overwriting them." A flag before `-std` is overridden by
  the standard; a flag after it overrides the standard.
- **GMQ-1.3** A Xonotic module is compiled with `-std=gmqcc` and, after it, `-futf8`,
  `-freturn-assignments`, `-frelaxed-switch` and `-Ooverlap-locals`, which are sections below.

## GMQ-2 Vector fields

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fadjust-vector-fields`.

### Behaviour

- **GMQ-2.1** With `-fadjust-vector-fields`, an assignment to a field pointer of type `.vector`
  assigns the whole field. The manual: "When assigning to field pointers of type .vector the
  common behaviour in compilers like fteqcc is to only assign the x-component of the pointer.
  This means that you can use the vector as such, but you cannot use its y and z components
  directly. This flag fixes this behaviour."

## GMQ-3 Vectors and strings as conditions

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fcorrect-logic`, `-ftrue-empty-strings`,
`-ffalse-empty-strings`.

### Behaviour

- **GMQ-3.1** With `-fcorrect-logic`, a vector used as a condition or a logical operand is true
  when any component is nonzero. The manual: "Most QC compilers translate if(a_vector) directly
  as an IF on the vector, which means only the x-component is checked. This option causes
  vectors to be cast to actual booleans via a NOT_V and, if necessary, a NOT_F chained to it";
  its example: "`if (a_vector)` becomes `if not(!a_vector)`" and "`a = a_vector && a_float`
  becomes `a = !!a_vector && a_float`".
- **GMQ-3.2** With `-ftrue-empty-strings`, an empty string is true. The manual: "An empty string
  is considered to be true everywhere. The NOT_S instruction usually considers an empty string
  to be false, this option effectively causes the unary not in strings to use NOT_F instead."
- **GMQ-3.3** `-std=gmqcc` turns `-ffalse-empty-strings` off (GMQ-1.1). With it on, the manual
  says: "An empty string is considered to be false everywhere. This means loops and if
  statements which depend on a string will perform a NOT_S instruction on the string before
  using it."

## GMQ-4 Labelled loops

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-floop-labels`.

### Behaviour

- **GMQ-4.1** With `-floop-labels`, a loop may carry a label, and `break` and `continue` may name
  the loop they leave or continue. The manual: "Allow loops to be labeled, and allow 'break' and
  'continue' to take an optional label to decide which loop to actually jump out of or
  continue"; its example labels a `for` as `for :outer (i = 0; i < n; ++i)` and continues it from
  an inner `while` with `continue outer;`.

## GMQ-5 Initialised variables

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-finitialized-nonconstants`.

### Behaviour

- **GMQ-5.1** With `-finitialized-nonconstants`, a global with an initialiser stays a variable.
  The manual: "Don't implicitly convert initialized variables to constants." Unlike vanilla
  `qcc`, where "An initialised global is a constant" ([`language.md`](language.md) QC-11.6),
  such a global may be assigned.

## GMQ-6 Translatable strings

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-ftranslatable-strings`.

### Behaviour

- **GMQ-6.1** With `-ftranslatable-strings`, `_("...")` is a string constant marked for
  translation. The manual: "Enable the underscore intrinsic: Using _("A string constant") will
  cause the string immediate to get a name with a "dotranslate_" prefix. The darkplaces engine
  recognizes these and translates them in a way similar to how gettext works."

## GMQ-7 The conditional operator's precedence

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fcorrect-ternary`; OPTIONS, `-std=gmqcc`.

### Behaviour

- **GMQ-7.1** Under `-std=gmqcc`, `?:` has C's precedence. The manual: "Use C's operator
  precedence for ternary expressions. Unless your code depends on fteqcc-compatible behaviour,
  you'll want to use this option"; `-std=gmqcc` includes it, and it "cannot be turned off"
  there.

## GMQ-8 UTF-8

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-futf8`.

### Behaviour

- **GMQ-8.1** With `-futf8`, a character constant may be a UTF-8 encoded character and an escape
  may name a code point. The manual: "Enable utf8 characters. This allows utf-8 encoded
  character constants, and escape sequence codepoints in the valid utf-8 range. Effectively
  enabling escape sequences like '\{x2211}'."

## GMQ-9 Assigning to `return`

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-freturn-assignments`.

### Behaviour

- **GMQ-9.1** With `-freturn-assignments`, `return` may be assigned as a local of the function's
  return type, and a bare `return;` returns its value. The manual: "Enabiling this option will
  allow assigning values or expressions to the return keyword as if it were a local variable of
  the same type as the function's signature's return type"; its example `return = bar();
  return; // returns value of bar`.

## GMQ-10 `switch` cases

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-frelaxed-switch`.

### Behaviour

- **GMQ-10.1** With `-frelaxed-switch`, a `case` label may be an expression that is not a
  constant. The manual: "Allow switch cases to use non constant variables."

## GMQ-11 Overlapping locals

**Source:** `doc/gmqcc.1`, OPTIMIZATIONS, `-Ooverlap-locals`.

Xonotic's build script requires this optimisation ("`-Ooverlap-locals` is required",
`qcsrc/tools/qcc.sh`), so a progs built for it lays locals out as below.

### Behaviour

- **GMQ-11.1** With `-Ooverlap-locals`, functions share one local section unless they have local
  arrays or a local GMQCC sees as possibly uninitialised. The manual: "Make all functions which
  use neither local arrays nor have locals which are seen as possibly uninitialized use the same
  local section", and of the risk: "only as long as the functions cannot be called in a
  recursive manner".
