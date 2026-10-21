// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's warnings on the preprocessor's path and its warning settings (FTEF-12.11, FTEF-14.57
//! to FTEF-14.70): each case was compiled with the pinned FTEQCC (`fteqcc -Tid`), which gave the
//! same warnings with the same texts.

use std::path::Path;

use super::{codes, one_fte, tokens};
use crate::fte_directive::FteState;
use crate::fte_tables::{OPCODES, REAL_OPCODES};
use crate::{
    Code, FileOutput, FteFlag, FteWarning, MemoryLoader, Preprocessor, WarningAction,
    WarningSettings,
};

/// The codes and messages `text` gives under FTEQCC.
fn warned(text: &str) -> Vec<(Code, String)> {
    let (_, out) = one_fte(text);
    out.diagnostics
        .into_iter()
        .map(|d| (d.code, d.message))
        .collect()
}

fn one(code: Code, message: &str) -> Vec<(Code, String)> {
    vec![(code, message.to_owned())]
}

// spec FTEF-12.11: `#pragma flag` and `keyword` read their state first and their names after,
// matched in `compiler_flag[]`, with FTEQCC's three warnings
// spec QCD-7.56: W0113 is FTEQCC's Q120 with its text
#[test]
fn flag_pragmas_warn_as_fteqcc_does() {
    assert_eq!(
        warned("#pragma flag enable foo\n"),
        one(Code::W0113, "keyword/flag foo not recognised")
    );
    assert_eq!(
        warned("#pragma flag maybe hashonly\n"),
        one(Code::W0113, "compiler flag state not recognised")
    );
    assert_eq!(
        warned("#pragma keyword enable acc\n"),
        one(
            Code::W0113,
            "Cannot enable/disable keyword/flag via a pragma"
        )
    );
    assert!(warned("#pragma flag DISABLE HashOnly assumeint\n").is_empty());
    assert!(warned("#pragma keyword off class\n").is_empty());
    let mut settings = crate::FlagSettings::default();
    assert!(settings.is_on(FteFlag::Qcfuncs) && !settings.is_on(FteFlag::Hashonly));
    assert!(settings.apply_pragma(b"on hashonly").is_empty());
    assert!(settings.is_on(FteFlag::Hashonly));
    assert!(settings.apply_pragma(b"off qcfuncs").is_empty());
    assert!(!settings.is_on(FteFlag::Qcfuncs));
    assert_eq!(settings.apply_pragma(b"on int").len(), 1);
}

// spec FTEF-12.11: `optimise`, `opcode` and `TARGET` warn for what FTEQCC's tables do not hold,
// and an unknown pragma is "Unknown pragma 'NAME'"
// spec QCD-7.60: W0117 is a warning FTEQCC gives no name, a target's among them
#[test]
fn build_pragmas_warn_against_fteqcc_s_tables() {
    assert_eq!(
        warned("#pragma optimise foo\n"),
        one(Code::W0113, "pragma optimise: foo unsupported")
    );
    assert_eq!(
        warned("#pragma OPTIMIZE no-bar\n"),
        one(Code::W0113, "pragma OPTIMIZE: bar unsupported")
    );
    for known in ["2", "addon", "MUTATOR x", "no-locals", "lo", "Overlaptemps"] {
        assert!(
            warned(&format!("#pragma optimise {known}\n")).is_empty(),
            "{known}"
        );
    }
    assert_eq!(
        warned("#pragma opcode foo\n"),
        one(Code::W0113, "opcode state not recognised")
    );
    assert_eq!(
        warned("#pragma opcode on STORE_F foo store_v\n"),
        one(Code::W0113, "opcode foo not recognised")
    );
    // `OP_NUMREALOPS` and the rows after it are internal.
    let internal = OPCODES[REAL_OPCODES];
    assert_eq!(
        warned(&format!("#pragma opcode off {internal}\n")),
        one(Code::W0113, &format!("opcode {internal} is internal"))
    );
    assert_eq!(
        warned("#pragma TARGET XYZ\n"),
        one(
            Code::W0117,
            "Unknown target 'XYZ'. Ignored.\nValid targets are: ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)"
        )
    );
    assert_eq!(
        warned("#pragma TARGET fte_99999\n"),
        one(
            Code::W0117,
            "target revision 99999 is unknown, assuming revision 6614"
        )
    );
    assert_eq!(
        warned("#pragma TARGET id_1\n"),
        one(
            Code::W0117,
            "target revision 1 is unknown, assuming revision 0"
        )
    );
    for known in ["FTE", "fte_6614", "dp_20250104", "id", "Hexen2", "fte_-1"] {
        assert!(
            warned(&format!("#pragma TARGET {known}\n")).is_empty(),
            "{known}"
        );
    }
    assert_eq!(
        warned("#pragma unknown thing\n"),
        one(Code::W0113, "Unknown pragma 'unknown'")
    );
    assert_eq!(warned("#pragma\n"), one(Code::W0113, "Unknown pragma ''"));
}

// spec FTEF-14.57: `#undef` of no macro, the empty name included, warns first, and an `#if`
// form with no white space after its name is "Unknown pragma" with the word
// spec QCD-7.57: W0114 is "Precompiler constant NAME was not defined"
#[test]
fn undef_of_no_macro_and_a_bad_if_warn() {
    assert_eq!(
        warned("#undef Z\n"),
        one(Code::W0114, "Precompiler constant Z was not defined")
    );
    assert_eq!(
        warned("#undef\n"),
        one(Code::W0114, "Precompiler constant  was not defined")
    );
    assert!(warned("#define Z\n#undef Z\n").is_empty());
    let (_, out) = one_fte("#undef Z junk\n");
    assert_eq!(codes(&out), vec![Code::W0114, Code::E0121]);
    assert_eq!(
        warned("#ifdefX\n"),
        one(Code::W0113, "Unknown pragma 'ifdefX'")
    );
    assert_eq!(warned("#if(1)\n")[0].1, "Unknown pragma 'if'");
}

// spec FTEF-14.58: a definition's errors have FTEQCC's texts, and its warnings are F315's and
// F316's
// spec QCD-7.58: W0115 is F315 with its text
// spec QCD-7.59: W0116 is F316, reported once a pragma turns it on
#[test]
fn definitions_warn_and_refuse_as_fteqcc_does() {
    assert_eq!(
        warned("#define X 1\n#define X 2\n"),
        one(
            Code::W0115,
            "Alternate precompiler definition of X (1 -> 2)"
        )
    );
    assert!(warned("#define X 1\n#define X 1\n").is_empty());
    assert_eq!(
        warned("#pragma warning on F316\n#define X 1\n#define X 1\n"),
        one(Code::W0116, "Identical precompiler definition of X")
    );
    assert_eq!(
        warned("#define B /* x\n"),
        one(Code::W0115, "EOF inside quote in define B")
    );
    assert!(warned("#define B /* x */ 1\n").is_empty());
    assert_eq!(
        warned("#define FTEQCC 1\n"),
        one(
            Code::W0115,
            "Alternate precompiler definition of FTEQCC ( -> 1)"
        )
    );
    for (text, message) in [
        ("#define\n", "No name defined for compiler constant"),
        ("#define X(a,a) a\n", "duplicate macro paramter name 'a'"),
        ("#define X(..., a) a\n", "varadic argument must be last"),
        ("#define X(a\n", "missing ) in macro parameter list"),
        (
            "#define X(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) a\n",
            "parameter name is too long (max 32)",
        ),
    ] {
        let found = warned(text);
        assert_eq!(found.len(), 1, "{text:?}");
        assert_eq!(found[0].1, message, "{text:?}");
    }
    let many: Vec<String> = (0..33).map(|i| format!("p{i}")).collect();
    let found = warned(&format!("#define X({}) a\n", many.join(",")));
    assert_eq!(
        found[0].1,
        "May not have more than 32 parameters to a macro"
    );
}

// spec FTEF-14.68: a ninth include directory and one too long for FTEQCC's table warn, and are
// not kept
// spec FTEF-14.70: `includedir` adds at most 8 directories, as FTEQCC's table holds
#[test]
fn include_directories_past_fteqcc_s_table_warn() {
    let mut text = String::new();
    for i in 0..9 {
        text.push_str(&format!("#pragma includedir d{i}\n"));
    }
    let (pp, out) = one_fte(&text);
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| (d.code, d.message.as_str()))
            .collect::<Vec<_>>(),
        [(
            Code::W0117,
            "Too many include dirs. Ignoring and hoping the stars align."
        )]
    );
    assert_eq!(pp.fte.include_dirs.len(), 8);
    // A `-I` name, which no `msg` buffer holds, reaches the table's 2048 bytes.
    let mut state = FteState::default();
    let long = "d".repeat(2047);
    assert_eq!(
        state.add_include_dir(long.as_bytes()),
        Some("Include path too long.")
    );
    assert!(state.include_dirs.is_empty());
    assert_eq!(state.add_include_dir("d".repeat(2046).as_bytes()), None);
    assert_eq!(state.include_dirs.len(), 1);
    let fits = "d".repeat(1012);
    let (pp, out) = one_fte(&format!("#pragma includedir {fits}\n"));
    assert!(out.diagnostics.is_empty());
    assert_eq!(pp.fte.include_dirs.len(), 1);
}

// spec FTEF-14.66: a `#pragma` name or text of 1024 bytes runs past FTEQCC's `msg[1024]`, which
// the pinned build aborts at, and is an error that ends the file
#[test]
fn a_pragma_past_fteqcc_s_buffer_is_an_error() {
    let message = "#pragma name or text longer than FTEQCC's 1023 bytes";
    let text = |len: usize| format!("#pragma includedir {}\nfloat x;\n", "d".repeat(len));
    let (_, out) = one_fte(&text(1024));
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| (d.code, d.message.as_str()))
            .collect::<Vec<_>>(),
        [(Code::E0121, message)]
    );
    assert_eq!(tokens(&out), "");
    let (_, out) = one_fte(&text(1023));
    assert!(out.diagnostics.is_empty());
    let (_, out) = one_fte(&format!("#pragma {}\n", "n".repeat(1024)));
    assert_eq!(codes(&out), [Code::E0121]);
}

// spec FTEF-14.61: a `#` in a body before a token that names no parameter warns at each
// expansion, unless the body holds a directive
#[test]
fn a_hash_before_no_parameter_warns() {
    assert_eq!(
        warned("#define E(x) #5:x\nE(ex_a)\n"),
        one(Code::W0117, "'#5' is not a macro parameter in E")
    );
    assert_eq!(
        warned("#define E(x) #y #x\nE(a)\nE(b)\n"),
        [
            (Code::W0117, "'#y' is not a macro parameter in E".to_owned()),
            (Code::W0117, "'#y' is not a macro parameter in E".to_owned())
        ]
    );
    assert!(warned("#define E(x) #x\nE(a)\n").is_empty());
    // An object-like macro writes its body as stored, and so reads no `#`.
    assert!(warned("#define E #5\nE\n").is_empty());
}

// spec FTEF-14.63: the warning settings start as `QCC_SetDefaultProperties` makes them, change
// with `#pragma warning` for the rest of the module, and report an error setting as an error
// spec QCD-8.17: E0125 is a warning FTEQCC reports as an error
#[test]
fn warning_settings_hold_from_the_pragma_on() {
    let defaults = WarningSettings::default();
    for (warning, action) in [
        (FteWarning::NestedComment, WarningAction::Warn),
        (FteWarning::IdenticalPrecompiler, WarningAction::Ignore),
        (FteWarning::NotUtf8, WarningAction::Ignore),
        (FteWarning::FteSpecific, WarningAction::Ignore),
        (FteWarning::EvilPreprocessor, WarningAction::Error),
        (FteWarning::Denormal, WarningAction::Error),
        (FteWarning::Octal, WarningAction::Warn),
    ] {
        assert_eq!(defaults.action(warning), action, "{warning:?}");
    }
    assert_eq!(
        codes(&one_fte("/* /* */\n#pragma warning disable F325\n/* /* */\n").1),
        vec![Code::W0112]
    );
    let (_, out) = one_fte("#pragma warning error octal\nfloat y = 017;\n");
    assert_eq!(codes(&out), vec![Code::E0125]);
    assert_eq!(
        out.diagnostics[0].message,
        "A leading 0 is interpreted as base-8."
    );
    assert_eq!(
        warned("#pragma warning foo F325\n"),
        one(Code::W0113, "warning state not recognised")
    );
    assert_eq!(
        warned("#pragma warning enable Fxxx F325\n"),
        one(Code::W0113, "warning id not recognised")
    );
    // A name already read may turn Q120 off for the names after it.
    assert!(warned("#pragma warning disable Q120 Fxxx\n").is_empty());
    // `toggle` sets `!!`: an error becomes a warning, an ignored warning stays ignored.
    let (settings, _) = defaults.after_pragma(b"toggle F303 F316");
    assert_eq!(
        settings.action(FteWarning::EvilPreprocessor),
        WarningAction::Warn
    );
    assert_eq!(
        settings.action(FteWarning::IdenticalPrecompiler),
        WarningAction::Ignore
    );
    assert_eq!(
        defaults
            .after_pragma(b"ENABLE deprecated")
            .0
            .action(FteWarning::DeprecatedVariable),
        WarningAction::Warn
    );
    // The settings hold for the files after it, and the output says where they change.
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "x\n#pragma warning error octal\ny\n");
    loader.insert("b.qc", "float z = 017;\n");
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let a = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(tokens(&a), "x y");
    let marks: Vec<usize> = a.warnings.iter().map(|(at, _)| *at).collect();
    assert_eq!(marks, [0, 1]);
    assert_eq!(
        a.warnings[1].1.action(FteWarning::Octal),
        WarningAction::Error
    );
    let b: FileOutput = pp.preprocess_file(Path::new("b.qc")).unwrap();
    assert_eq!(codes(&b), vec![Code::E0125]);
    // An included file's warnings are as the settings stood where it was read.
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", "#include \"b.qh\"\n/* /* */\n");
    loader.insert("b.qh", "#pragma warning off F325\n");
    let mut pp = Preprocessor::new(loader).with_dialect(qc_lexer::Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(codes(&out), vec![]);
    // The vanilla dialect has no settings.
    let (_, out) = super::one("#pragma warning error octal\n");
    assert!(out.warnings.is_empty());
}

// spec FTEF-14.63: each warning code of this profile is reported as the setting of the FTEQCC
// warning it stands for
#[test]
fn each_warning_code_stands_for_its_fteqcc_warning() {
    for (code, warning) in [
        (Code::W0103, FteWarning::PrecompilerMessage),
        (Code::W0104, FteWarning::CaseInsensitiveFrameMacro),
        (Code::W0105, FteWarning::StaleMacro),
        (Code::W0106, FteWarning::DuplicateMacro),
        (Code::W0107, FteWarning::StringTooLong),
        (Code::W0108, FteWarning::BadPragma),
        (Code::W0113, FteWarning::BadPragma),
        (Code::W0109, FteWarning::EvilPreprocessor),
        (Code::W0110, FteWarning::Octal),
        (Code::W0111, FteWarning::Overflow),
        (Code::W0112, FteWarning::NestedComment),
        (Code::W0114, FteWarning::UndefNotDefined),
        (Code::W0115, FteWarning::DuplicatePrecompiler),
        (Code::W0116, FteWarning::IdenticalPrecompiler),
        (Code::W0118, FteWarning::GmqccSpecific),
        (Code::W0119, FteWarning::NotUtf8),
        (Code::W0120, FteWarning::FteSpecific),
        (Code::W0121, FteWarning::Denormal),
        (Code::W0122, FteWarning::MacroInString),
        (Code::W0123, FteWarning::HangingSlashR),
    ] {
        assert_eq!(FteWarning::of_code(code), Some(warning), "{code:?}");
    }
    assert_eq!(FteWarning::of_code(Code::W0101), None);
    assert_eq!(FteWarning::of_code(Code::E0125), None);
}
