// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_WarningForName, QCC_SetDefaultProperties
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_Precompiler, QCC_PR_PrintWarning, QCC_PR_ParseWarning

//! FTEQCC's warning settings (`docs/spec/dialect-fteqcc.md` FTEF-14.63): what each warning is,
//! `qccwarningaction`'s setting of it, which `#pragma warning` changes, and the diagnostic this
//! profile reports for it. Ported from FTEQCC's behaviour under ADR-0017's clean-port policy:
//! `warningnames[]` and `QCC_SetDefaultProperties`' defaults as data, the pragma's reading as
//! `QCC_PR_Precompiler` does it.

use qc_lexer::Code;

use crate::fte_directive::com_parse;

/// What FTEQCC does with a warning: `qccwarningaction`'s settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningAction {
    /// `WA_IGNORE`: not reported.
    Ignore,
    /// `WA_WARN`: a warning.
    Warn,
    /// `WA_ERROR`: an error, which FTEQCC prints as "werror".
    Error,
}

/// One of FTEQCC's warnings (`enum` in `qcc.h`): those `warningnames[]` names, which `#pragma
/// warning` sets, and those with no name this profile reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FteWarning {
    /// Q100, `WARN_PRECOMPILERMESSAGE`.
    PrecompilerMessage,
    /// Q101, `WARN_TOOMANYPARAMS`.
    TooManyParams,
    /// Q105, `WARN_TOOFEWPARAMS`.
    TooFewParams,
    /// Q106, `WARN_ASSIGNMENTTOCONSTANT`.
    AssignmentToConstant,
    /// Q120, `WARN_BADPRAGMA`.
    BadPragma,
    /// F129, `WARN_COMPATIBILITYHACK`.
    CompatibilityHack,
    /// Q203, `WARN_MISSINGRETURNVALUE`.
    MissingReturnValue,
    /// Q204, `WARN_WRONGRETURNTYPE`.
    WrongReturnType,
    /// Q205, `WARN_POINTLESSSTATEMENT`.
    PointlessStatement,
    /// Q206, `WARN_MISSINGRETURN`.
    MissingReturn,
    /// Q207, `WARN_DUPLICATEDEFINITION`.
    DuplicateDefinition,
    /// Q208, `WARN_SYSTEMCRC`.
    SystemCrc,
    /// Q302, `WARN_NOTREFERENCED`.
    NotReferenced,
    /// Q111, `WARN_DUPLICATELABEL`.
    DuplicateLabel,
    /// Q201, `WARN_ASSIGNMENTINCONDITIONAL`.
    AssignmentInConditional,
    /// F300, `WARN_DEADCODE`.
    DeadCode,
    /// F301, `WARN_NOTUTF8`.
    NotUtf8,
    /// F302, `WARN_UNINITIALIZED`.
    Uninitialized,
    /// F303, `WARN_EVILPREPROCESSOR`.
    EvilPreprocessor,
    /// F304, `WARN_UNARYNOTSCOPE`.
    UnaryNotScope,
    /// F305, `WARN_CASEINSENSITIVEFRAMEMACRO`.
    CaseInsensitiveFrameMacro,
    /// F306, `WARN_SAMENAMEASGLOBAL`.
    SameNameAsGlobal,
    /// F307, `WARN_STRICTTYPEMISMATCH`.
    StrictTypeMismatch,
    /// F308, `WARN_TYPEMISMATCHREDECOPTIONAL`.
    TypeMismatchRedecOptional,
    /// F309, `WARN_IGNORECOMMANDLINE`.
    IgnoreCommandLine,
    /// F310, `WARN_MISUSEDAUTOCVAR`.
    MisusedAutocvar,
    /// F311, `WARN_FTE_SPECIFIC`.
    FteSpecific,
    /// F312, `WARN_OVERFLOW`.
    Overflow,
    /// F313, `WARN_DENORMAL`.
    Denormal,
    /// F314, `WARN_LAXCAST`.
    LaxCast,
    /// F315, `WARN_DUPLICATEPRECOMPILER`.
    DuplicatePrecompiler,
    /// F316, `WARN_IDENTICALPRECOMPILER`.
    IdenticalPrecompiler,
    /// F317, `WARN_STALEMACRO`.
    StaleMacro,
    /// F318, `WARN_DUPLICATEMACRO`.
    DuplicateMacro,
    /// F319, `WARN_CONSTANTCOMPARISON`.
    ConstantComparison,
    /// F320, `WARN_PARAMWITHNONAME`.
    ParamWithNoName,
    /// F321, `WARN_GMQCC_SPECIFIC`.
    GmqccSpecific,
    /// F322, `WARN_IFSTRING_USED`.
    IfstringUsed,
    /// F323, `WARN_UNREACHABLECODE`.
    UnreachableCode,
    /// F324, `WARN_FORMATSTRING`.
    FormatString,
    /// F325, `WARN_NESTEDCOMMENT`.
    NestedComment,
    /// F326 and `deprecated`, `WARN_DEPRECATEDVARIABLE`.
    DeprecatedVariable,
    /// F327, `WARN_ENUMFLAGS_NOTINTEGER`.
    EnumflagsNotInteger,
    /// F328, `WARN_DEPRECACTEDSYNTAX`.
    DeprecatedSyntax,
    /// F329, `WARN_REDECLARATIONMISMATCH`.
    RedeclarationMismatch,
    /// F330, `WARN_MUTEDEPRECATEDVARIABLE`.
    MutedDeprecatedVariable,
    /// F331, `WARN_SELFNOTTHIS`.
    SelfNotThis,
    /// F332, `WARN_DIVISIONBY0`.
    DivisionBy0,
    /// F333, `WARN_ARGUMENTCHECK`.
    ArgumentCheck,
    /// F334, `WARN_MISSINGMEMBERQUALIFIER`.
    MissingMemberQualifier,
    /// F335, `WARN_MEMBERNOTDEFINED`.
    MemberNotDefined,
    /// F207, `WARN_NOTREFERENCEDFIELD`.
    NotReferencedField,
    /// F208, `WARN_NOTREFERENCEDCONST`.
    NotReferencedConst,
    /// F209, `WARN_EXTRAPRECACHE`.
    ExtraPrecache,
    /// F210, `WARN_NOTPRECACHED`.
    NotPrecached,
    /// F211, `WARN_SYSTEMCRC2`.
    SystemCrc2,
    /// `field-redeclared`, `WARN_REMOVEDWARNING`.
    RemovedWarning,
    /// `bounds`, `WARN_BOUNDS`.
    Bounds,
    /// `octal`, `WARN_OCTAL_IMMEDIATE`.
    Octal,
    /// `unimplemented`, `WARN_IGNOREDKEYWORD`.
    IgnoredKeyword,
    /// `localptr`, `WARN_UNSAFELOCALPOINTER`.
    UnsafeLocalPointer,
    /// `largereturn`, `WARN_SLOW_LARGERETURN`.
    SlowLargeReturn,
    /// `WARN_UNDEFNOTDEFINED`, which has no name.
    UndefNotDefined,
    /// `WARN_STRINGTOOLONG`, which has no name.
    StringTooLong,
    /// `WARN_BADTARGET`, which has no name.
    BadTarget,
    /// `WARN_DEBUGGING`, the warning of code 0, which has no name.
    Debugging,
    /// `WARN_MACROINSTRING`, which has no name.
    MacroInString,
    /// `WARN_HANGINGSLASHR`, which has no name.
    HangingSlashR,
    /// `WARN_UNDESIRABLECONVENTION`, which has no name.
    UndesirableConvention,
    /// `WARN_UNEXPECTEDPUNCT`, which has no name.
    UnexpectedPunct,
    /// `WARN_NOTSTANDARDBEHAVIOUR`, which has no name.
    NotStandardBehaviour,
    /// `WARN_NOTCONSTANT`, which has no name.
    NotConstant,
    /// `WARN_KEYWORDDISABLED`, which has no name.
    KeywordDisabled,
    /// `WARN_IFVECTOR_DISABLED`, which has no name.
    IfvectorDisabled,
    /// `WARN_CONDITIONALTYPEMISMATCH`, which has no name.
    ConditionalTypeMismatch,
    /// `WARN_TOOMANYPARAMETERSFORFUNC`, which has no name.
    TooManyParametersForFunc,
    /// `WARN_TOOMANYPARAMETERSVARARGS`, which has no name.
    TooManyParametersVarargs,
    /// `WARN_SWITCHTYPEMISMATCH`, which has no name.
    SwitchTypeMismatch,
}

/// `warningnames[]`, each name without its leading space, in FTEQCC's order: `QCC_WarningForName`
/// returns the first that matches, compared without regard to ASCII case (`stricmp(name,
/// warningnames[i].name+1)`).
const NAMES: [(&str, FteWarning); 63] = [
    ("Q100", FteWarning::PrecompilerMessage),
    ("Q101", FteWarning::TooManyParams),
    ("Q105", FteWarning::TooFewParams),
    ("Q106", FteWarning::AssignmentToConstant),
    ("Q120", FteWarning::BadPragma),
    ("F129", FteWarning::CompatibilityHack),
    ("Q203", FteWarning::MissingReturnValue),
    ("Q204", FteWarning::WrongReturnType),
    ("Q205", FteWarning::PointlessStatement),
    ("Q206", FteWarning::MissingReturn),
    ("Q207", FteWarning::DuplicateDefinition),
    ("Q208", FteWarning::SystemCrc),
    ("Q302", FteWarning::NotReferenced),
    ("Q111", FteWarning::DuplicateLabel),
    ("Q201", FteWarning::AssignmentInConditional),
    ("F300", FteWarning::DeadCode),
    ("F301", FteWarning::NotUtf8),
    ("F302", FteWarning::Uninitialized),
    ("F303", FteWarning::EvilPreprocessor),
    ("F304", FteWarning::UnaryNotScope),
    ("F305", FteWarning::CaseInsensitiveFrameMacro),
    ("F306", FteWarning::SameNameAsGlobal),
    ("F307", FteWarning::StrictTypeMismatch),
    ("F308", FteWarning::TypeMismatchRedecOptional),
    ("F309", FteWarning::IgnoreCommandLine),
    ("F310", FteWarning::MisusedAutocvar),
    ("F311", FteWarning::FteSpecific),
    ("F312", FteWarning::Overflow),
    ("F313", FteWarning::Denormal),
    ("F314", FteWarning::LaxCast),
    ("F315", FteWarning::DuplicatePrecompiler),
    ("F316", FteWarning::IdenticalPrecompiler),
    ("F317", FteWarning::StaleMacro),
    ("F318", FteWarning::DuplicateMacro),
    ("F319", FteWarning::ConstantComparison),
    ("F320", FteWarning::ParamWithNoName),
    ("F321", FteWarning::GmqccSpecific),
    ("F322", FteWarning::IfstringUsed),
    ("F323", FteWarning::UnreachableCode),
    ("F324", FteWarning::FormatString),
    ("F325", FteWarning::NestedComment),
    ("F326", FteWarning::DeprecatedVariable),
    ("F327", FteWarning::EnumflagsNotInteger),
    ("F328", FteWarning::DeprecatedSyntax),
    ("F329", FteWarning::RedeclarationMismatch),
    ("F330", FteWarning::MutedDeprecatedVariable),
    ("F331", FteWarning::SelfNotThis),
    ("F332", FteWarning::DivisionBy0),
    ("F333", FteWarning::ArgumentCheck),
    ("F334", FteWarning::MissingMemberQualifier),
    ("F335", FteWarning::MemberNotDefined),
    ("F207", FteWarning::NotReferencedField),
    ("F208", FteWarning::NotReferencedConst),
    ("F209", FteWarning::ExtraPrecache),
    ("F210", FteWarning::NotPrecached),
    ("F211", FteWarning::SystemCrc2),
    ("field-redeclared", FteWarning::RemovedWarning),
    ("deprecated", FteWarning::DeprecatedVariable),
    ("bounds", FteWarning::Bounds),
    ("octal", FteWarning::Octal),
    ("unimplemented", FteWarning::IgnoredKeyword),
    ("localptr", FteWarning::UnsafeLocalPointer),
    ("largereturn", FteWarning::SlowLargeReturn),
];

/// Every warning, for the settings' table.
const ALL: [FteWarning; 78] = [
    FteWarning::PrecompilerMessage,
    FteWarning::TooManyParams,
    FteWarning::TooFewParams,
    FteWarning::AssignmentToConstant,
    FteWarning::BadPragma,
    FteWarning::CompatibilityHack,
    FteWarning::MissingReturnValue,
    FteWarning::WrongReturnType,
    FteWarning::PointlessStatement,
    FteWarning::MissingReturn,
    FteWarning::DuplicateDefinition,
    FteWarning::SystemCrc,
    FteWarning::NotReferenced,
    FteWarning::DuplicateLabel,
    FteWarning::AssignmentInConditional,
    FteWarning::DeadCode,
    FteWarning::NotUtf8,
    FteWarning::Uninitialized,
    FteWarning::EvilPreprocessor,
    FteWarning::UnaryNotScope,
    FteWarning::CaseInsensitiveFrameMacro,
    FteWarning::SameNameAsGlobal,
    FteWarning::StrictTypeMismatch,
    FteWarning::TypeMismatchRedecOptional,
    FteWarning::IgnoreCommandLine,
    FteWarning::MisusedAutocvar,
    FteWarning::FteSpecific,
    FteWarning::Overflow,
    FteWarning::Denormal,
    FteWarning::LaxCast,
    FteWarning::DuplicatePrecompiler,
    FteWarning::IdenticalPrecompiler,
    FteWarning::StaleMacro,
    FteWarning::DuplicateMacro,
    FteWarning::ConstantComparison,
    FteWarning::ParamWithNoName,
    FteWarning::GmqccSpecific,
    FteWarning::IfstringUsed,
    FteWarning::UnreachableCode,
    FteWarning::FormatString,
    FteWarning::NestedComment,
    FteWarning::DeprecatedVariable,
    FteWarning::EnumflagsNotInteger,
    FteWarning::DeprecatedSyntax,
    FteWarning::RedeclarationMismatch,
    FteWarning::MutedDeprecatedVariable,
    FteWarning::SelfNotThis,
    FteWarning::DivisionBy0,
    FteWarning::ArgumentCheck,
    FteWarning::MissingMemberQualifier,
    FteWarning::MemberNotDefined,
    FteWarning::NotReferencedField,
    FteWarning::NotReferencedConst,
    FteWarning::ExtraPrecache,
    FteWarning::NotPrecached,
    FteWarning::SystemCrc2,
    FteWarning::RemovedWarning,
    FteWarning::Bounds,
    FteWarning::Octal,
    FteWarning::IgnoredKeyword,
    FteWarning::UnsafeLocalPointer,
    FteWarning::SlowLargeReturn,
    FteWarning::UndefNotDefined,
    FteWarning::StringTooLong,
    FteWarning::BadTarget,
    FteWarning::Debugging,
    FteWarning::MacroInString,
    FteWarning::HangingSlashR,
    FteWarning::UndesirableConvention,
    FteWarning::UnexpectedPunct,
    FteWarning::NotStandardBehaviour,
    FteWarning::NotConstant,
    FteWarning::KeywordDisabled,
    FteWarning::IfvectorDisabled,
    FteWarning::ConditionalTypeMismatch,
    FteWarning::TooManyParametersForFunc,
    FteWarning::TooManyParametersVarargs,
    FteWarning::SwitchTypeMismatch,
];

impl FteWarning {
    /// The warning `QCC_WarningForName` finds for `name`, if any.
    #[must_use]
    pub fn for_name(name: &[u8]) -> Option<Self> {
        NAMES
            .iter()
            .find(|(known, _)| known.as_bytes().eq_ignore_ascii_case(name))
            .map(|&(_, warning)| warning)
    }

    /// FTEQCC's setting of the warning before any `#pragma warning`: `QCC_SetDefaultProperties`
    /// sets every warning to `WA_WARN`, then these to `WA_IGNORE` and `WA_ERROR`
    /// (`qccwarningaction[WARN_FTE_SPECIFIC] = WA_IGNORE;` and the rest).
    #[must_use]
    pub const fn default_action(self) -> WarningAction {
        match self {
            Self::NotReferencedConst
            | Self::ExtraPrecache
            | Self::DeadCode
            | Self::FteSpecific
            | Self::DivisionBy0
            | Self::MutedDeprecatedVariable
            | Self::IfstringUsed
            | Self::NotUtf8
            | Self::Uninitialized
            | Self::SelfNotThis
            | Self::UnsafeLocalPointer
            | Self::IdenticalPrecompiler
            | Self::MacroInString => WarningAction::Ignore,
            Self::EvilPreprocessor | Self::Denormal => WarningAction::Error,
            Self::PrecompilerMessage
            | Self::TooManyParams
            | Self::TooFewParams
            | Self::AssignmentToConstant
            | Self::BadPragma
            | Self::CompatibilityHack
            | Self::MissingReturnValue
            | Self::WrongReturnType
            | Self::PointlessStatement
            | Self::MissingReturn
            | Self::DuplicateDefinition
            | Self::SystemCrc
            | Self::NotReferenced
            | Self::DuplicateLabel
            | Self::AssignmentInConditional
            | Self::UnaryNotScope
            | Self::CaseInsensitiveFrameMacro
            | Self::SameNameAsGlobal
            | Self::StrictTypeMismatch
            | Self::TypeMismatchRedecOptional
            | Self::IgnoreCommandLine
            | Self::MisusedAutocvar
            | Self::Overflow
            | Self::LaxCast
            | Self::DuplicatePrecompiler
            | Self::StaleMacro
            | Self::DuplicateMacro
            | Self::ConstantComparison
            | Self::ParamWithNoName
            | Self::GmqccSpecific
            | Self::UnreachableCode
            | Self::FormatString
            | Self::NestedComment
            | Self::DeprecatedVariable
            | Self::EnumflagsNotInteger
            | Self::DeprecatedSyntax
            | Self::RedeclarationMismatch
            | Self::ArgumentCheck
            | Self::IfvectorDisabled
            | Self::ConditionalTypeMismatch
            | Self::TooManyParametersForFunc
            | Self::TooManyParametersVarargs
            | Self::SwitchTypeMismatch
            | Self::MissingMemberQualifier
            | Self::MemberNotDefined
            | Self::NotReferencedField
            | Self::NotPrecached
            | Self::SystemCrc2
            | Self::RemovedWarning
            | Self::Bounds
            | Self::Octal
            | Self::IgnoredKeyword
            | Self::SlowLargeReturn
            | Self::UndefNotDefined
            | Self::StringTooLong
            | Self::BadTarget
            | Self::Debugging
            | Self::HangingSlashR
            | Self::UndesirableConvention
            | Self::UnexpectedPunct
            | Self::NotStandardBehaviour
            | Self::NotConstant
            | Self::KeywordDisabled => WarningAction::Warn,
        }
    }

    /// The FTEQCC warning a warning code of this profile reports, under an FTEQCC dialect: the
    /// lexer's and preprocessor's (FTEF-14.63), and the parser's and semantic model's, each the
    /// `WARN_` value `QCC_PR_ParseWarning` gives its text with (FTEF-19.22).
    #[must_use]
    pub const fn of_code(code: Code) -> Option<Self> {
        Some(match code {
            Code::W0103 => Self::PrecompilerMessage,
            Code::W0104 => Self::CaseInsensitiveFrameMacro,
            Code::W0105 => Self::StaleMacro,
            Code::W0106 => Self::DuplicateMacro,
            Code::W0107 | Code::W0117 => Self::StringTooLong,
            Code::W0108 | Code::W0113 => Self::BadPragma,
            Code::W0109 => Self::EvilPreprocessor,
            Code::W0110 => Self::Octal,
            Code::W0111 | Code::W0234 => Self::Overflow,
            Code::W0112 => Self::NestedComment,
            Code::W0114 => Self::UndefNotDefined,
            Code::W0115 => Self::DuplicatePrecompiler,
            Code::W0116 => Self::IdenticalPrecompiler,
            Code::W0118 => Self::GmqccSpecific,
            Code::W0119 => Self::NotUtf8,
            Code::W0120 => Self::FteSpecific,
            Code::W0121 | Code::W0237 => Self::Denormal,
            Code::W0122 => Self::MacroInString,
            Code::W0123 => Self::HangingSlashR,
            Code::W0203 => Self::UndesirableConvention,
            Code::W0204 | Code::W0313 => Self::PointlessStatement,
            Code::W0205 | Code::W0215 => Self::UnexpectedPunct,
            Code::W0206 | Code::W0210 | Code::W0211 | Code::W0223 => Self::CompatibilityHack,
            Code::W0207 => Self::DuplicateDefinition,
            Code::W0316 => Self::SelfNotThis,
            Code::W0208 | Code::W0315 => Self::SameNameAsGlobal,
            Code::W0209 => Self::TypeMismatchRedecOptional,
            Code::W0212 => Self::RedeclarationMismatch,
            Code::W0213 => Self::MisusedAutocvar,
            Code::W0214 | Code::W0226 => Self::IgnoredKeyword,
            Code::W0216 => Self::MemberNotDefined,
            Code::W0217 | Code::W0238 => Self::NotStandardBehaviour,
            Code::W0218 => Self::GmqccSpecific,
            Code::W0219 => Self::NotConstant,
            Code::W0220 | Code::W0231 | Code::W0243 | Code::W0248 => Self::LaxCast,
            Code::W0233 => Self::DivisionBy0,
            Code::W0239 => Self::ConstantComparison,
            Code::W0241 => Self::AssignmentToConstant,
            Code::W0242 => Self::AssignmentInConditional,
            Code::W0245 => Self::IfstringUsed,
            Code::W0249 => Self::Bounds,
            Code::W0251 => Self::TooManyParametersVarargs,
            Code::W0252 => Self::TooManyParametersForFunc,
            Code::W0253 => Self::TooFewParams,
            Code::W0254 => Self::FormatString,
            Code::W0255 => Self::ArgumentCheck,
            Code::W0250 => Self::UnsafeLocalPointer,
            Code::W0246 => Self::IfvectorDisabled,
            Code::W0247 => Self::ConditionalTypeMismatch,
            Code::W0256 => Self::MissingReturnValue,
            Code::W0257 => Self::WrongReturnType,
            Code::W0258 => Self::MissingReturn,
            Code::W0259 => Self::UnreachableCode,
            Code::W0260 => Self::SwitchTypeMismatch,
            Code::W0261 => Self::FteSpecific,
            Code::W0262 => Self::SlowLargeReturn,
            Code::W0265 => Self::MissingMemberQualifier,
            Code::W0240 => Self::StrictTypeMismatch,
            Code::W0221 => Self::KeywordDisabled,
            Code::W0222 => Self::ParamWithNoName,
            Code::W0224 => Self::DeprecatedSyntax,
            Code::W0225 => Self::EnumflagsNotInteger,
            Code::W0227 => Self::UnaryNotScope,
            Code::W0228 => Self::GmqccSpecific,
            Code::W0229 | Code::W0263 | Code::W0264 | Code::W0319 => Self::Debugging,
            Code::W0230 => Self::LaxCast,
            Code::W0312 => Self::DuplicateLabel,
            Code::W0314 => Self::DeprecatedVariable,
            _ => return None,
        })
    }

    /// The index of the warning in [`ALL`] and the settings' table.
    fn index(self) -> usize {
        ALL.iter().position(|&w| w == self).unwrap_or(0)
    }
}

/// `qccwarningaction`: FTEQCC's setting of each warning (FTEF-14.63).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarningSettings {
    actions: [WarningAction; ALL.len()],
}

impl Default for WarningSettings {
    /// `QCC_SetDefaultProperties`' settings.
    fn default() -> Self {
        Self {
            actions: ALL.map(FteWarning::default_action),
        }
    }
}

/// What a warning pragma's state word sets: `enable` and `on` warn, `disable`, `off` and
/// `ignore` ignore, `error` errs, and `toggle` sets `!!` of the setting.
#[derive(Clone, Copy)]
enum PragmaState {
    Set(WarningAction),
    Toggle,
}

impl WarningSettings {
    /// FTEQCC's setting of `warning`.
    #[must_use]
    pub fn action(&self, warning: FteWarning) -> WarningAction {
        self.actions
            .get(warning.index())
            .copied()
            .unwrap_or(WarningAction::Warn)
    }

    /// `#pragma warning STATE NAME ...` with the text `message` (FTEF-14.63), read with
    /// `QCC_COM_Parse`: the settings it leaves, and the texts of the Q120 warnings it gives in
    /// order, each with the settings at the time, as a name already read may have set Q120:
    /// "warning state not recognised", or "warning id not recognised" for each name
    /// `QCC_WarningForName` does not know. `toggle` sets `!!` of the setting, so a warning stays
    /// one and an error becomes one (`qccwarningaction[wn] = !!qccwarningaction[wn];`).
    #[must_use]
    pub fn after_pragma(mut self, message: &[u8]) -> (Self, Vec<(&'static str, Self)>) {
        let mut warnings = Vec::new();
        let state = com_parse(message, 0).and_then(|(state, at)| {
            let state = match state.to_ascii_lowercase().as_slice() {
                b"enable" | b"on" => PragmaState::Set(WarningAction::Warn),
                b"disable" | b"off" | b"ignore" => PragmaState::Set(WarningAction::Ignore),
                b"error" => PragmaState::Set(WarningAction::Error),
                b"toggle" => PragmaState::Toggle,
                _ => return None,
            };
            Some((state, at))
        });
        let Some((state, mut at)) = state else {
            warnings.push(("warning state not recognised", self));
            return (self, warnings);
        };
        while let Some((name, next)) = com_parse(message, at) {
            at = next;
            let Some(warning) = FteWarning::for_name(&name) else {
                warnings.push(("warning id not recognised", self));
                continue;
            };
            let set = match state {
                PragmaState::Set(action) => action,
                PragmaState::Toggle => match self.action(warning) {
                    WarningAction::Ignore => WarningAction::Ignore,
                    WarningAction::Warn | WarningAction::Error => WarningAction::Warn,
                },
            };
            self.set(warning, set);
        }
        (self, warnings)
    }

    /// Sets `warning` to `action`.
    pub(crate) fn set(&mut self, warning: FteWarning, action: WarningAction) {
        if let Some(slot) = self.actions.get_mut(warning.index()) {
            *slot = action;
        }
    }

    /// Sets every warning to what `to` makes of its setting.
    pub(crate) fn set_all(&mut self, to: impl Fn(WarningAction) -> WarningAction) {
        for slot in &mut self.actions {
            *slot = to(*slot);
        }
    }

    /// Makes each ignored warning that `chosen` takes a warning.
    pub(crate) fn turn_on_ignored(&mut self, chosen: impl Fn(FteWarning) -> bool) {
        for (warning, slot) in ALL.iter().zip(self.actions.iter_mut()) {
            if *slot == WarningAction::Ignore && chosen(*warning) {
                *slot = WarningAction::Warn;
            }
        }
    }

    /// The code a diagnostic of code `code` is reported with under these settings, or `None`
    /// when it is not reported (FTEF-14.63): a warning code of an FTEQCC warning is dropped when
    /// the warning is ignored and, when it is an error, is an error: `E0122` for F303
    /// (QCD-8.15), `E0125` for any other (QCD-8.17). Any other code is reported as it is.
    #[must_use]
    pub fn reported_code(&self, code: Code) -> Option<Code> {
        let Some(warning) = FteWarning::of_code(code) else {
            return Some(code);
        };
        match self.action(warning) {
            WarningAction::Ignore => None,
            WarningAction::Warn => Some(code),
            WarningAction::Error if warning == FteWarning::EvilPreprocessor => Some(Code::E0122),
            WarningAction::Error => Some(Code::E0125),
        }
    }
}
