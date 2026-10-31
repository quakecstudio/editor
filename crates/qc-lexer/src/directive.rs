// SPDX-License-Identifier: GPL-3.0-or-later

//! The names of preprocessor directives and of FTEQCC's pragmas, read once here for the lexer,
//! the preprocessor and the project graph (QC-9.1, FTEF-12.11, FTEF-14).

/// A preprocessor directive, the word after a line's `#`: `qcc`'s (QC-9) and FTEQCC's, as
/// `QCC_PR_Precompiler` names them (FTEF-14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Directive {
    /// `#define`.
    Define,
    /// FTEQCC's `#append`.
    Append,
    /// `#undef`.
    Undef,
    /// FTEQCC's `#if`.
    If,
    /// `#ifdef`.
    Ifdef,
    /// `#ifndef`.
    Ifndef,
    /// `#else`.
    Else,
    /// FTEQCC's `#elif`.
    Elif,
    /// `#elifdef`, which FTEQCC reads only in a skipped block (PRJ-5.5).
    Elifdef,
    /// `#elifndef`, which FTEQCC reads only in a skipped block (PRJ-5.5).
    Elifndef,
    /// `#endif`.
    Endif,
    /// FTEQCC's `#eof`.
    Eof,
    /// FTEQCC's `#error`.
    Error,
    /// FTEQCC's `#warning`.
    Warning,
    /// FTEQCC's `#message`.
    Message,
    /// FTEQCC's `#copyright`.
    Copyright,
    /// FTEQCC's `#package`.
    Package,
    /// FTEQCC's `#pack`.
    Pack,
    /// FTEQCC's `#forcecrc`.
    ForceCrc,
    /// FTEQCC's `#merge`.
    Merge,
    /// FTEQCC's `#includelist`.
    IncludeList,
    /// `#endlist`, which ends an `#includelist` (FTEF-14.41); no directive of its own.
    EndList,
    /// `#include`.
    Include,
    /// FTEQCC's `#datafile`.
    DataFile,
    /// FTEQCC's `#output`.
    Output,
    /// FTEQCC's `#pragma`.
    Pragma,
}

/// Every directive with its name, in the order `QCC_PR_Precompiler` tests them with
/// `strncmp` (FTEF-14): a name that starts another comes after it, `package` before `pack`
/// and `includelist` before `include`.
const DIRECTIVES: [(Directive, &str); 26] = [
    (Directive::Define, "define"),
    (Directive::Append, "append"),
    (Directive::Undef, "undef"),
    (Directive::Ifdef, "ifdef"),
    (Directive::Ifndef, "ifndef"),
    (Directive::If, "if"),
    (Directive::Else, "else"),
    (Directive::Elifdef, "elifdef"),
    (Directive::Elifndef, "elifndef"),
    (Directive::Elif, "elif"),
    (Directive::Endif, "endif"),
    (Directive::Eof, "eof"),
    (Directive::Error, "error"),
    (Directive::Warning, "warning"),
    (Directive::Message, "message"),
    (Directive::Copyright, "copyright"),
    (Directive::Package, "package"),
    (Directive::Pack, "pack"),
    (Directive::ForceCrc, "forcecrc"),
    (Directive::Merge, "merge"),
    (Directive::IncludeList, "includelist"),
    (Directive::EndList, "endlist"),
    (Directive::Include, "include"),
    (Directive::DataFile, "datafile"),
    (Directive::Output, "output"),
    (Directive::Pragma, "pragma"),
];

impl Directive {
    /// The directive whose name is `word`, exactly.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        DIRECTIVES
            .iter()
            .find(|(_, name)| *name == word)
            .map(|(directive, _)| *directive)
    }

    /// The directive whose name `rest`, the text after the `#`, starts with, as
    /// `QCC_PR_Precompiler` finds it (`if (!strncmp(directive, "define", 6))` and on, in that
    /// order), and the length of the name. Every name that starts with `if` is [`Self::If`],
    /// whose branch then tells `ifdef` and `ifndef` apart, and one that starts with `elif` is
    /// [`Self::Elif`]; `#endlist` is none.
    #[must_use]
    pub fn fteqcc_prefix(rest: &[u8]) -> Option<(Self, usize)> {
        DIRECTIVES
            .iter()
            .filter(|(directive, _)| {
                !matches!(
                    directive,
                    Self::Ifdef | Self::Ifndef | Self::Elifdef | Self::Elifndef | Self::EndList
                )
            })
            .find(|(_, name)| rest.starts_with(name.as_bytes()))
            .map(|(directive, name)| (*directive, name.len()))
    }

    /// The directive's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        DIRECTIVES
            .iter()
            .find(|(directive, _)| *directive == self)
            .map_or("", |(_, name)| name)
    }
}

/// A pragma `QCC_PR_Precompiler` knows, the word after `#pragma` matched with `QC_strcasecmp`;
/// any other is "Unknown pragma" (FTEF-12.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pragma {
    /// `DONT_COMPILE_THIS_FILE`.
    DontCompileThisFile,
    /// `COPYRIGHT`.
    Copyright,
    /// `compress`.
    Compress,
    /// `forcecrc`.
    ForceCrc,
    /// `framerate`.
    Framerate,
    /// `once`.
    Once,
    /// `file`.
    File,
    /// `line`.
    Line,
    /// `includedir`.
    IncludeDir,
    /// `noref`.
    Noref,
    /// `nosave`.
    Nosave,
    /// `defaultstatic`.
    DefaultStatic,
    /// `autoproto`.
    Autoproto,
    /// `wrasm`.
    Wrasm,
    /// `optimise`.
    Optimise,
    /// `optimize`.
    Optimize,
    /// `sourcefile`.
    SourceFile,
    /// `TARGET`.
    Target,
    /// `PROGS_SRC`.
    ProgsSrc,
    /// `PROGS_DAT`.
    ProgsDat,
    /// `opcode`.
    Opcode,
    /// `keyword`.
    Keyword,
    /// `flag`.
    Flag,
    /// `warning`.
    Warning,
}

/// Every pragma with its name, in the order `QCC_PR_Precompiler` tests them.
const PRAGMAS: [(Pragma, &str); 24] = [
    (Pragma::DontCompileThisFile, "DONT_COMPILE_THIS_FILE"),
    (Pragma::Copyright, "COPYRIGHT"),
    (Pragma::Compress, "compress"),
    (Pragma::ForceCrc, "forcecrc"),
    (Pragma::Framerate, "framerate"),
    (Pragma::Once, "once"),
    (Pragma::File, "file"),
    (Pragma::Line, "line"),
    (Pragma::IncludeDir, "includedir"),
    (Pragma::Noref, "noref"),
    (Pragma::Nosave, "nosave"),
    (Pragma::DefaultStatic, "defaultstatic"),
    (Pragma::Autoproto, "autoproto"),
    (Pragma::Wrasm, "wrasm"),
    (Pragma::Optimise, "optimise"),
    (Pragma::Optimize, "optimize"),
    (Pragma::SourceFile, "sourcefile"),
    (Pragma::Target, "TARGET"),
    (Pragma::ProgsSrc, "PROGS_SRC"),
    (Pragma::ProgsDat, "PROGS_DAT"),
    (Pragma::Opcode, "opcode"),
    (Pragma::Keyword, "keyword"),
    (Pragma::Flag, "flag"),
    (Pragma::Warning, "warning"),
];

impl Pragma {
    /// The pragma `word` names, in any case (`QC_strcasecmp`).
    #[must_use]
    pub fn from_word(word: &[u8]) -> Option<Self> {
        PRAGMAS
            .iter()
            .find(|(_, name)| name.as_bytes().eq_ignore_ascii_case(word))
            .map(|(pragma, _)| *pragma)
    }

    /// The pragma's name as FTEQCC writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        PRAGMAS
            .iter()
            .find(|(pragma, _)| *pragma == self)
            .map_or("", |(_, name)| name)
    }
}

#[cfg(test)]
mod tests {
    use super::{DIRECTIVES, Directive, PRAGMAS, Pragma};

    // spec QC-9.1: each directive's word names it exactly, and nothing else does
    #[test]
    fn a_directive_is_its_exact_word() {
        for (directive, name) in DIRECTIVES {
            assert_eq!(Directive::from_word(name), Some(directive));
            assert_eq!(directive.name(), name);
        }
        assert_eq!(Directive::from_word("Define"), None);
        assert_eq!(Directive::from_word("defines"), None);
        assert_eq!(Directive::from_word(""), None);
    }

    // spec FTEF-14.52: FTEQCC takes the first directive whose name the text starts with, in its
    // order, so `#defined` is `#define` and a longer name is tried before the one it starts with
    #[test]
    fn fteqcc_finds_a_directive_by_its_prefix() {
        let found = |text: &str| Directive::fteqcc_prefix(text.as_bytes());
        assert_eq!(found("define X"), Some((Directive::Define, 6)));
        assert_eq!(found("definedX"), Some((Directive::Define, 6)));
        assert_eq!(found("package x"), Some((Directive::Package, 7)));
        assert_eq!(found("pack x"), Some((Directive::Pack, 4)));
        assert_eq!(found("includelist"), Some((Directive::IncludeList, 11)));
        assert_eq!(found("include \"a\""), Some((Directive::Include, 7)));
        assert_eq!(found("ifdef X"), Some((Directive::If, 2)));
        assert_eq!(found("ifndef X"), Some((Directive::If, 2)));
        assert_eq!(found("elif 1"), Some((Directive::Elif, 4)));
        assert_eq!(found("elifdef X"), Some((Directive::Elif, 4)));
        assert_eq!(found("endlist"), None);
        assert_eq!(found("endif"), Some((Directive::Endif, 5)));
        assert_eq!(found("bogus"), None);
        assert_eq!(found(""), None);
    }

    // spec FTEF-12.11: a pragma is known in any case, by its name only
    #[test]
    fn a_pragma_is_known_in_any_case() {
        for (pragma, name) in PRAGMAS {
            assert_eq!(Pragma::from_word(name.as_bytes()), Some(pragma));
            assert_eq!(
                Pragma::from_word(name.to_ascii_lowercase().as_bytes()),
                Some(pragma)
            );
            assert_eq!(
                Pragma::from_word(name.to_ascii_uppercase().as_bytes()),
                Some(pragma)
            );
            assert_eq!(pragma.name(), name);
        }
        assert_eq!(Pragma::from_word(b"once2"), None);
        assert_eq!(Pragma::from_word(b""), None);
    }
}
