# L2 upstream inventory: from `progs.src` to tokens

Every function id's `qcc` and FTEQCC run from opening `progs.src`, or the file a module
compiles, to handing a token to the parser, with the C library calls whose behaviour matters,
and for each the Rust that models it, the spec statements that say what it does and the
differential cases that prove the two agree (P2-116). It is the checklist the L2 readiness
review (P2-R2) works from: no row may be unported or unproven.

The upstreams, at the commits [`../porting.md`](../porting.md#upstreams) pins:

- id's `qcc`: `qcc/qcc.c`, `qcc/cmdlib.c`, `qcc/pr_lex.c` and the entry of `qcc/pr_comp.c`
  (`PR_CompileFile`), id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`.
- FTEQCC: `engine/qclib/qccmain.c`, `qcc_cmdlib.c`, `qcc_pr_lex.c` and the entry of
  `qcc_pr_comp.c` (`QCC_PR_CompileFile`, `QCC_Include`), with `qcctui.c`'s file callbacks,
  fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`, built as `cargo xtask oracles`
  builds it and run as `fteqcc -Tid` in the directory of `progs.src`.

The parser's helpers (`PR_Expect`, `PR_Check`, `PR_ParseName`, `PR_FindType`, `PR_ParseType`,
`PR_SkipToSemicolon` and FTEQCC's `QCC_PR_CheckToken`, `QCC_PR_ParseName` and the rest) take
tokens rather than make them, and are P2-117's inventory (`inventory-l3.md`).

## How to read it

Each table row is one function. **Rust** names the item that models it. **Statements** lists
the statement IDs (or a section ID, `QC-6`, for a whole section, or a range `FTEF-14.35 to
FTEF-14.70`) of `docs/spec/` that say what it does, each cited by a test. **Differential**
names the case families of `cargo xtask oracles builtins` (`xtask/src/oracles/builtins.rs` and
`builtins/*.rs`) that compile the behaviour with the pinned compiler and compare the outcome
with this project's, or `none` and the reason. **Status** is `ported` (modelled and proven) or
`not modelled` (a deliberate divergence, whose statement gives the reason). The cross-crate test
`inventory` (`crates/qs-tests/tests/it/inventory.rs`) checks that every statement ID resolves to
an active statement, every named case family exists and no row is anything else.

## The lineage

FTEQCC's compiler descends from id's `qcc`; GMQCC was written independently. The evidence, from
the sources themselves:

- fteqw's history of `engine/qclib` starts at commit `7c00f2b1` (Spoike, 2004-08-23, "Initial
  Checkin"), which already holds `qcc_pr_lex.c`, `qcc_pr_comp.c`, `qcc_cmdlib.c` and
  `QccMain.c`. Its `QCC_PR_LexName` is id's `PR_LexName` statement for statement, renamed:
  id's `c = *pr_file_p; do { pr_token[len] = c; len++; pr_file_p++; c = *pr_file_p; } while (
  (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || (c >= '0' && c <= '9'));
  pr_token[len] = 0; pr_token_type = tt_name;` is the 2004 body with only white space changed.
  `QCC_PR_LexPunctuation`, `QCC_PR_SimpleGetToken` and `QCC_PR_PrintNextLine` match id's
  likewise, and `qcc_cmdlib.c` keeps id's `DefaultExtension`, `DefaultPath`, `StripFilename`,
  `StripExtension`, `ExtractFilePath`, `ExtractFileBase`, `ExtractFileExtension` and `ParseNum`
  under their own names with their bodies unchanged. Every function of id's `pr_lex.c` has a
  `QCC_`-prefixed counterpart in that commit, and id's globals keep their names and comments
  (`char *pr_line_start; // start of current source line`, `type_t type_void = {ev_void,
  &def_void};` left as the comment `// = {ev_void/*, &def_void*/};`).
- The 2004 files carry no copyright notice; fteqw's `README.md` says "FTEQW is supplied to you
  under the terms of the same license as the original Quake sources", and `engine/qclib/LICENSE`
  is the GPL-2.0 text ([`../porting.md`](../porting.md#upstreams)).
- GMQCC (graphitemaster/gmqcc, first commit `48a95ec`, Dale Weiler, 2012-04-09, "initial
  commit") is MIT-licensed (`LICENSE`: "Copyright (C) 2012, 2013, 2014, 2015 Dale Weiler,
  Wolfgang Bumiller"), and none of its sources holds id's function or global names (`COM_Parse`,
  `PR_Lex`, `pr_file_p`, `LoadFile`, `PR_ParseDefs`) or the text "id software". It shares no
  code with id's `qcc`; this inventory has no GMQCC rows, and the GMQCC dialect reads its files
  as the FTEQCC profile does until its own profile (PRJ-5.15).

So each FTEQCC row below that pairs with an id function is the behavioural difference between
the two bodies: what the FTEQCC profile adds on top of the vanilla port, which is proven against
id's `qcc`. FTEQCC's functions with no id counterpart follow, all of whose behaviour is new, and
then id's functions FTEQCC dropped.

## The run this project models

id's `qcc` is run with no arguments in the directory of `progs.src`; FTEQCC as `fteqcc -Tid`
there, with a `-I` for each of the module's include directories and a `-D` for each of its
defines (FTEF-14.48, FTEF-14.49). A module's `quakec.toml` entry gives no other option, so the
rest of each command line is not modelled (PRJ-3.4).

## id's `qcc`

| Function | File | On the path | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `main` | `qcc.c` | Opens `progs.src`, reads the output and then each listed file with `COM_Parse`, compiles them in order and stops after the first file with an error | `qc_project::build_graph` (list form) | QC-23.1, QC-23.3, QC-23.4, QC-23.5, PRJ-4.1, PRJ-4.12, QC-24.5 | `VANILLA_PROGS`, `cases` | ported |
| `LoadFile` | `cmdlib.c` | Reads a whole file and writes a NUL after it, so the text ends at its first NUL | `qc_lexer::lex_with`, `qc_project` `progs_text`, `Cursor::vanilla` | QC-1.1, QC-23.7, PRJ-4.11, PRJ-5.16 | `VANILLA_PROGS`, `cases` | ported |
| `SafeOpenRead`, `filelength`, `SafeRead`, `SafeMalloc`, `Error` | `cmdlib.c` | A file that cannot be opened is fatal, "Error opening %s: %s" | `qc_project::build_graph`, `qc_preproc::Loader` | QCD-5.2, PRJ-4.5, PRJ-6.6 | `VANILLA_PROGS` | ported |
| `COM_Parse` | `cmdlib.c` | Splits `progs.src` into words: white space is a byte `<= ' '` as a signed `char`, so 0x80 to 0xFF too; `//` comments; quoted words, which run across lines; `{ } ( ) ' :` alone | `qc_project` `com_parse_words`, `com_parse_end` | QC-23.2, QC-23.7, QC-23.8, QC-23.9, PRJ-4.11 | `VANILLA_PROGS` | ported |
| `CheckParm` | `cmdlib.c` | `-src`, `-asm`, `-copy`, `-pak`, `-bspmodels`, `-help` | none | PRJ-3.4 | none: the command line is not modelled | not modelled |
| `PR_BeginCompilation` | `qcc.c` | Sets up the program's tables for code generation; no part in reading the files | none | PRJ-3.4 | none: the compiler's | not modelled |
| `PR_CompileFile` | `pr_comp.c` | Clears the frame macros, counts the first line, lexes the first token, then parses declarations, recovering after an error | `qc_preproc::Preprocessor`, `qc_syntax::parse_with` | QC-8.3, QC-1.2, QC-24.2, QC-24.7 | `cases` | ported |
| `PR_ClearGrabMacros` | `pr_lex.c` | Forgets every frame macro at a file's start | `qc_preproc::Preprocessor` (frame list per file) | QC-8.3 | `frame_cases` | ported |
| `PR_NewLine` | `pr_lex.c` | Counts a line at each LF | `qc_lexer::LineIndex` | QC-1.2, QC-1.5 | `line_pragma_cases` | ported |
| `PR_LexString` | `pr_lex.c` | `"..."` with `\n` and `\"`; "EOF inside quote", "newline inside quote", "Unknown escape char" | `qc_lexer` `Lexer::string` | QC-6, QCD-2.1, QCD-2.2, QCD-2.3 | `VALUES` | ported |
| `PR_LexNumber` | `pr_lex.c` | The first byte, then digits and `.`, read with `atof` | `qc_lexer` `Lexer::number` | QC-4.1, QC-4.2, QC-4.3, QC-4.4 | `VALUES` | ported |
| `PR_LexVector` | `pr_lex.c` | `'x y z'`, three numbers with white space between; "Bad vector" | `qc_lexer` `Lexer::vector` | QC-5, QCD-2.4 | `VALUES` | ported |
| `PR_LexName` | `pr_lex.c` | ASCII letters, `_` and digits | `qc_lexer` `Lexer::name` | QC-3.1 | `VALUES`, `NAME_LENGTHS` | ported |
| `PR_LexPunctuation` | `pr_lex.c` | id's `pr_punctuation` table, first match; "Unknown punctuation" | `qc_lexer` `Lexer::punctuation`, `Punct::ALL` | QC-7.1, QC-7.2, QCD-2.5 | `VALUES` | ported |
| `PR_LexWhitespace` | `pr_lex.c` | Bytes `<= ' '` as a signed `char`, so 0x80 to 0xFF too; `//` and `/* */` comments | `qc_lexer` `Lexer::skip_trivia`, `is_whitespace` | QC-1.3, QC-2 | `VALUES`, `LINES` | ported |
| `PR_FindMacro`, `PR_ParseFrame`, `PR_LexGrab`, `PR_SimpleGetToken` | `pr_lex.c` | `$frame` names and references, the ignored `$` commands, a word up to white space, `,` or `;` | `qc_lexer` `Lexer::dollar`, `qc_preproc` frame list | QC-8, QCD-2.6, QCD-2.7 | `frame_cases` | ported |
| `PR_Lex` | `pr_lex.c` | Picks the token by its first byte: `"`, `'`, a digit or `-` and a digit, a letter or `_`, `$`, else punctuation | `qc_lexer` `Lexer::run` | QC-1, QC-4.1, QC-4.2 | `VALUES` | ported |
| `PR_ParseError` | `pr_lex.c` | Prints `file:line:message` and abandons the declaration | `qc_diag::Diagnostic` | QCD-1.3, QCD-1.4, QC-24.1 | `cases` | ported |

## FTEQCC's counterparts of id's functions

| id | FTEQCC | What FTEQCC adds | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `main` | `QCC_main`, `QCC_ContinueCompile` | `progs.src` loaded with `QCC_LoadFile`; a `#!` first line passed over; source form when the first `QCC_COM_Parse` word starts with `#`; in list form the output read with `QCC_PR_LexWhitespace` and `QCC_PR_SimpleGetToken`, each entry with `QCC_PR_LexWhitespace` (which runs directives) and `QCC_COM_Parse`, found with `QCC_FindBestInclude`; a list with no word no error | `qc_project::build_graph`, `qc_preproc::FteFileReader::progs`, `skip_script_line` | PRJ-4.6, PRJ-4.9, PRJ-4.10, PRJ-5.13, FTEF-12.10 | `LOADED_PROGS`, `WIDE_PROGS`, `SCRIPT_PROGS`, `FTE_LIST_PROGS`, `progs_cases` | ported |
| `LoadFile` | `QCC_LoadFile`, `QCC_LoadFileHunk` | Decodes a UTF-16 or UTF-32 file after its mark, skips a UTF-8 mark, makes each NUL a space ("file contains null bytes"), writes a line feed after the text | `qc_preproc` `qcc_load_file`, `qcc_load_in_place` | FTEF-14.64, FTEF-14.56, PRJ-4.6 | `LOADED`, `wide_cases`, `LOADED_PROGS`, `WIDE_PROGS` | ported |
| `SafeOpenRead`, `filelength`, `SafeRead` | `QCC_ReadFile`, `QCC_FileSize` (`qcctui.c`) | A file that cannot be read is fatal "Couldn't open file %s" | `qc_preproc::Loader`, `qc_project::build_graph` | QCD-5.2, PRJ-4.5, PRJ-6.6 | `include_cases`, `FTE_LIST_PROGS` | ported |
| `COM_Parse` | `QCC_COM_Parse` | White space is `qcc_iswhite` (space, CR, LF, tab, VT); `/* */` comments; `,` a word of its own and the end of a word, as is `"`; a quoted word takes `\"`, `\\`, `\n`, `\r` and `\t`, ends at a NUL and at a line break after an escaped quote; a word keeps 1023 bytes | `qc_preproc::com_parse` | PRJ-4.9, FTEF-14.66, FTEF-14.70 | `FTE_LIST_PROGS`, `PRAGMAS` | ported |
| `PR_CompileFile` | `QCC_PR_CompileFile`, `QCC_Include` | `compilingfile` and `s_filen` set; frame macros kept across files and only renumbered; an `#include`d file read as a chunk of the including file, an `#includelist` entry compiled as a unit of its own | `qc_preproc::Preprocessor`, `qc_project::build_graph` | FTEF-14.32, FTEF-14.40, FTEF-14.52, FTEF-18.17, FTEF-18.29, PRJ-5.11 | `include_cases`, `INCLUDE_LISTS` | ported |
| `PR_ClearGrabMacros` | `QCC_PR_ClearGrabMacros` | `newfile` keeps the frame names and resets only the numbering | `qc_preproc` frame list | FTEF-18.16, FTEF-18.17 | `frame_cases` | ported |
| `PR_NewLine` | `QCC_PR_NewLine` | Skips spaces and tabs and runs `QCC_PR_Precompiler` at a `#`, unless in a comment | `qc_preproc` `FteReader` | FTEF-14.52, FTEF-14.45 | `place_cases`, `snippet_cases` | ported |
| `PR_LexString` | `QCC_PR_LexString`, `QCC_PR_LexEscapedCodepoint` | Joins adjacent strings; `R"`, `Q"`, `u"`, `U"`, `L"`, `u8"` prefixes; every escape; `\s` and `\b` bronze; UTF-8 checking; "Hanging \\" before a lone CR; the `qcfuncs`, `qccx`, `utf8` and `macroinstrings` flags, the `acc` flag only from the command line; 8191 bytes at most | `qc_lexer` `fteqcc_string_bytes`, `Lexer::fte_string`, `Lexer::string_macro` | FTEF-13.7, FTEF-13.8, FTEF-13.9, FTEF-13.10, FTEF-13.11, FTEF-13.20, FTEF-13.21, FTEF-13.22, FTEF-13.25, FTEF-12.12, FTEF-12.14, FTE-20.4, FTE-23.5 | `VALUES`, `LINES`, `BYTE_LINES`, `LENGTHS`, `FLAG_LINES`, `FLAG_BYTE_LINES` | ported |
| `PR_LexNumber` | `QCC_PR_LexNumber`, `QCC_PR_LexInteger`, `QCC_PR_LexFloat` | `0x` and octal bases, `.` fractions, type suffixes, a leading `.`; "A leading 0 is interpreted as base-8.", "numerical overflow", "bad suffix on number"; the `assumeint`, `ILP32` and `qccx` flags; `assumedouble`'s type, the compiler's | `qc_lexer` `Lexer::fteqcc_number`, `integer_suffix` | FTEF-3.1, FTEF-3.2, FTEF-3.3, FTEF-13.5, FTEF-13.6, FTEF-13.17, FTEF-13.19, FTEF-12.12, FTEF-12.14, FTE-20.1, FTE-20.2, FTE-23.2 | `VALUES`, `LINES`, `NUMBERS`, `FLAG_LINES` | ported |
| `PR_LexVector` | `QCC_PR_LexVector` | Character constants, two-component vectors, `QCC_PR_LexFloat` components, the `qccx` character constants | `qc_lexer` `Lexer::vector`, `Lexer::character` | FTEF-13.3, FTEF-13.6, FTEF-13.13, FTEF-13.14, FTEF-13.23, FTEF-12.12 | `VALUES`, `CHARACTER_ESCAPES`, `FLAG_LINES` | ported |
| `PR_LexName` | `QCC_PR_LexName` | A byte of 0x80 or more starts and continues a name, read as UTF-8 by `utf8_check`; an invalid sequence is "Invalid UTF-8 code sequence" | `qc_lexer` `Lexer::fte_name` | FTEF-13.16, QCD-2.9 | `LINES`, `BYTE_LINES` | ported |
| `PR_LexPunctuation` | `QCC_PR_LexPunctuation` | FTEQCC's longer table and its remapping (`->` to `.`, `(+)` to `|=`, `(-)` to `&~=`); `\` and a line break join lines; `**` under the `ssp` flag; "Unknown punctuation: '\x%x'" | `qc_lexer` `Lexer::punctuation`, `Punct::FTEQCC_TABLE` | FTEF-13.1, FTEF-13.2, FTEF-13.15, FTEF-13.24, FTEF-14.53, FTEF-12.12, FTE-5, FTE-6, FTE-7, FTE-8, FTE-19 | `VALUES`, `JOINED`, `LINES`, `FLAG_LINES` | ported |
| `PR_LexWhitespace` | `QCC_PR_LexWhitespace`, `QCC_PR_LexComment` | White space is `qcc_iswhite` only, so a control byte or a byte of 0x80 or more is no white space; a lone CR ends a line; each line ending runs `QCC_PR_NewLine`; "EOF inside comment", "\"/*\" inside comment" | `qc_lexer` `Lexer::skip_trivia`, `qc_preproc` `fte_whitespace`, `whitespace` | FTEF-13.15, FTEF-13.18, FTEF-14.45, FTEF-14.52, FTEF-14.67, QCD-7.34 | `LINES`, `place_cases`, `word_bound_cases` | ported |
| `PR_FindMacro`, `PR_ParseFrame`, `PR_LexGrab`, `PR_SimpleGetToken` | `QCC_PR_FindMacro`, `QCC_PR_ParseFrame`, `QCC_PR_LexGrab`, `QCC_PR_LexMacroName`, `QCC_PR_MacroFrame`, `QCC_PR_ExpandMacro`, `QCC_PR_SimpleGetToken` | `$framesave`, `$flush`, `$frame_reset`, `$framevalue`, `$framerestore`, `$modelname`; case-insensitive and stale lookups; names up to 63 bytes and 8192 macros; a word of at most 1023 bytes ("token exceeds 1023 chars") | `qc_lexer` `Lexer::dollar`, `macro_name`, `qc_preproc` frame list, `simple_get_token` | FTEF-18.13, FTEF-18.14, FTEF-18.16, FTEF-18.21, FTEF-18.29, FTEF-18.30, FTEF-14.66 | `frame_cases`, `FRAME_VALUES`, `FRAME_USES`, `word_bound_cases` | ported |
| `PR_Lex` | `QCC_PR_Lex` | Macros expanded at each name and after `#` (`QCC_PR_CheckCompConst`), only after `#` under `hashonly`; `%5` under `qccx`; a string's prefixes; chunks ended with `QCC_PR_UnInclude`; each token read with the flags of its moment | `qc_preproc::Preprocessor`, `Lexing`, `qc_lexer` `Lexer::run` | FTEF-14.13, FTE-23.10, FTEF-14.17, FTEF-12.12, FTEF-12.13 | `cases`, `FLAG_LINES` | ported |
| `PR_ParseError` | `QCC_PR_ParseError`, `QCC_PR_ParseWarning`, `QCC_PR_Warning`, `QCC_PR_PrintWarning`, `QCC_Error` | Warnings, each ignored, warned or an error as `qccwarningaction` and `#pragma warning` set it; an `ERR_` code given to `QCC_PR_ParseWarning` is an error; `QCC_Error` ends the compile | `qc_diag::Diagnostic`, `qc_preproc::WarningSettings` | QCD-8.14, QCD-8.17, FTEF-14.46, FTEF-14.63, FTE-16 | `evil_cases`, `EVIL_SETTINGS`, `FLAG_LINES` | ported |

## FTEQCC's functions with no id counterpart

| Function | File | Behaviour | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `QCC_SanitizeCharSet`, `decodeUTF`, `utf8_check` | `qcc_cmdlib.c` | Byte order marks and wide encodings; UTF-8 sequences | `qc_preproc` `decode_utf`, `qc_lexer` `utf8_check` | FTEF-14.64, FTEF-13.8, FTEF-13.14 | `wide_cases`, `WIDE_PROGS` | ported |
| `QCC_COM_Parse2` | `qcc_cmdlib.c` | A macro body's tokens for `#param` and argument copying | `qc_preproc` `parse2` | FTEF-14.20, FTEF-14.25, QC-9.16 | `parse2_uses` | ported |
| `QCC_SetDefaultProperties`, `QCC_PR_CommandLinePrecompilerOptions`, `QCC_PR_CloseProcessor` | `qccmain.c`, `qcc_pr_lex.c` | `FTEQCC` and `__FTEQCC__` defined; `-I` and `-D`; warning settings; flags and keywords at their defaults | `qc_preproc` `fte_default_properties`, `FlagSettings` | FTEF-14.48, FTEF-14.49, FTEF-12.11, PRJ-3.4 | `DIR_PROJECTS` | ported |
| `QCC_PR_AddIncludePath` | `qcc_pr_lex.c` | A `/` added; at most 8 directories ("Too many include dirs"); 2048 bytes ("Include path too long."); an empty name `.`; one already listed kept once | `qc_preproc` `add_include_dir` | FTEF-14.70, FTEF-14.49, FTEF-14.68 | `DIR_PROJECTS`, `word_bound_cases` | ported |
| `QCC_JoinPaths`, `QCC_FindBestInclude` | `qcc_pr_lex.c` | Joined after the last `/` or `\` of the file being compiled, then each include directory; none is fatal; `#pragma once` marks compared | `qc_preproc` `join_paths`, `include_candidates` | FTEF-14.40, FTEF-14.32, PRJ-5.11 | `include_cases` | ported |
| `QCC_PR_IncludeChunkEx`, `QCC_PR_IncludeChunk`, `QCC_PR_UnInclude` | `qcc_pr_lex.c` | A macro's expansion and an included file read as chunks, with `s_filen` and the line restored after | `qc_preproc` chunks | FTEF-14.16, FTEF-14.17, FTEF-14.43 | `macro_text_cases` | ported |
| `QCC_PR_Precompiler` | `qcc_pr_lex.c` | Every directive and pragma, its `msg[1024]` among them | `qc_preproc` `fte_directive`, `fte_pragma` | FTEF-12, FTEF-14.35 to FTEF-14.70, FTEF-14.52 | `NAME_FORMS`, `SNIPPETS`, `PRAGMAS`, `pragma_cases`, `word_bound_cases` | ported |
| `ParsePrecompilerIf` | `qcc_pr_lex.c` | `#if` and `#elif` expressions | `qc_preproc::cond` `fte_eval` | FTEF-14.33, FTEF-14.12, FTEF-14.55, FTEF-14.66 | `CONDITIONALS`, `word_bound_cases` | ported |
| `QCC_PR_SkipToEndOfLine` | `qcc_pr_lex.c` | A directive's line end, with `errorifnonwhite` | `qc_preproc` `skip_to_end_of_line` | FTEF-14.35 | `LINE_DIRECTIVES`, `line_end_cases` | ported |
| `QCC_PR_FalsePreProcessorIf` | `qcc_pr_lex.c` | Skipping a side | `qc_preproc` `false_side` | FTEF-14.37, FTEF-14.1 | `CONDITIONALS` | ported |
| `QCC_PR_SimpleGetString` | `qcc_pr_lex.c` | `#output`, `#merge` and `#datafile` strings; "new line inside string" and "token exceeds 1023 chars" | `qc_preproc` `simple_get_string` | FTEF-14.39, FTEF-14.66 | `NAME_FORMS`, `word_bound_cases` | ported |
| `QCC_PR_DefineName`, `QCC_PR_UndefineName`, `QCC_PR_PreProcessor_Define` | `qcc_pr_lex.c` | `#define`, `#append`, `#undef`, `-D`; their errors and warnings | `qc_preproc` `fte_define`, `read_define`, `read_params` | FTEF-14.5, FTEF-14.23, FTEF-14.24, FTEF-14.57, FTEF-14.58, FTEF-14.42, FTEF-14.62, FTEF-14.66 | `name_length_cases`, `OTHER_MACROS`, `word_bound_cases` | ported |
| `QCC_PR_CheckCompConst`, `QCC_PR_ExpandPreProcessorMacro`, `QCC_PR_ExpandStrCat`, `QCC_PR_ExpandStrCatMarkup`, `QCC_PR_CheckCompConstDefined`, `QCC_PR_CheckCompConstString` | `qcc_pr_lex.c` | Macro uses, arguments, `#param`, `##`, `__VA_ARGS__`, `__VA_COUNT__`, nesting of 8; the `hashonly` and `macroinstrings` flags | `qc_preproc` `fte_try_expand`, `fte_scan_arguments`, `qc_lexer` `Lexer::string_macro` | FTEF-14.9, FTEF-14.13, FTEF-14.14, FTEF-14.16, FTEF-14.17, FTEF-14.20, FTEF-14.22, FTEF-14.25, FTEF-14.26, FTEF-14.28, FTEF-14.29, FTEF-14.31, FTEF-12.12, FTEF-12.13 | `MACRO_BODIES`, `argument_uses`, `variadic_uses`, `body_uses`, `parameter_uses`, `FLAG_LINES` | ported |
| `QCC_PR_CheckBuiltinCompConst`, `QCC_CurrentTime`, `QCC_PR_PopenMacro` | `qcc_pr_lex.c` | `__FILE__`, `__LINE__`, `__LINESTR__`, `__FUNC__`, `__NULL__`, `__TIME__`, `__DATE__`, `__RAND__`, `__QCCVER__`; the `__GIT...__` constants, which the pinned build does not compile | `qc_preproc` `fte_builtin`, `builtin_text` | FTEF-14.7, FTEF-14.27, FTEF-14.32 | `line_uses`, `file_sources` | ported |
| `QCC_PR_LexComment` | `qcc_pr_lex.c` | Comments before a token kept for documentation; reads white space as `QCC_PR_LexWhitespace` | `qc_lexer` `Lexer::skip_trivia` | FTEF-14.45, FTEF-13.18 | `place_cases`, `LINES` | ported |
| `QCC_RegisterSourceFile` | `qccmain.c` | `#pragma sourcefile` and `-srcfile` programs, each once | `qc_project` `sourcefiles` | FTEF-12.3, PRJ-5.7 | none: the graph's unit tests hold it, as FTEQCC compiles the registered programs one after another | ported |
| `StartNewStyleCompile`, `new_QCC_ContinueCompile` | `qccmain.c` | The source form: `progs.src` read as QuakeC from its first line | `qc_preproc::Preprocessor`, `qc_project`, `qc_syntax::parse_module_source` | PRJ-4.4, PRJ-4.10, PRJ-5.13 | `progs_cases` | ported |
| `QCC_GenerateRelativePath` | `qccmain.c` | A list's output name joined to `progs.src`'s directory | `qc_project` `declared_output` | PRJ-4.12, PRJ-3.2 | none: the declared output changes nothing the graph reads (PRJ-3.2) | ported |
| `QCC_FindQCFiles` | `qccmain.c` | `-Facc` with no `progs.src`: every `.qc` in the directory | none | PRJ-3.4 | none: the command line is not modelled | not modelled |
| `QCC_ImportProgs`, `QCC_AddFile` | `qccmain.c`, `qcc_cmdlib.c` | `#merge` and `#datafile` read a file into the build | none | FTEF-14.39 | none: the build's files | not modelled |
| `QCC_WarningForName`, `QCC_NameForWarning` | `qccmain.c` | Warning names (`F303`, `Q120`, `octal`) for `#pragma warning` | `qc_preproc::FteWarning` | FTEF-14.63, FTEF-14.46 | `EVIL_SETTINGS` | ported |
| `QC_strcasecmp`, `QC_strlcpy`, `QC_strlcat`, `QC_strnlcpy`, `Q_strlcpy` | `qcc_cmdlib.c`, `qcc_pr_lex.c` | ASCII case-insensitive pragma names; truncation of copies | `qc_preproc` pragma matching | FTEF-14.43 | `PRAGMAS` | ported |

## id's functions FTEQCC dropped or replaced

| id | Replaced by | Status |
| --- | --- | --- |
| `PR_PrintNextLine` | `QCC_PR_PrintNextLine`, unused on the path in both: id calls it only from a commented-out `pr_dumpasm` line in `PR_NewLine` | not modelled |
| `PR_LexWhitespace` before each token | `QCC_PR_LexComment`, which reads comments for documentation and then calls `QCC_PR_LexWhitespace` | ported |
| `CheckParm` | `QCC_CheckParm` and `QCC_ReadParm` (PRJ-3.4) | not modelled |

## C library calls

| Call | Where | Behaviour that matters | Rust | Statements | Differential | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `atof` | `PR_LexNumber`, `QCC_PR_LexNumber`, `QCC_PR_LexFloat` | Decimal and, after `0x`, hexadecimal floats, rounded to a `double` then a `float` | `qc_lexer::atof`, `hex_float` | QC-4.1, QC-4.3, FTE-23.2 | `VALUES` | ported |
| `atoi` | `ParsePrecompilerIf`, `QCC_PR_LexInteger`, pragmas, `$framevalue` | `(int)strtol(s, NULL, 10)` of an LP64 host | `qc_preproc::cond::fte_atoi`, `qc_lexer` `c_atoi` | FTEF-14.55, FTEF-12.12 | `CONDITIONALS`, `FRAME_VALUES`, `FLAG_LINES` | ported |
| `strtoul` | `#pragma line` | Base 0, a sign, saturation; low 32 bits kept | `qc_preproc` `strtoul` | FTEF-14.43, FTEF-14.50 | `LINE_PRAGMAS` | ported |
| `strtoull` | `QCC_CurrentTime` | `SOURCE_DATE_EPOCH` | none | FTEF-14.7 | none: the time of the build | not modelled |
| `atof` | `#pragma framerate` | The model's frame rate | none | FTEF-14.70 | none: the build's | not modelled |
| `strcmp`, `strncmp`, `strchr` | directive names, pragma arguments | Exact byte comparison; a directive's name matched by its first bytes | `qc_preproc` `fte_directive` | FTEF-14.52 | `NAME_FORMS` | ported |
| `qcc_iswhite`, `qcc_iswhitesameline`, `qcc_islineending` | everywhere | Space, CR, LF, tab, VT; space and tab; LF or a CR not before a LF | `qc_lexer::is_fteqcc_whitespace`, `qc_preproc` `blank`, `line_ending` | FTEF-13.15, FTEF-14.13, FTEF-14.52 | `place_cases`, `LINES` | ported |
| `tolower`, `stricmp`, `strnicmp` | `QC_strcasecmp`, `QCC_PR_FindMacro`, pragma states | ASCII case folding in the C locale | `qc_preproc` | FTEF-14.43, FTEF-18.21 | `PRAGMAS`, `FRAME_USES` | ported |
| `rand` | `__RAND__` | A number | `qc_preproc` `builtin_text` | FTEF-14.7 | none: not reproducible | not modelled |

## Diagnostics on FTEQCC's path

Every `QCC_PR_ParseError`, `QCC_PR_ParseWarning`, `QCC_PR_Warning` and `QCC_Error` FTEQCC can
reach from `progs.src` to a token. **Default** is what `fteqcc -Tid` does with it:
`error` (an `ERR_` code, or `QCC_Error`, which ends the compile), `warning` (on by default) or
`ignored` (off by default, `QCC_SetDefaultProperties`). **Profile** is this project's code, or
`none` and the statement that says why.

| Site | Message | Default | Profile | Statements | Status |
| --- | --- | --- | --- | --- | --- |
| `QCC_PR_AddIncludePath` | "Include path too long." | warning | none: only a `-I` option reaches it, printed against "cmdline" | FTEF-14.68 | not modelled |
| `QCC_PR_AddIncludePath` | "Too many include dirs. Ignoring and hoping the stars align." | warning | W0117 | FTEF-14.68, QCD-7.39 | ported |
| `QCC_FindBestInclude` | "Couldn't open file %s" | error | E0111, E0402 | FTEF-14.40, PRJ-4.9, PRJ-6.6 | ported |
| `ParsePrecompilerIf` | "unclosed bracket condition", "unclosed defined condition", "unexpected end-of-line" | error | E0121 | FTEF-14.33 | ported |
| `QCC_PR_SkipToEndOfLine` | "unexpected tokens at end of line" | error | E0121 | FTEF-14.35 | ported |
| `QCC_PR_FalsePreProcessorIf` | "#if with no endif", "malformed #elif" | error | E0110, E0121 | FTEF-14.37 | ported |
| `QCC_PR_Precompiler` | "Hanging # with no directive" | error | E0121 | FTEF-14.52 | ported |
| `QCC_PR_Precompiler` | "Unknown pragma '%s'" (an `#if` form) | warning | W0113 | FTEF-14.57 | ported |
| `QCC_PR_Precompiler` | "#else outside of #if", "unmatched #endif" | error | E0109 | FTEF-14.37 | ported |
| `QCC_PR_Precompiler` | "#Error: %s" | error | E0118 | FTEF-14.38 | ported |
| `QCC_PR_Precompiler` | "#warning: %s" | warning | W0103 | FTEF-14.38 | ported |
| `QCC_PR_Precompiler` | "Copyright message is too long" | warning | none: it cannot be reached | FTEF-14.69, FTEF-14.70 | ported |
| `QCC_PR_Precompiler` | "No more than 5 packs are allowed" | error | E0121 | FTEF-14.39 | ported |
| `QCC_PR_Precompiler` | "eof in includelist" | error | E0406 | FTEF-14.41 | ported |
| `QCC_PR_Precompiler` | "Not a string literal (on a #include)", "#include continued over line boundry" | error | E0113 | FTEF-14.40 | ported |
| `QCC_PR_Precompiler` | "#pragma autoproto must appear before any definitions" | warning | W0108 | FTEF-12.10 | ported |
| `QCC_PR_Precompiler` | the `optimise`, `opcode`, `TARGET`, `keyword`, `flag` and `warning` pragma warnings | warning | W0113, W0117 | FTEF-12.1, FTEF-12.11, FTEF-14.63 | ported |
| `QCC_PR_Precompiler` | "Unknown pragma '%s'" | warning | W0113 | FTEF-12.11, FTEF-14.70 | ported |
| `QCC_PR_Precompiler` | a `#pragma` name or text past `msg[1024]`, which the pinned build aborts at | error | E0121 | FTEF-14.66 | ported |
| `QCC_PR_LexEscapedCodepoint` | "EOF inside quote", "Bad character code", "Unknown escape char %c", "Unicode character terminated unexpectedly" | error | E0101, E0103 | FTEF-13.11, FTEF-13.13, FTEF-12.12 | ported |
| `QCC_PR_LexEscapedCodepoint` | "Hanging \\" and a CR | warning | W0123 | FTEF-13.25, QCD-7.45 | ported |
| `QCC_PR_LexString` | "EOF while parsing raw string delimiter", "EOF inside quote", "newline inside quote" | error | E0101, E0102 | FTEF-13.9, QCD-2.1, QCD-2.2 | ported |
| `QCC_PR_LexString` | "char32_t/char16_t/wchar_t strings are not supported, ..." | ignored | W0119 | FTEF-13.8, FTEF-13.22 | ported |
| `QCC_PR_LexString` | "Input string is not valid utf-8", "Unable to mask non-ascii chars. Attempting to mask bytes" | error | E0103 | FTEF-13.10, FTEF-13.21, FTEF-12.12 | ported |
| `QCC_PR_LexString` | "Cannot convert codepoint %#x to quake's charset", "Bad unicode character code - codepoint is above 0x10FFFFu" | ignored | W0119 | FTEF-13.8, FTEF-13.22 | ported |
| `QCC_PR_LexString` | "String length exceeds %u" | error | E0502 | FTEF-13.20, FTEF-12.12 | ported |
| `QCC_PR_LexString` | "Unable to expand string macro %s", "Macro %s expansion in string" | ignored | W0122 | FTEF-12.12, QCD-7.44 | ported |
| `QCC_PR_LexNumber` | "A leading 0 is interpreted as base-8." | warning | W0110 | FTEF-13.19, QCD-7.32 | ported |
| `QCC_PR_LexNumber` | "numerical overflow" | warning | W0111 | FTEF-13.19, FTEF-12.12, QCD-7.33 | ported |
| `QCC_PR_LexNumber` | "bad suffix on number %s" | error | E0124 | FTEF-13.17, QCD-2.10 | ported |
| `QCC_PR_LexVector` | "Bad character constant", "Bad vector" | error | E0104 | FTEF-13.13, QCD-2.4 | ported |
| `QCC_PR_LexVector` | "2d vector" | ignored | W0120 | FTEF-13.3, QCD-7.42 | ported |
| `QCC_PR_LexVector`, `QCC_PR_Lex` | "char constant: denormal", "denormalized immediate" | error | W0121, as E0125 | FTEF-12.12, QCD-7.43 | ported |
| `QCC_PR_LexName` | "Invalid UTF-8 code sequence ..." | error | E0123 | FTEF-13.16, QCD-2.9 | ported |
| `QCC_PR_LexPunctuation` | "** is unsafe around pointers, use *^ instead." | warning | W0118 | FTEF-12.12, QCD-7.40 | ported |
| `QCC_PR_LexPunctuation` | "Unknown punctuation: '\x%x'" | error | E0105 | FTEF-13.15, QCD-2.5 | ported |
| `QCC_PR_LexWhitespace`, `QCC_PR_LexComment` | "EOF inside comment" | error | E0501 | FTEF-13.18, FTEF-14.67, QC-27.2 | ported |
| `QCC_PR_LexWhitespace`, `QCC_PR_LexComment` | "\"/*\" inside comment" | warning | W0112 | FTEF-13.18, QCD-7.34 | ported |
| `QCC_PR_FindMacro`, `QCC_PR_MacroFrame`, `QCC_PR_LexGrab`, `QCC_PR_ExpandMacro` | "Stale macro used", "Case insensitive frame macro", "Duplicate macro defined", "Name for frame macro %s is too long", "Too many frame macros defined", "hanging $", "$modelname name too long", "Unknown frame macro $%s" | warning, error | W0105, W0104, W0106, E0119, E0107, W0107, E0106 | FTEF-18.29, FTEF-18.21, FTEF-18.30, QCD-7.24 to QCD-7.27 | ported |
| `QCC_PR_SimpleGetString`, `QCC_PR_SimpleGetToken`, `QCC_PR_LexMacroName` | "new line inside string", "token exceeds %i chars" | error | E0121, E0502 | FTEF-14.66 | ported |
| `QCC_PR_UndefineName` | "Precompiler constant %s was not defined" | warning | W0114 | FTEF-14.57, QCD-7.36 | ported |
| `QCC_PR_DefineName` | "Compiler constant name length is too long or short" | error | E0121 | FTEF-14.42 | ported |
| `QCC_PR_DefineName` | "Duplicate definition for Precompiler constant %s" | warning | none: only a `-D` option reaches it, printed against "cmdline" | FTEF-14.62 | not modelled |
| `QCC_PR_PreProcessor_Define` | "No name defined for compiler constant", "May not have more than %i parameters to a macro", "parameter name is too long (max %i)", "varadic argument must be last", "duplicate macro paramter name '%s'", "missing ) in macro parameter list" | error | E0114, E0115 | FTEF-14.5, FTEF-14.23, FTEF-14.58 | ported |
| `QCC_PR_PreProcessor_Define` | "different number of macro arguments in macro append", "arguments differ in macro append" | warning | W0115 | FTEF-14.24, FTEF-14.58, QCD-7.37 | ported |
| `QCC_PR_PreProcessor_Define` | "preprocessor directive within preprocessor macro %s" | error | E0122, W0109 | FTEF-14.46 | ported |
| `QCC_PR_PreProcessor_Define` | "EOF inside quote in define %s" | warning | W0115 | FTEF-14.58, QCD-7.37 | ported |
| `QCC_PR_PreProcessor_Define` | "Alternate precompiler definition of %s (%s -> %s)" | warning | W0115 | FTEF-14.58, QCD-7.37 | ported |
| `QCC_PR_PreProcessor_Define` | "Identical precompiler definition of %s" | ignored | W0116 | FTEF-14.58, QCD-7.38 | ported |
| `QCC_PR_ExpandPreProcessorMacro` | "Too many parameters in macro call", "EOF on macro call", "Not enough macro parameters", "__VA_COUNT__ without any variable args" | error | E0116, E0117 | FTEF-14.9, FTEF-14.22, FTEF-14.26 | ported |
| `QCC_PR_ExpandPreProcessorMacro` | "'#%s' is not a macro parameter in %s" | warning | W0117 | FTEF-14.61, QCD-7.39 | ported |
| `QCC_PR_Lex` | "Explicit precompiler usage when not defined %s" | error | E0209 | FTEF-14.52, FTE-23.10 | ported |
| `QCC_LoadFile` | "file contains null bytes" | warning | W0117 | FTEF-14.64, QCD-7.39 | ported |
| `QCC_main` | "No destination filename.  qcc -help for info." | error | none: FTEQCC never reaches it | PRJ-4.9 | ported |
