// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnostic codes raised before parsing.

/// A diagnostic code from `docs/spec/language-diagnostics.md` for an error or warning the
/// lexer or preprocessor reports.
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
    /// "unterminated comment" (QCD-6.1).
    E0501,
    /// "token too long" (QCD-6.2).
    E0502,
    /// "`,` or `;` in a $frame list" (QCD-6.3).
    E0503,
    /// "too many frames" (QCD-6.4).
    E0504,
    /// "frame name longer than 15 bytes; id's qcc cannot look it up" (QCD-7.1).
    W0101,
    /// "macro NAME redefined" (QCD-7.2).
    W0102,
}

impl Code {
    /// Every code, in order.
    pub const ALL: [Self; 23] = [
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
        Self::E0501,
        Self::E0502,
        Self::E0503,
        Self::E0504,
        Self::W0101,
        Self::W0102,
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
            Self::E0501 => "E0501",
            Self::E0502 => "E0502",
            Self::E0503 => "E0503",
            Self::E0504 => "E0504",
            Self::W0101 => "W0101",
            Self::W0102 => "W0102",
        }
    }

    /// Whether the code is an error rather than a warning.
    #[must_use]
    pub const fn is_error(self) -> bool {
        match self {
            Self::W0101 | Self::W0102 => false,
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
            | Self::E0501
            | Self::E0502
            | Self::E0503
            | Self::E0504 => true,
        }
    }
}
