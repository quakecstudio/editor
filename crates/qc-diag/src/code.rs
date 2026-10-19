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
    /// "Unknown value \"NAME\"" (QCD-4.1).
    E0301,
    /// "Goto statement with no matching label \"NAME\"" (QCD-4.8).
    E0308,
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
    /// "No destination filename.  qcc -help for info." (QCD-5.1).
    E0401,
    /// "Error opening PATH: REASON" (QCD-5.2).
    E0402,
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
    /// "#pragma autoproto must appear before any definitions" (QCD-7.28).
    W0108,
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
    /// "NAME is builtin #N, which ENGINE does not have" (QCD-7.17).
    W0311,
    /// "Duplicate label NAME" (QCD-7.30).
    W0312,
    /// "Statement does not do anything" (QCD-7.29).
    W0313,
    /// "NAME differs in case from the file it opens, STORED" (QCD-7.16).
    W0402,
}

impl Code {
    /// Every code, in order.
    pub const ALL: [Self; 64] = [
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
        Self::E0201,
        Self::E0202,
        Self::E0203,
        Self::E0204,
        Self::E0205,
        Self::E0206,
        Self::E0208,
        Self::E0209,
        Self::E0210,
        Self::E0211,
        Self::E0212,
        Self::E0301,
        Self::E0308,
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
        Self::E0401,
        Self::E0402,
        Self::E0406,
        Self::E0407,
        Self::E0408,
        Self::W0101,
        Self::W0102,
        Self::W0103,
        Self::W0104,
        Self::W0105,
        Self::W0106,
        Self::W0107,
        Self::W0108,
        Self::W0201,
        Self::W0203,
        Self::W0204,
        Self::W0205,
        Self::W0206,
        Self::W0311,
        Self::W0312,
        Self::W0313,
        Self::W0402,
    ];

    /// The code as written in messages, such as `E0101`.
    #[must_use]
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
            Self::E0301 => "E0301",
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
            Self::W0201 => "W0201",
            Self::W0203 => "W0203",
            Self::W0204 => "W0204",
            Self::W0205 => "W0205",
            Self::W0206 => "W0206",
            Self::W0311 => "W0311",
            Self::W0312 => "W0312",
            Self::W0313 => "W0313",
            Self::W0402 => "W0402",
        }
    }

    /// Whether the code is an error rather than a warning.
    #[must_use]
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
            | Self::W0201
            | Self::W0203
            | Self::W0204
            | Self::W0205
            | Self::W0206
            | Self::W0311
            | Self::W0312
            | Self::W0313
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
