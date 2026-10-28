// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialect profile a module is resolved under, and the rules that differ between `qcc`
//! and FTEQCC, each by name.
//!
//! The resolver never asks which compiler it follows; it asks the profile a question named for
//! the behaviour, so every place a rule applies can be found by its name, and the rule's
//! statements are cited once, here. Rules come in two kinds: where both compilers do something
//! and differ (`locals_shadow_globals`), and FTEQCC's language features and checks that `qcc`
//! has no counterpart of (`type_declarations`). Settings that change in the middle of a file
//! (`#pragma flag`, `#pragma warning`) are not here; the resolver reads them where it reads.

use qc_preproc::TargetFormat;

use crate::resolve::ResolveOptions;

/// The compiler whose behaviour a module follows (PRJ-2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compiler {
    /// id's `qcc` (`docs/spec/language.md`).
    Qcc,
    /// FTEQCC, in the rerelease subset or the full dialect (`dialect-fteqcc-rerelease.md`,
    /// `dialect-fteqcc.md`).
    Fteqcc,
}

/// The dialect and the module-wide options a module is resolved with.
#[derive(Clone, Copy, Debug)]
pub(in crate::resolve) struct Profile {
    compiler: Compiler,
    /// FTEQCC's `-Fautoproto` (FTE-22.3).
    autoproto: bool,
    /// FTEQCC's `-Fcaseinsens` (FTEF-19.33).
    case_insensitive: bool,
    /// Whether `qcfuncs` was off when FTEQCC named its core types (FTEF-19.28).
    qcfuncs_off: bool,
    /// The output format `QCC_PR_FinishCompilation` reads (FTEF-22.5).
    target: Option<TargetFormat>,
}

impl Profile {
    /// The profile of `options`. FTEQCC's options mean nothing under `qcc`, so they are off.
    pub(in crate::resolve) fn new(options: ResolveOptions) -> Self {
        let compiler = if options.dialect.has_fteqcc_extensions() {
            Compiler::Fteqcc
        } else {
            Compiler::Qcc
        };
        let fteqcc = compiler == Compiler::Fteqcc;
        Self {
            compiler,
            autoproto: fteqcc && options.autoproto,
            case_insensitive: fteqcc && options.case_insensitive,
            qcfuncs_off: fteqcc && options.qcfuncs_off,
            target: options.target.filter(|_| fteqcc),
        }
    }

    const fn fteqcc(self) -> bool {
        matches!(self.compiler, Compiler::Fteqcc)
    }

    /// FTEQCC reads the module twice with autoprototyping, declaring in the first pass and
    /// compiling in the second (FTE-22.3, FTEF-12.18).
    pub(in crate::resolve) const fn two_passes(self) -> bool {
        self.autoproto
    }

    /// Whether `#pragma autoproto` and the first pass's plan apply (FTEF-12.18).
    pub(in crate::resolve) const fn reads_autoproto_pragmas(self) -> bool {
        self.fteqcc()
    }

    /// A name no definition has is looked up again in any case (FTEF-19.33).
    pub(in crate::resolve) const fn case_insensitive(self) -> bool {
        self.case_insensitive
    }

    /// `string`, `vector` and `entity` are no type names (FTEF-19.28).
    pub(in crate::resolve) const fn qcfuncs_off(self) -> bool {
        self.qcfuncs_off
    }

    /// The output format `extern` functions are checked against (FTEF-22.5).
    pub(in crate::resolve) const fn target(self) -> Option<TargetFormat> {
        self.target
    }

    // Rules both compilers have, which differ.

    /// A local or parameter shadows a global of its name (FTE-22.1); under `qcc` it reuses
    /// a global declared before it (QC-12.3).
    pub(in crate::resolve) const fn locals_shadow_globals(self) -> bool {
        self.fteqcc()
    }

    /// A redeclaration's type is compared by FTEQCC's `typecmp` and its exceptions
    /// (FTEF-17.12, FTEF-17.21) and its parameters' defaults must agree (FTEF-19.6); under
    /// `qcc` the types must be equal (QC-11.5).
    pub(in crate::resolve) const fn redeclaration_by_typecmp(self) -> bool {
        self.fteqcc()
    }

    /// A vector field's components are of `qcc`'s `type_floatfield`, which no `.float`
    /// written in the source is (QC-11.12); FTEQCC gives them `.float`.
    pub(in crate::resolve) const fn field_components_typed_apart(self) -> bool {
        !self.fteqcc()
    }

    /// A vector's components are initialised with it, and made with no lookup of their names
    /// (FTEF-19.58); under `qcc` they are not initialised (QC-11.11).
    pub(in crate::resolve) const fn components_initialised(self) -> bool {
        self.fteqcc()
    }

    /// A second initialiser of the same value is accepted, and a `noref` definition's is
    /// replaced (FTE-22.6, FTEF-17.8); under `qcc` any second initialiser is "redeclared"
    /// (QC-11.5, QC-11.13).
    pub(in crate::resolve) const fn reinitialise_with_same_value(self) -> bool {
        self.fteqcc()
    }

    /// An initialiser is cast to its definition's type as `QCC_TryEvaluateCast` casts it
    /// (FTEF-19.2); under `qcc` a literal must have the definition's type (QC-11.3).
    pub(in crate::resolve) const fn initialisers_cast(self) -> bool {
        self.fteqcc()
    }

    /// Parameters must lie in the global area in order, "bad parm order" (QC-13.5); FTEQCC
    /// does not check.
    pub(in crate::resolve) const fn checks_parameter_order(self) -> bool {
        !self.fteqcc()
    }

    /// Diagnostics carry FTEQCC's texts and follow its warning settings (FTEF-19.22).
    pub(in crate::resolve) const fn fteqcc_diagnostics(self) -> bool {
        self.fteqcc()
    }

    // FTEQCC's features and checks, which `qcc` has no counterpart of.

    /// Structs, unions, enums, typedefs, classes and accessors, and the checks of the type
    /// names a declaration writes (FTE-14, FTE-15, FTEF-6 to FTEF-10, FTEF-19.20).
    pub(in crate::resolve) const fn type_declarations(self) -> bool {
        self.fteqcc()
    }

    /// The qualifiers and attributes of a definition: `static`, `shared`, `const`, `var`,
    /// `extern`, `alias`, `noref`, `deprecated` and `autocvar_` names (FTEF-15, FTEF-19.11 to
    /// FTEF-19.13, FTEF-22.5).
    pub(in crate::resolve) const fn qualifiers(self) -> bool {
        self.fteqcc()
    }

    /// The checks `QCC_PR_ParseDefs` makes of a definition beyond its type: default values,
    /// the constness of an initialiser, array sizes and initialisers, and builtins given for a
    /// function with a body (FTEF-17.9, FTEF-19.3, FTEF-19.6, FTEF-19.8).
    pub(in crate::resolve) const fn definition_checks(self) -> bool {
        self.fteqcc()
    }

    /// `Scope::name`, `this`, `super` and FTEQCC's intrinsics in expressions (FTEF-16.4,
    /// FTEF-20.11), and the checks of a name's use: a type's name is no value, a call of a
    /// value, a `deprecated` definition (FTEF-19.38, FTEF-19.40, FTEF-19.13).
    pub(in crate::resolve) const fn name_use_checks(self) -> bool {
        self.fteqcc()
    }

    /// The checks of a function body FTEQCC makes as it reads it: unnamed parameters, the
    /// literals of a state header, `subscope` and C functions declared in a body (FTEF-19.1,
    /// FTEF-19.14, FTEF-21.10, FTEF-21.12).
    pub(in crate::resolve) const fn body_checks(self) -> bool {
        self.fteqcc()
    }

    /// A definition named as a keyword that is on is warned of (FTEF-20.4).
    pub(in crate::resolve) const fn warns_keyword_names(self) -> bool {
        self.fteqcc()
    }
}
