# QuakeC diagnostics

Prefix: `QCD` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

The catalogue of errors and warnings for vanilla QuakeC, with the codes the compiler, the
editor, the language server and the command line all report (`qc-diag`, P2-05). Each error
id's `qcc` can raise is listed with its message, quoted from `qcc/pr_lex.c`, `qcc/pr_comp.c`,
`qcc/qcc.c` and `qcc/cmdlib.c` in
[id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools) at commit
`c0d1b91c74eb654365ac7755bc837e497caaca73`, and the rule in [`language.md`](language.md) that
raises it. id's `qcc` has no warnings (QC-24.1); the errors this compiler adds for QC-27 and
every warning are this project's own.

To be implemented by `qc-diag` and every crate that reports a diagnostic. Until code cites
them, the statements are in [`coverage-baseline.tsv`](coverage-baseline.tsv).

## QCD-1 Codes

**Source:** this project; id's message format from `qcc/pr_lex.c`, `PR_ParseError`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QCD-1.1** A code is `E` (error) or `W` (warning) and four digits. The first two digits
  are the group: `01` lexical, `02` declarations and types, `03` expressions and statements,
  `04` project, limits and fatal errors, `05` errors this compiler adds where id's `qcc` has
  undefined behaviour (QC-27). Warnings use the same groups.
- **QCD-1.2** A code is never reused for a different diagnostic. A diagnostic that no longer
  applies is withdrawn with its statement, and its code stays unassigned.
- **QCD-1.3** Every diagnostic has a primary span: the token the rule names, or, for errors
  id's `qcc` raises at its lookahead token, the construct the rule is about. id's own report
  is `<file>:<line>:<message>` with the line of QC-1.5; the vanilla profile's differential
  tests compare messages and codes, not lines.
- **QCD-1.4** For the errors id's `qcc` raises, the message is id's text, with `%s` and `%i`
  filled in as id does. The parser keeps going after an error with recovery of its own
  (LANG-2); the error limit and stop-at-first-file of QC-24 are id's behaviour, not
  requirements on this compiler.

## QCD-2 Lexical errors

**Source:** `qcc/pr_lex.c`, `PR_LexString`, `PR_LexVector`, `PR_LexPunctuation`,
`PR_FindMacro`, `PR_LexGrab` (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QCD-2.1** `E0101` "EOF inside quote": the file ends inside a string literal (QC-6.3).
- **QCD-2.2** `E0102` "newline inside quote": a line break inside a string literal (QC-6.3).
- **QCD-2.3** `E0103` "Unknown escape char": a backslash followed by anything but `n` or `"`
  (QC-6.2).
- **QCD-2.4** `E0104` "Bad vector": a vector literal's third component is not followed by
  `'` (QC-5.2).
- **QCD-2.5** `E0105` "Unknown punctuation": a byte that starts no token (QC-7.2).
- **QCD-2.6** `E0106` "Unknown frame macro $NAME": a `$` word that is neither a known command
  nor a frame defined earlier in the file (QC-8.5, QC-8.8).
- **QCD-2.7** `E0107` "hanging $": `$` with no word after it on its line (QC-8.1).
- **QCD-2.8** `E0119` "Name for frame macro NAME is too long": under an FTEQCC profile, a new
  frame name of 64 bytes or more, at the name, FTEQCC's error of the same text
  (`dialect-fteqcc.md`, FTEF-18.22).

## QCD-3 Declaration and type errors

**Source:** `qcc/pr_lex.c`, `PR_ParseName`, `PR_ParseType`, `PR_Expect`; `qcc/pr_comp.c`,
`PR_ParseDefs`, `PR_GetDef`, `PR_ParseImmediateStatements`, `PR_ParseState`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QCD-3.1** `E0201` "not a name": a name was required (QC-3, QC-14.2, QC-4.5).
- **QCD-3.2** `E0202` "name too long": a name of 63 bytes or more, `LIMIT_QCC_MAX_NAME - 1`
  (QC-3.3).
- **QCD-3.3** `E0203` "\"TOKEN\" is not a type": a type was required (QC-10, QC-10.4).
- **QCD-3.4** `E0204` "expected X, found Y": the parser required the token X (QC-11.2,
  QC-14.6, QC-14.7, QC-4.2).
- **QCD-3.5** `E0205` "Type mismatch on redeclaration of NAME": a name redeclared with
  another type (QC-11.5, QC-12.3, QC-13.6).
- **QCD-3.6** `E0206` "NAME redeclared": a definition initialised twice (QC-11.5).
- **QCD-3.7** `E0207` "wrong immediate type for NAME": an initialiser of the wrong type, or
  any initialiser for an entity, void or field (QC-11.3).
- **QCD-3.8** `E0208` "Fields and functions must be global": a `local` field or function
  (QC-11.9).
- **QCD-3.9** `E0209` "Bad builtin immediate": `#` not followed by an integral float literal
  (QC-13.3).
- **QCD-3.10** `E0210` "state frame must be a number": a state function's frame is not a float
  literal or frame macro reference (QC-13.6).
- **QCD-3.11** `E0211` "function NAME was not defined": reported after all files for each
  global function never defined (QC-11.8, QC-24.6).
- **QCD-3.12** `E0212` "Forwards State Cycle with backwards range": under an FTEQCC profile, a
  frame cycle whose two constant frames run against its `++` or `--`, FTEQCC's error of the
  same text (`dialect-fteqcc.md`, FTEF-18.19), over the two frames.

## QCD-4 Expression and statement errors

**Source:** `qcc/pr_comp.c`, `PR_ParseValue`, `PR_Term`, `PR_Expression`,
`PR_ParseFunctionCall` (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QCD-4.1** `E0301` "Unknown value \"NAME\"": a name with no definition in scope (QC-12.5).
- **QCD-4.2** `E0302` "type mismatch for OP": no row of the operator table matches the
  operand types (QC-16.2, QC-16.4); `OP` is the operator's spelling.
- **QCD-4.3** `E0303` "not a function": a call of something that is not a function
  (QC-18.1).
- **QCD-4.4** `E0304` "too many parameters": more arguments than a fixed parameter list
  (QC-18.1).
- **QCD-4.5** `E0305` "too few parameters": fewer arguments than a fixed parameter list
  (QC-18.1).
- **QCD-4.6** `E0306` "type mismatch on parm N": argument N, counting from 0, has the wrong
  type (QC-18.1).
- **QCD-4.8** `E0308` "Goto statement with no matching label "NAME"": under an FTEQCC
  profile, a `goto` whose label is not among those read since the last function's end by the
  end of its function, at the label name, FTEQCC's error of the same text
  (`dialect-fteqcc.md`, FTEF-18.24).
- **QCD-4.7** `E0307` "More than eight parameters": nine or more arguments to a variadic
  function (QC-18.2).

## QCD-5 Project, limit and fatal errors

**Source:** `qcc/qcc.c`, `main`, `PR_FinishCompilation`; `qcc/pr_comp.c`,
`PR_ParseImmediateStatements`, `PrecacheSound`, `PrecacheModel`, `PrecacheFile`;
`qcc/cmdlib.c`, `SafeOpenRead` (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).

### Behaviour

- **QCD-5.1** `E0401` "No destination filename.  qcc -help for info.": `progs.src` has no
  tokens (QC-23.5).
- **QCD-5.2** `E0402` "Error opening PATH: REASON": `progs.src` or a listed file cannot be
  read (QC-23.5).
- **QCD-5.3** `E0403` "compilation errors": compilation ended with errors (QC-24.6). It
  summarises and never stands alone.
- **QCD-5.4** `E0404` "bad parm order": a parameter resolved to a global below the previous
  parameter's offset (QC-13.5).
- **QCD-5.5** `E0405` "PrecacheSound: numsounds == MAX_SOUNDS", "PrecacheModels: numsounds ==
  MAX_SOUNDS" or "PrecacheFile: numfiles == MAX_FILES": a precache list is full (QC-26.4).
- **QCD-5.6** `E0406` "#includelist without #endlist": a file ends inside an `#includelist`
  (PRJ-5.3). This project's own; FTEQCC's text is "eof in includelist".
- **QCD-5.7** `E0407` "too many files in the project graph": a module's file graph would hold
  more than `PRJ_MAX_FILES` entries (PRJ-6.4). This project's own.
- **QCD-5.8** `E0408` "files nested too deeply to read PATH": bringing a file in would make
  more than `PRJ_MAX_DEPTH` files being read at once (PRJ-6.3). This project's own.

## QCD-6 Errors this compiler adds

**Source:** this project, for the divergences in QC-27.

### Behaviour

- **QCD-6.1** `E0501` "unterminated comment": a `/*` with no `*/`, at the `/*` (QC-27.2).
- **QCD-6.2** `E0502` "token too long": a name, string literal or other token longer than
  `QC_MAX_TOKEN - 1` bytes (QC-27.4).
- **QCD-6.3** `E0503` "`,` or `;` in a $frame list": on a `$frame` line (QC-27.5).
- **QCD-6.4** `E0504` "too many frames": more than `LIMIT_QCC_MAX_FRAMES` frames in one file
  (QC-27.5).
- **QCD-6.5** `E0505` "more than eight parameters in a function type" (QC-27.7).
- **QCD-6.6** `E0506` "initialiser is not a constant": a declaration initialiser that is not
  a literal or frame macro reference (QC-27.8).
- **QCD-6.7** `E0507` "builtin number must be positive": `#0` or `#-n` (QC-27.9).
- **QCD-6.8** `E0508` "cannot assign to this expression": the left side of `=` is a
  literal, a call, or an operator's result (QC-27.10).
- **QCD-6.9** `E0509` "nesting too deep": statements, types or expressions nested more than
  `QC_MAX_NESTING` levels (QC-27.15).
- **QCD-6.10** `E0510` "NAME is an FTEQCC extension, not in vanilla QuakeC": a construct the
  FTEQCC dialect adds (`dialect-fteqcc-rerelease.md`, FTE-2 to FTE-16) used under the vanilla
  profile, at the construct's keyword. The FTEQCC dialect accepts it; the vanilla dialect
  reports this error in its place for the keyword constructs (`for`, `switch`, `break`,
  `continue`), so the profile that rejects each extension is observable (P2-03). The operator
  and declaration extensions keep their vanilla diagnostic (QCD-2.5 "Unknown punctuation" for
  `%`, `?` and `:`; QCD-3.1 "not a name" for a stray `+`/`*` of `++`/`+=`; QCD-3.3 "NAME is
  not a type" for `const`, `var`, `nosave`, `typedef`, `enum` and a user type), which is also
  a diagnostic under the vanilla profile.

## QCD-7 Warnings

**Source:** this project. Each warning flags a construct id's `qcc` compiles whose meaning
differs from what it appears to say, most of them listed by id itself in the `qcc.h`
"gotchas" and notes. A warning never changes the emitted progs.

### Behaviour

- **QCD-7.1** `W0101` "frame name longer than 15 bytes; id's qcc cannot look it up"
  (QC-27.6).
- **QCD-7.2** `W0102` "macro NAME redefined" when a `#define` changes a macro's body (QC-9.3).
- **QCD-7.3** `W0201` "local NAME is the global NAME": a `local` or parameter whose name
  resolves to an earlier global (QC-12.3).
- **QCD-7.4** `W0202` "initialiser runs once, not on each call": a `local` with an
  initialiser (QC-11.10).
- **QCD-7.5** `W0301` "assigning to constant NAME": the left side of `=` is an initialised
  definition (QC-11.6, QC-27.10).
- **QCD-7.6** `W0302` "both calls return through the same global": an expression uses the
  results of two calls before either is stored (QC-18.6).
- **QCD-7.7** `W0303` "this call overwrites earlier arguments": a call inside an argument
  after the first (QC-18.5).
- **QCD-7.8** `W0304` "`=` binds tighter than `&&` and `||`": an unparenthesised `=` operand
  of `&&` or `||` (QC-15.3).
- **QCD-7.9** `W0305` "`!` applies to the whole comparison": `!` followed by an
  unparenthesised operand with a binary operator of priority 2 to 4 (QC-15.6).
- **QCD-7.10** `W0306` "`&` and `|` bind as tightly as `*`": `&` or `|` mixed with `+`, `-`
  or a comparison without parentheses (QC-15.2).
- **QCD-7.11** `W0307` "a vector condition tests only its x component" (QC-19.3).
- **QCD-7.12** `W0308` "return type does not match": `return e;` whose type differs from the
  function's return type, `return e;` in a `void` function, or `return;` in a non-`void`
  function (QC-13.8).
- **QCD-7.13** `W0309` "expression has no effect": an expression statement with no call and
  no assignment (QC-14.9).
- **QCD-7.14** `W0310` "`&&` and `||` evaluate both sides": the right operand of `&&` or `||`
  contains a call or assignment (QC-15.10).
- **QCD-7.15** `W0401` "exceeds a limit of id's qcc: LIMIT": the program needs more of one
  of `qcc`'s tables than it has (LIMIT-7.1, LIMIT-7.3), so it would not build reliably with
  id's `qcc`. This compiler is not bound by those limits.
- **QCD-7.16** `W0402` "NAME differs in case from the file it opens, STORED": a list entry,
  `#include` or `#includelist` entry opened a file whose stored name differs in case from the
  name, so a case-sensitive file system would not open it (PRJ-7.2).
- **QCD-7.17** `W0311` "NAME is builtin #N, which ENGINE does not have": a call of a builtin
  the project's target engine lacks, at the callee's name (ENGT-4.1).
- **QCD-7.18** `W0203` "Missing semicolon at end of definition": under an FTEQCC profile, a
  definition that is not a function ends without its `;`, FTEQCC's warning of the same text
  (`dialect-fteqcc-rerelease.md`, FTE-21.3). Under the vanilla profile it is "expected ;"
  (QCD-3.4).
- **QCD-7.19** `W0204` "Hanging ';'": under an FTEQCC profile, a `;` alone as a statement,
  FTEQCC's warning of the same text (`dialect-fteqcc-rerelease.md`, FTE-21.5).
- **QCD-7.20** `W0103` "#warning: TEXT": under an FTEQCC profile, a `#warning` on a taken
  side, FTEQCC's warning of the same text (`dialect-fteqcc.md`, FTEF-14.3).
- **QCD-7.21** `W0205` "missing comma in state definition": under an FTEQCC profile, a state
  header with no `,` between its frame and its next function, at the token where the `,`
  belongs, FTEQCC's warning of the same text (`dialect-fteqcc.md`, FTEF-18.3).
- **QCD-7.22** **Withdrawn** (P2-42, #429): it gave `W0312` for a label the function already
  has, but FTEQCC compares with the labels read since the last function's end, which a
  function defined inside another shares. Replaced by QCD-7.30.
- **QCD-7.23** `W0206` "incompatible redeclaration. Ignoring replacement of qc function with
  builtin.": under an FTEQCC profile, a builtin number given for a function that has a body, at
  the redeclared name, FTEQCC's warning of the same text (`dialect-fteqcc.md`, FTEF-17.9).
- **QCD-7.24** `W0104` "Case insensitive frame macro (using NAME)": under an FTEQCC profile, a
  frame macro found only by a name in another case, at the reference, FTEQCC's warning of the
  same text (`dialect-fteqcc.md`, FTEF-18.21).
- **QCD-7.25** `W0105` "Stale macro used (NAME, defined in FILE)": under an FTEQCC profile, a
  frame macro another reading of a file defined, at the use, FTEQCC's warning of the same text
  (`dialect-fteqcc.md`, FTEF-18.22).
- **QCD-7.26** `W0106` "Duplicate macro defined (NAME). Rename it.": under an FTEQCC profile, a
  frame name the same reading of a file already defined, at the name, FTEQCC's warning of the
  same text (`dialect-fteqcc.md`, FTEF-18.22).
- **QCD-7.27** `W0107` "$modelname name too long": under an FTEQCC profile, a `$modelname` of
  64 bytes or more, at the command, FTEQCC's warning of the same text (`dialect-fteqcc.md`,
  FTEF-18.22).
- **QCD-7.28** `W0108` "#pragma autoproto must appear before any definitions": under an FTEQCC
  profile, a `#pragma autoproto` after the first definition, at the directive, FTEQCC's warning
  of the same text (`dialect-fteqcc.md`, FTEF-12.10).
- **QCD-7.29** `W0313` "Statement does not do anything": under an FTEQCC profile, a discarded
  expression that calls, assigns and increments nothing, at the expression, FTEQCC's warning of
  the same text (`dialect-fteqcc.md`, FTEF-18.25).
- **QCD-7.30** `W0312` "Duplicate label NAME": under an FTEQCC profile, a label read since the
  end of the last function, at its name, FTEQCC's warning of the same text
  (`dialect-fteqcc.md`, FTEF-18.23).

## QCD-8 Preprocessor errors

**Source:** this project, for the rules in QC-9, which has no id source.

### Behaviour

- **QCD-8.1** `E0108` "unknown directive #NAME": a directive name the vanilla preprocessor
  does not know, outside a skipped block (QC-9.9).
- **QCD-8.2** `E0109` "#NAME without #ifdef or #ifndef": a `#else` or `#endif` with no open
  block (QC-9.6).
- **QCD-8.3** `E0110` "#ifdef or #ifndef without #endif": a block still open at the end of
  the file that opened it, at the directive that opened it (QC-9.6, QC-9.12).
- **QCD-8.4** `E0111` "cannot include PATH: REASON": an `#include` of a file that cannot be
  read (QC-9.11).
- **QCD-8.5** `E0112` "recursive #include of PATH": an `#include` of a file already being
  included (QC-9.11).
- **QCD-8.6** `E0113` "#include expects a file name in quotes" (QC-9.9).
- **QCD-8.7** `E0114` "#NAME expects a macro name": a `#define`, `#undef`, `#ifdef` or
  `#ifndef` without one (QC-9.9).
- **QCD-8.8** `E0115` "bad parameter list for macro NAME" (QC-9.9).
- **QCD-8.9** `E0116` "macro NAME expects N arguments, found M": a function-like macro use
  with the wrong argument count (QC-9.10).
- **QCD-8.10** `E0117` "unterminated argument list for macro NAME" (QC-9.10).
- **QCD-8.11** `E0118` "#Error: TEXT": under an FTEQCC profile, an `#error` on a taken side,
  FTEQCC's error of the same text (`dialect-fteqcc.md`, FTEF-14.3).
- **QCD-8.12** **Withdrawn** (P2-83, #494): it said `E0120` is reported under an FTEQCC profile
  only; the vanilla dialect has the bound too. Replaced by QCD-8.13.
- **QCD-8.13** `E0120` "macro expansion too large": the first macro use past the preprocessor's
  bound on the expansions of one file, under either dialect (`language.md` QC-9.18,
  `dialect-fteqcc.md` FTEF-14.28).
