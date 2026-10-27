// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnostic codes and their severity.
//!
//! The codes of `docs/spec/language-diagnostics.md`, raised by the lexer, the preprocessor,
//! the parser (`qc-syntax`), the project graph (`qc-project`), the semantic model (`qc-sema`)
//! and the compiler.

/// Whether a diagnostic is an error or a warning (QCD-1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    /// An error: the code begins with `E`.
    Error,
    /// A warning: the code begins with `W`. A warning never changes the emitted progs
    /// (QCD-7).
    Warning,
}

/// A diagnostic code from `docs/spec/language-diagnostics.md` for an error or warning the
/// lexer, the preprocessor, the parser (`qc-syntax`), the project graph (`qc-project`), the
/// semantic model (`qc-sema`) or the compiler reports.
///
/// Implements QCD-1.1: a code is `E` or `W` and four digits, the first two its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// "EOF inside quote" (QCD-2.1).
    E0101,
    /// "newline inside quote" (QCD-2.2).
    E0102,
    /// "Unknown escape char" (QCD-2.3).
    E0103,
    /// "Bad vector" (QCD-2.4).
    E0104,
    /// "Unknown punctuation" (QCD-2.5).
    E0105,
    /// "Unknown frame macro $NAME" (QCD-2.6).
    E0106,
    /// "hanging $" (QCD-2.7).
    E0107,
    /// "unknown directive #NAME" (QCD-8.1).
    E0108,
    /// "#NAME without #ifdef or #ifndef" (QCD-8.2).
    E0109,
    /// "#ifdef or #ifndef without #endif" (QCD-8.3).
    E0110,
    /// "cannot include PATH: REASON" (QCD-8.4).
    E0111,
    /// "recursive #include of PATH" (QCD-8.5).
    E0112,
    /// "#include expects a file name in quotes" (QCD-8.6).
    E0113,
    /// "#NAME expects a macro name" (QCD-8.7).
    E0114,
    /// "bad parameter list for macro NAME" (QCD-8.8).
    E0115,
    /// "macro NAME expects N arguments, found M" (QCD-8.9).
    E0116,
    /// "unterminated argument list for macro NAME" (QCD-8.10).
    E0117,
    /// "#Error: TEXT" (QCD-8.11).
    E0118,
    /// "Name for frame macro NAME is too long" (QCD-2.8).
    E0119,
    /// "macro expansion too large" (QCD-8.13).
    E0120,
    /// An error FTEQCC's preprocessor reports, with FTEQCC's text (QCD-8.14).
    E0121,
    /// "preprocessor directive within preprocessor macro NAME", as an error (QCD-8.15).
    E0122,
    /// "Invalid UTF-8 code sequence ...": a byte that starts no UTF-8 sequence in an FTEQCC name (QCD-2.9).
    E0123,
    /// "bad suffix on number TEXT": a letter, digit, `_` or high byte right after an FTEQCC number (QCD-2.10).
    E0124,
    /// An FTEQCC warning `#pragma warning error` made an error, with FTEQCC's text (QCD-8.17).
    E0125,
    /// "not a name" (QCD-3.1).
    E0201,
    /// "name too long" (QCD-3.2).
    E0202,
    /// "\"TOKEN\" is not a type" (QCD-3.3).
    E0203,
    /// "expected X, found Y" (QCD-3.4).
    E0204,
    /// "Type mismatch on redeclaration of NAME" (QCD-3.5).
    E0205,
    /// "NAME redeclared" (QCD-3.6).
    E0206,
    /// "wrong immediate type for NAME" (QCD-3.7).
    E0207,
    /// "Fields and functions must be global" (QCD-3.8).
    E0208,
    /// "Bad builtin immediate" (QCD-3.9).
    E0209,
    /// "state frame must be a number" (QCD-3.10).
    E0210,
    /// "function NAME was not defined" (QCD-3.11).
    E0211,
    /// "Forwards State Cycle with backwards range" (QCD-3.12).
    E0212,
    /// "Cannot cast from TYPE to TYPE" (QCD-3.13).
    E0213,
    /// "initializer for 'NAME' is not constant" (QCD-3.14).
    E0214,
    /// "Duplicate definition of NAME." (QCD-3.15).
    E0215,
    /// "redeclaration of function body" (QCD-3.16).
    E0216,
    /// "Default value not a constant" (QCD-3.17).
    E0217,
    /// "Default value not initialized yet" (QCD-3.18).
    E0218,
    /// "Cannot redeclare typedef NAME from TYPE to TYPE" (QCD-3.19).
    E0219,
    /// "Array sizes for redecleration of NAME do not match (A->B)" (QCD-3.20).
    E0220,
    /// "shared values may not be assigned an initial value" (QCD-3.21).
    E0221,
    /// "wrapper function does not wrap anything" (QCD-3.22).
    E0222,
    /// "Wrapper function \"NAME\" does not refer to its prior function" (QCD-3.23).
    E0223,
    /// "bad combination of modifiers with typedef (defining NAME)" (QCD-3.24).
    E0224,
    /// "NAME is not a class" (QCD-3.25).
    E0225,
    /// "NAME is not part of class CLASS", for a struct (QCD-3.26).
    E0226,
    /// "Implicit type mismatch. Needed TYPE, got TYPE." (QCD-3.27).
    E0227,
    /// "enum - NAME is not a compile-time constant" (QCD-3.28).
    E0228,
    /// "values for enums of this type must be initialised" (QCD-3.29).
    E0229,
    /// "accumulate applies only to functions, not NAME" (QCD-3.30).
    E0230,
    /// "can't redefine non-static as static" (QCD-3.31).
    E0231,
    /// "_() intrinsic accepts only a string immediate" (QCD-3.32).
    E0232,
    /// "initializer-string too long" (QCD-3.33).
    E0233,
    /// "NAME not yet defined, cannot create NAME as an alias" (QCD-3.34).
    E0234,
    /// "alias NAME may not be initialised" (QCD-3.35).
    E0235,
    /// "Automatic type conversions disabled. Needed TYPE, got TYPE." (QCD-3.36).
    E0236,
    /// "pointer<->string casts were disabled." (QCD-3.37).
    E0237,
    /// "state type mismatch" (QCD-3.38).
    E0238,
    /// "extern is not supported with this target format" (QCD-3.39).
    E0239,
    /// "Parent class NAME was not yet defined" (QCD-3.40).
    E0240,
    /// "Parent type NAME is not a class/entity" (QCD-3.41).
    E0241,
    /// "Parent class changed on redeclaration of NAME" (QCD-3.42).
    E0242,
    /// "struct NAME has no members" (QCD-3.43).
    E0243,
    /// "function contains illegal breaks" (QCD-4.14).
    E0244,
    /// "function contains illegal continues" (QCD-4.15).
    E0245,
    /// "struct NAME redeclared differently" (QCD-3.44).
    E0246,
    /// "Parent type NAME was not yet defined" (QCD-3.45).
    E0247,
    /// "Parent type NAME is not a struct" (QCD-3.46).
    E0248,
    /// "Redeclaration of struct with different parent type" (QCD-3.47).
    E0249,
    /// "bitfields must be integer types" (QCD-3.48).
    E0250,
    /// "ENUM::NAME already declared" (QCD-3.49).
    E0251,
    /// "multiple members found matching STRUCT.NAME" (QCD-4.16).
    E0252,
    /// "Declaration of class NAME within function" (QCD-3.50).
    E0253,
    /// "Struct or union in class NAME" (QCD-3.51).
    E0254,
    /// "type NAME not fully defined yet" (QCD-3.52).
    E0255,
    /// "not a function" (QCD-4.17).
    E0256,
    /// "Can only initialise member functions" (QCD-3.53).
    E0257,
    /// "Accessor NAME cannot be based upon TYPE", "Accessor NAME basic type mismatch (TYPE,
    /// expected TYPE)" and "Type NAME cannot be redefined as an accessor" (QCD-3.54).
    E0258,
    /// "function contains illegal cases" (QCD-4.18).
    E0259,
    /// "Incompatible redeclaration of class NAME" (QCD-3.55).
    E0260,
    /// "NAME is not a member of STRUCT" (QCD-4.19).
    E0261,
    /// "Value is not an integer constant" (QCD-3.56).
    E0262,
    /// "Unknown value \"NAME\"" (QCD-4.1).
    E0301,
    /// "type mismatch for OP", under an FTEQCC profile "type mismatch for OP (A and B)" (QCD-4.2).
    E0302,
    /// "Goto statement with no matching label \"NAME\"" (QCD-4.8).
    E0308,
    /// "Type mismatch: TYPE, should be void()" (QCD-4.9).
    E0309,
    /// "Cannot use 'this' outside of an OO function" (QCD-4.10).
    E0310,
    /// "Cannot use 'super' outside of an OO function" (QCD-4.11).
    E0311,
    /// "class NAME has no super" (QCD-4.12).
    E0312,
    /// "NAME is not defined" (QCD-4.13).
    E0313,
    /// "unsupported vector swizzle '.NAME'" (QCD-4.20).
    E0314,
    /// "unsupported string method NAME" (QCD-4.21).
    E0315,
    /// "not a constant": the `length` of a string that is no constant (QCD-4.22).
    E0316,
    /// "indirection in TYPE - not a struct or union" (QCD-4.23).
    E0317,
    /// "NAME is not a field", "NAME is not a field of class CLASS" (QCD-4.24).
    E0318,
    /// "Unknown field \"NAME\" in class \"CLASS\"" (QCD-4.25).
    E0319,
    /// "Opcode \"OP|NAME\" not valid for target. Consider the use of: #pragma target fte" and its
    /// two variants (QCD-4.26).
    E0320,
    /// "strcmp function not defined: cannot emulate string<=>string" (QCD-4.27).
    E0321,
    /// "Assignment to post-inc result", "postincrement: lhs is readonly" and "postdecrement: lhs is
    /// readonly" (QCD-4.28).
    E0322,
    /// "Type mismatch on assignment. A OP B is not supported" (QCD-4.29).
    E0323,
    /// "Implicit type mismatch on assignment to NAME. Needed A, got B." (QCD-4.30).
    E0324,
    /// "left operand must be an l-value (did you mean NAME[0]?)" (QCD-4.31).
    E0325,
    /// "void* maths was disabled.", "pointer* maths was disabled on this type.", "pointer sizes
    /// were marked as undefined." and "Making assumptions about pointer sizes was blocked."
    /// (QCD-4.32).
    E0326,
    /// "incompatible pointer types" (QCD-4.33).
    E0327,
    /// "Type mismatch: !TYPE" and "Type mismatch: ~TYPE" (QCD-4.34).
    E0328,
    /// "++ operator on read-only value", "-- operator on read-only value" and "++ operator on
    /// unsupported type" (QCD-4.35).
    E0329,
    /// "Unable to dereference non-pointer type.", "attempt to dereference function type." and "'@'
    /// operator only functions on floats and ints. go figure." (QCD-4.36).
    E0330,
    /// "Ternary operator with mismatching types" (QCD-4.37).
    E0332,
    /// "_length(string) requires an initialised constant" and "_length() unsupported argument type
    /// for intrinsic" (QCD-4.38).
    E0334,
    /// "unterminated comment" (QCD-6.1).
    E0501,
    /// "token too long" (QCD-6.2).
    E0502,
    /// "`,` or `;` in a $frame list" (QCD-6.3).
    E0503,
    /// "too many frames" (QCD-6.4).
    E0504,
    /// "more than eight parameters in a function type" (QCD-6.5).
    E0505,
    /// "initialiser is not a constant" (QCD-6.6).
    E0506,
    /// "builtin number must be positive" (QCD-6.7).
    E0507,
    /// "cannot assign to this expression" (QCD-6.8).
    E0508,
    /// "nesting too deep" (QCD-6.9).
    E0509,
    /// "NAME is an FTEQCC extension, not in vanilla QuakeC" (QCD-6.10).
    E0510,
    /// "Bad op code name NAME" (QCD-6.11).
    E0511,
    /// "No destination filename.  qcc -help for info." (QCD-5.1).
    E0401,
    /// "Error opening PATH: REASON" (QCD-5.2).
    E0402,
    /// "bad parm order" (QCD-5.4).
    E0404,
    /// "#includelist without #endlist" (QCD-5.6).
    E0406,
    /// "too many files in the project graph" (QCD-5.7).
    E0407,
    /// "files nested too deeply to read PATH" (QCD-5.8).
    E0408,
    /// "frame name longer than 15 bytes; id's qcc cannot look it up" (QCD-7.1).
    W0101,
    /// "macro NAME redefined" (QCD-7.2).
    W0102,
    /// "#warning: TEXT" (QCD-7.20).
    W0103,
    /// "Case insensitive frame macro (using NAME)" (QCD-7.24).
    W0104,
    /// "Stale macro used (NAME, defined in FILE)" (QCD-7.25).
    W0105,
    /// "Duplicate macro defined (NAME). Rename it." (QCD-7.26).
    W0106,
    /// "$modelname name too long" (QCD-7.27).
    W0107,
    /// "#pragma autoproto must appear before any definitions": FTEQCC never gives it (FTEF-12.18).
    W0108,
    /// "preprocessor directive within preprocessor macro NAME", as a warning (QCD-7.31).
    W0109,
    /// "A leading 0 is interpreted as base-8." (QCD-7.53).
    W0110,
    /// "numerical overflow", FTEQCC's F312 (QCD-7.54).
    W0111,
    /// "\"/*\" inside comment", FTEQCC's F325 (QCD-7.55).
    W0112,
    /// A `#pragma` FTEQCC warns about, its Q120, with FTEQCC's text (QCD-7.56).
    W0113,
    /// "Precompiler constant NAME was not defined" (QCD-7.57).
    W0114,
    /// A macro definition FTEQCC warns about, its F315, with FTEQCC's text (QCD-7.58).
    W0115,
    /// "Identical precompiler definition of NAME", FTEQCC's F316 (QCD-7.59).
    W0116,
    /// A warning FTEQCC gives no name, with FTEQCC's text (QCD-7.60).
    W0117,
    /// "** is unsafe around pointers, use *^ instead.", FTEQCC's F321 (QCD-7.61).
    W0118,
    /// A string FTEQCC warns about, its F301, with FTEQCC's text (QCD-7.62).
    W0119,
    /// "2d vector", FTEQCC's F311 (QCD-7.63).
    W0120,
    /// A float FTEQCC writes as an `int`'s bits, FTEQCC's F313, with its text (QCD-7.64).
    W0121,
    /// A `#NAME` in a string under FTEQCC's `macroinstrings` flag, its warning with no name
    /// (QCD-7.65).
    W0122,
    /// "Hanging \\" and a CR: a `\` and a lone CR in an FTEQCC string, its warning with no name
    /// (QCD-7.66).
    W0123,
    /// "local NAME is the global NAME" (QCD-7.3).
    W0201,
    /// "Missing semicolon at end of definition" (QCD-7.18).
    W0203,
    /// "Hanging ';'" (QCD-7.19).
    W0204,
    /// "missing comma in state definition" (QCD-7.21).
    W0205,
    /// "incompatible redeclaration. Ignoring replacement of qc function with builtin."
    /// (QCD-7.23).
    W0206,
    /// "NAME duplicate definition ignored" (QCD-7.32).
    W0207,
    /// "Local \"NAME\" hides global with same name and type" (QCD-7.33).
    W0208,
    /// "Optional arguments differ on redeclaration of NAME. TYPE, should be TYPE" (QCD-7.34).
    W0209,
    /// "NAME builtin was wrongly redefined as TYPE. ignoring later definition" (QCD-7.35).
    W0210,
    /// "Type redeclaration of TYPE NAME replaces existing variable" (QCD-7.36).
    W0211,
    /// "Redeclaration of NAME would change to const." and its kin (QCD-7.37).
    W0212,
    /// "Autocvar \"NAME\" defined with local scope. promoting to global." and its kin (QCD-7.38).
    W0213,
    /// "function modifier on non-function." and the other ignored keywords (QCD-7.39).
    W0214,
    /// "typedef defines no types" (QCD-7.40).
    W0215,
    /// "NAME is not part of class CLASS", for a class (QCD-7.41).
    W0216,
    /// "NAME already declared as a builtin" (QCD-7.42).
    W0217,
    /// "NAME redeclared to accumulate after initial declaration" (QCD-7.43).
    W0218,
    /// "initializer for NAME is not initialised yet, NAME will be treated as 0" (QCD-7.45).
    W0219,
    /// "Implicit cast from TYPE to TYPE" (QCD-7.46).
    W0220,
    /// "\"NAME\" keyword used as variable name - coexisting", or "- disabling" (QCD-7.67).
    W0221,
    /// "Parameter N of NAME is not named" (QCD-7.48).
    W0222,
    /// "incompatible redeclaration. Please validate builtin numbers. parseentitydata is #613" (QCD-7.49).
    W0223,
    /// "legacy enum base type. Use \"enum [class] [name_e]:type\" instead" (QCD-7.50).
    W0224,
    /// "enumflags - VALUE has multiple bits set" and "enumflags - VALUE not an integer value" (QCD-7.51).
    W0225,
    /// "ignoring unsupported unsigned keyword, type will be signed", "long doubles are not
    /// supported, using double" and "ignoring unsupported const keyword" (QCD-7.52).
    W0226,
    /// "suggest parenthesis for unary operator that applies to multiple terms" (QCD-7.68).
    W0227,
    /// "Unknown attribute \"NAME\"" and "accumulating an accumulating accumulation"
    /// (QCD-7.69).
    W0228,
    /// "Implicit lax cast from TYPE to TYPE" (QCD-7.70).
    W0229,
    /// "Implicit type mismatch. Needed TYPE, got TYPE." as a warning (QCD-7.71).
    W0230,
    /// "type mismatch for OP (A and B)" with `lax` on (QCD-7.73).
    W0231,
    /// "Implicit conversion from A to B" (QCD-7.74).
    W0232,
    /// "Division of N by 0", "Division by constant 0", "Signed overflow on division by -1" and
    /// "Division by 0" (QCD-7.75).
    W0233,
    /// "Numerical overflow. F will be rounded to N", "Numerical truncation of A to B." and
    /// "Constant float operand F will be truncated to N" (QCD-7.76).
    W0234,
    /// "NAME function not defined: cannot emulate OP" and the other notes of an emulation
    /// (QCD-7.77).
    W0235,
    /// "OP_ADD_SF: string+float may be unsafe" and "OP_ADD_SI: string+int may be unsafe"
    /// (QCD-7.78).
    W0236,
    /// "itof emulation: denormals are unsafe NAME", "ftoi emulation: denormals are unsafe" and the
    /// other denormal warnings of an emulation (QCD-7.79).
    W0237,
    /// "utof emulation: will break if NAME is big" (QCD-7.80).
    W0238,
    /// "Result of comparison is constant", "Parameter offsets for && are the same" and "Parameters
    /// for || are the same" (QCD-7.81).
    W0239,
    /// "'OP' type mismatch: A with B" (QCD-7.82).
    W0240,
    /// "Assignment to const" and "Assignment to constant NAME" (QCD-7.83).
    W0241,
    /// "suggest parenthesis for assignment used as truth value" (QCD-7.84).
    W0242,
    /// "Implicit type mismatch on assignment to NAME. Needed A, got B." with `lax` on (QCD-7.85).
    W0243,
    /// "Type mismatch on ternary operator: A vs B" (QCD-7.86).
    W0244,
    /// "if (string) tests for null, not empty" (QCD-7.87).
    W0245,
    /// "if (vector) tests only the first element with the current compiler flags" (QCD-7.88).
    W0246,
    /// "conditional type mismatch: TYPE" (QCD-7.89).
    W0247,
    /// "Type mismatch: !void", "Type mismatch: ~void" and the other tests of a void value under lax
    /// (QCD-7.90).
    W0248,
    /// "NAME is builtin #N, which ENGINE does not have" (QCD-7.17).
    W0311,
    /// "Duplicate label NAME" (QCD-7.30).
    W0312,
    /// "Statement does not do anything" (QCD-7.29).
    W0313,
    /// "Variable \"NAME\" is deprecated" (QCD-7.44).
    W0314,
    /// "NAME differs in case from the file it opens, STORED" (QCD-7.16).
    W0402,
}

impl Code {
    /// Every code, in order.
    pub const ALL: [Self; 206] = [
        Self::E0101,
        Self::E0102,
        Self::E0103,
        Self::E0104,
        Self::E0105,
        Self::E0106,
        Self::E0107,
        Self::E0108,
        Self::E0109,
        Self::E0110,
        Self::E0111,
        Self::E0112,
        Self::E0113,
        Self::E0114,
        Self::E0115,
        Self::E0116,
        Self::E0117,
        Self::E0118,
        Self::E0119,
        Self::E0120,
        Self::E0121,
        Self::E0122,
        Self::E0123,
        Self::E0124,
        Self::E0125,
        Self::E0201,
        Self::E0202,
        Self::E0203,
        Self::E0204,
        Self::E0205,
        Self::E0206,
        Self::E0207,
        Self::E0208,
        Self::E0209,
        Self::E0210,
        Self::E0211,
        Self::E0212,
        Self::E0213,
        Self::E0214,
        Self::E0215,
        Self::E0216,
        Self::E0217,
        Self::E0218,
        Self::E0219,
        Self::E0220,
        Self::E0221,
        Self::E0222,
        Self::E0223,
        Self::E0224,
        Self::E0225,
        Self::E0226,
        Self::E0227,
        Self::E0228,
        Self::E0229,
        Self::E0230,
        Self::E0231,
        Self::E0232,
        Self::E0233,
        Self::E0234,
        Self::E0235,
        Self::E0236,
        Self::E0237,
        Self::E0238,
        Self::E0239,
        Self::E0240,
        Self::E0241,
        Self::E0242,
        Self::E0243,
        Self::E0244,
        Self::E0245,
        Self::E0246,
        Self::E0247,
        Self::E0248,
        Self::E0249,
        Self::E0250,
        Self::E0251,
        Self::E0252,
        Self::E0253,
        Self::E0254,
        Self::E0255,
        Self::E0256,
        Self::E0257,
        Self::E0258,
        Self::E0259,
        Self::E0260,
        Self::E0261,
        Self::E0262,
        Self::E0301,
        Self::E0302,
        Self::E0308,
        Self::E0309,
        Self::E0310,
        Self::E0311,
        Self::E0312,
        Self::E0313,
        Self::E0314,
        Self::E0315,
        Self::E0316,
        Self::E0317,
        Self::E0318,
        Self::E0319,
        Self::E0320,
        Self::E0321,
        Self::E0322,
        Self::E0323,
        Self::E0324,
        Self::E0325,
        Self::E0326,
        Self::E0327,
        Self::E0328,
        Self::E0329,
        Self::E0330,
        Self::E0332,
        Self::E0334,
        Self::E0401,
        Self::E0402,
        Self::E0404,
        Self::E0406,
        Self::E0407,
        Self::E0408,
        Self::E0501,
        Self::E0502,
        Self::E0503,
        Self::E0504,
        Self::E0505,
        Self::E0506,
        Self::E0507,
        Self::E0508,
        Self::E0509,
        Self::E0510,
        Self::E0511,
        Self::W0101,
        Self::W0102,
        Self::W0103,
        Self::W0104,
        Self::W0105,
        Self::W0106,
        Self::W0107,
        Self::W0108,
        Self::W0109,
        Self::W0110,
        Self::W0111,
        Self::W0112,
        Self::W0113,
        Self::W0114,
        Self::W0115,
        Self::W0116,
        Self::W0117,
        Self::W0118,
        Self::W0119,
        Self::W0120,
        Self::W0121,
        Self::W0122,
        Self::W0123,
        Self::W0201,
        Self::W0203,
        Self::W0204,
        Self::W0205,
        Self::W0206,
        Self::W0207,
        Self::W0208,
        Self::W0209,
        Self::W0210,
        Self::W0211,
        Self::W0212,
        Self::W0213,
        Self::W0214,
        Self::W0215,
        Self::W0216,
        Self::W0217,
        Self::W0218,
        Self::W0219,
        Self::W0220,
        Self::W0221,
        Self::W0222,
        Self::W0223,
        Self::W0224,
        Self::W0225,
        Self::W0226,
        Self::W0227,
        Self::W0228,
        Self::W0229,
        Self::W0230,
        Self::W0231,
        Self::W0232,
        Self::W0233,
        Self::W0234,
        Self::W0235,
        Self::W0236,
        Self::W0237,
        Self::W0238,
        Self::W0239,
        Self::W0240,
        Self::W0241,
        Self::W0242,
        Self::W0243,
        Self::W0244,
        Self::W0245,
        Self::W0246,
        Self::W0247,
        Self::W0248,
        Self::W0311,
        Self::W0312,
        Self::W0313,
        Self::W0314,
        Self::W0402,
    ];

    /// The code as written in messages, such as `E0101`.
    #[must_use]
    #[expect(clippy::too_many_lines, reason = "one arm for each code")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E0101 => "E0101",
            Self::E0102 => "E0102",
            Self::E0103 => "E0103",
            Self::E0104 => "E0104",
            Self::E0105 => "E0105",
            Self::E0106 => "E0106",
            Self::E0107 => "E0107",
            Self::E0108 => "E0108",
            Self::E0109 => "E0109",
            Self::E0110 => "E0110",
            Self::E0111 => "E0111",
            Self::E0112 => "E0112",
            Self::E0113 => "E0113",
            Self::E0114 => "E0114",
            Self::E0115 => "E0115",
            Self::E0116 => "E0116",
            Self::E0117 => "E0117",
            Self::E0118 => "E0118",
            Self::E0119 => "E0119",
            Self::E0120 => "E0120",
            Self::E0121 => "E0121",
            Self::E0122 => "E0122",
            Self::E0123 => "E0123",
            Self::E0124 => "E0124",
            Self::E0125 => "E0125",
            Self::E0201 => "E0201",
            Self::E0202 => "E0202",
            Self::E0203 => "E0203",
            Self::E0204 => "E0204",
            Self::E0205 => "E0205",
            Self::E0206 => "E0206",
            Self::E0208 => "E0208",
            Self::E0209 => "E0209",
            Self::E0210 => "E0210",
            Self::E0211 => "E0211",
            Self::E0212 => "E0212",
            Self::E0231 => "E0231",
            Self::E0232 => "E0232",
            Self::E0233 => "E0233",
            Self::E0234 => "E0234",
            Self::E0235 => "E0235",
            Self::E0236 => "E0236",
            Self::E0237 => "E0237",
            Self::E0238 => "E0238",
            Self::E0239 => "E0239",
            Self::E0240 => "E0240",
            Self::E0241 => "E0241",
            Self::E0242 => "E0242",
            Self::E0243 => "E0243",
            Self::E0244 => "E0244",
            Self::E0245 => "E0245",
            Self::E0246 => "E0246",
            Self::E0247 => "E0247",
            Self::E0248 => "E0248",
            Self::E0249 => "E0249",
            Self::E0250 => "E0250",
            Self::E0251 => "E0251",
            Self::E0252 => "E0252",
            Self::E0253 => "E0253",
            Self::E0254 => "E0254",
            Self::E0255 => "E0255",
            Self::E0256 => "E0256",
            Self::E0257 => "E0257",
            Self::E0258 => "E0258",
            Self::E0259 => "E0259",
            Self::E0260 => "E0260",
            Self::E0261 => "E0261",
            Self::E0262 => "E0262",
            Self::E0301 => "E0301",
            Self::E0302 => "E0302",
            Self::E0308 => "E0308",
            Self::E0501 => "E0501",
            Self::E0502 => "E0502",
            Self::E0503 => "E0503",
            Self::E0504 => "E0504",
            Self::E0505 => "E0505",
            Self::E0506 => "E0506",
            Self::E0507 => "E0507",
            Self::E0508 => "E0508",
            Self::E0509 => "E0509",
            Self::E0510 => "E0510",
            Self::E0401 => "E0401",
            Self::E0402 => "E0402",
            Self::E0406 => "E0406",
            Self::E0407 => "E0407",
            Self::E0408 => "E0408",
            Self::W0101 => "W0101",
            Self::W0102 => "W0102",
            Self::W0103 => "W0103",
            Self::W0104 => "W0104",
            Self::W0105 => "W0105",
            Self::W0106 => "W0106",
            Self::W0107 => "W0107",
            Self::W0108 => "W0108",
            Self::W0109 => "W0109",
            Self::W0110 => "W0110",
            Self::W0111 => "W0111",
            Self::W0112 => "W0112",
            Self::W0113 => "W0113",
            Self::W0114 => "W0114",
            Self::W0115 => "W0115",
            Self::W0116 => "W0116",
            Self::W0117 => "W0117",
            Self::W0118 => "W0118",
            Self::W0119 => "W0119",
            Self::W0120 => "W0120",
            Self::W0121 => "W0121",
            Self::W0122 => "W0122",
            Self::W0123 => "W0123",
            Self::W0201 => "W0201",
            Self::W0203 => "W0203",
            Self::W0204 => "W0204",
            Self::W0205 => "W0205",
            Self::W0206 => "W0206",
            Self::W0219 => "W0219",
            Self::W0220 => "W0220",
            Self::W0221 => "W0221",
            Self::W0222 => "W0222",
            Self::W0223 => "W0223",
            Self::W0224 => "W0224",
            Self::W0225 => "W0225",
            Self::W0226 => "W0226",
            Self::W0227 => "W0227",
            Self::W0228 => "W0228",
            Self::W0229 => "W0229",
            Self::W0230 => "W0230",
            Self::W0231 => "W0231",
            Self::W0232 => "W0232",
            Self::W0233 => "W0233",
            Self::W0234 => "W0234",
            Self::W0235 => "W0235",
            Self::W0236 => "W0236",
            Self::W0237 => "W0237",
            Self::W0238 => "W0238",
            Self::W0239 => "W0239",
            Self::W0240 => "W0240",
            Self::W0241 => "W0241",
            Self::W0242 => "W0242",
            Self::W0243 => "W0243",
            Self::W0244 => "W0244",
            Self::W0245 => "W0245",
            Self::W0246 => "W0246",
            Self::W0247 => "W0247",
            Self::W0248 => "W0248",
            Self::W0311 => "W0311",
            Self::W0312 => "W0312",
            Self::W0313 => "W0313",
            Self::W0402 => "W0402",
            Self::W0314 => "W0314",
            Self::W0218 => "W0218",
            Self::W0217 => "W0217",
            Self::W0216 => "W0216",
            Self::W0215 => "W0215",
            Self::W0214 => "W0214",
            Self::W0213 => "W0213",
            Self::W0212 => "W0212",
            Self::W0211 => "W0211",
            Self::W0210 => "W0210",
            Self::W0209 => "W0209",
            Self::W0208 => "W0208",
            Self::W0207 => "W0207",
            Self::E0404 => "E0404",
            Self::E0313 => "E0313",
            Self::E0314 => "E0314",
            Self::E0315 => "E0315",
            Self::E0316 => "E0316",
            Self::E0317 => "E0317",
            Self::E0318 => "E0318",
            Self::E0319 => "E0319",
            Self::E0320 => "E0320",
            Self::E0321 => "E0321",
            Self::E0322 => "E0322",
            Self::E0323 => "E0323",
            Self::E0324 => "E0324",
            Self::E0325 => "E0325",
            Self::E0326 => "E0326",
            Self::E0327 => "E0327",
            Self::E0328 => "E0328",
            Self::E0329 => "E0329",
            Self::E0330 => "E0330",
            Self::E0332 => "E0332",
            Self::E0334 => "E0334",
            Self::E0511 => "E0511",
            Self::E0312 => "E0312",
            Self::E0311 => "E0311",
            Self::E0310 => "E0310",
            Self::E0309 => "E0309",
            Self::E0230 => "E0230",
            Self::E0229 => "E0229",
            Self::E0228 => "E0228",
            Self::E0227 => "E0227",
            Self::E0226 => "E0226",
            Self::E0225 => "E0225",
            Self::E0224 => "E0224",
            Self::E0223 => "E0223",
            Self::E0222 => "E0222",
            Self::E0221 => "E0221",
            Self::E0220 => "E0220",
            Self::E0219 => "E0219",
            Self::E0218 => "E0218",
            Self::E0217 => "E0217",
            Self::E0216 => "E0216",
            Self::E0215 => "E0215",
            Self::E0214 => "E0214",
            Self::E0213 => "E0213",
            Self::E0207 => "E0207",
        }
    }

    /// Whether the code is an error rather than a warning.
    #[must_use]
    #[expect(clippy::too_many_lines, reason = "one arm for each code")]
    pub const fn is_error(self) -> bool {
        match self {
            Self::W0101
            | Self::W0102
            | Self::W0103
            | Self::W0104
            | Self::W0105
            | Self::W0106
            | Self::W0107
            | Self::W0108
            | Self::W0109
            | Self::W0110
            | Self::W0111
            | Self::W0112
            | Self::W0113
            | Self::W0114
            | Self::W0115
            | Self::W0116
            | Self::W0117
            | Self::W0118
            | Self::W0119
            | Self::W0120
            | Self::W0121
            | Self::W0122
            | Self::W0123
            | Self::W0201
            | Self::W0203
            | Self::W0204
            | Self::W0205
            | Self::W0206
            | Self::W0219
            | Self::W0220
            | Self::W0221
            | Self::W0222
            | Self::W0223
            | Self::W0224
            | Self::W0225
            | Self::W0226
            | Self::W0227
            | Self::W0228
            | Self::W0229
            | Self::W0230
            | Self::W0311
            | Self::W0312
            | Self::W0313
            | Self::W0207
            | Self::W0208
            | Self::W0209
            | Self::W0210
            | Self::W0211
            | Self::W0212
            | Self::W0213
            | Self::W0214
            | Self::W0215
            | Self::W0216
            | Self::W0217
            | Self::W0218
            | Self::W0314
            | Self::W0231
            | Self::W0232
            | Self::W0233
            | Self::W0234
            | Self::W0235
            | Self::W0236
            | Self::W0237
            | Self::W0238
            | Self::W0239
            | Self::W0240
            | Self::W0241
            | Self::W0242
            | Self::W0243
            | Self::W0244
            | Self::W0245
            | Self::W0246
            | Self::W0247
            | Self::W0248
            | Self::W0402 => false,
            Self::E0101
            | Self::E0102
            | Self::E0103
            | Self::E0104
            | Self::E0105
            | Self::E0106
            | Self::E0107
            | Self::E0108
            | Self::E0109
            | Self::E0110
            | Self::E0111
            | Self::E0112
            | Self::E0113
            | Self::E0114
            | Self::E0115
            | Self::E0116
            | Self::E0117
            | Self::E0118
            | Self::E0119
            | Self::E0120
            | Self::E0121
            | Self::E0122
            | Self::E0123
            | Self::E0124
            | Self::E0125
            | Self::E0201
            | Self::E0202
            | Self::E0203
            | Self::E0204
            | Self::E0205
            | Self::E0206
            | Self::E0208
            | Self::E0209
            | Self::E0210
            | Self::E0211
            | Self::E0212
            | Self::E0231
            | Self::E0232
            | Self::E0233
            | Self::E0234
            | Self::E0235
            | Self::E0236
            | Self::E0237
            | Self::E0238
            | Self::E0239
            | Self::E0240
            | Self::E0241
            | Self::E0242
            | Self::E0243
            | Self::E0244
            | Self::E0245
            | Self::E0246
            | Self::E0247
            | Self::E0248
            | Self::E0249
            | Self::E0250
            | Self::E0251
            | Self::E0252
            | Self::E0253
            | Self::E0254
            | Self::E0255
            | Self::E0256
            | Self::E0257
            | Self::E0258
            | Self::E0259
            | Self::E0260
            | Self::E0261
            | Self::E0262
            | Self::E0301
            | Self::E0308
            | Self::E0501
            | Self::E0502
            | Self::E0503
            | Self::E0504
            | Self::E0505
            | Self::E0506
            | Self::E0507
            | Self::E0508
            | Self::E0509
            | Self::E0510
            | Self::E0401
            | Self::E0402
            | Self::E0406
            | Self::E0407
            | Self::E0207
            | Self::E0213
            | Self::E0214
            | Self::E0215
            | Self::E0216
            | Self::E0217
            | Self::E0218
            | Self::E0219
            | Self::E0220
            | Self::E0221
            | Self::E0222
            | Self::E0223
            | Self::E0224
            | Self::E0225
            | Self::E0226
            | Self::E0227
            | Self::E0228
            | Self::E0229
            | Self::E0230
            | Self::E0309
            | Self::E0310
            | Self::E0311
            | Self::E0312
            | Self::E0313
            | Self::E0511
            | Self::E0404
            | Self::E0302
            | Self::E0314
            | Self::E0315
            | Self::E0316
            | Self::E0317
            | Self::E0318
            | Self::E0319
            | Self::E0320
            | Self::E0321
            | Self::E0322
            | Self::E0323
            | Self::E0324
            | Self::E0325
            | Self::E0326
            | Self::E0327
            | Self::E0328
            | Self::E0329
            | Self::E0330
            | Self::E0332
            | Self::E0334
            | Self::E0408 => true,
        }
    }

    /// The code's severity: [`Severity::Error`] for an `E` code, [`Severity::Warning`] for a
    /// `W` code (QCD-1.1).
    #[must_use]
    pub const fn severity(self) -> Severity {
        if self.is_error() {
            Severity::Error
        } else {
            Severity::Warning
        }
    }
}
