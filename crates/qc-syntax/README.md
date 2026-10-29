# qc-syntax

The vanilla QuakeC parser. It turns one source file into a lossless concrete syntax tree:
every byte of the file is in exactly one token, comments, whitespace, directive lines,
`$` command lines and bytes the lexer skipped included, so the tree gives back the file byte
for byte. The parser is hand-written recursive descent over the grammar of id's `qcc`, and it
never fails: an error becomes a diagnostic and an error node, and parsing resumes at the next
statement or declaration. The tree has the two layers of rust-analyzer's `rowan` (an
immutable green tree and a red cursor over it, with byte tokens since QuakeC source has no
encoding), and `ast` gives a typed view of it.

The tree is of the file as written: macros are not expanded and both sides of a conditional
are parsed. Directive lines and `$frame` lines are trivia to the parser; frame macro
references are literals, resolved later. Under the vanilla dialect a directive line runs from a
`#` first on its line to the line break (QC-9.1); under an FTEQCC dialect it is the text the
preprocessor reads as one directive, found with `qc_preproc::fte_directive_end`, and the text
after it is lexed again as the preprocessor lexes it (`qc_preproc::relex_from`, FTEF-14.51),
all in the text FTEQCC's file loader gives, at the file's offsets: each NUL byte a space, and
the text after a UTF-8 byte order mark (`qc_preproc::qcc_load_in_place`, FTEF-14.56).

`parse` parses the vanilla dialect; `parse_with(source, dialect)` parses a chosen `Dialect`;
`parse_preprocessed(source, dialect)` parses text a preprocessor wrote, which has no directive
lines, so a `#` first on its line is a token (QC-9.19), and `parse_preprocessed_flagged` such
text lexed with the FTEQCC compiler flags the preprocessor marked (FTEF-12.12), and with the
seams between strings of different texts, which `QCC_PR_LexString` does not join (FTEF-21.17);
`parse_module_source` parses a module's source in source form, past a `#!` first line under an
FTEQCC dialect (PRJ-4.10). FTEQCC's `*^`, `><` and `<=>` parse as binary operators (FTEF-13.24).
An FTEQCC dialect also parses the FTEQCC constructs the Quake rerelease uses
(`docs/spec/dialect-fteqcc-rerelease.md`, FTE-2 to FTE-16): `for`, `switch`/`case`/`default`,
`break`/`continue`, postfix `++`/`--`, the compound assignments, `%`, the ternary `?:` at the
assignment priority (FTE-8.4),
`const`/`var`/`nosave`, arrays and subscripts, `typedef struct` and `enum`, named enums
(FTE-15.4), C-style function declarations and `#n:name`, `#"name"` and `:name` builtins
(FTE-17), read as FTEQCC's lexer reads them: a `#` only before `"`, `-` or a digit, no
number for a `d` literal, adjacent strings joined into one name of at most 255 bytes
(FTE-23.1 to FTE-23.6), a string with a prefix only after `:` or another string
(FTEF-13.7), names of up to 254 bytes (FTEF-15.7), `...` after named
parameters, default values and user-typed parameters (FTE-18), the bitwise and shift
operators, `->`, prefix `-`, `+` and `~`, `a -1` and casts (FTE-19), and locals without
`local`, field and function locals, empty blocks and empty statements (FTE-21, with the
warnings `W0203` and `W0204` of QCD-7.18 and QCD-7.19). Under the vanilla
dialect each keyword construct is rejected with `E0510` (QCD-6.10) and the operator and
declaration extensions keep their vanilla diagnostic, so the two profiles are observably
different; a vanilla program parses identically through `parse` or `parse_with(.., Vanilla)`.

The FTEQCC dialect also parses the full dialect of `docs/spec/dialect-fteqcc.md` (FTEF-2 to
FTEF-16), in `parser/fte.rs`: the C scalar types, pointers, sized and multi-dimensional arrays,
named structs and unions, typedefs of any type, `enumflags` and enums written as types,
classes with their members and `Class::member` definitions, accessors, `__variant`,
declaration and parameter qualifiers, nameless parameters, functions returning any type, casts
to any type, `[x, y, z]` vectors, scoped names, `if not`, prefix `++`/`--`/`&`/`*`, adjacent
strings and `case` labels in nested blocks; and FTEF-18: expression initialisers and function
bodies inside function bodies, state headers of expressions and frame cycles, sequences of
expressions, labels and `goto`, `return =`, `if !`, `__until` loops, case ranges, `?:` with no
middle, `sizeof`, functions returning functions and `..T` fields, with FTEQCC's "Duplicate
label" warning `W0312` and "Statement
does not do anything" warning `W0313` (FTEF-18.23, FTEF-18.25; QCD-7.29,
QCD-7.30). FTEQCC knows every type name when it parses; this
parser runs before names are known, so it tells a declaration or a cast from an expression by
the shape of the tokens (FTEF-15.1, FTEF-16.1, FTEF-16.2). Under `qccx` an `@` before a term is a unary
term (FTEF-19.26), and a parse keeps the compiler flags its tokens were lexed and read with
(`Parse::flags_at`, `Parse::lex_flags_at`), which a builtin's engine name is read with
(FTEF-19.23).

The flags change in the middle of a file (FTEF-21): the parser applies the preprocessor's
changes, `qc_preproc::FlagEdit`s, and its own `extern "C"` and `extern "QC"` blocks', which
save and restore six flags (`src/flags.rs`, ported from FTEQCC's `QCC_PR_ParseDefs`), and lexes
the text again with the blocks it read, reporting the lexical diagnostics of the tokens they
lex otherwise (FTEF-21.1 to FTEF-21.3). It reads expressions with the operator priorities the
flags give, QuakeC's or C's, and warns `W0227` of an operator the operand of `!` or `~` takes
(FTEF-21.4 to FTEF-21.6, QCD-7.68); with `qcfuncs` off it reads a declaration's `*`s and
parameter lists as each declarator's, C's function pointers and a first parameter that is a
lone name (FTEF-21.7, FTEF-21.8); it reads attribute lists after a qualifier, or with
`attributes` on at file level, with their warnings `W0228` (FTEF-21.9, QCD-7.69); and it reads
`typedef` among the qualifiers (FTEF-21.14).

Under an FTEQCC dialect the parser reads each keyword with FTEQCC's keyword flags in force at
its token (`docs/spec/dialect-fteqcc.md` FTEF-20): the marks `parse_preprocessed_flagged` is
given, from the preprocessor's `#pragma keyword`, `#pragma flag` and command line, and the
changes a definition named as a keyword makes with `kce` off (`Parse::keywords_at`). A `__`
spelling is every keyword, a bare word only one whose flag is on, in any case under
`caseinsens` (`qc_preproc::Keywords::checks`). A name FTEQCC registers as a type, or the program defined, starts a
definition of locals in the case `QCC_TypeForName` finds it in, any case under `caseinsens`
(FTEF-20.14). With their flags on, or written with `__`, the
statements `loop`, `until`, `state`, `thinktime`, `using`, `__exit` and `asm` parse, the last
reporting an instruction that names no opcode as `E0511` (QCD-6.11); and a type followed by `;`
defines nothing (FTEF-20.12).

Written from [`docs/spec/language.md`](../../docs/spec/language.md), not ported; the spec's
test vectors were recorded against id's `qcc` at the pinned commit, and the tests here check
the syntactic ones. The exceptions are `src/flags.rs`, ported from FTEQCC's `QCC_PR_ParseDefs`
(the flags an `extern` block saves, sets and restores, FTEF-21.17, FTEF-21.3), and
`src/c_scalar.rs`, ported from FTEQCC's
`QCC_PR_ParseType` and `QCC_PR_CheckKeyword` (FTEF-2.7 to FTEF-2.11): which C type words the
default keywords enable, the loop that reads them and the type they make, with the
"ignoring unsupported" warnings `W0226` (QCD-7.52). The parser reads a type's C words with it,
and `qc-sema` names the type with it.

- **Layer:** L2 syntax. It depends on `qc-lexer` and `qc-preproc`, whose FTEQCC directive reader
  ends a directive line.
- **Spec sections:** the syntax-error codes QCD-6 and the recovery rule QCD-1 of
  [`language-diagnostics.md`](../../docs/spec/language-diagnostics.md), and the parser's
  keywords FTEF-20.1, FTEF-20.2 and FTEF-20.5 to FTEF-20.13 of
  [`dialect-fteqcc.md`](../../docs/spec/dialect-fteqcc.md). These are the sections
  the parser owns in full, so a readiness run attributes only them to it and every active
  statement of each is cited by a test here.
- **Also cited here, in sections a higher layer completes:** the syntactic parts of QC-10,
  QC-11, QC-13, QC-14 and QC-15 in [`language.md`](../../docs/spec/language.md), the parser's
  divergences QC-27.3, QC-27.7, QC-27.8, QC-27.9, QC-27.10, QC-27.15 and QC-27.16, the FTEQCC
  profile's nesting (FTEF-18.32), and the parser error
  codes QCD-3.1 to QCD-3.4. The semantics, type rules and code generation of QC-10 to QC-15,
  and the redeclaration and resolution errors of QCD-3, belong to the semantic model and the
  compiler, so those sections are not declared above.
- **Shipped:** yes.

## Error recovery

Two rules, neither of which changes how a correct program parses:

- A declaration-shaped line at column 0 (a type word followed by a name, or by a parameter
  list and a name, or a `.`) cannot start a statement, so inside a function body it ends the
  body.
- After an error, tokens are skipped to the end of the statement or declaration (`;` at brace
  depth 0), to the `}` that closes the block, to a statement keyword starting a line, or to
  such a declaration line.

Only the first error of each statement or declaration is reported, as `qcc` reports one and
abandons the declaration (QC-24.2). Nesting is bounded (QC-27.15), the FTEQCC constructs
included (FTEF-18.32), and once the limit is passed nothing in the statement or declaration
nests deeper (QC-27.16), so no input exhausts the stack.

## Invariants

Each has a test in `src/tests/`:

- Parsing any bytes terminates and the tree's tokens tile the input exactly, under both
  profiles (property tests in `lossless.rs`).
- Deleting any single token of a correct file leaves every declaration before it, and every
  declaration from two after it on, parsed as before, with every error between them
  (`recovery::check_deletions` and, in a chosen dialect, `check_deletions_with`, run on
  `src/tests/sample.qc` and `src/tests/sample_fteqcc.qc`, on generated programs of both
  dialects, and by `cargo xtask parse-corpus --deletions` on id's QuakeC 1.01 and
  `--fteqcc --deletions` on the rerelease in CI).
- Every file of the corpus, FTEQCC codebases included, round-trips
  (`cargo xtask parse-corpus`).
- Parsing never panics or overflows the stack and round-trips every input, under the
  grammar-aware fuzz target `fuzz/fuzz_targets/parser.rs` (`cargo xtask fuzz parser`), which
  mutates whole tokens and nests brackets past the nesting limit.
