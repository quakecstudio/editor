# Vanilla QuakeC

Prefix: `QC` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

Describes the QuakeC language as id's compiler `qcc` accepts it, and the progs it emits for
each construct, as implemented by `qcc/pr_lex.c`, `qcc/pr_comp.c`, `qcc/qcc.c`, `qcc/qcc.h`
and `qcc/cmdlib.c` in [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)
at commit `c0d1b91c74eb654365ac7755bc837e497caaca73`. What the engine does with an emitted
statement is cited from `WinQuake/pr_exec.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`; the VM spec owns those rules and this file only
names the ones a language rule depends on. The preprocessor (QC-9) has no id source and is
this project's own definition. Diagnostics and their codes are in
[`language-diagnostics.md`](language-diagnostics.md).

To be implemented by the lexer and preprocessor (P1-20), the parser (P1-21), the semantic
model (P2-02), the dialect profiles (P2-03) and `qc-codegen` (P2-06). Until code cites them,
the statements are in [`coverage-baseline.tsv`](coverage-baseline.tsv).

How to read it:

- Statements describe what id's `qcc` does. Where this project's compiler deliberately does
  something else, the difference is a statement in QC-27, and the differential suite lists it
  as expected.
- "Error" means `PR_ParseError`: the message is printed as `<file>:<line>:<message>` and the
  parser recovers (QC-24). "Fatal" means `Error` in `cmdlib.c`: the compiler exits at once.
- `qcc.h` opens with id's own description of the language, the comment block after its
  `TODO` list. It is quoted where it states a rule. Where it and the code disagree, the code
  wins and the statement says so.

Test vectors were checked on 2026-10-16 against id's `qcc` built from the pinned commit:
`qcc.c`, `pr_lex.c`, `pr_comp.c` and `cmdlib.c` unchanged, compiled by Apple clang 21 for
arm64 macOS, where `char` is signed as on x86, with the POSIX headers the 1996 sources omit
force-included. Each snippet was the only file of a `progs.src`; `qcc -asm <function>` printed
the statements, and a small reader printed the function and definition tables of the output.
The same build compiles id's 1.01 QuakeC to a 410,616-byte `progs.dat` whose generated
`progdefs.h` ends `#define PROGHEADER_CRC 5927`, the value the engine's `progdefs.h` expects.
In the vectors, `\n` inside a snippet is a line break, and statements are written
`OPCODE a, b, c` with operands by name.

## QC-1 Source text

**Source:** `qcc/cmdlib.c`, `LoadFile`; `qcc/pr_lex.c`, `PR_Lex`, `PR_NewLine`,
`PR_LexWhitespace`; `qcc/pr_comp.c`, `PR_Statement`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

A source file is read whole into memory with a NUL byte appended, and the lexer walks it as
C `char`s.

### Behaviour

- **QC-1.1** A source file is a sequence of bytes with no encoding. The text ends at the
  first NUL byte; anything after it is ignored.
- **QC-1.2** Line numbers start at 1 and increase by one at each LF (0x0A) byte. CR (0x0D) is
  whitespace and never starts a line, so CRLF and LF files have the same line numbers.
- **QC-1.3** Bytes 0x80 to 0xFF outside a string literal are whitespace. `PR_LexWhitespace`
  skips every byte whose `char` value is `<= ' '`, and `char` is signed on the x86 build, so
  these bytes are negative.
- **QC-1.4** Bytes 0x80 to 0xFF inside a string literal are copied into the string unchanged.
- **QC-1.5** The line number recorded for an emitted statement is the line of the token the
  lexer holds when the statement is emitted: the token after the last one the statement
  consumed. In `if (a)` followed by `{` on the next line, the `IFNOT` gets the line of `{`.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `float\xa0a;` | compiles; declares `a` | QC-1.3 |
| `string s = "\xe9";` | compiles; the value is the one byte 0xE9 | QC-1.4 |
| lines 1 to 10: `float a;`, `void() f =`, `{`, `if (a)`, `{`, `a = 1;`, `}`, `a = 2`, `;`, `};` | `IFNOT` line 5, `STORE_F 1, a` line 6, `STORE_F 2, a` line 9, `DONE` line 10 | QC-1.5 |

## QC-2 Whitespace and comments

**Source:** `qcc/pr_lex.c`, `PR_LexWhitespace`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "// comments discard text until the end of line", and `/* */` comments "discard all
enclosed text".

### Behaviour

- **QC-2.1** Whitespace is every byte from 0x01 to 0x20, plus the bytes in QC-1.3. It
  separates tokens and is otherwise ignored.
- **QC-2.2** `//` starts a comment that runs to the end of the line. A `//` comment with no
  LF after it ends the file. (id's loop then steps past the terminating NUL and reads beyond
  the buffer; the reference build ends the file there, and this spec fixes that answer.)
- **QC-2.3** `/*` starts a comment that ends at the first `*/` found by a scan that starts at
  the `*` of the `/*`. So `/*/` is a complete comment, and comments do not nest.
- **QC-2.4** A `/*` comment with no closing `*/` stops one byte before the end of the file,
  and that last byte is then lexed as a token. QC-27.2 replaces this with an error.
- **QC-2.5** Line breaks inside a comment count towards line numbers (QC-1.2).
- **QC-2.6** A comment may stand between any two tokens, and between the components of a
  vector literal (QC-5.3). Inside a string literal, `//` and `/*` are text.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `/*/ float x;\nfloat y;\nvoid() f = { y = x; };` | compiles; `f` is `STORE_F x, y` | QC-2.3 |
| `float a;\n/* abc` | `t.qc:2:"c" is not a type` | QC-2.4 |
| `float a;\n// abc` with no final LF | compiles | QC-2.2 |

## QC-3 Names and keywords

**Source:** `qcc/pr_lex.c`, `PR_LexName`, `PR_ParseName`, `PR_Check`, `PR_ParseType`;
`qcc/pr_comp.c`, `PR_ParseStatement`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "Names are a maximum of 64 characters, must begin with A-Z,a-z, or _, and can
continue with those characters or 0-9." The code allows 62 (QC-3.3).

### Behaviour

- **QC-3.1** A name starts with an ASCII letter or `_` and continues with ASCII letters,
  digits and `_`. Case matters.
- **QC-3.2** No name is reserved. The words `float`, `vector`, `string`, `entity`, `void`,
  `local`, `return`, `if`, `else`, `while` and `do` act as keywords only where the parser
  compares the current token with them: types at the start of a declaration or parameter,
  statement keywords at the start of a statement, `else` after an `if` statement and `while`
  after a `do` body. Anywhere else they are ordinary names, so `float do;` declares a global
  `do` and `a = do;` reads it, but a statement `do = 1;` starts a `do` loop.
- **QC-3.3** A name of 63 bytes or more is an error, "name too long" (`PR_ParseName` tests
  `strlen(pr_token) >= MAX_NAME-1`). 62 bytes is accepted. LIMIT-7.2 states the same limit.
- **QC-3.4** `PR_Check` and `PR_Expect` compare the text of the current token whatever its
  kind, and a string literal's text is its contents. So a string literal whose contents are a
  keyword or punctuation is taken as that keyword or punctuation wherever the parser checks
  for one: `"if" (a) b = 1;` compiles as an `if` statement. QC-27.3 diverges.
- **QC-3.5** `for`, `break`, `continue`, `switch`, `case`, `goto`, `int`, `const` and every
  other word not in QC-3.2 have no meaning to vanilla `qcc`; they are names.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QC_MAX_TOKEN` | 2048 | `qcc/pr_lex.c`, `pr_token[2048]` and `pr_immediate_string[2048]` |

The name limit is `LIMIT_QCC_MAX_NAME` in [`limits.md`](limits.md) (LIMIT-7.2).

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `float ` + 62 `n` + `;` | compiles | QC-3.3 |
| `float ` + 63 `n` + `;` | `name too long` | QC-3.3 |
| `float do;\nfloat a;\nvoid() f = { a = do; };` | `STORE_F do, a` | QC-3.2 |
| `float do;\nvoid() f = { do = 1; };` | `not a name` (the `do` loop body starts at `=`) | QC-3.2 |
| `float a, b; void() f = { "if" (a) b = 1; };` | `IFNOT a`, `STORE_F 1, b` | QC-3.4 |

## QC-4 Float literals

**Source:** `qcc/pr_lex.c`, `PR_Lex`, `PR_LexNumber`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "Float immediates must begin with 0-9 or minus sign. .5 is illegal." and "A parsing
ambiguity is present with negative constants. "a-5" will be parsed as "a", then "-5",
causing an error. Seperate the - from the digits with a space "a - 5" to get the proper
behavior."

### Behaviour

- **QC-4.1** A float literal starts with a digit, or with `-` immediately followed by a digit,
  and continues over digits and `.` characters. Its value is C `atof` of that text, rounded
  to a 32-bit float.
- **QC-4.2** A `-` immediately followed by a digit is always the start of a negative literal,
  whatever precedes it. `a-5` is the name `a` then the literal `-5`, which is an error
  ("expected ;, found -5" in a statement); `a - 5` and `a - -5` subtract.
- **QC-4.3** `atof` stops at the second `.`, so `1.2.3` is one token whose value is 1.2.
- **QC-4.4** There are no exponents, hex or suffixes: `1e5` is the literal `1` then the name
  `e5`. A literal cannot start with `.`: `.5` is the `.` operator then `5`.
- **QC-4.5** There is no unary minus operator. `-` before anything but a digit is binary
  subtraction, so `a = -b;` is an error, "not a name"; id 1.01 writes `0 - b`.

### Test vectors

| Input (in a function body) | Result | Statements |
| --- | --- | --- |
| `a = b-5;` | `expected ;, found -5` | QC-4.2 |
| `a = b - 5;` | `SUB_F b, 5, t`; `STORE_F t, a` | QC-4.2 |
| `a = b - -5;` | `SUB_F b, -5, t` | QC-4.2 |
| `a = 1.2.3;` | `STORE_F 1.2, a` | QC-4.3 |
| `a = 1e5;` | `expected ;, found e5` | QC-4.4 |
| `a = .5;` | `not a name` | QC-4.4 |
| `a = -b;` | `not a name` | QC-4.5 |

## QC-5 Vector literals

**Source:** `qcc/pr_lex.c`, `PR_LexVector`, `PR_LexNumber`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "Vector immediates are three float immediates enclosed in single quotes."

### Behaviour

- **QC-5.1** A vector literal is `'`, three numbers, `'`. Each number is read as in QC-4.1,
  except that its first byte is taken whatever it is, so each component may be negative.
- **QC-5.2** After each component the lexer skips whitespace and comments. If the byte after
  the third component is not `'`, it is an error, "Bad vector".
- **QC-5.3** Whitespace (including line breaks) and comments may appear between the
  components and before the closing quote, and count towards line numbers.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `vector v = '-1 -2.5 3';` | `v` initialised to (-1, -2.5, 3) | QC-5.1 |
| `vector v = '1 2 3 4';` | `Bad vector` | QC-5.2 |

## QC-6 String literals

**Source:** `qcc/pr_lex.c`, `PR_LexString`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "The string cannot contain explicit newlines, but the escape character \n can embed
one. The \" escape can be used to include a quote in the string."

### Behaviour

- **QC-6.1** A string literal is `"`, its contents, `"`. There is no concatenation of
  adjacent literals.
- **QC-6.2** `\n` in the contents is one LF byte and `\"` is one `"` byte. A backslash
  followed by anything else, including a second backslash, is an error, "Unknown escape
  char".
- **QC-6.3** A line break inside the literal is an error, "newline inside quote"; the end of
  the file inside it, or after its backslash, is an error, "EOF inside quote".
- **QC-6.4** The contents are at most `QC_MAX_TOKEN - 1` bytes. id's lexer does not check and
  overruns `pr_token`; QC-27.4 makes a longer literal an error.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `s = "a\"b";` | `STORE_S "a\"b", s` (the value is `a"b`) | QC-6.2 |
| `string s = "a\\b";` | `Unknown escape char` | QC-6.2 |
| `string s = "a` LF `b";` | `newline inside quote` | QC-6.3 |
| `string s = "ab` at the end of the file | `EOF inside quote` | QC-6.3 |

## QC-7 Punctuation and operators

**Source:** `qcc/pr_lex.c`, `pr_punctuation`, `PR_LexPunctuation`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QC-7.1** The punctuation tokens are, in the order the lexer tries them:
  `&&` `||` `<=` `>=` `==` `!=` `;` `,` `!` `*` `/` `(` `)` `-` `+` `=` `[` `]` `{` `}` `...`
  `.` `<` `>` `#` `&` `|`. The first one that matches the input wins, which is the longest
  match for every prefix pair in the list.
- **QC-7.2** Any other byte that does not start a token (for example `%`, `^`, `~`, `?`, `:`,
  `@`, `\` or a backquote) is an error, "Unknown punctuation". The lexer does not advance past
  it, so recovery (QC-24.3) meets it again until the error limit stops the file.
- **QC-7.3** `{` and `}` also adjust a brace depth that error recovery uses (QC-24.3).
- **QC-7.4** There are no compound assignment, increment or decrement operators, no `%`,
  `^`, `~`, `?:`, shifts or comma operator. `qcc.h`: "There are no ++ / -- operators, or
  operate/assign operators."

## QC-8 Frame macros and `$` commands

**Source:** `qcc/pr_lex.c`, `PR_LexGrab`, `PR_ParseFrame`, `PR_FindMacro`,
`PR_SimpleGetToken`, `PR_ClearGrabMacros`; `qcc/pr_comp.c`, `PR_CompileFile`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

Model source files share commands with id's `modelgen` (`$cd`, `$base` and so on); `qcc`
reads only `$frame` and skips the rest.

### Behaviour

- **QC-8.1** `$` followed by a word starts a `$` command or a frame macro reference. The word
  runs from the first byte after `$` and any blanks to the next byte that is whitespace, `,`
  or `;`. `$` followed only by blanks before the end of the line is an error, "hanging $".
- **QC-8.2** `$frame` defines frame macros: each word after it on the same line, split at
  whitespace, is appended to the file's frame list. The first frame defined in a file is 0,
  the next 1, and so on, across all `$frame` lines in the file.
- **QC-8.3** The frame list is cleared at the start of each source file, so a frame macro is
  visible only in the file that defines it, after its `$frame` line.
- **QC-8.4** `$cd`, `$origin`, `$base`, `$flags`, `$scale` and `$skin` are ignored with the
  rest of their line.
- **QC-8.5** Any other `$word` is a frame macro reference. It becomes a float literal whose
  value is the index of the first frame in the list whose name equals the word; it can stand
  wherever a float literal can, including a global initialiser and a state function's frame
  (QC-13.6). A word not in the list is an error, "Unknown frame macro $word".
- **QC-8.6** Because the word ends only at whitespace, `,` or `;`, a reference followed
  directly by another character includes it: `($b)` looks up `b)` and fails.
- **QC-8.7** A frame name defined twice keeps its first index; the second definition still
  takes the next index, which no reference can reach.
- **QC-8.8** `$modelname`, `$framegroupstart`, `$framegroupend`, `$load`, `$spritename` and
  `$type` (modelgen and spritegen commands that id 1.01's `models.qc` and `sprites.qc` use)
  are not `qcc` commands: they are references to undefined frame macros and fail as in
  QC-8.5. Those two files are not in id 1.01's `progs.src`. `$modelname` as FTEQCC reads it
  is part of the FTEQCC dialect spec (P2-01).
- **QC-8.9** A `,` or `;` on a `$frame` line makes id's `qcc` loop forever appending empty
  names past the end of the frame table, and a frame name of 16 bytes or more overruns its
  slot into the next. QC-27.5 and QC-27.6 define what this compiler does.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QC_MAX_FRAME_NAME` | 15 | `qcc/pr_lex.c`, `pr_framemacros[MAX_FRAMES][16]`: 16 bytes with the NUL |

The frame count limit is `LIMIT_QCC_MAX_FRAMES` in [`limits.md`](limits.md).

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `$frame a b a\nfloat x;\nvoid() f = { x = $a; x = $b; };` | `STORE_F 0, x`; `STORE_F 1, x` | QC-8.2, QC-8.7 |
| `$frame a b\n$frame c\n...x = $c;` | `STORE_F 2, x` | QC-8.2 |
| `$frame a b\nfloat x;\nvoid() f = { x = ($b); };` | `Unknown frame macro $b)` | QC-8.6 |
| `$cd /a/b/c`, `$origin 0 0 24`, `$base base`, `$skin skin`, `$flags 8`, `$scale 1` on separate lines, then `float a;` | compiles | QC-8.4 |
| `float a;\n$modelname foo\nfloat b;` | `Unknown frame macro $modelname` | QC-8.8 |
| `float a = $` then a line break | `hanging $` | QC-8.1 |
| `$frame a b\nfloat x = $b;` | `x` initialised to 1 | QC-8.5 |
| `$frame a, b` | `qcc` did not finish within 5 seconds | QC-8.9 |

## QC-9 Preprocessor

**Source:** none in id's code: `qcc` has no preprocessor, and `#` is only the builtin marker
(QC-13.3). The directive names follow FTEQCC's, from
`QCC_PR_Precompiler` in `engine/qclib/qcc_pr_lex.c` of
[fte-team/fteqw](https://github.com/fte-team/fteqw) at commit
`f937b9d88f71fc4429db5fe56c6a98d922711b2e`, read on 2026-10-16. FTEQCC is not a registered
upstream (`docs/porting.md`), so nothing here is ported from it; these statements are this
project's own rules. Pragmas, `#if` expressions and the rest of FTEQCC's directives are
dialect features (P2-01, P2-03).

### Behaviour

- **QC-9.1** A directive is a line, outside any comment, whose first non-blank byte is `#`
  followed, after optional blanks, by a name. A `#` anywhere else, or followed by anything
  but a name, is the `#` token. No program id's `qcc` accepts has a line that starts with `#`
  and a name, because a vanilla `#` is always followed by a number (QC-13.3), so the
  preprocessor changes nothing for vanilla code.
- **QC-9.2** `#include "path"` replaces the directive with the contents of the named file,
  resolved relative to the directory of the file containing the directive. Diagnostics and
  statements in the included text carry the included file's name and lines.
- **QC-9.3** `#define NAME` and `#define NAME text` define an object-like macro whose body is
  the rest of the line, trimmed. `#define NAME(a, b) text`, with the `(` directly after the
  name, defines a function-like macro. A later identical `#define` of the same name is
  accepted; a different one replaces the old one with a warning.
- **QC-9.4** After its definition, a name token that matches a macro is replaced by its body,
  and a function-like macro's parameters in the body are replaced by the comma-separated
  arguments of the use. The replacement is rescanned for further macros, except the macro
  being expanded. Names inside string literals and comments are never replaced.
- **QC-9.5** `#undef NAME` removes a macro; removing one that is not defined does nothing.
- **QC-9.6** `#ifdef NAME` and `#ifndef NAME` start a conditional block that is kept when the
  name is (or is not) a defined macro. `#else` flips it and `#endif` closes it. Blocks nest;
  directives inside a skipped block are only counted for nesting. A `#else` or `#endif`
  without an open block, or a block still open at the end of the file, is an error.
- **QC-9.7** Every token produced by expansion keeps the span of the macro use it came from,
  and the span of its text in the macro definition, so a diagnostic can point at both
  (LANG-1).
- **QC-9.8** Macros and conditional state run on across the files of one `progs.src`, in
  compilation order (QC-23.3); frame macros do not (QC-8.3).
- **QC-9.9** A directive name other than `include`, `define`, `undef`, `ifdef`, `ifndef`,
  `else` and `endif` is an error outside a skipped block, and so is a directive whose operand
  is missing or malformed: `#include` without a string literal, `#define`, `#undef`,
  `#ifdef` or `#ifndef` without a name, or a parameter list that is not names separated by
  `,` and closed by `)`. The directive line is then dropped. Tokens after the operand of
  `#include`, `#undef`, `#ifdef`, `#ifndef`, `#else` and `#endif` are ignored.
- **QC-9.10** A function-like macro name not followed by `(` is an ordinary name and is not
  replaced. A use whose argument count differs from the parameter count, or whose argument
  list is not closed by `)` before the end of the file, is an error; the use is left
  unreplaced. Arguments are split at commas outside parentheses, and `F()` passes no
  arguments.
- **QC-9.11** An `#include` of a file that cannot be read, or of a file that is already being
  included (directly or through other includes), is an error, and the directive is dropped.
- **QC-9.12** A conditional block is closed in the file that opens it, whether that file is
  listed in `progs.src` or included. At the end of each file, every block it opened that is
  still open is reported (QC-9.6) and closed, so the next file starts outside it.
- **QC-9.13** Expansion nests to any depth, with no limit and no diagnostic: a chain of macros,
  each defined as the name of the one before (`#define M1 M0`, `#define M2 M1`, ...), is
  rescanned to its end however long it is, and no chain exhausts the stack. FTEQCC sets no
  nesting limit: `QCC_PR_CheckCompConst` (`qcc_pr_lex.c`, fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`)
  expands a macro unless `currentchunk->cnst != c` fails (`//macros don't expand themselves`)
  or, for an object-like macro, `if (c->inside >= 8)`, which counts only that macro's own
  open expansions; each expansion is pushed by `QCC_PR_IncludeChunkEx` as a heap-allocated
  chunk (`qcc_includechunk_t *chunk = qccHunkAlloc(sizeof(qcc_includechunk_t)); chunk->prev = currentchunk;`),
  not a C stack frame. But `QCC_PR_Lex` calls itself once for each expansion (`//we expanded
  a macro. we need to read the tokens out of it now though`, then `QCC_PR_Lex();`), so a long
  enough chain exhausts FTEQCC's own stack: the pinned FTEQCC, built by `cargo xtask oracles
  build` on macOS arm64 with its 8 MiB main stack, compiles `float x = M49999;` after a
  50000-long chain to `x = 1` and dies with `SIGSEGV` at 100000. Expanding every chain to its
  end is a deliberate divergence from that crash, which depends on the host's stack, not on
  the program. id's `qcc` has no macros (QC-9). The rule against self-expansion stays QC-9.4's.
- **QC-9.14** **Withdrawn** (P2-77, #482): it said an argument written against a body token
  stays apart from it under FTEQCC too. Replaced by QC-9.15.
- **QC-9.15** An expansion keeps the spacing of the text it came from: two tokens that touch in a
  macro's body, or in one argument of it, touch in the expansion, and two that do not, or that
  come from different macros or a macro and the text around its use, are apart. A rule that reads
  whether two tokens touch then reads a macro's tokens as it reads the source: under FTEQCC
  `#define B #5` then `void() f = B;` is the builtin 5 (FTE-23.1), and `#define L lbl:` used as a
  statement is a label (FTEF-18.23). A token whose text the preprocessor made (a `##` paste, a `#`
  string, a built-in constant, FTEF-14.6, FTEF-14.7) touches nothing. In the vanilla dialect an
  argument's tokens are apart from the body's. FTEQCC reads an expansion as text (`qcc_pr_lex.c`,
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`): `QCC_PR_IncludeChunkEx` lexes the
  body as `QCC_PR_PreProcessor_Define` stored it, which drops each `/* */` comment ("multi-line c
  style comments become part of the define itself", skipped to the `*/` and not copied) and each
  `\` before a line break, so tokens with only those between them touch under FTEQCC: `#define L
  lbl/**/:` is a label. `QCC_PR_ExpandPreProcessorMacro` substitutes an argument into the body as
  text: it copies the body's white space (`QCC_PR_LexWhitespace(true)` then `QCC_PR_ExpandStrCat`)
  and, for a parameter, the argument's tokens each with what stands before it, white space and
  comments before its first token dropped (`QCC_PR_LexWhitespace(false)` after the `(` and each
  `,`) and white space and `//` comments after its last too (the copy loop stops when
  `QCC_COM_Parse2`, which skips those but not a `/* */` comment, finds no token), and the variadic
  argument whole, anything before its `)` kept (`QCC_PR_ExpandStrCat(...,
  paramoffset[c->numparams], strlen(...))`). So under FTEQCC an argument's first token touches the
  body token before the parameter when the parameter touches it, an argument's last token touches
  the body token after the parameter when the parameter touches that and nothing the copy keeps
  follows the argument, and an empty argument leaves the body tokens either side touching when
  both touch the parameter: `#define L(x) x:` used as `L(lbl)`, `L( lbl)` or `L(lbl )` is the
  label `lbl:` and as `L(lbl/**/)` is not, `#define V(...) __VA_ARGS__:` used as `V(lbl)` is a
  label and as `V(lbl )` is not, `#define L(x,y) x y:` used as `L(,lbl)` is a label and as
  `L(lbl,)` is not, and the pinned FTEQCC compiles each so. A token touches only the one written
  right before it, so tokens that an expansion to nothing came to stand between stay apart: with
  `#define E`, `#define K lbl E:` used as a statement is no label, as FTEQCC reads `lbl`, then `E`
  and the `:` after a space. A parameter that under FTEQCC touches a word across what
  `QCC_PR_PreProcessor_Define` drops is part of that word as stored, which `QCC_COM_Parse2` reads
  as one token, and no parameter: `#define A(a) a/**/b` is the name `ab` (the pinned FTEQCC:
  "Unknown value \"ab\""), while a number before a parameter ends at it.

## QC-10 Types

**Source:** `qcc/pr_lex.c`, `PR_ParseType`, `PR_FindType`, `type_size`;
`qcc/pr_comp.h`, `etype_t` (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "simple types: void, float, vector, string, or entity", "A function type is
specified as: simpletype ( type name {,type name} )", "A field type is specified as:
.type", and "The language is strongly typed and there are no casts."

### Behaviour

- **QC-10.1** The simple types are `void`, `float`, `vector`, `string` and `entity`. A
  `vector` is three floats; every other type is one 32-bit word.
- **QC-10.2** `.T` is the type of a field holding a `T`, for any type `T`, including a
  function type and another field type (`..float` is accepted).
- **QC-10.3** `T(T1 n1, T2 n2, ...)` is the type of a function returning the simple type `T`
  with those parameters; `T()` takes none. A parameter's type may itself be a field or
  function type. The parameter names are part of the declaration, not the type (QC-13.2).
- **QC-10.4** `T(...)` is a function taking any number of arguments of any types. `...` must
  be the whole parameter list: `void(float a, ...)` is an error, `"..." is not a type`.
- **QC-10.5** A function type cannot return a function: after `T(...)` the type ends, so
  `void()()` is not a type.
- **QC-10.6** Two function or field types are the same type when they have the same return
  or field type, the same parameter count and the same parameter types; parameter names do
  not matter (`PR_FindType` interns them).
- **QC-10.7** A variable of type `void` may be declared; it occupies one word.
- **QC-10.8** A function type with more than eight parameters is accepted by id's `qcc`,
  which writes past `parm_types[8]`; QC-27.7 makes it an error.
- **QC-10.9** There is no `int`, array, struct, pointer or `const` type in the source
  language. The compiler uses a pointer type internally for field stores (QC-17.3).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QC_TYPE_SIZE` | `{void 1, string 1, float 1, vector 3, entity 1, field 1, function 1, pointer 1}` words | `qcc/pr_lex.c`, `type_size` |

The parameter limit is `LIMIT_MAX_PARMS` in [`limits.md`](limits.md); `qcc/qcc.h` has the
same `MAX_PARMS`.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `void(float a, ...) f;` | `"..." is not a type` | QC-10.4 |
| `..float ff;` | compiles | QC-10.2 |
| `void x;` | compiles | QC-10.7 |
| `void(float a, ... nine float parameters) z;` | no error about the parameters | QC-10.8 |

## QC-11 Declarations

**Source:** `qcc/pr_comp.c`, `PR_ParseDefs`, `PR_GetDef`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "A definition is: <type> <name> [ = <immediate>] {, <name> [ = <immediate>] };" and
"Names can be defined multiple times until they are defined with an initialization, allowing
functions to be prototyped before their definition."

### Behaviour

- **QC-11.1** A file is a sequence of declarations: a type, then one or more names separated
  by `,`, each optionally followed by `=` and an initialiser, then `;`. At file level there
  is nothing else: no statements and no stray `;`.
- **QC-11.2** A declaration of a function type with an initialiser defines the function
  (QC-13). A function definition in a list is followed by `,` or `;` like any other name, so
  `void() a = {}, b = {};` defines two functions, and the `;` after a function body is
  required. `qcc.h`: "Don't forget the ; after the trailing brace of a function
  initialization."
- **QC-11.3** Any other initialiser must be a single literal (or frame macro reference)
  whose type is exactly the declared type: a float, vector or string literal for `float`,
  `vector` or `string`. No initialiser is valid for `entity`, `void` or a field type; those
  are errors, "wrong immediate type for NAME".
- **QC-11.4** id's `qcc` checks the initialiser's type against the type of the last literal
  the lexer read, without checking that the current token is a literal. So `float x = y;`
  after any earlier float literal is accepted and initialises `x` with that earlier literal's
  value, and the name `y` is consumed. QC-27.8 makes a non-literal initialiser an error.
- **QC-11.5** An initialised declaration marks the definition initialised. A declaration of a
  name that already exists in scope with the same type refers to the same definition, so a
  name may be declared any number of times and initialised once. Declaring it with a
  different type is an error, "Type mismatch on redeclaration of NAME"; initialising it a
  second time is an error, "NAME redeclared".
- **QC-11.6** An initialised global is a constant: its value is stored in the global area of
  the progs, it is not saved in saved games (QC-21.6), and literals of the same value reuse
  it (QC-20.2). Nothing stops code from assigning to it. `qcc.h`: "Anything that is
  initialized is assumed to be constant, and will have immediates folded into it. If you
  change the value, your program will malfunction."
- **QC-11.7** Declaring a `vector` named `v` also declares `float` definitions `v_x`, `v_y`
  and `v_z` at its three words; declaring a field `.vector f` also declares `.float` fields
  `f_x`, `f_y` and `f_z` at its three field offsets. These are ordinary names (QC-15.7).
- **QC-11.8** Every global function must be defined somewhere in the program; at the end of
  compilation each one that is not is reported, "function NAME was not defined", and
  compilation fails. A global of function type is therefore always a function, never a
  variable.
- **QC-11.9** Fields and functions can only be declared at file level: `local .float f;` and
  `local void() g;` are errors, "Fields and functions must be global". Parameters of function
  type are allowed.
- **QC-11.10** `local` declarations inside a function body have the same grammar as QC-11.1
  and QC-11.3. A local initialiser is stored once in the progs' global area, like a global
  constant; it is not executed on each call, and later literals of the same value reuse the
  local (QC-20.2).

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `void() a = {}, b = {};\nvoid() f = { a(); b(); };` | `CALL0 a`; `CALL0 b` | QC-11.2 |
| `void() f = {}\nfloat a;` | `expected ;, found float` | QC-11.2 |
| `.float fl = 1;` | `wrong immediate type for fl` | QC-11.3 |
| `entity e = 1;` | `wrong immediate type for e` | QC-11.3 |
| `float k = 7;\nfloat y;\nfloat x = y;\nvoid() f = { y = x; };` | compiles; `x` holds 7 | QC-11.4 |
| `float a;\nvector a;` | `Type mismatch on redeclaration of a` | QC-11.5 |
| `float a = 1;\nfloat a = 2;` | `a redeclared` | QC-11.5 |
| `void() f;\nvoid() f = {};\nvoid() f;` | compiles | QC-11.5 |
| `void() f;` | `function f was not defined`, then fatal `compilation errors` | QC-11.8 |
| `void() f = { local .float g; };` | `Fields and functions must be global` | QC-11.9 |
| `float a;\nvoid() f = { local float l = 5; a = 5; };` | `STORE_F l, a`: the literal 5 is the local `l` | QC-11.10 |

## QC-12 Scopes and name lookup

**Source:** `qcc/pr_comp.c`, `PR_GetDef`, `PR_ParseValue`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "There are two levels of scoping: global, and function."

### Behaviour

- **QC-12.1** There are two scopes: global, and the function being defined. Blocks do not
  open scopes; a `local` anywhere in a function body is visible from its declaration to the
  end of the function.
- **QC-12.2** All definitions, global and local, are kept in one list in declaration order.
  A name is looked up by walking that list from the start and taking the first definition
  with that name that is global or belongs to the current function.
- **QC-12.3** A local or parameter cannot shadow a global declared before it: the lookup in
  `local float x;` or in the parameter `x` finds the global `x` first and reuses it (or
  reports a type mismatch if the types differ). Both then read and write the global.
- **QC-12.4** A local of one function is invisible to every other function, so two functions
  may declare locals with the same name, and a global declared later may take a name that an
  earlier function used for a local.
- **QC-12.5** Names are resolved when they are parsed: a function body sees only the
  definitions made before it in compilation order (QC-23.3). Using a name before any
  declaration is an error, "Unknown value \"NAME\"". `qcc.h`: "Source files are processed
  sequentially without dumping any state, so if a defs file is the first one processed, the
  definitions will be available to all other files."
- **QC-12.6** The next-think name in a state function header is declared on the spot as a
  global `void()` when it does not exist yet (QC-13.6).

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `float x;\nvoid() f = { local float x; x = 1; };` | `STORE_F 1, x` with `x` the global at offset 28 | QC-12.3 |
| `float x;\nvoid(float x) f = { x = 1; };` | `STORE_F 1, x` with `x` the global at offset 28 | QC-12.3 |
| `void() f = { zz = 1; };` | `Unknown value "zz"` | QC-12.5 |

## QC-13 Function definitions

**Source:** `qcc/pr_comp.c`, `PR_ParseDefs`, `PR_ParseImmediateStatements`, `PR_ParseState`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`). The engine's `OP_STATE`
is `WinQuake/pr_exec.c`, `PR_ExecuteProgram`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`qcc.h`: "A built in function immediate is a number sign followed by an integer." The state
syntax is documented in `pr_comp.c` above `PR_ParseState`: "void() name = [framenum,
nextthink] {code}".

### Behaviour

- **QC-13.1** A function is defined by `= body`, `= [frame, next] body` or `= #number` on a
  declaration of function type. A body is `{`, zero or more statements, `}`; an empty body is
  allowed.
- **QC-13.2** The parameter names used inside the body are the ones written in the type of
  the defining declaration. Names in prototypes are ignored. `qcc.h`: "The names are ignored
  except when the function is initialized."
- **QC-13.3** `#n` makes the function the engine builtin number `n`: the function record's
  `first_statement` is `-n` and no statements are emitted. `n` must be a float literal with
  an integral value, else "Bad builtin immediate"; `#1.5` is rejected. Builtins allocate no
  parameter or local storage.
- **QC-13.4** `#0` and negative numbers are accepted. `#0` leaves `first_statement`
  uninitialised in id's `qcc` (the reference build wrote 0), and `#-1` writes
  `first_statement` 1, a call to statement 1. QC-27.9 makes both errors.
- **QC-13.5** Parameters are allocated first, in order, at consecutive global offsets; then
  the body's locals and temporaries as they are met (QC-21.4). If a parameter resolves to a
  global (QC-12.3) at a lower offset than the previous parameter, `qcc` stops with the fatal
  error "bad parm order".
- **QC-13.6** In `= [frame, next] body`, `frame` must be a float literal or frame macro
  reference, else "state frame must be a number", and `next` a name, which is declared as a
  global `void()` if it does not exist and must have that type if it does ("Type mismatch on
  redeclaration of NAME"). The first statement of the function is then `STATE frame, next`.
  When it runs, the engine sets `self.nextthink` to `time + 0.1`, `self.frame` to `frame` and
  `self.think` to `next` (`OP_STATE` in `pr_exec.c`).
- **QC-13.7** A body ends with a `DONE` statement after its last statement, so falling off
  the end returns.
- **QC-13.8** Neither the return type nor the presence of a return value is checked:
  `return "x";` in a `float()` and `return;` in a `float()` both compile (`qcc.h` lists
  "return type checking" under TODO).
- **QC-13.9** A function record holds `first_statement`, `parm_start` (the offset of its
  first parameter), `locals` (QC-21.4), the name and file as string offsets, `numparms` and
  each parameter's size in words. A variadic function's `numparms` is -1.
- **QC-13.10** The builtin number is the literal's 32-bit float value (QC-4.1) converted to
  `int`, so above 2^24 it is the float's, not the digits': `PR_ParseImmediateStatements`
  (`qcc/pr_comp.c`, id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`) reads
  `if (pr_token_type != tt_immediate || pr_immediate_type != &type_float || pr_immediate._float != (int)pr_immediate._float) PR_ParseError ("Bad builtin immediate"); f->builtin = (int)pr_immediate._float;`,
  where `PR_LexNumber` gives `return atof (pr_token);`. `#16777217` is builtin 16777216 and
  `#123456789` is 123456792. A value outside -2^31 to 2^31 - 1 is "Bad builtin immediate":
  C leaves its conversion to `int` undefined, and the x86 build gives -2147483648 (VM-9.2),
  which differs from the value. id's `qcc` built for arm64 by `cargo xtask oracles build`
  saturates instead and binds `#2147483648` as 2147483647; the x86 result is the rule.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `void() f = {};` | `DONE` | QC-13.1, QC-13.7 |
| `void() z = #0;\nvoid() n = #-1;\nvoid() o = #7;` | `first_statement` 0, 1 and -7 | QC-13.3, QC-13.4 |
| `void() f = #1.5;` | `Bad builtin immediate` | QC-13.3 |
| `void() f = #16777217;`, `void() g = #123456789;` | builtins 16777216 and 123456792 | QC-13.10 |
| `void() f = #2147483648;` | `Bad builtin immediate` | QC-13.10 |
| `void() th2;\n$frame s1 s2\nvoid() th1 = [$s2, th2] { };` (and `th2` defined) | `STATE 1, th2`; `DONE` | QC-13.6 |
| `$frame s1\nvoid() th1 = [$s1, th3] { };` | `function th3 was not defined` | QC-13.6, QC-11.8 |
| `float th3;\n$frame s1\nvoid() th1 = [$s1, th3] { };` | `Type mismatch on redeclaration of th3` | QC-13.6 |
| `float() f = { return "x"; };` | `RETURN "x"` | QC-13.8 |
| `float() f = { return; };` | `RETURN`; `DONE` | QC-13.8 |
| `void(float p, vector q) f = { local float l; local vector lv; a = p; };` | `numparms` 2, sizes 1 and 3, `locals` 8 | QC-13.9 |

## QC-14 Statements

**Source:** `qcc/pr_comp.c`, `PR_ParseStatement`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h` lists the statements: "{ <multiple statements> }", "<expression>;", "local <type>
<name> [ = <immediate>] {, <name> [ = <immediate>] };", "return <expression>;", "if (
<expression> ) <statement> [ else <statement> ];", "while ( <expression> ) <statement>;",
"do <statement> while ( <expression> );".

### Behaviour

- **QC-14.1** A statement is a block, `return`, `while`, `do`, `local`, `if` or an expression
  followed by `;`, chosen by the first token (QC-3.2).
- **QC-14.2** A block `{ ... }` inside a function holds one or more statements. `{}` inside a
  body is an error ("not a name"), as is a lone `;`: there is no empty statement.
- **QC-14.3** `return;` emits `RETURN` with no operand; `return e;` emits `RETURN e`.
- **QC-14.4** `if (e) s` emits `IFNOT e, +n` jumping past `s`. `if (e) s1 else s2` emits
  `IFNOT e` jumping to `s2`, and a `GOTO` after `s1` jumping past `s2`. `else` binds to the
  nearest `if`.
- **QC-14.5** `while (e) s` emits the condition, `IFNOT` past the loop, `s`, then a `GOTO`
  back to the first statement of the condition.
- **QC-14.6** `do s while (e);` emits `s`, the condition, then `IF e` back to the first
  statement of `s`. The `;` is required.
- **QC-14.7** The parentheses around an `if`, `while` or `do` condition are required.
- **QC-14.8** There is no `for`, `switch`, `break`, `continue` or `goto`.
- **QC-14.9** An expression statement may be any expression; one with no effect, such as
  `a;`, is accepted and its statements are emitted.

### Test vectors

| Input (in a function body) | Statements emitted | Statements |
| --- | --- | --- |
| `if (a) b = 1; else b = 2;` | `IFNOT a, +3`; `STORE_F 1, b`; `GOTO +2`; `STORE_F 2, b` | QC-14.4 |
| `if (a) if (b) c = 1; else c = 2;` | `IFNOT a, +5`; `IFNOT b, +3`; `STORE_F 1, c`; `GOTO +2`; `STORE_F 2, c` | QC-14.4 |
| `while (a) a = a - 1;` | `IFNOT a, +4`; `SUB_F a, 1, t`; `STORE_F t, a`; `GOTO -3` | QC-14.5 |
| `do a = a + 1; while (a < 3);` | `ADD_F a, 1, t`; `STORE_F t, a`; `LT a, 3, u`; `IF u, -3` | QC-14.6 |
| `;` | `not a name` | QC-14.2 |
| `if (a) {}` | `not a name` | QC-14.2 |
| `while a a = 1;` | `expected (, found a` | QC-14.7 |
| `do a = 1; while (a) }` | `expected ;, found }` | QC-14.6 |

## QC-15 Expressions and precedence

**Source:** `qcc/pr_comp.c`, `pr_opcodes`, `PR_Expression`, `PR_Term`, `PR_ParseValue`,
`TOP_PRIORITY`, `NOT_PRIORITY`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h` says the operators have "standard C precedence". They do not; the table in
`pr_opcodes` decides, and this section follows it.

### Behaviour

- **QC-15.1** Binary operators have the priorities and associativity in the table below,
  tightest first.

| Priority | Operators | Associativity |
| --- | --- | --- |
| 1 | `.` (field access), `(` (call) | left |
| 2 | `*` `/` `&` `\|` | left |
| 3 | `+` `-` | left |
| 4 | `==` `!=` `<=` `>=` `<` `>` | left |
| 5 | `=` | right |
| 6 | `&&` `\|\|` | left |

- **QC-15.2** `&` and `|` bind as tightly as `*` and `/`, so `b & c | 4` is `(b & c) | 4` and
  `a + b & c` is `a + (b & c)`.
- **QC-15.3** `=` binds tighter than `&&` and `||`: `a = b && c` is `(a = b) && c`, and
  `a && b = c` is `a && (b = c)`.
- **QC-15.4** `&&` and `||` have the same priority and associate left: `a || b && c` is
  `(a || b) && c`.
- **QC-15.5** Comparisons associate left: `a == b == c` is `(a == b) == c`.
- **QC-15.6** `!` is the only prefix operator. Its operand is an expression at priority 4,
  so `!a == b` is `!(a == b)` and `!a + b` is `!(a + b)`, while `!a && b` is `(!a) && b`.
- **QC-15.7** A term is `!` expression, `(` expression `)`, a literal, a frame macro
  reference or a name. `v_x`, `v_y` and `v_z` are names (QC-11.7), not an operator.
- **QC-15.8** The left operand of a call is the term or field access before `(`. A call
  ends the priority-1 loop, so neither `g().fl` nor `g()()` parses: `expected ;, found .`.
  Field access chains: `e.en.hp` is `(e.en).hp`.
- **QC-15.9** Operands are evaluated left to right, and each operator's statement is emitted
  after both operands' statements; every operator result goes into a new temporary (QC-21.5).
- **QC-15.10** `&&` and `||` evaluate both operands. `qcc.h`: "The && and || operators DO
  NOT EARLY OUT like C!"

### Test vectors

| Input (in a function body) | Statements emitted | Statements |
| --- | --- | --- |
| `a = b + c * a - b / c;` | `MUL_F c, a, t1`; `ADD_F b, t1, t2`; `DIV b, c, t3`; `SUB_F t2, t3, t4`; `STORE_F t4, a` | QC-15.1, QC-15.9 |
| `a = b & c \| 4;` | `BITAND b, c, t1`; `BITOR t1, 4, t2`; `STORE_F t2, a` | QC-15.2 |
| `a = b && c;` | `STORE_F b, a`; `AND b, c, t` | QC-15.3 |
| `a && b = c;` | `STORE_F c, b`; `AND a, c, t` | QC-15.3 |
| `a = a == b == c;` | `EQ_F a, b, t1`; `EQ_F t1, c, t2`; `STORE_F t2, a` | QC-15.5 |
| `a = !b == c;` | `EQ_F b, c, t1`; `NOT_F t1, t2`; `STORE_F t2, a` | QC-15.6 |
| `a = !b + c;` | `ADD_F b, c, t1`; `NOT_F t1, t2`; `STORE_F t2, a` | QC-15.6 |
| `a = g().fl;` | `expected ;, found .` | QC-15.8 |
| `a = e.en.hp;` | `INDIRECT e, en, t1`; `INDIRECT t1, hp, t2`; `STORE_F t2, a` | QC-15.8 |

## QC-16 Operator types

**Source:** `qcc/pr_comp.c`, `pr_opcodes`, `PR_Expression`, `PR_Term`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`); `WinQuake/pr_exec.c`,
`PR_ExecuteProgram` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

A binary operator picks the first row of `pr_opcodes` with its spelling whose operand types
equal the operands' types exactly; if none does, it is an error, "type mismatch for OP".

### Behaviour

- **QC-16.1** The binary operators accept exactly the operand types in the table below and
  give its result types, emitting the named opcode.

| Operator | Left | Right | Result | Opcode |
| --- | --- | --- | --- | --- |
| `*` | float | float | float | `MUL_F` |
| `*` | vector | vector | float (dot product) | `MUL_V` |
| `*` | float | vector | vector | `MUL_FV` |
| `*` | vector | float | vector | `MUL_VF` |
| `/` | float | float | float | `DIV_F` |
| `+` | float, vector | the same | the same | `ADD_F`, `ADD_V` |
| `-` | float, vector | the same | the same | `SUB_F`, `SUB_V` |
| `==` | float, vector, string, entity, function | the same | float | `EQ_F`, `EQ_V`, `EQ_S`, `EQ_E`, `EQ_FNC` |
| `!=` | float, vector, string, entity, function | the same | float | `NE_F`, `NE_V`, `NE_S`, `NE_E`, `NE_FNC` |
| `<=` `>=` `<` `>` | float | float | float | `LE`, `GE`, `LT`, `GT` |
| `&&` `\|\|` | float | float | float | `AND`, `OR` |
| `&` `\|` | float | float | float | `BITAND`, `BITOR` |
| `.` | entity | field of T | T | `LOAD_F`, `LOAD_V`, `LOAD_S`, `LOAD_ENT`, `LOAD_FLD`, `LOAD_FNC` |

- **QC-16.2** There is no `vector / float`, no ordering of vectors, strings or entities, no
  equality of fields, and no `&&`, `||`, `&` or `|` on anything but floats: `if (e && a)`
  with an entity `e` is an error, "type mismatch for &&".
- **QC-16.3** "The same" means the same type: two function operands of `==` must have the
  same function type (QC-10.6).
- **QC-16.4** `!` accepts float, vector, string, entity and function operands and emits
  `NOT_F`, `NOT_V`, `NOT_S`, `NOT_ENT` or `NOT_FNC`; a field operand is an error, "type
  mismatch for !". The result is a float.
- **QC-16.5** What the engine computes: `&` and `|` convert both floats to `int`, combine
  them and convert back (`OP_BITAND`, `OP_BITOR`); `&&` and `||` treat a float as true when
  it is not 0.0 (`OP_AND`, `OP_OR`); `NOT_V` is true when all three components are 0.0;
  `NOT_S` is true for the null string and for an empty string; `NOT_ENT` is true for the
  world entity; `EQ_S` compares string contents. The VM spec states the full rules.
- **QC-16.6** A field access `e.f` reads the field `f` of entity `e`. On the left of `=` it
  becomes a store into the field (QC-17.3).

### Test vectors

| Input (in a function body) | Result | Statements |
| --- | --- | --- |
| `a = v * v; v = v * a; v = a * v;` | `MUL_V`, `MUL_VF`, `MUL_FV` | QC-16.1 |
| `v = v / 2;` | `type mismatch for /` | QC-16.2 |
| `a = v < v;` | `type mismatch for <` | QC-16.2 |
| `a = fl == fl;` with `.float fl` | `type mismatch for ==` | QC-16.2 |
| `if (e && a) a = 1;` with `entity e` | `type mismatch for &&` | QC-16.2 |
| `if (g == g) g();` with `void() g` | `EQ_FNC g, g, t`; `IFNOT t, +2`; `CALL0 g` | QC-16.3 |
| `a = s == "x"; a = e != world; a = !s; a = !e; a = !v;` | `EQ_S`, `NE_E`, `NOT_S`, `NOT_ENT`, `NOT_V` | QC-16.1, QC-16.4 |

## QC-17 Assignment

**Source:** `qcc/pr_comp.c`, `PR_Expression`, `PR_Statement`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QC-17.1** `x = e` requires `x` and `e` to have the same type and emits
  `STORE_F`, `STORE_V`, `STORE_S`, `STORE_ENT`, `STORE_FLD` or `STORE_FNC` with `e` as
  operand a and `x` as operand b. Its value is `e`, so `a = b = c` stores `c` into `b` and
  then `c` into `a`.
- **QC-17.2** id's `qcc` does not check that the left side can be assigned: `5 = a;` stores
  into the literal's global, `f() = 3` into the return slot and `(a + b) = 3` into a
  temporary. QC-27.10 makes these errors.
- **QC-17.3** When `=` is met and the last statement emitted so far is a `LOAD_*`, that
  statement is rewritten to `ADDRESS` (its result becomes a pointer to the field) and the
  assignment emits `STOREP_F`, `STOREP_V`, `STOREP_S`, `STOREP_ENT`, `STOREP_FLD` or
  `STOREP_FNC`, storing the right side through the pointer. This is how `e.f = x` stores
  into a field; in `e.en.hp = 3` only the last access becomes `ADDRESS`.
- **QC-17.4** QC-17.3 looks at the last statement emitted, not at the left operand. If the
  previous statement ended with a field load and the next assignment's left side is a plain
  name, the earlier load is rewritten and the name's definition is retyped as a pointer:
  `e.fl; a = 2;` emits `ADDRESS e, fl, t` and `STOREP_F 2, a`. QC-27.11 diverges.
- **QC-17.5** There are no compound assignments (QC-7.4).

### Test vectors

| Input (in a function body) | Statements emitted | Statements |
| --- | --- | --- |
| `a = b = c;` | `STORE_F c, b`; `STORE_F c, a` | QC-17.1 |
| `5 = a;` | `STORE_F a, IMMEDIATE` | QC-17.2 |
| `a = e.o_z; e.o_x = 1;` with `.vector o` | `INDIRECT e, o_z, t1`; `STORE_F t1, a`; `ADDRESS e, o_x, t2`; `STOREP_F 1, t2` | QC-17.3 |
| `e.o = '1 2 3';` | `ADDRESS e, o, t`; `STOREP_V '1 2 3', t` | QC-17.3 |
| `e.en.hp = 3;` | `INDIRECT e, en, t1`; `ADDRESS t1, hp, t2`; `STOREP_F 3, t2` | QC-17.3 |
| `e.fl; a = 2;` | `ADDRESS e, fl, t`; `STOREP_F 2, a` | QC-17.4 |

(`-asm` prints `LOAD_*` as `INDIRECT`, its name in `pr_opcodes`.)

## QC-18 Function calls

**Source:** `qcc/pr_comp.c`, `PR_ParseFunctionCall`; `qcc/qcc.c`, `InitData`; `qcc/qcc.h`
(`OFS_RETURN`, `OFS_PARM0`) (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "Functions cannot have more than eight parameters."

### Behaviour

- **QC-18.1** `f(a1, ..., an)` requires `f` to have a function type, else "not a function".
  For a fixed parameter list, more arguments than parameters is an error, "too many
  parameters" (raised before the extra argument is parsed); fewer is "too few parameters";
  an argument whose type is not exactly the parameter's is "type mismatch on parm N",
  counting from 0.
- **QC-18.2** A variadic function (QC-10.4) takes up to eight arguments of any types; nine or
  more is an error, "More than eight parameters".
- **QC-18.3** Each argument is evaluated and immediately copied to its parameter slot with
  `STORE_V` (three words, whatever its type), slot `i` being global offset `4 + 3i`. After
  the last argument, `CALLn f` is emitted, `n` the argument count.
- **QC-18.4** The call's value is the global at offset 1 (`OFS_RETURN`), typed as the
  function's return type.
- **QC-18.5** Because the slots are shared, a call inside a later argument overwrites the
  slots of earlier arguments: in `g(4, h(1))`, `h`'s argument is stored into slot 0 over the
  4, so `g` receives 1. `qcc.h`: "afunc ( 4, bfunc(1,2,3)); will fail because there is a
  shared parameter marshaling area, which will cause the 1 from bfunc to overwrite the 4
  allready placed in parm0."
- **QC-18.6** Because the return slot is shared, two calls in one expression read the same
  slot after both calls: `g() + h()` adds `h()`'s result to itself. `qcc.h`: "total =
  factorial(3) + factorial(4); Will fail because the return value from functions is held in
  a single global area."
- **QC-18.7** A call through a field, `e.th()`, loads the function from the field and calls
  the loaded value.

### Constants

The slots are `PROGS_OFS_RETURN` and `PROGS_OFS_PARM0` in [`progs.md`](progs.md); `qcc/qcc.h`
defines the same `OFS_RETURN` and `OFS_PARM0`, and `InitData` puts slot `i` at
`OFS_PARM0 + 3*i`.

### Test vectors

| Input | Statements emitted | Statements |
| --- | --- | --- |
| `a = g(4, h(1));` with `float(float p, float q) g` and `float(float p) h` | `STORE_V 4, parm0`; `STORE_V 1, parm0`; `CALL1 h`; `STORE_V return, parm1`; `CALL2 g`; `STORE_F return, a` | QC-18.3, QC-18.5 |
| `a = g() + h();` | `CALL0 g`; `CALL0 h`; `ADD_F return, return, t`; `STORE_F t, a` | QC-18.6 |
| `f(1, "x", '1 2 3');` with `void(...) f` | `STORE_V 1, parm0`; `STORE_V "x", parm1`; `STORE_V '1 2 3', parm2`; `CALL3 f` | QC-18.2, QC-18.3 |
| `g(1,2,3,4,5,6,7,8,9);` with `void(...) g` | `More than eight parameters` | QC-18.2 |
| `g(1, 2);` with `void(float a) g` | `too many parameters` | QC-18.1 |
| `g(1);` with `void(float a, float b) g` | `too few parameters` | QC-18.1 |
| `g("x");` with `void(float a) g` | `type mismatch on parm 0` | QC-18.1 |
| `a();` with `float a` | `not a function` | QC-18.1 |
| `e.th();` with `.void() th` | `INDIRECT e, th, t`; `CALL0 t` | QC-18.7 |

## QC-19 Conversions and conditions

**Source:** `qcc/pr_comp.c`, `PR_ParseStatement`, `PR_Expression`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`); `WinQuake/pr_exec.c`,
`PR_ExecuteProgram`, `OP_IF`, `OP_IFNOT`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **QC-19.1** There are no implicit conversions and no casts. Every operand, argument and
  assigned value must have exactly the required type; the only exceptions are QC-19.2,
  QC-19.3 and the variadic arguments of QC-18.2.
- **QC-19.2** The condition of `if`, `while` and `do` may have any type. The statement tests
  the first word of the value as a 32-bit integer (`if (!a->_int)` in `OP_IFNOT`): a float is
  false only when it is +0.0 (-0.0 is true), a string only when it is the null string, an
  entity only when it is the world, a function only when it is null.
- **QC-19.3** A vector condition tests only its first word, the x component: `if (v)` emits
  `IFNOT v_x`, so `'0 0 1'` is false. `!v` tests all three (QC-16.5).
- **QC-19.4** An empty string literal is not the null string: `if ("")` is true and `!""` is
  true (QC-16.5).
- **QC-19.5** A `return` value is not converted or checked (QC-13.8); the engine copies three
  words from it into the return slot (`OP_RETURN`).

### Test vectors

| Input (in a function body) | Statements emitted | Statements |
| --- | --- | --- |
| `if (v) a = 1;` | `IFNOT v_x, +2`; `STORE_F 1, a` | QC-19.3 |

## QC-20 Literals and constants in the progs

**Source:** `qcc/pr_comp.c`, `PR_ParseImmediate`; `qcc/qcc.c`, `CopyString`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QC-20.1** Every literal in an expression is a global holding its value. A string
  literal's text is copied into the string table and the global holds its offset.
- **QC-20.2** Before allocating one, `qcc` walks all definitions in order and reuses the
  first initialised definition of the same type and equal value: a named constant, a local
  initialiser, or an earlier literal. Floats compare with `==`, vectors component by
  component with `==`, strings by the text at the definition's string offset.
- **QC-20.3** Since `0.0 == -0.0`, `0` and `-0` share one global, holding whichever was met
  first.
- **QC-20.4** A new literal's definition is named `IMMEDIATE`, is global, and is written to
  the global definitions table like any other definition (QC-25.3).
- **QC-20.5** A string literal's text is copied into the string table once, when its global
  is allocated; a reused literal adds nothing.
- **QC-20.6** A `string` declaration initialiser does not copy its text anywhere. The global
  receives the first word of the lexer's last literal value: the string offset of the last
  string literal allocated by QC-20.1, or the bits of the last float or vector literal read,
  whichever came last, and 0 (the empty string) before any. So `string s = "x";` gives `s` a
  wrong value, and no later `"x"` literal reuses it. id 1.01 declares no string constants.
  QC-27.12 diverges.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| `float CONST = 3;\nfloat a;\nvoid() f = { a = 3; a = -0; a = 0; };` | `STORE_F CONST, a`; then `STORE_F -0.0, a` twice, one new global | QC-20.2, QC-20.3 |
| `float a;\nvoid() f = { a = 0; a = -0; };` | `STORE_F 0.0, a` twice, one global | QC-20.3 |
| `vector V = '1 2 3';\nvector w;\nvoid() f = { w = '1 2 3'; };` | `STORE_V V, w` | QC-20.2 |
| `string s1 = "x";\nstring s2;\nvoid() f = { s2 = "x"; s2 = "y"; s2 = "y"; };` | `s1` holds the empty string; the first `"x"` is a new global; both `"y"` share one | QC-20.5, QC-20.6 |
| `string t;\nvoid() f = { t = "y"; };\nstring s = "x";` | `s` holds `"y"` | QC-20.6 |
| `float a = 1;` then `string s = "hi";` | `s` holds 0x3F800000, the bits of 1.0 | QC-20.6 |

## QC-21 Global and field layout

**Source:** `qcc/pr_comp.c`, `PR_GetDef`, `PR_Statement`, `PR_ParseDefs`; `qcc/qcc.c`,
`PR_BeginCompilation`, `InitData`; `qcc/qcc.h`, `RESERVED_OFS`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QC-21.1** Global offsets 0 to 27 are reserved: 0 is null, 1 to 3 the return value, and
  4 to 27 the eight three-word parameter slots. The first definition is at offset 28.
- **QC-21.2** Every definition, temporary and literal takes the next free global offsets in
  the order the compiler meets them, its size from `QC_TYPE_SIZE`. A vector takes the three
  offsets of its `_x`, `_y` and `_z` definitions (QC-11.7).
- **QC-21.3** A field definition's global holds its offset in the entity, in words. Field
  offsets are assigned from 0 in declaration order, a `.vector` taking three; the total is
  the progs' `entityfields`.
- **QC-21.4** A function's `parm_start` is the offset of its first parameter and its `locals`
  count is the number of words from there to the end of the last `local` declaration in its
  body, including any temporaries allocated before that declaration; temporaries after the
  last `local` are not counted. A function with no `local` declaration has `locals` 0 even
  when it has parameters: 26 functions of id 1.01 do, `SUB_AttackFinished` among them.
- **QC-21.5** Each operator, comparison or field load gets a new temporary that is never
  reused, not even across statements. Assignments, `IF`, `IFNOT`, `GOTO`, `RETURN`, `STATE`
  and calls get none.
- **QC-21.6** A global definition is marked to be saved in saved games
  (`DEF_SAVEGLOBGAL`) when it is global, not initialised, and not a function or field.
  `qcc.h`: "All uninitialized globals will be saved to savegame files."

### Constants

`PROGS_RESERVED_OFS` and `PROGS_DEF_SAVEGLOBAL` in [`progs.md`](progs.md); `qcc` uses the same
values (`RESERVED_OFS` in `qcc/qcc.h`, `DEF_SAVEGLOBGAL` in `qcc/pr_comp.h`).

### Test vectors

For `float a = 1; vector v; string s = "hi"; .float fl; .vector fv; entity e; float
end_sys_globals; .float end_sys_fields; void(float p, vector q) f = { local float l; local
vector lv; a = p; };`:

| Definition | Offset | Saved | Statements |
| --- | --- | --- | --- |
| `a` | 28 | no | QC-21.1, QC-21.6 |
| `v`, `v_x`, `v_y`, `v_z` | 29, 29, 30, 31 | yes | QC-21.2 |
| `fl` (field offset 0), `fv` (field offset 1, `fv_z` 3) | 33, 34 | no | QC-21.3 |
| `end_sys_fields` (field offset 4); `entityfields` 5 | 40 | no | QC-21.3 |
| `f`: `parm_start` 42, `locals` 8 (`p` 42, `q` 43, `l` 46, `lv` 47) | | | QC-21.4 |
| In another program, `void(float p, vector q) f = { a = p + 1; };`: `locals` 0 | | | QC-21.4 |
| In another program, `float a; void(float p) f = { a = p + 1; local float l; l = 2; };`: the sum's temporary at 32, then `l` at 33 | | | QC-21.4, QC-21.5 |

## QC-22 Construct to statement mapping

**Source:** `qcc/pr_comp.c`, `PR_ParseStatement`, `PR_Expression`, `PR_ParseFunctionCall`,
`PR_ParseImmediateStatements`; `qcc/pr_comp.h`, the opcode enumeration
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

A summary for code generation; the sections cited hold the rules.

### Behaviour

- **QC-22.1** Each construct emits exactly the progs statements in the table below, in that
  order.

| Construct | Statements | Rule |
| --- | --- | --- |
| `a OP b` | statements of `a`, of `b`, then `OP a, b, t` | QC-15.9, QC-16.1 |
| `!a` | statements of `a`, `NOT_* a, t` | QC-16.4 |
| `e.f` | statements of `e`, `LOAD_* e, f, t` | QC-16.1 |
| `x = e` | statements of `e`, `STORE_* e, x` | QC-17.1 |
| `e.f = v` | `ADDRESS e, f, t`, statements of `v`, `STOREP_* v, t` | QC-17.3 |
| `f(a, b)` | statements of `a`, `STORE_V a, parm0`, statements of `b`, `STORE_V b, parm1`, `CALL2 f` | QC-18.3 |
| `return;`, `return e;` | `RETURN`, `RETURN e` | QC-14.3 |
| `if`, `if`/`else` | `IFNOT`, then `GOTO` for `else` | QC-14.4 |
| `while` | condition, `IFNOT`, body, `GOTO` | QC-14.5 |
| `do`/`while` | body, condition, `IF` | QC-14.6 |
| `= [frame, next] { }` | `STATE frame, next`, body | QC-13.6 |
| end of a body | `DONE` | QC-13.7 |
| `local` | nothing | QC-11.10 |

- **QC-22.2** Branch operands are relative statement counts: `IF` and `IFNOT` carry the
  offset in operand b and `GOTO` in operand a, and the engine adds it to the index of the
  branch statement itself.
- **QC-22.3** Operands a, b and c are signed 16-bit global offsets in the progs, as
  `dstatement_t` declares them; an unused operand is 0.

## QC-23 `progs.src`

**Source:** `qcc/qcc.c`, `main`; `qcc/cmdlib.c`, `COM_Parse`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QC-23.1** `qcc` reads `progs.src` from the current directory, or from the directory given
  with `-src`. It is a list of tokens: the first names the output file and each later one a
  source file.
- **QC-23.2** Tokens are split as `COM_Parse` does: whitespace separates them; `//` starts a
  comment to the end of the line; `"..."` is one token without the quotes; each of `{`, `}`,
  `(`, `)`, `'` and `:` is a token of its own and ends a word. There are no `/* */` comments.
- **QC-23.3** The source files are compiled in the order listed, as one program: each sees
  every definition of the files before it (QC-12.5). A file may be listed twice and is then
  compiled twice.
- **QC-23.4** The output path is used as written, relative to the current directory, not to
  the `-src` directory; source paths are prefixed with the `-src` directory. id 1.01's
  `progs.src` names `../progs.dat`.
- **QC-23.5** An empty `progs.src` is fatal, `No destination filename.  qcc -help for info.`
  (with two spaces). A source file that cannot be opened is fatal.
- **QC-23.6** Files that are in the source directory but not in `progs.src` are not part of
  the program (id 1.01 ships `amtest.qc`, `jctest.qc`, `models.qc` and `sprites.qc` this way).

## QC-24 Errors and recovery

**Source:** `qcc/pr_comp.c`, `PR_CompileFile`; `qcc/pr_lex.c`, `PR_ParseError`,
`PR_SkipToSemicolon`; `qcc/qcc.c`, `main`, `PR_FinishCompilation`; `qcc/qcc.h`, `MAX_ERRORS`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

`qcc.h`: "Error recovery during compilation is minimal. It will skip to the next global
definition, so you will never see more than one error at a time in a given function. All
compilation aborts after ten error messages."

### Behaviour

- **QC-24.1** id's `qcc` has no warnings. Every diagnostic is an error, printed as
  `<file>:<line>:<message>` with the line of QC-1.5.
- **QC-24.2** An error abandons the current top-level declaration. Recovery then skips tokens
  until it consumes a `;` at brace depth 0 and resumes parsing declarations after it.
- **QC-24.3** Recovery re-lexes from the error position, so a lexical error can repeat; an
  unknown byte (QC-7.2) repeats until the error limit.
- **QC-24.4** The error count runs across the whole compilation. When the 11th error is
  reported, the current file stops.
- **QC-24.5** After a file that had any error, `qcc` exits with status 1 without compiling
  the remaining files.
- **QC-24.6** After all files, every global function that was never defined is reported
  (QC-11.8) and compilation fails with the fatal error "compilation errors".
- **QC-24.7** An error raised while lexing the first token of a file happens before the
  recovery point is set up; id's `qcc` jumps through an uninitialised `jmp_buf` and crashes.
  QC-27.1 makes it an ordinary error.

### Constants

`LIMIT_QCC_MAX_ERRORS` in [`limits.md`](limits.md): the file stops when the count exceeds it.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| twelve lines `x1;` to `x12;` | errors for `x1` to `x11`, then the file stops | QC-24.4 |
| `float a;\nfloat b % c;\nfloat d;` | `Unknown punctuation` eleven times | QC-24.3 |
| `void() f = { a = 1; b = 2; };\nfloat q = zz;` | one error, `Unknown value "a"`; recovery skips the body | QC-24.2 |
| `%float a;` | `qcc` crashes (signal 11) | QC-24.7 |

## QC-25 Program output

**Source:** `qcc/qcc.c`, `WriteData`, `PR_WriteProgdefs`, `CRC_ProcessByte`, `InitData`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`). The file format is the
progs spec's; this section states only what the compiler puts in it.

### Behaviour

- **QC-25.1** The output is a version 6 `progs.dat`. Statement 0, function 0, global
  definition 0, field definition 0 and string offset 0 (the empty string) are placeholders
  written before anything compiled.
- **QC-25.2** Functions are numbered in the order their definitions finish parsing, from 1.
- **QC-25.3** The global definitions table has one entry for every definition, in
  declaration order: globals, locals, parameters, `_x`/`_y`/`_z` components and `IMMEDIATE`
  literals, but not temporaries. Field definitions appear in both the field table (with the
  field's type) and the global table (with type field).
- **QC-25.4** Strings are appended to the string table as they are met, never shared: each
  source file's name when the file starts, each string literal's text when its global is
  allocated, and each function's name when its definition finishes. Then, while writing,
  the name of every definition is appended once for the global table and once more for each
  field definition's entry in the field table. The table's length is rounded up to a
  multiple of 4.
- **QC-25.5** `qcc` also writes `progdefs.h` in the current directory: a C `globalvars_t`
  with every global declared before the one named `end_sys_globals`, and an `entvars_t` with
  every field declared before the one named `end_sys_fields`, each element typed by its
  QuakeC type.
- **QC-25.6** The progs header's `crc` is the 16-bit CRC (CCITT polynomial 0x1021,
  initial value 0xFFFF, no final xor) of the text of `progdefs.h` up to the `#define` line,
  with LF line ends, as `qcc` reads it back. The engine refuses a progs whose CRC differs
  from its own `PROGHEADER_CRC`, 5927 for id's 1.01 system definitions (`PR_LoadProgs` in
  `WinQuake/pr_edict.c`: "progs.dat system vars have been modified, progdefs.h is out of
  date").
- **QC-25.7** Writing `progdefs.h`, `qcc` assumes every vector definition before
  `end_sys_globals` is followed by its three component definitions. A vector literal (an
  `IMMEDIATE` with no components) before `end_sys_globals`, or the absence of
  `end_sys_globals` with any vector literal, makes it read past the end of the definition
  list; the reference build crashed. QC-27.13 diverges.

### Constants

`PROGS_VERSION`, `PROGS_CRC_POLY`, `PROGS_CRC_INIT`, `PROGS_CRC_XOR` and `PROGS_HEADER_CRC` in
[`progs.md`](progs.md). `qcc.c` has the same CRC table and values (`CRC_INIT_VALUE`,
`CRC_XOR_VALUE`), and compiling id 1.01 reproduces `PROGS_HEADER_CRC`.

### Test vectors

| Input | Result | Statements |
| --- | --- | --- |
| id 1.01 `progs.src` | 410,616 bytes; 20,732 statements, 2,091 functions, 4,272 global and 218 field definitions, 11,350 globals; `PROGHEADER_CRC 5927` | QC-25.1, QC-25.6 |
| `vector v;\nvoid() f = { v = '1 2 3'; };\nfloat end_sys_globals;` | `qcc` crashes (signal 11) | QC-25.7 |

## QC-26 Precache lists

**Source:** `qcc/pr_comp.c`, `PR_ParseFunctionCall`, `PrecacheSound`, `PrecacheModel`,
`PrecacheFile`; `qcc/qcc.c`, `CopyFiles`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

These lists do not reach `progs.dat`; `qcc -copy` and `-pak` use them to collect game files.

### Behaviour

- **QC-26.1** When the called function's name starts with `precache_sound`,
  `precache_model` or `precache_file`, the compile-time string value of the first argument's
  global is added to that list, once per distinct string.
- **QC-26.2** The byte after the prefix chooses the pak block: `1` to `9` give that number
  (`precache_sound2` is block 2), anything else block 1.
- **QC-26.3** A first argument that is not a constant adds the string at its global's
  compile-time value, normally the empty string.
- **QC-26.4** `PrecacheModel` checks the sound count, not the model count, against its limit
  ("PrecacheModels: numsounds == MAX_SOUNDS"), so the model list itself is unchecked.

### Constants

`LIMIT_QCC_MAX_PRECACHE` in [`limits.md`](limits.md) (LIMIT-7.3).

## QC-27 Divergences from id's `qcc`

**Source:** this project. Each statement names the id behaviour it replaces.

This compiler's vanilla profile emits the same progs as id's `qcc` for every program `qcc`
compiles, except where `qcc`'s result depends on undefined behaviour, stale state or a
crash, or where it silently compiles something other than what is written. In those cases
it reports an error, or does what the source says, as each statement below specifies. The
differential suite lists each of these as an expected divergence.

### Behaviour

- **QC-27.1** A lexical error in the first token of a file is an ordinary error (QC-24.7).
- **QC-27.2** A `/*` comment with no `*/` is an error, "unterminated comment", at the line of
  the `/*` (QC-2.4).
- **QC-27.3** Keywords and punctuation are matched only against name and punctuation tokens;
  a string literal is always a string (QC-3.4).
- **QC-27.4** A name, string literal or other token longer than `QC_MAX_TOKEN - 1` bytes is an
  error (QC-6.4).
- **QC-27.5** A `,` or `;` on a `$frame` line is an error, and more than
  `LIMIT_QCC_MAX_FRAMES` frames in one file is an error (QC-8.9).
- **QC-27.6** A frame name longer than `QC_MAX_FRAME_NAME` bytes is accepted and looked up by
  its full name, with a warning that id's `qcc` cannot use it (QC-8.9).
- **QC-27.7** A function type with more than `LIMIT_MAX_PARMS` parameters is an error (QC-10.8).
- **QC-27.8** An initialiser that is not a literal or frame macro reference is an error,
  "initialiser is not a constant" (QC-11.4).
- **QC-27.9** `#0` and negative builtin numbers are errors (QC-13.4).
- **QC-27.10** The left side of `=` must be a name or a field access; a literal, a call or an
  operator's result is an error, "cannot assign to this expression" (QC-17.2). Assigning to
  an initialised definition stays legal, as in id, with a warning (QC-11.6).
- **QC-27.11** Only a field access on the left of `=` is turned into an address; an earlier
  statement's field load is never rewritten (QC-17.4).
- **QC-27.12** A `string` initialiser stores the offset of its own text in the string table
  (QC-20.6).
- **QC-27.13** `progdefs.h` generation skips component definitions only for vectors that
  have them (QC-25.7).
- **QC-27.14** A `,` or `;` on a `$cd`, `$origin`, `$base`, `$flags`, `$scale` or `$skin`
  line makes id's `qcc` loop forever, as on a `$frame` line (QC-8.9). This compiler ignores
  the whole rest of such a line, `,` and `;` included (QC-8.4).
- **QC-27.15** Constructs nested more than `QC_MAX_NESTING` levels deep are an error, "nesting
  too deep", at the token where the limit is passed. Each of these is one level inside the
  construct around it: a statement, a type, an expression (a whole statement's or
  initialiser's, a parenthesised one, the operand of `!`, a call argument, the right operand
  of a binary operator), and each binary operator, field access or call, which nests the
  expression before it one level deeper. id's `qcc` has no such limit; this compiler bounds
  the depth of its syntax tree so that no input can exhaust its stack.
- **QC-27.16** Once the limit of QC-27.15 is passed in a statement or a declaration, no
  construct in it opens another level: the constructs it unwinds through read no further
  operator, field access, call or type around what they hold, so the tree stays within the
  limit, and the rest of the statement or declaration is skipped as after any error (QCD-1.4).
  The next statement or declaration nests afresh.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `QC_MAX_NESTING` | 256 | this project (QC-27.15) |

## Coverage of id 1.01

Every construct `corpus/language_coverage.py` finds in id's QuakeC 1.01 (the `id-1.01`
codebase in `corpus/manifest.toml`), with the sections that specify it. The script fails if
the source uses a construct with no row, or a row names a section that does not exist. Rows
for constructs 1.01 does not use (`...`, `escape \"`) complete the language.

| Construct | Sections |
| --- | --- |
| `!` | QC-15, QC-16 |
| `!=` | QC-16 |
| `#` | QC-13, QC-9 |
| `$base` | QC-8 |
| `$cd` | QC-8 |
| `$flags` | QC-8 |
| `$frame` | QC-8 |
| `$framegroupend` | QC-8 |
| `$framegroupstart` | QC-8 |
| `$load` | QC-8 |
| `$modelname` | QC-8 |
| `$origin` | QC-8 |
| `$scale` | QC-8 |
| `$skin` | QC-8 |
| `$spritename` | QC-8 |
| `$type` | QC-8 |
| `&` | QC-15, QC-16 |
| `&&` | QC-15, QC-16 |
| `(` | QC-15, QC-18, QC-10 |
| `)` | QC-15, QC-18, QC-10 |
| `*` | QC-15, QC-16 |
| `+` | QC-15, QC-16 |
| `,` | QC-11, QC-18, QC-10 |
| `-` | QC-15, QC-16, QC-4 |
| `.` | QC-10, QC-16, QC-17 |
| `...` | QC-10 |
| `/` | QC-15, QC-16 |
| `/* comment */` | QC-2 |
| `// comment` | QC-2 |
| `;` | QC-11, QC-14 |
| `<` | QC-16 |
| `<=` | QC-16 |
| `=` | QC-11, QC-13, QC-17 |
| `==` | QC-16 |
| `>` | QC-16 |
| `>=` | QC-16 |
| `[` | QC-13 |
| `]` | QC-13 |
| `do` | QC-14 |
| `else` | QC-14 |
| `entity` | QC-10 |
| `escape \"` | QC-6 |
| `escape \n` | QC-6 |
| `float` | QC-10 |
| `float literal` | QC-4, QC-20 |
| `frame macro reference` | QC-8 |
| `if` | QC-14 |
| `local` | QC-11, QC-14 |
| `negative float literal` | QC-4 |
| `return` | QC-14, QC-13 |
| `string` | QC-10 |
| `string literal` | QC-6, QC-20 |
| `vector` | QC-10 |
| `vector component name` | QC-11, QC-15 |
| `vector literal` | QC-5, QC-20 |
| `void` | QC-10 |
| `while` | QC-14 |
| `{` | QC-13, QC-14 |
| `\|` | QC-15, QC-16 |
| `\|\|` | QC-15, QC-16 |
| `}` | QC-13, QC-14 |
