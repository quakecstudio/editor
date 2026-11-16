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

Where the manual is silent or disagrees with GMQCC's code, the statement says so and follows
the code at the same commit (`main.cpp`, `opts.cpp`), or GMQCC's own tests in `tests/`, whose
`.tmpl` files give each test's options and expected output.

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
- **GMQ-1.4** GMQCC's code turns on more under `-std=gmqcc` than the manual lists, and the code
  is what GMQCC does: `main.cpp`'s `-std=gmqcc` branch (lines 136 to 150 at the pinned commit)
  also sets `SHORT_LOGIC`, `UNTYPED_NIL`, `VARIADIC_ARGS` and `EXPRESSIONS_FOR_BUILTINS` and the
  `BREAKDEF` warning, which GMQ-12 to GMQ-15 describe. `CORRECT_TERNARY` and
  `ADJUST_VECTOR_FIELDS` are on by default (`opts.cpp`, `opts_init`, lines 75 and 76), and
  `-std=gmqcc` leaves them on.

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

## GMQ-12 `nil`

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-funtyped-nil`; on under `-std=gmqcc` (GMQ-1.4).
Xonotic: `lib/compiler.qh` defines `QCC_SUPPORT_NIL`, and `lib/nil.qh` then defines `func_null`
and `string_null` as `nil`.

### Behaviour

- **GMQ-12.1** `nil` is a global of no type that may be assigned to anything, with no type check.
  The manual: "Adds a global named 'nil' which is of no type and can be assigned to anything. No
  typechecking will be performed on assignments. Assigning to it is forbidden, using it in any
  other kind of expression is also not allowed."
- **GMQ-12.2** Assigning `nil` zeroes the whole target. The manual: "the nil global is an actual
  global filled with zeroes, and can be assigned to anything including fields, vectors or
  function pointers, and they end up becoming zeroed."

## GMQ-13 Variadic arguments

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fvariadic-args`; on under `-std=gmqcc` (GMQ-1.4).
Xonotic: `common/notifications/all.qc`, `Local_Notification(..., ...count)`;
`common/minigames/cl_minigames.qc`, `...(i,string)`.

### Behaviour

- **GMQ-13.1** A function's last parameter may be `...` with a name, optionally after a type,
  which holds the number of variadic arguments passed, and `...(index, type)` reads the variadic
  argument `index` as `type`. The manual: "Allow variadic parameters to be accessed by QC code.
  This can be achieved via the '...' function, which takes a parameter index and a typename";
  its example `void vafunc(string...count)` loops `for (i = 0; i < count; ++i)
  print(...(i, string), "\n");`.
- **GMQ-13.2** GMQCC's test `tests/varargs.qc` calls `nbva(1, "Hello", "You", "There")` on
  `void nbva(float a, string...count)` and expects "You gave me 3 additional parameters", and
  `...(0, string)` gives "Hello": the count excludes the named parameters, and index 0 is the
  first variadic argument.

## GMQ-14 Short-circuit logic

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fshort-logic`; on under `-std=gmqcc` (GMQ-1.4).

### Behaviour

- **GMQ-14.1** `&&` and `||` stop at the first operand that decides them, and give 0 or 1. The
  manual: "Perform early out in logical AND and OR expressions. The final result will be either a
  0 or a 1".

## GMQ-15 Builtin numbers

**Source:** `doc/gmqcc.1`, COMPILE FLAGS, `-fexpressions-for-builtins`; on under `-std=gmqcc`
(GMQ-1.4).

### Behaviour

- **GMQ-15.1** A builtin's number may be any constant expression. The manual: "Usually
  builtin-numbers are just immediate constants. With this flag expressions can be used, as long
  as they are compile-time constant"; its example `void printB() = #2-1;`.

## GMQ-16 Attributes

**Source:** `doc/gmqcc.1`, COMPILE WARNINGS, `-Wunknown-attribute`, and COMPILE FLAGS,
`-fdefault-eraseable`; `tests/accumulate.qc`, `tests/aliases.qc`. Xonotic: `lib/compiler.qh`
defines `QCC_SUPPORT_ACCUMULATE`, `QCC_SUPPORT_ERASEABLE` and `QCC_SUPPORT_ALIAS`, and
`lib/_all.inc` then defines `ACCUMULATE` as `[[accumulate]]`, `ERASEABLE` as `[[eraseable]]` and
`ALIAS(var)` as `[[alias(var)]]`.

### Behaviour

- **GMQ-16.1** A definition may be preceded by attributes in double brackets. An attribute GMQCC
  does not know is warned about: "Warn on an unknown attribute. The warning will include only the
  first token inside the enclosing attribute-brackets."
- **GMQ-16.2** `[[accumulate]]` on a function lets it be defined more than once, each definition's
  body run after the ones before it. `tests/accumulate.qc` defines `[[accumulate]] void foo ()`
  three times, printing `"hello "`, `"accumulation "` and `"world\n"`, and expects one call of `foo()`
  to print "hello accumulation world".
- **GMQ-16.3** `[[alias("name")]]` declares another name for the definition `name`.
  `tests/aliases.qc` declares `[[alias("alias_2")]] void alias_2_aliased();` and
  `[[alias("alias_1")]] float alias_1_aliased;`, and expects a call of `alias_2_aliased()` to run
  `alias_2` and `alias_1_aliased` to read `alias_1`'s 3.14; a `vector` alias's components are
  named `alias_3_aliased_x` and so on.
- **GMQ-16.4** `[[eraseable]]` lets GMQCC remove the definition if it is unused. The manual, of
  `-fdefault-eraseable`: it "permits the compiler to remove unused functions, variables and
  statements. This is equivlant to putting [[eraseable]] on all definitions", and `[[noerase]]`
  prevents it.

## GMQ-17 `noref`

**Source:** `doc/gmqcc.1`, COMPILE WARNINGS, `-Wunused-variable`; `tests/noref.qc`. Xonotic:
`lib/spawnfunc.qh`, `noref bool require_spawnfunc_prefix;`.

### Behaviour

- **GMQ-17.1** `noref` before a declaration keeps an unused variable from being warned about,
  and `#pragma noref 1` to `#pragma noref 0` does so for every declaration between. The manual:
  "This can be avoided by adding the noref keyword in front of the variable declaration.
  Additionally a complete section of unreferenced variables can be opened using #pragma noref 1
  and closed via #pragma noref 0."

## GMQ-18 Exponentiation

**Source:** `tests/exponentiation.qc` (options `-std=gmqcc`). The manual does not describe the
operator. Xonotic: `lib/compiler.qh` defines `QCC_SUPPORT_POW`.

### Behaviour

- **GMQ-18.1** `a ** b` raises `a` to the power `b`, as `pow(a, b)` and `__builtin_pow(a, b)` do.
  `tests/exponentiation.qc` sets `hundy` to 10 and expects `ftos(hundy ** 2)` and
  `ftos(pow(hundy, 2))` each to print "100".

## GMQ-19 `goto` and labels

**Source:** `tests/goto.qc` (options `-std=gmqcc`). Xonotic: `lib/compiler.qh` defines
`LABEL(id)` as `:id`, used with `goto` (`client/hud/panel/scoreboard.qc`,
`LABEL(downarrow_action);`).

### Behaviour

- **GMQ-19.1** A label is written as a colon before its name, `:name`, where a statement may
  stand, and `goto name;` jumps to it. `tests/goto.qc` writes `:label_1; print("label_1",
  "\n"); goto label_5;` and expects its jumps to print the labels in the order its comment
  gives: label_3, label_2, label_4, label_3, label_1, label_5.
- **GMQ-19.2** `goto` may take a conditional choice between two labels.
  `tests/goto.qc`: `goto (x == y) ? label_1 : label_2;` goes to `label_2` while `x` is 1 and `y`
  is 2.

## GMQ-20 The corpus inventory

**Source:** `xtask/src/gmqcc_coverage.rs`.

`cargo xtask gmqcc-coverage <dir>...` scans every `.qc`, `.qh`, `.inc` and `.h` file under the
GMQCC corpus (Xonotic's `qcsrc`, fetched with `cargo xtask corpus fetch xonotic`) for the
constructs above that the FTEQCC grammar lacks. The recorded run is
[`vectors/dialect-gmqcc/coverage.txt`](vectors/dialect-gmqcc/coverage.txt).

### Behaviour

- **GMQ-20.1** Every construct class the scan finds is named by a section of this file, and every
  class it looks for is found in the corpus; the scan fails otherwise.
- **GMQ-20.2** Each class is found by its tokens, a directive's body included, since Xonotic
  writes several of them through C preprocessor macros: `_` then `(` then a string (GMQ-6),
  `return` then `=` (GMQ-9), `nil` (GMQ-12), `...` then a name or `(` (GMQ-13), `[[` then a
  name (GMQ-16), `noref` (GMQ-17), two touching `*` (GMQ-18), and `goto` or a `:` touching a
  name (GMQ-19).

## GMQ-21 Stringizing the variadic arguments

**Source:** `ftepp.cpp`, `ftepp_macro_expand`, at the commit above. Xonotic: `qcsrc/Makefile`
(`CPP := cc -xc -E`), `lib/deglobalization.qh` (`#define DEGLOB_LOG(kind, name, ...)
deglob_log(kind, name, __FILE__, __LINE__, __FUNC__, #__VA_ARGS__)`, with the comment "Varargs to
this should already be stringized").

### Behaviour

- **GMQ-21.1** Under this dialect, `#__VA_ARGS__` in a variadic macro's body is the variadic
  arguments' text as one string, `""` when none is given, as the C preprocessor that Xonotic runs
  before GMQCC writes it: `L(1, "o", x + 2)` of `#define L(a, ...) f(a, #__VA_ARGS__)` is `f(1,
  "\"o\", x + 2")`. The string is C's `#` (C11, N1570 6.10.3.2): "Each occurrence of white space
  between the argument's preprocessing tokens becomes a single space character in the character
  string literal. White space before the first preprocessing token and after the last
  preprocessing token composing the argument is deleted. ... a \ character is inserted before
  each " and \ character of a character constant or string literal (including the delimiting "
  characters)", and a comment is white space, "Each comment is replaced by one space character"
  (5.1.1.2). A named parameter's `#param` is the same string; FTEQCC's keeps the text's white
  space and comments (`QCC_PR_ExpandStrCatMarkup`, FTEF-14.31). This is a deliberate divergence from GMQCC's own
  preprocessor. There, `case '#':` stringizes only a parameter's name (`nextok == TOKEN_IDENT ...
  && macro_params_find(...)`), `__VA_ARGS__` is a token of its own (`TOKEN_VA_ARGS`), and so `#`
  is written as it is. Xonotic is never built that way, and `lib/deglobalization.qh` relies on the
  C behaviour in 165 places.
- **GMQ-21.2** Under this dialect, `__VA_OPT__(x)` in a variadic macro's body is `x`, its own
  `__VA_ARGS__` written, when the variadic arguments are given and hold a token, white space and
  comments being none, and nothing otherwise, as the C preprocessor Xonotic runs first writes it: `O(1)` and `O(1, )` of `#define
  O(a, ...) f(a __VA_OPT__(, g(__VA_ARGS__)))` are `f(1)`, `O(1, 2)` is `f(1, g(2))`. GMQCC's own
  preprocessor has no `__VA_OPT__`, a deliberate divergence from it as in GMQ-21.1; Xonotic's
  `server/compat/quake3.qh` writes `__VA_OPT__(this.count *= (__VA_ARGS__);)`.
- **GMQ-21.3** Under this dialect, an expansion whose text ends with the name of a function-like
  macro takes the parenthesised group that follows the use, as the C preprocessor Xonotic runs
  first rescans an expansion together with the text after it. `ORDERED(F)(a, b)` of `#define
  ORDERED(F) F##_UNORDERED` calls `F_UNORDERED(a, b)`, and `LATE(3)` of `#define LATE G` calls
  `G(3)`. Xonotic's `lib/iter.qh` writes `#define FOREACH_ENTITY_CLASS(class, cond, body)
  ORDERED(FOREACH_ENTITY_CLASS)(class, cond, body)`. GMQCC's own preprocessor reads an
  expansion's output as a string of its own (`lex_open_string(ftepp->output_string, ...)`), so
  this is a deliberate divergence from it as in GMQ-21.1. The group is the one in the text the use
  stands in, past the strings and comments in it; one beyond that text's end, as after a chain of
  object-like macros (`#define W ORDERED(F)` and `W(1, 2)`), is not taken, where the C
  preprocessor would take it. FTEQCC finds a function-like macro's `(` only in the text it stands
  in (FTEF-14.22).
- **GMQ-21.4** Under this dialect a directive at the start of a line inside a macro call's
  arguments is run as it would be outside the call, and the argument is its text around the
  directive and what the directive skips, the line break before the directive kept. Xonotic's
  `common/items/item/health.qh` writes `ATTRIB(HealthSmall, m_icon, string, // ...` with `#ifdef
  CSQC`, `#else` and `#endif` among its last argument's lines. C leaves this undefined (C11, N1570
  6.10.3p11: "If there are sequences of preprocessing tokens within the list of arguments that
  would otherwise act as preprocessing directives, the behavior is undefined."), and the C
  preprocessor Xonotic runs first defines it: "GNU CPP processes arbitrary directives within
  macro arguments in exactly the same way as it would have processed the directive were the
  function-like macro invocation not present" (the GCC manual, Directives Within Macro
  Arguments, fetched 2026-11-16). FTEQCC runs a directive only in the white space after the `(`
  or a `,` (FTEF-14.45).
- **GMQ-21.5** Under this dialect a `#define`'s parameter list continued across lines by a `\`
  before each line break names its parameters as written across the lines: `#define R(x, y, \`
  then ` z, ...) z` has the parameters `x`, `y` and `z`. The C preprocessor Xonotic runs first
  joins the lines before it reads any token: "Each instance of a backslash character (\)
  immediately followed by a new-line character is deleted, splicing physical source lines to
  form logical source lines." (C11, N1570 5.1.1.2, phase 2). Xonotic's `lib/p99.qh` writes
  `P00_ARG` so.
  FTEQCC keeps the `\`, the line break and the white space after them in the name
  (`QCC_PR_PreProcessor_Define` passes over only `qcc_iswhitesameline` bytes before a
  parameter), so its `z` is never replaced.

## GMQ-22 Arrays of fields

**Source:** `parser.cpp`, the `[` operator in `parser_sy_apply_operator`, `parse_variable` and
`parser_create_array_accessor`, at the commit above. Xonotic: `common/scores.qh` (`.int
_scores[MAX_SCORE];`, `#define scores(this) _scores[(this).m_id]`), used as
`pl.(scores(ps_primary))`.

### Behaviour

- **GMQ-22.1** An element of an array of fields picked inside `e.( )`, `e.(arr[i])`, is the field
  its index names, at a constant index or another. GMQCC's `[` takes a field whose type is an
  array (`exprs[0]->m_vtype == TYPE_FIELD && exprs[0]->m_next->m_vtype == TYPE_ARRAY`), and a
  global array of fields gets accessor functions that give the field (`parse_variable`'s
  `"%s##SETF"`, made by `parser_create_array_accessor`). FTEQCC on a target without
  `OP_GLOBALADDRESS` takes the array's address there, which is the error `E0331` (FTEF-31.6);
  under this dialect it is not. Only an element is picked: the array named alone, `e.(arr)`, is
  its head as under FTEQCC. The model does not follow the body that reads such an element.
- **GMQ-22.2** A constant index past the end of an array of fields picked inside `e.( )` is
  reported as any array's is, the warning `W0249` "(constant) array index out of bounds (0 <= I <
  N)". GMQCC refuses it, `ast_array_index::codegen`'s `if (arridx >= m_array->m_count) {
  compile_error(m_context, "array index out of bounds: %i", arridx); return false; }` (`ast.cpp`
  at the commit above); the model's diagnostics under this dialect are FTEQCC's.
